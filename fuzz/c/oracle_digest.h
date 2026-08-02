/*
 * Shared digest layout for C oracle helpers and the Rust mirror.
 *
 * Record format (16 bytes, little-endian fields):
 *   [0]     mpack_type_t  (reader/node) or expect opcode
 *   [1]     bool (0/1), ext typeid, or expect ok_flag
 *   [2..3]  reserved (0)
 *   [4..11] u64: int/uint value, float/double bit pattern, length/count,
 *           or expect return bits
 *   [12..15] FNV-1a-32 of raw str/bin/ext / expect buffer payload (0 otherwise)
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
#define ORACLE_MAX_OUTPUT (1u << 20)

typedef struct oracle_digest_t {
    int32_t error;
    uint32_t bytes_used;
    uint32_t record_count;
    uint32_t truncated;
    uint8_t records[ORACLE_MAX_RECORDS * ORACLE_RECORD_SIZE];
} oracle_digest_t;

typedef struct oracle_writer_result_t {
    int32_t reader_error;
    int32_t writer_error;
    uint32_t out_len;
    uint32_t truncated;
} oracle_writer_result_t;

void oracle_reader_digest(const uint8_t* data, size_t len, oracle_digest_t* out);
void oracle_node_digest(const uint8_t* data, size_t len, oracle_digest_t* out);

/* Read→rewrite transfer (mirrors upstream AFL fuzz.c transfer_element). */
void oracle_writer_transfer(
        const uint8_t* in,
        size_t in_len,
        uint8_t* out,
        size_t out_cap,
        oracle_writer_result_t* result);

/* Opcode-driven expect walk over a MessagePack payload. */
void oracle_expect_digest(
        const uint8_t* ops,
        size_t ops_len,
        const uint8_t* payload,
        size_t payload_len,
        oracle_digest_t* out);

#ifdef __cplusplus
}
#endif

#endif
