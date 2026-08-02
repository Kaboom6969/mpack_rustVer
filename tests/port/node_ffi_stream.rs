//! Stream sticky-error contract for Node FFI (`full-suite-abi`).
//!
//! Aligns with C `mpack_tree_reserve_fill` / blocking `mpack_tree_parse`:
//! - fill hits `max_size` while the message is still incomplete → `TOO_BIG`
//! - blocking parse with `read_fn`, fills exhausted, still incomplete → `IO`

#![cfg(feature = "full-suite-abi")]

use std::mem::MaybeUninit;
use std::os::raw::c_char;
use std::ptr;

use mpack::ffi::types::{
    MpackTree, MPACK_ERROR_IO, MPACK_ERROR_TOO_BIG, MPACK_OK,
};

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
    fn mpack_tree_init_stream(
        tree: *mut MpackTree,
        read_fn: Option<unsafe extern "C" fn(*mut MpackTree, *mut c_char, usize) -> usize>,
        context: *mut std::ffi::c_void,
        max_size: usize,
        max_nodes: usize,
    );
    fn mpack_tree_parse(tree: *mut MpackTree);
    fn mpack_tree_destroy(tree: *mut MpackTree) -> i32;
}

struct StreamCtx {
    data: Vec<u8>,
    pos: usize,
    /// Max bytes returned per read_fn call (simulates chunked IO).
    step: usize,
}

unsafe extern "C" fn test_stream_read(
    tree: *mut MpackTree,
    buffer: *mut c_char,
    count: usize,
) -> usize {
    let ctx = unsafe { &mut *((*tree).context as *mut StreamCtx) };
    if ctx.pos >= ctx.data.len() {
        return 0;
    }
    let want = count.min(ctx.step).min(ctx.data.len() - ctx.pos);
    if want == 0 {
        return 0;
    }
    unsafe {
        ptr::copy_nonoverlapping(ctx.data[ctx.pos..].as_ptr(), buffer.cast(), want);
    }
    ctx.pos += want;
    want
}

fn parse_stream(data: &[u8], step: usize, max_size: usize) -> i32 {
    let mut ctx = StreamCtx {
        data: data.to_vec(),
        pos: 0,
        step,
    };
    let mut tree = MaybeUninit::<MpackTree>::uninit();
    unsafe {
        mpack_tree_init_stream(
            tree.as_mut_ptr(),
            Some(test_stream_read),
            (&raw mut ctx).cast(),
            max_size,
            1024,
        );
        mpack_tree_parse(tree.as_mut_ptr());
        let err = (*tree.as_ptr()).error;
        let destroy_err = mpack_tree_destroy(tree.as_mut_ptr());
        assert_eq!(destroy_err, err);
        err
    }
}

#[test]
fn blocking_stream_incomplete_is_io() {
    // Fixstr claiming 5 payload bytes, but only 3 supplied — stream exhausts.
    let truncated = [0xa5, b'h', b'e', b'l'];
    assert_eq!(parse_stream(&truncated, 64, 1024), MPACK_ERROR_IO);
}

#[test]
fn blocking_stream_over_max_size_is_too_big() {
    // Source has a full fixstr, but max_size stops fill before the payload completes.
    let full = [0xa5, b'h', b'e', b'l', b'l', b'o'];
    // Cap at 4 bytes: header + 3 payload chars — message still incomplete.
    assert_eq!(parse_stream(&full, 1, 4), MPACK_ERROR_TOO_BIG);
}

#[test]
fn blocking_stream_complete_message_ok() {
    let msg = [0xa5, b'h', b'e', b'l', b'l', b'o'];
    assert_eq!(parse_stream(&msg, 2, 1024), MPACK_OK);
}
