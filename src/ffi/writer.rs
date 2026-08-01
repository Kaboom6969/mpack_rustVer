//! C ABI boundary for the fixed-buffer `embed-writer` slice.

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_uint, c_void, CStr};
use std::ptr;
use std::slice;
use std::sync::{Mutex, OnceLock};

use crate::common::Error;
use crate::ffi::guard::catch_ffi_panic;
use crate::ffi::types::{
    core_error_to_abi, MpackError, MpackTag, MpackWriter, MPACK_ERROR_BUG, MPACK_OK,
};
use crate::writer::Writer;

const OWNED_BUFFER_CAPACITY: usize = 4096;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn realloc(pointer: *mut c_void, size: usize) -> *mut c_void;
    fn free(pointer: *mut c_void);
    fn fopen(filename: *const c_char, mode: *const c_char) -> *mut c_void;
    fn fwrite(data: *const c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    fn fclose(file: *mut c_void) -> c_int;
}

struct GrowableContext {
    target_data: *mut *mut c_char,
    target_size: *mut usize,
}

struct FileContext {
    file: *mut c_void,
    close_when_done: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BuildKind {
    Array,
    Map,
}

struct FfiBuildFrame {
    kind: BuildKind,
    start: usize,
    elements: usize,
    known_compounds: Vec<usize>,
}

fn ffi_builders() -> &'static Mutex<HashMap<usize, Vec<FfiBuildFrame>>> {
    static BUILDERS: OnceLock<Mutex<HashMap<usize, Vec<FfiBuildFrame>>>> = OnceLock::new();
    BUILDERS.get_or_init(|| Mutex::new(HashMap::new()))
}

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
        clear_builder_state(writer);
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

/// Initializes a writer directly in an error state.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_init_error(writer: *mut MpackWriter, error: MpackError) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        // SAFETY: The non-null destination points to writable writer storage.
        unsafe { writer.write(MpackWriter::error_state(error)) };
    })
    .is_err()
    {
        initialize_as_bug(writer);
    }
}

/// Initializes an allocator-backed growable writer.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_init_growable(
    writer: *mut MpackWriter,
    target_data: *mut *mut c_char,
    target_size: *mut usize,
) {
    if writer.is_null() {
        return;
    }
    if target_data.is_null() || target_size.is_null() {
        initialize_as_bug(writer);
        return;
    }
    if catch_ffi_panic(|| {
        // SAFETY: Both result pointers are writable by the C contract.
        unsafe {
            target_data.write(ptr::null_mut());
            target_size.write(0);
        }
        // SAFETY: C malloc returns suitably aligned storage or null.
        let buffer = unsafe { malloc(OWNED_BUFFER_CAPACITY).cast::<c_char>() };
        if buffer.is_null() {
            // SAFETY: `writer` is writable and non-null.
            unsafe { writer.write(MpackWriter::error_state(crate::ffi::types::MPACK_ERROR_MEMORY)) };
            return;
        }
        let context = Box::new(GrowableContext {
            target_data,
            target_size,
        });
        let mut state = MpackWriter::fixed_buffer(buffer, OWNED_BUFFER_CAPACITY);
        state.context = Box::into_raw(context).cast::<c_void>();
        state.flush = Some(growable_flush);
        state.teardown = Some(growable_teardown);
        // SAFETY: `writer` points to writable C writer storage.
        unsafe { writer.write(state) };
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
        if builder_is_open(writer) {
            flag_error_impl(writer, MPACK_ERROR_BUG);
        }
        // SAFETY: The C API requires `writer` to point to an initialized
        // `mpack_writer_t`. The null case was handled above.
        let state = unsafe { &mut *writer };
        if state.error == MPACK_OK && state.position != state.buffer {
            if let Some(flush) = state.flush.take() {
                let used = state.position as usize - state.buffer as usize;
                unsafe { flush(writer, state.buffer, used) };
            }
        }
        if let Some(teardown) = state.teardown.take() {
            unsafe { teardown(writer) };
        }
        clear_builder_state(writer);
        state.error
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

fn write_with_core(writer: *mut MpackWriter, mut write: impl FnMut(&mut Writer<'_>)) {
    builder_record_value(writer);
    loop {
        // SAFETY: The caller must provide a live, uniquely writable writer.
        let state = unsafe { &mut *writer };
        if state.error != MPACK_OK {
            return;
        }
        let Some(remaining) = writer_remaining(state) else {
            flag_error_impl(writer, MPACK_ERROR_BUG);
            return;
        };

        // SAFETY: `writer_remaining` validated the live position..end range.
        let output =
            unsafe { slice::from_raw_parts_mut(state.position.cast::<u8>(), remaining) };
        let mut core = Writer::new(output);
        write(&mut core);
        state.position = state.position.wrapping_add(core.used());

        let error = core_error_to_abi(core.error());
        if error == MPACK_OK {
            return;
        }
        if error != crate::ffi::types::MPACK_ERROR_TOO_BIG
            || core.used() != 0
            || state.flush.is_none()
            || !flush_buffer(writer)
        {
            flag_error_impl(writer, error);
            return;
        }
    }
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

writer_count_operation!(mpack_start_str => write_str_header);
writer_count_operation!(mpack_start_bin => write_bin_header);

#[no_mangle]
pub unsafe extern "C" fn mpack_start_array(writer: *mut MpackWriter, count: c_uint) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        write_with_core(writer, |core| core.write_array_header(count as usize));
        builder_start_known(writer, count as usize);
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_start_map(writer: *mut MpackWriter, count: c_uint) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        write_with_core(writer, |core| core.write_map_header(count as usize));
        builder_start_known(writer, (count as usize).saturating_mul(2));
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

/// Opens an extension payload.
#[no_mangle]
pub unsafe extern "C" fn mpack_start_ext(
    writer: *mut MpackWriter,
    ext_type: i8,
    count: c_uint,
) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        write_with_core(writer, |core| {
            core.write_ext_header(ext_type, count as usize)
        });
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

/// Writes a complete extension value.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_ext(
    writer: *mut MpackWriter,
    ext_type: i8,
    data: *const c_char,
    count: c_uint,
) {
    write_c_bytes(writer, data, count as usize, |writer, bytes| {
        write_with_core(writer, |core| {
            core.write_ext_header(ext_type, bytes.len())
        });
        write_raw_buffered(writer, bytes);
    });
}

/// Writes a timestamp using MessagePack's 32/64/96-bit canonical forms.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_timestamp(
    writer: *mut MpackWriter,
    seconds: i64,
    nanoseconds: u32,
) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        write_with_core(writer, |core| {
            core.write_timestamp(seconds, nanoseconds)
        });
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

/// Starts an automatic-size array builder.
#[no_mangle]
pub unsafe extern "C" fn mpack_build_array(writer: *mut MpackWriter) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| builder_start(writer, BuildKind::Array)).is_err() {
        flag_bug(writer);
    }
}

/// Starts an automatic-size map builder.
#[no_mangle]
pub unsafe extern "C" fn mpack_build_map(writer: *mut MpackWriter) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| builder_start(writer, BuildKind::Map)).is_err() {
        flag_bug(writer);
    }
}

/// Completes an automatic-size array builder.
#[no_mangle]
pub unsafe extern "C" fn mpack_complete_array(writer: *mut MpackWriter) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| builder_complete(writer, BuildKind::Array)).is_err() {
        flag_bug(writer);
    }
}

/// Completes an automatic-size map builder.
#[no_mangle]
pub unsafe extern "C" fn mpack_complete_map(writer: *mut MpackWriter) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| builder_complete(writer, BuildKind::Map)).is_err() {
        flag_bug(writer);
    }
}

/// Tracking ABI hook. The configuration-neutral cdylib cannot infer whether
/// the C translation unit enabled tracking, so these hooks intentionally keep
/// layout/link compatibility while safe-core tracking remains opt-in.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_track_push(
    _writer: *mut MpackWriter,
    _type: c_int,
    _count: u32,
) {
}

#[no_mangle]
pub unsafe extern "C" fn mpack_writer_track_push_builder(
    _writer: *mut MpackWriter,
    _type: c_int,
) {
}

#[no_mangle]
pub unsafe extern "C" fn mpack_writer_track_pop(
    _writer: *mut MpackWriter,
    _type: c_int,
) {
}

#[no_mangle]
pub unsafe extern "C" fn mpack_writer_track_pop_builder(
    _writer: *mut MpackWriter,
    _type: c_int,
) {
}

#[no_mangle]
pub unsafe extern "C" fn mpack_writer_track_bytes(_writer: *mut MpackWriter, _count: usize) {}

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
        flag_error_impl(writer, error);
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

/// Configures the sticky-error callback.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_set_error_handler(
    writer: *mut MpackWriter,
    error_fn: crate::ffi::types::MpackWriterError,
) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        // SAFETY: The C API requires a live, uniquely writable writer.
        unsafe { (*writer).error_fn = error_fn };
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

/// Configures the teardown callback.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_set_teardown(
    writer: *mut MpackWriter,
    teardown: crate::ffi::types::MpackWriterTeardown,
) {
    if writer.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        // SAFETY: The C API requires a live, uniquely writable writer.
        unsafe { (*writer).teardown = teardown };
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

/// Initializes a buffered writer over an existing C `FILE*`.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_init_stdfile(
    writer: *mut MpackWriter,
    file: *mut c_void,
    close_when_done: bool,
) {
    if writer.is_null() {
        return;
    }
    if file.is_null() {
        initialize_as_bug(writer);
        return;
    }
    if catch_ffi_panic(|| init_file_writer(writer, file, close_when_done)).is_err() {
        initialize_as_bug(writer);
    }
}

/// Opens a filename and initializes a buffered file writer.
#[no_mangle]
pub unsafe extern "C" fn mpack_writer_init_filename(
    writer: *mut MpackWriter,
    filename: *const c_char,
) {
    if writer.is_null() {
        return;
    }
    if filename.is_null() {
        initialize_as_bug(writer);
        return;
    }
    if catch_ffi_panic(|| {
        // SAFETY: `filename` is a NUL-terminated C string and the mode is static.
        let file = unsafe { fopen(filename, c"wb".as_ptr()) };
        if file.is_null() {
            // SAFETY: The writer destination is writable.
            unsafe { writer.write(MpackWriter::error_state(crate::ffi::types::MPACK_ERROR_IO)) };
        } else {
            init_file_writer(writer, file, true);
        }
    })
    .is_err()
    {
        initialize_as_bug(writer);
    }
}

/// Writes raw payload bytes to an active fixed-buffer writer.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_bytes(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: usize,
) {
    write_c_bytes(writer, data, count, write_raw_buffered);
}

/// Writes a MessagePack string header followed by its raw bytes.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_str(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: c_uint,
) {
    write_c_bytes(writer, data, count as usize, |writer, bytes| {
        write_with_core(writer, |core| core.write_str_header(bytes.len()));
        write_raw_buffered(writer, bytes);
    });
}

/// Writes a MessagePack binary header followed by its raw bytes.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_bin(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: c_uint,
) {
    write_c_bytes(writer, data, count as usize, |writer, bytes| {
        write_with_core(writer, |core| core.write_bin_header(bytes.len()));
        write_raw_buffered(writer, bytes);
    });
}

/// Writes raw object bytes. With write tracking disabled this is equivalent to
/// `mpack_write_bytes`.
#[no_mangle]
pub unsafe extern "C" fn mpack_write_object_bytes(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: usize,
) {
    write_c_bytes(writer, data, count, write_raw_buffered);
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
            3 => core.write_i64(tag.value as i64),
            4 => core.write_u64(tag.value),
            5 => core.write_f32_bits(tag.value as u32),
            6 => core.write_f64_bits(tag.value),
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
        write_with_core(writer, |core| core.write_str_header(bytes.len()));
        write_raw_buffered(writer, bytes);
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
            write_with_core(writer, |core| core.write_str_header(bytes.len()));
            write_raw_buffered(writer, bytes);
        } else {
            flag_error_impl(writer, crate::ffi::types::MPACK_ERROR_INVALID);
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
        if state.error != MPACK_OK {
            return;
        }
        let Some(flush) = state.flush else {
            flag_error_impl(writer, MPACK_ERROR_BUG);
            return;
        };
        let used = state.position as usize - state.buffer as usize;
        if used == 0 {
            return;
        }
        state.position = state.buffer;
        // SAFETY: The callback is supplied by the C caller and the buffer range
        // is the writer's live fixed allocation.
        unsafe { flush(writer, state.buffer, used) };
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
    write: impl FnOnce(*mut MpackWriter, &[u8]),
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
        write(writer, bytes);
    })
    .is_err()
    {
        flag_bug(writer);
    }
}

fn writer_remaining(state: &MpackWriter) -> Option<usize> {
    if state.buffer.is_null() || state.position.is_null() || state.end.is_null() {
        return None;
    }
    let buffer = state.buffer as usize;
    let position = state.position as usize;
    let end = state.end as usize;
    if position < buffer || position > end {
        return None;
    }
    let remaining = end - position;
    (remaining <= isize::MAX as usize).then_some(remaining)
}

fn flush_buffer(writer: *mut MpackWriter) -> bool {
    // SAFETY: Callers provide a live, uniquely writable writer.
    let state = unsafe { &mut *writer };
    let Some(flush) = state.flush else {
        return false;
    };
    let used = state.position as usize - state.buffer as usize;
    state.position = state.buffer;
    // SAFETY: The callback and buffer range are owned by the C caller.
    unsafe { flush(writer, state.buffer, used) };
    // SAFETY: The callback has returned, so the writer can be inspected again.
    let state = unsafe { &*writer };
    state.error == MPACK_OK && writer_remaining(state).is_some()
}

fn write_raw_buffered(writer: *mut MpackWriter, bytes: &[u8]) {
    // SAFETY: The caller established a live writer for this operation.
    let state = unsafe { &mut *writer };
    if state.error != MPACK_OK || bytes.is_empty() {
        return;
    }
    let Some(remaining) = writer_remaining(state) else {
        flag_error_impl(writer, MPACK_ERROR_BUG);
        return;
    };
    if bytes.len() <= remaining {
        // SAFETY: `remaining` proves the destination has enough writable bytes.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), state.position.cast::<u8>(), bytes.len());
            state.position = state.position.add(bytes.len());
        }
        return;
    }
    let Some(flush) = state.flush else {
        flag_error_impl(writer, crate::ffi::types::MPACK_ERROR_TOO_BIG);
        return;
    };
    if !flush_buffer(writer) {
        return;
    }

    // SAFETY: The flush callback has returned and may have replaced the buffer.
    let state = unsafe { &mut *writer };
    let Some(remaining) = writer_remaining(state) else {
        flag_error_impl(writer, MPACK_ERROR_BUG);
        return;
    };
    if bytes.len() > remaining {
        // SAFETY: MPack flush callbacks accept external readable data.
        unsafe { flush(writer, bytes.as_ptr().cast::<c_char>(), bytes.len()) };
    } else {
        // SAFETY: The refreshed writer buffer has enough room.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), state.position.cast::<u8>(), bytes.len());
            state.position = state.position.add(bytes.len());
        }
    }
}

fn flag_error_impl(writer: *mut MpackWriter, error: MpackError) {
    // SAFETY: All callers have established that `writer` points to a live,
    // uniquely writable C writer.
    let state = unsafe { &mut *writer };
    if state.error != MPACK_OK {
        return;
    }
    state.error = error;
    if let Some(error_fn) = state.error_fn {
        // SAFETY: The callback and writer pointer are supplied by the C caller.
        unsafe { error_fn(writer, error) };
    }
}

fn clear_builder_state(writer: *mut MpackWriter) {
    if let Ok(mut builders) = ffi_builders().lock() {
        builders.remove(&(writer as usize));
    }
}

fn builder_is_open(writer: *mut MpackWriter) -> bool {
    ffi_builders()
        .lock()
        .map(|builders| builders.get(&(writer as usize)).is_some_and(|frames| !frames.is_empty()))
        .unwrap_or(true)
}

fn builder_record_value(writer: *mut MpackWriter) {
    let Ok(mut builders) = ffi_builders().lock() else {
        flag_error_impl(writer, MPACK_ERROR_BUG);
        return;
    };
    let Some(frame) = builders
        .get_mut(&(writer as usize))
        .and_then(|frames| frames.last_mut())
    else {
        return;
    };
    while frame.known_compounds.last() == Some(&0) {
        frame.known_compounds.pop();
    }
    if let Some(left) = frame.known_compounds.last_mut() {
        *left -= 1;
    } else {
        frame.elements = frame.elements.saturating_add(1);
    }
}

fn builder_start_known(writer: *mut MpackWriter, elements: usize) {
    if elements == 0 {
        return;
    }
    let Ok(mut builders) = ffi_builders().lock() else {
        flag_error_impl(writer, MPACK_ERROR_BUG);
        return;
    };
    if let Some(frame) = builders
        .get_mut(&(writer as usize))
        .and_then(|frames| frames.last_mut())
    {
        frame.known_compounds.push(elements);
    }
}

fn builder_start(writer: *mut MpackWriter, kind: BuildKind) {
    // A nested builder is one value in its parent builder.
    builder_record_value(writer);
    // SAFETY: Exported callers provide a live writer.
    let state = unsafe { &mut *writer };
    if state.error != MPACK_OK {
        return;
    }
    let Some(remaining) = writer_remaining(state) else {
        flag_error_impl(writer, MPACK_ERROR_BUG);
        return;
    };
    if remaining < 5 {
        // Builders need a contiguous placeholder. Growable writers can expand
        // through the regular flush path before the placeholder is installed.
        if !flush_buffer(writer) {
            flag_error_impl(writer, crate::ffi::types::MPACK_ERROR_TOO_BIG);
            return;
        }
    }
    // SAFETY: Re-read after a possible intrusive growable flush.
    let state = unsafe { &mut *writer };
    let Some(remaining) = writer_remaining(state) else {
        flag_error_impl(writer, MPACK_ERROR_BUG);
        return;
    };
    if remaining < 5 {
        flag_error_impl(writer, crate::ffi::types::MPACK_ERROR_TOO_BIG);
        return;
    }
    let start = state.position as usize - state.buffer as usize;
    // SAFETY: The remaining range contains at least five writable bytes.
    unsafe {
        ptr::write_bytes(state.position.cast::<u8>(), 0, 5);
        state.position = state.position.add(5);
    }
    let Ok(mut builders) = ffi_builders().lock() else {
        flag_error_impl(writer, MPACK_ERROR_BUG);
        return;
    };
    builders
        .entry(writer as usize)
        .or_default()
        .push(FfiBuildFrame {
            kind,
            start,
            elements: 0,
            known_compounds: Vec::new(),
        });
}

fn builder_complete(writer: *mut MpackWriter, kind: BuildKind) {
    let frame = {
        let Ok(mut builders) = ffi_builders().lock() else {
            flag_error_impl(writer, MPACK_ERROR_BUG);
            return;
        };
        let Some(frames) = builders.get_mut(&(writer as usize)) else {
            drop(builders);
            flag_error_impl(writer, MPACK_ERROR_BUG);
            return;
        };
        let Some(frame) = frames.pop() else {
            drop(builders);
            flag_error_impl(writer, MPACK_ERROR_BUG);
            return;
        };
        if frames.is_empty() {
            builders.remove(&(writer as usize));
        }
        frame
    };
    if frame.kind != kind
        || frame.known_compounds.iter().any(|&left| left != 0)
        || (kind == BuildKind::Map && frame.elements % 2 != 0)
    {
        flag_error_impl(writer, MPACK_ERROR_BUG);
        return;
    }
    let count = if kind == BuildKind::Map {
        frame.elements / 2
    } else {
        frame.elements
    };
    if count > u32::MAX as usize {
        flag_error_impl(writer, crate::ffi::types::MPACK_ERROR_TOO_BIG);
        return;
    }
    let mut header_storage = [0_u8; 5];
    let mut encoder = Writer::new(&mut header_storage);
    if kind == BuildKind::Map {
        encoder.write_map_header(count);
    } else {
        encoder.write_array_header(count);
    }
    if encoder.error() != Error::Ok {
        flag_error_impl(writer, core_error_to_abi(encoder.error()));
        return;
    }
    let header_len = encoder.used();
    // SAFETY: The active writer owns one contiguous buffer. Builder writes
    // reserve five bytes, so replacing them with a <=5-byte header only moves
    // initialized payload bytes toward the beginning of the same allocation.
    let state = unsafe { &mut *writer };
    let used = state.position as usize - state.buffer as usize;
    if frame.start.checked_add(5).is_none_or(|payload| payload > used) {
        flag_error_impl(writer, MPACK_ERROR_BUG);
        return;
    }
    let payload_start = frame.start + 5;
    let payload_len = used - payload_start;
    unsafe {
        ptr::copy(
            state.buffer.add(payload_start).cast::<u8>(),
            state.buffer.add(frame.start + header_len).cast::<u8>(),
            payload_len,
        );
        ptr::copy_nonoverlapping(
            encoder.written().as_ptr(),
            state.buffer.add(frame.start).cast::<u8>(),
            header_len,
        );
        state.position = state
            .buffer
            .add(frame.start + header_len + payload_len);
    }
}

fn init_file_writer(writer: *mut MpackWriter, file: *mut c_void, close_when_done: bool) {
    // SAFETY: C malloc returns a writable allocation or null.
    let buffer = unsafe { malloc(OWNED_BUFFER_CAPACITY).cast::<c_char>() };
    if buffer.is_null() {
        if close_when_done {
            // SAFETY: The caller supplied a live FILE pointer.
            unsafe {
                fclose(file);
            }
        }
        // SAFETY: The caller supplied writable writer storage.
        unsafe {
            writer.write(MpackWriter::error_state(
                crate::ffi::types::MPACK_ERROR_MEMORY,
            ));
        }
        return;
    }
    let context = Box::new(FileContext {
        file,
        close_when_done,
    });
    let mut state = MpackWriter::fixed_buffer(buffer, OWNED_BUFFER_CAPACITY);
    state.context = Box::into_raw(context).cast::<c_void>();
    state.flush = Some(file_flush);
    state.teardown = Some(file_teardown);
    // SAFETY: The caller supplied writable writer storage.
    unsafe { writer.write(state) };
}

unsafe extern "C" fn growable_flush(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: usize,
) {
    if writer.is_null() {
        return;
    }
    // SAFETY: MPack invokes this callback with its live writer.
    let state = unsafe { &mut *writer };
    let current_used = state.position as usize - state.buffer as usize;
    if data == state.buffer && current_used == count {
        return;
    }
    let external_data = data != state.buffer;
    let buffered = if external_data { current_used } else { count };
    let required = match buffered.checked_add(if external_data { count } else { 0 }) {
        Some(required) => required,
        None => {
            flag_error_impl(writer, crate::ffi::types::MPACK_ERROR_TOO_BIG);
            return;
        }
    };
    let old_capacity = state.end as usize - state.buffer as usize;
    let mut capacity = old_capacity.max(1);
    while capacity < required.max(old_capacity.saturating_add(1)) {
        let Some(next) = capacity.checked_mul(2) else {
            flag_error_impl(writer, crate::ffi::types::MPACK_ERROR_TOO_BIG);
            return;
        };
        capacity = next;
    }
    // SAFETY: The existing buffer came from C malloc/realloc.
    let new_buffer = unsafe { realloc(state.buffer.cast::<c_void>(), capacity).cast::<c_char>() };
    if new_buffer.is_null() {
        flag_error_impl(writer, crate::ffi::types::MPACK_ERROR_MEMORY);
        return;
    }
    state.buffer = new_buffer;
    state.position = new_buffer.wrapping_add(buffered);
    state.end = new_buffer.wrapping_add(capacity);
    if external_data && count != 0 {
        // SAFETY: Capacity was grown to fit the external payload.
        unsafe {
            ptr::copy_nonoverlapping(data.cast::<u8>(), state.position.cast::<u8>(), count);
            state.position = state.position.add(count);
        }
    }
}

unsafe extern "C" fn growable_teardown(writer: *mut MpackWriter) {
    if writer.is_null() {
        return;
    }
    // SAFETY: The callback is installed only with this context type.
    let state = unsafe { &mut *writer };
    let context = unsafe { Box::from_raw(state.context.cast::<GrowableContext>()) };
    let used = state.position as usize - state.buffer as usize;
    if state.error == MPACK_OK {
        // Preserve C MPack's non-null result for an empty successful message.
        let wanted = used.max(1);
        // SAFETY: The buffer came from C malloc/realloc.
        let resized = unsafe { realloc(state.buffer.cast::<c_void>(), wanted).cast::<c_char>() };
        if resized.is_null() {
            // SAFETY: Free the original allocation after resize failure.
            unsafe { free(state.buffer.cast::<c_void>()) };
            state.buffer = ptr::null_mut();
            flag_error_impl(writer, crate::ffi::types::MPACK_ERROR_MEMORY);
        } else {
            // SAFETY: Result pointers remain live through destroy by contract.
            unsafe {
                context.target_data.write(resized);
                context.target_size.write(used);
            }
            state.buffer = ptr::null_mut();
        }
    } else if !state.buffer.is_null() {
        // SAFETY: The buffer came from C malloc/realloc.
        unsafe { free(state.buffer.cast::<c_void>()) };
        state.buffer = ptr::null_mut();
    }
    state.context = ptr::null_mut();
}

unsafe extern "C" fn file_flush(
    writer: *mut MpackWriter,
    data: *const c_char,
    count: usize,
) {
    if writer.is_null() {
        return;
    }
    // SAFETY: The callback is installed only with this context type.
    let context = unsafe { &*((*writer).context.cast::<FileContext>()) };
    // SAFETY: `data` has `count` bytes and context contains a live FILE pointer.
    if unsafe { fwrite(data.cast::<c_void>(), 1, count, context.file) } != count {
        flag_error_impl(writer, crate::ffi::types::MPACK_ERROR_IO);
    }
}

unsafe extern "C" fn file_teardown(writer: *mut MpackWriter) {
    if writer.is_null() {
        return;
    }
    // SAFETY: The callback is installed only with this context type.
    let state = unsafe { &mut *writer };
    let context = unsafe { Box::from_raw(state.context.cast::<FileContext>()) };
    if context.close_when_done {
        // SAFETY: The context owns the live FILE pointer.
        if unsafe { fclose(context.file) } != 0 {
            flag_error_impl(writer, crate::ffi::types::MPACK_ERROR_IO);
        }
    }
    if !state.buffer.is_null() {
        // SAFETY: The buffer came from C malloc.
        unsafe { free(state.buffer.cast::<c_void>()) };
    }
    state.buffer = ptr::null_mut();
    state.context = ptr::null_mut();
}
