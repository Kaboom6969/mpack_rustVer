//! Incremental / buffered decoder (mirrors `mpack-reader`).

use crate::common::Error;

/// Placeholder reader; decode APIs will land in a later vertical slice.
#[derive(Debug, Default)]
pub struct Reader {
    pub error: Error,
}

impl Reader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error(&self) -> Error {
        self.error
    }
}
