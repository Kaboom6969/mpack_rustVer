//! C ABI boundary for the buffered MessagePack reader (`full-suite-abi`).
//!
//! Mirrors the writer pattern: C owns `mpack_reader_t` storage; each operation
//! builds a temporary safe-core [`crate::reader::Reader`] over `data..end`,
//! advances the C cursor, and maps sticky errors through `flag_error`.

use std::ffi::{c_char, c_int, c_void};
use std::io::Write;
use std::ptr;
use std::slice;

use crate::common::{Tag, Timestamp};
use crate::ffi::guard::catch_ffi_panic;
use crate::ffi::types::{
    core_error_to_abi, MpackError, MpackReader, MpackReaderFill, MpackReaderSkip, MpackTag,
    MpackTimestamp, MpackTrack, MPACK_ERROR_BUG, MPACK_ERROR_EOF, MPACK_ERROR_INVALID,
    MPACK_ERROR_IO, MPACK_ERROR_MEMORY, MPACK_ERROR_TOO_BIG, MPACK_ERROR_TYPE, MPACK_OK,
};
use crate::reader::{self, Reader};

const OWNED_BUFFER_CAPACITY: usize = 4096;
const READER_MINIMUM_BUFFER_SIZE: usize = 32;
const MAXIMUM_TAG_SIZE: usize = 9;
const PRINT_BYTE_COUNT: usize = 12;

const TYPE_BOOL: c_int = 2;
const TYPE_INT: c_int = 3;
const TYPE_UINT: c_int = 4;
const TYPE_FLOAT: c_int = 5;
const TYPE_DOUBLE: c_int = 6;
const TYPE_STR: c_int = 7;
const TYPE_BIN: c_int = 8;
const TYPE_ARRAY: c_int = 9;
const TYPE_MAP: c_int = 10;
const TYPE_EXT: c_int = 11;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(pointer: *mut c_void);
    fn fopen(filename: *const c_char, mode: *const c_char) -> *mut c_void;
    fn fread(data: *mut c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    fn fwrite(data: *const c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    fn fclose(file: *mut c_void) -> c_int;
    fn feof(file: *mut c_void) -> c_int;
    fn fseek(file: *mut c_void, offset: i64, whence: c_int) -> c_int;
    fn ftell(file: *mut c_void) -> i64;
    fn ferror(file: *mut c_void) -> c_int;
}

const SEEK_CUR: c_int = 1;

struct FileContext {
    file: *mut c_void,
}

fn empty_reader() -> MpackReader {
    MpackReader {
        context: ptr::null_mut(),
        fill: None,
        error_fn: None,
        teardown: None,
        skip: None,
        buffer: ptr::null_mut(),
        size: 0,
        data: ptr::null(),
        end: ptr::null(),
        error: MPACK_OK,
        track: MpackTrack::empty(),
    }
}

pub(crate) fn tag_to_abi(tag: Tag) -> MpackTag {
    match tag {
        Tag::Nil => MpackTag::nil(),
        Tag::Bool(value) => MpackTag {
            type_: TYPE_BOOL,
            exttype: 0,
            _pad: [0; 3],
            value: u64::from(value),
        },
        Tag::Int(value) => MpackTag {
            type_: TYPE_INT,
            exttype: 0,
            _pad: [0; 3],
            value: value as u64,
        },
        Tag::Uint(value) => MpackTag {
            type_: TYPE_UINT,
            exttype: 0,
            _pad: [0; 3],
            value,
        },
        Tag::Float(value) => MpackTag {
            type_: TYPE_FLOAT,
            exttype: 0,
            _pad: [0; 3],
            value: u64::from(value.to_bits()),
        },
        Tag::Double(value) => MpackTag {
            type_: TYPE_DOUBLE,
            exttype: 0,
            _pad: [0; 3],
            value: value.to_bits(),
        },
        Tag::Str(length) => MpackTag {
            type_: TYPE_STR,
            exttype: 0,
            _pad: [0; 3],
            value: u64::from(length),
        },
        Tag::Bin(length) => MpackTag {
            type_: TYPE_BIN,
            exttype: 0,
            _pad: [0; 3],
            value: u64::from(length),
        },
        Tag::Array(count) => MpackTag {
            type_: TYPE_ARRAY,
            exttype: 0,
            _pad: [0; 3],
            value: u64::from(count),
        },
        Tag::Map(count) => MpackTag {
            type_: TYPE_MAP,
            exttype: 0,
            _pad: [0; 3],
            value: u64::from(count),
        },
        Tag::Ext {
            extension_type,
            length,
        } => MpackTag {
            type_: TYPE_EXT,
            exttype: extension_type,
            _pad: [0; 3],
            value: u64::from(length),
        },
    }
}

fn tag_length(tag: &MpackTag) -> u32 {
    (tag.value & u32::MAX as u64) as u32
}

/// Borrows C reader storage after the ABI entry has null-checked `reader`.
///
/// # Safety
///
/// `reader` must be non-null and point to uniquely writable `mpack_reader_t`
/// storage for the duration of the returned borrow.
pub(crate) unsafe fn borrow_reader<'a>(reader: *mut MpackReader) -> &'a mut MpackReader {
    // SAFETY: Caller upholds the null/liveness contract above.
    unsafe { &mut *reader }
}

/// Sticky error field after the ABI entry has null-checked `reader`.
///
/// # Safety
///
/// Same contract as [`borrow_reader`].
pub(crate) unsafe fn reader_error(reader: *mut MpackReader) -> MpackError {
    // SAFETY: Caller upholds the null/liveness contract.
    unsafe { borrow_reader(reader).error }
}

pub(crate) fn remaining_of(state: &MpackReader) -> Option<usize> {
    if state.data.is_null() && state.end.is_null() {
        return Some(0);
    }
    if state.data.is_null() || state.end.is_null() {
        return None;
    }
    let data = state.data as usize;
    let end = state.end as usize;
    if end < data {
        return None;
    }
    Some(end - data)
}

pub(crate) fn data_slice(state: &MpackReader) -> Option<&[u8]> {
    let remaining = remaining_of(state)?;
    if remaining == 0 {
        return Some(&[]);
    }
    // SAFETY: `remaining_of` validated `data..end` as a live contiguous range.
    Some(unsafe { slice::from_raw_parts(state.data.cast::<u8>(), remaining) })
}

pub(crate) fn advance(state: &mut MpackReader, count: usize) {
    state.data = state.data.wrapping_add(count);
}

pub(crate) fn flag_error_on(state: &mut MpackReader, reader: *mut MpackReader, error: MpackError) {
    if error == MPACK_OK || state.error != MPACK_OK {
        return;
    }
    state.error = error;
    state.end = state.data;
    if let Some(error_fn) = state.error_fn {
        // SAFETY: `reader` is the live C object `state` borrows; callback may
        // re-enter only through the documented MPack error-handler contract.
        unsafe { error_fn(reader, error) };
    }
}

pub(crate) fn flag_error_impl(reader: *mut MpackReader, error: MpackError) {
    if reader.is_null() || error == MPACK_OK {
        return;
    }
    // SAFETY: Non-null reader points at writable C storage.
    let state = unsafe { borrow_reader(reader) };
    flag_error_on(state, reader, error);
}

pub(crate) fn flag_bug(reader: *mut MpackReader) {
    if reader.is_null() {
        return;
    }
    // SAFETY: Non-null reader points at writable C storage.
    let state = unsafe { borrow_reader(reader) };
    state.error = MPACK_ERROR_BUG;
    state.end = state.data;
}

fn initialize_as_bug(reader: *mut MpackReader) {
    if reader.is_null() {
        return;
    }
    let mut state = empty_reader();
    state.error = MPACK_ERROR_BUG;
    // SAFETY: Non-null destination is writable `mpack_reader_t` storage.
    unsafe {
        reader.write(state);
    }
}

fn fill_range(reader: *mut MpackReader, destination: *mut c_char, min_bytes: usize, max_bytes: usize) -> usize {
    // SAFETY: Internal callers pass a live non-null reader.
    let state = unsafe { borrow_reader(reader) };
    let Some(fill) = state.fill else {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        return 0;
    };
    if min_bytes == 0 || max_bytes < min_bytes {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        return 0;
    }

    let mut count = 0usize;
    while count < min_bytes {
        // SAFETY: fill callback and destination buffer come from the C reader contract.
        let read = unsafe { fill(reader, destination.wrapping_add(count), max_bytes - count) };
        let state = unsafe { borrow_reader(reader) };
        if state.error != MPACK_OK {
            return 0;
        }
        if read == 0 || read == usize::MAX {
            flag_error_on(state, reader, MPACK_ERROR_IO);
            return 0;
        }
        count += read;
    }
    count
}

pub(crate) fn ensure_impl(reader: *mut MpackReader, count: usize) -> bool {
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return false;
    }
    let Some(left) = remaining_of(state) else {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        return false;
    };
    if count <= left {
        return true;
    }
    ensure_straddle_impl(reader, count)
}

fn header_size_for_marker(marker: u8) -> usize {
    let base = match marker {
        0x00..=0xbf | 0xc0..=0xc3 | 0xd4..=0xd8 | 0xe0..=0xff => 1,
        0xc4 | 0xc7 | 0xcc | 0xd0 | 0xd9 => 2,
        0xc5 | 0xc8 | 0xcd | 0xd1 | 0xda | 0xdc | 0xde => 3,
        0xc6 | 0xc9 | 0xce | 0xd2 | 0xdb | 0xdd | 0xdf => 5,
        0xca => 5,
        0xcb | 0xcf | 0xd3 => 9,
    };
    base + usize::from(matches!(marker, 0xc7..=0xc9 | 0xd4..=0xd8))
}

pub(crate) fn ensure_tag_header(reader: *mut MpackReader) -> bool {
    if !ensure_impl(reader, 1) {
        return false;
    }
    let state = unsafe { borrow_reader(reader) };
    // SAFETY: ensure(1) established at least one readable byte at `data`.
    let marker = unsafe { *state.data.cast::<u8>() };
    let needed = header_size_for_marker(marker).min(MAXIMUM_TAG_SIZE + 1);
    ensure_impl(reader, needed)
}

pub(crate) fn read_with_core<T>(
    reader: *mut MpackReader,
    mut operation: impl FnMut(&mut Reader<'_>) -> T,
    on_error: impl FnOnce() -> T,
) -> T {
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return on_error();
    }
    let Some(remaining) = remaining_of(state) else {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        return on_error();
    };
    // Copy the raw cursor before building the temporary slice so `state` is
    // not considered borrowed across the safe-core call.
    let data_ptr = state.data.cast::<u8>();
    let input = if remaining == 0 {
        &[][..]
    } else {
        // SAFETY: `remaining_of` validated the live data..end range.
        unsafe { slice::from_raw_parts(data_ptr, remaining) }
    };
    let mut core = Reader::new(input);
    let result = operation(&mut core);
    let used = core.used();
    let error = core_error_to_abi(core.error());
    drop(core);
    if used > remaining {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        return on_error();
    }
    advance(state, used);
    if error != MPACK_OK {
        flag_error_on(state, reader, error);
        return on_error();
    }
    result
}

fn skip_using_fill(reader: *mut MpackReader, mut count: usize) {
    let state = unsafe { borrow_reader(reader) };
    if state.fill.is_none() {
        flag_error_on(state, reader, MPACK_ERROR_INVALID);
        return;
    }
    let buffer_size = state.size;
    if buffer_size == 0 {
        flag_error_on(state, reader, MPACK_ERROR_IO);
        return;
    }
    let buffer = state.buffer;

    while count > buffer_size {
        if fill_range(reader, buffer, buffer_size, buffer_size) < buffer_size {
            return;
        }
        count -= buffer_size;
    }

    let state = unsafe { borrow_reader(reader) };
    state.data = state.buffer;
    let read = fill_range(reader, state.buffer, count, buffer_size);
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return;
    }
    if read < count {
        flag_error_on(state, reader, MPACK_ERROR_IO);
        return;
    }
    state.end = state.buffer.wrapping_add(read);
    state.data = state.buffer.wrapping_add(count);
}

fn skip_bytes_straddle(reader: *mut MpackReader, count: usize) {
    let state = unsafe { borrow_reader(reader) };
    if state.fill.is_none() {
        flag_error_on(state, reader, MPACK_ERROR_INVALID);
        return;
    }

    let left = remaining_of(state).unwrap_or(0);
    let mut remaining = count;
    if left > remaining {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        return;
    }
    remaining -= left;
    state.data = state.end;

    let skip = state.skip;
    let size = state.size;
    if skip.is_some() && size > 0 && remaining > size / 16 {
        if let Some(skip) = skip {
            // SAFETY: Optional skip callback installed by C.
            unsafe { skip(reader, remaining) };
        }
        return;
    }
    skip_using_fill(reader, remaining);
}

pub(crate) fn read_native(reader: *mut MpackReader, destination: *mut c_char, count: usize) {
    if count == 0 {
        return;
    }
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        // SAFETY: Caller provides destination writable for `count` bytes.
        unsafe { ptr::write_bytes(destination.cast::<u8>(), 0, count) };
        return;
    }
    let Some(left) = remaining_of(state) else {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        unsafe { ptr::write_bytes(destination.cast::<u8>(), 0, count) };
        return;
    };
    if count <= left {
        // SAFETY: `left` bytes readable at data; destination has `count` bytes.
        unsafe {
            ptr::copy_nonoverlapping(state.data.cast::<u8>(), destination.cast::<u8>(), count);
        }
        advance(state, count);
        return;
    }
    read_native_straddle_impl(reader, destination, count);
}

fn skip_bytes_impl(reader: *mut MpackReader, count: usize) {
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return;
    }
    let Some(left) = remaining_of(state) else {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        return;
    };
    if left >= count {
        advance(state, count);
        return;
    }
    skip_bytes_straddle(reader, count);
}

fn read_bytes_inplace_notrack(reader: *mut MpackReader, count: usize) -> *const c_char {
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return ptr::null();
    }
    let Some(left) = remaining_of(state) else {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        return ptr::null();
    };
    if left >= count {
        let bytes = state.data;
        advance(state, count);
        return bytes;
    }
    if !ensure_impl(reader, count) {
        return ptr::null();
    }
    let state = unsafe { borrow_reader(reader) };
    let bytes = state.data;
    advance(state, count);
    bytes
}

fn read_tag_impl(reader: *mut MpackReader) -> MpackTag {
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return MpackTag::nil();
    }
    let track_error = crate::ffi::stubs::track::track_element(&mut state.track);
    if track_error != MPACK_OK {
        flag_error_on(state, reader, track_error);
        return MpackTag::nil();
    }
    if !ensure_tag_header(reader) {
        return MpackTag::nil();
    }
    let tag = read_with_core(
        reader,
        |core| core.read_tag().map(tag_to_abi).unwrap_or_else(MpackTag::nil),
        MpackTag::nil,
    );
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return tag;
    }
    let count = (tag.value & u32::MAX as u64) as u32;
    let push_error =
        crate::ffi::stubs::track::track_push_for_tag(&mut state.track, tag.type_, count);
    if push_error != MPACK_OK {
        flag_error_on(state, reader, push_error);
        return MpackTag::nil();
    }
    tag
}

fn peek_tag_impl(reader: *mut MpackReader) -> MpackTag {
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return MpackTag::nil();
    }
    let track_error = crate::ffi::stubs::track::track_peek_element(&state.track);
    if track_error != MPACK_OK {
        flag_error_on(state, reader, track_error);
        return MpackTag::nil();
    }
    if !ensure_tag_header(reader) {
        return MpackTag::nil();
    }
    let state = unsafe { borrow_reader(reader) };
    let Some(input) = data_slice(state) else {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        return MpackTag::nil();
    };
    let mut core = Reader::new(input);
    match core.peek_tag() {
        Some(tag) => {
            let error = core_error_to_abi(core.error());
            if error != MPACK_OK {
                flag_error_on(state, reader, error);
                MpackTag::nil()
            } else {
                tag_to_abi(tag)
            }
        }
        None => {
            let error = core_error_to_abi(core.error());
            if error != MPACK_OK {
                flag_error_on(state, reader, error);
            }
            MpackTag::nil()
        }
    }
}

pub(crate) fn done_type_impl(reader: *mut MpackReader, type_: c_int) {
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return;
    }
    let error = crate::ffi::stubs::track::track_pop(&mut state.track, type_);
    if error != MPACK_OK {
        flag_error_on(state, reader, error);
    }
}

fn init_stdfile_impl(reader: *mut MpackReader, file: *mut c_void, close_when_done: bool) {
    let buffer = unsafe { malloc(OWNED_BUFFER_CAPACITY).cast::<c_char>() };
    if buffer.is_null() {
        if close_when_done {
            unsafe {
                fclose(file);
            }
        }
        unsafe {
            reader.write(MpackReader::error_state(MPACK_ERROR_MEMORY));
        }
        return;
    }

    let mut state = empty_reader();
    state.buffer = buffer;
    state.size = OWNED_BUFFER_CAPACITY;
    state.data = buffer;
    state.end = buffer;
    state.context = Box::into_raw(Box::new(FileContext { file })).cast();
    state.fill = Some(file_reader_fill);
    state.skip = Some(file_reader_skip);
    state.teardown = Some(if close_when_done {
        file_reader_teardown_close
    } else {
        file_reader_teardown
    });
    let track_error = crate::ffi::stubs::track::track_init(&mut state.track);
    if track_error != MPACK_OK {
        state.error = track_error;
    }
    unsafe {
        reader.write(state);
    }
}

unsafe extern "C" fn file_reader_fill(reader: *mut MpackReader, buffer: *mut c_char, count: usize) -> usize {
    // SAFETY: Fill callback is only installed on a live non-null reader.
    let state = unsafe { borrow_reader(reader) };
    let context = unsafe { &*state.context.cast::<FileContext>() };
    if unsafe { feof(context.file) } != 0 {
        flag_error_impl(reader, MPACK_ERROR_EOF);
        return 0;
    }
    unsafe { fread(buffer.cast(), 1, count, context.file) }
}

unsafe extern "C" fn file_reader_skip(reader: *mut MpackReader, count: usize) {
    // SAFETY: Skip callback is only installed on a live non-null reader.
    if unsafe { reader_error(reader) } != MPACK_OK {
        return;
    }
    let state = unsafe { borrow_reader(reader) };
    let context = unsafe { &*state.context.cast::<FileContext>() };
    if unsafe { ftell(context.file) } >= 0 {
        if unsafe { fseek(context.file, count as i64, SEEK_CUR) } == 0 {
            return;
        }
        if unsafe { ferror(context.file) } != 0 {
            flag_error_impl(reader, MPACK_ERROR_IO);
            return;
        }
    }
    skip_using_fill(reader, count);
}

unsafe extern "C" fn file_reader_teardown(reader: *mut MpackReader) {
    // SAFETY: Teardown is only invoked on a live non-null reader.
    let state = unsafe { borrow_reader(reader) };
    if !state.buffer.is_null() {
        unsafe { free(state.buffer.cast()) };
    }
    if !state.context.is_null() {
        unsafe {
            drop(Box::from_raw(state.context.cast::<FileContext>()));
        }
    }
    state.buffer = ptr::null_mut();
    state.context = ptr::null_mut();
    state.size = 0;
    state.fill = None;
    state.skip = None;
    state.teardown = None;
}

unsafe extern "C" fn file_reader_teardown_close(reader: *mut MpackReader) {
    // SAFETY: Teardown is only invoked on a live non-null reader.
    let state = unsafe { borrow_reader(reader) };
    let context = unsafe { &*state.context.cast::<FileContext>() };
    let file = context.file;
    let close_result = unsafe { fclose(file) };
    unsafe { file_reader_teardown(reader) };
    if close_result != 0 {
        flag_error_impl(reader, MPACK_ERROR_IO);
    }
}

/// Initializes a buffered reader over a writable buffer.
///
/// # Safety
///
/// `reader` must be null or writable storage for one `mpack_reader_t`.
/// `buffer` must be non-null and writable for `size` bytes when `size > 0`.
#[no_mangle]
pub unsafe extern "C" fn mpack_reader_init(
    reader: *mut MpackReader,
    buffer: *mut c_char,
    size: usize,
    count: usize,
) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        if buffer.is_null() {
            // C asserts on null buffer; fail closed with sticky BUG.
            unsafe {
                reader.write(MpackReader::error_state(MPACK_ERROR_BUG));
            }
            return;
        }
        let mut state = empty_reader();
        state.buffer = buffer;
        state.size = size;
        state.data = buffer;
        state.end = buffer.wrapping_add(count);
        let track_error = crate::ffi::stubs::track::track_init(&mut state.track);
        if track_error != MPACK_OK {
            state.error = track_error;
        }
        // SAFETY: Non-null writer storage for one mpack_reader_t.
        unsafe {
            reader.write(state);
        }
    })
    .is_err()
    {
        initialize_as_bug(reader);
    }
}

/// Initializes a reader directly in an error state.
///
/// # Safety
///
/// `reader` must be null or writable storage for one `mpack_reader_t`.
#[no_mangle]
pub unsafe extern "C" fn mpack_reader_init_error(reader: *mut MpackReader, error: MpackError) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        unsafe {
            reader.write(MpackReader::error_state(error));
        }
    })
    .is_err()
    {
        initialize_as_bug(reader);
    }
}

/// Initializes a reader over a contiguous data slice (no fill).
///
/// # Safety
///
/// `reader` must be null or writable storage for one `mpack_reader_t`.
/// When `count > 0`, `data` must point at `count` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn mpack_reader_init_data(
    reader: *mut MpackReader,
    data: *const c_char,
    count: usize,
) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        let mut state = empty_reader();
        state.data = data;
        state.end = if data.is_null() {
            ptr::null()
        } else {
            data.wrapping_add(count)
        };
        let track_error = crate::ffi::stubs::track::track_init(&mut state.track);
        if track_error != MPACK_OK {
            state.error = track_error;
        }
        unsafe {
            reader.write(state);
        }
    })
    .is_err()
    {
        initialize_as_bug(reader);
    }
}

/// Opens `filename` and initializes a stdfile reader that owns the FILE.
///
/// # Safety
///
/// `reader` must be null or writable storage for one `mpack_reader_t`.
/// `filename` must be a valid C string.
#[no_mangle]
pub unsafe extern "C" fn mpack_reader_init_filename(reader: *mut MpackReader, filename: *const c_char) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        if filename.is_null() {
            unsafe {
                reader.write(MpackReader::error_state(MPACK_ERROR_BUG));
            }
            return;
        }
        let file = unsafe { fopen(filename, c"rb".as_ptr()) };
        if file.is_null() {
            unsafe {
                reader.write(MpackReader::error_state(MPACK_ERROR_IO));
            }
            return;
        }
        init_stdfile_impl(reader, file, true);
    })
    .is_err()
    {
        initialize_as_bug(reader);
    }
}

/// Initializes a reader that fills from an existing stdio FILE.
///
/// # Safety
///
/// `reader` must be null or writable storage for one `mpack_reader_t`.
/// `stdfile` must be a live `FILE*`.
#[no_mangle]
pub unsafe extern "C" fn mpack_reader_init_stdfile(
    reader: *mut MpackReader,
    stdfile: *mut c_void,
    close_when_done: bool,
) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        if stdfile.is_null() {
            unsafe {
                reader.write(MpackReader::error_state(MPACK_ERROR_BUG));
            }
            return;
        }
        init_stdfile_impl(reader, stdfile, close_when_done);
    })
    .is_err()
    {
        initialize_as_bug(reader);
    }
}

/// Destroys a reader and returns its sticky error.
///
/// # Safety
///
/// `reader` must be null or a live reader previously initialized by an init API.
#[no_mangle]
pub unsafe extern "C" fn mpack_reader_destroy(reader: *mut MpackReader) -> MpackError {
    if reader.is_null() {
        return MPACK_ERROR_BUG;
    }
    match catch_ffi_panic(|| {
        // SAFETY: Non-null reader points at writable C storage.
        let state = unsafe { borrow_reader(reader) };
        let cancel = state.error != MPACK_OK;
        let track_error = crate::ffi::stubs::track::track_destroy(&mut state.track, cancel);
        if track_error != MPACK_OK && state.error == MPACK_OK {
            state.error = track_error;
        }
        if let Some(teardown) = state.teardown.take() {
            unsafe { teardown(reader) };
        }
        state.error
    }) {
        Ok(error) => error,
        Err(_) => MPACK_ERROR_BUG,
    }
}

/// Installs a fill callback (requires a writable buffer of minimum size).
///
/// # Safety
///
/// `reader` must be null or a live reader with a buffer sized for fill use.
#[no_mangle]
pub unsafe extern "C" fn mpack_reader_set_fill(reader: *mut MpackReader, fill: MpackReaderFill) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        // SAFETY: Non-null reader points at writable C storage.
        let state = unsafe { borrow_reader(reader) };
        if state.size == 0 {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return;
        }
        if state.size < READER_MINIMUM_BUFFER_SIZE {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return;
        }
        state.fill = fill;
    })
    .is_err()
    {
        flag_bug(reader);
    }
}

/// Installs an optional skip callback.
///
/// # Safety
///
/// `reader` must be null or a live reader. Requires a writable buffer
/// (`size != 0`); otherwise flags sticky `mpack_error_bug` (fail-closed vs C assert).
#[no_mangle]
pub unsafe extern "C" fn mpack_reader_set_skip(reader: *mut MpackReader, skip: MpackReaderSkip) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        // SAFETY: Non-null reader points at writable C storage.
        let state = unsafe { borrow_reader(reader) };
        if state.size == 0 {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return;
        }
        state.skip = skip;
    })
    .is_err()
    {
        flag_bug(reader);
    }
}

/// Flags a sticky reader error (first error wins; truncates remaining data).
///
/// # Safety
///
/// `reader` must be null or a live reader.
#[no_mangle]
pub unsafe extern "C" fn mpack_reader_flag_error(reader: *mut MpackReader, error: MpackError) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| flag_error_impl(reader, error)).is_err() {
        flag_bug(reader);
    }
}

/// Returns bytes left in the current window (0 if the reader is in error).
///
/// # Safety
///
/// `reader` must be null or a live reader. `data`, when non-null, must be writable.
#[no_mangle]
pub unsafe extern "C" fn mpack_reader_remaining(
    reader: *mut MpackReader,
    data: *mut *const c_char,
) -> usize {
    if reader.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| {
        // SAFETY: Non-null reader points at writable C storage.
        let state = unsafe { borrow_reader(reader) };
        if state.error != MPACK_OK {
            if !data.is_null() {
                unsafe { *data = ptr::null() };
            }
            return 0;
        }
        let Some(remaining) = remaining_of(state) else {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            if !data.is_null() {
                unsafe { *data = ptr::null() };
            }
            return 0;
        };
        if !data.is_null() {
            unsafe { *data = state.data };
        }
        remaining
    }) {
        Ok(remaining) => remaining,
        Err(_) => {
            flag_bug(reader);
            0
        }
    }
}

/// Reads the next MessagePack tag.
///
/// # Safety
///
/// `reader` must be null or point to a live, uniquely writable `mpack_reader_t`.
#[no_mangle]
pub unsafe extern "C" fn mpack_read_tag(reader: *mut MpackReader) -> MpackTag {
    if reader.is_null() {
        return MpackTag::nil();
    }
    match catch_ffi_panic(|| read_tag_impl(reader)) {
        Ok(tag) => tag,
        Err(_) => {
            flag_bug(reader);
            MpackTag::nil()
        }
    }
}

/// Peeks the next MessagePack tag without advancing the cursor.
///
/// # Safety
///
/// `reader` must be null or point to a live, uniquely writable `mpack_reader_t`.
#[no_mangle]
pub unsafe extern "C" fn mpack_peek_tag(reader: *mut MpackReader) -> MpackTag {
    if reader.is_null() {
        return MpackTag::nil();
    }
    match catch_ffi_panic(|| peek_tag_impl(reader)) {
        Ok(tag) => tag,
        Err(_) => {
            flag_bug(reader);
            MpackTag::nil()
        }
    }
}

/// Copies `count` bytes from the open str/bin/ext into `p`.
///
/// # Safety
///
/// `reader` must be null or a live reader. Non-null `p` must be writable for `count`.
#[no_mangle]
pub unsafe extern "C" fn mpack_read_bytes(reader: *mut MpackReader, p: *mut c_char, count: usize) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        if p.is_null() && count != 0 {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return;
        }
        let state = unsafe { borrow_reader(reader) };
        if state.error == MPACK_OK {
            let track_error = crate::ffi::stubs::track::track_bytes(&mut state.track, count);
            if track_error != MPACK_OK {
                flag_error_on(state, reader, track_error);
                return;
            }
        }
        read_native(reader, p, count);
    })
    .is_err()
    {
        flag_bug(reader);
    }
}

/// Skips `count` bytes from the open str/bin/ext.
///
/// # Safety
///
/// `reader` must be null or point to a live, uniquely writable `mpack_reader_t`.
#[no_mangle]
pub unsafe extern "C" fn mpack_skip_bytes(reader: *mut MpackReader, count: usize) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        let state = unsafe { borrow_reader(reader) };
        if state.error == MPACK_OK {
            let track_error = crate::ffi::stubs::track::track_bytes(&mut state.track, count);
            if track_error != MPACK_OK {
                flag_error_on(state, reader, track_error);
                return;
            }
        }
        skip_bytes_impl(reader, count)
    })
    .is_err()
    {
        flag_bug(reader);
    }
}

/// Returns an in-place pointer into the reader buffer for `count` bytes.
///
/// # Safety
///
/// `reader` must be null or a live reader. The returned pointer is valid only
/// until the next mutating reader operation.
#[no_mangle]
pub unsafe extern "C" fn mpack_read_bytes_inplace(
    reader: *mut MpackReader,
    count: usize,
) -> *const c_char {
    if reader.is_null() {
        return ptr::null();
    }
    match catch_ffi_panic(|| {
        let state = unsafe { borrow_reader(reader) };
        if state.error == MPACK_OK {
            let track_error = crate::ffi::stubs::track::track_bytes(&mut state.track, count);
            if track_error != MPACK_OK {
                flag_error_on(state, reader, track_error);
                return ptr::null();
            }
        }
        read_bytes_inplace_notrack(reader, count)
    }) {
        Ok(pointer) => pointer,
        Err(_) => {
            flag_bug(reader);
            ptr::null()
        }
    }
}

/// In-place UTF-8 read of an open string.
///
/// # Safety
///
/// `reader` must be null or a live reader.
#[no_mangle]
pub unsafe extern "C" fn mpack_read_utf8_inplace(
    reader: *mut MpackReader,
    count: usize,
) -> *const c_char {
    if reader.is_null() {
        return ptr::null();
    }
    match catch_ffi_panic(|| {
        let pointer = read_bytes_inplace_notrack(reader, count);
        if pointer.is_null() {
            return ptr::null();
        }
        let bytes = unsafe { slice::from_raw_parts(pointer.cast::<u8>(), count) };
        if !reader::check_utf8(bytes) {
            flag_error_impl(reader, MPACK_ERROR_TYPE);
            return ptr::null();
        }
        pointer
    }) {
        Ok(pointer) => pointer,
        Err(_) => {
            flag_bug(reader);
            ptr::null()
        }
    }
}

/// Reads bytes and validates UTF-8 into a caller buffer.
///
/// # Safety
///
/// `reader` must be null or a live reader. Non-null `p` must be writable for `byte_count`.
#[no_mangle]
pub unsafe extern "C" fn mpack_read_utf8(reader: *mut MpackReader, p: *mut c_char, byte_count: usize) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        if p.is_null() && byte_count != 0 {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return;
        }
        read_native(reader, p, byte_count);
        // SAFETY: Non-null reader; read_native does not invalidate storage.
        if unsafe { reader_error(reader) } != MPACK_OK {
            return;
        }
        let bytes = unsafe { slice::from_raw_parts(p.cast::<u8>(), byte_count) };
        if !reader::check_utf8(bytes) {
            flag_error_impl(reader, MPACK_ERROR_TYPE);
        }
    })
    .is_err()
    {
        flag_bug(reader);
    }
}

fn read_cstr_unchecked(
    reader: *mut MpackReader,
    buf: *mut c_char,
    buffer_size: usize,
    byte_count: usize,
) {
    if buf.is_null() || buffer_size == 0 {
        flag_error_impl(reader, MPACK_ERROR_BUG);
        return;
    }
    // SAFETY: Callers null-check `reader` before invoking this helper.
    if unsafe { reader_error(reader) } != MPACK_OK {
        unsafe { *buf = 0 };
        return;
    }
    if byte_count > buffer_size - 1 {
        flag_error_impl(reader, MPACK_ERROR_TOO_BIG);
        unsafe { *buf = 0 };
        return;
    }
    let state = unsafe { borrow_reader(reader) };
    let track_error = crate::ffi::stubs::track::track_str_bytes_all(&mut state.track, byte_count);
    if track_error != MPACK_OK {
        flag_error_on(state, reader, track_error);
        unsafe { *buf = 0 };
        return;
    }
    read_native(reader, buf, byte_count);
    unsafe {
        *buf.add(byte_count) = 0;
    }
}

/// Reads a cstring, rejecting interior NUL bytes.
///
/// # Safety
///
/// `reader` must be null or a live reader. `buf` must be writable for `buffer_size`.
#[no_mangle]
pub unsafe extern "C" fn mpack_read_cstr(
    reader: *mut MpackReader,
    buf: *mut c_char,
    buffer_size: usize,
    byte_count: usize,
) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        read_cstr_unchecked(reader, buf, buffer_size, byte_count);
        // SAFETY: Non-null reader; cstr helper does not invalidate storage.
        if unsafe { reader_error(reader) } != MPACK_OK {
            return;
        }
        let bytes = unsafe { slice::from_raw_parts(buf.cast::<u8>(), byte_count) };
        if bytes.iter().any(|&b| b == 0) {
            unsafe { *buf = 0 };
            flag_error_impl(reader, MPACK_ERROR_TYPE);
        }
    })
    .is_err()
    {
        flag_bug(reader);
    }
}

/// Reads a UTF-8 cstring, rejecting interior NULs.
///
/// # Safety
///
/// `reader` must be null or a live reader. `buf` must be writable for `buffer_size`.
#[no_mangle]
pub unsafe extern "C" fn mpack_read_utf8_cstr(
    reader: *mut MpackReader,
    buf: *mut c_char,
    buffer_size: usize,
    byte_count: usize,
) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        read_cstr_unchecked(reader, buf, buffer_size, byte_count);
        // SAFETY: Non-null reader; cstr helper does not invalidate storage.
        if unsafe { reader_error(reader) } != MPACK_OK {
            return;
        }
        let bytes = unsafe { slice::from_raw_parts(buf.cast::<u8>(), byte_count) };
        if !reader::check_utf8_no_null(bytes) {
            unsafe { *buf = 0 };
            flag_error_impl(reader, MPACK_ERROR_TYPE);
        }
    })
    .is_err()
    {
        flag_bug(reader);
    }
}

/// Allocates and reads bytes (suite frees via `MPACK_FREE` / `test_free`).
///
/// # Safety
///
/// `reader` must be null or a live reader. Returned pointer is owned by the
/// caller and must be freed with the suite allocator when non-null.
#[no_mangle]
pub unsafe extern "C" fn mpack_read_bytes_alloc_impl(
    reader: *mut MpackReader,
    count: usize,
    null_terminated: bool,
) -> *mut c_char {
    if reader.is_null() {
        return ptr::null_mut();
    }
    match catch_ffi_panic(|| {
        // SAFETY: Non-null reader points at writable C storage.
        if unsafe { reader_error(reader) } != MPACK_OK {
            return ptr::null_mut();
        }
        if count == 0 && !null_terminated {
            return ptr::null_mut();
        }
        // Reject size wrap on 32-bit (e.g. count == usize::MAX with NUL).
        // Upstream C has the same latent TODO; this port fails closed.
        let Some(size) = count.checked_add(usize::from(null_terminated)) else {
            flag_error_impl(reader, MPACK_ERROR_TOO_BIG);
            return ptr::null_mut();
        };
        let state = unsafe { borrow_reader(reader) };
        let track_error = crate::ffi::stubs::track::track_bytes(&mut state.track, count);
        if track_error != MPACK_OK {
            flag_error_on(state, reader, track_error);
            return ptr::null_mut();
        }
        let pointer = unsafe { malloc(size.max(1)).cast::<c_char>() };
        if pointer.is_null() {
            flag_error_impl(reader, MPACK_ERROR_MEMORY);
            return ptr::null_mut();
        }
        // Disable error callback while holding the allocation (C parity).
        let state = unsafe { borrow_reader(reader) };
        let error_fn = state.error_fn.take();
        read_native(reader, pointer, count);
        let state = unsafe { borrow_reader(reader) };
        state.error_fn = error_fn;
        if state.error != MPACK_OK {
            if let Some(error_fn) = state.error_fn {
                unsafe { error_fn(reader, state.error) };
            }
            unsafe { free(pointer.cast()) };
            return ptr::null_mut();
        }
        if null_terminated {
            unsafe { *pointer.add(count) = 0 };
        }
        pointer
    }) {
        Ok(pointer) => pointer,
        Err(_) => {
            flag_bug(reader);
            ptr::null_mut()
        }
    }
}

/// Tracking done hook — pops the open track entry for `type_`.
///
/// # Safety
///
/// `reader` may be null; non-null values must point at a live `mpack_reader_t`.
#[no_mangle]
pub unsafe extern "C" fn mpack_done_type(reader: *mut MpackReader, type_: c_int) {
    if reader.is_null() {
        return;
    }
    done_type_impl(reader, type_);
}

/// Discards one complete MessagePack object (iterative; fill-aware).
///
/// # Safety
///
/// `reader` must be null or point to a live, uniquely writable `mpack_reader_t`.
#[no_mangle]
pub unsafe extern "C" fn mpack_discard(reader: *mut MpackReader) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| discard_impl(reader)).is_err() {
        flag_bug(reader);
    }
}

fn discard_impl(reader: *mut MpackReader) {
    // Iterative discard avoids unbounded Rust stack growth on hostile nesting.
    // Semantics match C's recursive discard (read tag, skip payload / children).
    enum Frame {
        Array { left: u32 },
        Map { left: u32, need_value: bool },
    }
    let mut stack: Vec<Frame> = Vec::new();
    loop {
        let tag = read_tag_impl(reader);
        let state = unsafe { borrow_reader(reader) };
        if state.error != MPACK_OK {
            return;
        }
        match tag.type_ {
            TYPE_STR | TYPE_BIN | TYPE_EXT => {
                skip_bytes_impl(reader, tag_length(&tag) as usize);
                done_type_impl(reader, tag.type_);
            }
            TYPE_ARRAY => {
                let n = tag_length(&tag);
                if n > 0 {
                    stack.push(Frame::Array { left: n });
                    continue;
                }
                done_type_impl(reader, TYPE_ARRAY);
            }
            TYPE_MAP => {
                let n = tag_length(&tag);
                if n > 0 {
                    stack.push(Frame::Map {
                        left: n,
                        need_value: false,
                    });
                    continue;
                }
                done_type_impl(reader, TYPE_MAP);
            }
            _ => {}
        }

        loop {
            let Some(frame) = stack.last_mut() else {
                return;
            };
            match frame {
                Frame::Array { left } => {
                    *left -= 1;
                    if *left == 0 {
                        stack.pop();
                        done_type_impl(reader, TYPE_ARRAY);
                        continue;
                    }
                    break;
                }
                Frame::Map { left, need_value } => {
                    if !*need_value {
                        *need_value = true;
                        break;
                    }
                    *need_value = false;
                    *left -= 1;
                    if *left == 0 {
                        stack.pop();
                        done_type_impl(reader, TYPE_MAP);
                        continue;
                    }
                    break;
                }
            }
        }
    }
}

/// Reads a timestamp payload of size 4/8/12 and closes the ext.
///
/// # Safety
///
/// `reader` must be null or a live reader positioned on an open ext payload.
#[no_mangle]
pub unsafe extern "C" fn mpack_read_timestamp(
    reader: *mut MpackReader,
    size: usize,
) -> MpackTimestamp {
    let zero = MpackTimestamp {
        seconds: 0,
        nanoseconds: 0,
    };
    if reader.is_null() {
        return zero;
    }
    match catch_ffi_panic(|| {
        if size != 4 && size != 8 && size != 12 {
            flag_error_impl(reader, MPACK_ERROR_INVALID);
            return zero;
        }
        let mut buf = [0u8; 12];
        read_native(reader, buf.as_mut_ptr().cast(), size);
        unsafe { mpack_done_type(reader, TYPE_EXT) };
        // SAFETY: Non-null reader; prior helpers do not invalidate storage.
        if unsafe { reader_error(reader) } != MPACK_OK {
            return zero;
        }
        match decode_timestamp_payload(&buf[..size]) {
            Some(ts) => MpackTimestamp {
                seconds: ts.seconds,
                nanoseconds: ts.nanoseconds,
            },
            None => {
                flag_error_impl(reader, MPACK_ERROR_INVALID);
                zero
            }
        }
    }) {
        Ok(timestamp) => timestamp,
        Err(_) => {
            flag_bug(reader);
            zero
        }
    }
}

fn decode_timestamp_payload(bytes: &[u8]) -> Option<Timestamp> {
    match bytes.len() {
        4 => {
            let seconds = u32::from_be_bytes(bytes.try_into().ok()?);
            Some(Timestamp {
                seconds: i64::from(seconds),
                nanoseconds: 0,
            })
        }
        8 => {
            let packed = u64::from_be_bytes(bytes.try_into().ok()?);
            let nanoseconds = (packed >> 34) as u32;
            let seconds = (packed & ((1u64 << 34) - 1)) as i64;
            if nanoseconds > 999_999_999 {
                return None;
            }
            Some(Timestamp {
                seconds,
                nanoseconds,
            })
        }
        12 => {
            let nanoseconds = u32::from_be_bytes(bytes[0..4].try_into().ok()?);
            let seconds = i64::from_be_bytes(bytes[4..12].try_into().ok()?);
            if nanoseconds > 999_999_999 {
                return None;
            }
            Some(Timestamp {
                seconds,
                nanoseconds,
            })
        }
        _ => None,
    }
}

fn ensure_straddle_impl(reader: *mut MpackReader, count: usize) -> bool {
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK || count == 0 {
        return false;
    }
    let Some(left) = remaining_of(state) else {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        return false;
    };
    if count <= left {
        return true;
    }
    if state.fill.is_none() {
        flag_error_on(state, reader, MPACK_ERROR_INVALID);
        return false;
    }
    if count > state.size {
        flag_error_on(state, reader, MPACK_ERROR_TOO_BIG);
        return false;
    }
    if state.buffer.is_null() {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        return false;
    }

    // SAFETY: left bytes at data are readable; buffer has size bytes writable.
    unsafe {
        ptr::copy(state.data.cast::<u8>(), state.buffer.cast::<u8>(), left);
    }
    state.data = state.buffer;
    state.end = state.buffer.wrapping_add(left);

    let buffer = state.buffer;
    let size = state.size;
    let read = fill_range(reader, buffer.wrapping_add(left), count - left, size - left);
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return false;
    }
    state.end = state.end.wrapping_add(read);
    true
}

fn read_native_straddle_impl(reader: *mut MpackReader, p: *mut c_char, count: usize) {
    if count == 0 {
        return;
    }
    if p.is_null() {
        flag_error_impl(reader, MPACK_ERROR_BUG);
        return;
    }
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        unsafe { ptr::write_bytes(p.cast::<u8>(), 0, count) };
        return;
    }
    let Some(left) = remaining_of(state) else {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        unsafe { ptr::write_bytes(p.cast::<u8>(), 0, count) };
        return;
    };
    if count <= left {
        flag_error_on(state, reader, MPACK_ERROR_BUG);
        unsafe { ptr::write_bytes(p.cast::<u8>(), 0, count) };
        return;
    }
    if state.fill.is_none() {
        flag_error_on(state, reader, MPACK_ERROR_INVALID);
        unsafe { ptr::write_bytes(p.cast::<u8>(), 0, count) };
        return;
    }
    if state.size == 0 {
        flag_error_on(state, reader, MPACK_ERROR_IO);
        unsafe { ptr::write_bytes(p.cast::<u8>(), 0, count) };
        return;
    }

    let mut destination = p;
    let mut needed = count;
    if left > 0 {
        unsafe {
            ptr::copy_nonoverlapping(state.data.cast::<u8>(), destination.cast::<u8>(), left);
        }
        destination = destination.wrapping_add(left);
        needed -= left;
        advance(state, left);
    }

    let state = unsafe { borrow_reader(reader) };
    if needed <= state.size / 32 {
        let buffer = state.buffer;
        let size = state.size;
        let read = fill_range(reader, buffer, needed, size);
        if unsafe { borrow_reader(reader) }.error != MPACK_OK {
            return;
        }
        unsafe {
            ptr::copy_nonoverlapping(
                borrow_reader(reader).buffer.cast::<u8>(),
                destination.cast::<u8>(),
                needed,
            );
        }
        let state = unsafe { borrow_reader(reader) };
        state.data = state.buffer.wrapping_add(needed);
        state.end = state.buffer.wrapping_add(read);
    } else {
        fill_range(reader, destination, needed, needed);
    }
}

/// Ensures `count` bytes are available, refilling via fill when needed.
///
/// # Safety
///
/// `reader` must be null or point to a live, uniquely writable `mpack_reader_t`
/// with a valid fill buffer when a fill callback is installed.
#[no_mangle]
pub unsafe extern "C" fn mpack_reader_ensure_straddle(
    reader: *mut MpackReader,
    count: usize,
) -> bool {
    if reader.is_null() {
        return false;
    }
    match catch_ffi_panic(|| ensure_straddle_impl(reader, count)) {
        Ok(ok) => ok,
        Err(_) => {
            flag_bug(reader);
            false
        }
    }
}

/// Reads `count` bytes when they straddle the buffer boundary.
///
/// # Safety
///
/// `reader` must be null or point to a live reader. Non-null `p` must be
/// writable for `count` bytes.
#[no_mangle]
pub unsafe extern "C" fn mpack_read_native_straddle(
    reader: *mut MpackReader,
    p: *mut c_char,
    count: usize,
) {
    if reader.is_null() {
        return;
    }
    if catch_ffi_panic(|| read_native_straddle_impl(reader, p, count)).is_err() {
        flag_bug(reader);
    }
}

/// Pretty-prints MessagePack data into a NUL-terminated buffer (debug/stdio).
///
/// # Safety
///
/// When `buffer_size > 0`, `buffer` must be writable for `buffer_size` bytes.
/// When `data_size > 0`, `data` must be readable for `data_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn mpack_print_data_to_buffer(
    data: *const c_char,
    data_size: usize,
    buffer: *mut c_char,
    buffer_size: usize,
) {
    if buffer.is_null() || buffer_size == 0 {
        return;
    }
    let _ = catch_ffi_panic(|| {
        let input = if data.is_null() || data_size == 0 {
            &[][..]
        } else {
            unsafe { slice::from_raw_parts(data.cast::<u8>(), data_size) }
        };
        let mut output = Vec::new();
        let mut core = Reader::new(input);
        print_element(&mut core, &mut output, 0);
        let remaining = core.remaining();
        if core.error() != crate::common::Error::Ok {
            let _ = write!(
                &mut output,
                "\n<mpack parsing error {}>",
                error_name(core.error())
            );
        } else if remaining > 0 {
            let _ = write!(
                &mut output,
                "\n<{remaining} extra bytes at end of message>"
            );
        }

        let copy = output.len().min(buffer_size.saturating_sub(1));
        unsafe {
            if copy > 0 {
                ptr::copy_nonoverlapping(output.as_ptr(), buffer.cast::<u8>(), copy);
            }
            *buffer.add(copy) = 0;
            // Always force a terminator at the end of the buffer (C parity).
            *buffer.add(buffer_size - 1) = 0;
        }
    });
}

/// Pretty-prints MessagePack data to a stdio FILE (C depth-2 indent + trailing newline).
///
/// # Safety
///
/// `file` must be a live `FILE*`. When `len > 0`, `data` must be readable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn mpack_print_data_to_file(
    data: *const c_char,
    len: usize,
    file: *mut c_void,
) {
    if file.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        let input = if data.is_null() || len == 0 {
            &[][..]
        } else {
            unsafe { slice::from_raw_parts(data.cast::<u8>(), len) }
        };
        let mut output = Vec::new();
        // C `mpack_print_data_to_file` starts at depth 2.
        for _ in 0..2 {
            let _ = write!(output, "    ");
        }
        let mut core = Reader::new(input);
        print_element(&mut core, &mut output, 2);
        let remaining = core.remaining();
        if core.error() != crate::common::Error::Ok {
            let _ = write!(
                &mut output,
                "\n<mpack parsing error {}>",
                error_name(core.error())
            );
        } else if remaining > 0 {
            let _ = write!(
                &mut output,
                "\n<{remaining} extra bytes at end of message>"
            );
        }
        output.push(b'\n');
        unsafe {
            fwrite(output.as_ptr().cast(), 1, output.len(), file);
        }
    });
}

fn error_name(error: crate::common::Error) -> &'static str {
    match error {
        crate::common::Error::Ok => "mpack_ok",
        crate::common::Error::Io => "mpack_error_io",
        crate::common::Error::Invalid => "mpack_error_invalid",
        crate::common::Error::Unsupported => "mpack_error_unsupported",
        crate::common::Error::Type => "mpack_error_type",
        crate::common::Error::TooBig => "mpack_error_too_big",
        crate::common::Error::Memory => "mpack_error_memory",
        crate::common::Error::Bug => "mpack_error_bug",
        crate::common::Error::Data => "mpack_error_data",
        crate::common::Error::Eof => "mpack_error_eof",
    }
}

fn write_print_indent(output: &mut Vec<u8>, depth: usize) {
    for _ in 0..depth {
        let _ = write!(output, "    ");
    }
}

fn print_leaf(core: &mut Reader<'_>, output: &mut Vec<u8>, tag: Tag) {
    match tag {
        Tag::Str(length) => {
            let _ = write!(output, "\"");
            if let Some(bytes) = core.read_bytes(length as usize) {
                for &byte in bytes {
                    match byte {
                        b'\n' => {
                            let _ = write!(output, "\\n");
                        }
                        b'\\' => {
                            let _ = write!(output, "\\\\");
                        }
                        b'"' => {
                            let _ = write!(output, "\\\"");
                        }
                        _ => output.push(byte),
                    }
                }
            }
            let _ = write!(output, "\"");
        }
        Tag::Bin(length) => {
            let prefix = read_print_prefix(core, length as usize);
            print_bin_pseudo_json(output, length, &prefix);
        }
        Tag::Ext {
            extension_type,
            length,
        } => {
            let prefix = read_print_prefix(core, length as usize);
            print_ext_pseudo_json(output, extension_type, length, &prefix);
        }
        Tag::Nil => {
            let _ = write!(output, "null");
        }
        Tag::Bool(true) => {
            let _ = write!(output, "true");
        }
        Tag::Bool(false) => {
            let _ = write!(output, "false");
        }
        Tag::Int(value) => {
            let _ = write!(output, "{value}");
        }
        Tag::Uint(value) => {
            let _ = write!(output, "{value}");
        }
        Tag::Float(value) => {
            // Match glibc `%f` style used by C mpack_tag_debug_pseudo_json.
            let _ = write!(output, "{value:.6}");
        }
        Tag::Double(value) => {
            let _ = write!(output, "{value:.6}");
        }
        Tag::Array(_) | Tag::Map(_) => unreachable!("compounds handled by iterative print"),
    }
}

/// Pretty-print one MessagePack value. Iterative (heap frame stack) so hostile
/// deep nesting cannot blow the Rust stack; output matches the prior recursive form.
fn print_element(core: &mut Reader<'_>, output: &mut Vec<u8>, depth: usize) {
    enum MapPhase {
        Key,
        Value,
    }
    enum Frame {
        Array { left: u32, depth: usize },
        Map {
            left: u32,
            depth: usize,
            phase: MapPhase,
        },
    }

    let mut stack: Vec<Frame> = Vec::new();
    let mut current_depth = depth;

    loop {
        let Some(tag) = core.read_tag() else {
            return;
        };

        match tag {
            Tag::Array(count) => {
                let _ = write!(output, "[\n");
                if count == 0 {
                    write_print_indent(output, current_depth);
                    let _ = write!(output, "]");
                } else {
                    write_print_indent(output, current_depth + 1);
                    stack.push(Frame::Array {
                        left: count,
                        depth: current_depth,
                    });
                    current_depth += 1;
                    continue;
                }
            }
            Tag::Map(count) => {
                let _ = write!(output, "{{\n");
                if count == 0 {
                    write_print_indent(output, current_depth);
                    let _ = write!(output, "}}");
                } else {
                    write_print_indent(output, current_depth + 1);
                    stack.push(Frame::Map {
                        left: count,
                        depth: current_depth,
                        phase: MapPhase::Key,
                    });
                    current_depth += 1;
                    continue;
                }
            }
            leaf => print_leaf(core, output, leaf),
        }

        if core.error() != crate::common::Error::Ok {
            return;
        }

        // Advance / close parent frames until another sibling is needed.
        loop {
            let Some(frame) = stack.last_mut() else {
                return;
            };
            match frame {
                Frame::Array { left, depth } => {
                    *left -= 1;
                    if *left > 0 {
                        let _ = write!(output, ",");
                    }
                    let _ = write!(output, "\n");
                    if *left == 0 {
                        let d = *depth;
                        stack.pop();
                        write_print_indent(output, d);
                        let _ = write!(output, "]");
                        continue;
                    }
                    write_print_indent(output, *depth + 1);
                    current_depth = *depth + 1;
                    break;
                }
                Frame::Map { left, depth, phase } => match phase {
                    MapPhase::Key => {
                        let _ = write!(output, ": ");
                        *phase = MapPhase::Value;
                        current_depth = *depth + 1;
                        break;
                    }
                    MapPhase::Value => {
                        *left -= 1;
                        if *left > 0 {
                            let _ = write!(output, ",");
                        }
                        let _ = write!(output, "\n");
                        if *left == 0 {
                            let d = *depth;
                            stack.pop();
                            write_print_indent(output, d);
                            let _ = write!(output, "}}");
                            continue;
                        }
                        *phase = MapPhase::Key;
                        write_print_indent(output, *depth + 1);
                        current_depth = *depth + 1;
                        break;
                    }
                },
            }
        }
    }
}

fn read_print_prefix(core: &mut Reader<'_>, length: usize) -> Vec<u8> {
    if length == 0 {
        return Vec::new();
    }
    let take = length.min(PRINT_BYTE_COUNT);
    let mut prefix = Vec::new();
    if let Some(bytes) = core.read_bytes(take) {
        prefix.extend_from_slice(bytes);
    }
    if length > take {
        let _ = core.skip_bytes(length - take);
    }
    prefix
}

fn print_bin_pseudo_json(output: &mut Vec<u8>, length: u32, prefix: &[u8]) {
    let _ = write!(output, "<binary data of length {length}");
    complete_bin_ext(output, length as usize, prefix);
}

fn print_ext_pseudo_json(output: &mut Vec<u8>, exttype: i8, length: u32, prefix: &[u8]) {
    let _ = write!(output, "<ext data of type {exttype} and length {length}");
    complete_bin_ext(output, length as usize, prefix);
}

fn complete_bin_ext(output: &mut Vec<u8>, total: usize, prefix: &[u8]) {
    if total == 0 {
        let _ = write!(output, ">");
        return;
    }
    let _ = write!(output, ": ");
    let mut hex_bytes = 0usize;
    for &byte in prefix.iter().take(PRINT_BYTE_COUNT) {
        let _ = write!(output, "{byte:02x}");
        hex_bytes += 1;
    }
    if total > hex_bytes {
        let _ = write!(output, "...>");
    } else {
        let _ = write!(output, ">");
    }
}
