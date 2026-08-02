/*
 * Shared C ABI benchmark driver.
 * Linked once against upstream MPack C sources and once against the Rust
 * full-suite-abi staticlib. Public MPack C API only.
 */
#ifndef _DEFAULT_SOURCE
#define _DEFAULT_SOURCE 1
#endif
#ifndef _POSIX_C_SOURCE
#define _POSIX_C_SOURCE 200809L
#endif
#define MPACK_HAS_CONFIG 1
#include "mpack-config.h"
#include "mpack/mpack.h"

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#if defined(__linux__)
#include <sys/resource.h>
#endif

enum {
    WARMUP_DEFAULT = 100,
    THROUGHPUT_ITERS_DEFAULT = 5000,
    LATENCY_ITERS_DEFAULT = 10000,
    TRIALS_DEFAULT = 1,
    LARGE_TARGET_BYTES = 16 * 1024 * 1024,
};

typedef struct {
    int warmup;
    int iters;
    int json;
    const char* fixture_path;
} run_opts;

static uint64_t nsec_now(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
        abort();
    }
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

static int cmp_u64(const void* a, const void* b) {
    uint64_t x = *(const uint64_t*)a;
    uint64_t y = *(const uint64_t*)b;
    return (x > y) - (x < y);
}

static uint64_t percentile_u64(uint64_t* samples, size_t n, double pct) {
    if (n == 0) {
        return 0;
    }
    qsort(samples, n, sizeof(uint64_t), cmp_u64);
    size_t idx = (size_t)((pct / 100.0) * (double)(n - 1));
    if (idx >= n) {
        idx = n - 1;
    }
    return samples[idx];
}

/* Fixed nested document used for encode / decode throughput + latency. */
static void write_fixture_doc(mpack_writer_t* writer) {
    static const char bin_payload[256] = {0};
    char name[32];
    int i;

    mpack_start_map(writer, 4);

    mpack_write_cstr(writer, "ints");
    mpack_start_array(writer, 64);
    for (i = 0; i < 64; ++i) {
        mpack_write_int(writer, i - 32);
    }
    mpack_finish_array(writer);

    mpack_write_cstr(writer, "strs");
    mpack_start_array(writer, 32);
    for (i = 0; i < 32; ++i) {
        snprintf(name, sizeof(name), "s%02d", i);
        mpack_write_cstr(writer, name);
    }
    mpack_finish_array(writer);

    mpack_write_cstr(writer, "bin");
    mpack_write_bin(writer, bin_payload, sizeof(bin_payload));

    mpack_write_cstr(writer, "nested");
    mpack_start_map(writer, 3);
    mpack_write_cstr(writer, "a");
    mpack_write_bool(writer, true);
    mpack_write_cstr(writer, "b");
    mpack_write_nil(writer);
    mpack_write_cstr(writer, "c");
    mpack_start_array(writer, 4);
    mpack_write_float(writer, 1.25f);
    mpack_write_double(writer, 2.5);
    mpack_write_uint(writer, 0xffffffffu);
    mpack_write_i64(writer, (int64_t)-1);
    mpack_finish_array(writer);
    mpack_finish_map(writer);

    mpack_finish_map(writer);
}

static char* encode_once(size_t* out_size) {
    char* data = NULL;
    size_t size = 0;
    mpack_writer_t writer;
    mpack_writer_init_growable(&writer, &data, &size);
    write_fixture_doc(&writer);
    if (mpack_writer_destroy(&writer) != mpack_ok) {
        free(data);
        return NULL;
    }
    *out_size = size;
    return data;
}

static int decode_reader_once(const char* data, size_t size) {
    mpack_reader_t reader;
    mpack_reader_init_data(&reader, data, size);
    mpack_discard(&reader);
    return mpack_reader_destroy(&reader) == mpack_ok ? 0 : 1;
}

static void visit_node(mpack_node_t node) {
    mpack_type_t type = mpack_node_type(node);
    size_t i;
    size_t count;

    /* Touch type and compound structure only — avoid typed accessors that can
     * sticky-error on encode-width differences (int vs uint, etc.). */
    switch (type) {
        case mpack_type_array:
            count = mpack_node_array_length(node);
            for (i = 0; i < count; ++i) {
                visit_node(mpack_node_array_at(node, i));
            }
            break;
        case mpack_type_map:
            count = mpack_node_map_count(node);
            for (i = 0; i < count; ++i) {
                visit_node(mpack_node_map_key_at(node, i));
                visit_node(mpack_node_map_value_at(node, i));
            }
            break;
        case mpack_type_str:
            count = mpack_node_strlen(node);
            if (count > 0) {
                (void)mpack_node_str(node)[0];
            }
            break;
        case mpack_type_bin:
            count = mpack_node_bin_size(node);
            if (count > 0) {
                (void)mpack_node_bin_data(node)[0];
            }
            break;
        default:
            break;
    }
}

static int decode_node_once(const char* data, size_t size) {
    mpack_tree_t tree;
    mpack_tree_init_data(&tree, data, size);
    mpack_tree_parse(&tree);
    if (mpack_tree_error(&tree) != mpack_ok) {
        mpack_tree_destroy(&tree);
        return 1;
    }
    visit_node(mpack_tree_root(&tree));
    if (mpack_tree_error(&tree) != mpack_ok) {
        mpack_tree_destroy(&tree);
        return 1;
    }
    return mpack_tree_destroy(&tree) == mpack_ok ? 0 : 1;
}

static char* encode_large(size_t target_bytes, size_t* out_size) {
    char* small = NULL;
    size_t small_size = 0;
    char* data = NULL;
    size_t size = 0;
    mpack_writer_t writer;
    size_t count;
    size_t i;

    small = encode_once(&small_size);
    if (small == NULL || small_size == 0) {
        free(small);
        return NULL;
    }
    count = target_bytes / small_size;
    if (count < 1) {
        count = 1;
    }

    mpack_writer_init_growable(&writer, &data, &size);
    mpack_start_array(&writer, (uint32_t)count);
    for (i = 0; i < count; ++i) {
        mpack_write_object_bytes(&writer, small, small_size);
    }
    mpack_finish_array(&writer);
    free(small);
    if (mpack_writer_destroy(&writer) != mpack_ok) {
        free(data);
        return NULL;
    }
    *out_size = size;
    return data;
}

#if defined(__linux__)
static long peak_rss_bytes(void) {
    struct rusage usage;
    if (getrusage(RUSAGE_SELF, &usage) != 0) {
        return -1;
    }
    /* Linux: ru_maxrss is kilobytes. */
    return usage.ru_maxrss * 1024L;
}
#else
static long peak_rss_bytes(void) {
    return -1;
}
#endif

static void print_throughput_json(
    const char* metric,
    double docs_per_s,
    double mb_per_s,
    size_t bytes_per_doc,
    int iters
) {
    printf(
        "{\"metric\":\"%s\",\"docs_per_s\":%.6f,\"mb_per_s\":%.6f,"
        "\"bytes_per_doc\":%zu,\"iters\":%d}\n",
        metric,
        docs_per_s,
        mb_per_s,
        bytes_per_doc,
        iters
    );
}

static void print_latency_json(
    const char* metric,
    uint64_t p50,
    uint64_t p99,
    uint64_t max_ns,
    int iters
) {
    printf(
        "{\"metric\":\"%s\",\"p50_ns\":%" PRIu64 ",\"p99_ns\":%" PRIu64
        ",\"max_ns\":%" PRIu64 ",\"iters\":%d}\n",
        metric,
        p50,
        p99,
        max_ns,
        iters
    );
}

static int run_encode_throughput(const run_opts* opts) {
    size_t doc_size = 0;
    char* sample = encode_once(&doc_size);
    int i;
    uint64_t t0;
    uint64_t t1;
    double secs;
    double docs_per_s;
    double mb_per_s;

    if (sample == NULL) {
        fprintf(stderr, "encode fixture failed\n");
        return 1;
    }
    free(sample);

    for (i = 0; i < opts->warmup; ++i) {
        char* data = encode_once(&doc_size);
        if (data == NULL) {
            return 1;
        }
        free(data);
    }

    t0 = nsec_now();
    for (i = 0; i < opts->iters; ++i) {
        char* data = encode_once(&doc_size);
        if (data == NULL) {
            return 1;
        }
        free(data);
    }
    t1 = nsec_now();
    secs = (double)(t1 - t0) / 1e9;
    docs_per_s = (double)opts->iters / secs;
    mb_per_s = (docs_per_s * (double)doc_size) / (1024.0 * 1024.0);
    if (opts->json) {
        print_throughput_json("encode_throughput", docs_per_s, mb_per_s, doc_size, opts->iters);
    } else {
        printf("encode_throughput docs/s=%.2f MB/s=%.2f bytes=%zu iters=%d\n",
            docs_per_s, mb_per_s, doc_size, opts->iters);
    }
    return 0;
}

static int run_decode_throughput(const run_opts* opts, int use_node) {
    size_t doc_size = 0;
    char* data = encode_once(&doc_size);
    int i;
    uint64_t t0;
    uint64_t t1;
    double secs;
    double docs_per_s;
    double mb_per_s;
    const char* metric = use_node ? "decode_node_throughput" : "decode_reader_throughput";

    if (data == NULL) {
        return 1;
    }
    for (i = 0; i < opts->warmup; ++i) {
        if (use_node) {
            if (decode_node_once(data, doc_size) != 0) {
                free(data);
                return 1;
            }
        } else if (decode_reader_once(data, doc_size) != 0) {
            free(data);
            return 1;
        }
    }
    t0 = nsec_now();
    for (i = 0; i < opts->iters; ++i) {
        if (use_node) {
            if (decode_node_once(data, doc_size) != 0) {
                free(data);
                return 1;
            }
        } else if (decode_reader_once(data, doc_size) != 0) {
            free(data);
            return 1;
        }
    }
    t1 = nsec_now();
    free(data);
    secs = (double)(t1 - t0) / 1e9;
    docs_per_s = (double)opts->iters / secs;
    mb_per_s = (docs_per_s * (double)doc_size) / (1024.0 * 1024.0);
    if (opts->json) {
        print_throughput_json(metric, docs_per_s, mb_per_s, doc_size, opts->iters);
    } else {
        printf("%s docs/s=%.2f MB/s=%.2f bytes=%zu iters=%d\n",
            metric, docs_per_s, mb_per_s, doc_size, opts->iters);
    }
    return 0;
}

static int run_encode_latency(const run_opts* opts) {
    uint64_t* samples;
    size_t doc_size = 0;
    int i;
    uint64_t p50;
    uint64_t p99;
    uint64_t max_ns = 0;

    samples = (uint64_t*)calloc((size_t)opts->iters, sizeof(uint64_t));
    if (samples == NULL) {
        return 1;
    }
    for (i = 0; i < opts->iters; ++i) {
        uint64_t t0 = nsec_now();
        char* data = encode_once(&doc_size);
        uint64_t t1 = nsec_now();
        if (data == NULL) {
            free(samples);
            return 1;
        }
        free(data);
        samples[i] = t1 - t0;
        if (samples[i] > max_ns) {
            max_ns = samples[i];
        }
    }
    p50 = percentile_u64(samples, (size_t)opts->iters, 50.0);
    p99 = percentile_u64(samples, (size_t)opts->iters, 99.0);
    if (opts->json) {
        print_latency_json("encode_latency", p50, p99, max_ns, opts->iters);
    } else {
        printf("encode_latency p50=%" PRIu64 " p99=%" PRIu64 " max=%" PRIu64 " ns\n",
            p50, p99, max_ns);
    }
    free(samples);
    return 0;
}

static int run_decode_latency(const run_opts* opts, int use_node) {
    uint64_t* samples;
    size_t doc_size = 0;
    char* data;
    int i;
    uint64_t p50;
    uint64_t p99;
    uint64_t max_ns = 0;
    const char* metric = use_node ? "decode_node_latency" : "decode_reader_latency";

    data = encode_once(&doc_size);
    if (data == NULL) {
        return 1;
    }
    samples = (uint64_t*)calloc((size_t)opts->iters, sizeof(uint64_t));
    if (samples == NULL) {
        free(data);
        return 1;
    }
    for (i = 0; i < opts->iters; ++i) {
        uint64_t t0 = nsec_now();
        int rc = use_node ? decode_node_once(data, doc_size) : decode_reader_once(data, doc_size);
        uint64_t t1 = nsec_now();
        if (rc != 0) {
            free(samples);
            free(data);
            return 1;
        }
        samples[i] = t1 - t0;
        if (samples[i] > max_ns) {
            max_ns = samples[i];
        }
    }
    p50 = percentile_u64(samples, (size_t)opts->iters, 50.0);
    p99 = percentile_u64(samples, (size_t)opts->iters, 99.0);
    if (opts->json) {
        print_latency_json(metric, p50, p99, max_ns, opts->iters);
    } else {
        printf("%s p50=%" PRIu64 " p99=%" PRIu64 " max=%" PRIu64 " ns\n",
            metric, p50, p99, max_ns);
    }
    free(samples);
    free(data);
    return 0;
}

static char* read_entire_file(const char* path, size_t* out_size) {
    FILE* file;
    long file_size;
    char* data;
    size_t nread;

    file = fopen(path, "rb");
    if (file == NULL) {
        fprintf(stderr, "failed to open fixture %s\n", path);
        return NULL;
    }
    if (fseek(file, 0, SEEK_END) != 0) {
        fclose(file);
        return NULL;
    }
    file_size = ftell(file);
    if (file_size < 0) {
        fclose(file);
        return NULL;
    }
    if (fseek(file, 0, SEEK_SET) != 0) {
        fclose(file);
        return NULL;
    }
    data = (char*)malloc((size_t)file_size);
    if (data == NULL) {
        fclose(file);
        return NULL;
    }
    nread = fread(data, 1, (size_t)file_size, file);
    fclose(file);
    if (nread != (size_t)file_size) {
        free(data);
        return NULL;
    }
    *out_size = (size_t)file_size;
    return data;
}

/* Decode-only RSS: load a pre-built fixture and parse it. Encode must not run
 * in this process so ru_maxrss reflects decode (+ input buffer), not encode. */
static int run_rss(const run_opts* opts) {
    size_t size = 0;
    char* data;
    long rss;

    if (opts->fixture_path == NULL) {
        fprintf(stderr, "rss requires --fixture PATH (decode-only process)\n");
        return 2;
    }
    data = read_entire_file(opts->fixture_path, &size);
    if (data == NULL) {
        return 1;
    }
    if (decode_node_once(data, size) != 0) {
        free(data);
        return 1;
    }
    rss = peak_rss_bytes();
    free(data);
    if (opts->json) {
        printf(
            "{\"metric\":\"rss\",\"peak_bytes\":%ld,\"fixture_bytes\":%zu,"
            "\"mode\":\"decode_only\"}\n",
            rss,
            size
        );
    } else {
        printf("rss peak_bytes=%ld fixture_bytes=%zu mode=decode_only\n", rss, size);
    }
    return rss < 0 ? 1 : 0;
}

static int run_dump_large_fixture(void) {
    size_t size = 0;
    char* data = encode_large(LARGE_TARGET_BYTES, &size);
    if (data == NULL) {
        return 1;
    }
    if (fwrite(data, 1, size, stdout) != size) {
        free(data);
        return 1;
    }
    free(data);
    return 0;
}

static int run_startup(const run_opts* opts) {
    mpack_writer_t writer;
    char buffer[16];
    (void)opts;
    mpack_writer_init(&writer, buffer, sizeof(buffer));
    mpack_write_nil(&writer);
    if (mpack_writer_destroy(&writer) != mpack_ok) {
        return 1;
    }
    if (opts->json) {
        printf("{\"metric\":\"startup\",\"ok\":true}\n");
    }
    return 0;
}

static int run_dump_fixture(void) {
    size_t size = 0;
    char* data = encode_once(&size);
    if (data == NULL) {
        return 1;
    }
    if (fwrite(data, 1, size, stdout) != size) {
        free(data);
        return 1;
    }
    free(data);
    return 0;
}

static void usage(const char* argv0) {
    fprintf(
        stderr,
        "usage: %s <workload> [--json] [--warmup N] [--iters N] [--fixture PATH]\n"
        "workloads: encode | decode-reader | decode-node |\n"
        "           encode-latency | decode-reader-latency | decode-node-latency |\n"
        "           rss | startup | dump-fixture | dump-large-fixture\n",
        argv0
    );
}

int main(int argc, char** argv) {
    run_opts opts;
    const char* workload;
    int i;

    opts.warmup = WARMUP_DEFAULT;
    opts.iters = 0;
    opts.json = 0;
    opts.fixture_path = NULL;

    if (argc < 2) {
        usage(argv[0]);
        return 2;
    }
    workload = argv[1];
    for (i = 2; i < argc; ++i) {
        if (strcmp(argv[i], "--json") == 0) {
            opts.json = 1;
        } else if (strcmp(argv[i], "--warmup") == 0 && i + 1 < argc) {
            opts.warmup = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--iters") == 0 && i + 1 < argc) {
            opts.iters = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--fixture") == 0 && i + 1 < argc) {
            opts.fixture_path = argv[++i];
        } else {
            usage(argv[0]);
            return 2;
        }
    }

    if (strcmp(workload, "dump-fixture") == 0) {
        return run_dump_fixture();
    }
    if (strcmp(workload, "dump-large-fixture") == 0) {
        return run_dump_large_fixture();
    }
    if (strcmp(workload, "startup") == 0) {
        return run_startup(&opts);
    }
    if (strcmp(workload, "rss") == 0) {
        return run_rss(&opts);
    }
    if (strcmp(workload, "encode") == 0) {
        if (opts.iters <= 0) {
            opts.iters = THROUGHPUT_ITERS_DEFAULT;
        }
        return run_encode_throughput(&opts);
    }
    if (strcmp(workload, "decode-reader") == 0) {
        if (opts.iters <= 0) {
            opts.iters = THROUGHPUT_ITERS_DEFAULT;
        }
        return run_decode_throughput(&opts, 0);
    }
    if (strcmp(workload, "decode-node") == 0) {
        if (opts.iters <= 0) {
            opts.iters = THROUGHPUT_ITERS_DEFAULT;
        }
        return run_decode_throughput(&opts, 1);
    }
    if (strcmp(workload, "encode-latency") == 0) {
        if (opts.iters <= 0) {
            opts.iters = LATENCY_ITERS_DEFAULT;
        }
        return run_encode_latency(&opts);
    }
    if (strcmp(workload, "decode-reader-latency") == 0) {
        if (opts.iters <= 0) {
            opts.iters = LATENCY_ITERS_DEFAULT;
        }
        return run_decode_latency(&opts, 0);
    }
    if (strcmp(workload, "decode-node-latency") == 0) {
        if (opts.iters <= 0) {
            opts.iters = LATENCY_ITERS_DEFAULT;
        }
        return run_decode_latency(&opts, 1);
    }

    usage(argv[0]);
    return 2;
}
