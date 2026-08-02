//! Audited C ABI boundary.

mod common;
mod guard;
pub mod types;
pub mod writer;

#[cfg(feature = "full-suite-abi")]
pub(crate) mod reader;

#[cfg(feature = "full-suite-abi")]
mod expect;

#[cfg(feature = "full-suite-abi")]
pub(crate) mod stubs;

#[cfg(feature = "ffi-harness")]
#[doc(hidden)]
pub mod harness;
