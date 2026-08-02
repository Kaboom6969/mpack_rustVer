//! Layout lock for the full-suite ABI against measured C offsets.

#![cfg(feature = "full-suite-abi")]

use std::mem::{offset_of, size_of};

use mpack::ffi::types::{
    MpackBuilder, MpackNode, MpackNodeData, MpackReader, MpackTag, MpackTrack,
    MpackTrackElement, MpackTree, MpackTreeParser, MpackWriter,
};

#[test]
fn full_suite_tag_layout() {
    assert_eq!(size_of::<MpackTag>(), 16);
    assert_eq!(offset_of!(MpackTag, type_), 0);
    assert_eq!(offset_of!(MpackTag, exttype), 4);
    assert_eq!(offset_of!(MpackTag, value), 8);
}

#[test]
fn full_suite_writer_layout() {
    assert_eq!(size_of::<MpackWriter>(), 168);
    assert_eq!(offset_of!(MpackWriter, version), 0);
    assert_eq!(offset_of!(MpackWriter, flush), 8);
    assert_eq!(offset_of!(MpackWriter, error_fn), 16);
    assert_eq!(offset_of!(MpackWriter, teardown), 24);
    assert_eq!(offset_of!(MpackWriter, context), 32);
    assert_eq!(offset_of!(MpackWriter, buffer), 40);
    assert_eq!(offset_of!(MpackWriter, position), 48);
    assert_eq!(offset_of!(MpackWriter, end), 56);
    assert_eq!(offset_of!(MpackWriter, error), 64);
    assert_eq!(offset_of!(MpackWriter, track), 72);
    assert_eq!(offset_of!(MpackWriter, reserved), 96);
    assert_eq!(offset_of!(MpackWriter, builder), 112);
    assert_eq!(size_of::<MpackTrack>(), 24);
    assert_eq!(size_of::<MpackTrackElement>(), 12);
    assert_eq!(size_of::<MpackBuilder>(), 56);
}

#[test]
fn full_suite_reader_layout() {
    assert_eq!(size_of::<MpackReader>(), 104);
    assert_eq!(offset_of!(MpackReader, context), 0);
    assert_eq!(offset_of!(MpackReader, fill), 8);
    assert_eq!(offset_of!(MpackReader, error_fn), 16);
    assert_eq!(offset_of!(MpackReader, teardown), 24);
    assert_eq!(offset_of!(MpackReader, skip), 32);
    assert_eq!(offset_of!(MpackReader, buffer), 40);
    assert_eq!(offset_of!(MpackReader, size), 48);
    assert_eq!(offset_of!(MpackReader, data), 56);
    assert_eq!(offset_of!(MpackReader, end), 64);
    assert_eq!(offset_of!(MpackReader, error), 72);
    assert_eq!(offset_of!(MpackReader, track), 80);
}

#[test]
fn full_suite_tree_layout() {
    assert_eq!(size_of::<MpackNode>(), 16);
    assert_eq!(size_of::<MpackNodeData>(), 16);
    assert_eq!(size_of::<MpackTreeParser>(), 120);
    assert_eq!(size_of::<MpackTree>(), 288);
    assert_eq!(offset_of!(MpackTree, error_fn), 0);
    assert_eq!(offset_of!(MpackTree, nil_node), 32);
    assert_eq!(offset_of!(MpackTree, missing_node), 48);
    assert_eq!(offset_of!(MpackTree, error), 64);
    assert_eq!(offset_of!(MpackTree, buffer), 72);
    assert_eq!(offset_of!(MpackTree, parser), 136);
    assert_eq!(offset_of!(MpackTree, root), 256);
    assert_eq!(offset_of!(MpackTree, next), 280);
}
