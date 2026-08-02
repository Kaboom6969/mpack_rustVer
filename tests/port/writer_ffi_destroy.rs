//! Writer destroy ordering regressions (`full-suite-abi`).
//!
//! Incomplete compounds must sticky-error via track cleanup before growable
//! teardown can hand a buffer to C.

#![cfg(feature = "full-suite-abi")]

use std::ffi::c_char;
use std::mem::MaybeUninit;
use std::ptr;

use mpack::ffi::{with_forced_track_init_fail, with_track_test_serial};
use mpack::ffi::types::{MpackWriter, MPACK_ERROR_BUG, MPACK_ERROR_MEMORY, MPACK_OK};
use mpack::ffi::writer::{
    mpack_start_array, mpack_write_nil, mpack_write_object_bytes, mpack_writer_destroy,
    mpack_writer_init_growable, mpack_writer_init_stdfile, mpack_writer_track_pop,
};

unsafe extern "C" {
    fn fopen(filename: *const c_char, mode: *const c_char) -> *mut std::ffi::c_void;
    fn fwrite(
        data: *const std::ffi::c_void,
        size: usize,
        count: usize,
        file: *mut std::ffi::c_void,
    ) -> usize;
    fn fclose(file: *mut std::ffi::c_void) -> i32;
    fn test_free(pointer: *mut std::ffi::c_void);
}

#[test]
fn open_compound_growable_destroy_does_not_hand_off_buffer() {
    with_track_test_serial(|| {
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
    });
}

#[test]
fn growable_track_init_failure_keeps_buffer_and_destroy_is_safe() {
    with_forced_track_init_fail(|| {
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
    });
}

#[test]
fn object_bytes_tracks_element_inside_open_compound() {
    with_track_test_serial(|| {
        let mut writer = MaybeUninit::<MpackWriter>::uninit();
        let mut data: *mut c_char = ptr::null_mut();
        let mut size = 0_usize;
        let nil = [0xc0_u8];
        unsafe {
            mpack_writer_init_growable(writer.as_mut_ptr(), &mut data, &mut size);
            let writer = writer.as_mut_ptr();
            mpack_start_array(writer, 1);
            mpack_write_object_bytes(writer, nil.as_ptr().cast(), nil.len());
            mpack_writer_track_pop(writer, 9);
            assert_eq!(mpack_writer_destroy(writer), MPACK_OK);
            assert!(!data.is_null());
            assert_eq!(size, 2);
            test_free(data.cast());
        }
    });
}

#[test]
fn object_bytes_extra_element_in_empty_array_flags_bug() {
    with_track_test_serial(|| {
        let mut writer = MaybeUninit::<MpackWriter>::uninit();
        let mut data: *mut c_char = ptr::null_mut();
        let mut size = 0_usize;
        let nil = [0xc0_u8];
        unsafe {
            mpack_writer_init_growable(writer.as_mut_ptr(), &mut data, &mut size);
            let writer = writer.as_mut_ptr();
            mpack_start_array(writer, 0);
            mpack_write_object_bytes(writer, nil.as_ptr().cast(), nil.len());
            assert_eq!((*writer).error, MPACK_ERROR_BUG);
            mpack_writer_track_pop(writer, 9);
            assert_eq!(mpack_writer_destroy(writer), MPACK_ERROR_BUG);
            assert!(data.is_null());
        }
    });
}

fn open_temp_file() -> *mut std::ffi::c_void {
    let path = c"mpack_writer_track_init_fail_port.tmp";
    let mode = c"wb+";
    unsafe { fopen(path.as_ptr(), mode.as_ptr()) }
}

#[test]
fn file_track_init_failure_close_true_keeps_buffer_and_destroy_is_safe() {
    with_forced_track_init_fail(|| {
        let file = open_temp_file();
        assert!(!file.is_null());
        let mut writer = MaybeUninit::<MpackWriter>::uninit();
        unsafe {
            mpack_writer_init_stdfile(writer.as_mut_ptr(), file, true);
            let writer = writer.as_mut_ptr();
            assert_eq!((*writer).error, MPACK_ERROR_MEMORY);
            assert!(!(*writer).buffer.is_null());
            assert!((*writer).teardown.is_some());
            assert_eq!(mpack_writer_destroy(writer), MPACK_ERROR_MEMORY);
            assert!((*writer).buffer.is_null());
        }
    });
}

#[test]
fn file_track_init_failure_close_false_leaves_file_open() {
    with_forced_track_init_fail(|| {
        let file = open_temp_file();
        assert!(!file.is_null());
        let mut writer = MaybeUninit::<MpackWriter>::uninit();
        unsafe {
            mpack_writer_init_stdfile(writer.as_mut_ptr(), file, false);
            let writer = writer.as_mut_ptr();
            assert_eq!((*writer).error, MPACK_ERROR_MEMORY);
            assert!(!(*writer).buffer.is_null());
            assert_eq!(mpack_writer_destroy(writer), MPACK_ERROR_MEMORY);
            let byte: u8 = b'x';
            assert_eq!(fwrite((&byte as *const u8).cast(), 1, 1, file), 1);
            assert_eq!(fclose(file), 0);
        }
    });
}
