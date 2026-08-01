//! Tree / DOM parse + typed accessors (mirrors `mpack-node`).

use crate::common::Error;

/// Placeholder tree; node APIs will land after reader/expect.
#[derive(Debug, Default)]
pub struct Tree {
    pub error: Error,
}

impl Tree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error(&self) -> Error {
        self.error
    }
}
