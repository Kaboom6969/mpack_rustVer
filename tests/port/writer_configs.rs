use std::ffi::{c_char, c_void, CString};
use std::mem::MaybeUninit;

use mpack::common::Error;
use mpack::ffi::types::MpackWriter;
use mpack::ffi::writer::{
    mpack_build_array, mpack_build_map, mpack_complete_array, mpack_complete_map,
    mpack_start_array, mpack_write_ext, mpack_write_timestamp, mpack_write_u64,
    mpack_writer_destroy, mpack_writer_init, mpack_writer_init_filename,
    mpack_writer_init_growable,
};
use mpack::writer::{Builder, GrowableWriter, TrackKind, WriteTracker, Writer};

unsafe extern "C" {
    fn free(pointer: *mut c_void);
}

#[test]
fn fixed_writer_encodes_extensions_and_all_timestamp_forms() {
    let mut bytes = [0_u8; 64];
    let mut writer = Writer::new(&mut bytes);
    writer.write_ext(7, &[1, 2]);
    writer.write_timestamp(256, 0);
    writer.write_timestamp(0, 999_999_999);
    writer.write_timestamp(-1, 1);

    assert_eq!(writer.error(), Error::Ok);
    assert_eq!(
        writer.written(),
        &[
            0xd5, 7, 1, 2, 0xd6, 0xff, 0, 0, 1, 0, 0xd7, 0xff, 0xee, 0x6b, 0x27, 0xfc, 0, 0,
            0, 0, 0xc7, 12, 0xff, 0, 0, 0, 1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        ]
    );
}

#[test]
fn timestamp_rejects_out_of_range_nanoseconds_stickily() {
    let mut bytes = [0_u8; 16];
    let mut writer = Writer::new(&mut bytes);
    writer.write_timestamp(0, 1_000_000_000);
    writer.write_nil();
    assert_eq!(writer.error(), Error::Bug);
    assert!(writer.written().is_empty());
}

#[test]
fn growable_core_handles_payloads_larger_than_fixed_buffers() {
    let payload = vec![0x5a; 16 * 1024];
    let mut writer = GrowableWriter::with_capacity(1);
    writer.write_array_header(2);
    writer.write_str(b"payload");
    writer.write_bin(&payload);
    assert_eq!(writer.error(), Error::Ok);
    assert_eq!(writer.as_slice()[0], 0x92);
    assert_eq!(&writer.as_slice()[1..9], b"\xa7payload");
    assert_eq!(writer.as_slice().len(), 1 + 8 + 3 + payload.len());
}

#[test]
fn builder_resolves_nested_unknown_compound_sizes() {
    let mut builder = Builder::new();
    builder.build_map();
    builder.write_str(b"nums");
    builder.build_array();
    builder.write_i64(1);
    builder.write_i64(2);
    builder.write_i64(3);
    builder.complete_array().unwrap();
    builder.write_str(b"nil");
    builder.write_nil();
    builder.complete_map().unwrap();
    assert_eq!(
        builder.finish().unwrap(),
        b"\x82\xa4nums\x93\x01\x02\x03\xa3nil\xc0"
    );
}

#[test]
fn tracker_checks_elements_bytes_and_map_pairs() {
    let mut tracker = WriteTracker::default();
    tracker.push(TrackKind::Array, 2);
    tracker.element().unwrap();
    tracker.element().unwrap();
    tracker.pop(TrackKind::Array).unwrap();
    tracker.push(TrackKind::Map, 1);
    tracker.element().unwrap();
    assert_eq!(tracker.pop(TrackKind::Map), Err(Error::Bug));
    tracker.element().unwrap();
    tracker.pop(TrackKind::Map).unwrap();
    tracker.push(TrackKind::Ext, 3);
    tracker.bytes(2).unwrap();
    assert_eq!(tracker.pop(TrackKind::Ext), Err(Error::Bug));
    tracker.bytes(1).unwrap();
    tracker.pop(TrackKind::Ext).unwrap();
    assert_eq!(tracker.finish(), Ok(()));
}

#[test]
fn c_abi_growable_writer_returns_c_freeable_memory() {
    let mut writer = MaybeUninit::<MpackWriter>::uninit();
    let mut data: *mut c_char = std::ptr::null_mut();
    let mut size = 0_usize;
    unsafe {
        mpack_writer_init_growable(writer.as_mut_ptr(), &mut data, &mut size);
        let writer = writer.as_mut_ptr();
        mpack_write_u64(writer, 42);
        mpack_write_timestamp(writer, 256, 0);
        mpack_write_ext(writer, 3, b"xy".as_ptr().cast::<c_char>(), 2);
        assert_eq!(mpack_writer_destroy(writer), 0);
        assert_eq!(
            std::slice::from_raw_parts(data.cast::<u8>(), size),
            &[42, 0xd6, 0xff, 0, 0, 1, 0, 0xd5, 3, b'x', b'y']
        );
        free(data.cast::<c_void>());
    }
}

#[test]
fn c_abi_builder_resolves_nested_and_known_compounds() {
    let mut storage = [0_u8; 64];
    let mut writer = MaybeUninit::<MpackWriter>::uninit();
    unsafe {
        mpack_writer_init(
            writer.as_mut_ptr(),
            storage.as_mut_ptr().cast::<c_char>(),
            storage.len(),
        );
        let writer = writer.as_mut_ptr();
        mpack_build_map(writer);
        mpack_write_u64(writer, 1);
        mpack_build_array(writer);
        mpack_start_array(writer, 2);
        mpack_write_u64(writer, 2);
        mpack_write_u64(writer, 3);
        mpack_write_u64(writer, 4);
        mpack_complete_array(writer);
        mpack_complete_map(writer);
        let used = (*writer).position as usize - (*writer).buffer as usize;
        assert_eq!(mpack_writer_destroy(writer), 0);
        assert_eq!(&storage[..used], &[0x81, 1, 0x92, 0x92, 2, 3, 4]);
    }
}

#[test]
fn c_abi_filename_writer_flushes_and_closes() {
    let path = std::env::temp_dir().join(format!(
        "mpack-rust-writer-{}-{}.msgpack",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let c_path = CString::new(path.to_string_lossy().as_bytes()).unwrap();
    let mut writer = MaybeUninit::<MpackWriter>::uninit();
    unsafe {
        mpack_writer_init_filename(writer.as_mut_ptr(), c_path.as_ptr());
        mpack_write_u64(writer.as_mut_ptr(), 0x1234);
        assert_eq!(mpack_writer_destroy(writer.as_mut_ptr()), 0);
    }
    assert_eq!(std::fs::read(&path).unwrap(), &[0xcd, 0x12, 0x34]);
    std::fs::remove_file(path).unwrap();
}
