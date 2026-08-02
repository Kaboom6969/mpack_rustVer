//! Expect safe-core atomic tests: Phase 1 (nil, bool, true_, false_)
//!
//! All fixtures are hand-written byte literals to avoid writer-side coupling.
//! Naming uses `{fn_name}__{scenario}` so tests can be filtered by prefix under
//! `--test-threads=1`.
#![allow(non_snake_case)]

use mpack::common::{Error, Tag, Timestamp};
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
    // Put the reader into sticky Invalid state first, then present a valid nil.
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
    // Empty slice: read_tag has no bytes available.
    let mut reader = Reader::new(&[]);
    assert!(
        !expect::nil(&mut reader),
        "EOF must return false without panicking"
    );
    // Reader::parse_tag currently sets Error::Invalid when no marker exists.
    // Safe-core does not change the reader public surface, so we only assert
    // "not Ok + nothing consumed" here.
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
    // 0x00 = fixuint 0, 0xa0 = empty fixstr, 0xc0 = nil — none are bool.
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
    // 0xc2 = valid bool, but we expect true, so this is a mismatch.
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
    // 0x01 = uint 1. Even though it is logically "truthy", this must still
    // be TypeError. The original C implementation (expect.c:327-342) makes
    // that behavior explicit.
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
// Phase 2: basic numeric reads (u8, u64, i8, i64, f32, f64)
//
// Byte encodings are all hand-written literals with zero writer coupling.
// MessagePack encoding cheat sheet:
//   fixint 0..127          : 0x00..0x7F          (1B, Tag::Uint or compatible Tag::Int>=0)
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
//   fixstr(0) / fixarr(0)  : 0xA0 / 0x90         (1B marker, no payload)
// ===========================================================================

// ---------------------------------------------------------------------------
// mpack_expect_u8 (expect::u8) — unsigned 8-bit
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

    // uint8 boundary: u8::MAX = 255 (0xCC 0xFF)
    let mut r3 = Reader::new(&[0xCC, 0xFF]);
    assert_eq!(expect::u8(&mut r3), Some(255));
    assert_eq!(r3.error(), Error::Ok);
    assert_eq!(r3.used(), 2);
}

#[test]
fn u8__out_of_bounds_uint16_256_sets_type_error() {
    // Explicitly requested case: uint16 256 = 0xCD 0x01 0x00 -> expect_u8 must fail.
    let mut reader = Reader::new(&[0xCD, 0x01, 0x00]);
    assert_eq!(expect::u8(&mut reader), None, "256 must NOT fit in u8");
    assert_eq!(
        reader.error(),
        Error::Type,
        "OOB integer -> Error::Type (not Ok / not silent truncate)"
    );
    // Marker + 2B payload are still consumed = 3B total; the type header was
    // already consumed by reader.read_tag().
    assert_eq!(reader.used(), 3);
}

#[test]
fn u8__sign_mismatch_negative_int_sets_type_error() {
    // int8 -1 = 0xD0 0xFF; unsigned reads of negatives must be TypeError, not
    // silently truncated to 255.
    let mut reader = Reader::new(&[0xD0, 0xFF]);
    assert_eq!(expect::u8(&mut reader), None, "negative -1 must NOT fit in u8");
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 2);

    // negative fixint -32 = 0xE0 (1B) must also raise Type.
    let mut r2 = Reader::new(&[0xE0]);
    assert_eq!(expect::u8(&mut r2), None);
    assert_eq!(r2.error(), Error::Type);
    assert_eq!(r2.used(), 1);
}

#[test]
fn u8__type_mismatch_non_numeric_marker_sets_type_error() {
    // empty fixstr = 0xA0
    let mut r_str = Reader::new(&[0xA0]);
    assert_eq!(expect::u8(&mut r_str), None);
    assert_eq!(r_str.error(), Error::Type);
    assert_eq!(r_str.used(), 1);

    // empty fixarr = 0x90
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
    // Enter Error::Data state first, then present valid uint8=9.
    let mut reader = Reader::new(&[0x09]);
    reader.flag_error(Error::Data);
    let used_before = reader.used();

    assert_eq!(expect::u8(&mut reader), None, "sticky error must return None");
    assert_eq!(reader.error(), Error::Data, "first sticky error must NOT be overwritten");
    assert_eq!(reader.used(), used_before, "sticky error must consume 0 bytes");
}

// ---------------------------------------------------------------------------
// mpack_expect_u64 (expect::u64) — unsigned 64-bit (the other extreme-width representative)
// ---------------------------------------------------------------------------

#[test]
fn u64__happy_fixint_uint32_uint64_max() {
    // fixint 0 = 0x00 (smallest uint)
    let mut r0 = Reader::new(&[0x00]);
    assert_eq!(expect::u64(&mut r0), Some(0));
    assert_eq!(r0.error(), Error::Ok);
    assert_eq!(r0.used(), 1);

    // uint32 = 0xCE 0x12 0x34 0x56 0x78 = 0x12345678 = 305,419,896
    let mut r32 = Reader::new(&[0xCE, 0x12, 0x34, 0x56, 0x78]);
    assert_eq!(expect::u64(&mut r32), Some(0x12345678));
    assert_eq!(r32.error(), Error::Ok);
    assert_eq!(r32.used(), 5);

    // uint64 MAX = 0xCF 0xFF..(8 bytes) = u64::MAX
    let mut r_max = Reader::new(&[
        0xCF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ]);
    assert_eq!(expect::u64(&mut r_max), Some(u64::MAX));
    assert_eq!(r_max.error(), Error::Ok);
    assert_eq!(r_max.used(), 9);
}

#[test]
fn u64__sign_mismatch_negative_int_sets_type_error() {
    // int64 -1 = 0xD3 0xFF..; u64 must reject this and must not coerce it to
    // u64::MAX.
    let mut reader = Reader::new(&[
        0xD3, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ]);
    assert_eq!(expect::u64(&mut reader), None, "negative -1 must NOT be coerced to u64::MAX");
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 9);

    // negative fixint -1 = 0xFF (1 byte) must also raise Type.
    let mut r2 = Reader::new(&[0xFF]);
    assert_eq!(expect::u64(&mut r2), None);
    assert_eq!(r2.error(), Error::Type);
    assert_eq!(r2.used(), 1);
}

#[test]
fn u64__out_of_bounds_non_integer_marker_sets_type_error() {
    // u64 is the widest integer type, so there is no integer encoding above
    // u64. Here, out-of-bounds is effectively equivalent to a non-integer tag.
    // bool true = 0xC3
    let mut r_bool = Reader::new(&[0xC3]);
    assert_eq!(expect::u64(&mut r_bool), None);
    assert_eq!(r_bool.error(), Error::Type);
    assert_eq!(r_bool.used(), 1);

    // f32 marker = 0xCA + 4B (the concrete value 1.0 here is arbitrary)
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
// mpack_expect_i8 (expect::i8) — signed 8-bit
// ---------------------------------------------------------------------------

#[test]
fn i8__happy_positive_fixint_and_neg_int8_boundary() {
    // fixint 0 = 0x00
    let mut r0 = Reader::new(&[0x00]);
    assert_eq!(expect::i8(&mut r0), Some(0));
    assert_eq!(r0.error(), Error::Ok);
    assert_eq!(r0.used(), 1);

    // int8 -128 = 0xD0 0x80 (i8::MIN, in range)
    let mut r_min = Reader::new(&[0xD0, 0x80]);
    assert_eq!(expect::i8(&mut r_min), Some(i8::MIN));
    assert_eq!(r_min.error(), Error::Ok);
    assert_eq!(r_min.used(), 2);

    // int8 127 = 0xD0 0x7F (i8::MAX, in range)
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
    // int16 200 = 0xD1 0x00 0xC8. Since 200 > i8::MAX(127), this must be TypeError.
    let mut reader = Reader::new(&[0xD1, 0x00, 0xC8]);
    assert_eq!(expect::i8(&mut reader), None);
    assert_eq!(reader.error(), Error::Type, "200 > i8::MAX must fail");
    assert_eq!(reader.used(), 3);

    // int16 -200 = 0xD1 0xFF 0x38. Since -200 < i8::MIN(-128), this must also be Type.
    let mut r2 = Reader::new(&[0xD1, 0xFF, 0x38]);
    assert_eq!(expect::i8(&mut r2), None);
    assert_eq!(r2.error(), Error::Type, "-200 < i8::MIN must fail");
    assert_eq!(r2.used(), 3);
}

#[test]
fn i8__sign_mismatch_uint_exceeding_i64_range_fails_for_any_width() {
    // Use uint16 300 = 0xCD 0x01 0x2C: it passes i64::try_from because
    // 300 <= i64::MAX, but 300 > i8::MAX(127), so the result is TypeError.
    // That is an out-of-bounds case. A true sign/range mismatch for i8 would
    // be a uint that cannot even pass i64::try_from, i.e. uint > i64::MAX.
    // Here we use uint8 255: it passes TryFrom into i64, but 255 > i8::MAX,
    // so it must still be TypeError.
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
// mpack_expect_i64 (expect::i64) — signed 64-bit (the other extreme-width representative)
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
    // This value is <= i64::MAX, so it is accepted. The original C i64 expect
    // accepts uint encodings when they fit in range.
    let mut r_u32 = Reader::new(&[0xCE, 0x1D, 0xCD, 0x65, 0x00]);
    assert_eq!(expect::i64(&mut r_u32), Some(500_000_000));
    assert_eq!(r_u32.error(), Error::Ok);
    assert_eq!(r_u32.used(), 5);
}

#[test]
fn i64__out_of_bounds_uint_exceeding_i64_max_sets_type_error() {
    // uint64 = i64::MAX as u64 + 1 = 0x8000000000000000
    // This value fails i64::try_from -> TypeError, which is the real i64
    // integer sign/range mismatch case.
    let big = u64::MAX - (i64::MAX as u64); // We directly encode the literal bytes for i64::MAX + 1 below.
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
    // Negative ints are valid for i64, so they are not a sign mismatch here.
    // This instead uses a non-numeric marker, fixstr(0) = 0xA0, to confirm TypeError.
    let mut r_str = Reader::new(&[0xA0]);
    assert_eq!(expect::i64(&mut r_str), None);
    assert_eq!(r_str.error(), Error::Type);
    assert_eq!(r_str.used(), 1);
}

#[test]
fn i64__type_mismatch_float32_marker_sets_type_error() {
    // f32 1.0 = 0xCA 0x3F800000; strict i64 does not accept floats, so this must be Type.
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
// mpack_expect_float (expect::float) — lax f32 (can widen/accept integers, matching the original C behavior)
// ---------------------------------------------------------------------------

/// Helper: f32 -> BE bytes (pure const math, no writer dependency)
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
    // The original C lax `expect_float` accepts integer encodings via cast.
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
    // For lax f32, numeric range is broad enough that we use this test to
    // validate type mismatch behavior instead of numeric overflow.
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
    // Float has no sign-mismatch case in this sense because floats naturally
    // support negatives; use bool true here as a non-numeric marker instead.
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
// mpack_expect_double (expect::double) — lax f64
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

    // f32 1.0 = 0xCA 0x3F800000 -> widened to 1.0f64 by the lax rule.
    // The original C expect.c:200-210 accepts Tag::Float for double, and the
    // current Rust implementation does as well.
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
    // int32 -1,000,000 = 0xD2 0xFF 0xF0 0xBD 0xC0 (decimal -1000000)
    let mut reader = Reader::new(&[0xD2, 0xFF, 0xF0, 0xBD, 0xC0]);
    assert_eq!(expect::double(&mut reader), Some(-1_000_000.0f64));
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 5);

    // small uint value encoded as fixint 42 = 0x2A
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

// ===========================================================================
// Phase 2 (second half): numeric range / match
// ===========================================================================

// ---------------------------------------------------------------------------
// Integer range
// ---------------------------------------------------------------------------

#[test]
fn u8_range__happy_inclusive_bounds_accepts_min_mid_max() {
    let mut r_min = Reader::new(&[0x05]);
    assert_eq!(expect::u8_range(&mut r_min, 5, 10), Some(5));
    assert_eq!(r_min.error(), Error::Ok);
    assert_eq!(r_min.used(), 1);

    let mut r_mid = Reader::new(&[0x07]);
    assert_eq!(expect::u8_range(&mut r_mid, 5, 10), Some(7));
    assert_eq!(r_mid.error(), Error::Ok);
    assert_eq!(r_mid.used(), 1);

    let mut r_max = Reader::new(&[0x0A]);
    assert_eq!(expect::u8_range(&mut r_max, 5, 10), Some(10));
    assert_eq!(r_max.error(), Error::Ok);
    assert_eq!(r_max.used(), 1);
}

#[test]
fn u8_range__out_of_range_sets_type_error_after_consuming_value() {
    let mut reader = Reader::new(&[0x0B]);
    assert_eq!(expect::u8_range(&mut reader, 5, 10), None);
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 1);
}

#[test]
fn u8_range__sticky_error_is_noop() {
    let mut reader = Reader::new(&[0x07]);
    reader.flag_error(Error::Invalid);
    let used_before = reader.used();
    assert_eq!(expect::u8_range(&mut reader, 5, 10), None);
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), used_before);
}

#[test]
fn u64_range__happy_accepts_wide_uint64_boundaries() {
    let mut r_min = Reader::new(&[0xCF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00]);
    assert_eq!(expect::u64_range(&mut r_min, 256, 1024), Some(256));
    assert_eq!(r_min.error(), Error::Ok);
    assert_eq!(r_min.used(), 9);

    let mut r_max = Reader::new(&[0xCD, 0x04, 0x00]);
    assert_eq!(expect::u64_range(&mut r_max, 256, 1024), Some(1024));
    assert_eq!(r_max.error(), Error::Ok);
    assert_eq!(r_max.used(), 3);
}

#[test]
fn u64_range__below_or_above_bounds_sets_type_error() {
    let mut below = Reader::new(&[0xCC, 0x7F]);
    assert_eq!(expect::u64_range(&mut below, 128, 255), None);
    assert_eq!(below.error(), Error::Type);
    assert_eq!(below.used(), 2);

    let mut above = Reader::new(&[0xCD, 0x01, 0x00]);
    assert_eq!(expect::u64_range(&mut above, 0, 255), None);
    assert_eq!(above.error(), Error::Type);
    assert_eq!(above.used(), 3);
}

#[test]
fn i8_range__happy_inclusive_negative_and_positive_bounds() {
    let mut r_min = Reader::new(&[0xFE]); // -2
    assert_eq!(expect::i8_range(&mut r_min, -2, 2), Some(-2));
    assert_eq!(r_min.error(), Error::Ok);
    assert_eq!(r_min.used(), 1);

    let mut r_mid = Reader::new(&[0x00]);
    assert_eq!(expect::i8_range(&mut r_mid, -2, 2), Some(0));
    assert_eq!(r_mid.error(), Error::Ok);
    assert_eq!(r_mid.used(), 1);

    let mut r_max = Reader::new(&[0x02]);
    assert_eq!(expect::i8_range(&mut r_max, -2, 2), Some(2));
    assert_eq!(r_max.error(), Error::Ok);
    assert_eq!(r_max.used(), 1);
}

#[test]
fn i8_range__out_of_range_sets_type_error() {
    let mut low = Reader::new(&[0xFD]); // -3
    assert_eq!(expect::i8_range(&mut low, -2, 2), None);
    assert_eq!(low.error(), Error::Type);
    assert_eq!(low.used(), 1);

    let mut high = Reader::new(&[0x03]);
    assert_eq!(expect::i8_range(&mut high, -2, 2), None);
    assert_eq!(high.error(), Error::Type);
    assert_eq!(high.used(), 1);
}

#[test]
fn i64_range__happy_accepts_signed_boundaries() {
    let mut r_min = Reader::new(&[0xD1, 0xFE, 0xD4]); // -300
    assert_eq!(expect::i64_range(&mut r_min, -300, 300), Some(-300));
    assert_eq!(r_min.error(), Error::Ok);
    assert_eq!(r_min.used(), 3);

    let mut r_max = Reader::new(&[0xCD, 0x01, 0x2C]); // 300
    assert_eq!(expect::i64_range(&mut r_max, -300, 300), Some(300));
    assert_eq!(r_max.error(), Error::Ok);
    assert_eq!(r_max.used(), 3);
}

#[test]
fn i64_range__out_of_range_and_sticky_error_behave_correctly() {
    let mut out = Reader::new(&[0xCD, 0x01, 0x2D]); // 301
    assert_eq!(expect::i64_range(&mut out, -300, 300), None);
    assert_eq!(out.error(), Error::Type);
    assert_eq!(out.used(), 3);

    let mut sticky = Reader::new(&[0x00]);
    sticky.flag_error(Error::Data);
    let used_before = sticky.used();
    assert_eq!(expect::i64_range(&mut sticky, -1, 1), None);
    assert_eq!(sticky.error(), Error::Data);
    assert_eq!(sticky.used(), used_before);
}

// ---------------------------------------------------------------------------
// Floating-point range
// ---------------------------------------------------------------------------

#[test]
fn float_range__happy_accepts_integer_and_float_inputs_within_bounds() {
    let bytes = f32_be(1.5);
    let buf = [&[0xCA][..], &bytes[..]].concat();
    let mut r_float = Reader::new(&buf);
    assert_eq!(expect::float_range(&mut r_float, 1.0, 2.0), Some(1.5));
    assert_eq!(r_float.error(), Error::Ok);
    assert_eq!(r_float.used(), 5);

    let mut r_int = Reader::new(&[0x02]);
    assert_eq!(expect::float_range(&mut r_int, 1.0, 2.0), Some(2.0));
    assert_eq!(r_int.error(), Error::Ok);
    assert_eq!(r_int.used(), 1);
}

#[test]
fn float_range__out_of_range_and_type_mismatch_set_type_error() {
    let bytes = f32_be(2.5);
    let buf = [&[0xCA][..], &bytes[..]].concat();
    let mut out = Reader::new(&buf);
    assert_eq!(expect::float_range(&mut out, 1.0, 2.0), None);
    assert_eq!(out.error(), Error::Type);
    assert_eq!(out.used(), 5);

    let mut wrong = Reader::new(&[0xA0]);
    assert_eq!(expect::float_range(&mut wrong, 1.0, 2.0), None);
    assert_eq!(wrong.error(), Error::Type);
    assert_eq!(wrong.used(), 1);
}

#[test]
fn double_range__happy_accepts_f64_and_integer_inputs_within_bounds() {
    let bytes = f64_be(1.25);
    let buf = [&[0xCB][..], &bytes[..]].concat();
    let mut r_double = Reader::new(&buf);
    assert_eq!(expect::double_range(&mut r_double, 1.0, 2.0), Some(1.25));
    assert_eq!(r_double.error(), Error::Ok);
    assert_eq!(r_double.used(), 9);

    let mut r_int = Reader::new(&[0x02]);
    assert_eq!(expect::double_range(&mut r_int, 1.0, 2.0), Some(2.0));
    assert_eq!(r_int.error(), Error::Ok);
    assert_eq!(r_int.used(), 1);
}

#[test]
fn double_range__out_of_range_and_sticky_error_behave_correctly() {
    let bytes = f64_be(2.25);
    let buf = [&[0xCB][..], &bytes[..]].concat();
    let mut out = Reader::new(&buf);
    assert_eq!(expect::double_range(&mut out, 1.0, 2.0), None);
    assert_eq!(out.error(), Error::Type);
    assert_eq!(out.used(), 9);

    let mut sticky = Reader::new(&[0x01]);
    sticky.flag_error(Error::Bug);
    let used_before = sticky.used();
    assert_eq!(expect::double_range(&mut sticky, 0.0, 2.0), None);
    assert_eq!(sticky.error(), Error::Bug);
    assert_eq!(sticky.used(), used_before);
}

// ---------------------------------------------------------------------------
// Numeric match
// ---------------------------------------------------------------------------

#[test]
fn uint_match__happy_exact_match_accepts_and_consumes() {
    let mut reader = Reader::new(&[0x2A]); // 42
    assert!(expect::uint_match(&mut reader, 42));
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 1);
}

#[test]
fn uint_match__mismatch_sets_type_error_after_consuming() {
    let mut reader = Reader::new(&[0x2B]); // 43
    assert!(!expect::uint_match(&mut reader, 42));
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 1);
}

#[test]
fn uint_match__negative_or_wrong_type_is_false_with_sticky_semantics() {
    let mut negative = Reader::new(&[0xFF]); // -1
    assert!(!expect::uint_match(&mut negative, 42));
    assert_eq!(negative.error(), Error::Type);
    assert_eq!(negative.used(), 1);

    let mut sticky = Reader::new(&[0x2A]);
    sticky.flag_error(Error::Invalid);
    let used_before = sticky.used();
    assert!(!expect::uint_match(&mut sticky, 42));
    assert_eq!(sticky.error(), Error::Invalid);
    assert_eq!(sticky.used(), used_before);
}

#[test]
fn int_match__happy_exact_match_accepts_negative_and_positive() {
    let mut neg = Reader::new(&[0xFF]); // -1
    assert!(expect::int_match(&mut neg, -1));
    assert_eq!(neg.error(), Error::Ok);
    assert_eq!(neg.used(), 1);

    let mut pos = Reader::new(&[0x2A]); // 42
    assert!(expect::int_match(&mut pos, 42));
    assert_eq!(pos.error(), Error::Ok);
    assert_eq!(pos.used(), 1);
}

#[test]
fn int_match__mismatch_and_type_mismatch_set_type_error() {
    let mut mismatch = Reader::new(&[0x2A]); // 42
    assert!(!expect::int_match(&mut mismatch, 41));
    assert_eq!(mismatch.error(), Error::Type);
    assert_eq!(mismatch.used(), 1);

    let mut wrong_type = Reader::new(&[0xC2]); // false
    assert!(!expect::int_match(&mut wrong_type, 0));
    assert_eq!(wrong_type.error(), Error::Type);
    assert_eq!(wrong_type.used(), 1);
}

// ===========================================================================
// Phase 3: string / buffer / bin / ext
// ===========================================================================

#[test]
fn str__happy_reads_fixstr_and_str8_headers() {
    let mut fix = Reader::new(&[0xA3, b'a', b'b', b'c']);
    assert_eq!(expect::r#str(&mut fix), Some(3));
    assert_eq!(fix.error(), Error::Ok);
    assert_eq!(fix.used(), 1);

    let mut str8 = Reader::new(&[0xD9, 0x04, b't', b'e', b's', b't']);
    assert_eq!(expect::r#str(&mut str8), Some(4));
    assert_eq!(str8.error(), Error::Ok);
    assert_eq!(str8.used(), 2);
}

#[test]
fn str__non_str_type_byte_flags_type_even_when_truncated() {
    // C type-byte expect_str: int32 marker 0xd2 with only 2 trailing bytes still
    // flags Type (does not attempt a full read_tag → Invalid).
    let mut truncated_int = Reader::new(&[0xD2, 0xB4, 0x00]);
    assert_eq!(expect::r#str(&mut truncated_int), None);
    assert_eq!(truncated_int.error(), Error::Type);
    assert_eq!(truncated_int.used(), 1);

    let mut complete_bool = Reader::new(&[0xC3]);
    assert_eq!(expect::r#str(&mut complete_bool), None);
    assert_eq!(complete_bool.error(), Error::Type);
    assert_eq!(complete_bool.used(), 1);
}

#[test]
fn str_buf__happy_copies_bytes_and_zero_length() {
    let mut buf = [0xAA; 4];
    let mut reader = Reader::new(&[0xA3, b'a', b'b', b'c']);
    assert_eq!(expect::str_buf(&mut reader, &mut buf), Some(3));
    assert_eq!(&buf[..3], b"abc");
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 4);

    let mut empty = [0xAA; 1];
    let mut empty_reader = Reader::new(&[0xA0]);
    assert_eq!(expect::str_buf(&mut empty_reader, &mut empty), Some(0));
    assert_eq!(empty_reader.error(), Error::Ok);
    assert_eq!(empty_reader.used(), 1);
}

#[test]
fn str_buf__too_small_sets_toobig_and_leaves_payload_unread() {
    let mut buf = [0u8; 2];
    let mut reader = Reader::new(&[0xD9, 0x03, b'a', b'b', b'c']);
    assert_eq!(expect::str_buf(&mut reader, &mut buf), None);
    assert_eq!(reader.error(), Error::TooBig);
    assert_eq!(reader.used(), 2);
    assert_eq!(reader.remaining(), 3);
}

#[test]
fn utf8__happy_valid_and_invalid_inputs() {
    let mut buf = [0u8; 4];
    let mut valid = Reader::new(&[0xA2, 0xC3, 0xA9]); // "é"
    assert_eq!(expect::utf8(&mut valid, &mut buf), Some(2));
    assert_eq!(&buf[..2], &[0xC3, 0xA9]);
    assert_eq!(valid.error(), Error::Ok);
    assert_eq!(valid.used(), 3);

    let mut invalid_buf = [0u8; 2];
    let mut invalid = Reader::new(&[0xA2, 0xC3, 0x28]);
    assert_eq!(expect::utf8(&mut invalid, &mut invalid_buf), None);
    assert_eq!(invalid.error(), Error::Type);
    assert_eq!(invalid.used(), 3);
    assert_eq!(&invalid_buf, &[0xC3, 0x28]);
}

#[test]
fn utf8__too_small_sets_toobig_without_consuming_payload() {
    let mut buf = [0u8; 1];
    let mut reader = Reader::new(&[0xA2, 0xC3, 0xA9]);
    assert_eq!(expect::utf8(&mut reader, &mut buf), None);
    assert_eq!(reader.error(), Error::TooBig);
    assert_eq!(reader.used(), 1);
    assert_eq!(reader.remaining(), 2);
}

#[test]
fn str_match__exact_match_accepts_and_mismatch_sets_type() {
    let mut ok = Reader::new(&[0xA3, b'k', b'e', b'y']);
    assert!(expect::str_match(&mut ok, b"key"));
    assert_eq!(ok.error(), Error::Ok);
    assert_eq!(ok.used(), 4);

    let mut wrong = Reader::new(&[0xA3, b'k', b'e', b'x']);
    assert!(!expect::str_match(&mut wrong, b"key"));
    assert_eq!(wrong.error(), Error::Type);
    assert_eq!(wrong.used(), 4);
}

#[test]
fn str_match__mismatch_before_truncated_payload_flags_type() {
    // fixstr(3) with only one payload byte that mismatches expected[0]:
    // C flags Type on the first native-byte compare (not Invalid from EOF).
    let mut reader = Reader::new(&[0xA3, 0x19]);
    assert!(!expect::str_match(&mut reader, &[0, 122, 53]));
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 2);
}

#[test]
fn cstr__happy_writes_nul_terminated_bytes() {
    let mut buf = [0xAA; 4];
    let mut reader = Reader::new(&[0xA3, b'a', b'b', b'c']);
    assert!(expect::cstr(&mut reader, &mut buf));
    assert_eq!(&buf, b"abc\0");
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 4);
}

#[test]
fn cstr__embedded_nul_and_too_small_follow_c_rules() {
    let mut with_nul = [0xAA; 4];
    let mut nul_reader = Reader::new(&[0xA3, b'a', 0x00, b'b']);
    assert!(!expect::cstr(&mut nul_reader, &mut with_nul));
    assert_eq!(nul_reader.error(), Error::Type);
    assert_eq!(nul_reader.used(), 4);
    assert_eq!(with_nul[0], 0);

    let mut too_small = [0xAA; 3];
    let mut small_reader = Reader::new(&[0xA3, b'a', b'b', b'c']);
    assert!(!expect::cstr(&mut small_reader, &mut too_small));
    assert_eq!(small_reader.error(), Error::TooBig);
    assert_eq!(small_reader.used(), 1);
    assert_eq!(small_reader.remaining(), 3);
    assert_eq!(too_small[0], 0);
}

#[test]
fn utf8_cstr__happy_invalid_utf8_and_too_small() {
    let mut ok_buf = [0xAA; 3];
    let mut ok = Reader::new(&[0xA2, 0xC3, 0xA9]);
    assert!(expect::utf8_cstr(&mut ok, &mut ok_buf));
    assert_eq!(&ok_buf, &[0xC3, 0xA9, 0x00]);
    assert_eq!(ok.error(), Error::Ok);
    assert_eq!(ok.used(), 3);

    let mut invalid_buf = [0xAA; 3];
    let mut invalid = Reader::new(&[0xA2, 0xC3, 0x28]);
    assert!(!expect::utf8_cstr(&mut invalid, &mut invalid_buf));
    assert_eq!(invalid.error(), Error::Type);
    assert_eq!(invalid.used(), 3);
    assert_eq!(invalid_buf[0], 0);

    let mut too_small_buf = [0xAA; 2];
    let mut too_small = Reader::new(&[0xA2, 0xC3, 0xA9]);
    assert!(!expect::utf8_cstr(&mut too_small, &mut too_small_buf));
    assert_eq!(too_small.error(), Error::TooBig);
    assert_eq!(too_small.used(), 1);
    assert_eq!(too_small.remaining(), 2);
    assert_eq!(too_small_buf[0], 0);
}

#[test]
fn bin__happy_header_and_payload_copy() {
    let mut header = Reader::new(&[0xC4, 0x03, 1, 2, 3]);
    assert_eq!(expect::bin(&mut header), Some(3));
    assert_eq!(header.error(), Error::Ok);
    assert_eq!(header.used(), 2);

    let mut buf = [0u8; 3];
    let mut full = Reader::new(&[0xC4, 0x03, 1, 2, 3]);
    assert_eq!(expect::bin_buf(&mut full, &mut buf), Some(3));
    assert_eq!(&buf, &[1, 2, 3]);
    assert_eq!(full.error(), Error::Ok);
    assert_eq!(full.used(), 5);
}

#[test]
fn bin_buf_and_bin_size_buf__buffer_and_size_failures_match_c() {
    let mut small = [0u8; 2];
    let mut too_small = Reader::new(&[0xC4, 0x03, 1, 2, 3]);
    assert_eq!(expect::bin_buf(&mut too_small, &mut small), None);
    assert_eq!(too_small.error(), Error::TooBig);
    assert_eq!(too_small.used(), 2);
    assert_eq!(too_small.remaining(), 3);

    let mut exact = [0u8; 2];
    let mut wrong_size = Reader::new(&[0xC4, 0x03, 1, 2, 3]);
    assert!(!expect::bin_size_buf(&mut wrong_size, &mut exact, 2));
    assert_eq!(wrong_size.error(), Error::Type);
    assert_eq!(wrong_size.used(), 2);
    assert_eq!(wrong_size.remaining(), 3);
}

#[test]
fn ext__happy_and_ext_buf_too_small() {
    // fixext1 type=5 payload=0xAA
    let mut header = Reader::new(&[0xD4, 0x05, 0xAA]);
    assert_eq!(expect::ext(&mut header), Some((5, 1)));
    assert_eq!(header.error(), Error::Ok);
    assert_eq!(header.used(), 2);

    let mut ok_buf = [0u8; 1];
    let mut full = Reader::new(&[0xD4, 0x05, 0xAA]);
    assert_eq!(expect::ext_buf(&mut full, &mut ok_buf), Some((5, 1)));
    assert_eq!(&ok_buf, &[0xAA]);
    assert_eq!(full.error(), Error::Ok);
    assert_eq!(full.used(), 3);

    let mut small_buf = [];
    let mut too_small = Reader::new(&[0xD4, 0x05, 0xAA]);
    assert_eq!(expect::ext_buf(&mut too_small, &mut small_buf), None);
    assert_eq!(too_small.error(), Error::TooBig);
    assert_eq!(too_small.used(), 2);
    assert_eq!(too_small.remaining(), 1);
}

// ===========================================================================
// Phase 4: compound headers (map / array)
// ===========================================================================

#[test]
fn map_and_array__happy_headers_and_wrong_type() {
    let mut map_reader = Reader::new(&[0x82]); // fixmap(2)
    assert_eq!(expect::map(&mut map_reader), Some(2));
    assert_eq!(map_reader.error(), Error::Ok);
    assert_eq!(map_reader.used(), 1);

    let mut array_reader = Reader::new(&[0x93]); // fixarray(3)
    assert_eq!(expect::array(&mut array_reader), Some(3));
    assert_eq!(array_reader.error(), Error::Ok);
    assert_eq!(array_reader.used(), 1);

    let mut wrong_for_map = Reader::new(&[0x90]); // fixarray(0)
    assert_eq!(expect::map(&mut wrong_for_map), None);
    assert_eq!(wrong_for_map.error(), Error::Type);
    assert_eq!(wrong_for_map.used(), 1);

    let mut wrong_for_array = Reader::new(&[0x80]); // fixmap(0)
    assert_eq!(expect::array(&mut wrong_for_array), None);
    assert_eq!(wrong_for_array.error(), Error::Type);
    assert_eq!(wrong_for_array.used(), 1);
}

#[test]
fn map_and_array_range_and_match__inclusive_and_mismatch_behave_like_c() {
    let mut map_ok = Reader::new(&[0x82]); // fixmap(2)
    assert_eq!(expect::map_range(&mut map_ok, 1, 2), Some(2));
    assert_eq!(map_ok.error(), Error::Ok);
    assert_eq!(map_ok.used(), 1);

    let mut map_out = Reader::new(&[0x83]); // fixmap(3)
    assert_eq!(expect::map_range(&mut map_out, 0, 2), None);
    assert_eq!(map_out.error(), Error::Type);
    assert_eq!(map_out.used(), 1);

    let mut map_match_ok = Reader::new(&[0x81]); // fixmap(1)
    assert!(expect::map_match(&mut map_match_ok, 1));
    assert_eq!(map_match_ok.error(), Error::Ok);
    assert_eq!(map_match_ok.used(), 1);

    let mut map_match_bad = Reader::new(&[0x82]); // fixmap(2)
    assert!(!expect::map_match(&mut map_match_bad, 1));
    assert_eq!(map_match_bad.error(), Error::Type);
    assert_eq!(map_match_bad.used(), 1);

    let mut array_ok = Reader::new(&[0x92]); // fixarray(2)
    assert_eq!(expect::array_range(&mut array_ok, 1, 2), Some(2));
    assert_eq!(array_ok.error(), Error::Ok);
    assert_eq!(array_ok.used(), 1);

    let mut array_out = Reader::new(&[0x90]); // fixarray(0)
    assert_eq!(expect::array_range(&mut array_out, 1, 2), None);
    assert_eq!(array_out.error(), Error::Type);
    assert_eq!(array_out.used(), 1);

    let mut array_match_ok = Reader::new(&[0x90]); // fixarray(0)
    assert!(expect::array_match(&mut array_match_ok, 0));
    assert_eq!(array_match_ok.error(), Error::Ok);
    assert_eq!(array_match_ok.used(), 1);

    let mut array_match_bad = Reader::new(&[0x91]); // fixarray(1)
    assert!(!expect::array_match(&mut array_match_bad, 0));
    assert_eq!(array_match_bad.error(), Error::Type);
    assert_eq!(array_match_bad.used(), 1);
}

#[test]
fn map_and_array_or_nil__nil_present_and_wrong_type_cases() {
    let mut map_nil = Reader::new(&[0xC0]);
    assert_eq!(
        expect::map_or_nil(&mut map_nil),
        Some(expect::ExpectCompound {
            is_nil: true,
            count: 0,
        })
    );
    assert_eq!(map_nil.error(), Error::Ok);
    assert_eq!(map_nil.used(), 1);

    let mut map_present = Reader::new(&[0x81]); // fixmap(1)
    assert_eq!(
        expect::map_or_nil(&mut map_present),
        Some(expect::ExpectCompound {
            is_nil: false,
            count: 1,
        })
    );
    assert_eq!(map_present.error(), Error::Ok);
    assert_eq!(map_present.used(), 1);

    let mut map_wrong = Reader::new(&[0x91]); // fixarray(1)
    assert_eq!(expect::map_or_nil(&mut map_wrong), None);
    assert_eq!(map_wrong.error(), Error::Type);
    assert_eq!(map_wrong.used(), 1);

    let mut array_nil = Reader::new(&[0xC0]);
    assert_eq!(
        expect::array_or_nil(&mut array_nil),
        Some(expect::ExpectCompound {
            is_nil: true,
            count: 0,
        })
    );
    assert_eq!(array_nil.error(), Error::Ok);
    assert_eq!(array_nil.used(), 1);

    let mut array_present = Reader::new(&[0x92]); // fixarray(2)
    assert_eq!(
        expect::array_or_nil(&mut array_present),
        Some(expect::ExpectCompound {
            is_nil: false,
            count: 2,
        })
    );
    assert_eq!(array_present.error(), Error::Ok);
    assert_eq!(array_present.used(), 1);

    let mut array_wrong = Reader::new(&[0x82]); // fixmap(2)
    assert_eq!(expect::array_or_nil(&mut array_wrong), None);
    assert_eq!(array_wrong.error(), Error::Type);
    assert_eq!(array_wrong.used(), 1);
}

#[test]
fn map_and_array_max_or_nil__max_bound_and_sticky_error() {
    let mut map_ok = Reader::new(&[0x82]); // fixmap(2)
    assert_eq!(
        expect::map_max_or_nil(&mut map_ok, 2),
        Some(expect::ExpectCompound {
            is_nil: false,
            count: 2,
        })
    );
    assert_eq!(map_ok.error(), Error::Ok);
    assert_eq!(map_ok.used(), 1);

    let mut map_too_large = Reader::new(&[0x83]); // fixmap(3)
    assert_eq!(expect::map_max_or_nil(&mut map_too_large, 2), None);
    assert_eq!(map_too_large.error(), Error::Type);
    assert_eq!(map_too_large.used(), 1);

    let mut array_ok = Reader::new(&[0x92]); // fixarray(2)
    assert_eq!(
        expect::array_max_or_nil(&mut array_ok, 2),
        Some(expect::ExpectCompound {
            is_nil: false,
            count: 2,
        })
    );
    assert_eq!(array_ok.error(), Error::Ok);
    assert_eq!(array_ok.used(), 1);

    let mut array_too_large = Reader::new(&[0x93]); // fixarray(3)
    assert_eq!(expect::array_max_or_nil(&mut array_too_large, 2), None);
    assert_eq!(array_too_large.error(), Error::Type);
    assert_eq!(array_too_large.used(), 1);

    let mut sticky_map = Reader::new(&[0x80]);
    sticky_map.flag_error(Error::Bug);
    let used_before = sticky_map.used();
    assert_eq!(expect::map_or_nil(&mut sticky_map), None);
    assert_eq!(sticky_map.error(), Error::Bug);
    assert_eq!(sticky_map.used(), used_before);

    let mut sticky_array = Reader::new(&[0x90]);
    sticky_array.flag_error(Error::Data);
    let used_before = sticky_array.used();
    assert_eq!(expect::array_or_nil(&mut sticky_array), None);
    assert_eq!(sticky_array.error(), Error::Data);
    assert_eq!(sticky_array.used(), used_before);
}

#[test]
fn timestamp_and_tag__consume_expected_headers_and_report_type_mismatch() {
    let mut ts = Reader::new(&[0xD6, 0xFF, 0, 0, 0, 42]);
    assert_eq!(
        expect::timestamp(&mut ts),
        Some(Timestamp {
            seconds: 42,
            nanoseconds: 0,
        })
    );
    assert_eq!(ts.error(), Error::Ok);
    assert_eq!(ts.used(), 6);

    let mut trunc = Reader::new(&[0xD6, 0xFF, 0, 0, 0, 7]);
    assert_eq!(expect::timestamp_truncate(&mut trunc), Some(7));
    assert_eq!(trunc.error(), Error::Ok);
    assert_eq!(trunc.used(), 6);

    let mut wrong = Reader::new(&[0xD6, 0x05, 0, 0, 0, 42]);
    assert_eq!(expect::timestamp(&mut wrong), None);
    assert_eq!(wrong.error(), Error::Type);
    assert_eq!(wrong.used(), 2);

    let mut tag_ok = Reader::new(&[0x93]);
    assert!(expect::tag(&mut tag_ok, Tag::Array(3)));
    let mut tag_bad = Reader::new(&[0x92]);
    assert!(!expect::tag(&mut tag_bad, Tag::Array(3)));
    assert_eq!(tag_bad.error(), Error::Type);
}

#[test]
fn key_uint_and_key_cstr__discard_unknown_and_reject_duplicates_like_c() {
    let mut found = [false; 2];
    let mut uint_unknown = Reader::new(&[0x02, 0xC3]);
    assert_eq!(expect::key_uint(&mut uint_unknown, &mut found), Some(2));
    assert_eq!(uint_unknown.error(), Error::Ok);
    assert_eq!(uint_unknown.used(), 1);
    assert_eq!(expect::r#bool(&mut uint_unknown), Some(true));
    assert_eq!(found, [false, false]);

    let mut uint_non_uint = Reader::new(&[0xA1, b'x', 0xC3]);
    assert_eq!(expect::key_uint(&mut uint_non_uint, &mut found), Some(2));
    assert_eq!(uint_non_uint.error(), Error::Ok);
    assert_eq!(uint_non_uint.used(), 2);
    assert_eq!(expect::r#bool(&mut uint_non_uint), Some(true));

    let mut uint_dup_found = [true, false];
    let mut uint_dup = Reader::new(&[0x00]);
    assert_eq!(expect::key_uint(&mut uint_dup, &mut uint_dup_found), None);
    assert_eq!(uint_dup.error(), Error::Invalid);

    let keys = ["id", "name"];
    let mut cstr_found = [false; 2];
    let mut cstr_unknown = Reader::new(&[0xA3, b'a', b'g', b'e', 0xC3]);
    assert_eq!(expect::key_cstr(&mut cstr_unknown, &keys, &mut cstr_found), Some(2));
    assert_eq!(cstr_unknown.error(), Error::Ok);
    assert_eq!(cstr_unknown.used(), 4);
    assert_eq!(expect::r#bool(&mut cstr_unknown), Some(true));

    let mut cstr_non_str = Reader::new(&[0x01, 0xC3]);
    assert_eq!(expect::key_cstr(&mut cstr_non_str, &keys, &mut cstr_found), Some(2));
    assert_eq!(cstr_non_str.error(), Error::Ok);
    assert_eq!(cstr_non_str.used(), 1);
    assert_eq!(expect::r#bool(&mut cstr_non_str), Some(true));

    let mut cstr_dup_found = [false, true];
    let mut cstr_dup = Reader::new(&[0xA4, b'n', b'a', b'm', b'e']);
    assert_eq!(expect::key_cstr(&mut cstr_dup, &keys, &mut cstr_dup_found), None);
    assert_eq!(cstr_dup.error(), Error::Invalid);
}

