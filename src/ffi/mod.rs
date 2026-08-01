//! Audited C ABI boundary.

mod common;
mod guard;
pub mod types;
pub mod writer;

#[cfg(feature = "full-suite-abi")]
mod reader;

#[cfg(feature = "full-suite-abi")]
mod stubs;

#[cfg(feature = "ffi-harness")]
#[doc(hidden)]
pub mod harness;
