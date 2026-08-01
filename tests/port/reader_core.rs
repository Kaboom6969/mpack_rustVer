use mpack::{
    common::{Error, Tag, Type},
    reader::Reader,
};

#[test]
fn decodes_fixed_and_explicit_scalar_tags() {
    let data = [
        0xc0, 0xc2, 0xc3, 0x2a, 0xff, 0xcc, 0x80, 0xcd, 0x12, 0x34, 0xce, 0x12, 0x34, 0x56, 0x78,
        0xcf, 1, 2, 3, 4, 5, 6, 7, 8, 0xd0, 0x80, 0xd1, 0x80, 0x00, 0xd2, 0x80, 0, 0, 0, 0xd3,
        0x80, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut reader = Reader::new(&data);

    assert_eq!(reader.read_tag(), Some(Tag::Nil));
    assert_eq!(reader.read_tag(), Some(Tag::Bool(false)));
    assert_eq!(reader.read_tag(), Some(Tag::Bool(true)));
    assert_eq!(reader.read_tag(), Some(Tag::Uint(42)));
    assert_eq!(reader.read_tag(), Some(Tag::Int(-1)));
    assert_eq!(reader.read_tag(), Some(Tag::Uint(128)));
    assert_eq!(reader.read_tag(), Some(Tag::Uint(0x1234)));
    assert_eq!(reader.read_tag(), Some(Tag::Uint(0x1234_5678)));
    assert_eq!(reader.read_tag(), Some(Tag::Uint(0x0102_0304_0506_0708)));
    assert_eq!(reader.read_tag(), Some(Tag::Int(-128)));
    assert_eq!(reader.read_tag(), Some(Tag::Int(-32_768)));
    assert_eq!(reader.read_tag(), Some(Tag::Int(i32::MIN as i64)));
    assert_eq!(reader.read_tag(), Some(Tag::Int(i64::MIN)));
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn decodes_float_bits_without_canonicalizing() {
    let data = [
        0xca, 0x7f, 0xc0, 0x00, 0x01, 0xcb, 0x3f, 0xf0, 0, 0, 0, 0, 0, 0,
    ];
    let mut reader = Reader::new(&data);

    let Some(Tag::Float(single)) = reader.read_tag() else {
        panic!("expected float tag");
    };
    assert_eq!(single.to_bits(), 0x7fc0_0001);
    assert_eq!(reader.read_f64(), Some(1.0));
    assert_eq!(reader.error(), Error::Ok);
}

#[test]
fn reads_all_compound_header_families() {
    let data = [
        0xa3, 0xd9, 0x20, 0xda, 0x01, 0x00, 0xdb, 0, 0, 1, 1, 0xc4, 3, 0xc5, 1, 0, 0xc6, 0, 0, 1,
        1, 0x92, 0xdc, 0, 16, 0xdd, 0, 0, 0, 17, 0x81, 0xde, 0, 16, 0xdf, 0, 0, 0, 17,
    ];
    let mut reader = Reader::new(&data);

    assert_eq!(reader.read_str_header(), Some(3));
    assert_eq!(reader.read_str_header(), Some(32));
    assert_eq!(reader.read_str_header(), Some(256));
    assert_eq!(reader.read_str_header(), Some(257));
    assert_eq!(reader.read_bin_header(), Some(3));
    assert_eq!(reader.read_bin_header(), Some(256));
    assert_eq!(reader.read_bin_header(), Some(257));
    assert_eq!(reader.read_array_header(), Some(2));
    assert_eq!(reader.read_array_header(), Some(16));
    assert_eq!(reader.read_array_header(), Some(17));
    assert_eq!(reader.read_map_header(), Some(1));
    assert_eq!(reader.read_map_header(), Some(16));
    assert_eq!(reader.read_map_header(), Some(17));
    assert_eq!(reader.error(), Error::Ok);
}

#[test]
fn reads_extension_headers_and_leaves_payload() {
    let data = [
        0xd4, 0xff, 0xaa, 0xd8, 0x7f, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0xc7,
        3, 0xfe, 1, 2, 3, 0xc8, 0, 2, 4, 9, 8, 0xc9, 0, 0, 0, 1, 5, 7,
    ];
    let mut reader = Reader::new(&data);

    assert_eq!(reader.read_ext_header(), Some((-1, 1)));
    assert_eq!(reader.read_bytes(1), Some(&[0xaa][..]));
    assert_eq!(reader.read_ext_header(), Some((127, 16)));
    assert!(reader.skip_bytes(16));
    assert_eq!(reader.read_ext_header(), Some((-2, 3)));
    assert_eq!(reader.read_bytes(3), Some(&[1, 2, 3][..]));
    assert_eq!(reader.read_ext_header(), Some((4, 2)));
    assert!(reader.skip_bytes(2));
    assert_eq!(reader.read_ext_header(), Some((5, 1)));
    assert_eq!(reader.read_bytes(1), Some(&[7][..]));
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn typed_scalar_reads_allow_lossless_integer_conversion() {
    let data = [
        0xc0, 0xc3, 0xd1, 0, 42, 0xcc, 42, 0xca, 0x3f, 0x80, 0, 0, 0xcb, 0x40, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut reader = Reader::new(&data);

    assert!(reader.read_nil());
    assert_eq!(reader.read_bool(), Some(true));
    assert_eq!(reader.read_u64(), Some(42));
    assert_eq!(reader.read_i64(), Some(42));
    assert_eq!(reader.read_f32(), Some(1.0));
    assert_eq!(reader.read_f64(), Some(2.0));
    assert_eq!(reader.error(), Error::Ok);
}

#[test]
fn tag_kind_reports_decoded_category() {
    assert_eq!(Tag::Str(12).kind(), Type::Str);
    assert_eq!(
        Tag::Ext {
            extension_type: -1,
            length: 4,
        }
        .kind(),
        Type::Ext
    );
}

#[test]
fn payload_truncation_is_atomic_and_error_is_sticky() {
    let data = [0xa3, b'a', b'b'];
    let mut reader = Reader::new(&data);

    assert_eq!(reader.read_str_header(), Some(3));
    assert_eq!(reader.used(), 1);
    assert_eq!(reader.read_bytes(3), None);
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), 1);
    assert_eq!(reader.read_tag(), None);
    assert!(!reader.skip_bytes(1));
    assert_eq!(reader.used(), 1);

    reader.flag_error(Error::Type);
    assert_eq!(reader.error(), Error::Invalid);
}

#[test]
fn malformed_or_wrong_type_sets_the_first_error() {
    let mut reserved = Reader::new(&[0xc1, 0xc3]);
    assert_eq!(reserved.read_tag(), None);
    assert_eq!(reserved.error(), Error::Invalid);
    assert_eq!(reserved.used(), 1);
    assert_eq!(reserved.read_bool(), None);
    assert_eq!(reserved.used(), 1);

    let mut truncated = Reader::new(&[0xcf, 1, 2]);
    assert_eq!(truncated.read_tag(), None);
    assert_eq!(truncated.error(), Error::Invalid);
    assert_eq!(truncated.used(), 0);

    let mut wrong_type = Reader::new(&[0xc3, 0x2a]);
    assert_eq!(wrong_type.read_u64(), None);
    assert_eq!(wrong_type.error(), Error::Type);
    assert_eq!(wrong_type.used(), 1);
    assert_eq!(wrong_type.read_u64(), None);
    assert_eq!(wrong_type.used(), 1);
}
