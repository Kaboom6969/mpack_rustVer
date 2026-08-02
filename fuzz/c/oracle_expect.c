/*
 * C MPack expect oracle: opcode stream drives mpack_expect_* over a payload.
 * Record layout matches oracle_digest_t (opcode in byte0, ok_flag in byte1).
 */

#include "oracle_digest.h"

#include "mpack/mpack.h"

#include <string.h>

enum {
    OP_NIL = 0,
    OP_BOOL = 1,
    OP_TRUE = 2,
    OP_FALSE = 3,
    OP_U8 = 4,
    OP_U16 = 5,
    OP_U32 = 6,
    OP_U64 = 7,
    OP_I8 = 8,
    OP_I16 = 9,
    OP_I32 = 10,
    OP_I64 = 11,
    OP_U8_RANGE = 12,
    OP_U16_RANGE = 13,
    OP_U32_RANGE = 14,
    OP_U64_RANGE = 15,
    OP_I8_RANGE = 16,
    OP_I16_RANGE = 17,
    OP_I32_RANGE = 18,
    OP_I64_RANGE = 19,
    OP_UINT_MATCH = 20,
    OP_INT_MATCH = 21,
    OP_FLOAT = 22,
    OP_DOUBLE = 23,
    OP_FLOAT_STRICT = 24,
    OP_DOUBLE_STRICT = 25,
    OP_FLOAT_RANGE = 26,
    OP_DOUBLE_RANGE = 27,
    OP_MAP = 28,
    OP_MAP_RANGE = 29,
    OP_MAP_MATCH = 30,
    OP_MAP_OR_NIL = 31,
    OP_MAP_MAX_OR_NIL = 32,
    OP_ARRAY = 33,
    OP_ARRAY_RANGE = 34,
    OP_ARRAY_MATCH = 35,
    OP_ARRAY_OR_NIL = 36,
    OP_ARRAY_MAX_OR_NIL = 37,
    OP_STR = 38,
    OP_STR_BUF = 39,
    OP_UTF8 = 40,
    OP_STR_MATCH = 41,
    OP_CSTR = 42,
    OP_UTF8_CSTR = 43,
    OP_BIN = 44,
    OP_BIN_BUF = 45,
    OP_BIN_SIZE_BUF = 46,
    OP_EXT = 47,
    OP_EXT_BUF = 48,
    OP_TAG = 49,
    OP_TIMESTAMP = 50,
    OP_TIMESTAMP_TRUNCATE = 51,
    OP_KEY_UINT = 52,
    OP_KEY_CSTR = 53,
    OP_COUNT = 54
};

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

static uint64_t read_u64_le(const uint8_t* src, size_t len, size_t* cursor) {
    uint64_t value = 0;
    for (int i = 0; i < 8; ++i) {
        uint8_t b = 0;
        if (*cursor < len) {
            b = src[(*cursor)++];
        }
        value |= (uint64_t)b << (8 * i);
    }
    return value;
}

static uint32_t read_u32_le(const uint8_t* src, size_t len, size_t* cursor) {
    uint32_t value = 0;
    for (int i = 0; i < 4; ++i) {
        uint8_t b = 0;
        if (*cursor < len) {
            b = src[(*cursor)++];
        }
        value |= (uint32_t)b << (8 * i);
    }
    return value;
}

static uint16_t read_u16_le(const uint8_t* src, size_t len, size_t* cursor) {
    uint16_t value = 0;
    for (int i = 0; i < 2; ++i) {
        uint8_t b = 0;
        if (*cursor < len) {
            b = src[(*cursor)++];
        }
        value |= (uint16_t)b << (8 * i);
    }
    return value;
}

static uint8_t read_u8(const uint8_t* src, size_t len, size_t* cursor) {
    if (*cursor < len) {
        return src[(*cursor)++];
    }
    return 0;
}

static uint32_t fnv1a32(const uint8_t* data, size_t len) {
    uint32_t hash = 2166136261u;
    for (size_t i = 0; i < len; ++i) {
        hash ^= data[i];
        hash *= 16777619u;
    }
    return hash;
}

static int digest_push(
        oracle_digest_t* out,
        uint8_t opcode,
        uint8_t ok,
        uint64_t value,
        uint32_t payload_hash) {
    if (out->record_count >= ORACLE_MAX_RECORDS) {
        out->truncated = 1;
        return 0;
    }
    size_t off = (size_t)out->record_count * ORACLE_RECORD_SIZE;
    uint8_t* rec = &out->records[off];
    rec[0] = opcode;
    rec[1] = ok;
    rec[2] = 0;
    rec[3] = 0;
    write_u64_le(rec + 4, value);
    write_u32_le(rec + 12, payload_hash);
    out->record_count += 1;
    return 1;
}

static uint8_t ok_flag(mpack_reader_t* reader) {
    return mpack_reader_error(reader) == mpack_ok ? 1u : 0u;
}

void oracle_expect_digest(
        const uint8_t* ops,
        size_t ops_len,
        const uint8_t* payload,
        size_t payload_len,
        oracle_digest_t* out) {
    memset(out, 0, sizeof(*out));
    if (payload_len > ORACLE_MAX_INPUT) {
        payload_len = ORACLE_MAX_INPUT;
    }

    mpack_reader_t reader;
    mpack_reader_init_data(&reader, (const char*)payload, payload_len);

    size_t cursor = 0;
    char buf[256];
    bool found_keys[8];
    static const char* cstr_keys[3] = {"a", "bb", "ccc"};
    bool found_cstr[3];

    while (cursor < ops_len && !out->truncated) {
        uint8_t raw = ops[cursor++];
        uint8_t opcode = (uint8_t)(raw % OP_COUNT);
        uint8_t ok = 0;
        uint64_t value = 0;
        uint32_t hash = 0;

        switch (opcode) {
            case OP_NIL:
                mpack_expect_nil(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_BOOL: {
                bool v = mpack_expect_bool(&reader);
                ok = ok_flag(&reader);
                value = v ? 1u : 0u;
                break;
            }
            case OP_TRUE:
                mpack_expect_true(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_FALSE:
                mpack_expect_false(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_U8:
                value = mpack_expect_u8(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_U16:
                value = mpack_expect_u16(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_U32:
                value = mpack_expect_u32(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_U64:
                value = mpack_expect_u64(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_I8:
                value = (uint64_t)(int64_t)mpack_expect_i8(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_I16:
                value = (uint64_t)(int64_t)mpack_expect_i16(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_I32:
                value = (uint64_t)(int64_t)mpack_expect_i32(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_I64:
                value = (uint64_t)mpack_expect_i64(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_U8_RANGE: {
                uint8_t min_v = read_u8(ops, ops_len, &cursor);
                uint8_t max_v = read_u8(ops, ops_len, &cursor);
                value = mpack_expect_u8_range(&reader, min_v, max_v);
                ok = ok_flag(&reader);
                break;
            }
            case OP_U16_RANGE: {
                uint16_t min_v = read_u16_le(ops, ops_len, &cursor);
                uint16_t max_v = read_u16_le(ops, ops_len, &cursor);
                value = mpack_expect_u16_range(&reader, min_v, max_v);
                ok = ok_flag(&reader);
                break;
            }
            case OP_U32_RANGE: {
                uint32_t min_v = read_u32_le(ops, ops_len, &cursor);
                uint32_t max_v = read_u32_le(ops, ops_len, &cursor);
                value = mpack_expect_u32_range(&reader, min_v, max_v);
                ok = ok_flag(&reader);
                break;
            }
            case OP_U64_RANGE: {
                uint64_t min_v = read_u64_le(ops, ops_len, &cursor);
                uint64_t max_v = read_u64_le(ops, ops_len, &cursor);
                value = mpack_expect_u64_range(&reader, min_v, max_v);
                ok = ok_flag(&reader);
                break;
            }
            case OP_I8_RANGE: {
                int8_t min_v = (int8_t)read_u8(ops, ops_len, &cursor);
                int8_t max_v = (int8_t)read_u8(ops, ops_len, &cursor);
                value = (uint64_t)(int64_t)mpack_expect_i8_range(&reader, min_v, max_v);
                ok = ok_flag(&reader);
                break;
            }
            case OP_I16_RANGE: {
                int16_t min_v = (int16_t)read_u16_le(ops, ops_len, &cursor);
                int16_t max_v = (int16_t)read_u16_le(ops, ops_len, &cursor);
                value = (uint64_t)(int64_t)mpack_expect_i16_range(&reader, min_v, max_v);
                ok = ok_flag(&reader);
                break;
            }
            case OP_I32_RANGE: {
                int32_t min_v = (int32_t)read_u32_le(ops, ops_len, &cursor);
                int32_t max_v = (int32_t)read_u32_le(ops, ops_len, &cursor);
                value = (uint64_t)(int64_t)mpack_expect_i32_range(&reader, min_v, max_v);
                ok = ok_flag(&reader);
                break;
            }
            case OP_I64_RANGE: {
                int64_t min_v = (int64_t)read_u64_le(ops, ops_len, &cursor);
                int64_t max_v = (int64_t)read_u64_le(ops, ops_len, &cursor);
                value = (uint64_t)mpack_expect_i64_range(&reader, min_v, max_v);
                ok = ok_flag(&reader);
                break;
            }
            case OP_UINT_MATCH: {
                uint64_t want = read_u64_le(ops, ops_len, &cursor);
                mpack_expect_uint_match(&reader, want);
                ok = ok_flag(&reader);
                value = want;
                break;
            }
            case OP_INT_MATCH: {
                int64_t want = (int64_t)read_u64_le(ops, ops_len, &cursor);
                mpack_expect_int_match(&reader, want);
                ok = ok_flag(&reader);
                value = (uint64_t)want;
                break;
            }
            case OP_FLOAT: {
                float v = mpack_expect_float(&reader);
                ok = ok_flag(&reader);
                {
                    uint32_t bits;
                    memcpy(&bits, &v, sizeof(bits));
                    value = bits;
                }
                break;
            }
            case OP_DOUBLE: {
                double v = mpack_expect_double(&reader);
                ok = ok_flag(&reader);
                {
                    uint64_t bits;
                    memcpy(&bits, &v, sizeof(bits));
                    value = bits;
                }
                break;
            }
            case OP_FLOAT_STRICT: {
                float v = mpack_expect_float_strict(&reader);
                ok = ok_flag(&reader);
                {
                    uint32_t bits;
                    memcpy(&bits, &v, sizeof(bits));
                    value = bits;
                }
                break;
            }
            case OP_DOUBLE_STRICT: {
                double v = mpack_expect_double_strict(&reader);
                ok = ok_flag(&reader);
                {
                    uint64_t bits;
                    memcpy(&bits, &v, sizeof(bits));
                    value = bits;
                }
                break;
            }
            case OP_FLOAT_RANGE: {
                uint32_t min_bits = read_u32_le(ops, ops_len, &cursor);
                uint32_t max_bits = read_u32_le(ops, ops_len, &cursor);
                float min_v, max_v, v;
                memcpy(&min_v, &min_bits, sizeof(min_v));
                memcpy(&max_v, &max_bits, sizeof(max_v));
                v = mpack_expect_float_range(&reader, min_v, max_v);
                ok = ok_flag(&reader);
                {
                    uint32_t bits;
                    memcpy(&bits, &v, sizeof(bits));
                    value = bits;
                }
                break;
            }
            case OP_DOUBLE_RANGE: {
                uint64_t min_bits = read_u64_le(ops, ops_len, &cursor);
                uint64_t max_bits = read_u64_le(ops, ops_len, &cursor);
                double min_v, max_v, v;
                memcpy(&min_v, &min_bits, sizeof(min_v));
                memcpy(&max_v, &max_bits, sizeof(max_v));
                v = mpack_expect_double_range(&reader, min_v, max_v);
                ok = ok_flag(&reader);
                {
                    uint64_t bits;
                    memcpy(&bits, &v, sizeof(bits));
                    value = bits;
                }
                break;
            }
            case OP_MAP:
                value = mpack_expect_map(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_MAP_RANGE: {
                uint32_t min_c = read_u32_le(ops, ops_len, &cursor);
                uint32_t max_c = read_u32_le(ops, ops_len, &cursor);
                value = mpack_expect_map_range(&reader, min_c, max_c);
                ok = ok_flag(&reader);
                break;
            }
            case OP_MAP_MATCH: {
                uint32_t count = read_u32_le(ops, ops_len, &cursor);
                mpack_expect_map_match(&reader, count);
                ok = ok_flag(&reader);
                value = count;
                break;
            }
            case OP_MAP_OR_NIL: {
                uint32_t count = 0;
                bool is_map = mpack_expect_map_or_nil(&reader, &count);
                ok = ok_flag(&reader);
                value = count;
                if (!is_map && ok) {
                    value |= (1ull << 32);
                }
                break;
            }
            case OP_MAP_MAX_OR_NIL: {
                uint32_t max_c = read_u32_le(ops, ops_len, &cursor);
                uint32_t count = 0;
                bool is_map = mpack_expect_map_max_or_nil(&reader, max_c, &count);
                ok = ok_flag(&reader);
                value = count;
                if (!is_map && ok) {
                    value |= (1ull << 32);
                }
                break;
            }
            case OP_ARRAY:
                value = mpack_expect_array(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_ARRAY_RANGE: {
                uint32_t min_c = read_u32_le(ops, ops_len, &cursor);
                uint32_t max_c = read_u32_le(ops, ops_len, &cursor);
                value = mpack_expect_array_range(&reader, min_c, max_c);
                ok = ok_flag(&reader);
                break;
            }
            case OP_ARRAY_MATCH: {
                uint32_t count = read_u32_le(ops, ops_len, &cursor);
                mpack_expect_array_match(&reader, count);
                ok = ok_flag(&reader);
                value = count;
                break;
            }
            case OP_ARRAY_OR_NIL: {
                uint32_t count = 0;
                bool is_arr = mpack_expect_array_or_nil(&reader, &count);
                ok = ok_flag(&reader);
                value = count;
                if (!is_arr && ok) {
                    value |= (1ull << 32);
                }
                break;
            }
            case OP_ARRAY_MAX_OR_NIL: {
                uint32_t max_c = read_u32_le(ops, ops_len, &cursor);
                uint32_t count = 0;
                bool is_arr = mpack_expect_array_max_or_nil(&reader, max_c, &count);
                ok = ok_flag(&reader);
                value = count;
                if (!is_arr && ok) {
                    value |= (1ull << 32);
                }
                break;
            }
            case OP_STR:
                value = mpack_expect_str(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_STR_BUF: {
                size_t n = mpack_expect_str_buf(&reader, buf, sizeof(buf));
                ok = ok_flag(&reader);
                value = n;
                if (ok) {
                    hash = fnv1a32((const uint8_t*)buf, n);
                }
                break;
            }
            case OP_UTF8: {
                size_t n = mpack_expect_utf8(&reader, buf, sizeof(buf));
                ok = ok_flag(&reader);
                value = n;
                if (ok) {
                    hash = fnv1a32((const uint8_t*)buf, n);
                }
                break;
            }
            case OP_STR_MATCH: {
                uint8_t n = read_u8(ops, ops_len, &cursor);
                if (n > 32) {
                    n = 32;
                }
                char expect_buf[32];
                memset(expect_buf, 0, sizeof(expect_buf));
                for (uint8_t i = 0; i < n; ++i) {
                    expect_buf[i] = (char)read_u8(ops, ops_len, &cursor);
                }
                mpack_expect_str_match(&reader, expect_buf, n);
                ok = ok_flag(&reader);
                value = n;
                hash = fnv1a32((const uint8_t*)expect_buf, n);
                break;
            }
            case OP_CSTR:
                mpack_expect_cstr(&reader, buf, sizeof(buf));
                ok = ok_flag(&reader);
                if (ok) {
                    size_t n = strlen(buf);
                    value = n;
                    hash = fnv1a32((const uint8_t*)buf, n);
                }
                break;
            case OP_UTF8_CSTR:
                mpack_expect_utf8_cstr(&reader, buf, sizeof(buf));
                ok = ok_flag(&reader);
                if (ok) {
                    size_t n = strlen(buf);
                    value = n;
                    hash = fnv1a32((const uint8_t*)buf, n);
                }
                break;
            case OP_BIN:
                value = mpack_expect_bin(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_BIN_BUF: {
                size_t n = mpack_expect_bin_buf(&reader, buf, sizeof(buf));
                ok = ok_flag(&reader);
                value = n;
                if (ok) {
                    hash = fnv1a32((const uint8_t*)buf, n);
                }
                break;
            }
            case OP_BIN_SIZE_BUF: {
                uint32_t size = read_u32_le(ops, ops_len, &cursor);
                if (size > sizeof(buf)) {
                    size = (uint32_t)sizeof(buf);
                }
                mpack_expect_bin_size_buf(&reader, buf, size);
                ok = ok_flag(&reader);
                value = size;
                if (ok) {
                    hash = fnv1a32((const uint8_t*)buf, size);
                }
                break;
            }
            case OP_EXT: {
                int8_t exttype = 0;
                uint32_t n = mpack_expect_ext(&reader, &exttype);
                ok = ok_flag(&reader);
                value = ((uint64_t)(uint8_t)exttype << 32) | n;
                break;
            }
            case OP_EXT_BUF: {
                int8_t exttype = 0;
                size_t n = mpack_expect_ext_buf(&reader, &exttype, buf, sizeof(buf));
                ok = ok_flag(&reader);
                value = ((uint64_t)(uint8_t)exttype << 32) | n;
                if (ok) {
                    hash = fnv1a32((const uint8_t*)buf, n);
                }
                break;
            }
            case OP_TAG: {
                uint8_t t = read_u8(ops, ops_len, &cursor) % 12;
                mpack_tag_t tag = mpack_tag_nil();
                switch (t) {
                    case 1: tag = mpack_tag_nil(); break;
                    case 2: tag = mpack_tag_bool(read_u8(ops, ops_len, &cursor) & 1); break;
                    case 3: tag = mpack_tag_int((int64_t)read_u64_le(ops, ops_len, &cursor)); break;
                    case 4: tag = mpack_tag_uint(read_u64_le(ops, ops_len, &cursor)); break;
                    case 5: {
                        uint32_t bits = read_u32_le(ops, ops_len, &cursor);
                        float f;
                        memcpy(&f, &bits, sizeof(f));
                        tag = mpack_tag_float(f);
                        break;
                    }
                    case 6: {
                        uint64_t bits = read_u64_le(ops, ops_len, &cursor);
                        double d;
                        memcpy(&d, &bits, sizeof(d));
                        tag = mpack_tag_double(d);
                        break;
                    }
                    case 7: tag = mpack_tag_str(read_u32_le(ops, ops_len, &cursor)); break;
                    case 8: tag = mpack_tag_bin(read_u32_le(ops, ops_len, &cursor)); break;
                    case 9: tag = mpack_tag_array(read_u32_le(ops, ops_len, &cursor)); break;
                    case 10: tag = mpack_tag_map(read_u32_le(ops, ops_len, &cursor)); break;
                    case 11: {
                        int8_t et = (int8_t)read_u8(ops, ops_len, &cursor);
                        uint32_t n = read_u32_le(ops, ops_len, &cursor);
                        tag = mpack_tag_ext(et, n);
                        break;
                    }
                    default: tag = mpack_tag_nil(); break;
                }
                mpack_expect_tag(&reader, tag);
                ok = ok_flag(&reader);
                value = t;
                break;
            }
            case OP_TIMESTAMP: {
                mpack_timestamp_t ts = mpack_expect_timestamp(&reader);
                ok = ok_flag(&reader);
                value = ((uint64_t)(uint32_t)ts.nanoseconds << 32) ^ (uint64_t)ts.seconds;
                break;
            }
            case OP_TIMESTAMP_TRUNCATE:
                value = (uint64_t)mpack_expect_timestamp_truncate(&reader);
                ok = ok_flag(&reader);
                break;
            case OP_KEY_UINT: {
                uint8_t n = read_u8(ops, ops_len, &cursor);
                if (n == 0 || n > 8) {
                    n = 4;
                }
                memset(found_keys, 0, sizeof(found_keys));
                size_t idx = mpack_expect_key_uint(&reader, found_keys, n);
                ok = ok_flag(&reader);
                value = idx;
                break;
            }
            case OP_KEY_CSTR: {
                memset(found_cstr, 0, sizeof(found_cstr));
                size_t idx = mpack_expect_key_cstr(&reader, cstr_keys, found_cstr, 3);
                ok = ok_flag(&reader);
                value = idx;
                break;
            }
            default:
                break;
        }

        if (!ok) {
            value = 0;
            hash = 0;
        }

        if (!digest_push(out, opcode, ok, value, hash)) {
            break;
        }
        /* First sticky error ends the op walk so truncated type-byte vs
         * read_tag paths cannot emit divergent trailing no-op records. */
        if (mpack_reader_error(&reader) != mpack_ok) {
            break;
        }
    }

    out->bytes_used = (uint32_t)(payload_len - mpack_reader_remaining(&reader, NULL));
    out->error = (int32_t)mpack_reader_destroy(&reader);
    if (out->error != 0) {
        out->bytes_used = 0;
    }
}
