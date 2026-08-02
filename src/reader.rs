//! Incremental / buffered decoder (mirrors `mpack-reader`).
//!
//! Public methods on [`Reader`] are a frozen safe-core contract. Signature
//! changes need lead approval and a `DECISIONS.md` entry (Reader vertical
//! slice / hotspots). Streaming fill, file init, and `done_*` tracking stay out
//! of this module (FFI / later slices).

use crate::common::{Error, Tag, Timestamp, TIMESTAMP_NANOSECONDS_MAX};

/// A MessagePack reader over caller-owned, fixed input data.
///
/// The first error is sticky: after it is set, decoding and payload operations
/// return without consuming any more input.
#[derive(Debug)]
pub struct Reader<'data> {
    data: &'data [u8],
    position: usize,
    error: Error,
}

impl<'data> Reader<'data> {
    /// Creates a reader over a complete MessagePack byte slice.
    pub const fn new(data: &'data [u8]) -> Self {
        Self {
            data,
            position: 0,
            error: Error::Ok,
        }
    }

    /// Returns the reader's sticky error.
    pub const fn error(&self) -> Error {
        self.error
    }

    /// Returns the number of bytes consumed.
    pub const fn used(&self) -> usize {
        self.position
    }

    /// Returns the number of unconsumed bytes.
    pub const fn remaining(&self) -> usize {
        self.data.len() - self.position
    }

    /// Records an error if the reader is currently error-free.
    pub fn flag_error(&mut self, error: Error) {
        if self.error == Error::Ok && error != Error::Ok {
            self.error = error;
        }
    }

    /// Decodes the next MessagePack tag, leaving any str/bin/ext payload unread.
    pub fn read_tag(&mut self) -> Option<Tag> {
        self.parse_tag(true)
    }

    /// Peeks the next MessagePack tag without consuming header bytes.
    pub fn peek_tag(&mut self) -> Option<Tag> {
        self.parse_tag(false)
    }

    /// Skips one full MessagePack value (recursive for array/map).
    pub fn discard(&mut self) {
        let Some(tag) = self.read_tag() else {
            return;
        };
        match tag {
            Tag::Str(length) | Tag::Bin(length) | Tag::Ext { length, .. } => {
                let _ = self.skip_bytes(length as usize);
            }
            Tag::Array(count) => {
                for _ in 0..count {
                    self.discard();
                    if self.error != Error::Ok {
                        break;
                    }
                }
            }
            Tag::Map(count) => {
                for _ in 0..count {
                    self.discard();
                    self.discard();
                    if self.error != Error::Ok {
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    /// Reads a timestamp extension payload of `size` bytes (4, 8, or 12).
    ///
    /// Call after opening an ext tag whose payload length is `size`. Does not
    /// validate the extension type byte (C leaves that to the caller/expect).
    pub fn read_timestamp(&mut self, size: usize) -> Option<Timestamp> {
        if self.error != Error::Ok {
            return None;
        }
        if size != 4 && size != 8 && size != 12 {
            self.flag_error(Error::Invalid);
            return None;
        }
        if !self.require(size) {
            return None;
        }
        let start = self.position;
        let end = start + size;
        let bytes = &self.data[start..end];
        let timestamp = match size {
            4 => Timestamp {
                seconds: u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64,
                nanoseconds: 0,
            },
            8 => {
                let packed = u64::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]);
                Timestamp {
                    seconds: (packed & ((1u64 << 34) - 1)) as i64,
                    nanoseconds: (packed >> 34) as u32,
                }
            }
            12 => Timestamp {
                nanoseconds: u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                seconds: i64::from_be_bytes([
                    bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10],
                    bytes[11],
                ]),
            },
            _ => unreachable!(),
        };
        if timestamp.nanoseconds > TIMESTAMP_NANOSECONDS_MAX {
            self.flag_error(Error::Invalid);
            return None;
        }
        self.position = end;
        Some(timestamp)
    }

    /// Reads a nil value.
    pub fn read_nil(&mut self) -> bool {
        match self.read_tag() {
            Some(Tag::Nil) => true,
            Some(_) => {
                self.flag_error(Error::Type);
                false
            }
            None => false,
        }
    }

    /// Reads a boolean value.
    pub fn read_bool(&mut self) -> Option<bool> {
        match self.read_tag()? {
            Tag::Bool(value) => Some(value),
            _ => {
                self.flag_error(Error::Type);
                None
            }
        }
    }

    /// Reads an integer representable as `u64`.
    pub fn read_u64(&mut self) -> Option<u64> {
        match self.read_tag()? {
            Tag::Uint(value) => Some(value),
            Tag::Int(value) if value >= 0 => Some(value as u64),
            _ => {
                self.flag_error(Error::Type);
                None
            }
        }
    }

    /// Reads an integer representable as `i64`.
    pub fn read_i64(&mut self) -> Option<i64> {
        match self.read_tag()? {
            Tag::Int(value) => Some(value),
            Tag::Uint(value) => match i64::try_from(value) {
                Ok(value) => Some(value),
                Err(_) => {
                    self.flag_error(Error::Type);
                    None
                }
            },
            _ => {
                self.flag_error(Error::Type);
                None
            }
        }
    }

    /// Reads a 32-bit floating-point value.
    pub fn read_f32(&mut self) -> Option<f32> {
        match self.read_tag()? {
            Tag::Float(value) => Some(value),
            _ => {
                self.flag_error(Error::Type);
                None
            }
        }
    }

    /// Reads a float, widening an encoded float32 when necessary.
    pub fn read_f64(&mut self) -> Option<f64> {
        match self.read_tag()? {
            Tag::Float(value) => Some(value as f64),
            Tag::Double(value) => Some(value),
            _ => {
                self.flag_error(Error::Type);
                None
            }
        }
    }

    /// Reads a string header and returns its byte length.
    pub fn read_str_header(&mut self) -> Option<u32> {
        self.read_length(|tag| match tag {
            Tag::Str(length) => Some(length),
            _ => None,
        })
    }

    /// Reads a binary header and returns its byte length.
    pub fn read_bin_header(&mut self) -> Option<u32> {
        self.read_length(|tag| match tag {
            Tag::Bin(length) => Some(length),
            _ => None,
        })
    }

    /// Reads an array header and returns its element count.
    pub fn read_array_header(&mut self) -> Option<u32> {
        self.read_length(|tag| match tag {
            Tag::Array(length) => Some(length),
            _ => None,
        })
    }

    /// Reads a map header and returns its key-value pair count.
    pub fn read_map_header(&mut self) -> Option<u32> {
        self.read_length(|tag| match tag {
            Tag::Map(length) => Some(length),
            _ => None,
        })
    }

    /// Reads an extension header and returns `(extension_type, byte_length)`.
    pub fn read_ext_header(&mut self) -> Option<(i8, u32)> {
        match self.read_tag()? {
            Tag::Ext {
                extension_type,
                length,
            } => Some((extension_type, length)),
            _ => {
                self.flag_error(Error::Type);
                None
            }
        }
    }

    /// Borrows exactly `length` payload bytes and advances past them.
    pub fn read_bytes(&mut self, length: usize) -> Option<&'data [u8]> {
        if self.error != Error::Ok || !self.require(length) {
            return None;
        }
        let start = self.position;
        self.position += length;
        Some(&self.data[start..self.position])
    }

    /// Reads `length` payload bytes and requires well-formed UTF-8.
    pub fn read_bytes_utf8(&mut self, length: usize) -> Option<&'data [u8]> {
        if self.error != Error::Ok || !self.require(length) {
            return None;
        }
        let start = self.position;
        let end = start + length;
        let bytes = &self.data[start..end];
        if !check_utf8(bytes) {
            self.flag_error(Error::Type);
            return None;
        }
        self.position = end;
        Some(bytes)
    }

    /// Advances past exactly `length` payload bytes.
    pub fn skip_bytes(&mut self, length: usize) -> bool {
        if self.error != Error::Ok || !self.require(length) {
            return false;
        }
        self.position += length;
        true
    }

    fn parse_tag(&mut self, consume: bool) -> Option<Tag> {
        if self.error != Error::Ok {
            return None;
        }

        let start = self.position;
        let marker = match self.data.get(self.position).copied() {
            Some(marker) => marker,
            None => {
                self.flag_error(Error::Invalid);
                return None;
            }
        };
        let header_size = match marker {
            0x00..=0xbf | 0xc0..=0xc3 | 0xd4..=0xd8 | 0xe0..=0xff => 1,
            0xc4 | 0xc7 | 0xcc | 0xd0 | 0xd9 => 2,
            0xc5 | 0xc8 | 0xcd | 0xd1 | 0xda | 0xdc | 0xde => 3,
            0xc6 | 0xc9 | 0xce | 0xd2 | 0xdb | 0xdd | 0xdf => 5,
            0xca => 5,
            0xcb | 0xcf | 0xd3 => 9,
        };
        // Extension headers additionally contain their signed extension type.
        let header_size = header_size
            + usize::from(matches!(marker, 0xc7..=0xc9 | 0xd4..=0xd8));
        if !self.require(header_size) {
            return None;
        }

        self.position += 1;
        let tag = match marker {
            0x00..=0x7f => Tag::Uint(marker as u64),
            0x80..=0x8f => Tag::Map((marker & 0x0f) as u32),
            0x90..=0x9f => Tag::Array((marker & 0x0f) as u32),
            0xa0..=0xbf => Tag::Str((marker & 0x1f) as u32),
            0xc0 => Tag::Nil,
            0xc1 => {
                self.flag_error(Error::Invalid);
                if !consume {
                    self.position = start;
                }
                return None;
            }
            0xc2 => Tag::Bool(false),
            0xc3 => Tag::Bool(true),
            0xc4 => Tag::Bin(self.take_u8() as u32),
            0xc5 => Tag::Bin(self.take_u16() as u32),
            0xc6 => Tag::Bin(self.take_u32()),
            0xc7 => {
                let length = self.take_u8() as u32;
                self.take_ext(length)
            }
            0xc8 => {
                let length = self.take_u16() as u32;
                self.take_ext(length)
            }
            0xc9 => {
                let length = self.take_u32();
                self.take_ext(length)
            }
            0xca => Tag::Float(f32::from_bits(self.take_u32())),
            0xcb => Tag::Double(f64::from_bits(self.take_u64())),
            0xcc => Tag::Uint(self.take_u8() as u64),
            0xcd => Tag::Uint(self.take_u16() as u64),
            0xce => Tag::Uint(self.take_u32() as u64),
            0xcf => Tag::Uint(self.take_u64()),
            0xd0 => Tag::Int(self.take_u8() as i8 as i64),
            0xd1 => Tag::Int(self.take_u16() as i16 as i64),
            0xd2 => Tag::Int(self.take_u32() as i32 as i64),
            0xd3 => Tag::Int(self.take_u64() as i64),
            0xd4 => self.take_ext(1),
            0xd5 => self.take_ext(2),
            0xd6 => self.take_ext(4),
            0xd7 => self.take_ext(8),
            0xd8 => self.take_ext(16),
            0xd9 => Tag::Str(self.take_u8() as u32),
            0xda => Tag::Str(self.take_u16() as u32),
            0xdb => Tag::Str(self.take_u32()),
            0xdc => Tag::Array(self.take_u16() as u32),
            0xdd => Tag::Array(self.take_u32()),
            0xde => Tag::Map(self.take_u16() as u32),
            0xdf => Tag::Map(self.take_u32()),
            0xe0..=0xff => Tag::Int(marker as i8 as i64),
        };
        if !consume {
            self.position = start;
        }
        Some(tag)
    }

    fn read_length(&mut self, select: impl FnOnce(Tag) -> Option<u32>) -> Option<u32> {
        let tag = self.read_tag()?;
        match select(tag) {
            Some(length) => Some(length),
            None => {
                self.flag_error(Error::Type);
                None
            }
        }
    }

    fn require(&mut self, length: usize) -> bool {
        if self.data.len().saturating_sub(self.position) < length {
            self.flag_error(Error::Invalid);
            false
        } else {
            true
        }
    }

    fn take_u8(&mut self) -> u8 {
        let value = self.data[self.position];
        self.position += 1;
        value
    }

    fn take_u16(&mut self) -> u16 {
        let bytes = [self.take_u8(), self.take_u8()];
        u16::from_be_bytes(bytes)
    }

    fn take_u32(&mut self) -> u32 {
        let bytes = [
            self.take_u8(),
            self.take_u8(),
            self.take_u8(),
            self.take_u8(),
        ];
        u32::from_be_bytes(bytes)
    }

    fn take_u64(&mut self) -> u64 {
        let bytes = [
            self.take_u8(),
            self.take_u8(),
            self.take_u8(),
            self.take_u8(),
            self.take_u8(),
            self.take_u8(),
            self.take_u8(),
            self.take_u8(),
        ];
        u64::from_be_bytes(bytes)
    }

    fn take_ext(&mut self, length: u32) -> Tag {
        Tag::Ext {
            extension_type: self.take_u8() as i8,
            length,
        }
    }
}

/// Returns whether `bytes` is well-formed UTF-8 (NUL bytes allowed).
pub fn check_utf8(bytes: &[u8]) -> bool {
    check_utf8_impl(bytes, true)
}

/// Returns whether `bytes` is well-formed UTF-8 with no interior NUL.
pub fn check_utf8_no_null(bytes: &[u8]) -> bool {
    check_utf8_impl(bytes, false)
}

fn check_utf8_impl(bytes: &[u8], allow_null: bool) -> bool {
    let mut i = 0;
    while i < bytes.len() {
        let lead = bytes[i];
        if !allow_null && lead == 0 {
            return false;
        }
        if lead <= 0x7f {
            i += 1;
            continue;
        }
        if lead & 0xe0 == 0xc0 {
            if i + 1 >= bytes.len() || bytes[i + 1] & 0xc0 != 0x80 {
                return false;
            }
            let z = u32::from(lead & !0xe0) << 6 | u32::from(bytes[i + 1] & !0xc0);
            if z < 0x80 {
                return false;
            }
            i += 2;
            continue;
        }
        if lead & 0xf0 == 0xe0 {
            if i + 2 >= bytes.len()
                || bytes[i + 1] & 0xc0 != 0x80
                || bytes[i + 2] & 0xc0 != 0x80
            {
                return false;
            }
            let z = u32::from(lead & !0xf0) << 12
                | u32::from(bytes[i + 1] & !0xc0) << 6
                | u32::from(bytes[i + 2] & !0xc0);
            if z < 0x800 || (0xd800..=0xdfff).contains(&z) {
                return false;
            }
            i += 3;
            continue;
        }
        if lead & 0xf8 == 0xf0 {
            if i + 3 >= bytes.len()
                || bytes[i + 1] & 0xc0 != 0x80
                || bytes[i + 2] & 0xc0 != 0x80
                || bytes[i + 3] & 0xc0 != 0x80
            {
                return false;
            }
            let z = u32::from(lead & !0xf8) << 18
                | u32::from(bytes[i + 1] & !0xc0) << 12
                | u32::from(bytes[i + 2] & !0xc0) << 6
                | u32::from(bytes[i + 3] & !0xc0);
            if !(0x10000..=0x10_ffff).contains(&z) {
                return false;
            }
            i += 4;
            continue;
        }
        return false;
    }
    true
}
