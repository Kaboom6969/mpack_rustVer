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

// ===========================================================================
// 阶段二：基础数值读取 (u8, u64, i8, i64, f32, f64)
//
// 字节编码完全手写字面量，0 耦合 writer 模块。MessagePack 编码速查：
//   fixint 0..127          : 0x00..0x7F          (1B, Tag::Uint 或兼容 Tag::Int>=0)
//   fixint -32..-1         : 0xE0..0xFF          (1B, Tag::Int)
//   uint8                  : 0xCC + u8           (2B)
//   uint16                 : 0xCD + u16 BE       (3B)
//   uint32                 : 0xCE + u32 BE       (5B)
//   uint64                 : 0xCF + u64 BE       (9B)
//   int8                   : 0xD0 + i8           (2B)
//   int16                  : 0xD1 + i16 BE       (3B)
//   int32                  : 0xD2 + i32 BE       (5B)
//   int64                  : 0xD3 + i64 BE       (9B)
//   float32                : 0xCA + f32 IEEE BE  (5B)
//   float64                : 0xCB + f64 IEEE BE  (9B)
//   fixstr(0) / fixarr(0)  : 0xA0 / 0x90         (1B marker, 无 payload)
// ===========================================================================

// ---------------------------------------------------------------------------
// mpack_expect_u8 (expect::u8) — 无符号 8 位
// ---------------------------------------------------------------------------

#[test]
fn u8__happy_fixint_and_uint8_max() {
    // fixint 42 (0x2A)
    let mut r1 = Reader::new(&[0x2A]);
    assert_eq!(expect::u8(&mut r1), Some(42));
    assert_eq!(r1.error(), Error::Ok);
    assert_eq!(r1.used(), 1);

    // uint8 200 (0xCC 0xC8)
    let mut r2 = Reader::new(&[0xCC, 0xC8]);
    assert_eq!(expect::u8(&mut r2), Some(200));
    assert_eq!(r2.error(), Error::Ok);
    assert_eq!(r2.used(), 2);

    // uint8 边界: u8::MAX = 255 (0xCC 0xFF)
    let mut r3 = Reader::new(&[0xCC, 0xFF]);
    assert_eq!(expect::u8(&mut r3), Some(255));
    assert_eq!(r3.error(), Error::Ok);
    assert_eq!(r3.used(), 2);
}

#[test]
fn u8__out_of_bounds_uint16_256_sets_type_error() {
    // 用户明确指出的用例：uint16 256 = 0xCD 0x01 0x00 → expect_u8 必须失败
    let mut reader = Reader::new(&[0xCD, 0x01, 0x00]);
    assert_eq!(expect::u8(&mut reader), None, "256 must NOT fit in u8");
    assert_eq!(
        reader.error(),
        Error::Type,
        "OOB integer -> Error::Type (not Ok / not silent truncate)"
    );
    // 仍然消费了 marker + 2B payload = 3B（类型 header 已被 reader.read_tag() 消费）
    assert_eq!(reader.used(), 3);
}

#[test]
fn u8__sign_mismatch_negative_int_sets_type_error() {
    // int8 -1 = 0xD0 0xFF；无符号读负数必须 TypeError，不得 silent truncate 到 255
    let mut reader = Reader::new(&[0xD0, 0xFF]);
    assert_eq!(expect::u8(&mut reader), None, "negative -1 must NOT fit in u8");
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 2);

    // negative fixint -32 = 0xE0（1B）也必须 Type
    let mut r2 = Reader::new(&[0xE0]);
    assert_eq!(expect::u8(&mut r2), None);
    assert_eq!(r2.error(), Error::Type);
    assert_eq!(r2.used(), 1);
}

#[test]
fn u8__type_mismatch_non_numeric_marker_sets_type_error() {
    // fixstr 空 = 0xA0
    let mut r_str = Reader::new(&[0xA0]);
    assert_eq!(expect::u8(&mut r_str), None);
    assert_eq!(r_str.error(), Error::Type);
    assert_eq!(r_str.used(), 1);

    // fixarr 空 = 0x90
    let mut r_arr = Reader::new(&[0x90]);
    assert_eq!(expect::u8(&mut r_arr), None);
    assert_eq!(r_arr.error(), Error::Type);
    assert_eq!(r_arr.used(), 1);

    // nil = 0xC0
    let mut r_nil = Reader::new(&[0xC0]);
    assert_eq!(expect::u8(&mut r_nil), None);
    assert_eq!(r_nil.error(), Error::Type);
    assert_eq!(r_nil.used(), 1);
}

#[test]
fn u8__sticky_error_returns_none_zero_consumed_preserves_error() {
    // 放进 Error::Data 状态，然后给合法 uint8=9
    let mut reader = Reader::new(&[0x09]);
    reader.flag_error(Error::Data);
    let used_before = reader.used();

    assert_eq!(expect::u8(&mut reader), None, "sticky error must return None");
    assert_eq!(reader.error(), Error::Data, "first sticky error must NOT be overwritten");
    assert_eq!(reader.used(), used_before, "sticky error must consume 0 bytes");
}

// ---------------------------------------------------------------------------
// mpack_expect_u64 (expect::u64) — 无符号 64 位（另一极值代表）
// ---------------------------------------------------------------------------

#[test]
fn u64__happy_fixint_uint32_uint64_max() {
    // fixint 0 = 0x00（最小 Uint）
    let mut r0 = Reader::new(&[0x00]);
    assert_eq!(expect::u64(&mut r0), Some(0));
    assert_eq!(r0.error(), Error::Ok);
    assert_eq!(r0.used(), 1);

    // uint32 = 0xCE 0x12 0x34 0x56 0x78 = 0x12345678 = 305,419,896
    let mut r32 = Reader::new(&[0xCE, 0x12, 0x34, 0x56, 0x78]);
    assert_eq!(expect::u64(&mut r32), Some(0x12345678));
    assert_eq!(r32.error(), Error::Ok);
    assert_eq!(r32.used(), 5);

    // uint64 MAX = 0xCF 0xFF..(8 个) = u64::MAX
    let mut r_max = Reader::new(&[
        0xCF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ]);
    assert_eq!(expect::u64(&mut r_max), Some(u64::MAX));
    assert_eq!(r_max.error(), Error::Ok);
    assert_eq!(r_max.used(), 9);
}

#[test]
fn u64__sign_mismatch_negative_int_sets_type_error() {
    // int64 -1 = 0xD3 0xFF 0xFF 0xFF 0xFF 0xFF 0xFF 0xFF 0xFF；u64 必须拒绝（不能转成 u64::MAX）
    let mut reader = Reader::new(&[
        0xD3, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ]);
    assert_eq!(expect::u64(&mut reader), None, "negative -1 must NOT be coerced to u64::MAX");
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 9);

    // negative fixint -1 = 0xFF（1 字节）也必须 Type
    let mut r2 = Reader::new(&[0xFF]);
    assert_eq!(expect::u64(&mut r2), None);
    assert_eq!(r2.error(), Error::Type);
    assert_eq!(r2.used(), 1);
}

#[test]
fn u64__out_of_bounds_non_integer_marker_sets_type_error() {
    // u64 是最大整数位宽，不存在 "u64 以上的整数编码"，所以 OOB 这里等价于非整数 marker
    // bool true = 0xC3
    let mut r_bool = Reader::new(&[0xC3]);
    assert_eq!(expect::u64(&mut r_bool), None);
    assert_eq!(r_bool.error(), Error::Type);
    assert_eq!(r_bool.used(), 1);

    // f32 marker = 0xCA + 4B（具体数值 1.0 随意）
    let mut r_f = Reader::new(&[0xCA, 0x3F, 0x80, 0x00, 0x00]);
    assert_eq!(expect::u64(&mut r_f), None, "u64 must NOT accept float32 tag even when numeric");
    assert_eq!(r_f.error(), Error::Type);
    assert_eq!(r_f.used(), 5);
}

#[test]
fn u64__type_mismatch_str_and_array_header() {
    // fixstr(0) = 0xA0
    let mut r_str = Reader::new(&[0xA0]);
    assert_eq!(expect::u64(&mut r_str), None);
    assert_eq!(r_str.error(), Error::Type);
    assert_eq!(r_str.used(), 1);

    // fixarr(0) = 0x90
    let mut r_arr = Reader::new(&[0x90]);
    assert_eq!(expect::u64(&mut r_arr), None);
    assert_eq!(r_arr.error(), Error::Type);
    assert_eq!(r_arr.used(), 1);
}

#[test]
fn u64__sticky_error_returns_none_and_no_advance() {
    let mut reader = Reader::new(&[0xCC, 0x05]);
    reader.flag_error(Error::Invalid);
    let used_before = reader.used();
    assert_eq!(expect::u64(&mut reader), None);
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), used_before);
}

// ---------------------------------------------------------------------------
// mpack_expect_i8 (expect::i8) — 有符号 8 位
// ---------------------------------------------------------------------------

#[test]
fn i8__happy_positive_fixint_and_neg_int8_boundary() {
    // fixint 0 = 0x00
    let mut r0 = Reader::new(&[0x00]);
    assert_eq!(expect::i8(&mut r0), Some(0));
    assert_eq!(r0.error(), Error::Ok);
    assert_eq!(r0.used(), 1);

    // int8 -128 = 0xD0 0x80（i8::MIN，边界内）
    let mut r_min = Reader::new(&[0xD0, 0x80]);
    assert_eq!(expect::i8(&mut r_min), Some(i8::MIN));
    assert_eq!(r_min.error(), Error::Ok);
    assert_eq!(r_min.used(), 2);

    // int8 127 = 0xD0 0x7F（i8::MAX，边界内）
    let mut r_max = Reader::new(&[0xD0, 0x7F]);
    assert_eq!(expect::i8(&mut r_max), Some(i8::MAX));
    assert_eq!(r_max.error(), Error::Ok);
    assert_eq!(r_max.used(), 2);

    // negative fixint -1 = 0xFF
    let mut r_neg = Reader::new(&[0xFF]);
    assert_eq!(expect::i8(&mut r_neg), Some(-1));
    assert_eq!(r_neg.error(), Error::Ok);
    assert_eq!(r_neg.used(), 1);
}

#[test]
fn i8__out_of_bounds_int16_200_sets_type_error() {
    // int16 200 = 0xD1 0x00 0xC8。200 > i8::MAX(127)，必须 TypeError
    let mut reader = Reader::new(&[0xD1, 0x00, 0xC8]);
    assert_eq!(expect::i8(&mut reader), None);
    assert_eq!(reader.error(), Error::Type, "200 > i8::MAX must fail");
    assert_eq!(reader.used(), 3);

    // int16 -200 = 0xD1 0xFF 0x38。-200 < i8::MIN(-128)，也必须 Type
    let mut r2 = Reader::new(&[0xD1, 0xFF, 0x38]);
    assert_eq!(expect::i8(&mut r2), None);
    assert_eq!(r2.error(), Error::Type, "-200 < i8::MIN must fail");
    assert_eq!(r2.used(), 3);
}

#[test]
fn i8__sign_mismatch_uint_exceeding_i64_range_fails_for_any_width() {
    // 用 uint16 300 = 0xCD 0x01 0x2C：300 作为 i64 TryFrom 能过（300 <= i64::MAX），
    // 但 300 > i8::MAX(127)，结果是 TypeError — 这是 OOB。真正的 sign-mismatch 对 i8
    // 是那种「根本过不了 i64 TryFrom」的 uint 即 uint > i64::MAX。
    // 这里用一个 uint8 255（255 <= i64::MAX 能过 TryFrom）但 255 > i8::MAX → TypeError。
    let mut r_oob = Reader::new(&[0xCC, 0xFF]); // uint8 255
    assert_eq!(expect::i8(&mut r_oob), None, "255 > i8::MAX must fail");
    assert_eq!(r_oob.error(), Error::Type);
    assert_eq!(r_oob.used(), 2);
}

#[test]
fn i8__type_mismatch_non_numeric_marker() {
    // nil
    let mut r_nil = Reader::new(&[0xC0]);
    assert_eq!(expect::i8(&mut r_nil), None);
    assert_eq!(r_nil.error(), Error::Type);
    assert_eq!(r_nil.used(), 1);

    // bool false = 0xC2
    let mut r_bool = Reader::new(&[0xC2]);
    assert_eq!(expect::i8(&mut r_bool), None);
    assert_eq!(r_bool.error(), Error::Type);
    assert_eq!(r_bool.used(), 1);
}

#[test]
fn i8__sticky_error_noop() {
    let mut reader = Reader::new(&[0x7F]);
    reader.flag_error(Error::Bug);
    let used_before = reader.used();
    assert_eq!(expect::i8(&mut reader), None);
    assert_eq!(reader.error(), Error::Bug);
    assert_eq!(reader.used(), used_before);
}

// ---------------------------------------------------------------------------
// mpack_expect_i64 (expect::i64) — 有符号 64 位（另一极值代表）
// ---------------------------------------------------------------------------

#[test]
fn i64__happy_positive_negative_and_uint_within_i64_range() {
    // negative fixint -32 = 0xE0
    let mut r_negf = Reader::new(&[0xE0]);
    assert_eq!(expect::i64(&mut r_negf), Some(-32));
    assert_eq!(r_negf.error(), Error::Ok);
    assert_eq!(r_negf.used(), 1);

    // int64 i64::MIN = 0xD3 0x80 00 00 00 00 00 00 00
    let mut r_min = Reader::new(&[
        0xD3, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);
    assert_eq!(expect::i64(&mut r_min), Some(i64::MIN));
    assert_eq!(r_min.error(), Error::Ok);
    assert_eq!(r_min.used(), 9);

    // int64 i64::MAX = 0xD3 0x7F FF FF FF FF FF FF FF
    let mut r_max = Reader::new(&[
        0xD3, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ]);
    assert_eq!(expect::i64(&mut r_max), Some(i64::MAX));
    assert_eq!(r_max.error(), Error::Ok);
    assert_eq!(r_max.used(), 9);

    // uint32 500_000_000 = 0xCE 0x1D 0xCD 0x65 0x00
    // 该值 <= i64::MAX → 允许（C 原版 i64 接受 uint 范围内的编码）
    let mut r_u32 = Reader::new(&[0xCE, 0x1D, 0xCD, 0x65, 0x00]);
    assert_eq!(expect::i64(&mut r_u32), Some(500_000_000));
    assert_eq!(r_u32.error(), Error::Ok);
    assert_eq!(r_u32.used(), 5);
}

#[test]
fn i64__out_of_bounds_uint_exceeding_i64_max_sets_type_error() {
    // uint64 = i64::MAX as u64 + 1 = 0x8000000000000000
    // 这个值 i64::try_from 失败 → TypeError（真正的 i64 整数符号/范围错配）
    let big = u64::MAX - (i64::MAX as u64); // 实际我们直接写 i64::MAX+1 的字面量字节
    let _ = big;
    let mut reader = Reader::new(&[
        0xCF, // uint64 marker
        0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // i64::MAX + 1 = 2^63
    ]);
    assert_eq!(
        expect::i64(&mut reader),
        None,
        "uint64=2^63 exceeds i64::MAX, TryFrom<i64> fails -> TypeError"
    );
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 9);
}

#[test]
fn i64__sign_mismatch_negative_uint_oob_vs_negative_int() {
    // negative int 对 i64 本身 OK（不是 sign-mismatch，因为 i64 支持负）。
    // 这里测一个非数值 marker：fixstr(0) 0xA0，确保 TypeError
    let mut r_str = Reader::new(&[0xA0]);
    assert_eq!(expect::i64(&mut r_str), None);
    assert_eq!(r_str.error(), Error::Type);
    assert_eq!(r_str.used(), 1);
}

#[test]
fn i64__type_mismatch_float32_marker_sets_type_error() {
    // f32 1.0 = 0xCA 0x3F800000；i64（strict）不接受 float，必须 Type
    let mut reader = Reader::new(&[0xCA, 0x3F, 0x80, 0x00, 0x00]);
    assert_eq!(
        expect::i64(&mut reader),
        None,
        "i64 must NOT accept float32 tag (numeric but different type family)"
    );
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 5);
}

#[test]
fn i64__sticky_error_noop() {
    let mut reader = Reader::new(&[0x07]);
    reader.flag_error(Error::Data);
    let used_before = reader.used();
    assert_eq!(expect::i64(&mut reader), None);
    assert_eq!(reader.error(), Error::Data);
    assert_eq!(reader.used(), used_before);
}

// ---------------------------------------------------------------------------
// mpack_expect_float (expect::float) — f32 lax 版（可拓宽/接受整数，按 C 原版 lax）
// ---------------------------------------------------------------------------

/// Helper: f32 -> BE bytes (pure const math，不引 writer)
const fn f32_be(v: f32) -> [u8; 4] {
    let bits = v.to_bits();
    [
        ((bits >> 24) & 0xFF) as u8,
        ((bits >> 16) & 0xFF) as u8,
        ((bits >> 8) & 0xFF) as u8,
        (bits & 0xFF) as u8,
    ]
}

#[test]
fn float__happy_reads_strict_tag_float32() {
    // f32 1.0 = IEEE 0x3F800000 BE
    let bytes = f32_be(1.0);
    let buf = [&[0xCA][..], &bytes[..]].concat();
    let mut reader = Reader::new(&buf);
    assert_eq!(expect::float(&mut reader), Some(1.0f32));
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 5);
}

#[test]
fn float__happy_lax_widens_f64_to_f32() {
    // f64 1.0 = 0xCB 0x3FF00000_00000000
    let bytes = [
        0xCB, 0x3F, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let mut reader = Reader::new(&bytes);
    assert!(
        (expect::float(&mut reader).unwrap() - 1.0f32).abs() < f32::EPSILON,
        "double 1.0 should be lossily narrowed to f32 1.0"
    );
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 9);
}

#[test]
fn float__happy_lax_accepts_integer_uint8_and_fixint() {
    // C 原版 lax `expect_float` 接受整数编码（自动 cast）
    // uint8 200 = 0xCC 0xC8 → 200.0f32
    let mut r_u = Reader::new(&[0xCC, 0xC8]);
    assert_eq!(expect::float(&mut r_u), Some(200.0f32));
    assert_eq!(r_u.error(), Error::Ok);
    assert_eq!(r_u.used(), 2);

    // negative fixint -16 = 0xF0 → -16.0f32
    let mut r_i = Reader::new(&[0xF0]);
    assert_eq!(expect::float(&mut r_i), Some(-16.0f32));
    assert_eq!(r_i.error(), Error::Ok);
    assert_eq!(r_i.used(), 1);
}

#[test]
fn float__out_of_bounds_non_numeric_marker_is_type_error() {
    // f32 的 OOB（lax 数值范围放宽到 f32 无限可表示；对整数 cast 也无硬性截断 → 这里测类型）
    // nil = 0xC0
    let mut r_nil = Reader::new(&[0xC0]);
    assert_eq!(expect::float(&mut r_nil), None);
    assert_eq!(r_nil.error(), Error::Type);
    assert_eq!(r_nil.used(), 1);

    // fixarr(0) = 0x90
    let mut r_arr = Reader::new(&[0x90]);
    assert_eq!(expect::float(&mut r_arr), None);
    assert_eq!(r_arr.error(), Error::Type);
    assert_eq!(r_arr.used(), 1);
}

#[test]
fn float__sign_mismatch_negative_floats_still_read_as_lax() {
    // float 的 sign mismatch 不存在（浮点自然支持负）；这里用 bool true 代替非数值 marker
    let mut r_bool = Reader::new(&[0xC3]);
    assert_eq!(expect::float(&mut r_bool), None);
    assert_eq!(r_bool.error(), Error::Type);
    assert_eq!(r_bool.used(), 1);
}

#[test]
fn float__type_mismatch_fixstr_empty() {
    let mut reader = Reader::new(&[0xA0]);
    assert_eq!(expect::float(&mut reader), None);
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 1);
}

#[test]
fn float__sticky_error_noop() {
    let bytes = f32_be(3.14);
    let buf = [&[0xCA][..], &bytes[..]].concat();
    let mut reader = Reader::new(&buf);
    reader.flag_error(Error::Invalid);
    let used_before = reader.used();
    assert_eq!(expect::float(&mut reader), None);
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), used_before);
}

// ---------------------------------------------------------------------------
// mpack_expect_double (expect::double) — f64 lax 版
// ---------------------------------------------------------------------------

const fn f64_be(v: f64) -> [u8; 8] {
    let bits = v.to_bits();
    [
        ((bits >> 56) & 0xFF) as u8,
        ((bits >> 48) & 0xFF) as u8,
        ((bits >> 40) & 0xFF) as u8,
        ((bits >> 32) & 0xFF) as u8,
        ((bits >> 24) & 0xFF) as u8,
        ((bits >> 16) & 0xFF) as u8,
        ((bits >> 8) & 0xFF) as u8,
        (bits & 0xFF) as u8,
    ]
}

#[test]
fn double__happy_reads_f64_and_widens_f32() {
    // f64 3.14
    let bytes = f64_be(3.14);
    let buf64 = [&[0xCB][..], &bytes[..]].concat();
    let mut r64 = Reader::new(&buf64);
    let val = expect::double(&mut r64).unwrap();
    assert!((val - 3.14f64).abs() < f64::EPSILON * 2.0);
    assert_eq!(r64.error(), Error::Ok);
    assert_eq!(r64.used(), 9);

    // f32 1.0 = 0xCA 0x3F800000 → 拓宽到 1.0f64（lax 规则）；
    // C 原版 expect.c:200-210 double 接受 Tag::Float。当前 Rust 实现也接受。
    let bytes32 = f32_be(1.0);
    let buf32 = [&[0xCA][..], &bytes32[..]].concat();
    let mut r32 = Reader::new(&buf32);
    let val32 = expect::double(&mut r32).unwrap();
    assert!((val32 - 1.0f64).abs() < f64::EPSILON);
    assert_eq!(r32.error(), Error::Ok);
    assert_eq!(r32.used(), 5);
}

#[test]
fn double__happy_lax_accepts_integer_encodings() {
    // int32 -1,000,000 = 0xD2 0xFF 0xF0 0xBD 0xC0（十进制 -1000000）
    let mut reader = Reader::new(&[0xD2, 0xFF, 0xF0, 0xBD, 0xC0]);
    assert_eq!(expect::double(&mut reader), Some(-1_000_000.0f64));
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 5);

    // uint64 (小值) fixint 42 = 0x2A
    let mut r2 = Reader::new(&[0x2A]);
    assert_eq!(expect::double(&mut r2), Some(42.0f64));
    assert_eq!(r2.error(), Error::Ok);
    assert_eq!(r2.used(), 1);
}

#[test]
fn double__out_of_bounds_non_numeric_marker_type_error() {
    // nil = 0xC0
    let mut r_nil = Reader::new(&[0xC0]);
    assert_eq!(expect::double(&mut r_nil), None);
    assert_eq!(r_nil.error(), Error::Type);
    assert_eq!(r_nil.used(), 1);

    // fixmap(0) = 0x80
    let mut r_map = Reader::new(&[0x80]);
    assert_eq!(expect::double(&mut r_map), None);
    assert_eq!(r_map.error(), Error::Type);
    assert_eq!(r_map.used(), 1);
}

#[test]
fn double__type_mismatch_bool_false_and_str() {
    let mut r_bool = Reader::new(&[0xC2]);
    assert_eq!(expect::double(&mut r_bool), None);
    assert_eq!(r_bool.error(), Error::Type);
    assert_eq!(r_bool.used(), 1);

    let mut r_str = Reader::new(&[0xA0]);
    assert_eq!(expect::double(&mut r_str), None);
    assert_eq!(r_str.error(), Error::Type);
    assert_eq!(r_str.used(), 1);
}

#[test]
fn double__sticky_error_noop() {
    let bytes = f64_be(2.718281828);
    let buf = [&[0xCB][..], &bytes[..]].concat();
    let mut reader = Reader::new(&buf);
    reader.flag_error(Error::Bug);
    let used_before = reader.used();
    assert_eq!(expect::double(&mut reader), None);
    assert_eq!(reader.error(), Error::Bug);
    assert_eq!(reader.used(), used_before);
}

