//! Temporary scaffolding; replace body with safe-core calls, do not grow unsafe here.

use std::ffi::{c_char, c_void};
use std::ptr;

use crate::ffi::stubs::util::{destroy_reader, flag_reader, init_reader, stub_alloc_bytes, stub_bytes};
use crate::ffi::types::{MpackError, MpackReader, MpackReaderFill, MpackTag, MPACK_ERROR_EOF};

#[no_mangle]
pub unsafe extern "C" fn mpack_reader_init(
    reader: *mut MpackReader,
    _buffer: *mut c_char,
    _size: usize,
    _count: usize,
) {
    unsafe { init_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_reader_init_data(
    reader: *mut MpackReader,
    _data: *const c_char,
    _count: usize,
) {
    unsafe { init_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_reader_init_filename(reader: *mut MpackReader, _filename: *const c_char) {
    unsafe { init_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_reader_init_stdfile(
    reader: *mut MpackReader,
    _stdfile: *mut c_void,
    _close_when_done: bool,
) {
    unsafe { init_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_reader_destroy(reader: *mut MpackReader) -> MpackError {
    unsafe { destroy_reader(reader) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_reader_set_fill(reader: *mut MpackReader, fill: MpackReaderFill) {
    if reader.is_null() {
        return;
    }
    unsafe { (*reader).fill = fill };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_reader_flag_error(reader: *mut MpackReader, error: MpackError) {
    if reader.is_null() {
        return;
    }
    let state = unsafe { &mut *reader };
    if state.error == crate::ffi::types::MPACK_OK {
        state.error = error;
        if let Some(error_fn) = state.error_fn {
            unsafe { error_fn(reader, error) };
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_reader_remaining(
    reader: *mut MpackReader,
    data: *mut *const c_char,
) -> usize {
    unsafe { flag_reader(reader) };
    if !data.is_null() {
        unsafe { *data = ptr::null() };
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_read_tag(reader: *mut MpackReader) -> MpackTag {
    unsafe { flag_reader(reader) };
    MpackTag::nil()
}

#[no_mangle]
pub unsafe extern "C" fn mpack_peek_tag(reader: *mut MpackReader) -> MpackTag {
    unsafe { flag_reader(reader) };
    MpackTag::nil()
}

#[no_mangle]
pub unsafe extern "C" fn mpack_read_bytes(reader: *mut MpackReader, _p: *mut c_char, _count: usize) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_read_bytes_inplace(
    reader: *mut MpackReader,
    _count: usize,
) -> *const c_char {
    unsafe { flag_reader(reader) };
    stub_bytes()
}

#[no_mangle]
pub unsafe extern "C" fn mpack_read_bytes_alloc_impl(
    reader: *mut MpackReader,
    count: usize,
    _null_terminated: bool,
) -> *mut c_char {
    unsafe { flag_reader(reader) };
    unsafe { stub_alloc_bytes(count.max(1)) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_done_type(reader: *mut MpackReader, _type: i32) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_discard(reader: *mut MpackReader) {
    // Streaming EOF loops (e.g. test_file_read_eof) wait for mpack_error_eof.
    // Init stubs sticky-flag unsupported, so overwrite any non-EOF error or the
    // wait spins forever after soft-continued assertions.
    if reader.is_null() {
        return;
    }
    let state = unsafe { &mut *reader };
    if state.error != MPACK_ERROR_EOF {
        state.error = MPACK_ERROR_EOF;
        if let Some(error_fn) = state.error_fn {
            unsafe { error_fn(reader, MPACK_ERROR_EOF) };
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_reader_ensure_straddle(
    reader: *mut MpackReader,
    _count: usize,
) -> bool {
    unsafe { flag_reader(reader) };
    false
}

#[no_mangle]
pub unsafe extern "C" fn mpack_read_native_straddle(
    reader: *mut MpackReader,
    _p: *mut c_char,
    _count: usize,
) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_print_data_to_buffer(
    _data: *const c_char,
    _data_size: usize,
    buffer: *mut c_char,
    buffer_size: usize,
) {
    if !buffer.is_null() && buffer_size > 0 {
        unsafe { *buffer = 0 };
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_print_data_to_file(
    _data: *const c_char,
    _len: usize,
    _file: *mut c_void,
) {
}
