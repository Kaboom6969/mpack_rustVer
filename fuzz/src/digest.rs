//! Rust digests mirroring `fuzz/c/oracle_*.c`.

use mpack::common::{Error, Tag, Timestamp, Type};
use mpack::expect::{self, ExpectCompound};
use mpack::node::{Node, Tree};
use mpack::reader::Reader;
use mpack::writer::GrowableWriter;

pub const MAX_RECORDS: usize = 4096;
pub const RECORD_SIZE: usize = 16;
pub const DEPTH_LIMIT: usize = 1024;
pub const MAX_INPUT_LEN: usize = 65536;
pub const MAX_OUTPUT_LEN: usize = 1 << 20;

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

/// Writer transfer result shared with the C oracle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriterTransfer {
    pub reader_error: i32,
    pub writer_error: i32,
    pub truncated: u32,
    pub out: Vec<u8>,
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

fn write_tag(writer: &mut GrowableWriter, tag: Tag) {
    match tag {
        Tag::Nil => writer.write_nil(),
        Tag::Bool(value) => writer.write_bool(value),
        Tag::Int(value) => writer.write_i64(value),
        Tag::Uint(value) => writer.write_u64(value),
        Tag::Float(value) => writer.write_f32(value),
        Tag::Double(value) => writer.write_f64(value),
        Tag::Str(length) => writer.write_str_header(length as usize),
        Tag::Bin(length) => writer.write_bin_header(length as usize),
        Tag::Array(count) => writer.write_array_header(count as usize),
        Tag::Map(count) => writer.write_map_header(count as usize),
        Tag::Ext {
            extension_type,
            length,
        } => writer.write_ext_header(extension_type, length as usize),
    }
}

fn transfer_bytes(reader: &mut Reader<'_>, writer: &mut GrowableWriter, count: u32) {
    let mut left = count as usize;
    while left > 0 {
        if reader.error() != Error::Ok || writer.error() != Error::Ok {
            return;
        }
        let step = left.min(256);
        let Some(bytes) = reader.read_bytes(step) else {
            return;
        };
        writer.write_bytes(bytes);
        left -= step;
    }
}

/// Read→rewrite transfer digest (mirrors C `oracle_writer_transfer`).
pub fn writer_transfer_rust(data: &[u8]) -> WriterTransfer {
    let data = if data.len() > MAX_INPUT_LEN {
        &data[..MAX_INPUT_LEN]
    } else {
        data
    };
    let mut reader = Reader::new(data);
    let mut writer = GrowableWriter::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut need = 1i32;

    while need > 0 || !stack.is_empty() {
        if reader.error() != Error::Ok || writer.error() != Error::Ok {
            break;
        }
        if stack.len() >= DEPTH_LIMIT {
            reader.flag_error(Error::TooBig);
            break;
        }

        let Some(tag) = reader.read_tag() else {
            break;
        };
        write_tag(&mut writer, tag);
        if writer.error() != Error::Ok {
            break;
        }

        if need > 0 {
            need -= 1;
        } else if let Some(frame) = stack.last_mut() {
            frame.remaining -= 1;
        }

        match tag {
            Tag::Str(count) | Tag::Bin(count) | Tag::Ext { length: count, .. } => {
                transfer_bytes(&mut reader, &mut writer, count);
            }
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

    let reader_error = error_to_c(reader.error());
    let writer_error = error_to_c(writer.error());
    let mut out = writer.as_slice().to_vec();
    let mut truncated = 0u32;
    if out.len() > MAX_OUTPUT_LEN {
        out.truncate(MAX_OUTPUT_LEN);
        truncated = 1;
    }
    // Match C growable destroy: on writer error the buffer is discarded.
    if writer_error != 0 {
        out.clear();
    }
    WriterTransfer {
        reader_error,
        writer_error,
        truncated,
        out,
    }
}

// ── expect opcode digest ───────────────────────────────────────────────────

const OP_COUNT: u8 = 54;

struct OpCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> OpCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take_u8(&mut self) -> u8 {
        if self.pos < self.data.len() {
            let v = self.data[self.pos];
            self.pos += 1;
            v
        } else {
            0
        }
    }

    fn take_u16(&mut self) -> u16 {
        u16::from_le_bytes([self.take_u8(), self.take_u8()])
    }

    fn take_u32(&mut self) -> u32 {
        u32::from_le_bytes([
            self.take_u8(),
            self.take_u8(),
            self.take_u8(),
            self.take_u8(),
        ])
    }

    fn take_u64(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        for b in &mut bytes {
            *b = self.take_u8();
        }
        u64::from_le_bytes(bytes)
    }

    fn remaining_ops(&self) -> bool {
        self.pos < self.data.len()
    }
}

fn ok_flag(reader: &Reader<'_>) -> u8 {
    u8::from(reader.error() == Error::Ok)
}

fn encode_or_nil(compound: ExpectCompound) -> u64 {
    let mut value = u64::from(compound.count);
    if compound.is_nil {
        value |= 1u64 << 32;
    }
    value
}

fn split_expect_input(data: &[u8]) -> (&[u8], &[u8]) {
    if data.is_empty() {
        return (&[], &[]);
    }
    let rest = &data[1..];
    let split = (data[0] as usize).min(rest.len());
    (&rest[..split], &rest[split..])
}

/// Opcode-driven expect digest over a MessagePack payload.
pub fn expect_digest_rust(data: &[u8]) -> Digest {
    let data = if data.len() > MAX_INPUT_LEN {
        &data[..MAX_INPUT_LEN]
    } else {
        data
    };
    let (ops, payload) = split_expect_input(data);
    let mut out = Digest::new();
    let mut reader = Reader::new(payload);
    let mut cursor = OpCursor::new(ops);
    let mut buf = [0u8; 256];
    let cstr_keys: [&str; 3] = ["a", "bb", "ccc"];

    while cursor.remaining_ops() && out.truncated == 0 {
        let opcode = cursor.take_u8() % OP_COUNT;
        let mut ok = 0u8;
        let mut value = 0u64;
        let mut hash = 0u32;

        match opcode {
            0 => {
                let _ = expect::nil(&mut reader);
                ok = ok_flag(&reader);
            }
            1 => {
                let v = expect::r#bool(&mut reader);
                ok = ok_flag(&reader);
                value = u64::from(v.unwrap_or(false));
            }
            2 => {
                let _ = expect::true_(&mut reader);
                ok = ok_flag(&reader);
            }
            3 => {
                let _ = expect::false_(&mut reader);
                ok = ok_flag(&reader);
            }
            4 => {
                value = u64::from(expect::u8(&mut reader).unwrap_or(0));
                ok = ok_flag(&reader);
            }
            5 => {
                value = u64::from(expect::u16(&mut reader).unwrap_or(0));
                ok = ok_flag(&reader);
            }
            6 => {
                value = u64::from(expect::u32(&mut reader).unwrap_or(0));
                ok = ok_flag(&reader);
            }
            7 => {
                value = expect::u64(&mut reader).unwrap_or(0);
                ok = ok_flag(&reader);
            }
            8 => {
                value = expect::i8(&mut reader).unwrap_or(0) as u64;
                ok = ok_flag(&reader);
            }
            9 => {
                value = expect::i16(&mut reader).unwrap_or(0) as u64;
                ok = ok_flag(&reader);
            }
            10 => {
                value = expect::i32(&mut reader).unwrap_or(0) as u64;
                ok = ok_flag(&reader);
            }
            11 => {
                value = expect::i64(&mut reader).unwrap_or(0) as u64;
                ok = ok_flag(&reader);
            }
            12 => {
                let min_v = cursor.take_u8();
                let max_v = cursor.take_u8();
                value = u64::from(expect::u8_range(&mut reader, min_v, max_v).unwrap_or(min_v));
                ok = ok_flag(&reader);
            }
            13 => {
                let min_v = cursor.take_u16();
                let max_v = cursor.take_u16();
                value = u64::from(expect::u16_range(&mut reader, min_v, max_v).unwrap_or(min_v));
                ok = ok_flag(&reader);
            }
            14 => {
                let min_v = cursor.take_u32();
                let max_v = cursor.take_u32();
                value = u64::from(expect::u32_range(&mut reader, min_v, max_v).unwrap_or(min_v));
                ok = ok_flag(&reader);
            }
            15 => {
                let min_v = cursor.take_u64();
                let max_v = cursor.take_u64();
                value = expect::u64_range(&mut reader, min_v, max_v).unwrap_or(min_v);
                ok = ok_flag(&reader);
            }
            16 => {
                let min_v = cursor.take_u8() as i8;
                let max_v = cursor.take_u8() as i8;
                value = expect::i8_range(&mut reader, min_v, max_v).unwrap_or(min_v) as u64;
                ok = ok_flag(&reader);
            }
            17 => {
                let min_v = cursor.take_u16() as i16;
                let max_v = cursor.take_u16() as i16;
                value = expect::i16_range(&mut reader, min_v, max_v).unwrap_or(min_v) as u64;
                ok = ok_flag(&reader);
            }
            18 => {
                let min_v = cursor.take_u32() as i32;
                let max_v = cursor.take_u32() as i32;
                value = expect::i32_range(&mut reader, min_v, max_v).unwrap_or(min_v) as u64;
                ok = ok_flag(&reader);
            }
            19 => {
                let min_v = cursor.take_u64() as i64;
                let max_v = cursor.take_u64() as i64;
                value = expect::i64_range(&mut reader, min_v, max_v).unwrap_or(min_v) as u64;
                ok = ok_flag(&reader);
            }
            20 => {
                let want = cursor.take_u64();
                let _ = expect::uint_match(&mut reader, want);
                ok = ok_flag(&reader);
                value = want;
            }
            21 => {
                let want = cursor.take_u64() as i64;
                let _ = expect::int_match(&mut reader, want);
                ok = ok_flag(&reader);
                value = want as u64;
            }
            22 => {
                let v = expect::float(&mut reader).unwrap_or(0.0);
                ok = ok_flag(&reader);
                value = u64::from(v.to_bits());
            }
            23 => {
                let v = expect::double(&mut reader).unwrap_or(0.0);
                ok = ok_flag(&reader);
                value = v.to_bits();
            }
            24 => {
                let v = expect::float_strict(&mut reader).unwrap_or(0.0);
                ok = ok_flag(&reader);
                value = u64::from(v.to_bits());
            }
            25 => {
                let v = expect::double_strict(&mut reader).unwrap_or(0.0);
                ok = ok_flag(&reader);
                value = v.to_bits();
            }
            26 => {
                let min_v = f32::from_bits(cursor.take_u32());
                let max_v = f32::from_bits(cursor.take_u32());
                let v = expect::float_range(&mut reader, min_v, max_v).unwrap_or(min_v);
                ok = ok_flag(&reader);
                value = u64::from(v.to_bits());
            }
            27 => {
                let min_v = f64::from_bits(cursor.take_u64());
                let max_v = f64::from_bits(cursor.take_u64());
                let v = expect::double_range(&mut reader, min_v, max_v).unwrap_or(min_v);
                ok = ok_flag(&reader);
                value = v.to_bits();
            }
            28 => {
                value = u64::from(expect::map(&mut reader).unwrap_or(0));
                ok = ok_flag(&reader);
            }
            29 => {
                let min_c = cursor.take_u32();
                let max_c = cursor.take_u32();
                value = u64::from(expect::map_range(&mut reader, min_c, max_c).unwrap_or(min_c));
                ok = ok_flag(&reader);
            }
            30 => {
                let count = cursor.take_u32();
                let _ = expect::map_match(&mut reader, count);
                ok = ok_flag(&reader);
                value = u64::from(count);
            }
            31 => {
                if let Some(compound) = expect::map_or_nil(&mut reader) {
                    value = encode_or_nil(compound);
                }
                ok = ok_flag(&reader);
            }
            32 => {
                let max_c = cursor.take_u32();
                if let Some(compound) = expect::map_max_or_nil(&mut reader, max_c) {
                    value = encode_or_nil(compound);
                }
                ok = ok_flag(&reader);
            }
            33 => {
                value = u64::from(expect::array(&mut reader).unwrap_or(0));
                ok = ok_flag(&reader);
            }
            34 => {
                let min_c = cursor.take_u32();
                let max_c = cursor.take_u32();
                value = u64::from(expect::array_range(&mut reader, min_c, max_c).unwrap_or(min_c));
                ok = ok_flag(&reader);
            }
            35 => {
                let count = cursor.take_u32();
                let _ = expect::array_match(&mut reader, count);
                ok = ok_flag(&reader);
                value = u64::from(count);
            }
            36 => {
                if let Some(compound) = expect::array_or_nil(&mut reader) {
                    value = encode_or_nil(compound);
                }
                ok = ok_flag(&reader);
            }
            37 => {
                let max_c = cursor.take_u32();
                if let Some(compound) = expect::array_max_or_nil(&mut reader, max_c) {
                    value = encode_or_nil(compound);
                }
                ok = ok_flag(&reader);
            }
            38 => {
                value = u64::from(expect::r#str(&mut reader).unwrap_or(0));
                ok = ok_flag(&reader);
            }
            39 => {
                if let Some(n) = expect::str_buf(&mut reader, &mut buf) {
                    value = n as u64;
                    hash = fnv1a32(&buf[..n]);
                }
                ok = ok_flag(&reader);
            }
            40 => {
                if let Some(n) = expect::utf8(&mut reader, &mut buf) {
                    value = n as u64;
                    hash = fnv1a32(&buf[..n]);
                }
                ok = ok_flag(&reader);
            }
            41 => {
                let mut n = cursor.take_u8();
                if n > 32 {
                    n = 32;
                }
                let mut expect_buf = [0u8; 32];
                for i in 0..n as usize {
                    expect_buf[i] = cursor.take_u8();
                }
                let _ = expect::str_match(&mut reader, &expect_buf[..n as usize]);
                ok = ok_flag(&reader);
                value = u64::from(n);
                hash = fnv1a32(&expect_buf[..n as usize]);
            }
            42 => {
                if expect::cstr(&mut reader, &mut buf) {
                    let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                    value = n as u64;
                    hash = fnv1a32(&buf[..n]);
                }
                ok = ok_flag(&reader);
            }
            43 => {
                if expect::utf8_cstr(&mut reader, &mut buf) {
                    let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                    value = n as u64;
                    hash = fnv1a32(&buf[..n]);
                }
                ok = ok_flag(&reader);
            }
            44 => {
                value = u64::from(expect::bin(&mut reader).unwrap_or(0));
                ok = ok_flag(&reader);
            }
            45 => {
                if let Some(n) = expect::bin_buf(&mut reader, &mut buf) {
                    value = n as u64;
                    hash = fnv1a32(&buf[..n]);
                }
                ok = ok_flag(&reader);
            }
            46 => {
                let mut size = cursor.take_u32();
                if size as usize > buf.len() {
                    size = buf.len() as u32;
                }
                if expect::bin_size_buf(&mut reader, &mut buf, size) {
                    hash = fnv1a32(&buf[..size as usize]);
                }
                ok = ok_flag(&reader);
                value = u64::from(size);
            }
            47 => {
                if let Some((ext_type, n)) = expect::ext(&mut reader) {
                    value = (u64::from(ext_type as u8) << 32) | u64::from(n);
                }
                ok = ok_flag(&reader);
            }
            48 => {
                if let Some((ext_type, n)) = expect::ext_buf(&mut reader, &mut buf) {
                    value = (u64::from(ext_type as u8) << 32) | n as u64;
                    hash = fnv1a32(&buf[..n]);
                }
                ok = ok_flag(&reader);
            }
            49 => {
                let t = cursor.take_u8() % 12;
                let expected = match t {
                    2 => Tag::Bool(cursor.take_u8() & 1 != 0),
                    3 => Tag::Int(cursor.take_u64() as i64),
                    4 => Tag::Uint(cursor.take_u64()),
                    5 => Tag::Float(f32::from_bits(cursor.take_u32())),
                    6 => Tag::Double(f64::from_bits(cursor.take_u64())),
                    7 => Tag::Str(cursor.take_u32()),
                    8 => Tag::Bin(cursor.take_u32()),
                    9 => Tag::Array(cursor.take_u32()),
                    10 => Tag::Map(cursor.take_u32()),
                    11 => Tag::Ext {
                        extension_type: cursor.take_u8() as i8,
                        length: cursor.take_u32(),
                    },
                    _ => Tag::Nil,
                };
                let _ = expect::tag(&mut reader, expected);
                ok = ok_flag(&reader);
                value = u64::from(t);
            }
            50 => {
                if let Some(Timestamp {
                    seconds,
                    nanoseconds,
                }) = expect::timestamp(&mut reader)
                {
                    value = (u64::from(nanoseconds) << 32) ^ seconds as u64;
                }
                ok = ok_flag(&reader);
            }
            51 => {
                value = expect::timestamp_truncate(&mut reader).unwrap_or(0) as u64;
                ok = ok_flag(&reader);
            }
            52 => {
                let mut n = cursor.take_u8();
                if n == 0 || n > 8 {
                    n = 4;
                }
                let mut found = [false; 8];
                if let Some(idx) = expect::key_uint(&mut reader, &mut found[..n as usize]) {
                    value = idx as u64;
                }
                ok = ok_flag(&reader);
            }
            53 => {
                let mut found = [false; 3];
                if let Some(idx) = expect::key_cstr(&mut reader, &cstr_keys, &mut found) {
                    value = idx as u64;
                }
                ok = ok_flag(&reader);
            }
            _ => {}
        }

        if ok == 0 {
            value = 0;
            hash = 0;
        }

        if !out.push(opcode, ok, value, hash) {
            break;
        }
        if reader.error() != Error::Ok {
            break;
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
