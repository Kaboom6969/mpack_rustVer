//! Incremental encoder / growable buffer writer (mirrors `mpack-writer`).

use crate::common::Error;

/// A fixed-buffer MessagePack writer.
#[derive(Debug)]
pub struct Writer<'buffer> {
    buffer: &'buffer mut [u8],
    position: usize,
    error: Error,
}

impl<'buffer> Writer<'buffer> {
    /// Creates a writer over a caller-owned fixed buffer.
    pub fn new(buffer: &'buffer mut [u8]) -> Self {
        Self {
            buffer,
            position: 0,
            error: Error::Ok,
        }
    }

    /// Returns the writer's sticky error.
    pub fn error(&self) -> Error {
        self.error
    }

    /// Returns the number of bytes written into the buffer.
    pub fn used(&self) -> usize {
        self.position
    }

    /// Returns the encoded bytes written so far.
    pub fn written(&self) -> &[u8] {
        &self.buffer[..self.position]
    }

    /// Writes the MessagePack nil marker.
    pub fn write_nil(&mut self) {
        self.write_byte(0xc0);
    }

    /// Writes a MessagePack boolean.
    pub fn write_bool(&mut self, value: bool) {
        self.write_byte(if value { 0xc3 } else { 0xc2 });
    }

    /// Writes an unsigned integer using the smallest MessagePack representation.
    pub fn write_u64(&mut self, value: u64) {
        if value <= 0x7f {
            self.write_header(&[value as u8]);
        } else if value <= u8::MAX as u64 {
            self.write_header(&[0xcc, value as u8]);
        } else if value <= u16::MAX as u64 {
            self.write_header_with_value(0xcd, &(value as u16).to_be_bytes());
        } else if value <= u32::MAX as u64 {
            self.write_header_with_value(0xce, &(value as u32).to_be_bytes());
        } else {
            self.write_header_with_value(0xcf, &value.to_be_bytes());
        }
    }

    /// Alias for [`Writer::write_u64`] matching MPack's generic uint API.
    pub fn write_uint(&mut self, value: u64) {
        self.write_u64(value);
    }

    /// Writes an unsigned 8-bit integer using compact MessagePack encoding.
    pub fn write_u8(&mut self, value: u8) {
        self.write_u64(value as u64);
    }

    /// Writes an unsigned 16-bit integer using compact MessagePack encoding.
    pub fn write_u16(&mut self, value: u16) {
        self.write_u64(value as u64);
    }

    /// Writes an unsigned 32-bit integer using compact MessagePack encoding.
    pub fn write_u32(&mut self, value: u32) {
        self.write_u64(value as u64);
    }

    /// Writes a signed integer using the smallest MessagePack representation.
    pub fn write_i64(&mut self, value: i64) {
        if value >= 0 {
            self.write_u64(value as u64);
        } else if value >= -32 {
            self.write_header(&[value as i8 as u8]);
        } else if value >= i8::MIN as i64 {
            self.write_header_with_value(0xd0, &(value as i8).to_be_bytes());
        } else if value >= i16::MIN as i64 {
            self.write_header_with_value(0xd1, &(value as i16).to_be_bytes());
        } else if value >= i32::MIN as i64 {
            self.write_header_with_value(0xd2, &(value as i32).to_be_bytes());
        } else {
            self.write_header_with_value(0xd3, &value.to_be_bytes());
        }
    }

    /// Alias for [`Writer::write_i64`] matching MPack's generic int API.
    pub fn write_int(&mut self, value: i64) {
        self.write_i64(value);
    }

    /// Writes a signed 8-bit integer using compact MessagePack encoding.
    pub fn write_i8(&mut self, value: i8) {
        self.write_i64(value as i64);
    }

    /// Writes a signed 16-bit integer using compact MessagePack encoding.
    pub fn write_i16(&mut self, value: i16) {
        self.write_i64(value as i64);
    }

    /// Writes a signed 32-bit integer using compact MessagePack encoding.
    pub fn write_i32(&mut self, value: i32) {
        self.write_i64(value as i64);
    }

    /// Writes a float32 marker followed by the supplied IEEE-754 bits.
    pub fn write_f32_bits(&mut self, bits: u32) {
        self.write_header_with_value(0xca, &bits.to_be_bytes());
    }

    /// Writes a float64 marker followed by the supplied IEEE-754 bits.
    pub fn write_f64_bits(&mut self, bits: u64) {
        self.write_header_with_value(0xcb, &bits.to_be_bytes());
    }

    /// Writes a float32 preserving its exact IEEE-754 bit pattern.
    pub fn write_f32(&mut self, value: f32) {
        self.write_f32_bits(value.to_bits());
    }

    /// Writes a float64 preserving its exact IEEE-754 bit pattern.
    pub fn write_f64(&mut self, value: f64) {
        self.write_f64_bits(value.to_bits());
    }

    /// Writes an array header for `length` elements.
    pub fn write_array_header(&mut self, length: usize) {
        if length <= 15 {
            self.write_header(&[0x90 | length as u8]);
        } else if let Ok(length) = u16::try_from(length) {
            self.write_header_with_value(0xdc, &length.to_be_bytes());
        } else if let Ok(length) = u32::try_from(length) {
            self.write_header_with_value(0xdd, &length.to_be_bytes());
        } else {
            self.flag_too_big();
        }
    }

    /// Alias for [`Writer::write_array_header`].
    pub fn write_array(&mut self, length: usize) {
        self.write_array_header(length);
    }

    /// Writes a map header for `length` key-value pairs.
    pub fn write_map_header(&mut self, length: usize) {
        if length <= 15 {
            self.write_header(&[0x80 | length as u8]);
        } else if let Ok(length) = u16::try_from(length) {
            self.write_header_with_value(0xde, &length.to_be_bytes());
        } else if let Ok(length) = u32::try_from(length) {
            self.write_header_with_value(0xdf, &length.to_be_bytes());
        } else {
            self.flag_too_big();
        }
    }

    /// Alias for [`Writer::write_map_header`].
    pub fn write_map(&mut self, length: usize) {
        self.write_map_header(length);
    }

    /// Writes a string header for a byte string of `length` bytes.
    pub fn write_str_header(&mut self, length: usize) {
        if length <= 31 {
            self.write_header(&[0xa0 | length as u8]);
        } else if let Ok(length) = u8::try_from(length) {
            self.write_header(&[0xd9, length]);
        } else if let Ok(length) = u16::try_from(length) {
            self.write_header_with_value(0xda, &length.to_be_bytes());
        } else if let Ok(length) = u32::try_from(length) {
            self.write_header_with_value(0xdb, &length.to_be_bytes());
        } else {
            self.flag_too_big();
        }
    }

    /// Writes a binary header for `length` bytes.
    pub fn write_bin_header(&mut self, length: usize) {
        if let Ok(length) = u8::try_from(length) {
            self.write_header(&[0xc4, length]);
        } else if let Ok(length) = u16::try_from(length) {
            self.write_header_with_value(0xc5, &length.to_be_bytes());
        } else if let Ok(length) = u32::try_from(length) {
            self.write_header_with_value(0xc6, &length.to_be_bytes());
        } else {
            self.flag_too_big();
        }
    }

    /// Writes a string header and its supplied bytes.
    ///
    /// This low-level API does not validate UTF-8.
    pub fn write_str(&mut self, value: &[u8]) {
        self.write_str_header(value.len());
        self.write_bytes(value);
    }

    /// Writes a binary header and its supplied bytes.
    pub fn write_bin(&mut self, value: &[u8]) {
        self.write_bin_header(value.len());
        self.write_bytes(value);
    }

    /// Writes raw payload bytes, stopping at the first byte that does not fit.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_byte(byte);
        }
    }

    fn write_header_with_value(&mut self, marker: u8, value: &[u8]) {
        let mut header = [0_u8; 9];
        header[0] = marker;
        header[1..value.len() + 1].copy_from_slice(value);
        self.write_header(&header[..value.len() + 1]);
    }

    /// Writes a complete header only when all of it fits.
    fn write_header(&mut self, header: &[u8]) {
        if self.error != Error::Ok {
            return;
        }
        if self.buffer.len().saturating_sub(self.position) < header.len() {
            self.error = Error::TooBig;
            return;
        }
        for &byte in header {
            self.write_byte(byte);
        }
    }

    fn flag_too_big(&mut self) {
        if self.error == Error::Ok {
            self.error = Error::TooBig;
        }
    }

    fn write_byte(&mut self, byte: u8) {
        if self.error != Error::Ok {
            return;
        }

        let Some(output) = self.buffer.get_mut(self.position) else {
            self.error = Error::TooBig;
            return;
        };

        *output = byte;
        self.position += 1;
    }
}
