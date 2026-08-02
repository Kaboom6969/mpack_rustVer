/*
 * Oracle config for differential fuzz (included as mpack-config.h via
 * MPACK_HAS_CONFIG). Enables reader+node+stdlib+extensions; disables writer,
 * expect, stdio, and debug asserts so the fuzzer stays lean and non-fatal.
 */

#ifndef MPACK_CONFIG_H
#define MPACK_CONFIG_H 1

#define MPACK_READER 1
#define MPACK_NODE 1
#define MPACK_WRITER 0
#define MPACK_EXPECT 0
#define MPACK_STDLIB 1
#define MPACK_STDIO 0
#define MPACK_EXTENSIONS 1
#define MPACK_COMPATIBILITY 1
#define MPACK_DEBUG 0
#define MPACK_READ_TRACKING 0
#define MPACK_WRITE_TRACKING 0

#endif
