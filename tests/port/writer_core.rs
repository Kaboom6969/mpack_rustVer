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
