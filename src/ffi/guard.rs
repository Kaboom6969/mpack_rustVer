//! Panic containment shared by C ABI entry points.

use std::panic::{catch_unwind, AssertUnwindSafe};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FfiPanic;

/// Runs an ABI operation without allowing a Rust panic to cross into C.
#[doc(hidden)]
pub fn catch_ffi_panic<T>(operation: impl FnOnce() -> T) -> Result<T, FfiPanic> {
    catch_unwind(AssertUnwindSafe(operation)).map_err(|_| FfiPanic)
}
