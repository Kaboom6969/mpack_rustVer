use mpack::{
    common::Error,
    reader::Reader,
};

#[test]
fn peek_tag_on_reserved_marker_sets_error_without_consuming() {
    let mut reader = Reader::new(&[0xc1, 0xc3]);

    assert_eq!(reader.peek_tag(), None);
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), 0);

    assert_eq!(reader.read_tag(), None);
    assert_eq!(reader.used(), 0);
}

#[test]
fn discard_is_noop_after_error() {
    let mut reader = Reader::new(&[0xcf, 1, 2, 3]);

    reader.discard();
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), 0);

    reader.discard();
    assert_eq!(reader.used(), 0);
}

#[test]
fn invalid_utf8_is_atomic_and_error_is_sticky() {
    let mut reader = Reader::new(&[0xa2, 0xc0, 0xaf]);

    assert_eq!(reader.read_str_header(), Some(2));
    assert_eq!(reader.used(), 1);

    assert_eq!(reader.read_bytes_utf8(2), None);
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 1);

    assert_eq!(reader.read_tag(), None);
    assert_eq!(reader.used(), 1);

    reader.flag_error(Error::Invalid);
    assert_eq!(reader.error(), Error::Type);
}
