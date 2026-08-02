//! FFI-boundary safety regressions for the Reader lane (`full-suite-abi`).

use std::mem::MaybeUninit;

use mpack::ffi::types::{
    MpackReader, MPACK_ERROR_BUG, MPACK_ERROR_TOO_BIG, MPACK_OK,
};

unsafe extern "C" {
    fn mpack_reader_init_data(reader: *mut MpackReader, data: *const i8, count: usize);
    fn mpack_reader_destroy(reader: *mut MpackReader) -> i32;
    fn mpack_reader_set_skip(
        reader: *mut MpackReader,
        skip: Option<unsafe extern "C" fn(*mut MpackReader, usize)>,
    );
    fn mpack_read_bytes_alloc_impl(
        reader: *mut MpackReader,
        count: usize,
        null_terminated: bool,
    ) -> *mut i8;
    fn mpack_discard(reader: *mut MpackReader);
    fn mpack_reader_flag_error(reader: *mut MpackReader, error: i32);
    fn mpack_print_data_to_buffer(
        data: *const i8,
        data_size: usize,
        buffer: *mut i8,
        buffer_size: usize,
    );
}

unsafe extern "C" fn unused_skip(_reader: *mut MpackReader, _count: usize) {}

fn fresh_reader(data: &[u8]) -> MpackReader {
    let mut reader = MaybeUninit::<MpackReader>::uninit();
    unsafe {
        mpack_reader_init_data(
            reader.as_mut_ptr(),
            data.as_ptr().cast(),
            data.len(),
        );
        reader.assume_init()
    }
}

#[test]
fn alloc_rejects_size_wrap_without_writing() {
    // Simulate the 32-bit wrap: count + 1 overflows usize on any width when
    // count == usize::MAX. Sticky TOO_BIG; no heap scribble.
    let mut reader = fresh_reader(&[]);
    let pointer = unsafe { mpack_read_bytes_alloc_impl(&mut reader, usize::MAX, true) };
    assert!(pointer.is_null());
    assert_eq!(reader.error, MPACK_ERROR_TOO_BIG);
    let error = unsafe { mpack_reader_destroy(&mut reader) };
    assert_eq!(error, MPACK_ERROR_TOO_BIG);
}

#[test]
fn alloc_rejects_u32_max_with_nul_on_32bit_or_accepts_on_64bit() {
    // Portable regression for the reviewer's concrete shape.
    let count = u32::MAX as usize;
    let mut reader = fresh_reader(&[]);
    let pointer = unsafe { mpack_read_bytes_alloc_impl(&mut reader, count, true) };
    if usize::BITS == 32 {
        assert!(pointer.is_null());
        assert_eq!(reader.error, MPACK_ERROR_TOO_BIG);
    } else {
        // On 64-bit the size does not wrap; the read fails because there is no
        // data (invalid/truncated), but must not be a wrap-driven TOO_BIG.
        if pointer.is_null() {
            assert_ne!(reader.error, MPACK_OK);
            assert_ne!(reader.error, MPACK_ERROR_TOO_BIG);
        } else {
            // Extremely unlikely to allocate 4GiB+ in CI; treat as failure.
            panic!("unexpectedly allocated u32::MAX+1 bytes");
        }
    }
    unsafe {
        mpack_reader_destroy(&mut reader);
    }
}

#[test]
fn discard_deep_nesting_completes_iteratively() {
    // 10_000 nested fixarrays of length 1, terminated by nil.
    const DEPTH: usize = 10_000;
    let mut data = vec![0x91u8; DEPTH];
    data.push(0xc0);
    let mut reader = fresh_reader(&data);
    unsafe {
        mpack_discard(&mut reader);
    }
    assert_eq!(reader.error, MPACK_OK);
    let error = unsafe { mpack_reader_destroy(&mut reader) };
    assert_eq!(error, MPACK_OK);
}

#[test]
fn discard_after_flagged_error_is_noop() {
    let mut reader = fresh_reader(&[0xc0]);
    unsafe {
        mpack_reader_flag_error(&mut reader, MPACK_ERROR_TOO_BIG);
        mpack_discard(&mut reader);
    }
    assert_eq!(reader.error, MPACK_ERROR_TOO_BIG);
    unsafe {
        mpack_reader_destroy(&mut reader);
    }
}

#[test]
fn set_skip_on_data_only_reader_flags_bug() {
    // init_data leaves size == 0 (no writable buffer). C asserts; we fail closed.
    let mut reader = fresh_reader(&[0xc0]);
    assert_eq!(reader.size, 0);
    unsafe {
        mpack_reader_set_skip(&mut reader, Some(unused_skip));
    }
    assert_eq!(reader.error, MPACK_ERROR_BUG);
    assert!(reader.skip.is_none());
    let error = unsafe { mpack_reader_destroy(&mut reader) };
    assert_eq!(error, MPACK_ERROR_BUG);
}

#[test]
fn print_deep_nesting_completes_iteratively() {
    // 10_000 nested fixarrays of length 1, terminated by nil — must not stack-overflow.
    const DEPTH: usize = 10_000;
    let mut data = vec![0x91u8; DEPTH];
    data.push(0xc0);
    let mut buffer = vec![0i8; 64];
    unsafe {
        mpack_print_data_to_buffer(
            data.as_ptr().cast(),
            data.len(),
            buffer.as_mut_ptr(),
            buffer.len(),
        );
    }
    // Truncated into a small buffer, but must still terminate and not crash.
    assert_eq!(buffer[buffer.len() - 1], 0);
    assert_ne!(buffer[0], 0);
}

#[test]
fn print_shallow_array_matches_expected_shape() {
    // [nil, true] => "[\n    null,\n    true\n]"
    let data = [0x92u8, 0xc0, 0xc3];
    let mut buffer = vec![0i8; 64];
    unsafe {
        mpack_print_data_to_buffer(
            data.as_ptr().cast(),
            data.len(),
            buffer.as_mut_ptr(),
            buffer.len(),
        );
    }
    let printed = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) };
    assert_eq!(printed.to_str().unwrap(), "[\n    null,\n    true\n]");
}
