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
pub(crate) mod node;

#[cfg(feature = "full-suite-abi")]
pub(crate) mod stubs;

/// Port-test hook: force the next writer/reader track init to fail.
#[cfg(feature = "full-suite-abi")]
#[doc(hidden)]
pub use stubs::track::force_track_init_fail_for_tests;

#[cfg(feature = "ffi-harness")]
#[doc(hidden)]
pub mod harness;
