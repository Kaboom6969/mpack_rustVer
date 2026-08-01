//! Common C ABI functions required by the embed-writer frozen-test slice.

use std::ffi::{c_char, c_int};
use std::slice;

use crate::ffi::types::MpackTag;

const TYPE_MISSING: i32 = 0;
const TYPE_NIL: i32 = 1;
const TYPE_BOOL: i32 = 2;
const TYPE_INT: i32 = 3;
const TYPE_UINT: i32 = 4;
const TYPE_FLOAT: i32 = 5;
const TYPE_DOUBLE: i32 = 6;
const TYPE_STR: i32 = 7;
const TYPE_BIN: i32 = 8;
const TYPE_ARRAY: i32 = 9;
const TYPE_MAP: i32 = 10;

/// Compares C ABI tags with MPack's numeric normalization rules.
#[no_mangle]
pub extern "C" fn mpack_tag_cmp(mut left: MpackTag, mut right: MpackTag) -> i32 {
    if left.type_ == TYPE_INT && (left.value as i64) >= 0 {
        left.type_ = TYPE_UINT;
    }
    if right.type_ == TYPE_INT && (right.value as i64) >= 0 {
        right.type_ = TYPE_UINT;
    }

    if left.type_ != right.type_ {
        return if left.type_ < right.type_ { -1 } else { 1 };
    }

    match left.type_ {
        TYPE_MISSING | TYPE_NIL => 0,
        TYPE_BOOL => compare_unsigned(left.value & 0xff, right.value & 0xff),
        TYPE_FLOAT | TYPE_STR | TYPE_BIN | TYPE_ARRAY | TYPE_MAP => {
            compare_unsigned(left.value & u32::MAX as u64, right.value & u32::MAX as u64)
        }
        TYPE_DOUBLE => compare_unsigned(left.value, right.value),
        TYPE_INT => compare_signed(left.value as i64, right.value as i64),
        TYPE_UINT => compare_unsigned(left.value, right.value),
        _ => 1,
    }
}

fn compare_unsigned(left: u64, right: u64) -> i32 {
    if left == right {
        0
    } else if left < right {
        -1
    } else {
        1
    }
}

fn compare_signed(left: i64, right: i64) -> i32 {
    if left == right {
        0
    } else if left < right {
        -1
    } else {
        1
    }
}

/// Returns the upstream spelling of an error constant.
#[no_mangle]
pub extern "C" fn mpack_error_to_string(error: c_int) -> *const c_char {
    match error {
        0 => c"mpack_ok".as_ptr(),
        2 => c"mpack_error_io".as_ptr(),
        3 => c"mpack_error_invalid".as_ptr(),
        4 => c"mpack_error_unsupported".as_ptr(),
        5 => c"mpack_error_type".as_ptr(),
        6 => c"mpack_error_too_big".as_ptr(),
        7 => c"mpack_error_memory".as_ptr(),
        8 => c"mpack_error_bug".as_ptr(),
        9 => c"mpack_error_data".as_ptr(),
        10 => c"mpack_error_eof".as_ptr(),
        _ => c"(unknown mpack_error_t)".as_ptr(),
    }
}

/// Returns the upstream spelling of a tag type constant.
#[no_mangle]
pub extern "C" fn mpack_type_to_string(type_: c_int) -> *const c_char {
    match type_ {
        TYPE_MISSING => c"mpack_type_missing".as_ptr(),
        TYPE_NIL => c"mpack_type_nil".as_ptr(),
        TYPE_BOOL => c"mpack_type_bool".as_ptr(),
        TYPE_FLOAT => c"mpack_type_float".as_ptr(),
        TYPE_DOUBLE => c"mpack_type_double".as_ptr(),
        TYPE_INT => c"mpack_type_int".as_ptr(),
        TYPE_UINT => c"mpack_type_uint".as_ptr(),
        TYPE_STR => c"mpack_type_str".as_ptr(),
        TYPE_BIN => c"mpack_type_bin".as_ptr(),
        TYPE_ARRAY => c"mpack_type_array".as_ptr(),
        TYPE_MAP => c"mpack_type_map".as_ptr(),
        _ => c"(unknown mpack_type_t)".as_ptr(),
    }
}

/// Validates UTF-8 in a C byte buffer.
#[no_mangle]
pub unsafe extern "C" fn mpack_utf8_check(data: *const c_char, count: usize) -> bool {
    check_utf8(data, count, false)
}

/// Validates UTF-8 and rejects embedded NUL bytes.
#[no_mangle]
pub unsafe extern "C" fn mpack_utf8_check_no_null(data: *const c_char, count: usize) -> bool {
    check_utf8(data, count, true)
}

fn check_utf8(data: *const c_char, count: usize, reject_null: bool) -> bool {
    if count == 0 {
        return true;
    }
    if data.is_null() {
        return false;
    }
    // SAFETY: The C ABI requires a non-null pointer to `count` readable bytes.
    let bytes = unsafe { slice::from_raw_parts(data.cast::<u8>(), count) };
    std::str::from_utf8(bytes).is_ok() && (!reject_null || !bytes.contains(&0))
}
