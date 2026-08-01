use mpack::{
    common::{Error, Tag},
    reader::Reader,
};

#[test]
fn discard_skips_nested_values_and_allows_following_reads() {
    let data = [
        0x93, 0x01, 0xa2, b'h', b'i', 0x82, 0xa1, b'a', 0xc4, 0x02, 0x01, 0x02, 0xa1, b'b',
        0x92, 0xc3, 0xc0, 0xc2,
    ];
    let mut reader = Reader::new(&data);

    assert_eq!(reader.peek_tag(), Some(Tag::Array(3)));
    assert_eq!(reader.used(), 0);

    reader.discard();
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 17);

    assert_eq!(reader.read_bool(), Some(false));
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 18);
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn discard_nested_truncation_sets_error_without_overconsuming() {
    let data = [0x92, 0xa3, b'a', b'b'];
    let mut reader = Reader::new(&data);

    reader.discard();
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), 2);

    reader.discard();
    assert_eq!(reader.used(), 2);
}
