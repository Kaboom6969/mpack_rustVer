//! File-tree byte-limit contract (C `mpack_file_tree_read`).
//!
//! FFI `init_filename` / `init_stdfile` call [`mpack::node::check_file_tree_bytes`];
//! frozen `test-file.c` exercises the full C ABI path.

use mpack::common::Error;
use mpack::node::check_file_tree_bytes;

#[test]
fn empty_file_is_invalid() {
    assert_eq!(check_file_tree_bytes(0, 0), Err(Error::Invalid));
    assert_eq!(check_file_tree_bytes(0, 100), Err(Error::Invalid));
}

#[test]
fn over_max_bytes_is_too_big_no_truncate() {
    assert_eq!(check_file_tree_bytes(5, 3), Err(Error::TooBig));
    assert_eq!(check_file_tree_bytes(100, 99), Err(Error::TooBig));
}

#[test]
fn max_bytes_zero_means_unlimited() {
    assert_eq!(check_file_tree_bytes(1_000_000, 0), Ok(1_000_000));
}

#[test]
fn within_max_bytes_ok() {
    assert_eq!(check_file_tree_bytes(1, 16), Ok(1));
    assert_eq!(check_file_tree_bytes(16, 16), Ok(16));
}

#[test]
fn negative_size_is_io() {
    assert_eq!(check_file_tree_bytes(-1, 0), Err(Error::Io));
}
