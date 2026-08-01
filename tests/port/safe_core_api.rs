//! Smoke tests for the frozen Reader + Expect safe-core API surface.

use mpack::common::{Error, Tag, Timestamp};
use mpack::expect::{self, ExpectCompound};
use mpack::reader::{self, Reader};

#[test]
fn reader_peek_discard_and_utf8_surface() {
    let data = [0xa3, b'a', b'b', b'c', 0xc0, 0x92, 0xc2, 0xc3];
    let mut reader = Reader::new(&data);

    assert_eq!(reader.peek_tag(), Some(Tag::Str(3)));
    assert_eq!(reader.used(), 0);
    assert_eq!(reader.read_str_header(), Some(3));
    assert_eq!(reader.read_bytes_utf8(3), Some(&b"abc"[..]));
    assert!(reader::check_utf8(b"abc"));
    assert!(!reader::check_utf8_no_null(&[b'a', 0, b'b']));

    reader.discard(); // nil
    reader.discard(); // array of two bools
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn reader_timestamp_surface() {
    let data = [0, 0, 0, 42];
    let mut reader = Reader::new(&data);
    assert_eq!(
        reader.read_timestamp(4),
        Some(Timestamp {
            seconds: 42,
            nanoseconds: 0
        })
    );
}

#[test]
fn expect_locked_surface_smoke() {
    let data = [
        0x2a, // 42
        0xc0, // nil
        0xa2, b'h', b'i', // "hi"
        0x81, 0x00, 0xc3, // map 1: key 0 -> true
    ];
    let mut reader = Reader::new(&data);

    assert_eq!(expect::u8(&mut reader), Some(42));
    assert!(expect::nil(&mut reader));

    let mut buf = [0u8; 8];
    assert_eq!(expect::str_buf(&mut reader, &mut buf), Some(2));
    assert_eq!(&buf[..2], b"hi");

    assert_eq!(
        expect::map_or_nil(&mut reader),
        Some(ExpectCompound {
            is_nil: false,
            count: 1
        })
    );
    let mut found = [false; 1];
    assert_eq!(expect::key_uint(&mut reader, &mut found), Some(0));
    assert_eq!(expect::r#bool(&mut reader), Some(true));
    assert_eq!(reader.error(), Error::Ok);
}
