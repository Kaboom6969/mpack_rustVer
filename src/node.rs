//! Tree / DOM parse + typed accessors (mirrors `mpack-node`).
//!
//! Public items here are a **frozen minimal safe-core contract** (see
//! `DECISIONS.md`). Bodies are intentional stubs for teammate fill-in:
//! implement parse + accessors; do **not** change public signatures without
//! lead approval.
//!
//! Out of scope for this module: stream/file init, C pools, `*_alloc`, and
//! copying into C `char*` (FFI / lead).

use std::cell::Cell;

use crate::common::{Error, Tag, Type};
use crate::reader::Reader;

/// Parsed MessagePack tree over caller-owned input bytes.
///
/// Sticky errors use [`Cell`] so [`Node`] accessors can flag errors through a
/// shared `&Tree` (mirrors C `mpack_node_*` writing the tree error).
#[derive(Debug)]
pub struct Tree<'data> {
    #[allow(dead_code)]
    data: &'data [u8],
    #[allow(dead_code)]
    nodes: Vec<NodeData>,
    root: Option<usize>,
    error: Cell<Error>,
}

/// Internal node storage. Teammate owns the layout behind the frozen API;
/// fields may be extended without changing public signatures.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct NodeData {
    tag: Tag,
    payload_off: usize,
    children: Vec<usize>,
}

/// Immutable handle into a [`Tree`] (mirrors `mpack_node_t`).
#[derive(Debug, Clone, Copy)]
pub struct Node<'tree, 'data> {
    tree: &'tree Tree<'data>,
    #[allow(dead_code)]
    index: usize,
}

impl<'data> Tree<'data> {
    /// Parses one MessagePack value from `data` into a node tree.
    ///
    /// Stub: leaves `Error::Unsupported` and no root. Teammate replaces body.
    pub fn parse(data: &'data [u8]) -> Self {
        let mut reader = Reader::new(data);
        let mut nodes = Vec::new();
        let root = parse_node(&mut reader, &mut nodes);

        let tree = Self {
            data,
            nodes,
            root,
            error: Cell::new(reader.error()),
        };

        tree
    }

    /// Returns the tree's sticky error.
    pub fn error(&self) -> Error {
        self.error.get()
    }

    /// Records an error if the tree is currently error-free.
    pub fn flag_error(&self, error: Error) {
        if self.error.get() == Error::Ok && error != Error::Ok {
            self.error.set(error);
        }
    }

    /// Returns the root node when parsing succeeded and the tree is error-free.
    pub fn root(&self) -> Option<Node<'_, 'data>> {
        if self.error.get() != Error::Ok {
            return None;
        }
        self.root.map(|index| Node { tree: self, index })
    }
}

fn parse_node<'data>(reader: &mut Reader<'data>, nodes: &mut Vec<NodeData>) -> Option<usize> {
    let tag = reader.read_tag()?;
    let payload_off = reader.used();
    let mut children = Vec::new();

    match tag {
        Tag::Str(length) | Tag::Bin(length) | Tag::Ext { length, .. } => {
            if !reader.skip_bytes(length as usize) {
                return None;
            }
        }
        Tag::Array(count) => {
            for _ in 0..count {
                let child = parse_node(reader, nodes)?;
                children.push(child);
                if reader.error() != Error::Ok {
                    return None;
                }
            }
        }
        Tag::Map(count) => {
            for _ in 0..count {
                let key = parse_node(reader, nodes)?;
                let value = parse_node(reader, nodes)?;
                children.push(key);
                children.push(value);
                if reader.error() != Error::Ok {
                    return None;
                }
            }
        }
        _ => {}
    }

    let index = nodes.len();
    nodes.push(NodeData {
        tag,
        payload_off,
        children,
    });
    Some(index)
}

impl<'tree, 'data> Node<'tree, 'data> {
    /// Returns the node's tag (even if the tree already has an error).
    ///
    /// Stub: returns `Tag::Nil`. Teammate replaces body.
    pub fn tag(self) -> Tag {
        let _ = self.tree;
        Tag::Nil
    }

    /// Returns the node's MessagePack type category.
    pub fn type_(self) -> Type {
        self.tag().kind()
    }

    /// Returns true if this node is nil.
    ///
    /// Stub: always `false`. Teammate replaces body.
    pub fn is_nil(self) -> bool {
        false
    }

    /// Reads a bool; flags `Error::Type` on mismatch.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn as_bool(self) -> Option<bool> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Reads an unsigned value (uint, or non-negative int); flags type errors.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn as_u64(self) -> Option<u64> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Reads a signed value (int, or uint that fits in `i64`); flags type errors.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn as_i64(self) -> Option<i64> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Reads `f32` (float tag only for the minimal freeze; widen later if needed).
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn as_f32(self) -> Option<f32> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Reads `f64` from float or double tags.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn as_f64(self) -> Option<f64> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Returns str payload bytes; flags `Error::Type` when not a str.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn str_bytes(self) -> Option<&'data [u8]> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Returns bin payload bytes; flags `Error::Type` when not a bin.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn bin_bytes(self) -> Option<&'data [u8]> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Returns `(ext_type, payload)`; flags `Error::Type` when not an ext.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn ext(self) -> Option<(i8, &'data [u8])> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Array element count; flags type error when not an array.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn array_len(self) -> Option<usize> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Array element at `index`; flags type/data errors like C bounds checks.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn array_at(self, _index: usize) -> Option<Node<'tree, 'data>> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Map entry count; flags type error when not a map.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn map_count(self) -> Option<usize> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Map key at entry `index`.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn map_key_at(self, _index: usize) -> Option<Node<'tree, 'data>> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Map value at entry `index`.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn map_value_at(self, _index: usize) -> Option<Node<'tree, 'data>> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Finds a map value whose key is unsigned/int equal to `key`.
    ///
    /// Missing key should flag `Error::Data` (required lookup) once implemented.
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn map_uint(self, _key: u64) -> Option<Node<'tree, 'data>> {
        self.tree.flag_error(Error::Unsupported);
        None
    }

    /// Finds a map value whose key is a str with exact byte contents `key`.
    ///
    /// Stub: flags `Error::Unsupported` and returns `None`.
    pub fn map_str(self, _key: &[u8]) -> Option<Node<'tree, 'data>> {
        self.tree.flag_error(Error::Unsupported);
        None
    }
}
