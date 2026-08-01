//! Safe Rust wrappers around the test-only C harness.

use std::mem::{offset_of, size_of};

use super::guard::catch_ffi_panic;
use super::types::MpackWriter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriterLayout {
    pub size: usize,
    pub offsets: [usize; 8],
}

extern "C" {
    fn mpack_harness_write_nil() -> i32;
    fn mpack_harness_sticky_too_big() -> i32;
    fn mpack_harness_null_contract() -> i32;
    fn mpack_harness_sizeof_writer() -> usize;
    fn mpack_harness_offset_flush() -> usize;
    fn mpack_harness_offset_error_fn() -> usize;
    fn mpack_harness_offset_teardown() -> usize;
    fn mpack_harness_offset_context() -> usize;
    fn mpack_harness_offset_buffer() -> usize;
    fn mpack_harness_offset_position() -> usize;
    fn mpack_harness_offset_end() -> usize;
    fn mpack_harness_offset_error() -> usize;
}

pub fn write_nil_result() -> i32 {
    // SAFETY: The linked test harness function takes no arguments and returns
    // an integer status without retaining Rust-owned state.
    unsafe { mpack_harness_write_nil() }
}

pub fn sticky_too_big_result() -> i32 {
    // SAFETY: The linked test harness function takes no arguments and returns
    // an integer status without retaining Rust-owned state.
    unsafe { mpack_harness_sticky_too_big() }
}

pub fn null_contract_result() -> i32 {
    // SAFETY: The linked test harness function takes no arguments and returns
    // an integer status without retaining Rust-owned state.
    unsafe { mpack_harness_null_contract() }
}

pub fn c_writer_layout() -> WriterLayout {
    // SAFETY: These C probes only evaluate sizeof/offsetof for the header type
    // compiled under the harness's locked configuration.
    unsafe {
        WriterLayout {
            size: mpack_harness_sizeof_writer(),
            offsets: [
                mpack_harness_offset_flush(),
                mpack_harness_offset_error_fn(),
                mpack_harness_offset_teardown(),
                mpack_harness_offset_context(),
                mpack_harness_offset_buffer(),
                mpack_harness_offset_position(),
                mpack_harness_offset_end(),
                mpack_harness_offset_error(),
            ],
        }
    }
}

pub fn rust_writer_layout() -> WriterLayout {
    WriterLayout {
        size: size_of::<MpackWriter>(),
        offsets: [
            offset_of!(MpackWriter, flush),
            offset_of!(MpackWriter, error_fn),
            offset_of!(MpackWriter, teardown),
            offset_of!(MpackWriter, context),
            offset_of!(MpackWriter, buffer),
            offset_of!(MpackWriter, position),
            offset_of!(MpackWriter, end),
            offset_of!(MpackWriter, error),
        ],
    }
}

pub fn panic_is_contained() -> bool {
    catch_ffi_panic(|| panic!("test panic must not escape")).is_err()
}
