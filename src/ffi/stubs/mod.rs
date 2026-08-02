//! Track + print helpers for full-suite-abi.
//!
//! Soft-continue stub helpers (`util`) were removed; do not reintroduce
//! unsupported/nil/`stub_bytes` happy paths for frozen-link parity.

pub(crate) mod track;

pub mod print;
