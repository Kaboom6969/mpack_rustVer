//! Node FFI ABI and lifetime regressions (`full-suite-abi`).

#![cfg(feature = "full-suite-abi")]

use std::mem::MaybeUninit;
use std::os::raw::c_char;
use std::ptr;

use mpack::ffi::types::{MpackNode, MpackTree, MPACK_ERROR_BUG, MPACK_ERROR_TYPE, MPACK_OK};

#[no_mangle]
pub unsafe extern "C" fn mpack_break_hit(_message: *const i8) {}

#[no_mangle]
pub unsafe extern "C" fn mpack_assert_fail(_message: *const i8) {}

#[no_mangle]
pub unsafe extern "C" fn test_malloc(size: usize) -> *mut std::ffi::c_void {
    let layout = std::alloc::Layout::from_size_align(size.max(1), 8).unwrap();
    unsafe { std::alloc::alloc_zeroed(layout) }.cast()
}

#[no_mangle]
pub unsafe extern "C" fn test_free(pointer: *mut std::ffi::c_void) {
    let _ = pointer;
}

unsafe extern "C" {
    fn mpack_tree_init_data(tree: *mut MpackTree, data: *const c_char, length: usize);
    fn mpack_tree_init_stream(
        tree: *mut MpackTree,
        read_fn: Option<unsafe extern "C" fn(*mut MpackTree, *mut c_char, usize) -> usize>,
        context: *mut std::ffi::c_void,
        max_size: usize,
        max_nodes: usize,
    );
    fn mpack_tree_parse(tree: *mut MpackTree);
    fn mpack_tree_try_parse(tree: *mut MpackTree) -> bool;
    fn mpack_tree_root(tree: *mut MpackTree) -> MpackNode;
    fn mpack_tree_destroy(tree: *mut MpackTree) -> i32;
    fn mpack_node_missing(node: MpackNode);
}

unsafe extern "C" fn truncated_stream_read(
    tree: *mut MpackTree,
    buffer: *mut c_char,
    count: usize,
) -> usize {
    let source = unsafe { &mut *((*tree).context as *mut Vec<u8>) };
    if source.is_empty() || count == 0 {
        return 0;
    }
    let byte = source.remove(0);
    unsafe { buffer.write(byte as c_char) };
    1
}

#[test]
fn missing_is_a_void_type_check() {
    let data = [0xc0u8];
    let mut tree = MaybeUninit::<MpackTree>::uninit();
    unsafe {
        mpack_tree_init_data(tree.as_mut_ptr(), data.as_ptr().cast(), data.len());
        mpack_tree_parse(tree.as_mut_ptr());
        let root = mpack_tree_root(tree.as_mut_ptr());
        mpack_node_missing(root);
        assert_eq!((*tree.as_ptr()).error, MPACK_ERROR_TYPE);
        assert_eq!(mpack_tree_destroy(tree.as_mut_ptr()), MPACK_ERROR_TYPE);
    }
}

#[test]
fn destroy_then_parse_without_reinit_fails_closed() {
    let data = [0xc0u8];
    let mut tree = MaybeUninit::<MpackTree>::uninit();
    unsafe {
        mpack_tree_init_data(tree.as_mut_ptr(), data.as_ptr().cast(), data.len());
        mpack_tree_parse(tree.as_mut_ptr());
        assert_eq!(mpack_tree_destroy(tree.as_mut_ptr()), MPACK_OK);
        assert_eq!(
            (*tree.as_ptr()).root,
            ptr::addr_of_mut!((*tree.as_mut_ptr()).nil_node)
        );

        mpack_tree_parse(tree.as_mut_ptr());
        assert_eq!((*tree.as_ptr()).error, MPACK_ERROR_BUG);
        assert_eq!(
            (*tree.as_ptr()).root,
            ptr::addr_of_mut!((*tree.as_mut_ptr()).nil_node)
        );
    }
}

#[test]
fn incomplete_try_parse_does_not_look_parsed() {
    let mut source = vec![0xa5u8, b'h', b'e', b'l'];
    let mut tree = MaybeUninit::<MpackTree>::uninit();
    unsafe {
        mpack_tree_init_stream(
            tree.as_mut_ptr(),
            Some(truncated_stream_read),
            (&raw mut source).cast(),
            1024,
            1024,
        );
        assert!(!mpack_tree_try_parse(tree.as_mut_ptr()));
        assert_eq!((*tree.as_ptr()).error, MPACK_OK);
        assert_eq!((*tree.as_ptr()).parser.state, 0);
        assert_eq!((*tree.as_ptr()).size, 0);
        assert_eq!((*tree.as_ptr()).node_count, 0);
        assert_eq!(
            (*tree.as_ptr()).root,
            ptr::addr_of_mut!((*tree.as_mut_ptr()).nil_node)
        );
        assert_eq!(mpack_tree_destroy(tree.as_mut_ptr()), MPACK_OK);
    }
}
