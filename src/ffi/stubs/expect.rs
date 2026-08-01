//! Temporary scaffolding; replace body with safe-core calls, do not grow unsafe here.

use std::ffi::{c_char, c_void};
use std::ptr;

use crate::ffi::stubs::util::{flag_reader, stub_alloc_bytes, stub_alloc_cstr};
use crate::ffi::types::{MpackReader, MpackTag, MpackTimestamp};

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_u8(reader: *mut MpackReader) -> u8 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_u8_range(reader: *mut MpackReader, _min: u8, _max: u8) -> u8 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_u16(reader: *mut MpackReader) -> u16 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_u16_range(reader: *mut MpackReader, _min: u16, _max: u16) -> u16 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_u32(reader: *mut MpackReader) -> u32 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_u32_range(reader: *mut MpackReader, _min: u32, _max: u32) -> u32 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_u64(reader: *mut MpackReader) -> u64 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_u64_range(reader: *mut MpackReader, _min: u64, _max: u64) -> u64 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_i8(reader: *mut MpackReader) -> i8 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_i8_range(reader: *mut MpackReader, _min: i8, _max: i8) -> i8 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_i16(reader: *mut MpackReader) -> i16 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_i16_range(reader: *mut MpackReader, _min: i16, _max: i16) -> i16 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_i32(reader: *mut MpackReader) -> i32 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_i32_range(reader: *mut MpackReader, _min: i32, _max: i32) -> i32 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_i64(reader: *mut MpackReader) -> i64 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_i64_range(reader: *mut MpackReader, _min: i64, _max: i64) -> i64 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_float(reader: *mut MpackReader) -> f32 {
    unsafe { flag_reader(reader) };
    0.0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_double(reader: *mut MpackReader) -> f64 {
    unsafe { flag_reader(reader) };
    0.0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_float_strict(reader: *mut MpackReader) -> f32 {
    unsafe { flag_reader(reader) };
    0.0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_double_strict(reader: *mut MpackReader) -> f64 {
    unsafe { flag_reader(reader) };
    0.0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_float_range(reader: *mut MpackReader, _min: f32, _max: f32) -> f32 {
    unsafe { flag_reader(reader) };
    0.0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_double_range(reader: *mut MpackReader, _min: f64, _max: f64) -> f64 {
    unsafe { flag_reader(reader) };
    0.0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_uint_match(reader: *mut MpackReader, _value: u64) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_int_match(reader: *mut MpackReader, _value: i64) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_nil(reader: *mut MpackReader) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_bool(reader: *mut MpackReader) -> bool {
    unsafe { flag_reader(reader) };
    false
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_true(reader: *mut MpackReader) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_false(reader: *mut MpackReader) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_timestamp(reader: *mut MpackReader) -> MpackTimestamp {
    unsafe { flag_reader(reader) };
    MpackTimestamp::default()
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_timestamp_truncate(reader: *mut MpackReader) -> i64 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_map(reader: *mut MpackReader) -> u32 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_map_range(reader: *mut MpackReader, _min: u32, _max: u32) -> u32 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_map_match(reader: *mut MpackReader, _count: u32) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_map_or_nil(reader: *mut MpackReader, count: *mut u32) -> bool {
    unsafe { flag_reader(reader) };
    if !count.is_null() { unsafe { *count = 0 }; }
    false
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_map_max_or_nil(reader: *mut MpackReader, _max: u32, count: *mut u32) -> bool {
    unsafe { flag_reader(reader) };
    if !count.is_null() { unsafe { *count = 0 }; }
    false
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array(reader: *mut MpackReader) -> u32 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array_range(reader: *mut MpackReader, _min: u32, _max: u32) -> u32 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array_match(reader: *mut MpackReader, _count: u32) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array_or_nil(reader: *mut MpackReader, count: *mut u32) -> bool {
    unsafe { flag_reader(reader) };
    if !count.is_null() { unsafe { *count = 0 }; }
    false
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array_max_or_nil(reader: *mut MpackReader, _max: u32, count: *mut u32) -> bool {
    unsafe { flag_reader(reader) };
    if !count.is_null() { unsafe { *count = 0 }; }
    false
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_array_alloc_impl(reader: *mut MpackReader, _element_size: usize, _max_count: u32, out_count: *mut u32, _allow_nil: bool) -> *mut c_void {
    unsafe { flag_reader(reader) };
    if !out_count.is_null() { unsafe { *out_count = 0 }; }
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_str(reader: *mut MpackReader) -> u32 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_str_buf(reader: *mut MpackReader, _buf: *mut c_char, _bufsize: usize) -> usize {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_utf8(reader: *mut MpackReader, _buf: *mut c_char, _bufsize: usize) -> usize {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_str_match(reader: *mut MpackReader, _str: *const c_char, _length: usize) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_cstr(reader: *mut MpackReader, buf: *mut c_char, size: usize) {
    unsafe { flag_reader(reader) };
    if !buf.is_null() && size > 0 { unsafe { *buf = 0 }; }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_utf8_cstr(reader: *mut MpackReader, buf: *mut c_char, size: usize) {
    unsafe { flag_reader(reader) };
    if !buf.is_null() && size > 0 { unsafe { *buf = 0 }; }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_cstr_alloc(reader: *mut MpackReader, _maxsize: usize) -> *mut c_char {
    unsafe { flag_reader(reader) };
    unsafe { stub_alloc_cstr() }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_utf8_cstr_alloc(reader: *mut MpackReader, _maxsize: usize) -> *mut c_char {
    unsafe { flag_reader(reader) };
    unsafe { stub_alloc_cstr() }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_bin(reader: *mut MpackReader) -> u32 {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_bin_buf(reader: *mut MpackReader, _buf: *mut c_char, _size: usize) -> usize {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_bin_size_buf(reader: *mut MpackReader, _buf: *mut c_char, _size: u32) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_bin_alloc(reader: *mut MpackReader, _maxsize: usize, size: *mut usize) -> *mut c_char {
    unsafe { flag_reader(reader) };
    if !size.is_null() { unsafe { *size = 0 }; }
    unsafe { stub_alloc_bytes(1) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_ext(reader: *mut MpackReader, type_: *mut i8) -> u32 {
    unsafe { flag_reader(reader) };
    if !type_.is_null() { unsafe { *type_ = 0 }; }
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_ext_buf(reader: *mut MpackReader, type_: *mut i8, _buf: *mut c_char, _size: usize) -> usize {
    unsafe { flag_reader(reader) };
    if !type_.is_null() { unsafe { *type_ = 0 }; }
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_ext_alloc(reader: *mut MpackReader, type_: *mut i8, _maxsize: usize, size: *mut usize) -> *mut c_char {
    unsafe { flag_reader(reader) };
    if !type_.is_null() { unsafe { *type_ = 0 }; }
    if !size.is_null() { unsafe { *size = 0 }; }
    unsafe { stub_alloc_bytes(1) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_tag(reader: *mut MpackReader, _tag: MpackTag) {
    unsafe { flag_reader(reader) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_key_uint(reader: *mut MpackReader, _found: *mut bool, _count: usize) -> usize {
    unsafe { flag_reader(reader) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_expect_key_cstr(reader: *mut MpackReader, _keys: *const *const c_char, _found: *mut bool, _count: usize) -> usize {
    unsafe { flag_reader(reader) };
    0
}

