//! Rust digests mirroring `fuzz/c/oracle_*.c`.

use mpack::common::{Error, Tag, Type};
use mpack::node::{Node, Tree};
use mpack::reader::Reader;

pub const MAX_RECORDS: usize = 4096;
pub const RECORD_SIZE: usize = 16;
pub const DEPTH_LIMIT: usize = 1024;
pub const MAX_INPUT_LEN: usize = 65536;

const TYPE_NIL: u8 = 1;
const TYPE_BOOL: u8 = 2;
const TYPE_INT: u8 = 3;
const TYPE_UINT: u8 = 4;
const TYPE_FLOAT: u8 = 5;
const TYPE_DOUBLE: u8 = 6;
const TYPE_STR: u8 = 7;
const TYPE_BIN: u8 = 8;
const TYPE_ARRAY: u8 = 9;
const TYPE_MAP: u8 = 10;
const TYPE_EXT: u8 = 11;

/// Fixed-layout digest shared with the C oracle (`oracle_digest_t`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digest {
    pub error: i32,
    pub bytes_used: u32,
    pub record_count: u32,
    pub truncated: u32,
    pub records: Vec<u8>,
}

impl Digest {
    fn new() -> Self {
        Self {
            error: 0,
            bytes_used: 0,
            record_count: 0,
            truncated: 0,
            records: vec![0; MAX_RECORDS * RECORD_SIZE],
        }
    }

    fn push(&mut self, type_: u8, aux: u8, value: u64, payload_hash: u32) -> bool {
        if self.record_count as usize >= MAX_RECORDS {
            self.truncated = 1;
            return false;
        }
        let off = self.record_count as usize * RECORD_SIZE;
        let rec = &mut self.records[off..off + RECORD_SIZE];
        rec[0] = type_;
        rec[1] = aux;
        rec[2] = 0;
        rec[3] = 0;
        rec[4..12].copy_from_slice(&value.to_le_bytes());
        rec[12..16].copy_from_slice(&payload_hash.to_le_bytes());
        self.record_count += 1;
        true
    }
}

/// Map safe-core [`Error`] to C `mpack_error_t` (gap after `ok`: `io = 2`).
pub fn error_to_c(error: Error) -> i32 {
    match error {
        Error::Ok => 0,
        Error::Io => 2,
        Error::Invalid => 3,
        Error::Unsupported => 4,
        Error::Type => 5,
        Error::TooBig => 6,
        Error::Memory => 7,
        Error::Bug => 8,
        Error::Data => 9,
        Error::Eof => 10,
    }
}

fn type_to_c(kind: Type) -> u8 {
    match kind {
        Type::Nil => TYPE_NIL,
        Type::Bool => TYPE_BOOL,
        Type::Int => TYPE_INT,
        Type::Uint => TYPE_UINT,
        Type::Float => TYPE_FLOAT,
        Type::Double => TYPE_DOUBLE,
        Type::Str => TYPE_STR,
        Type::Bin => TYPE_BIN,
        Type::Array => TYPE_ARRAY,
        Type::Map => TYPE_MAP,
        Type::Ext => TYPE_EXT,
    }
}

fn fnv1a32(data: &[u8]) -> u32 {
    let mut hash = 2_166_136_261u32;
    for &byte in data {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

fn encode_tag(tag: Tag) -> (u8, u8, u64, Option<u32>) {
    match tag {
        Tag::Nil => (TYPE_NIL, 0, 0, None),
        Tag::Bool(value) => (TYPE_BOOL, u8::from(value), 0, None),
        Tag::Int(value) => (TYPE_INT, 0, value as u64, None),
        Tag::Uint(value) => (TYPE_UINT, 0, value, None),
        Tag::Float(value) => (TYPE_FLOAT, 0, u64::from(value.to_bits()), None),
        Tag::Double(value) => (TYPE_DOUBLE, 0, value.to_bits(), None),
        Tag::Str(length) => (TYPE_STR, 0, u64::from(length), Some(length)),
        Tag::Bin(length) => (TYPE_BIN, 0, u64::from(length), Some(length)),
        Tag::Array(count) => (TYPE_ARRAY, 0, u64::from(count), None),
        Tag::Map(count) => (TYPE_MAP, 0, u64::from(count), None),
        Tag::Ext {
            extension_type,
            length,
        } => (TYPE_EXT, extension_type as u8, u64::from(length), Some(length)),
    }
}

struct Frame {
    remaining: u32,
}

/// Reader digest for one top-level value (iterative, depth-capped, raw payloads).
pub fn reader_digest_rust(data: &[u8]) -> Digest {
    let data = if data.len() > MAX_INPUT_LEN {
        &data[..MAX_INPUT_LEN]
    } else {
        data
    };
    let mut out = Digest::new();
    let mut reader = Reader::new(data);
    let mut stack: Vec<Frame> = Vec::new();
    let mut need = 1i32;

    while need > 0 || !stack.is_empty() {
        if reader.error() != Error::Ok || out.truncated != 0 {
            break;
        }
        if stack.len() >= DEPTH_LIMIT {
            reader.flag_error(Error::TooBig);
            break;
        }

        let Some(tag) = reader.read_tag() else {
            break;
        };
        let (type_, aux, value, payload_len) = encode_tag(tag);
        let mut payload_hash = 0u32;
        if let Some(length) = payload_len {
            match reader.read_bytes(length as usize) {
                Some(bytes) => payload_hash = fnv1a32(bytes),
                None => break,
            }
        }
        if !out.push(type_, aux, value, payload_hash) {
            break;
        }

        if need > 0 {
            need -= 1;
        } else if let Some(frame) = stack.last_mut() {
            frame.remaining -= 1;
        }

        match tag {
            Tag::Array(count) => stack.push(Frame { remaining: count }),
            Tag::Map(count) => {
                let Some(remaining) = count.checked_mul(2) else {
                    reader.flag_error(Error::TooBig);
                    break;
                };
                stack.push(Frame { remaining });
            }
            _ => {}
        }

        while stack
            .last()
            .map(|frame| frame.remaining == 0)
            .unwrap_or(false)
        {
            let _ = stack.pop();
        }
    }

    out.records.truncate(out.record_count as usize * RECORD_SIZE);
    out.bytes_used = reader.used() as u32;
    out.error = error_to_c(reader.error());
    if out.error != 0 {
        out.bytes_used = 0;
    }
    out
}

fn walk_node<'tree, 'data>(
    tree: &'tree Tree<'data>,
    node: Node<'tree, 'data>,
    out: &mut Digest,
    depth: usize,
) -> bool {
    if out.truncated != 0 {
        return false;
    }
    if depth >= DEPTH_LIMIT {
        tree.flag_error(Error::TooBig);
        return false;
    }
    if tree.error() != Error::Ok {
        return false;
    }

    let tag = node.tag();
    let type_ = type_to_c(tag.kind());
    let (aux, value, payload_hash) = match tag {
        Tag::Nil => (0, 0, 0),
        Tag::Bool(v) => (u8::from(v), 0, 0),
        Tag::Int(v) => (0, v as u64, 0),
        Tag::Uint(v) => (0, v, 0),
        Tag::Float(v) => (0, u64::from(v.to_bits()), 0),
        Tag::Double(v) => (0, v.to_bits(), 0),
        Tag::Str(length) => {
            let hash = node.str_bytes().map(fnv1a32).unwrap_or(0);
            (0, u64::from(length), hash)
        }
        Tag::Bin(length) => {
            let hash = node.bin_bytes().map(fnv1a32).unwrap_or(0);
            (0, u64::from(length), hash)
        }
        Tag::Ext {
            extension_type,
            length,
        } => {
            let hash = node.ext().map(|(_, bytes)| fnv1a32(bytes)).unwrap_or(0);
            (extension_type as u8, u64::from(length), hash)
        }
        Tag::Array(count) => (0, u64::from(count), 0),
        Tag::Map(count) => (0, u64::from(count), 0),
    };

    if tree.error() != Error::Ok {
        return false;
    }
    if !out.push(type_, aux, value, payload_hash) {
        return false;
    }

    match tag {
        Tag::Array(count) => {
            for index in 0..count as usize {
                let Some(child) = node.array_at(index) else {
                    return false;
                };
                if !walk_node(tree, child, out, depth + 1) {
                    return false;
                }
            }
        }
        Tag::Map(count) => {
            for index in 0..count as usize {
                let Some(key) = node.map_key_at(index) else {
                    return false;
                };
                if !walk_node(tree, key, out, depth + 1) {
                    return false;
                }
                let Some(val) = node.map_value_at(index) else {
                    return false;
                };
                if !walk_node(tree, val, out, depth + 1) {
                    return false;
                }
            }
        }
        _ => {}
    }
    tree.error() == Error::Ok
}

/// Node/tree digest for one MessagePack message.
pub fn node_digest_rust(data: &[u8]) -> Digest {
    let data = if data.len() > MAX_INPUT_LEN {
        &data[..MAX_INPUT_LEN]
    } else {
        data
    };
    let mut out = Digest::new();
    let tree = Tree::parse(data);
    if tree.error() == Error::Ok {
        if let Some(root) = tree.root() {
            let _ = walk_node(&tree, root, &mut out, 0);
        }
    }
    out.records.truncate(out.record_count as usize * RECORD_SIZE);
    out.bytes_used = tree.size() as u32;
    out.error = error_to_c(tree.error());
    if out.error != 0 {
        out.bytes_used = 0;
    }
    out
}
