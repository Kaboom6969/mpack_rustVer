//! Differential fuzz suite: original C MPack vs Rust safe core.
//!
//! Run under Linux/WSL with nightly + cargo-fuzz (see README.md).

pub mod digest;
pub mod ffi;

pub use digest::{
    expect_digest_rust, node_digest_rust, reader_digest_rust, writer_transfer_rust, Digest,
    WriterTransfer, MAX_INPUT_LEN,
};
pub use ffi::{expect_digest_c, node_digest_c, reader_digest_c, writer_transfer_c};
