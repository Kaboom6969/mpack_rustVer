//! C ABI boundary for Expect (`full-suite-abi`).
//!
//! Thin wrappers: null-check + `catch_ffi_panic`, then shared helpers. Pointer
//! work lives in a few helpers / `read_with_core`; algorithm stays in
//! `crate::expect` (`forbid(unsafe_code)`).

use std::ffi::{c_char, c_void, CStr};
use std::ptr;
use std::slice;

use crate::common::Timestamp;
use crate::expect::{self, ExpectCompound};
use crate::ffi::common::mpack_tag_cmp;
use crate::ffi::guard::catch_ffi_panic;
use crate::ffi::reader::{
    borrow_reader, done_type_impl, ensure_tag_header, flag_bug, flag_error_impl, flag_error_on,
    read_native, read_with_core, reader_error,
};
use crate::ffi::stubs::track::{self, track_element, track_push};
use crate::ffi::types::{
    MpackReader, MpackTag, MpackTimestamp, MPACK_ERROR_BUG, MPACK_ERROR_INVALID,
    MPACK_ERROR_MEMORY, MPACK_ERROR_TOO_BIG, MPACK_ERROR_TYPE, MPACK_OK,
};
use crate::reader::{self, Reader};

const TYPE_UINT: i32 = 4;
const TYPE_STR: i32 = 7;
const TYPE_BIN: i32 = 8;
const TYPE_ARRAY: i32 = 9;
const TYPE_MAP: i32 = 10;
const TYPE_EXT: i32 = 11;

unsafe extern "C" {
    fn test_malloc(size: usize) -> *mut c_void;
    fn test_free(pointer: *mut c_void);
    /// Provided by the frozen suite under `MPACK_CUSTOM_ASSERT`.
    fn mpack_assert_fail(message: *const c_char);
    fn mpack_break_hit(message: *const c_char);
}

fn assert_fail(message: &[u8]) {
    // SAFETY: Suite provides `mpack_assert_fail` (longjmp in unit tests).
    unsafe {
        mpack_assert_fail(message.as_ptr().cast());
    }
}

fn break_hit(message: &[u8]) {
    // SAFETY: Suite provides `mpack_break_hit`.
    unsafe {
        mpack_break_hit(message.as_ptr().cast());
    }
}

fn expect_entry<T>(
    reader: *mut MpackReader,
    on_null: T,
    on_panic: T,
    body: impl FnOnce() -> T,
) -> T {
    if reader.is_null() {
        return on_null;
    }
    match catch_ffi_panic(body) {
        Ok(value) => value,
        Err(_) => {
            flag_bug(reader);
            on_panic
        }
    }
}

fn write_out<T: Copy>(pointer: *mut T, value: T) {
    if !pointer.is_null() {
        // SAFETY: Caller out-param is null or writable for one `T`.
        unsafe {
            *pointer = value;
        }
    }
}

fn error_of(reader: *mut MpackReader) -> i32 {
    // SAFETY: Caller null-checked reader.
    unsafe { reader_error(reader) }
}

fn track_element_or_flag(reader: *mut MpackReader) -> bool {
    // SAFETY: Caller null-checked `reader`.
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return false;
    }
    let error = track_element(&mut state.track);
    if error != MPACK_OK {
        flag_error_on(state, reader, error);
        return false;
    }
    true
}

fn push_type(reader: *mut MpackReader, type_: i32, count: u32) {
    // SAFETY: Caller null-checked reader.
    let state = unsafe { borrow_reader(reader) };
    if state.error != MPACK_OK {
        return;
    }
    let error = track_push(&mut state.track, type_, count);
    if error != MPACK_OK {
        flag_error_on(state, reader, error);
    }
}

fn prepare_scalar(reader: *mut MpackReader) -> bool {
    track_element_or_flag(reader) && ensure_tag_header(reader)
}

fn expect_option<T: Copy>(
    reader: *mut MpackReader,
    zero: T,
    mut op: impl FnMut(&mut Reader<'_>) -> Option<T>,
) -> T {
    if !prepare_scalar(reader) {
        return zero;
    }
    read_with_core(reader, |core| op(core).unwrap_or(zero), || zero)
}

fn expect_void(reader: *mut MpackReader, mut op: impl FnMut(&mut Reader<'_>)) {
    if !prepare_scalar(reader) {
        return;
    }
    read_with_core(reader, |core| op(core), || ())
}

fn compound_open(
    reader: *mut MpackReader,
    type_: i32,
    mut op: impl FnMut(&mut Reader<'_>) -> Option<u32>,
) -> u32 {
    if !prepare_scalar(reader) {
        return 0;
    }
    let count = read_with_core(reader, |core| op(core).unwrap_or(0), || 0);
    if error_of(reader) == MPACK_OK {
        push_type(reader, type_, count);
    }
    count
}

fn compound_or_nil_open(
    reader: *mut MpackReader,
    type_: i32,
    count_out: *mut u32,
    mut op: impl FnMut(&mut Reader<'_>) -> Option<ExpectCompound>,
) -> bool {
    if !prepare_scalar(reader) {
        write_out(count_out, 0);
        return false;
    }
    let result = read_with_core(
        reader,
        |core| {
            op(core).unwrap_or(ExpectCompound {
                is_nil: true,
                count: 0,
            })
        },
        || ExpectCompound {
            is_nil: true,
            count: 0,
        },
    );
    write_out(count_out, result.count);
    if error_of(reader) != MPACK_OK {
        return false;
    }
    if result.is_nil {
        return false;
    }
    push_type(reader, type_, result.count);
    true
}

fn c_bytes<'a>(data: *const c_char, count: usize) -> Option<&'a [u8]> {
    if count == 0 {
        return Some(&[]);
    }
    if data.is_null() {
        return None;
    }
    // SAFETY: Caller guarantees `data` points at `count` readable bytes.
    Some(unsafe { slice::from_raw_parts(data.cast::<u8>(), count) })
}

fn cstr_bytes<'a>(pointer: *const c_char) -> Option<&'a [u8]> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: Caller guarantees a live NUL-terminated C string.
    Some(unsafe { CStr::from_ptr(pointer) }.to_bytes())
}

fn key_cstr_table<'a>(keys: *const *const c_char, count: usize) -> Option<Vec<&'a str>> {
    if count == 0 {
        return Some(Vec::new());
    }
    if keys.is_null() {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        // SAFETY: `keys` points at `count` pointers from the C caller.
        let pointer = unsafe { *keys.add(index) };
        let bytes = cstr_bytes(pointer)?;
        let text = std::str::from_utf8(bytes).ok()?;
        out.push(text);
    }
    Some(out)
}

fn found_slice<'a>(found: *mut bool, count: usize) -> Option<&'a mut [bool]> {
    if count == 0 {
        return Some(&mut []);
    }
    if found.is_null() {
        return None;
    }
    // SAFETY: Caller guarantees writable `count` bools.
    Some(unsafe { slice::from_raw_parts_mut(found, count) })
}

fn timestamp_to_abi(value: Timestamp) -> MpackTimestamp {
    MpackTimestamp {
        seconds: value.seconds,
        nanoseconds: value.nanoseconds,
    }
}

fn suite_alloc(size: usize) -> *mut u8 {
    // SAFETY: Suite allocator; returns null on OOM.
    unsafe { test_malloc(size.max(1)) }.cast()
}

fn suite_free(pointer: *mut c_void) {
    if !pointer.is_null() {
        // SAFETY: Pointer came from suite/`test_malloc` (reader alloc path).
        unsafe { test_free(pointer) };
    }
}

fn read_bytes_tracked(reader: *mut MpackReader, buf: *mut c_char, count: usize) {
    // SAFETY: Same contract as `mpack_read_bytes`.
    unsafe {
        crate::ffi::reader::mpack_read_bytes(reader, buf, count);
    }
}

fn expect_str_impl(reader: *mut MpackReader) -> u32 {
    compound_open(reader, TYPE_STR, expect::r#str)
}

fn expect_bin_impl(reader: *mut MpackReader) -> u32 {
    compound_open(reader, TYPE_BIN, expect::bin)
}

fn expect_array_max_or_nil_impl(
    reader: *mut MpackReader,
    max_value: u32,
    count: *mut u32,
) -> bool {
    compound_or_nil_open(reader, TYPE_ARRAY, count, |core| {
        expect::array_max_or_nil(core, max_value)
    })
}

macro_rules! expect_op {
    ($export:ident => $core:path, $ty:ty, $zero:expr) => {
        #[no_mangle]
        pub unsafe extern "C" fn $export(reader: *mut MpackReader) -> $ty {
            expect_entry(reader, $zero, $zero, || {
                expect_option(reader, $zero, |core| $core(core))
            })
        }
    };
}

macro_rules! expect_range_op {
    ($export:ident => $core:path, $ty:ty, $zero:expr) => {
        #[no_mangle]
        pub unsafe extern "C" fn $export(
            reader: *mut MpackReader,
            min_value: $ty,
            max_value: $ty,
        ) -> $ty {
            expect_entry(reader, min_value, min_value, || {
                if min_value > max_value {
                    assert_fail(b"min_value must be less than or equal to max_value\0");
                    flag_error_impl(reader, MPACK_ERROR_BUG);
                    return min_value;
                }
                if !prepare_scalar(reader) {
                    return min_value;
                }
                read_with_core(
                    reader,
                    |core| $core(core, min_value, max_value).unwrap_or(min_value),
                    || min_value,
                )
            })
        }
    };
}

expect_op!(mpack_expect_u8 => expect::u8, u8, 0u8);
expect_range_op!(mpack_expect_u8_range => expect::u8_range, u8, 0u8);
expect_op!(mpack_expect_u16 => expect::u16, u16, 0u16);
expect_range_op!(mpack_expect_u16_range => expect::u16_range, u16, 0u16);
expect_op!(mpack_expect_u32 => expect::u32, u32, 0u32);
expect_range_op!(mpack_expect_u32_range => expect::u32_range, u32, 0u32);
expect_op!(mpack_expect_u64 => expect::u64, u64, 0u64);
expect_range_op!(mpack_expect_u64_range => expect::u64_range, u64, 0u64);
expect_op!(mpack_expect_i8 => expect::i8, i8, 0i8);
expect_range_op!(mpack_expect_i8_range => expect::i8_range, i8, 0i8);
expect_op!(mpack_expect_i16 => expect::i16, i16, 0i16);
expect_range_op!(mpack_expect_i16_range => expect::i16_range, i16, 0i16);
expect_op!(mpack_expect_i32 => expect::i32, i32, 0i32);
expect_range_op!(mpack_expect_i32_range => expect::i32_range, i32, 0i32);
expect_op!(mpack_expect_i64 => expect::i64, i64, 0i64);
expect_range_op!(mpack_expect_i64_range => expect::i64_range, i64, 0i64);
expect_op!(mpack_expect_float => expect::float, f32, 0.0f32);
expect_op!(mpack_expect_double => expect::double, f64, 0.0f64);
expect_op!(mpack_expect_float_strict => expect::float_strict, f32, 0.0f32);
expect_op!(mpack_expect_double_strict => expect::double_strict, f64, 0.0f64);
expect_range_op!(mpack_expect_float_range => expect::float_range, f32, 0.0f32);
expect_range_op!(mpack_expect_double_range => expect::double_range, f64, 0.0f64);

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_uint_match(reader: *mut MpackReader, value: u64) {
    expect_entry(reader, (), (), || {
        expect_void(reader, |core| {
            let _ = expect::uint_match(core, value);
        })
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_int_match(reader: *mut MpackReader, value: i64) {
    expect_entry(reader, (), (), || {
        expect_void(reader, |core| {
            let _ = expect::int_match(core, value);
        })
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_nil(reader: *mut MpackReader) {
    expect_entry(reader, (), (), || {
        expect_void(reader, |core| {
            let _ = expect::nil(core);
        })
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_bool(reader: *mut MpackReader) -> bool {
    expect_entry(reader, false, false, || {
        expect_option(reader, false, |core| expect::r#bool(core))
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_true(reader: *mut MpackReader) {
    expect_entry(reader, (), (), || {
        expect_void(reader, |core| {
            let _ = expect::true_(core);
        })
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_false(reader: *mut MpackReader) {
    expect_entry(reader, (), (), || {
        expect_void(reader, |core| {
            let _ = expect::false_(core);
        })
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_timestamp(reader: *mut MpackReader) -> MpackTimestamp {
    let zero = MpackTimestamp::default();
    expect_entry(reader, zero, zero, || {
        if !prepare_scalar(reader) {
            return zero;
        }
        read_with_core(
            reader,
            |core| {
                expect::timestamp(core)
                    .map(timestamp_to_abi)
                    .unwrap_or(zero)
            },
            || zero,
        )
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_timestamp_truncate(reader: *mut MpackReader) -> i64 {
    expect_entry(reader, 0, 0, || {
        expect_option(reader, 0, |core| expect::timestamp_truncate(core))
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_map(reader: *mut MpackReader) -> u32 {
    expect_entry(reader, 0, 0, || compound_open(reader, TYPE_MAP, expect::map))
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_map_range(
    reader: *mut MpackReader,
    min_value: u32,
    max_value: u32,
) -> u32 {
    expect_entry(reader, min_value, min_value, || {
        if min_value > max_value {
            assert_fail(b"min_value must be less than or equal to max_value\0");
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return min_value;
        }
        if !prepare_scalar(reader) {
            return min_value;
        }
        let count = read_with_core(
            reader,
            |core| expect::map_range(core, min_value, max_value).unwrap_or(min_value),
            || min_value,
        );
        if error_of(reader) == MPACK_OK {
            push_type(reader, TYPE_MAP, count);
        }
        count
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_map_match(reader: *mut MpackReader, count: u32) {
    expect_entry(reader, (), (), || {
        if !prepare_scalar(reader) {
            return;
        }
        let matched = read_with_core(reader, |core| expect::map_match(core, count), || false);
        if matched && error_of(reader) == MPACK_OK {
            push_type(reader, TYPE_MAP, count);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_map_or_nil(
    reader: *mut MpackReader,
    count: *mut u32,
) -> bool {
    expect_entry(reader, false, false, || {
        compound_or_nil_open(reader, TYPE_MAP, count, expect::map_or_nil)
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_map_max_or_nil(
    reader: *mut MpackReader,
    max_value: u32,
    count: *mut u32,
) -> bool {
    expect_entry(reader, false, false, || {
        compound_or_nil_open(reader, TYPE_MAP, count, |core| {
            expect::map_max_or_nil(core, max_value)
        })
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array(reader: *mut MpackReader) -> u32 {
    expect_entry(reader, 0, 0, || {
        compound_open(reader, TYPE_ARRAY, expect::array)
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array_range(
    reader: *mut MpackReader,
    min_value: u32,
    max_value: u32,
) -> u32 {
    expect_entry(reader, min_value, min_value, || {
        if min_value > max_value {
            assert_fail(b"min_value must be less than or equal to max_value\0");
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return min_value;
        }
        if !prepare_scalar(reader) {
            return min_value;
        }
        let count = read_with_core(
            reader,
            |core| expect::array_range(core, min_value, max_value).unwrap_or(min_value),
            || min_value,
        );
        if error_of(reader) == MPACK_OK {
            push_type(reader, TYPE_ARRAY, count);
        }
        count
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array_match(reader: *mut MpackReader, count: u32) {
    expect_entry(reader, (), (), || {
        if !prepare_scalar(reader) {
            return;
        }
        let matched = read_with_core(reader, |core| expect::array_match(core, count), || false);
        if matched && error_of(reader) == MPACK_OK {
            push_type(reader, TYPE_ARRAY, count);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array_or_nil(
    reader: *mut MpackReader,
    count: *mut u32,
) -> bool {
    expect_entry(reader, false, false, || {
        compound_or_nil_open(reader, TYPE_ARRAY, count, expect::array_or_nil)
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array_max_or_nil(
    reader: *mut MpackReader,
    max_value: u32,
    count: *mut u32,
) -> bool {
    expect_entry(reader, false, false, || {
        expect_array_max_or_nil_impl(reader, max_value, count)
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array_alloc_impl(
    reader: *mut MpackReader,
    element_size: usize,
    max_count: u32,
    out_count: *mut u32,
    allow_nil: bool,
) -> *mut c_void {
    expect_entry(reader, ptr::null_mut(), ptr::null_mut(), || {
        write_out(out_count, 0);
        if out_count.is_null() {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return ptr::null_mut();
        }
        let mut count = 0u32;
        let has_array = if allow_nil {
            expect_array_max_or_nil_impl(reader, max_count, &mut count)
        } else {
            count = compound_open(reader, TYPE_ARRAY, |core| {
                expect::array_range(core, 0, max_count)
            });
            error_of(reader) == MPACK_OK
        };
        if error_of(reader) != MPACK_OK {
            return ptr::null_mut();
        }
        if count == 0 {
            if allow_nil && has_array {
                done_type_impl(reader, TYPE_ARRAY);
            }
            return ptr::null_mut();
        }
        let Some(bytes) = element_size.checked_mul(count as usize) else {
            flag_error_impl(reader, MPACK_ERROR_TOO_BIG);
            return ptr::null_mut();
        };
        let pointer = suite_alloc(bytes).cast::<c_void>();
        if pointer.is_null() {
            flag_error_impl(reader, MPACK_ERROR_MEMORY);
            return ptr::null_mut();
        }
        // SAFETY: Fresh suite allocation of `bytes` bytes.
        unsafe {
            ptr::write_bytes(pointer.cast::<u8>(), 0, bytes);
        }
        write_out(out_count, count);
        pointer
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_str(reader: *mut MpackReader) -> u32 {
    expect_entry(reader, 0, 0, || expect_str_impl(reader))
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_str_buf(
    reader: *mut MpackReader,
    buf: *mut c_char,
    bufsize: usize,
) -> usize {
    expect_entry(reader, 0, 0, || {
        let length = expect_str_impl(reader) as usize;
        if error_of(reader) != MPACK_OK {
            return 0;
        }
        if length > bufsize {
            flag_error_impl(reader, MPACK_ERROR_TOO_BIG);
            return 0;
        }
        if length > 0 && buf.is_null() {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return 0;
        }
        read_bytes_tracked(reader, buf, length);
        if error_of(reader) != MPACK_OK {
            return 0;
        }
        done_type_impl(reader, TYPE_STR);
        length
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_utf8(
    reader: *mut MpackReader,
    buf: *mut c_char,
    bufsize: usize,
) -> usize {
    expect_entry(reader, 0, 0, || {
        let length = expect_str_impl(reader) as usize;
        if error_of(reader) != MPACK_OK {
            return 0;
        }
        if length > bufsize {
            flag_error_impl(reader, MPACK_ERROR_TOO_BIG);
            return 0;
        }
        if length > 0 && buf.is_null() {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return 0;
        }
        read_bytes_tracked(reader, buf, length);
        if error_of(reader) != MPACK_OK {
            return 0;
        }
        done_type_impl(reader, TYPE_STR);
        // SAFETY: `buf` holds `length` bytes just written.
        let bytes = unsafe { slice::from_raw_parts(buf.cast::<u8>(), length) };
        if !reader::check_utf8(bytes) {
            flag_error_impl(reader, MPACK_ERROR_TYPE);
            return 0;
        }
        length
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_str_match(
    reader: *mut MpackReader,
    string: *const c_char,
    length: usize,
) {
    expect_entry(reader, (), (), || {
        if length > u32::MAX as usize {
            flag_error_impl(reader, MPACK_ERROR_TYPE);
            return;
        }
        let Some(expected) = c_bytes(string, length) else {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return;
        };
        let got = expect_str_impl(reader);
        if error_of(reader) != MPACK_OK {
            return;
        }
        if got as usize != length {
            flag_error_impl(reader, MPACK_ERROR_TYPE);
            return;
        }
        // SAFETY: Caller null-checked reader.
        let state = unsafe { borrow_reader(reader) };
        let track_error = track::track_bytes(&mut state.track, length);
        if track_error != MPACK_OK {
            flag_error_on(state, reader, track_error);
            return;
        }
        for &byte in expected {
            let mut temp = 0u8;
            read_native(reader, (&mut temp as *mut u8).cast(), 1);
            if error_of(reader) != MPACK_OK {
                return;
            }
            if temp != byte {
                flag_error_impl(reader, MPACK_ERROR_TYPE);
                return;
            }
        }
        done_type_impl(reader, TYPE_STR);
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_cstr(
    reader: *mut MpackReader,
    buf: *mut c_char,
    size: usize,
) {
    expect_entry(reader, (), (), || {
        if size < 1 {
            assert_fail(b"buffer size is zero; you must have room for at least a null-terminator\0");
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return;
        }
        let length = expect_str_impl(reader) as usize;
        // SAFETY: Same contract as the public cstr reader export.
        unsafe {
            crate::ffi::reader::mpack_read_cstr(reader, buf, size, length);
        }
        if error_of(reader) == MPACK_OK {
            done_type_impl(reader, TYPE_STR);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_utf8_cstr(
    reader: *mut MpackReader,
    buf: *mut c_char,
    size: usize,
) {
    expect_entry(reader, (), (), || {
        if size < 1 {
            assert_fail(b"buffer size is zero; you must have room for at least a null-terminator\0");
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return;
        }
        let length = expect_str_impl(reader) as usize;
        // SAFETY: Same contract as the public utf8_cstr reader export.
        unsafe {
            crate::ffi::reader::mpack_read_utf8_cstr(reader, buf, size, length);
        }
        if error_of(reader) == MPACK_OK {
            done_type_impl(reader, TYPE_STR);
        }
    });
}

fn expect_cstr_alloc_unchecked(
    reader: *mut MpackReader,
    maxsize: usize,
    out_length: &mut usize,
) -> *mut c_char {
    *out_length = 0;
    if maxsize < 1 {
        break_hit(b"maxsize is zero; you must have room for at least a null-terminator\0");
        flag_error_impl(reader, MPACK_ERROR_BUG);
        return ptr::null_mut();
    }
    let max_payload = (maxsize - 1).min(u32::MAX as usize) as u32;
    let length = compound_open(reader, TYPE_STR, |core| {
        expect::r#str(core).and_then(|n| {
            if n <= max_payload {
                Some(n)
            } else {
                core.flag_error(crate::common::Error::TooBig);
                None
            }
        })
    }) as usize;
    if error_of(reader) != MPACK_OK {
        return ptr::null_mut();
    }
    // SAFETY: Same contract as the public alloc export.
    let pointer =
        unsafe { crate::ffi::reader::mpack_read_bytes_alloc_impl(reader, length, true) };
    done_type_impl(reader, TYPE_STR);
    if !pointer.is_null() {
        *out_length = length;
    }
    pointer
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_cstr_alloc(
    reader: *mut MpackReader,
    maxsize: usize,
) -> *mut c_char {
    expect_entry(reader, ptr::null_mut(), ptr::null_mut(), || {
        let mut length = 0usize;
        let pointer = expect_cstr_alloc_unchecked(reader, maxsize, &mut length);
        if pointer.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: Alloc returned `length` payload bytes.
        let bytes = unsafe { slice::from_raw_parts(pointer.cast::<u8>(), length) };
        if bytes.contains(&0) {
            suite_free(pointer.cast());
            flag_error_impl(reader, MPACK_ERROR_TYPE);
            return ptr::null_mut();
        }
        pointer
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_utf8_cstr_alloc(
    reader: *mut MpackReader,
    maxsize: usize,
) -> *mut c_char {
    expect_entry(reader, ptr::null_mut(), ptr::null_mut(), || {
        let mut length = 0usize;
        let pointer = expect_cstr_alloc_unchecked(reader, maxsize, &mut length);
        if pointer.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: Alloc returned `length` payload bytes.
        let bytes = unsafe { slice::from_raw_parts(pointer.cast::<u8>(), length) };
        if !reader::check_utf8(bytes) || bytes.contains(&0) {
            suite_free(pointer.cast());
            flag_error_impl(reader, MPACK_ERROR_TYPE);
            return ptr::null_mut();
        }
        pointer
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_bin(reader: *mut MpackReader) -> u32 {
    expect_entry(reader, 0, 0, || expect_bin_impl(reader))
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_bin_buf(
    reader: *mut MpackReader,
    buf: *mut c_char,
    size: usize,
) -> usize {
    expect_entry(reader, 0, 0, || {
        let length = expect_bin_impl(reader) as usize;
        if error_of(reader) != MPACK_OK {
            return 0;
        }
        if length > size {
            flag_error_impl(reader, MPACK_ERROR_TOO_BIG);
            return 0;
        }
        if length > 0 && buf.is_null() {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return 0;
        }
        read_bytes_tracked(reader, buf, length);
        if error_of(reader) != MPACK_OK {
            return 0;
        }
        done_type_impl(reader, TYPE_BIN);
        length
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_bin_size_buf(
    reader: *mut MpackReader,
    buf: *mut c_char,
    size: u32,
) {
    expect_entry(reader, (), (), || {
        let length = expect_bin_impl(reader);
        if error_of(reader) != MPACK_OK {
            return;
        }
        if length != size {
            flag_error_impl(reader, MPACK_ERROR_TYPE);
            return;
        }
        if size > 0 && buf.is_null() {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return;
        }
        read_bytes_tracked(reader, buf, size as usize);
        if error_of(reader) == MPACK_OK {
            done_type_impl(reader, TYPE_BIN);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_bin_alloc(
    reader: *mut MpackReader,
    maxsize: usize,
    size: *mut usize,
) -> *mut c_char {
    expect_entry(reader, ptr::null_mut(), ptr::null_mut(), || {
        write_out(size, 0);
        let max_payload = maxsize.min(u32::MAX as usize) as u32;
        let length = compound_open(reader, TYPE_BIN, |core| {
            expect::bin(core).and_then(|n| {
                if n <= max_payload {
                    Some(n)
                } else {
                    core.flag_error(crate::common::Error::Type);
                    None
                }
            })
        }) as usize;
        if error_of(reader) != MPACK_OK {
            return ptr::null_mut();
        }
        let pointer =
            unsafe { crate::ffi::reader::mpack_read_bytes_alloc_impl(reader, length, false) };
        done_type_impl(reader, TYPE_BIN);
        if !pointer.is_null() {
            write_out(size, length);
        }
        pointer
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_ext(reader: *mut MpackReader, type_: *mut i8) -> u32 {
    expect_entry(reader, 0, 0, || {
        if !prepare_scalar(reader) {
            write_out(type_, 0);
            return 0;
        }
        let result = read_with_core(
            reader,
            |core| expect::ext(core).unwrap_or((0, 0)),
            || (0, 0),
        );
        write_out(type_, result.0);
        if error_of(reader) == MPACK_OK {
            push_type(reader, TYPE_EXT, result.1);
        } else {
            write_out(type_, 0);
        }
        result.1
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_ext_buf(
    reader: *mut MpackReader,
    type_: *mut i8,
    buf: *mut c_char,
    size: usize,
) -> usize {
    expect_entry(reader, 0, 0, || {
        let length = {
            if !prepare_scalar(reader) {
                write_out(type_, 0);
                return 0;
            }
            let result = read_with_core(
                reader,
                |core| expect::ext(core).unwrap_or((0, 0)),
                || (0, 0),
            );
            write_out(type_, result.0);
            if error_of(reader) == MPACK_OK {
                push_type(reader, TYPE_EXT, result.1);
            } else {
                write_out(type_, 0);
                return 0;
            }
            result.1 as usize
        };
        if length > size {
            write_out(type_, 0);
            flag_error_impl(reader, MPACK_ERROR_TOO_BIG);
            return 0;
        }
        if length > 0 && buf.is_null() {
            write_out(type_, 0);
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return 0;
        }
        read_bytes_tracked(reader, buf, length);
        if error_of(reader) != MPACK_OK {
            write_out(type_, 0);
            return 0;
        }
        done_type_impl(reader, TYPE_EXT);
        length
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_ext_alloc(
    reader: *mut MpackReader,
    type_: *mut i8,
    maxsize: usize,
    size: *mut usize,
) -> *mut c_char {
    expect_entry(reader, ptr::null_mut(), ptr::null_mut(), || {
        write_out(size, 0);
        let max_payload = maxsize.min(u32::MAX as usize) as u32;
        if !prepare_scalar(reader) {
            write_out(type_, 0);
            return ptr::null_mut();
        }
        let result = read_with_core(
            reader,
            |core| {
                expect::ext(core).and_then(|(ext_type, length)| {
                    if length <= max_payload {
                        Some((ext_type, length))
                    } else {
                        core.flag_error(crate::common::Error::Type);
                        None
                    }
                })
            },
            || None,
        );
        let Some((ext_type, length)) = result else {
            write_out(type_, 0);
            return ptr::null_mut();
        };
        write_out(type_, ext_type);
        push_type(reader, TYPE_EXT, length);
        let pointer = unsafe {
            crate::ffi::reader::mpack_read_bytes_alloc_impl(reader, length as usize, false)
        };
        done_type_impl(reader, TYPE_EXT);
        if pointer.is_null() {
            write_out(type_, 0);
            return ptr::null_mut();
        }
        write_out(size, length as usize);
        pointer
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_tag(reader: *mut MpackReader, expected: MpackTag) {
    expect_entry(reader, (), (), || {
        // SAFETY: Public read_tag export; reader already null-checked.
        let actual = unsafe { crate::ffi::reader::mpack_read_tag(reader) };
        if error_of(reader) != MPACK_OK {
            return;
        }
        if mpack_tag_cmp(actual, expected) != 0 {
            flag_error_impl(reader, MPACK_ERROR_TYPE);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_key_uint(
    reader: *mut MpackReader,
    found: *mut bool,
    count: usize,
) -> usize {
    expect_entry(reader, count, count, || {
        if error_of(reader) != MPACK_OK {
            return count;
        }
        if count == 0 {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return count;
        }
        let Some(found_slice) = found_slice(found, count) else {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return count;
        };
        // SAFETY: Public peek_tag export.
        let peeked = unsafe { crate::ffi::reader::mpack_peek_tag(reader) };
        if error_of(reader) != MPACK_OK {
            return count;
        }
        if peeked.type_ != TYPE_UINT {
            unsafe { crate::ffi::reader::mpack_discard(reader) };
            return count;
        }
        let value = expect_option(reader, 0u64, |core| expect::u64(core));
        if error_of(reader) != MPACK_OK {
            return count;
        }
        if value >= count as u64 {
            return count;
        }
        let index = value as usize;
        if found_slice[index] {
            flag_error_impl(reader, MPACK_ERROR_INVALID);
            return count;
        }
        found_slice[index] = true;
        index
    })
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_key_cstr(
    reader: *mut MpackReader,
    keys: *const *const c_char,
    found: *mut bool,
    count: usize,
) -> usize {
    expect_entry(reader, count, count, || {
        if error_of(reader) != MPACK_OK {
            return count;
        }
        let Some(key_table) = key_cstr_table(keys, count) else {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return count;
        };
        let Some(found_slice) = found_slice(found, count) else {
            flag_error_impl(reader, MPACK_ERROR_BUG);
            return count;
        };
        let peeked = unsafe { crate::ffi::reader::mpack_peek_tag(reader) };
        if error_of(reader) != MPACK_OK {
            return count;
        }
        if peeked.type_ != TYPE_STR {
            unsafe { crate::ffi::reader::mpack_discard(reader) };
            return count;
        }
        let keylen = expect_str_impl(reader) as usize;
        if error_of(reader) != MPACK_OK {
            return count;
        }
        let key_ptr = unsafe { crate::ffi::reader::mpack_read_bytes_inplace(reader, keylen) };
        done_type_impl(reader, TYPE_STR);
        if error_of(reader) != MPACK_OK || (keylen > 0 && key_ptr.is_null()) {
            return count;
        }
        let key_bytes = if keylen == 0 {
            &[][..]
        } else {
            // SAFETY: inplace pointer covers `keylen` bytes until next mutate.
            unsafe { slice::from_raw_parts(key_ptr.cast::<u8>(), keylen) }
        };
        for (index, key) in key_table.iter().enumerate() {
            if key.as_bytes() == key_bytes {
                if found_slice[index] {
                    flag_error_impl(reader, MPACK_ERROR_INVALID);
                    return count;
                }
                found_slice[index] = true;
                return index;
            }
        }
        count
    })
}
