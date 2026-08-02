/*
 * C MPack reader oracle: iterative tag walk of one top-level value.
 * Raw payload bytes only (no UTF-8 checks). Depth capped at ORACLE_DEPTH_LIMIT.
 */

#include "oracle_digest.h"

#include "mpack/mpack.h"

#include <string.h>

typedef struct {
    uint32_t remaining;
} oracle_frame_t;

static void digest_clear(oracle_digest_t* out) {
    memset(out, 0, sizeof(*out));
}

static void write_u64_le(uint8_t* dst, uint64_t value) {
    for (int i = 0; i < 8; ++i) {
        dst[i] = (uint8_t)(value >> (8 * i));
    }
}

static void write_u32_le(uint8_t* dst, uint32_t value) {
    for (int i = 0; i < 4; ++i) {
        dst[i] = (uint8_t)(value >> (8 * i));
    }
}

static int digest_push(
        oracle_digest_t* out,
        mpack_type_t type,
        uint8_t aux,
        uint64_t value,
        uint32_t payload_hash) {
    if (out->record_count >= ORACLE_MAX_RECORDS) {
        out->truncated = 1;
        return 0;
    }
    size_t off = (size_t)out->record_count * ORACLE_RECORD_SIZE;
    uint8_t* rec = &out->records[off];
    rec[0] = (uint8_t)type;
    rec[1] = aux;
    rec[2] = 0;
    rec[3] = 0;
    write_u64_le(rec + 4, value);
    write_u32_le(rec + 12, payload_hash);
    out->record_count += 1;
    return 1;
}

static void read_payload_hash(mpack_reader_t* reader, uint32_t count, uint32_t* hash_out) {
    uint32_t hash = 2166136261u;
    while (count > 0) {
        char buffer[256];
        uint32_t step = count < (uint32_t)sizeof(buffer) ? count : (uint32_t)sizeof(buffer);
        mpack_read_bytes(reader, buffer, step);
        if (mpack_reader_error(reader) != mpack_ok) {
            *hash_out = 0;
            return;
        }
        for (uint32_t i = 0; i < step; ++i) {
            hash ^= (uint8_t)buffer[i];
            hash *= 16777619u;
        }
        count -= step;
    }
    *hash_out = hash;
}

static void walk_reader(mpack_reader_t* reader, oracle_digest_t* out) {
    oracle_frame_t stack[ORACLE_DEPTH_LIMIT];
    mpack_type_t kinds[ORACLE_DEPTH_LIMIT];
    int depth = 0;
    int need = 1;

    while (need > 0 || depth > 0) {
        if (mpack_reader_error(reader) != mpack_ok || out->truncated) {
            return;
        }
        if (depth >= ORACLE_DEPTH_LIMIT) {
            mpack_reader_flag_error(reader, mpack_error_too_big);
            return;
        }

        mpack_tag_t tag = mpack_read_tag(reader);
        if (mpack_reader_error(reader) != mpack_ok) {
            return;
        }

        uint8_t aux = 0;
        uint64_t value = 0;
        uint32_t payload_hash = 0;
        mpack_type_t type = mpack_tag_type(&tag);

        switch (type) {
            case mpack_type_nil:
                break;
            case mpack_type_bool:
                aux = mpack_tag_bool_value(&tag) ? 1 : 0;
                break;
            case mpack_type_int:
                value = (uint64_t)mpack_tag_int_value(&tag);
                break;
            case mpack_type_uint:
                value = mpack_tag_uint_value(&tag);
                break;
            case mpack_type_float: {
                float f = mpack_tag_float_value(&tag);
                uint32_t bits = 0;
                memcpy(&bits, &f, 4);
                value = bits;
                break;
            }
            case mpack_type_double: {
                double d = mpack_tag_double_value(&tag);
                memcpy(&value, &d, 8);
                break;
            }
            case mpack_type_str:
            case mpack_type_bin:
                value = mpack_tag_bytes(&tag);
                read_payload_hash(reader, (uint32_t)value, &payload_hash);
                if (mpack_reader_error(reader) != mpack_ok) {
                    return;
                }
                mpack_done_type(reader, type);
                break;
            case mpack_type_ext:
                aux = (uint8_t)mpack_tag_ext_exttype(&tag);
                value = mpack_tag_bytes(&tag);
                read_payload_hash(reader, (uint32_t)value, &payload_hash);
                if (mpack_reader_error(reader) != mpack_ok) {
                    return;
                }
                mpack_done_type(reader, type);
                break;
            case mpack_type_array:
                value = mpack_tag_array_count(&tag);
                break;
            case mpack_type_map:
                value = mpack_tag_map_count(&tag);
                break;
            default:
                break;
        }

        if (!digest_push(out, type, aux, value, payload_hash)) {
            return;
        }

        if (need > 0) {
            need -= 1;
        } else {
            stack[depth - 1].remaining -= 1;
        }

        if (type == mpack_type_array) {
            stack[depth].remaining = (uint32_t)value;
            kinds[depth] = mpack_type_array;
            depth += 1;
        } else if (type == mpack_type_map) {
            if (value > (UINT32_MAX / 2)) {
                mpack_reader_flag_error(reader, mpack_error_too_big);
                return;
            }
            stack[depth].remaining = (uint32_t)(value * 2u);
            kinds[depth] = mpack_type_map;
            depth += 1;
        }

        while (depth > 0 && stack[depth - 1].remaining == 0) {
            if (kinds[depth - 1] == mpack_type_array) {
                mpack_done_array(reader);
            } else {
                mpack_done_map(reader);
            }
            depth -= 1;
            if (mpack_reader_error(reader) != mpack_ok) {
                return;
            }
        }
    }
}

void oracle_reader_digest(const uint8_t* data, size_t len, oracle_digest_t* out) {
    digest_clear(out);
    if (data == NULL && len != 0) {
        out->error = (int32_t)mpack_error_bug;
        return;
    }
    if (len > ORACLE_MAX_INPUT) {
        len = ORACLE_MAX_INPUT;
    }

    mpack_reader_t reader;
    mpack_reader_init_data(&reader, (const char*)data, len);
    walk_reader(&reader, out);

    size_t remaining = mpack_reader_remaining(&reader, NULL);
    out->bytes_used = (uint32_t)(len - remaining);
    out->error = (int32_t)mpack_reader_destroy(&reader);
    /* Cursor advancement on truncated payloads can differ from the Rust port;
     * compare structure + sticky error only when failing. */
    if (out->error != (int32_t)mpack_ok) {
        out->bytes_used = 0;
    }
}
