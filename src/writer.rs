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

    /// Writes a MessagePack v4 raw/string header, which never uses `str8`.
    pub(crate) fn write_str_header_v4(&mut self, length: usize) {
        if length <= 31 {
            self.write_header(&[0xa0 | length as u8]);
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

    /// Writes an extension header using the shortest MessagePack encoding.
    pub fn write_ext_header(&mut self, ext_type: i8, length: usize) {
        let ext_type = ext_type as u8;
        match length {
            1 => self.write_header(&[0xd4, ext_type]),
            2 => self.write_header(&[0xd5, ext_type]),
            4 => self.write_header(&[0xd6, ext_type]),
            8 => self.write_header(&[0xd7, ext_type]),
            16 => self.write_header(&[0xd8, ext_type]),
            length if u8::try_from(length).is_ok() => {
                self.write_header(&[0xc7, length as u8, ext_type]);
            }
            length if u16::try_from(length).is_ok() => {
                let length = (length as u16).to_be_bytes();
                self.write_header(&[0xc8, length[0], length[1], ext_type]);
            }
            length if u32::try_from(length).is_ok() => {
                let length = (length as u32).to_be_bytes();
                self.write_header(&[
                    0xc9, length[0], length[1], length[2], length[3], ext_type,
                ]);
            }
            _ => self.flag_too_big(),
        }
    }

    /// Writes an extension header and payload.
    pub fn write_ext(&mut self, ext_type: i8, value: &[u8]) {
        self.write_ext_header(ext_type, value.len());
        self.write_bytes(value);
    }

    /// Writes a MessagePack timestamp extension.
    pub fn write_timestamp(&mut self, seconds: i64, nanoseconds: u32) {
        if nanoseconds > 999_999_999 {
            if self.error == Error::Ok {
                self.error = Error::Bug;
            }
            return;
        }

        if seconds >= 0 && seconds <= u32::MAX as i64 && nanoseconds == 0 {
            self.write_header(&[
                0xd6,
                0xff,
                (seconds >> 24) as u8,
                (seconds >> 16) as u8,
                (seconds >> 8) as u8,
                seconds as u8,
            ]);
        } else if seconds >= 0 && seconds < (1_i64 << 34) {
            let packed = ((nanoseconds as u64) << 34) | seconds as u64;
            let packed = packed.to_be_bytes();
            self.write_header(&[
                0xd7, 0xff, packed[0], packed[1], packed[2], packed[3], packed[4], packed[5],
                packed[6], packed[7],
            ]);
        } else {
            let nanos = nanoseconds.to_be_bytes();
            let seconds = seconds.to_be_bytes();
            self.write_header(&[
                0xc7, 12, 0xff, nanos[0], nanos[1], nanos[2], nanos[3], seconds[0], seconds[1],
                seconds[2], seconds[3], seconds[4], seconds[5], seconds[6], seconds[7],
            ]);
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
        if self.error != Error::Ok || bytes.is_empty() {
            return;
        }
        let available = self.buffer.len().saturating_sub(self.position);
        let n = bytes.len().min(available);
        if n > 0 {
            self.buffer[self.position..self.position + n].copy_from_slice(&bytes[..n]);
            self.position += n;
        }
        if n < bytes.len() {
            self.error = Error::TooBig;
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
        self.buffer[self.position..self.position + header.len()].copy_from_slice(header);
        self.position += header.len();
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

/// An allocator-backed writer that grows without exposing Rust allocations
/// through the C ABI.
#[derive(Debug, Default)]
pub struct GrowableWriter {
    bytes: Vec<u8>,
    error: Error,
}

impl GrowableWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
            error: Error::Ok,
        }
    }

    pub fn error(&self) -> Error {
        self.error
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_vec(self) -> Result<Vec<u8>, Error> {
        if self.error == Error::Ok {
            Ok(self.bytes)
        } else {
            Err(self.error)
        }
    }

    pub fn write_nil(&mut self) {
        self.encode(|writer| writer.write_nil());
    }

    pub fn write_bool(&mut self, value: bool) {
        self.encode(|writer| writer.write_bool(value));
    }

    pub fn write_u64(&mut self, value: u64) {
        self.encode(|writer| writer.write_u64(value));
    }

    pub fn write_i64(&mut self, value: i64) {
        self.encode(|writer| writer.write_i64(value));
    }

    pub fn write_array_header(&mut self, count: usize) {
        self.encode(|writer| writer.write_array_header(count));
    }

    pub fn write_map_header(&mut self, count: usize) {
        self.encode(|writer| writer.write_map_header(count));
    }

    pub fn write_str(&mut self, value: &[u8]) {
        self.encode(|writer| writer.write_str_header(value.len()));
        self.append(value);
    }

    pub fn write_bin(&mut self, value: &[u8]) {
        self.encode(|writer| writer.write_bin_header(value.len()));
        self.append(value);
    }

    pub fn write_ext(&mut self, ext_type: i8, value: &[u8]) {
        self.encode(|writer| writer.write_ext_header(ext_type, value.len()));
        self.append(value);
    }

    pub fn write_timestamp(&mut self, seconds: i64, nanoseconds: u32) {
        self.encode(|writer| writer.write_timestamp(seconds, nanoseconds));
    }

    fn append(&mut self, bytes: &[u8]) {
        if self.error == Error::Ok {
            self.bytes.extend_from_slice(bytes);
        }
    }

    fn encode(&mut self, encode: impl FnOnce(&mut Writer<'_>)) {
        if self.error != Error::Ok {
            return;
        }
        let mut scratch = [0_u8; 16];
        let mut writer = Writer::new(&mut scratch);
        encode(&mut writer);
        self.error = writer.error();
        self.bytes.extend_from_slice(writer.written());
    }
}

/// Compound write tracker used by checked writers and the FFI sidecar.
#[derive(Debug, Default)]
pub struct WriteTracker {
    stack: Vec<TrackElement>,
}

#[derive(Debug)]
struct TrackElement {
    kind: TrackKind,
    left: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Array,
    Map,
    Str,
    Bin,
    Ext,
}

impl WriteTracker {
    pub fn push(&mut self, kind: TrackKind, count: usize) {
        let left = if kind == TrackKind::Map {
            count.saturating_mul(2)
        } else {
            count
        };
        self.stack.push(TrackElement { kind, left });
    }

    pub fn element(&mut self) -> Result<(), Error> {
        let Some(top) = self.stack.last_mut() else {
            return Ok(());
        };
        if matches!(top.kind, TrackKind::Str | TrackKind::Bin | TrackKind::Ext) || top.left == 0 {
            return Err(Error::Bug);
        }
        top.left -= 1;
        Ok(())
    }

    pub fn bytes(&mut self, count: usize) -> Result<(), Error> {
        let Some(top) = self.stack.last_mut() else {
            return Err(Error::Bug);
        };
        if !matches!(top.kind, TrackKind::Str | TrackKind::Bin | TrackKind::Ext) || count > top.left {
            return Err(Error::Bug);
        }
        top.left -= count;
        Ok(())
    }

    pub fn pop(&mut self, kind: TrackKind) -> Result<(), Error> {
        match self.stack.last() {
            Some(top) if top.kind == kind && top.left == 0 => {
                self.stack.pop();
                Ok(())
            }
            _ => Err(Error::Bug),
        }
    }

    pub fn finish(self) -> Result<(), Error> {
        if self.stack.is_empty() {
            Ok(())
        } else {
            Err(Error::Bug)
        }
    }
}

/// Automatic-size map/array builder.
#[derive(Debug, Default)]
pub struct Builder {
    frames: Vec<BuildFrame>,
    output: GrowableWriter,
}

#[derive(Debug)]
struct BuildFrame {
    kind: TrackKind,
    elements: usize,
    bytes: Vec<u8>,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build_array(&mut self) {
        self.frames.push(BuildFrame {
            kind: TrackKind::Array,
            elements: 0,
            bytes: Vec::new(),
        });
    }

    pub fn build_map(&mut self) {
        self.frames.push(BuildFrame {
            kind: TrackKind::Map,
            elements: 0,
            bytes: Vec::new(),
        });
    }

    pub fn write_nil(&mut self) {
        self.write_value(|writer| writer.write_nil());
    }

    pub fn write_bool(&mut self, value: bool) {
        self.write_value(|writer| writer.write_bool(value));
    }

    pub fn write_u64(&mut self, value: u64) {
        self.write_value(|writer| writer.write_u64(value));
    }

    pub fn write_i64(&mut self, value: i64) {
        self.write_value(|writer| writer.write_i64(value));
    }

    pub fn write_str(&mut self, value: &[u8]) {
        let mut encoded = GrowableWriter::new();
        encoded.write_str(value);
        self.append_value(encoded.as_slice());
    }

    pub fn complete_array(&mut self) -> Result<(), Error> {
        self.complete(TrackKind::Array)
    }

    pub fn complete_map(&mut self) -> Result<(), Error> {
        self.complete(TrackKind::Map)
    }

    pub fn finish(self) -> Result<Vec<u8>, Error> {
        if self.frames.is_empty() {
            self.output.into_vec()
        } else {
            Err(Error::Bug)
        }
    }

    fn write_value(&mut self, write: impl FnOnce(&mut GrowableWriter)) {
        let mut encoded = GrowableWriter::new();
        write(&mut encoded);
        self.append_value(encoded.as_slice());
    }

    fn append_value(&mut self, bytes: &[u8]) {
        if let Some(frame) = self.frames.last_mut() {
            frame.bytes.extend_from_slice(bytes);
            frame.elements += 1;
        } else {
            self.output.append(bytes);
        }
    }

    fn complete(&mut self, kind: TrackKind) -> Result<(), Error> {
        let Some(frame) = self.frames.pop() else {
            return Err(Error::Bug);
        };
        if frame.kind != kind || (kind == TrackKind::Map && frame.elements % 2 != 0) {
            return Err(Error::Bug);
        }
        let count = if kind == TrackKind::Map {
            frame.elements / 2
        } else {
            frame.elements
        };
        let mut encoded = GrowableWriter::new();
        if kind == TrackKind::Map {
            encoded.write_map_header(count);
        } else {
            encoded.write_array_header(count);
        }
        encoded.append(&frame.bytes);
        self.append_value(encoded.as_slice());
        Ok(())
    }
}
