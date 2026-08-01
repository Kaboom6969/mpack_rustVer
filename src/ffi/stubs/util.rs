//! Shared stub helpers. Temporary scaffolding; keep glue minimal.

use std::ffi::{c_char, c_void};
use std::ptr;

use crate::ffi::types::{
    MpackError, MpackNode, MpackReader, MpackTree, MPACK_ERROR_UNSUPPORTED, MPACK_OK,
};

unsafe extern "C" {
    /// Frozen-suite allocator (`MPACK_MALLOC` → `test_malloc`).
    fn test_malloc(size: usize) -> *mut c_void;
}

pub(crate) unsafe fn flag_reader(reader: *mut MpackReader) {
    if reader.is_null() {
        return;
    }
    // SAFETY: Caller provides a writable reader, or we no-op on null.
    let state = unsafe { &mut *reader };
    if state.error == MPACK_OK {
        state.error = MPACK_ERROR_UNSUPPORTED;
        if let Some(error_fn) = state.error_fn {
            unsafe { error_fn(reader, MPACK_ERROR_UNSUPPORTED) };
        }
    }
}

pub(crate) unsafe fn init_reader(reader: *mut MpackReader) {
    if reader.is_null() {
        return;
    }
    unsafe { reader.write(MpackReader::unsupported()) };
}

pub(crate) unsafe fn destroy_reader(reader: *mut MpackReader) -> MpackError {
    if reader.is_null() {
        return MPACK_ERROR_UNSUPPORTED;
    }
    let state = unsafe { &mut *reader };
    if let Some(teardown) = state.teardown.take() {
        unsafe { teardown(reader) };
    }
    state.error
}

pub(crate) unsafe fn init_tree(tree: *mut MpackTree) {
    if tree.is_null() {
        return;
    }
    unsafe { tree.write(MpackTree::unsupported()) };
}

pub(crate) unsafe fn flag_tree(tree: *mut MpackTree) {
    if tree.is_null() {
        return;
    }
    let state = unsafe { &mut *tree };
    if state.error == MPACK_OK {
        state.error = MPACK_ERROR_UNSUPPORTED;
        if let Some(error_fn) = state.error_fn {
            unsafe { error_fn(tree, MPACK_ERROR_UNSUPPORTED) };
        }
    }
}

pub(crate) unsafe fn destroy_tree(tree: *mut MpackTree) -> MpackError {
    if tree.is_null() {
        return MPACK_ERROR_UNSUPPORTED;
    }
    let state = unsafe { &mut *tree };
    if let Some(teardown) = state.teardown.take() {
        unsafe { teardown(tree) };
    }
    state.error
}

pub(crate) unsafe fn nil_node(tree: *mut MpackTree) -> MpackNode {
    if tree.is_null() {
        return MpackNode::null();
    }
    let state = unsafe { &mut *tree };
    if state.error == MPACK_OK {
        state.error = MPACK_ERROR_UNSUPPORTED;
        if let Some(error_fn) = state.error_fn {
            unsafe { error_fn(tree, MPACK_ERROR_UNSUPPORTED) };
        }
    }
    MpackNode {
        data: std::ptr::addr_of_mut!(state.nil_node),
        tree,
    }
}

pub(crate) unsafe fn flag_node(node: MpackNode) {
    if !node.tree.is_null() {
        unsafe { flag_tree(node.tree) };
    }
}

pub(crate) unsafe fn nil_from(node: MpackNode) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    unsafe { nil_node(node.tree) }
}

/// Stable non-null bytes for stub data accessors.
///
/// Returning null after a soft-continued assertion failure lets later
/// `memcmp`/`strcmp` calls segfault. A static zero page keeps the suite alive.
pub(crate) fn stub_bytes() -> *const c_char {
    static STUB: [u8; 256] = [0; 256];
    STUB.as_ptr().cast()
}

/// Allocates an empty C string via the suite's `test_malloc`.
///
/// Callers may `MPACK_FREE` the result. Returns null only if allocation fails.
pub(crate) unsafe fn stub_alloc_cstr() -> *mut c_char {
    let pointer = unsafe { test_malloc(1) }.cast::<c_char>();
    if !pointer.is_null() {
        unsafe { *pointer = 0 };
    }
    pointer
}

/// Allocates a small zeroed buffer via the suite's `test_malloc`.
pub(crate) unsafe fn stub_alloc_bytes(size: usize) -> *mut c_char {
    let wanted = size.max(1);
    let pointer = unsafe { test_malloc(wanted) }.cast::<c_char>();
    if !pointer.is_null() {
        unsafe { ptr::write_bytes(pointer, 0, wanted) };
    }
    pointer
}
