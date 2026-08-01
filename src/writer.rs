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
