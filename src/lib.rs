//! Rust port of MPack (MessagePack). Scaffold modules mirror the C library.

#![deny(unsafe_op_in_unsafe_fn)]

#[forbid(unsafe_code)]
pub mod common;
#[forbid(unsafe_code)]
pub mod expect;
#[cfg(feature = "ffi")]
pub mod ffi;
#[forbid(unsafe_code)]
pub mod node;
#[forbid(unsafe_code)]
pub mod reader;
#[forbid(unsafe_code)]
pub mod writer;

/// Crate version string (scaffold; not the upstream MPack version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
