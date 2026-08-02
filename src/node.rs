//! Tree / DOM parse + typed accessors (mirrors `mpack-node`).
//!
//! Public items here are a **frozen minimal safe-core contract**. Do not change
//! public signatures without lead approval; document semantic divergences from
//! C in `DECISIONS.md` (Node table / hotspots).
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
    data: &'data [u8],
    nodes: Vec<NodeData>,
    root: Option<usize>,
    error: Cell<Error>,
    size: usize,
}

/// Internal node storage behind the frozen public API; fields may be extended
/// without changing public signatures.
#[derive(Debug, Clone)]
pub(crate) struct NodeData {
    pub(crate) tag: Tag,
    pub(crate) payload_off: usize,
    pub(crate) children: Vec<usize>,
}

/// Immutable handle into a [`Tree`] (mirrors `mpack_node_t`).
#[derive(Debug, Clone, Copy)]
pub struct Node<'tree, 'data> {
    tree: &'tree Tree<'data>,
    index: usize,
}

struct ParseFrame {
    node_index: usize,
    remaining: u32,
}

impl<'data> Tree<'data> {
    /// Parses one MessagePack value from `data` into a node tree.
    ///
    /// Nesting is iterative (see `DECISIONS.md`); trailing bytes after the first
    /// value are allowed.
    pub fn parse(data: &'data [u8]) -> Self {
        Self::parse_with_limits(data, None)
    }

    /// Like [`parse`](Self::parse), but fails with [`Error::TooBig`] once more
    /// than `max_nodes` nodes would be allocated (C pool / `max_nodes` limit).
    pub fn parse_with_limits(data: &'data [u8], max_nodes: Option<usize>) -> Self {
        let mut reader = Reader::new(data);
        let mut nodes = Vec::new();
        let root = parse_tree(&mut reader, &mut nodes, max_nodes);
        let error = reader.error();
        let size = if error == Error::Ok {
            reader.used()
        } else {
            0
        };
        Self {
            data,
            nodes,
            root: if error == Error::Ok { root } else { None },
            error: Cell::new(error),
            size,
        }
    }

    /// Bytes consumed by the successful parse (0 when the tree has an error).
    pub fn size(&self) -> usize {
        self.size
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
        self.root.map(|index| Node {
            tree: self,
            index,
        })
    }

    pub(crate) fn from_parts(
        data: &'data [u8],
        nodes: Vec<NodeData>,
        root: Option<usize>,
        error: Error,
        size: usize,
    ) -> Self {
        Self {
            data,
            nodes,
            root,
            error: Cell::new(error),
            size,
        }
    }

    pub(crate) fn into_parts(self) -> (Vec<NodeData>, Option<usize>, Error, usize) {
        (self.nodes, self.root, self.error.get(), self.size)
    }

    pub(crate) fn node_at(&self, index: usize) -> Node<'_, 'data> {
        Node {
            tree: self,
            index,
        }
    }

    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn nodes(&self) -> &[NodeData] {
        &self.nodes
    }
}

fn can_add_node(nodes: &[NodeData], max_nodes: Option<usize>) -> bool {
    match max_nodes {
        Some(max) => nodes.len() < max,
        None => true,
    }
}

fn reserve_children(reader: &mut Reader<'_>, child_nodes: u32) -> bool {
    if child_nodes == 0 {
        return true;
    }
    if child_nodes as usize > reader.remaining() {
        reader.flag_error(Error::Invalid);
        return false;
    }
    true
}

fn parse_tree(
    reader: &mut Reader<'_>,
    nodes: &mut Vec<NodeData>,
    max_nodes: Option<usize>,
) -> Option<usize> {
    if reader.remaining() == 0 {
        reader.flag_error(Error::Invalid);
        return None;
    }

    let root = parse_push_node(reader, nodes, max_nodes)?;
    let mut stack: Vec<ParseFrame> = Vec::new();
    if let Some(remaining) = compound_remaining(nodes[root].tag) {
        if !reserve_children(reader, remaining) {
            return None;
        }
        nodes[root].children.reserve(remaining as usize);
        if remaining > 0 {
            stack.push(ParseFrame {
                node_index: root,
                remaining,
            });
        }
    }

    while let Some(frame) = stack.last_mut() {
        if frame.remaining == 0 {
            stack.pop();
            continue;
        }
        frame.remaining -= 1;
        let parent = frame.node_index;
        let child = parse_push_node(reader, nodes, max_nodes)?;
        nodes[parent].children.push(child);
        if reader.error() != Error::Ok {
            return None;
        }
        if let Some(remaining) = compound_remaining(nodes[child].tag) {
            if !reserve_children(reader, remaining) {
                return None;
            }
            // Reserve only after byte-budget checks so hostile counts cannot OOM.
            nodes[child].children.reserve(remaining as usize);
            if remaining > 0 {
                stack.push(ParseFrame {
                    node_index: child,
                    remaining,
                });
            }
        }
    }

    if reader.error() != Error::Ok {
        None
    } else {
        Some(root)
    }
}

fn compound_remaining(tag: Tag) -> Option<u32> {
    match tag {
        Tag::Array(count) => Some(count),
        Tag::Map(count) => count.checked_mul(2),
        _ => None,
    }
}

fn parse_push_node(
    reader: &mut Reader<'_>,
    nodes: &mut Vec<NodeData>,
    max_nodes: Option<usize>,
) -> Option<usize> {
    if !can_add_node(nodes, max_nodes) {
        reader.flag_error(Error::TooBig);
        return None;
    }
    let tag = reader.read_tag()?;
    let payload_off = reader.used();
    match tag {
        Tag::Str(length) | Tag::Bin(length) | Tag::Ext { length, .. } => {
            if !reader.skip_bytes(length as usize) {
                return None;
            }
        }
        Tag::Map(count) => {
            if count.checked_mul(2).is_none() {
                reader.flag_error(Error::Invalid);
                return None;
            }
        }
        _ => {}
    }
    let index = nodes.len();
    nodes.push(NodeData {
        tag,
        payload_off,
        children: Vec::new(),
    });
    Some(index)
}

impl<'tree, 'data> Node<'tree, 'data> {
    pub(crate) fn index(self) -> usize {
        self.index
    }

    /// Returns the node's tag (even if the tree already has an error).
    pub fn tag(self) -> Tag {
        self.tree
            .nodes
            .get(self.index)
            .map(|node| node.tag)
            .unwrap_or_else(|| {
                self.tree.flag_error(Error::Bug);
                Tag::Nil
            })
    }

    /// Returns the node's MessagePack type category.
    pub fn type_(self) -> Type {
        self.tag().kind()
    }

    /// Returns true if this node is nil.
    pub fn is_nil(self) -> bool {
        matches!(self.tag(), Tag::Nil)
    }

    /// Reads a bool; flags `Error::Type` on mismatch.
    pub fn as_bool(self) -> Option<bool> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        match self.tag() {
            Tag::Bool(value) => Some(value),
            _ => {
                self.tree.flag_error(Error::Type);
                None
            }
        }
    }

    /// Reads an unsigned value (uint, or non-negative int); flags type errors.
    pub fn as_u64(self) -> Option<u64> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        match self.tag() {
            Tag::Uint(value) => Some(value),
            Tag::Int(value) if value >= 0 => Some(value as u64),
            _ => {
                self.tree.flag_error(Error::Type);
                None
            }
        }
    }

    /// Reads a signed value (int, or uint that fits in `i64`); flags type errors.
    pub fn as_i64(self) -> Option<i64> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        match self.tag() {
            Tag::Int(value) => Some(value),
            Tag::Uint(value) => match i64::try_from(value) {
                Ok(value) => Some(value),
                Err(_) => {
                    self.tree.flag_error(Error::Type);
                    None
                }
            },
            _ => {
                self.tree.flag_error(Error::Type);
                None
            }
        }
    }

    /// Reads `f32` (`Tag::Float` only; see `DECISIONS.md` Node table).
    pub fn as_f32(self) -> Option<f32> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        match self.tag() {
            Tag::Float(value) => Some(value),
            _ => {
                self.tree.flag_error(Error::Type);
                None
            }
        }
    }

    /// Reads `f64` from float or double tags.
    pub fn as_f64(self) -> Option<f64> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        match self.tag() {
            Tag::Float(value) => Some(value as f64),
            Tag::Double(value) => Some(value),
            _ => {
                self.tree.flag_error(Error::Type);
                None
            }
        }
    }

    /// Returns str payload bytes; flags `Error::Type` when not a str.
    pub fn str_bytes(self) -> Option<&'data [u8]> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        let Tag::Str(length) = self.tag() else {
            self.tree.flag_error(Error::Type);
            return None;
        };
        let node = self.tree.nodes.get(self.index)?;
        let start = node.payload_off;
        let end = start.saturating_add(length as usize);
        self.tree.data.get(start..end).or_else(|| {
            self.tree.flag_error(Error::Bug);
            None
        })
    }

    /// Returns bin payload bytes; flags `Error::Type` when not a bin.
    pub fn bin_bytes(self) -> Option<&'data [u8]> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        let Tag::Bin(length) = self.tag() else {
            self.tree.flag_error(Error::Type);
            return None;
        };
        let node = self.tree.nodes.get(self.index)?;
        let start = node.payload_off;
        let end = start.saturating_add(length as usize);
        self.tree.data.get(start..end).or_else(|| {
            self.tree.flag_error(Error::Bug);
            None
        })
    }

    /// Returns `(ext_type, payload)`; flags `Error::Type` when not an ext.
    pub fn ext(self) -> Option<(i8, &'data [u8])> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        let Tag::Ext {
            extension_type,
            length,
        } = self.tag()
        else {
            self.tree.flag_error(Error::Type);
            return None;
        };
        let node = self.tree.nodes.get(self.index)?;
        let start = node.payload_off;
        let end = start.saturating_add(length as usize);
        let bytes = self.tree.data.get(start..end).or_else(|| {
            self.tree.flag_error(Error::Bug);
            None
        })?;
        Some((extension_type, bytes))
    }

    /// Array element count; flags type error when not an array.
    pub fn array_len(self) -> Option<usize> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        let Tag::Array(_) = self.tag() else {
            self.tree.flag_error(Error::Type);
            return None;
        };
        self.tree
            .nodes
            .get(self.index)
            .map(|node| node.children.len())
    }

    /// Array element at `index`; flags type/data errors like C bounds checks.
    pub fn array_at(self, index: usize) -> Option<Node<'tree, 'data>> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        let Tag::Array(_) = self.tag() else {
            self.tree.flag_error(Error::Type);
            return None;
        };
        let node = self.tree.nodes.get(self.index)?;
        match node.children.get(index).copied() {
            Some(index) => Some(Node {
                tree: self.tree,
                index,
            }),
            None => {
                self.tree.flag_error(Error::Data);
                None
            }
        }
    }

    /// Map entry count; flags type error when not a map.
    pub fn map_count(self) -> Option<usize> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        let Tag::Map(_) = self.tag() else {
            self.tree.flag_error(Error::Type);
            return None;
        };
        self.tree
            .nodes
            .get(self.index)
            .map(|node| node.children.len() / 2)
    }

    /// Map key at entry `index`.
    pub fn map_key_at(self, index: usize) -> Option<Node<'tree, 'data>> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        let Tag::Map(_) = self.tag() else {
            self.tree.flag_error(Error::Type);
            return None;
        };
        let node = self.tree.nodes.get(self.index)?;
        let at = index.saturating_mul(2);
        match node.children.get(at).copied() {
            Some(index) => Some(Node {
                tree: self.tree,
                index,
            }),
            None => {
                self.tree.flag_error(Error::Data);
                None
            }
        }
    }

    /// Map value at entry `index`.
    pub fn map_value_at(self, index: usize) -> Option<Node<'tree, 'data>> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        let Tag::Map(_) = self.tag() else {
            self.tree.flag_error(Error::Type);
            return None;
        };
        let node = self.tree.nodes.get(self.index)?;
        let at = index.saturating_mul(2).saturating_add(1);
        match node.children.get(at).copied() {
            Some(index) => Some(Node {
                tree: self.tree,
                index,
            }),
            None => {
                self.tree.flag_error(Error::Data);
                None
            }
        }
    }

    /// Finds a map value whose key is unsigned/int equal to `key`.
    ///
    /// Missing key flags `Error::Data` (required lookup; see `DECISIONS.md`).
    pub fn map_uint(self, key: u64) -> Option<Node<'tree, 'data>> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        let Tag::Map(_) = self.tag() else {
            self.tree.flag_error(Error::Type);
            return None;
        };
        let node = self.tree.nodes.get(self.index)?;
        for i in 0..(node.children.len() / 2) {
            let key_index = node.children[i * 2];
            let value_index = node.children[i * 2 + 1];
            let key_tag = self
                .tree
                .nodes
                .get(key_index)
                .map(|n| n.tag)
                .unwrap_or(Tag::Nil);
            let matches = match key_tag {
                Tag::Uint(value) => value == key,
                Tag::Int(value) if value >= 0 => value as u64 == key,
                _ => false,
            };
            if matches {
                return Some(Node {
                    tree: self.tree,
                    index: value_index,
                });
            }
        }
        self.tree.flag_error(Error::Data);
        None
    }

    /// Finds a map value whose key is a str with exact byte contents `key`.
    ///
    /// Missing key flags `Error::Data` (required lookup; see `DECISIONS.md`).
    pub fn map_str(self, key: &[u8]) -> Option<Node<'tree, 'data>> {
        if self.tree.error.get() != Error::Ok {
            return None;
        }
        let Tag::Map(_) = self.tag() else {
            self.tree.flag_error(Error::Type);
            return None;
        };
        let node = self.tree.nodes.get(self.index)?;
        for i in 0..(node.children.len() / 2) {
            let key_index = node.children[i * 2];
            let value_index = node.children[i * 2 + 1];
            let Some(key_node) = self.tree.nodes.get(key_index) else {
                continue;
            };
            let Tag::Str(length) = key_node.tag else {
                continue;
            };
            let start = key_node.payload_off;
            let end = start.saturating_add(length as usize);
            let Some(bytes) = self.tree.data.get(start..end) else {
                continue;
            };
            if bytes == key {
                return Some(Node {
                    tree: self.tree,
                    index: value_index,
                });
            }
        }
        self.tree.flag_error(Error::Data);
        None
    }
}

/// Byte-size gates for loading a whole file into a tree (C `mpack_file_tree_read`).
///
/// - `file_size < 0` → [`Error::Io`]
/// - `file_size == 0` → [`Error::Invalid`] (empty file)
/// - `max_bytes != 0 && size > max_bytes` → [`Error::TooBig`] (never truncate)
pub fn check_file_tree_bytes(file_size: i64, max_bytes: usize) -> Result<usize, Error> {
    if file_size < 0 {
        return Err(Error::Io);
    }
    if file_size == 0 {
        return Err(Error::Invalid);
    }
    let size = file_size as usize;
    if max_bytes != 0 && size > max_bytes {
        return Err(Error::TooBig);
    }
    Ok(size)
}
