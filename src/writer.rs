//! Incremental encoder / growable buffer writer (mirrors `mpack-writer`).

use crate::common::Error;

/// Placeholder writer; encode APIs will land in a later vertical slice.
#[derive(Debug, Default)]
pub struct Writer {
    pub error: Error,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error(&self) -> Error {
        self.error
    }
}
