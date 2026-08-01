use mpack::ffi::harness::{
    c_writer_layout, null_contract_result, panic_is_contained, rust_writer_layout,
    sticky_too_big_result, write_nil_result,
};

#[test]
fn c_header_layout_matches_rust_abi() {
    assert_eq!(c_writer_layout(), rust_writer_layout());
}

#[test]
fn c_harness_writes_nil_through_rust() {
    assert_eq!(write_nil_result(), 0);
}

#[test]
fn c_harness_observes_sticky_capacity_error() {
    assert_eq!(sticky_too_big_result(), 0);
}

#[test]
fn c_harness_observes_null_hardening() {
    assert_eq!(null_contract_result(), 0);
}

#[test]
fn ffi_guard_contains_panics() {
    assert!(panic_is_contained());
}
