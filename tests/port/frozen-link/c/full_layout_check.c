/*
 * Full-suite ABI layout probe. Compiled against tests/original mpack-config.h
 * with C everything feature macros. Linked from the frozen-link --everything gate.
 */
#include <stddef.h>
#include <stdio.h>

#include "mpack/mpack.h"

int mpack_full_layout_check(void) {
    int failures = 0;

#define CHECK_SIZE(T, expected)                                                    \
    do {                                                                           \
        if (sizeof(T) != (size_t)(expected)) {                                     \
            fprintf(stderr, "sizeof(" #T ")=%zu expected %d\n", sizeof(T),         \
                    (int)(expected));                                              \
            ++failures;                                                            \
        }                                                                          \
    } while (0)

#define CHECK_OFF(T, field, expected)                                              \
    do {                                                                           \
        if (offsetof(T, field) != (size_t)(expected)) {                            \
            fprintf(stderr, "offsetof(" #T ", " #field ")=%zu expected %d\n",      \
                    offsetof(T, field), (int)(expected));                          \
            ++failures;                                                            \
        }                                                                          \
    } while (0)

    CHECK_SIZE(mpack_tag_t, 16);
    CHECK_OFF(mpack_tag_t, type, 0);
    CHECK_OFF(mpack_tag_t, exttype, 4);

    CHECK_SIZE(mpack_writer_t, 168);
    CHECK_OFF(mpack_writer_t, version, 0);
    CHECK_OFF(mpack_writer_t, flush, 8);
    CHECK_OFF(mpack_writer_t, error, 64);
    CHECK_OFF(mpack_writer_t, track, 72);
    CHECK_OFF(mpack_writer_t, builder, 112);

    CHECK_SIZE(mpack_reader_t, 104);
    CHECK_OFF(mpack_reader_t, error, 72);
    CHECK_OFF(mpack_reader_t, track, 80);

    CHECK_SIZE(mpack_tree_t, 288);
    CHECK_OFF(mpack_tree_t, error, 64);
    CHECK_OFF(mpack_tree_t, parser, 136);
    CHECK_OFF(mpack_tree_t, root, 256);

    CHECK_SIZE(mpack_node_t, 16);
    CHECK_SIZE(mpack_node_data_t, 16);

#undef CHECK_SIZE
#undef CHECK_OFF
    return failures;
}
