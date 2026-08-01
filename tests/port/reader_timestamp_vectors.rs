use mpack::{
    common::{Error, Timestamp},
    reader::Reader,
};

#[test]
fn reads_timestamp32() {
    let mut reader = Reader::new(&[0, 0, 0, 42]);
    assert_eq!(
        reader.read_timestamp(4),
        Some(Timestamp {
            seconds: 42,
            nanoseconds: 0
        })
    );
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 4);
}

#[test]
fn reads_timestamp64() {
    let nanoseconds = 42u64;
    let seconds = 1u64;
    let packed = (nanoseconds << 34) | seconds;
    let mut reader = Reader::new(&packed.to_be_bytes());
    assert_eq!(
        reader.read_timestamp(8),
        Some(Timestamp {
            seconds: 1,
            nanoseconds: 42
        })
    );
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 8);
}

#[test]
fn reads_timestamp96() {
    let mut data = [0u8; 12];
    data[..4].copy_from_slice(&999_999_999u32.to_be_bytes());
    data[4..].copy_from_slice(&(-1i64).to_be_bytes());
    let mut reader = Reader::new(&data);
    assert_eq!(
        reader.read_timestamp(12),
        Some(Timestamp {
            seconds: -1,
            nanoseconds: 999_999_999
        })
    );
    assert_eq!(reader.error(), Error::Ok);
    assert_eq!(reader.used(), 12);
}

#[test]
fn invalid_timestamp_size_is_atomic_and_sticky() {
    let mut reader = Reader::new(&[0, 0, 0, 42]);
    assert_eq!(reader.read_timestamp(5), None);
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), 0);
    assert_eq!(reader.read_timestamp(4), None);
    assert_eq!(reader.used(), 0);
}

#[test]
fn truncated_timestamp_payload_is_atomic_and_sticky() {
    let mut reader = Reader::new(&[0, 0, 0]);
    assert_eq!(reader.read_timestamp(4), None);
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), 0);
    assert_eq!(reader.read_timestamp(4), None);
    assert_eq!(reader.used(), 0);
}

#[test]
fn out_of_range_nanoseconds_is_atomic_and_sticky() {
    let nanoseconds = 1_000_000_000u64;
    let seconds = 0u64;
    let packed = (nanoseconds << 34) | seconds;
    let mut reader = Reader::new(&packed.to_be_bytes());
    assert_eq!(reader.read_timestamp(8), None);
    assert_eq!(reader.error(), Error::Invalid);
    assert_eq!(reader.used(), 0);
    assert_eq!(reader.read_timestamp(8), None);
    assert_eq!(reader.used(), 0);
}
