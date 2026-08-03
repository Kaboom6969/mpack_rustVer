//! Node safe-core acceptance tests (see `DECISIONS.md` Node table).

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
fn map_optional_miss_does_not_sticky() {
    let data = [0x81, 0xa1, b'a', 0xc0];
    let tree = Tree::parse(&data);
    let root = tree.root().expect("root");
    assert!(root.map_str_optional(b"missing").is_none());
    assert_eq!(tree.error(), Error::Ok);
    assert!(root.map_uint_optional(99).is_none());
    assert_eq!(tree.error(), Error::Ok);
    assert!(root.map_int_optional(-1).is_none());
    assert_eq!(tree.error(), Error::Ok);
    assert!(root.map_str(b"a").expect("a").is_nil());
}

#[test]
fn map_required_miss_flags_data() {
    let data = [0x81, 0xa1, b'a', 0xc0];
    let tree = Tree::parse(&data);
    let root = tree.root().expect("root");
    assert!(root.map_str(b"nope").is_none());
    assert_eq!(tree.error(), Error::Data);

    let tree2 = Tree::parse(&data);
    let root2 = tree2.root().expect("root");
    assert!(root2.map_uint(99).is_none());
    assert_eq!(tree2.error(), Error::Data);
}

#[test]
fn map_int_uint_cross_key() {
    // map { -1 => true } — uint lookup must not match negative int key
    let data = [0x81, 0xff, 0xc3];
    let tree = Tree::parse(&data);
    let root = tree.root().expect("root");
    assert!(root.map_int(-1).expect("key -1").as_bool().unwrap());
    assert_eq!(tree.error(), Error::Ok);

    let tree2 = Tree::parse(&data);
    let root2 = tree2.root().expect("root");
    assert!(root2.map_uint(u64::MAX).is_none());
    assert_eq!(tree2.error(), Error::Data);

    // map { 7 => nil } — int lookup accepts non-negative uint key
    let data3 = [0x81, 0x07, 0xc0];
    let tree3 = Tree::parse(&data3);
    let root3 = tree3.root().expect("root");
    assert!(root3.map_int(7).expect("key 7").is_nil());
}

#[test]
fn map_contains_and_int_lookup() {
    // map { 7 => true, -3 => nil }
    let data = [0x82, 0x07, 0xc3, 0xfd, 0xc0];
    let tree = Tree::parse(&data);
    let root = tree.root().expect("root");
    assert!(root.map_contains_uint(7));
    assert!(root.map_contains_int(-3));
    assert!(!root.map_contains_uint(8));
    assert_eq!(tree.error(), Error::Ok);
    assert_eq!(root.map_int(-3).expect("key -3").tag(), Tag::Nil);
    assert!(root.map_int(7).expect("key 7").as_bool().unwrap());
}

#[test]
fn map_duplicate_key_flags_data() {
    // map { "a" => 1, "a" => 2 }
    let data = [0x82, 0xa1, b'a', 0x01, 0xa1, b'a', 0x02];
    let tree = Tree::parse(&data);
    let root = tree.root().expect("root");
    assert!(root.map_str(b"a").is_none());
    assert_eq!(tree.error(), Error::Data);

    let tree2 = Tree::parse(&data);
    let root2 = tree2.root().expect("root");
    assert!(!root2.map_contains_str(b"a"));
    assert_eq!(tree2.error(), Error::Data);

    let tree3 = Tree::parse(&data);
    let root3 = tree3.root().expect("root");
    assert!(root3.map_str_optional(b"a").is_none());
    assert_eq!(tree3.error(), Error::Data);
}

#[test]
fn enum_str_required_and_optional() {
    let tree = Tree::parse(&[0xa3, b'r', b'e', b'd']);
    let root = tree.root().expect("root");
    let strings: &[&[u8]] = &[b"red", b"green", b"blue"];
    assert_eq!(root.enum_str(strings, true), 0);
    assert_eq!(tree.error(), Error::Ok);

    let tree2 = Tree::parse(&[0xa3, b'r', b'e', b'd']);
    let root2 = tree2.root().expect("root");
    assert_eq!(root2.enum_str(&[b"green", b"blue"], false), 2);
    assert_eq!(tree2.error(), Error::Type);

    let tree3 = Tree::parse(&[0xa3, b'r', b'e', b'd']);
    let root3 = tree3.root().expect("root");
    assert_eq!(root3.enum_str(&[b"green", b"blue"], true), 2);
    assert_eq!(tree3.error(), Error::Ok);

    let tree4 = Tree::parse(&[0xc0]);
    let root4 = tree4.root().expect("root");
    assert_eq!(root4.enum_str(strings, true), 3);
    assert_eq!(tree4.error(), Error::Ok);
    assert_eq!(root4.enum_str(strings, false), 3);
    assert_eq!(tree4.error(), Error::Type);
}

#[test]
fn type_mismatch_is_sticky_on_tree() {
    let tree = Tree::parse(&[0xc0]); // nil
    let root = tree.root().expect("root");
    assert_eq!(root.as_u64(), None);
    assert_eq!(tree.error(), Error::Type);
    assert!(tree.root().is_none());
}

#[test]
fn parse_reports_size_excluding_trailing() {
    let tree = Tree::parse(&[0xc0, 0xff]);
    assert_eq!(tree.error(), Error::Ok);
    assert_eq!(tree.size(), 1);
}

#[test]
fn parse_with_limits_too_big() {
    let tree = Tree::parse_with_limits(&[0x91, 0xc0], Some(1));
    assert_eq!(tree.error(), Error::TooBig);
}
