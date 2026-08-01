#include <stddef.h>
#include <stdint.h>

#include "mpack/mpack.h"

#define CANARY_BEFORE UINT64_C(0x13579bdf2468ace0)
#define CANARY_AFTER UINT64_C(0xfedcba9876543210)

typedef struct guarded_writer_t {
    uint64_t before;
    mpack_writer_t writer;
    uint64_t after;
} guarded_writer_t;

static int canaries_are_intact(const guarded_writer_t* guarded) {
    return guarded->before == CANARY_BEFORE && guarded->after == CANARY_AFTER;
}

int mpack_harness_write_nil(void) {
    char buffer[1] = {0};
    guarded_writer_t guarded;
    guarded.before = CANARY_BEFORE;
    guarded.after = CANARY_AFTER;

    mpack_writer_init(&guarded.writer, buffer, sizeof(buffer));
    if (guarded.writer.flush != NULL ||
            guarded.writer.error_fn != NULL ||
            guarded.writer.teardown != NULL ||
            guarded.writer.context != NULL)
        return 1;

    mpack_write_nil(&guarded.writer);
    if (mpack_writer_error(&guarded.writer) != mpack_ok)
        return 2;
    if (mpack_writer_buffer_used(&guarded.writer) != 1)
        return 3;
    if ((unsigned char)buffer[0] != 0xc0)
        return 4;
    if (mpack_writer_destroy(&guarded.writer) != mpack_ok)
        return 5;
    if (!canaries_are_intact(&guarded))
        return 6;

    return 0;
}

int mpack_harness_sticky_too_big(void) {
    char buffer[1] = {0};
    guarded_writer_t guarded;
    guarded.before = CANARY_BEFORE;
    guarded.after = CANARY_AFTER;

    mpack_writer_init(&guarded.writer, buffer, 0);
    mpack_write_nil(&guarded.writer);
    if (mpack_writer_error(&guarded.writer) != mpack_error_too_big)
        return 1;
    if ((int)mpack_writer_error(&guarded.writer) != 6)
        return 2;
    if (mpack_writer_buffer_used(&guarded.writer) != 0)
        return 3;

    mpack_write_nil(&guarded.writer);
    if (mpack_writer_error(&guarded.writer) != mpack_error_too_big)
        return 4;
    if (mpack_writer_buffer_used(&guarded.writer) != 0)
        return 5;
    if (mpack_writer_destroy(&guarded.writer) != mpack_error_too_big)
        return 6;
    if (!canaries_are_intact(&guarded))
        return 7;

    return 0;
}

int mpack_harness_null_contract(void) {
    char buffer[1] = {0};
    mpack_writer_t writer;

    mpack_writer_init(NULL, buffer, sizeof(buffer));
    mpack_write_nil(NULL);
    if (mpack_writer_destroy(NULL) != mpack_error_bug)
        return 1;

    mpack_writer_init(&writer, NULL, 0);
    if (mpack_writer_error(&writer) != mpack_error_bug)
        return 2;
    if (mpack_writer_destroy(&writer) != mpack_error_bug)
        return 3;

    return 0;
}

size_t mpack_harness_sizeof_writer(void) {
    return sizeof(mpack_writer_t);
}

size_t mpack_harness_offset_flush(void) {
    return offsetof(mpack_writer_t, flush);
}

size_t mpack_harness_offset_error_fn(void) {
    return offsetof(mpack_writer_t, error_fn);
}

size_t mpack_harness_offset_teardown(void) {
    return offsetof(mpack_writer_t, teardown);
}

size_t mpack_harness_offset_context(void) {
    return offsetof(mpack_writer_t, context);
}

size_t mpack_harness_offset_buffer(void) {
    return offsetof(mpack_writer_t, buffer);
}

size_t mpack_harness_offset_position(void) {
    return offsetof(mpack_writer_t, position);
}

size_t mpack_harness_offset_end(void) {
    return offsetof(mpack_writer_t, end);
}

size_t mpack_harness_offset_error(void) {
    return offsetof(mpack_writer_t, error);
}
