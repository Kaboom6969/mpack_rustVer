//! Schema-typed reads on top of the reader (mirrors `mpack-expect`).

use crate::common::Error;
use crate::reader::Reader;

/// Expect helpers will wrap [`Reader`] once decode is implemented.
pub fn expect_ok(reader: &Reader) -> Error {
    reader.error()
}
