//! Audited C ABI boundary.

mod guard;
pub mod types;
mod writer;

#[cfg(feature = "ffi-harness")]
#[doc(hidden)]
pub mod harness;
