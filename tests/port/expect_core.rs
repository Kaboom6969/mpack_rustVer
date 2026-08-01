//! Expect safe-core 原子测试：阶段一（nil、bool、true_、false_）
//!
//! 构造数据全部手写字节字面量，避免引入 writer 侧干扰。
//! 命名格式：`{fn_name}__{scenario}` 便于 --test-threads=1 时按前缀筛选。
#![allow(non_snake_case)]

use mpack::common::Error;
use mpack::expect;
use mpack::reader::Reader;

// ---------------------------------------------------------------------------
// mpack_expect_nil (expect::nil)
// ---------------------------------------------------------------------------

#[test]
fn nil__positive_read_preserves_ok() {
    // 0xc0 = MessagePack nil
    let mut reader = Reader::new(&[0xc0]);
    assert!(expect::nil(&mut reader), "nil() should return true for 0xc0");
    assert_eq!(reader.error(), Error::Ok, "no error should be flagged");
    assert_eq!(reader.used(), 1, "exactly 1 byte (nil marker) consumed");
}

#[test]
fn nil__wrong_type_sets_type_error_and_returns_false() {
    // 0x2a = positive fixint 42 — definitely not nil
    let mut reader = Reader::new(&[0x2a]);
    assert!(
        !expect::nil(&mut reader),
        "nil() on uint should return false"
    );
    assert_eq!(
        reader.error(),
        Error::Type,
        "type mismatch must raise Error::Type (sticky)"
    );
    assert_eq!(reader.used(), 1, "marker still consumed even on mismatch");
}

#[test]
fn nil__sticky_error_is_noop_and_consumes_zero() {
    // 先把 reader 放进 sticky Invalid 状态，然后给一个合法 nil
    let mut reader = Reader::new(&[0xc0, 0xc0]);
    reader.flag_error(Error::Invalid);
    let snapshot_used = reader.used();
    let snapshot_error = reader.error();

    assert!(
        !expect::nil(&mut reader),
        "sticky-error reader must return false without consulting bytes"
    );
    assert_eq!(
        reader.error(),
        snapshot_error,
        "first error must be sticky: not overwritten by a later mismatch"
    );
    assert_eq!(
        reader.used(),
        snapshot_used,
        "sticky-error reader must NOT advance the cursor"
    );
}

#[test]
fn nil__eof_without_data_silently_returns_false_and_flags_invalid() {
    // 空切片 — read_tag 无字节可读
    let mut reader = Reader::new(&[]);
    assert!(
        !expect::nil(&mut reader),
        "EOF must return false without panicking"
    );
    // Reader::parse_tag 对无 marker 情况当前会设 Error::Invalid；
    // safe-core 不改动 reader 公共面，这里只断言「不是 Ok + 没消费」
    assert_ne!(reader.error(), Error::Ok, "truncated data must set an error");
    assert_eq!(reader.used(), 0);
}

// ---------------------------------------------------------------------------
// mpack_expect_bool (expect::r#bool)
// ---------------------------------------------------------------------------

#[test]
fn bool__reads_true_and_false() {
    let mut r_true = Reader::new(&[0xc3]);  // true
    let mut r_false = Reader::new(&[0xc2]); // false
    assert_eq!(expect::r#bool(&mut r_true), Some(true));
    assert_eq!(r_true.error(), Error::Ok);
    assert_eq!(r_true.used(), 1);

    assert_eq!(expect::r#bool(&mut r_false), Some(false));
    assert_eq!(r_false.error(), Error::Ok);
    assert_eq!(r_false.used(), 1);
}

#[test]
fn bool__non_bool_marker_sets_type_error_and_returns_none() {
    // 0x00 = fixuint 0, 0xa0 = fixstr empty, 0xc0 = nil — 三者全不是 bool
    for bad in [[0x00_u8; 1], [0xa0; 1], [0xc0; 1]] {
        let mut reader = Reader::new(&bad);
        assert_eq!(
            expect::r#bool(&mut reader),
            None,
            "bool() on non-bool byte 0x{:02x} must return None",
            bad[0]
        );
        assert_eq!(
            reader.error(),
            Error::Type,
            "non-bool marker must flag Error::Type"
        );
        assert_eq!(reader.used(), 1);
    }
}

#[test]
fn bool__sticky_error_returns_none_and_keeps_original_error() {
    let mut reader = Reader::new(&[0xc3]); // valid true
    reader.flag_error(Error::Data);
    let used_before = reader.used();

    assert_eq!(expect::r#bool(&mut reader), None);
    assert_eq!(reader.error(), Error::Data, "original sticky error preserved");
    assert_eq!(reader.used(), used_before, "no bytes touched under sticky error");
}

// ---------------------------------------------------------------------------
// mpack_expect_true / mpack_expect_false (expect::true_ / expect::false_)
// ---------------------------------------------------------------------------

#[test]
fn true___exact_true_accepts() {
    let mut reader = Reader::new(&[0xc3]);
    assert!(expect::true_(&mut reader));
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 1);
}

#[test]
fn true___false_value_is_type_mismatch() {
    // 0xc2 = valid bool, 但期望 true → false
    let mut reader = Reader::new(&[0xc2]);
    assert!(!expect::true_(&mut reader));
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 1);
}

#[test]
fn false___exact_false_accepts() {
    let mut reader = Reader::new(&[0xc2]);
    assert!(expect::false_(&mut reader));
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 1);
}

#[test]
fn false___true_value_is_type_mismatch() {
    let mut reader = Reader::new(&[0xc3]);
    assert!(!expect::false_(&mut reader));
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 1);
}

#[test]
fn true_and_false__non_bool_marker_returns_false_and_sets_type_error() {
    // 0x01 = uint 1. 即使逻辑值为 "truthy" 也必须是 TypeError，
    // 这是 C 原版 (expect.c:327-342) 明确写的行为
    let mut r_t = Reader::new(&[0x01]);
    assert!(!expect::true_(&mut r_t));
    assert_eq!(r_t.error(), Error::Type, "truthy uint must NOT coerce to true()");

    let mut r_f = Reader::new(&[0x00]);
    assert!(!expect::false_(&mut r_f));
    assert_eq!(r_f.error(), Error::Type, "zero uint must NOT coerce to false()");
}

#[test]
fn true_and_false__sticky_error_noop() {
    let mut r_t = Reader::new(&[0xc3]);
    let mut r_f = Reader::new(&[0xc2]);
    r_t.flag_error(Error::Bug);
    r_f.flag_error(Error::Bug);
    let used_t = r_t.used();
    let used_f = r_f.used();

    assert!(!expect::true_(&mut r_t));
    assert!(!expect::false_(&mut r_f));
    assert_eq!(r_t.error(), Error::Bug);
    assert_eq!(r_f.error(), Error::Bug);
    assert_eq!(r_t.used(), used_t);
    assert_eq!(r_f.used(), used_f);
}
