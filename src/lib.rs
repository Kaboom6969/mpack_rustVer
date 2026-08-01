//! Rust port of MPack (MessagePack). Scaffold modules mirror the C library.

pub mod common;
pub mod expect;
pub mod node;
pub mod reader;
pub mod writer;

/// Crate version string (scaffold; not the upstream MPack version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
