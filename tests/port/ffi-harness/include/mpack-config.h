#ifndef MPACK_CONFIG_H
#define MPACK_CONFIG_H 1

/*
 * Lock the C harness to the upstream embed-writer variant. These values are
 * explicit because they define the mpack_writer_t layout mirrored by Rust.
 */
#define MPACK_VARIANT_BUILDS 1
#define MPACK_READER 0
#define MPACK_EXPECT 0
#define MPACK_NODE 0
#define MPACK_WRITER 1
#define MPACK_STDLIB 0
#define MPACK_STDIO 0
#define MPACK_COMPATIBILITY 0
#define MPACK_EXTENSIONS 0
#define MPACK_BUILDER 0
#define MPACK_READ_TRACKING 0
#define MPACK_WRITE_TRACKING 0

#ifdef MPACK_MALLOC
#error "embed-writer harness must not define MPACK_MALLOC"
#endif

#endif
