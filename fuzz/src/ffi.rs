//! FFI bindings to the original-C oracle helpers.

use crate::digest::{Digest, WriterTransfer, MAX_OUTPUT_LEN, MAX_RECORDS, RECORD_SIZE};

#[repr(C)]
struct OracleDigest {
    error: i32,
    bytes_used: u32,
    record_count: u32,
    truncated: u32,
    records: [u8; MAX_RECORDS * RECORD_SIZE],
}

#[repr(C)]
struct OracleWriterResult {
    reader_error: i32,
    writer_error: i32,
    out_len: u32,
    truncated: u32,
}

extern "C" {
    fn oracle_reader_digest(data: *const u8, len: usize, out: *mut OracleDigest);
    fn oracle_node_digest(data: *const u8, len: usize, out: *mut OracleDigest);
    fn oracle_writer_transfer(
        data: *const u8,
        len: usize,
        out: *mut u8,
        out_cap: usize,
        result: *mut OracleWriterResult,
    );
    fn oracle_expect_digest(
        ops: *const u8,
        ops_len: usize,
        payload: *const u8,
        payload_len: usize,
        out: *mut OracleDigest,
    );
}

fn from_c(raw: OracleDigest) -> Digest {
    let n = (raw.record_count as usize).min(MAX_RECORDS) * RECORD_SIZE;
    Digest {
        error: raw.error,
        bytes_used: raw.bytes_used,
        record_count: raw.record_count,
        truncated: raw.truncated,
        records: raw.records[..n].to_vec(),
    }
}

/// C-oracle reader digest for one top-level value.
pub fn reader_digest_c(data: &[u8]) -> Digest {
    let mut raw = OracleDigest {
        error: 0,
        bytes_used: 0,
        record_count: 0,
        truncated: 0,
        records: [0; MAX_RECORDS * RECORD_SIZE],
    };
    unsafe {
        oracle_reader_digest(data.as_ptr(), data.len(), &mut raw);
    }
    from_c(raw)
}

/// C-oracle node/tree digest for one MessagePack message.
pub fn node_digest_c(data: &[u8]) -> Digest {
    let mut raw = OracleDigest {
        error: 0,
        bytes_used: 0,
        record_count: 0,
        truncated: 0,
        records: [0; MAX_RECORDS * RECORD_SIZE],
    };
    unsafe {
        oracle_node_digest(data.as_ptr(), data.len(), &mut raw);
    }
    from_c(raw)
}

/// C-oracle read→rewrite transfer.
pub fn writer_transfer_c(data: &[u8]) -> WriterTransfer {
    let mut out = vec![0u8; MAX_OUTPUT_LEN];
    let mut result = OracleWriterResult {
        reader_error: 0,
        writer_error: 0,
        out_len: 0,
        truncated: 0,
    };
    unsafe {
        oracle_writer_transfer(
            data.as_ptr(),
            data.len(),
            out.as_mut_ptr(),
            out.len(),
            &mut result,
        );
    }
    out.truncate(result.out_len as usize);
    WriterTransfer {
        reader_error: result.reader_error,
        writer_error: result.writer_error,
        truncated: result.truncated,
        out,
    }
}

fn split_expect_input(data: &[u8]) -> (&[u8], &[u8]) {
    if data.is_empty() {
        return (&[], &[]);
    }
    let rest = &data[1..];
    let split = (data[0] as usize).min(rest.len());
    (&rest[..split], &rest[split..])
}

/// C-oracle expect opcode digest.
pub fn expect_digest_c(data: &[u8]) -> Digest {
    let (ops, payload) = split_expect_input(data);
    let mut raw = OracleDigest {
        error: 0,
        bytes_used: 0,
        record_count: 0,
        truncated: 0,
        records: [0; MAX_RECORDS * RECORD_SIZE],
    };
    unsafe {
        oracle_expect_digest(
            ops.as_ptr(),
            ops.len(),
            payload.as_ptr(),
            payload.len(),
            &mut raw,
        );
    }
    let mut digest = from_c(raw);
    if digest.error != 0 {
        digest.bytes_used = 0;
    }
    digest
}
