//! Smoke tests for the Rust port scaffold (not the frozen C suite).

use mpack::{common::Error, writer::Writer, VERSION};

#[test]
fn crate_version_is_nonempty() {
    assert!(!VERSION.is_empty());
}

#[test]
fn writer_starts_ok() {
    let w = Writer::new();
    assert_eq!(w.error(), Error::Ok);
}
