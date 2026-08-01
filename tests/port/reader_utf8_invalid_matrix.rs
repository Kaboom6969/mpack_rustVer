use mpack::{
    common::Error,
    reader::{self, Reader},
};

fn assert_invalid_str_payload(payload: &[u8]) {
    assert!(payload.len() <= 31);
    let mut data = Vec::with_capacity(1 + payload.len());
    data.push(0xa0 | payload.len() as u8);
    data.extend_from_slice(payload);
    let mut reader = Reader::new(&data);

    assert_eq!(reader.read_str_header(), Some(payload.len() as u32));
    assert_eq!(reader.used(), 1);
    assert_eq!(reader.read_bytes_utf8(payload.len()), None);
    assert_eq!(reader.error(), Error::Type);
    assert_eq!(reader.used(), 1);
    assert_eq!(reader.read_tag(), None);
    assert_eq!(reader.used(), 1);
}

#[test]
fn rejects_invalid_utf8_payload_matrix() {
    let cases: &[&[u8]] = &[
        &[0x80],
        &[0xc0, 0xaf],
        &[0xc2],
        &[0xe0, 0x80, 0x80],
        &[0xe2, 0x82],
        &[0xed, 0xa0, 0x80],
        &[0xf0, 0x80, 0x80, 0x80],
        &[0xf4, 0x90, 0x80, 0x80],
        &[0xf5, 0x80, 0x80, 0x80],
        &[0xff],
    ];

    for payload in cases {
        assert!(!reader::check_utf8(payload));
        assert_invalid_str_payload(payload);
    }
}

#[test]
fn check_utf8_no_null_rejects_interior_null() {
    assert!(reader::check_utf8(&[b'a', 0, b'b']));
    assert!(!reader::check_utf8_no_null(&[b'a', 0, b'b']));
}
