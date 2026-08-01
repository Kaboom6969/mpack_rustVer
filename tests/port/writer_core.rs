use mpack::{common::Error, writer::Writer};

#[test]
fn writes_nil_to_a_fixed_buffer() {
    let mut buffer = [0_u8; 1];
    let mut writer = Writer::new(&mut buffer);

    writer.write_nil();

    assert_eq!(writer.error(), Error::Ok);
    assert_eq!(writer.used(), 1);
    assert_eq!(writer.written(), &[0xc0]);
}

#[test]
fn capacity_error_is_sticky() {
    let mut buffer = [];
    let mut writer = Writer::new(&mut buffer);

    writer.write_nil();
    writer.write_nil();

    assert_eq!(writer.error(), Error::TooBig);
    assert_eq!(writer.used(), 0);
    assert!(writer.written().is_empty());
}

#[test]
fn writes_compact_scalar_encodings() {
    let mut buffer = [0_u8; 32];
    let mut writer = Writer::new(&mut buffer);

    writer.write_bool(true);
    writer.write_u64(128);
    writer.write_i64(-33);
    writer.write_i64(128);
    writer.write_f32_bits(0x7fc0_0001);

    assert_eq!(writer.error(), Error::Ok);
    assert_eq!(
        writer.written(),
        &[0xc3, 0xcc, 128, 0xd0, 0xdf, 0xcc, 128, 0xca, 0x7f, 0xc0, 0x00, 0x01]
    );
}

#[test]
fn writes_compound_headers_and_payloads() {
    let mut buffer = [0_u8; 64];
    let mut writer = Writer::new(&mut buffer);

    writer.write_array_header(16);
    writer.write_map_header(2);
    writer.write_str(b"hello");
    writer.write_bin(&[1, 2, 3]);

    assert_eq!(writer.error(), Error::Ok);
    assert_eq!(
        writer.written(),
        &[0xdc, 0x00, 0x10, 0x82, 0xa5, b'h', b'e', b'l', b'l', b'o', 0xc4, 3, 1, 2, 3]
    );
}

#[test]
fn header_is_not_partially_written_but_payload_can_be() {
    let mut header_buffer = [0_u8; 2];
    let mut header_writer = Writer::new(&mut header_buffer);

    header_writer.write_array_header(16);

    assert_eq!(header_writer.error(), Error::TooBig);
    assert!(header_writer.written().is_empty());

    let mut payload_buffer = [0_u8; 2];
    let mut payload_writer = Writer::new(&mut payload_buffer);

    payload_writer.write_str(b"ab");

    assert_eq!(payload_writer.error(), Error::TooBig);
    assert_eq!(payload_writer.written(), &[0xa2, b'a']);
}
