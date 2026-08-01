use mpack::{
    common::{Error, Tag},
    reader::Reader,
};

#[test]
fn peek_does_not_consume_on_success() {
    let mut reader = Reader::new(&[0xcd, 0x12, 0x34, 0xc3]);

    assert_eq!(reader.peek_tag(), Some(Tag::Uint(0x1234)));
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 0);

    assert_eq!(reader.read_tag(), Some(Tag::Uint(0x1234)));
    assert_eq!(reader.used(), 3);
    assert_eq!(reader.read_bool(), Some(true));
    assert_eq!(reader.used(), 4);
}

#[test]
fn peek_does_not_consume_on_truncated_header() {
    let mut reader = Reader::new(&[0xcf, 1, 2, 3]);

    assert_eq!(reader.peek_tag(), None);
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), 0);

    assert_eq!(reader.read_tag(), None);
    assert_eq!(reader.used(), 0);
}

#[test]
fn peek_does_not_consume_str_payload() {
    let mut reader = Reader::new(&[0xa3, b'a', b'b', b'c', 0xc0]);

    assert_eq!(reader.peek_tag(), Some(Tag::Str(3)));
    assert_eq!(reader.used(), 0);

    assert_eq!(reader.read_str_header(), Some(3));
    assert_eq!(reader.used(), 1);
    assert_eq!(reader.read_bytes(3), Some(&[b'a', b'b', b'c'][..]));
    assert_eq!(reader.used(), 4);

    assert!(reader.read_nil());
    assert_eq!(reader.used(), 5);
}

#[test]
fn discard_reads_from_original_position_after_peek() {
    let mut reader = Reader::new(&[0x91, 0x2a, 0xc3]);

    assert_eq!(reader.peek_tag(), Some(Tag::Array(1)));
    assert_eq!(reader.used(), 0);

    reader.discard();
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 2);

    assert_eq!(reader.read_bool(), Some(true));
    assert_eq!(reader.used(), 3);
}

