/*
 * C MPack writer oracle: read→rewrite transfer mirroring upstream AFL fuzz.c.
 * Iterative compound stack (depth capped) so hostile nesting does not blow the
 * fuzz worker stack.
 */

#include "oracle_digest.h"

#include "mpack/mpack.h"

#include <string.h>

typedef struct {
    mpack_type_t type;
    uint32_t remaining;
} transfer_frame_t;

static void transfer_bytes(mpack_reader_t* reader, mpack_writer_t* writer, uint32_t count) {
    while (count > 0) {
        char buffer[256];
        uint32_t step = count < (uint32_t)sizeof(buffer) ? count : (uint32_t)sizeof(buffer);
        mpack_read_bytes(reader, buffer, step);
        if (mpack_reader_error(reader) != mpack_ok) {
            return;
        }
        mpack_write_bytes(writer, buffer, step);
        if (mpack_writer_error(writer) != mpack_ok) {
            return;
        }
        count -= step;
    }
}

static void transfer_walk(mpack_reader_t* reader, mpack_writer_t* writer) {
    transfer_frame_t stack[ORACLE_DEPTH_LIMIT];
    int depth = 0;
    int need = 1;

    while (need > 0 || depth > 0) {
        if (mpack_reader_error(reader) != mpack_ok || mpack_writer_error(writer) != mpack_ok) {
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

        mpack_write_tag(writer, tag);
        if (mpack_writer_error(writer) != mpack_ok) {
            return;
        }

        if (need > 0) {
            need -= 1;
        } else if (depth > 0) {
            stack[depth - 1].remaining -= 1;
        }

        switch (tag.type) {
            #if MPACK_EXTENSIONS
            case mpack_type_ext: /* fallthrough */
            #endif
            case mpack_type_str: /* fallthrough */
            case mpack_type_bin: {
                uint32_t count = mpack_tag_bytes(&tag);
                transfer_bytes(reader, writer, count);
                if (mpack_reader_error(reader) != mpack_ok || mpack_writer_error(writer) != mpack_ok) {
                    return;
                }
                mpack_done_type(reader, tag.type);
                mpack_finish_type(writer, tag.type);
                break;
            }
            case mpack_type_array: {
                stack[depth].type = mpack_type_array;
                stack[depth].remaining = mpack_tag_array_count(&tag);
                depth += 1;
                break;
            }
            case mpack_type_map: {
                uint32_t pairs = mpack_tag_map_count(&tag);
                uint64_t elems = (uint64_t)pairs * 2u;
                if (elems > UINT32_MAX) {
                    mpack_reader_flag_error(reader, mpack_error_too_big);
                    return;
                }
                stack[depth].type = mpack_type_map;
                stack[depth].remaining = (uint32_t)elems;
                depth += 1;
                break;
            }
            default:
                break;
        }

        while (depth > 0 && stack[depth - 1].remaining == 0) {
            mpack_type_t type = stack[depth - 1].type;
            depth -= 1;
            mpack_done_type(reader, type);
            mpack_finish_type(writer, type);
            if (mpack_reader_error(reader) != mpack_ok || mpack_writer_error(writer) != mpack_ok) {
                return;
            }
        }
    }
}

void oracle_writer_transfer(
        const uint8_t* in,
        size_t in_len,
        uint8_t* out,
        size_t out_cap,
        oracle_writer_result_t* result) {
    memset(result, 0, sizeof(*result));
    if (out != NULL && out_cap > 0) {
        memset(out, 0, out_cap);
    }

    if (in_len > ORACLE_MAX_INPUT) {
        in_len = ORACLE_MAX_INPUT;
    }

    char* data = NULL;
    size_t size = 0;
    mpack_writer_t writer;
    mpack_writer_init_growable(&writer, &data, &size);

    mpack_reader_t reader;
    mpack_reader_init_data(&reader, (const char*)in, in_len);

    transfer_walk(&reader, &writer);

    result->reader_error = (int32_t)mpack_reader_destroy(&reader);
    result->writer_error = (int32_t)mpack_writer_destroy(&writer);

    if (data != NULL) {
        size_t copy = size;
        if (copy > out_cap) {
            copy = out_cap;
            result->truncated = 1;
        }
        if (out != NULL && copy > 0) {
            memcpy(out, data, copy);
        }
        result->out_len = (uint32_t)copy;
        MPACK_FREE(data);
    }
}
