//! C ABI types for the explicitly supported `embed-writer` configuration.

use std::ffi::{c_char, c_int, c_void};

use crate::common::Error;

pub type MpackError = c_int;

pub const MPACK_OK: MpackError = 0;
pub const MPACK_ERROR_IO: MpackError = 2;
pub const MPACK_ERROR_INVALID: MpackError = 3;
pub const MPACK_ERROR_UNSUPPORTED: MpackError = 4;
pub const MPACK_ERROR_TYPE: MpackError = 5;
pub const MPACK_ERROR_TOO_BIG: MpackError = 6;
pub const MPACK_ERROR_MEMORY: MpackError = 7;
pub const MPACK_ERROR_BUG: MpackError = 8;
pub const MPACK_ERROR_DATA: MpackError = 9;
pub const MPACK_ERROR_EOF: MpackError = 10;

pub type MpackWriterFlush = Option<unsafe extern "C" fn(*mut MpackWriter, *const c_char, usize)>;
pub type MpackWriterError = Option<unsafe extern "C" fn(*mut MpackWriter, MpackError)>;
pub type MpackWriterTeardown = Option<unsafe extern "C" fn(*mut MpackWriter)>;

/// `mpack_writer_t` under the upstream `embed-writer` configuration.
///
/// This layout intentionally excludes compatibility, tracking, allocator
/// reserve, and builder fields.
#[repr(C)]
pub struct MpackWriter {
    pub flush: MpackWriterFlush,
    pub error_fn: MpackWriterError,
    pub teardown: MpackWriterTeardown,
    pub context: *mut c_void,
    pub buffer: *mut c_char,
    pub position: *mut c_char,
    pub end: *mut c_char,
    pub error: MpackError,
}

impl MpackWriter {
    pub(crate) fn fixed_buffer(buffer: *mut c_char, size: usize) -> Self {
        let (end, error) = if buffer.is_null() {
            (buffer, MPACK_ERROR_BUG)
        } else {
            (buffer.wrapping_add(size), MPACK_OK)
        };

        Self {
            flush: None,
            error_fn: None,
            teardown: None,
            context: std::ptr::null_mut(),
            buffer,
            position: buffer,
            end,
            error,
        }
    }

    pub(crate) fn error_state(error: MpackError) -> Self {
        Self {
            flush: None,
            error_fn: None,
            teardown: None,
            context: std::ptr::null_mut(),
            buffer: std::ptr::null_mut(),
            position: std::ptr::null_mut(),
            end: std::ptr::null_mut(),
            error,
        }
    }
}

pub(crate) fn core_error_to_abi(error: Error) -> MpackError {
    match error {
        Error::Ok => MPACK_OK,
        Error::Io => MPACK_ERROR_IO,
        Error::Invalid => MPACK_ERROR_INVALID,
        Error::Unsupported => MPACK_ERROR_UNSUPPORTED,
        Error::Type => MPACK_ERROR_TYPE,
        Error::TooBig => MPACK_ERROR_TOO_BIG,
        Error::Memory => MPACK_ERROR_MEMORY,
        Error::Bug => MPACK_ERROR_BUG,
        Error::Data => MPACK_ERROR_DATA,
        Error::Eof => MPACK_ERROR_EOF,
    }
}
