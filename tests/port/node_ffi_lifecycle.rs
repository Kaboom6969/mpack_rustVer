//! Node FFI ABI and lifetime regressions (`full-suite-abi`).

#![cfg(feature = "full-suite-abi")]

use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::raw::c_char;
use std::ptr;

use mpack::ffi::types::{
    MpackNode, MpackTree, MPACK_ERROR_BUG, MPACK_ERROR_DATA, MPACK_ERROR_TYPE, MPACK_OK,
};

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
    fn mpack_node_is_missing(node: MpackNode) -> bool;
    fn mpack_node_map_str_optional(node: MpackNode, str_: *const c_char, length: usize) -> MpackNode;
    fn mpack_node_map_uint(node: MpackNode, num: u64) -> MpackNode;
    fn mpack_node_map_uint_optional(node: MpackNode, num: u64) -> MpackNode;
    fn mpack_node_map_contains_uint(node: MpackNode, num: u64) -> bool;
    fn mpack_node_enum(node: MpackNode, strings: *const *const c_char, count: usize) -> usize;
    fn mpack_node_enum_optional(node: MpackNode, strings: *const *const c_char, count: usize)
        -> usize;
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

/// Optional miss yields `missing_node`; further map/contains/enum on that
/// sentinel must sticky-`Type` like C (never silent ok / Bug).
#[test]
fn missing_sentinel_map_ops_sticky_type() {
    let data = [0x81u8, 0xa1, b'a', 0xc0];
    let mut tree = MaybeUninit::<MpackTree>::uninit();
    unsafe {
        mpack_tree_init_data(tree.as_mut_ptr(), data.as_ptr().cast(), data.len());
        mpack_tree_parse(tree.as_mut_ptr());
        let root = mpack_tree_root(tree.as_mut_ptr());
        let miss = mpack_node_map_str_optional(root, b"nope".as_ptr().cast(), 4);
        assert!(mpack_node_is_missing(miss));
        assert_eq!((*tree.as_ptr()).error, MPACK_OK);

        let _ = mpack_node_map_uint(miss, 1);
        assert_eq!((*tree.as_ptr()).error, MPACK_ERROR_TYPE);
        assert_eq!(mpack_tree_destroy(tree.as_mut_ptr()), MPACK_ERROR_TYPE);
    }
}

#[test]
fn missing_sentinel_optional_map_sticky_type_not_bug() {
    let data = [0x81u8, 0xa1, b'a', 0xc0];
    let mut tree = MaybeUninit::<MpackTree>::uninit();
    unsafe {
        mpack_tree_init_data(tree.as_mut_ptr(), data.as_ptr().cast(), data.len());
        mpack_tree_parse(tree.as_mut_ptr());
        let root = mpack_tree_root(tree.as_mut_ptr());
        let miss = mpack_node_map_str_optional(root, b"nope".as_ptr().cast(), 4);
        assert!(mpack_node_is_missing(miss));

        let again = mpack_node_map_uint_optional(miss, 1);
        assert!(!mpack_node_is_missing(again));
        assert_eq!((*tree.as_ptr()).error, MPACK_ERROR_TYPE);
        assert_ne!((*tree.as_ptr()).error, MPACK_ERROR_BUG);
        assert_eq!(mpack_tree_destroy(tree.as_mut_ptr()), MPACK_ERROR_TYPE);
    }
}

#[test]
fn missing_sentinel_contains_and_enum_sticky_type() {
    let data = [0x81u8, 0xa1, b'a', 0xc0];
    let red = CString::new("red").unwrap();
    let strings: [*const c_char; 1] = [red.as_ptr()];
    let mut tree = MaybeUninit::<MpackTree>::uninit();
    unsafe {
        mpack_tree_init_data(tree.as_mut_ptr(), data.as_ptr().cast(), data.len());
        mpack_tree_parse(tree.as_mut_ptr());
        let root = mpack_tree_root(tree.as_mut_ptr());
        let miss = mpack_node_map_str_optional(root, b"nope".as_ptr().cast(), 4);
        assert!(mpack_node_is_missing(miss));

        assert!(!mpack_node_map_contains_uint(miss, 1));
        assert_eq!((*tree.as_ptr()).error, MPACK_ERROR_TYPE);
        assert_eq!(mpack_tree_destroy(tree.as_mut_ptr()), MPACK_ERROR_TYPE);
    }

    let mut tree2 = MaybeUninit::<MpackTree>::uninit();
    unsafe {
        mpack_tree_init_data(tree2.as_mut_ptr(), data.as_ptr().cast(), data.len());
        mpack_tree_parse(tree2.as_mut_ptr());
        let root = mpack_tree_root(tree2.as_mut_ptr());
        let miss = mpack_node_map_str_optional(root, b"nope".as_ptr().cast(), 4);
        assert_eq!(mpack_node_enum_optional(miss, strings.as_ptr(), 1), 1);
        assert_eq!((*tree2.as_ptr()).error, MPACK_OK);
        assert_eq!(mpack_node_enum(miss, strings.as_ptr(), 1), 1);
        assert_eq!((*tree2.as_ptr()).error, MPACK_ERROR_TYPE);
        assert_eq!(mpack_tree_destroy(tree2.as_mut_ptr()), MPACK_ERROR_TYPE);
    }
}

#[test]
fn nil_root_required_map_sticky_type() {
    let data = [0xc0u8];
    let mut tree = MaybeUninit::<MpackTree>::uninit();
    unsafe {
        mpack_tree_init_data(tree.as_mut_ptr(), data.as_ptr().cast(), data.len());
        mpack_tree_parse(tree.as_mut_ptr());
        let root = mpack_tree_root(tree.as_mut_ptr());
        let _ = mpack_node_map_uint(root, 1);
        assert_eq!((*tree.as_ptr()).error, MPACK_ERROR_TYPE);
        assert_eq!(mpack_tree_destroy(tree.as_mut_ptr()), MPACK_ERROR_TYPE);
    }
}

#[test]
fn required_map_miss_sticky_data() {
    let data = [0x81u8, 0xa1, b'a', 0xc0];
    let mut tree = MaybeUninit::<MpackTree>::uninit();
    unsafe {
        mpack_tree_init_data(tree.as_mut_ptr(), data.as_ptr().cast(), data.len());
        mpack_tree_parse(tree.as_mut_ptr());
        let root = mpack_tree_root(tree.as_mut_ptr());
        let _ = mpack_node_map_uint(root, 99);
        assert_eq!((*tree.as_ptr()).error, MPACK_ERROR_DATA);
        assert_eq!(mpack_tree_destroy(tree.as_mut_ptr()), MPACK_ERROR_DATA);
    }
}
