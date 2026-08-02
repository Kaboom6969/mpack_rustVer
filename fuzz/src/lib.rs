//! Differential fuzz suite: original C MPack vs Rust safe core.
//!
//! Run under Linux/WSL with nightly + cargo-fuzz (see README.md).

pub mod digest;
pub mod ffi;

pub use digest::{node_digest_rust, reader_digest_rust, Digest, MAX_INPUT_LEN};
pub use ffi::{node_digest_c, reader_digest_c};
