//! C ABI boundary for the fixed-buffer `embed-writer` slice.

use std::ffi::{c_char, c_uint, CStr};
use std::slice;

use crate::ffi::guard::catch_ffi_panic;
use crate::ffi::types::{
    core_error_to_abi, MpackError, MpackTag, MpackWriter, MPACK_ERROR_BUG, MPACK_OK,
};
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
    write_with_core(writer, |core| core.write_nil());
}

fn write_with_core(writer: *mut MpackWriter, write: impl FnOnce(&mut Writer<'_>)) {
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
    write(&mut core);

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

macro_rules! writer_operation {
    ($name:ident($argument:ident: $argument_type:ty) => $method:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(writer: *mut MpackWriter, $argument: $argument_type) {
            if writer.is_null() {
                return;
            }
            if catch_ffi_panic(|| write_with_core(writer, |core| core.$method($argument))).is_err() {
                flag_bug(writer);
            }
        }
    };
}

writer_operation!(mpack_write_bool(value: bool) => write_bool);
writer_operation!(mpack_write_u8(value: u8) => write_u8);
writer_operation!(mpack_write_u16(value: u16) => write_u16);
writer_operation!(mpack_write_u32(value: u32) => write_u32);
writer_operation!(mpack_write_u64(value: u64) => write_u64);
writer_operation!(mpack_write_i8(value: i8) => write_i8);
writer_operation!(mpack_write_i16(value: i16) => write_i16);
writer_operation!(mpack_write_i32(value: i32) => write_i32);
writer_operation!(mpack_write_i64(value: i64) => write_i64);
writer_operation!(mpack_write_float(value: f32) => write_f32);
writer_operation!(mpack_write_double(value: f64) => write_f64);
writer_operation!(mpack_write_raw_float(value: u32) => write_f32_bits);
writer_operation!(mpack_write_raw_double(value: u64) => write_f64_bits);

macro_rules! writer_count_operation {
    ($name:ident => $method:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(writer: *mut MpackWriter, count: c_uint) {
            if writer.is_null() {
                return;
            }
            if catch_ffi_panic(|| write_with_core(writer, |core| core.$method(count as usize))).is_err()
            {
                flag_bug(writer);
            }
        }
    };
}

writer_count_operation!(mpack_start_array => write_array_header);
writer_count_operation!(mpack_start_map => write_map_header);
writer_count_operation!(mpack_start_str => write_str_header);
writer_count_operation!(mpack_start_bin => write_bin_header);

/// Writes the MessagePack `true` marker.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_true(writer: *mut MpackWriter) {
    // SAFETY: This forwards the C writer contract to the bool implementation.
    unsafe { mpack_write_bool(writer, true) };
}

/// Writes the MessagePack `false` marker.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_false(writer: *mut MpackWriter) {
    // SAFETY: This forwards the C writer contract to the bool implementation.
    unsafe { mpack_write_bool(writer, false) };
}

/// Sets the sticky writer error without overwriting an earlier error.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_flag_error(writer: *mut MpackWriter, error: MpackError) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        // SAFETY: A non-null writer is required by the C API to be initialized
        // and uniquely writable.
        let state = unsafe { &mut *writer };
        if state.error == MPACK_OK {
            state.error = error;
        }
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

/// Configures a flush callback. Fixed-buffer flushing is not implemented yet;
/// the callback is retained for the ABI and future buffered-writer slice.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_set_flush(
    writer: *mut MpackWriter,
    flush: crate::ffi::types::MpackWriterFlush,
) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        // SAFETY: A non-null writer is required by the C API to be initialized
        // and uniquely writable.
        unsafe { (*writer).flush = flush };
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

/// Writes raw payload bytes to an active fixed-buffer writer.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_bytes(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: usize,
) {
    write_c_bytes(writer, data, count, |core, bytes| core.write_bytes(bytes));
}

/// Writes a MessagePack string header followed by its raw bytes.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_str(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: c_uint,
) {
    write_c_bytes(writer, data, count as usize, |core, bytes| core.write_str(bytes));
}

/// Writes a MessagePack binary header followed by its raw bytes.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_bin(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: c_uint,
) {
    write_c_bytes(writer, data, count as usize, |core, bytes| core.write_bin(bytes));
}

/// Writes raw object bytes. With write tracking disabled this is equivalent to
/// `mpack_write_bytes`.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_object_bytes(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: usize,
) {
    write_c_bytes(writer, data, count, |core, bytes| core.write_bytes(bytes));
}

/// Writes an MPack object header represented by `mpack_tag_t`.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_tag(writer: *mut MpackWriter, tag: MpackTag) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        write_with_core(writer, |core| match tag.type_ {
            1 => core.write_nil(),
            2 => core.write_bool((tag.value & 0xff) != 0),
            3 => core.write_f32_bits(tag.value as u32),
            4 => core.write_f64_bits(tag.value),
            5 => core.write_i64(tag.value as i64),
            6 => core.write_u64(tag.value),
            7 => core.write_str_header(tag.value as usize),
            8 => core.write_bin_header(tag.value as usize),
            9 => core.write_array_header(tag.value as usize),
            10 => core.write_map_header(tag.value as usize),
            _ => flag_bug(writer),
        });
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

/// Writes a null-terminated C string as a MessagePack string.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_cstr(writer: *mut MpackWriter, cstr: *const c_char) {
    if writer.is_null() {
        return;
    }
    if cstr.is_null() {
        flag_bug(writer);
        return;
    }
    if catch_ffi_panic(|| {
        // SAFETY: The C API requires `cstr` to point to a valid NUL-terminated
        // byte string for the duration of this call.
        let bytes = unsafe { CStr::from_ptr(cstr).to_bytes() };
        write_with_core(writer, |core| core.write_str(bytes));
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

/// Writes a C string, or a nil marker when the pointer is null.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_cstr_or_nil(writer: *mut MpackWriter, cstr: *const c_char) {
    if cstr.is_null() {
        // SAFETY: This forwards the C writer contract to the nil implementation.
        unsafe { mpack_write_nil(writer) };
    } else {
        // SAFETY: The non-null C string contract is forwarded to the cstr API.
        unsafe { mpack_write_cstr(writer, cstr) };
    }
}

/// Validates UTF-8 before writing a MessagePack string.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_utf8(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: c_uint,
) {
    if writer.is_null() {
        return;
    }
    if data.is_null() && count != 0 {
        flag_bug(writer);
        return;
    }
    if catch_ffi_panic(|| {
        let bytes = if count == 0 {
            &[]
        } else {
            // SAFETY: The C API requires `data` to reference `count` readable bytes.
            unsafe { slice::from_raw_parts(data.cast::<u8>(), count as usize) }
        };
        if std::str::from_utf8(bytes).is_ok() {
            write_with_core(writer, |core| core.write_str(bytes));
        } else {
            // SAFETY: `writer` is non-null and initialized by the C contract.
            unsafe { mpack_writer_flag_error(writer, crate::ffi::types::MPACK_ERROR_INVALID) };
        }
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

/// Validates and writes a null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_utf8_cstr(writer: *mut MpackWriter, cstr: *const c_char) {
    if writer.is_null() {
        return;
    }
    if cstr.is_null() {
        flag_bug(writer);
        return;
    }
    // SAFETY: The non-null C string contract is forwarded to the length form.
    let bytes = unsafe { CStr::from_ptr(cstr).to_bytes() };
    // SAFETY: `bytes` remains borrowed from `cstr` for this immediate call.
    unsafe { mpack_write_utf8(writer, cstr, bytes.len() as c_uint) };
}

/// Writes a null-terminated UTF-8 string, or nil for a null pointer.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_utf8_cstr_or_nil(
    writer: *mut MpackWriter,
    cstr: *const c_char,
) {
    if cstr.is_null() {
        // SAFETY: This forwards the C writer contract to the nil implementation.
        unsafe { mpack_write_nil(writer) };
    } else {
        // SAFETY: The non-null C string contract is forwarded to the UTF-8 API.
        unsafe { mpack_write_utf8_cstr(writer, cstr) };
    }
}

/// Invokes the configured flush callback with buffered bytes and resets the
/// fixed-buffer position on success.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_flush_message(writer: *mut MpackWriter) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        // SAFETY: The C API requires an initialized, uniquely writable writer.
        let state = unsafe { &mut *writer };
        if state.error != MPACK_OK || state.position == state.buffer {
            return;
        }
        let Some(flush) = state.flush else {
            return;
        };
        let used = state.position as usize - state.buffer as usize;
        // SAFETY: The callback is supplied by the C caller and the buffer range
        // is the writer's live fixed allocation.
        unsafe { flush(writer, state.buffer, used) };
        state.position = state.buffer;
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

fn write_c_bytes(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: usize,
    write: impl FnOnce(&mut Writer<'_>, &[u8]),
) {
    if writer.is_null() {
        return;
    }
    if data.is_null() && count != 0 {
        flag_bug(writer);
        return;
    }

    if catch_ffi_panic(|| {
        let bytes = if count == 0 {
            &[]
        } else {
            // SAFETY: The C API requires a non-null data pointer to reference
            // `count` readable bytes. The null/zero case is handled above.
            unsafe { slice::from_raw_parts(data.cast::<u8>(), count) }
        };
        write_with_core(writer, |core| write(core, bytes));
    })
    .is_err()
    {
        flag_bug(writer);
    }
}
