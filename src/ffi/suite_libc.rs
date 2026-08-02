//! Stdio / allocator entry points for FFI.
//!
//! Under frozen-link (`--cfg mpack_frozen_link` + `full-suite-abi`) Rust must call
//! the suite's `test_*` hooks explicitly: C macros that remap `fopen`/`MPACK_MALLOC`
//! are invisible to Rust. Without that cfg, the library embeds cargo-test shims.

use std::ffi::{c_char, c_int, c_long, c_void};
use std::ptr;

/// Matches suite `MPACK_BUFFER_SIZE` (33) under frozen-link everything; 4096 otherwise.
#[cfg(all(feature = "full-suite-abi", mpack_frozen_link))]
pub const OWNED_BUFFER_CAPACITY: usize = 33;
#[cfg(not(all(feature = "full-suite-abi", mpack_frozen_link)))]
pub const OWNED_BUFFER_CAPACITY: usize = 4096;

/// Matches suite `MPACK_TRACKING_INITIAL_CAPACITY` under frozen-link / full-suite.
#[cfg(feature = "full-suite-abi")]
pub const TRACKING_INITIAL_CAPACITY: usize = 3;
#[cfg(not(feature = "full-suite-abi"))]
pub const TRACKING_INITIAL_CAPACITY: usize = 8;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn realloc(pointer: *mut c_void, size: usize) -> *mut c_void;
    fn free(pointer: *mut c_void);
    fn fopen(filename: *const c_char, mode: *const c_char) -> *mut c_void;
    fn fclose(file: *mut c_void) -> c_int;
    fn fread(data: *mut c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    fn fwrite(data: *const c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    fn fseek(file: *mut c_void, offset: c_long, whence: c_int) -> c_int;
    fn ftell(file: *mut c_void) -> c_long;
    fn ferror(file: *mut c_void) -> c_int;
}

#[cfg(feature = "full-suite-abi")]
unsafe extern "C" {
    fn test_malloc(size: usize) -> *mut c_void;
    fn test_free(pointer: *mut c_void);
    fn test_fopen(filename: *const c_char, mode: *const c_char) -> *mut c_void;
    fn test_fclose(file: *mut c_void) -> c_int;
    fn test_fread(data: *mut c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    fn test_fwrite(data: *const c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    fn test_fseek(file: *mut c_void, offset: c_long, whence: c_int) -> c_int;
    fn test_ftell(file: *mut c_void) -> c_long;
    fn test_ferror(file: *mut c_void) -> c_int;
}

/// Allocate with the ABI allocator (`test_malloc` under full-suite-abi).
pub unsafe fn suite_malloc(size: usize) -> *mut c_void {
    #[cfg(feature = "full-suite-abi")]
    {
        unsafe { test_malloc(size) }
    }
    #[cfg(not(feature = "full-suite-abi"))]
    {
        unsafe { malloc(size) }
    }
}

/// Free with the ABI allocator (`test_free` under full-suite-abi).
pub unsafe fn suite_free(pointer: *mut c_void) {
    #[cfg(feature = "full-suite-abi")]
    {
        unsafe { test_free(pointer) };
    }
    #[cfg(not(feature = "full-suite-abi"))]
    {
        unsafe { free(pointer) };
    }
}

/// Realloc with initialized-prefix copy under full-suite-abi (no `MPACK_REALLOC`).
pub unsafe fn suite_realloc(pointer: *mut c_void, used: usize, size: usize) -> *mut c_void {
    #[cfg(feature = "full-suite-abi")]
    {
        let replacement = unsafe { test_malloc(size) };
        if replacement.is_null() {
            return ptr::null_mut();
        }
        if !pointer.is_null() && used > 0 {
            unsafe {
                ptr::copy_nonoverlapping(pointer.cast::<u8>(), replacement.cast::<u8>(), used);
            }
        }
        if !pointer.is_null() {
            unsafe { test_free(pointer) };
        }
        replacement
    }
    #[cfg(not(feature = "full-suite-abi"))]
    {
        let _ = used;
        unsafe { realloc(pointer, size) }
    }
}

pub unsafe fn suite_fopen(filename: *const c_char, mode: *const c_char) -> *mut c_void {
    #[cfg(feature = "full-suite-abi")]
    {
        unsafe { test_fopen(filename, mode) }
    }
    #[cfg(not(feature = "full-suite-abi"))]
    {
        unsafe { fopen(filename, mode) }
    }
}

pub unsafe fn suite_fclose(file: *mut c_void) -> c_int {
    #[cfg(feature = "full-suite-abi")]
    {
        unsafe { test_fclose(file) }
    }
    #[cfg(not(feature = "full-suite-abi"))]
    {
        unsafe { fclose(file) }
    }
}

pub unsafe fn suite_fread(
    data: *mut c_void,
    size: usize,
    count: usize,
    file: *mut c_void,
) -> usize {
    #[cfg(feature = "full-suite-abi")]
    {
        unsafe { test_fread(data, size, count, file) }
    }
    #[cfg(not(feature = "full-suite-abi"))]
    {
        unsafe { fread(data, size, count, file) }
    }
}

pub unsafe fn suite_fwrite(
    data: *const c_void,
    size: usize,
    count: usize,
    file: *mut c_void,
) -> usize {
    #[cfg(feature = "full-suite-abi")]
    {
        unsafe { test_fwrite(data, size, count, file) }
    }
    #[cfg(not(feature = "full-suite-abi"))]
    {
        unsafe { fwrite(data, size, count, file) }
    }
}

pub unsafe fn suite_fseek(file: *mut c_void, offset: c_long, whence: c_int) -> c_int {
    #[cfg(feature = "full-suite-abi")]
    {
        unsafe { test_fseek(file, offset, whence) }
    }
    #[cfg(not(feature = "full-suite-abi"))]
    {
        unsafe { fseek(file, offset, whence) }
    }
}

pub unsafe fn suite_ftell(file: *mut c_void) -> c_long {
    #[cfg(feature = "full-suite-abi")]
    {
        unsafe { test_ftell(file) }
    }
    #[cfg(not(feature = "full-suite-abi"))]
    {
        unsafe { ftell(file) }
    }
}

pub unsafe fn suite_ferror(file: *mut c_void) -> c_int {
    #[cfg(feature = "full-suite-abi")]
    {
        unsafe { test_ferror(file) }
    }
    #[cfg(not(feature = "full-suite-abi"))]
    {
        unsafe { ferror(file) }
    }
}

/// Cargo-test / local shims. Omitted under `--cfg mpack_frozen_link` so the
/// frozen C suite owns these symbols at final executable link.
#[cfg(all(feature = "full-suite-abi", not(mpack_frozen_link)))]
mod suite_shims {
    use super::*;
    use std::alloc::{alloc_zeroed, Layout};
    use std::ffi::CStr;

    #[no_mangle]
    pub unsafe extern "C" fn mpack_break_hit(_message: *const c_char) {}

    #[no_mangle]
    pub unsafe extern "C" fn mpack_assert_fail(_message: *const c_char) {}

    #[no_mangle]
    pub unsafe extern "C" fn test_malloc(size: usize) -> *mut c_void {
        let layout = Layout::from_size_align(size.max(1), 8).unwrap();
        unsafe { alloc_zeroed(layout) }.cast()
    }

    #[no_mangle]
    pub unsafe extern "C" fn test_free(pointer: *mut c_void) {
        let _ = pointer;
    }

    #[no_mangle]
    pub unsafe extern "C" fn test_strlen(value: *const c_char) -> usize {
        if value.is_null() {
            return 0;
        }
        unsafe { CStr::from_ptr(value) }.to_bytes().len()
    }

    #[no_mangle]
    pub unsafe extern "C" fn test_fopen(
        filename: *const c_char,
        mode: *const c_char,
    ) -> *mut c_void {
        unsafe { fopen(filename, mode) }
    }

    #[no_mangle]
    pub unsafe extern "C" fn test_fclose(file: *mut c_void) -> c_int {
        unsafe { fclose(file) }
    }

    #[no_mangle]
    pub unsafe extern "C" fn test_fread(
        data: *mut c_void,
        size: usize,
        count: usize,
        file: *mut c_void,
    ) -> usize {
        unsafe { fread(data, size, count, file) }
    }

    #[no_mangle]
    pub unsafe extern "C" fn test_fwrite(
        data: *const c_void,
        size: usize,
        count: usize,
        file: *mut c_void,
    ) -> usize {
        unsafe { fwrite(data, size, count, file) }
    }

    #[no_mangle]
    pub unsafe extern "C" fn test_fseek(file: *mut c_void, offset: c_long, whence: c_int) -> c_int {
        unsafe { fseek(file, offset, whence) }
    }

    #[no_mangle]
    pub unsafe extern "C" fn test_ftell(file: *mut c_void) -> c_long {
        unsafe { ftell(file) }
    }

    #[no_mangle]
    pub unsafe extern "C" fn test_ferror(file: *mut c_void) -> c_int {
        unsafe { ferror(file) }
    }
}
