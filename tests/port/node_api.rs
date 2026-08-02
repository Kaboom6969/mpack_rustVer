//! Minimal Node safe-core acceptance tests (see `DECISIONS.md` Node table).

use mpack::common::{Error, Tag, Type};
use mpack::node::Tree;

#[test]
fn parse_nil_is_ok_and_has_root() {
    let tree = Tree::parse(&[0xc0]);
    assert_eq!(tree.error(), Error::Ok);
    let root = tree.root().expect("root");
    assert_eq!(root.type_(), Type::Nil);
}

#[test]
fn parse_allows_trailing_bytes() {
    let tree = Tree::parse(&[0xc0, 0xff]);
    assert_eq!(tree.error(), Error::Ok);
    let root = tree.root().expect("root");
    assert_eq!(root.type_(), Type::Nil);
}

#[test]
fn parse_scalars_array_map_and_lookups() {
    let data = [
        0x82, // map 2
        0xa1, b'a', // "a"
        0x92, 0xc3, 0x2a, // [true, 42]
        0xa1, b'b', // "b"
        0xc0, // nil
    ];
    let tree = Tree::parse(&data);
    assert_eq!(tree.error(), Error::Ok);
    let root = tree.root().expect("root");
    assert_eq!(root.type_(), Type::Map);
    assert_eq!(root.map_count(), Some(2));

    let a = root.map_str(b"a").expect("key a");
    assert_eq!(a.array_len(), Some(2));
    assert_eq!(a.array_at(0).and_then(|n| n.as_bool()), Some(true));
    assert_eq!(a.array_at(1).and_then(|n| n.as_u64()), Some(42));

    let b = root.map_str(b"b").expect("key b");
    assert!(b.is_nil());
    assert_eq!(b.tag(), Tag::Nil);
}

#[test]
fn map_uint_and_bin_ext_surface() {
    let data = [0x81, 0x07, 0xc4, 2, b'h', b'i'];
    let tree = Tree::parse(&data);
    let root = tree.root().expect("root");
    let value = root.map_uint(7).expect("key 7");
    assert_eq!(value.bin_bytes(), Some(&b"hi"[..]));

    let ext_data = [0xd4, 0xff, 0xaa];
    let ext_tree = Tree::parse(&ext_data);
    let ext_root = ext_tree.root().expect("ext root");
    assert_eq!(ext_root.ext(), Some((-1, &[0xaa][..])));
}

#[test]
fn type_mismatch_is_sticky_on_tree() {
    let tree = Tree::parse(&[0xc0]); // nil
    let root = tree.root().expect("root");
    assert_eq!(root.as_u64(), None);
    assert_eq!(tree.error(), Error::Type);
    assert!(tree.root().is_none());
}
