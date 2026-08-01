//! Temporary scaffolding; replace body with safe-core calls, do not grow unsafe here.

use std::ffi::{c_char, c_int, c_uint, c_void};

use crate::ffi::stubs::util::{
    destroy_tree, flag_node, flag_tree, init_tree, nil_from, nil_node, stub_alloc_bytes,
    stub_alloc_cstr, stub_bytes,
};
use crate::ffi::types::{
    MpackError, MpackNode, MpackNodeData, MpackTag, MpackTimestamp, MpackTree, MpackTreeRead,
};

#[no_mangle]
pub unsafe extern "C" fn mpack_tree_init_data(tree: *mut MpackTree, _data: *const c_char, _length: usize) {
    unsafe { init_tree(tree) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_tree_init_stream(tree: *mut MpackTree, _read_fn: MpackTreeRead, _context: *mut c_void, _max_size: usize, _max_nodes: usize) {
    unsafe { init_tree(tree) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_tree_init_pool(tree: *mut MpackTree, _data: *const c_char, _length: usize, _pool: *mut MpackNodeData, _pool_count: usize) {
    unsafe { init_tree(tree) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_tree_init_filename(tree: *mut MpackTree, _filename: *const c_char, _max_bytes: usize) {
    unsafe { init_tree(tree) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_tree_init_stdfile(tree: *mut MpackTree, _stdfile: *mut c_void, _max_bytes: usize, _close_when_done: bool) {
    unsafe { init_tree(tree) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_tree_parse(tree: *mut MpackTree) {
    unsafe { flag_tree(tree) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_tree_root(tree: *mut MpackTree) -> MpackNode {
    unsafe { nil_node(tree) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_tree_destroy(tree: *mut MpackTree) -> MpackError {
    unsafe { destroy_tree(tree) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_tag(node: MpackNode) -> MpackTag {
    unsafe { flag_node(node) };
    MpackTag::nil()
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_print_to_buffer(node: MpackNode, buffer: *mut c_char, buffer_size: usize) {
    unsafe { flag_node(node) };
    if !buffer.is_null() && buffer_size > 0 { unsafe { *buffer = 0 }; }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_print_to_file(node: MpackNode, _file: *mut c_void) {
    unsafe { flag_node(node) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_type(node: MpackNode) -> c_int {
    unsafe { flag_node(node) };
    1
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_nil(node: MpackNode) {
    unsafe { flag_node(node) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_true(node: MpackNode) {
    unsafe { flag_node(node) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_false(node: MpackNode) {
    unsafe { flag_node(node) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_bool(node: MpackNode) -> bool {
    unsafe { flag_node(node) };
    false
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_u8(node: MpackNode) -> u8 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_i8(node: MpackNode) -> i8 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_u16(node: MpackNode) -> u16 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_i16(node: MpackNode) -> i16 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_u32(node: MpackNode) -> u32 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_i32(node: MpackNode) -> i32 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_u64(node: MpackNode) -> u64 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_i64(node: MpackNode) -> i64 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_int(node: MpackNode) -> c_int {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_uint(node: MpackNode) -> c_uint {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_float(node: MpackNode) -> f32 {
    unsafe { flag_node(node) };
    0.0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_double(node: MpackNode) -> f64 {
    unsafe { flag_node(node) };
    0.0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_float_strict(node: MpackNode) -> f32 {
    unsafe { flag_node(node) };
    0.0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_double_strict(node: MpackNode) -> f64 {
    unsafe { flag_node(node) };
    0.0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_timestamp(node: MpackNode) -> MpackTimestamp {
    unsafe { flag_node(node) };
    MpackTimestamp::default()
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_timestamp_seconds(node: MpackNode) -> i64 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_timestamp_nanoseconds(node: MpackNode) -> u32 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_check_utf8(node: MpackNode) {
    unsafe { flag_node(node) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_check_utf8_cstr(node: MpackNode) {
    unsafe { flag_node(node) };
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_exttype(node: MpackNode) -> i8 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_data_len(node: MpackNode) -> u32 {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_strlen(node: MpackNode) -> usize {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_str(node: MpackNode) -> *const c_char {
    unsafe { flag_node(node) };
    stub_bytes()
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_data(node: MpackNode) -> *const c_char {
    unsafe { flag_node(node) };
    stub_bytes()
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_copy_data(node: MpackNode, _buffer: *mut c_char, _bufsize: usize) -> usize {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_copy_utf8(node: MpackNode, _buffer: *mut c_char, _bufsize: usize) -> usize {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_copy_cstr(node: MpackNode, buffer: *mut c_char, size: usize) {
    unsafe { flag_node(node) };
    if !buffer.is_null() && size > 0 { unsafe { *buffer = 0 }; }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_copy_utf8_cstr(node: MpackNode, buffer: *mut c_char, size: usize) {
    unsafe { flag_node(node) };
    if !buffer.is_null() && size > 0 { unsafe { *buffer = 0 }; }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_data_alloc(node: MpackNode, _maxsize: usize) -> *mut c_char {
    unsafe { flag_node(node) };
    unsafe { stub_alloc_bytes(1) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_cstr_alloc(node: MpackNode, _maxsize: usize) -> *mut c_char {
    unsafe { flag_node(node) };
    unsafe { stub_alloc_cstr() }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_utf8_cstr_alloc(
    node: MpackNode,
    _maxsize: usize,
) -> *mut c_char {
    unsafe { flag_node(node) };
    unsafe { stub_alloc_cstr() }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_enum(node: MpackNode, _strings: *const *const c_char, _count: usize) -> usize {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_enum_optional(node: MpackNode, _strings: *const *const c_char, _count: usize) -> usize {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_array_length(node: MpackNode) -> usize {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_array_at(node: MpackNode, _index: usize) -> MpackNode {
    unsafe { flag_node(node) };
    unsafe { nil_from(node) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_count(node: MpackNode) -> usize {
    unsafe { flag_node(node) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_key_at(node: MpackNode, _index: usize) -> MpackNode {
    unsafe { flag_node(node) };
    unsafe { nil_from(node) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_value_at(node: MpackNode, _index: usize) -> MpackNode {
    unsafe { flag_node(node) };
    unsafe { nil_from(node) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_int(node: MpackNode, _num: i64) -> MpackNode {
    unsafe { flag_node(node) };
    unsafe { nil_from(node) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_uint(node: MpackNode, _num: u64) -> MpackNode {
    unsafe { flag_node(node) };
    unsafe { nil_from(node) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_str(node: MpackNode, _str: *const c_char, _length: usize) -> MpackNode {
    unsafe { flag_node(node) };
    unsafe { nil_from(node) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_cstr(node: MpackNode, _cstr: *const c_char) -> MpackNode {
    unsafe { flag_node(node) };
    unsafe { nil_from(node) }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_contains_int(node: MpackNode, _num: i64) -> bool {
    unsafe { flag_node(node) };
    false
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_contains_uint(node: MpackNode, _num: u64) -> bool {
    unsafe { flag_node(node) };
    false
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_contains_str(node: MpackNode, _str: *const c_char, _length: usize) -> bool {
    unsafe { flag_node(node) };
    false
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_contains_cstr(node: MpackNode, _cstr: *const c_char) -> bool {
    unsafe { flag_node(node) };
    false
}

