/*
 * Shared digest layout for C oracle helpers and the Rust mirror.
 *
 * Record format (16 bytes, little-endian fields):
 *   [0]     mpack_type_t
 *   [1]     bool (0/1) or ext typeid as i8 bit pattern
 *   [2..3]  reserved (0)
 *   [4..11] u64: int/uint value, float/double bit pattern, or length/count
 *   [12..15] FNV-1a-32 of raw str/bin/ext payload (0 otherwise)
 */

#ifndef ORACLE_DIGEST_H
#define ORACLE_DIGEST_H 1

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define ORACLE_MAX_RECORDS 4096u
#define ORACLE_RECORD_SIZE 16u
#define ORACLE_DEPTH_LIMIT 1024
#define ORACLE_MAX_INPUT 65536u

typedef struct oracle_digest_t {
    int32_t error;
    uint32_t bytes_used;
    uint32_t record_count;
    uint32_t truncated;
    uint8_t records[ORACLE_MAX_RECORDS * ORACLE_RECORD_SIZE];
} oracle_digest_t;

void oracle_reader_digest(const uint8_t* data, size_t len, oracle_digest_t* out);
void oracle_node_digest(const uint8_t* data, size_t len, oracle_digest_t* out);

#ifdef __cplusplus
}
#endif

#endif
