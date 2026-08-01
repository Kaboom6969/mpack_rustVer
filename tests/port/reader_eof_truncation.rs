use mpack::{
    common::Error,
    reader::Reader,
};

#[test]
fn tag_header_truncation_is_atomic() {
    let mut u16_missing = Reader::new(&[0xcd, 0x12]);
    assert_eq!(u16_missing.read_tag(), None);
    assert_eq!(u16_missing.error(), Error::Invalid);
    assert_eq!(u16_missing.used(), 0);

    let mut u64_missing = Reader::new(&[0xcf, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(u64_missing.read_tag(), None);
    assert_eq!(u64_missing.error(), Error::Invalid);
    assert_eq!(u64_missing.used(), 0);

    let mut ext_missing_type = Reader::new(&[0xd4]);
    assert_eq!(ext_missing_type.read_tag(), None);
    assert_eq!(ext_missing_type.error(), Error::Invalid);
    assert_eq!(ext_missing_type.used(), 0);

    let mut str16_missing_len = Reader::new(&[0xda, 0x01]);
    assert_eq!(str16_missing_len.read_str_header(), None);
    assert_eq!(str16_missing_len.error(), Error::Invalid);
    assert_eq!(str16_missing_len.used(), 0);
}

#[test]
fn payload_truncation_on_skip_is_atomic_and_sticky() {
    let mut reader = Reader::new(&[0xc4, 3, 1, 2]);

    assert_eq!(reader.read_bin_header(), Some(3));
    assert_eq!(reader.used(), 2);

    assert!(!reader.skip_bytes(3));
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), 2);

    assert_eq!(reader.read_tag(), None);
    assert_eq!(reader.used(), 2);
}

#[test]
fn discard_truncation_does_not_overconsume() {
    let mut reader = Reader::new(&[0x92, 0x2a]);

    reader.discard();
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), 1);

    reader.discard();
    assert_eq!(reader.used(), 1);
}
