//! C ABI boundary for the fixed-buffer `embed-writer` slice.

use std::ffi::c_char;
use std::slice;

use crate::ffi::guard::catch_ffi_panic;
use crate::ffi::types::{core_error_to_abi, MpackError, MpackWriter, MPACK_ERROR_BUG, MPACK_OK};
use crate::writer::Writer;

/// Initializes a fixed-buffer MPack writer.
///
/// # Safety
///
/// `writer` must be null or point to writable storage for one
/// `mpack_writer_t`. A non-null `buffer` must be writable for `size` bytes.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_init(
    writer: *mut MpackWriter,
    buffer: *mut c_char,
    size: usize,
) {
    if writer.is_null() {
        return;
    }

    if catch_ffi_panic(|| {
        let initialized = MpackWriter::fixed_buffer(buffer, size);

        // SAFETY: The C API requires `writer` to point to writable storage for
        // one `mpack_writer_t`. The null case was handled above.
        unsafe {
            writer.write(initialized);
        }
    })
    .is_err()
    {
        initialize_as_bug(writer);
    }
}

/// Writes one MessagePack nil marker.
///
/// # Safety
///
/// `writer` must be null or point to a live, uniquely writable
/// `mpack_writer_t` initialized by `mpack_writer_init`.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_nil(writer: *mut MpackWriter) {
    if writer.is_null() {
        return;
    }

    if catch_ffi_panic(|| write_nil_impl(writer)).is_err() {
        flag_bug(writer);
    }
}

/// Finishes a fixed-buffer writer and returns its sticky error.
///
/// # Safety
///
/// `writer` must be null or point to a live `mpack_writer_t` initialized by
/// `mpack_writer_init`.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_destroy(writer: *mut MpackWriter) -> MpackError {
    if writer.is_null() {
        return MPACK_ERROR_BUG;
    }

    match catch_ffi_panic(|| {
        // SAFETY: The C API requires `writer` to point to an initialized
        // `mpack_writer_t`. The null case was handled above.
        unsafe { (*writer).error }
    }) {
        Ok(error) => error,
        Err(_) => {
            flag_bug(writer);
            MPACK_ERROR_BUG
        }
    }
}

fn write_nil_impl(writer: *mut MpackWriter) {
    // SAFETY: The caller must provide a live, uniquely writable
    // `mpack_writer_t` for every mutating MPack writer operation.
    let state = unsafe { &mut *writer };

    if state.error != MPACK_OK {
        return;
    }

    if state.buffer.is_null() || state.position.is_null() || state.end.is_null() {
        state.error = MPACK_ERROR_BUG;
        return;
    }

    let buffer_address = state.buffer as usize;
    let position_address = state.position as usize;
    let end_address = state.end as usize;
    if position_address < buffer_address || position_address > end_address {
        state.error = MPACK_ERROR_BUG;
        return;
    }

    let remaining = end_address - position_address;
    if remaining > isize::MAX as usize {
        state.error = MPACK_ERROR_BUG;
        return;
    }

    // SAFETY: The C caller guarantees that buffer..end is one live writable
    // allocation. The address checks above constrain position to that range,
    // and `remaining` is valid for Rust slice construction.
    let output = unsafe { slice::from_raw_parts_mut(state.position.cast::<u8>(), remaining) };
    let mut core = Writer::new(output);
    core.write_nil();

    state.error = core_error_to_abi(core.error());
    state.position = state.position.wrapping_add(core.used());
}

fn initialize_as_bug(writer: *mut MpackWriter) {
    if writer.is_null() {
        return;
    }

    // SAFETY: This is the initialization panic fallback. The non-null C
    // pointer is required to reference writable `mpack_writer_t` storage.
    unsafe {
        writer.write(MpackWriter::error_state(MPACK_ERROR_BUG));
    }
}

fn flag_bug(writer: *mut MpackWriter) {
    if writer.is_null() {
        return;
    }

    // SAFETY: Callers use this only after the C contract established an
    // initialized, writable `mpack_writer_t`.
    unsafe {
        (*writer).error = MPACK_ERROR_BUG;
    }
}
