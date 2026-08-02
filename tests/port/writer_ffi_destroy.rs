//! Writer destroy ordering regressions (`full-suite-abi`).
//!
//! Incomplete compounds must sticky-error via track cleanup before growable
//! teardown can hand a buffer to C.

#![cfg(feature = "full-suite-abi")]

use std::ffi::c_char;
use std::mem::MaybeUninit;
use std::ptr;

use mpack::ffi::force_track_init_fail_for_tests;
use mpack::ffi::types::{MpackWriter, MPACK_ERROR_BUG, MPACK_ERROR_MEMORY, MPACK_OK};
use mpack::ffi::writer::{
    mpack_start_array, mpack_write_nil, mpack_writer_destroy, mpack_writer_init_growable,
};

#[no_mangle]
pub unsafe extern "C" fn mpack_break_hit(_message: *const i8) {}

#[no_mangle]
pub unsafe extern "C" fn mpack_assert_fail(_message: *const i8) {}

#[no_mangle]
pub unsafe extern "C" fn test_malloc(size: usize) -> *mut std::ffi::c_void {
    let layout = std::alloc::Layout::from_size_align(size.max(1), 8).unwrap();
    unsafe { std::alloc::alloc_zeroed(layout) }.cast()
}

#[no_mangle]
pub unsafe extern "C" fn test_free(pointer: *mut std::ffi::c_void) {
    if pointer.is_null() {
        return;
    }
    // Growable teardown may free the private buffer through the suite hook
    // without knowing the exact Layout; leak is acceptable in this shim.
    let _ = pointer;
}

#[no_mangle]
pub unsafe extern "C" fn test_strlen(value: *const c_char) -> usize {
    if value.is_null() {
        return 0;
    }
    unsafe { std::ffi::CStr::from_ptr(value).to_bytes().len() }
}

#[test]
fn open_compound_growable_destroy_does_not_hand_off_buffer() {
    let mut writer = MaybeUninit::<MpackWriter>::uninit();
    let mut data: *mut c_char = ptr::null_mut();
    let mut size = 0_usize;
    unsafe {
        mpack_writer_init_growable(writer.as_mut_ptr(), &mut data, &mut size);
        let writer = writer.as_mut_ptr();
        mpack_start_array(writer, 1);
        mpack_write_nil(writer);
        // Leave the array unfinished so track_destroy flags bug before teardown.
        assert_eq!(mpack_writer_destroy(writer), MPACK_ERROR_BUG);
        assert!(data.is_null(), "incomplete growable must not hand off buffer");
        assert_eq!(size, 0);
        assert_eq!((*writer).error, MPACK_ERROR_BUG);
        assert_ne!((*writer).error, MPACK_OK);
    }
}

#[test]
fn growable_track_init_failure_keeps_buffer_and_destroy_is_safe() {
    force_track_init_fail_for_tests(true);
    let mut writer = MaybeUninit::<MpackWriter>::uninit();
    let mut data: *mut c_char = ptr::null_mut();
    let mut size = 0_usize;
    unsafe {
        mpack_writer_init_growable(writer.as_mut_ptr(), &mut data, &mut size);
        let writer = writer.as_mut_ptr();
        // C-aligned: sticky memory error, buffer retained (not dangling), teardown wired.
        assert_eq!((*writer).error, MPACK_ERROR_MEMORY);
        assert!(!(*writer).buffer.is_null(), "track fail must not free buffer early");
        assert!((*writer).teardown.is_some(), "teardown must stay wired for reclaim");
        assert!(data.is_null());
        assert_eq!(size, 0);
        assert_eq!(mpack_writer_destroy(writer), MPACK_ERROR_MEMORY);
        assert!(data.is_null(), "error path must not hand off buffer");
        assert_eq!(size, 0);
        assert!((*writer).buffer.is_null(), "teardown must clear buffer");
    }
}
