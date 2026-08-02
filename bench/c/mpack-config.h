/* Locked bench config: everything feature surface + forced tracking + libc malloc.
 * Must stay in sync with bench/methodology.md and Rust full-suite-abi defaults
 * (buffer 4096, tracking capacity 3).
 */
#ifndef MPACK_BENCH_CONFIG_H
#define MPACK_BENCH_CONFIG_H 1

#define MPACK_HAS_CONFIG 1

#define MPACK_READER 1
#define MPACK_WRITER 1
#define MPACK_EXPECT 1
#define MPACK_NODE 1
#define MPACK_COMPATIBILITY 1
#define MPACK_EXTENSIONS 1
#define MPACK_STDLIB 1
#define MPACK_STDIO 1

/* Force tracking even under NDEBUG (upstream everything-release would skip). */
#define MPACK_READ_TRACKING 1
#define MPACK_WRITE_TRACKING 1

/* Match Rust full-suite-abi TRACKING_INITIAL_CAPACITY / default buffer. */
#define MPACK_TRACKING_INITIAL_CAPACITY 3
#define MPACK_BUFFER_SIZE 4096

/* Libc allocators (not suite fail-injection). Rust FFI still calls symbols
 * named test_malloc/test_free; bench/c/bench_shims.c provides identity wraps. */
#define MPACK_MALLOC malloc
#define MPACK_FREE free

#endif /* MPACK_BENCH_CONFIG_H */
