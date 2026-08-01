//! Schema-typed reads on top of the reader (mirrors `mpack-expect`).
//!
//! Public free functions here are a frozen safe-core contract (see
//! `DECISIONS.md`). Teammates may fill or fix bodies; signature changes need
//! lead approval. Allocator-backed `*_alloc` APIs stay in FFI only.

use crate::common::{Error, Tag, Timestamp};
use crate::reader::{self, Reader};

/// Result of `*_or_nil` compound expects (maps to C `(bool, *count)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectCompound {
    pub is_nil: bool,
    pub count: u32,
}

fn type_error<T>(reader: &mut Reader<'_>) -> Option<T> {
    reader.flag_error(Error::Type);
    None
}

fn too_big<T>(reader: &mut Reader<'_>) -> Option<T> {
    reader.flag_error(Error::TooBig);
    None
}

fn fail_bool(reader: &mut Reader<'_>) -> bool {
    reader.flag_error(Error::Type);
    false
}

fn fail_too_big(reader: &mut Reader<'_>) -> bool {
    reader.flag_error(Error::TooBig);
    false
}

fn read_i64_value(reader: &mut Reader<'_>) -> Option<i64> {
    match reader.read_tag()? {
        Tag::Int(value) => Some(value),
        Tag::Uint(value) => i64::try_from(value).ok().or_else(|| type_error(reader)),
        _ => type_error(reader),
    }
}

fn read_u64_value(reader: &mut Reader<'_>) -> Option<u64> {
    match reader.read_tag()? {
        Tag::Uint(value) => Some(value),
        Tag::Int(value) if value >= 0 => Some(value as u64),
        _ => type_error(reader),
    }
}

macro_rules! expect_uint {
    ($name:ident, $ty:ty) => {
        pub fn $name(reader: &mut Reader<'_>) -> Option<$ty> {
            let value = read_u64_value(reader)?;
            if value <= <$ty>::MAX as u64 {
                Some(value as $ty)
            } else {
                type_error(reader)
            }
        }
    };
}

macro_rules! expect_int {
    ($name:ident, $ty:ty) => {
        pub fn $name(reader: &mut Reader<'_>) -> Option<$ty> {
            let value = read_i64_value(reader)?;
            if (i64::from(<$ty>::MIN)..=i64::from(<$ty>::MAX)).contains(&value) {
                Some(value as $ty)
            } else {
                type_error(reader)
            }
        }
    };
}

macro_rules! expect_uint_range {
    ($name:ident, $base:ident, $ty:ty) => {
        pub fn $name(reader: &mut Reader<'_>, min_value: $ty, max_value: $ty) -> Option<$ty> {
            let value = $base(reader)?;
            if value >= min_value && value <= max_value {
                Some(value)
            } else {
                type_error(reader)
            }
        }
    };
}

macro_rules! expect_int_range {
    ($name:ident, $base:ident, $ty:ty) => {
        pub fn $name(reader: &mut Reader<'_>, min_value: $ty, max_value: $ty) -> Option<$ty> {
            let value = $base(reader)?;
            if value >= min_value && value <= max_value {
                Some(value)
            } else {
                type_error(reader)
            }
        }
    };
}

expect_uint!(u8, u8);
expect_uint!(u16, u16);
expect_uint!(u32, u32);
expect_uint!(u64, u64);
expect_int!(i8, i8);
expect_int!(i16, i16);
expect_int!(i32, i32);
expect_int!(i64, i64);

expect_uint_range!(u8_range, u8, u8);
expect_uint_range!(u16_range, u16, u16);
expect_uint_range!(u32_range, u32, u32);
expect_uint_range!(u64_range, u64, u64);
expect_int_range!(i8_range, i8, i8);
expect_int_range!(i16_range, i16, i16);
expect_int_range!(i32_range, i32, i32);
expect_int_range!(i64_range, i64, i64);

pub fn float(reader: &mut Reader<'_>) -> Option<f32> {
    match reader.read_tag()? {
        Tag::Float(value) => Some(value),
        Tag::Double(value) => Some(value as f32),
        Tag::Uint(value) => Some(value as f32),
        Tag::Int(value) => Some(value as f32),
        _ => type_error(reader),
    }
}

pub fn double(reader: &mut Reader<'_>) -> Option<f64> {
    match reader.read_tag()? {
        Tag::Float(value) => Some(f64::from(value)),
        Tag::Double(value) => Some(value),
        Tag::Uint(value) => Some(value as f64),
        Tag::Int(value) => Some(value as f64),
        _ => type_error(reader),
    }
}

pub fn float_strict(reader: &mut Reader<'_>) -> Option<f32> {
    match reader.read_tag()? {
        Tag::Float(value) => Some(value),
        _ => type_error(reader),
    }
}

pub fn double_strict(reader: &mut Reader<'_>) -> Option<f64> {
    match reader.read_tag()? {
        Tag::Double(value) => Some(value),
        _ => type_error(reader),
    }
}

pub fn float_range(reader: &mut Reader<'_>, min_value: f32, max_value: f32) -> Option<f32> {
    let value = float(reader)?;
    if value >= min_value && value <= max_value {
        Some(value)
    } else {
        type_error(reader)
    }
}

pub fn double_range(reader: &mut Reader<'_>, min_value: f64, max_value: f64) -> Option<f64> {
    let value = double(reader)?;
    if value >= min_value && value <= max_value {
        Some(value)
    } else {
        type_error(reader)
    }
}

pub fn uint_match(reader: &mut Reader<'_>, value: u64) -> bool {
    match u64(reader) {
        Some(got) if got == value => true,
        Some(_) => fail_bool(reader),
        None => false,
    }
}

pub fn int_match(reader: &mut Reader<'_>, value: i64) -> bool {
    match i64(reader) {
        Some(got) if got == value => true,
        Some(_) => fail_bool(reader),
        None => false,
    }
}

pub fn nil(reader: &mut Reader<'_>) -> bool {
    match reader.read_tag() {
        Some(Tag::Nil) => true,
        Some(_) => fail_bool(reader),
        None => false,
    }
}

pub fn r#bool(reader: &mut Reader<'_>) -> Option<bool> {
    match reader.read_tag()? {
        Tag::Bool(value) => Some(value),
        _ => type_error(reader),
    }
}

pub fn true_(reader: &mut Reader<'_>) -> bool {
    match r#bool(reader) {
        Some(true) => true,
        Some(false) => fail_bool(reader),
        None => false,
    }
}

pub fn false_(reader: &mut Reader<'_>) -> bool {
    match r#bool(reader) {
        Some(false) => true,
        Some(true) => fail_bool(reader),
        None => false,
    }
}

/// Timestamp extension type used by MessagePack timestamp ext.
pub const TIMESTAMP_EXT_TYPE: i8 = -1;

pub fn timestamp(reader: &mut Reader<'_>) -> Option<Timestamp> {
    let (ext_type, length) = ext(reader)?;
    if ext_type != TIMESTAMP_EXT_TYPE {
        return type_error(reader);
    }
    reader.read_timestamp(length as usize)
}

pub fn timestamp_truncate(reader: &mut Reader<'_>) -> Option<i64> {
    timestamp(reader).map(|value| value.seconds)
}

fn compound_count(reader: &mut Reader<'_>, want_map: bool) -> Option<u32> {
    match reader.read_tag()? {
        Tag::Map(count) if want_map => Some(count),
        Tag::Array(count) if !want_map => Some(count),
        _ => type_error(reader),
    }
}

fn compound_range(
    reader: &mut Reader<'_>,
    want_map: bool,
    min_count: u32,
    max_count: u32,
) -> Option<u32> {
    let count = compound_count(reader, want_map)?;
    if count >= min_count && count <= max_count {
        Some(count)
    } else {
        type_error(reader)
    }
}

fn compound_or_nil(reader: &mut Reader<'_>, want_map: bool, max_count: Option<u32>) -> Option<ExpectCompound> {
    match reader.peek_tag()? {
        Tag::Nil => {
            let _ = reader.read_tag();
            Some(ExpectCompound {
                is_nil: true,
                count: 0,
            })
        }
        Tag::Map(_) if want_map => {
            let count = match max_count {
                Some(max) => compound_range(reader, true, 0, max)?,
                None => compound_count(reader, true)?,
            };
            Some(ExpectCompound {
                is_nil: false,
                count,
            })
        }
        Tag::Array(_) if !want_map => {
            let count = match max_count {
                Some(max) => compound_range(reader, false, 0, max)?,
                None => compound_count(reader, false)?,
            };
            Some(ExpectCompound {
                is_nil: false,
                count,
            })
        }
        _ => {
            let _ = reader.read_tag();
            type_error(reader)
        }
    }
}

pub fn map(reader: &mut Reader<'_>) -> Option<u32> {
    compound_count(reader, true)
}

pub fn map_range(reader: &mut Reader<'_>, min_count: u32, max_count: u32) -> Option<u32> {
    compound_range(reader, true, min_count, max_count)
}

pub fn map_match(reader: &mut Reader<'_>, count: u32) -> bool {
    match map(reader) {
        Some(got) if got == count => true,
        Some(_) => fail_bool(reader),
        None => false,
    }
}

pub fn map_or_nil(reader: &mut Reader<'_>) -> Option<ExpectCompound> {
    compound_or_nil(reader, true, None)
}

pub fn map_max_or_nil(reader: &mut Reader<'_>, max_count: u32) -> Option<ExpectCompound> {
    compound_or_nil(reader, true, Some(max_count))
}

pub fn array(reader: &mut Reader<'_>) -> Option<u32> {
    compound_count(reader, false)
}

pub fn array_range(reader: &mut Reader<'_>, min_count: u32, max_count: u32) -> Option<u32> {
    compound_range(reader, false, min_count, max_count)
}

pub fn array_match(reader: &mut Reader<'_>, count: u32) -> bool {
    match array(reader) {
        Some(got) if got == count => true,
        Some(_) => fail_bool(reader),
        None => false,
    }
}

pub fn array_or_nil(reader: &mut Reader<'_>) -> Option<ExpectCompound> {
    compound_or_nil(reader, false, None)
}

pub fn array_max_or_nil(reader: &mut Reader<'_>, max_count: u32) -> Option<ExpectCompound> {
    compound_or_nil(reader, false, Some(max_count))
}

pub fn r#str(reader: &mut Reader<'_>) -> Option<u32> {
    match reader.read_tag()? {
        Tag::Str(length) => Some(length),
        _ => type_error(reader),
    }
}

fn copy_bytes(dst: &mut [u8], src: &[u8]) -> Option<usize> {
    if src.len() > dst.len() {
        return None;
    }
    dst[..src.len()].copy_from_slice(src);
    Some(src.len())
}

fn copy_cstr(dst: &mut [u8], src: &[u8]) -> bool {
    if src.len() + 1 > dst.len() {
        return false;
    }
    dst[..src.len()].copy_from_slice(src);
    dst[src.len()] = 0;
    true
}

pub fn str_buf(reader: &mut Reader<'_>, buf: &mut [u8]) -> Option<usize> {
    let length = r#str(reader)? as usize;
    if length > buf.len() {
        return too_big(reader);
    }
    let bytes = reader.read_bytes(length)?;
    copy_bytes(buf, bytes)
}

pub fn utf8(reader: &mut Reader<'_>, buf: &mut [u8]) -> Option<usize> {
    let length = r#str(reader)? as usize;
    if length > buf.len() {
        return too_big(reader);
    }
    let bytes = reader.read_bytes(length)?;
    let written = copy_bytes(buf, bytes)?;
    if !reader::check_utf8(&buf[..written]) {
        reader.flag_error(Error::Type);
        return None;
    }
    Some(written)
}

pub fn str_match(reader: &mut Reader<'_>, expected: &[u8]) -> bool {
    let Some(length) = r#str(reader) else {
        return false;
    };
    if length as usize != expected.len() {
        return fail_bool(reader);
    }
    match reader.read_bytes(expected.len()) {
        Some(bytes) if bytes == expected => true,
        Some(_) => fail_bool(reader),
        None => false,
    }
}

pub fn cstr(reader: &mut Reader<'_>, buf: &mut [u8]) -> bool {
    if buf.is_empty() {
        reader.flag_error(Error::Bug);
        return false;
    }
    let Some(length) = r#str(reader) else {
        buf[0] = 0;
        return false;
    };
    let length = length as usize;
    if length + 1 > buf.len() {
        buf[0] = 0;
        return fail_too_big(reader);
    }
    let Some(bytes) = reader.read_bytes(length) else {
        buf[0] = 0;
        return false;
    };
    if bytes.contains(&0) {
        buf[0] = 0;
        return fail_bool(reader);
    }
    let _ = copy_cstr(buf, bytes);
    true
}

pub fn utf8_cstr(reader: &mut Reader<'_>, buf: &mut [u8]) -> bool {
    if buf.is_empty() {
        reader.flag_error(Error::Bug);
        return false;
    }
    let Some(length) = r#str(reader) else {
        buf[0] = 0;
        return false;
    };
    let length = length as usize;
    if length + 1 > buf.len() {
        buf[0] = 0;
        return fail_too_big(reader);
    }
    let Some(bytes) = reader.read_bytes(length) else {
        buf[0] = 0;
        return false;
    };
    if !reader::check_utf8_no_null(bytes) {
        buf[0] = 0;
        return fail_bool(reader);
    }
    let _ = copy_cstr(buf, bytes);
    true
}

pub fn bin(reader: &mut Reader<'_>) -> Option<u32> {
    match reader.read_tag()? {
        Tag::Bin(length) => Some(length),
        _ => type_error(reader),
    }
}

pub fn bin_buf(reader: &mut Reader<'_>, buf: &mut [u8]) -> Option<usize> {
    let length = bin(reader)? as usize;
    if length > buf.len() {
        return too_big(reader);
    }
    let bytes = reader.read_bytes(length)?;
    copy_bytes(buf, bytes)
}

pub fn bin_size_buf(reader: &mut Reader<'_>, buf: &mut [u8], size: u32) -> bool {
    match bin(reader) {
        Some(length) if length == size => {}
        Some(_) => return fail_bool(reader),
        None => return false,
    }
    match reader.read_bytes(size as usize) {
        Some(bytes) => copy_bytes(buf, bytes).is_some() || fail_bool(reader),
        None => false,
    }
}

pub fn ext(reader: &mut Reader<'_>) -> Option<(i8, u32)> {
    match reader.read_tag()? {
        Tag::Ext {
            extension_type,
            length,
        } => Some((extension_type, length)),
        _ => type_error(reader),
    }
}

pub fn ext_buf(reader: &mut Reader<'_>, buf: &mut [u8]) -> Option<(i8, usize)> {
    let (ext_type, length) = ext(reader)?;
    if length as usize > buf.len() {
        return too_big(reader);
    }
    let bytes = reader.read_bytes(length as usize)?;
    let written = copy_bytes(buf, bytes)?;
    Some((ext_type, written))
}

pub fn tag(reader: &mut Reader<'_>, expected: Tag) -> bool {
    match reader.read_tag() {
        Some(got) if tags_equal(got, expected) => true,
        Some(_) => fail_bool(reader),
        None => false,
    }
}

fn tags_equal(left: Tag, right: Tag) -> bool {
    match (left, right) {
        (Tag::Nil, Tag::Nil) => true,
        (Tag::Bool(a), Tag::Bool(b)) => a == b,
        (Tag::Int(a), Tag::Int(b)) => a == b,
        (Tag::Uint(a), Tag::Uint(b)) => a == b,
        (Tag::Float(a), Tag::Float(b)) => a.to_bits() == b.to_bits(),
        (Tag::Double(a), Tag::Double(b)) => a.to_bits() == b.to_bits(),
        (Tag::Str(a), Tag::Str(b)) => a == b,
        (Tag::Bin(a), Tag::Bin(b)) => a == b,
        (Tag::Array(a), Tag::Array(b)) => a == b,
        (Tag::Map(a), Tag::Map(b)) => a == b,
        (
            Tag::Ext {
                extension_type: a_ty,
                length: a_len,
            },
            Tag::Ext {
                extension_type: b_ty,
                length: b_len,
            },
        ) => a_ty == b_ty && a_len == b_len,
        _ => false,
    }
}

pub fn key_uint(reader: &mut Reader<'_>, found: &mut [bool]) -> Option<usize> {
    if found.is_empty() {
        reader.flag_error(Error::Bug);
        return None;
    }
    if !matches!(reader.peek_tag()?, Tag::Uint(_)) {
        reader.discard();
        return Some(found.len());
    }
    let key = u64(reader)?;
    if key >= found.len() as u64 {
        return Some(found.len());
    }
    let index = key as usize;
    if found[index] {
        reader.flag_error(Error::Invalid);
        return None;
    }
    found[index] = true;
    Some(index)
}

pub fn key_cstr(reader: &mut Reader<'_>, keys: &[&str], found: &mut [bool]) -> Option<usize> {
    if keys.is_empty() || keys.len() != found.len() {
        reader.flag_error(Error::Bug);
        return None;
    }
    if !matches!(reader.peek_tag()?, Tag::Str(_)) {
        reader.discard();
        return Some(keys.len());
    }
    let length = r#str(reader)? as usize;
    let bytes = reader.read_bytes(length)?;
    for (index, key) in keys.iter().enumerate() {
        if key.as_bytes() == bytes {
            if found[index] {
                reader.flag_error(Error::Invalid);
                return None;
            }
            found[index] = true;
            return Some(index);
        }
    }
    Some(keys.len())
}
