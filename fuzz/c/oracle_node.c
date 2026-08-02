/*
 * C MPack node oracle: tree parse + preorder walk of one message.
 */

#include "oracle_digest.h"

#include "mpack/mpack.h"

#include <string.h>

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

static uint32_t fnv1a32(const uint8_t* data, size_t len) {
    uint32_t hash = 2166136261u;
    for (size_t i = 0; i < len; ++i) {
        hash ^= data[i];
        hash *= 16777619u;
    }
    return hash;
}

static int walk_node(mpack_node_t node, oracle_digest_t* out, int depth) {
    if (out->truncated) {
        return 0;
    }
    if (depth >= ORACLE_DEPTH_LIMIT) {
        mpack_tree_flag_error(node.tree, mpack_error_too_big);
        return 0;
    }
    if (mpack_tree_error(node.tree) != mpack_ok) {
        return 0;
    }

    mpack_type_t type = mpack_node_type(node);
    uint8_t aux = 0;
    uint64_t value = 0;
    uint32_t payload_hash = 0;

    switch (type) {
        case mpack_type_nil:
            break;
        case mpack_type_bool:
            aux = mpack_node_bool(node) ? 1 : 0;
            break;
        case mpack_type_int:
            value = (uint64_t)mpack_node_i64(node);
            break;
        case mpack_type_uint:
            value = mpack_node_u64(node);
            break;
        case mpack_type_float: {
            float f = mpack_node_float_strict(node);
            uint32_t bits = 0;
            memcpy(&bits, &f, 4);
            value = bits;
            break;
        }
        case mpack_type_double: {
            double d = mpack_node_double_strict(node);
            memcpy(&value, &d, 8);
            break;
        }
        case mpack_type_str: {
            value = mpack_node_data_len(node);
            const char* payload = mpack_node_str(node);
            if (payload != NULL) {
                payload_hash = fnv1a32((const uint8_t*)payload, (size_t)value);
            }
            break;
        }
        case mpack_type_bin: {
            value = mpack_node_data_len(node);
            const char* payload = mpack_node_bin_data(node);
            if (payload != NULL) {
                payload_hash = fnv1a32((const uint8_t*)payload, (size_t)value);
            }
            break;
        }
        case mpack_type_ext: {
            aux = (uint8_t)mpack_node_exttype(node);
            value = mpack_node_data_len(node);
            const char* payload = mpack_node_data(node);
            if (payload != NULL) {
                payload_hash = fnv1a32((const uint8_t*)payload, (size_t)value);
            }
            break;
        }
        case mpack_type_array:
            value = mpack_node_array_length(node);
            break;
        case mpack_type_map:
            value = mpack_node_map_count(node);
            break;
        default:
            break;
    }

    if (mpack_tree_error(node.tree) != mpack_ok) {
        return 0;
    }
    if (!digest_push(out, type, aux, value, payload_hash)) {
        return 0;
    }

    if (type == mpack_type_array) {
        for (size_t i = 0; i < (size_t)value; ++i) {
            if (!walk_node(mpack_node_array_at(node, i), out, depth + 1)) {
                return 0;
            }
        }
    } else if (type == mpack_type_map) {
        for (size_t i = 0; i < (size_t)value; ++i) {
            if (!walk_node(mpack_node_map_key_at(node, i), out, depth + 1)) {
                return 0;
            }
            if (!walk_node(mpack_node_map_value_at(node, i), out, depth + 1)) {
                return 0;
            }
        }
    }
    return mpack_tree_error(node.tree) == mpack_ok;
}

void oracle_node_digest(const uint8_t* data, size_t len, oracle_digest_t* out) {
    digest_clear(out);
    if (data == NULL && len != 0) {
        out->error = (int32_t)mpack_error_bug;
        return;
    }
    if (len > ORACLE_MAX_INPUT) {
        len = ORACLE_MAX_INPUT;
    }

    mpack_tree_t tree;
    mpack_tree_init_data(&tree, (const char*)data, len);
    mpack_tree_parse(&tree);

    if (mpack_tree_error(&tree) == mpack_ok) {
        walk_node(mpack_tree_root(&tree), out, 0);
    }

    out->bytes_used = (uint32_t)mpack_tree_size(&tree);
    out->error = (int32_t)mpack_tree_destroy(&tree);
    if (out->error != (int32_t)mpack_ok) {
        out->bytes_used = 0;
    }
}
