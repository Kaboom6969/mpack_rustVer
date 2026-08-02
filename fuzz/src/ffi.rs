//! FFI bindings to the original-C oracle helpers.

use crate::digest::{Digest, MAX_RECORDS, RECORD_SIZE};

#[repr(C)]
struct OracleDigest {
    error: i32,
    bytes_used: u32,
    record_count: u32,
    truncated: u32,
    records: [u8; MAX_RECORDS * RECORD_SIZE],
}

extern "C" {
    fn oracle_reader_digest(data: *const u8, len: usize, out: *mut OracleDigest);
    fn oracle_node_digest(data: *const u8, len: usize, out: *mut OracleDigest);
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
