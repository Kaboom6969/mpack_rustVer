//! Shared tags, errors, and platform-facing types (mirrors `mpack-common` / platform).

/// Sticky error codes (will match `mpack_error_t` when FFI lands).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Error {
    #[default]
    Ok,
    Io,
    Invalid,
    Unsupported,
    Type,
    TooBig,
    Memory,
    Bug,
    Data,
    Eof,
}

/// MessagePack type tag (scaffold placeholder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Nil,
    Bool,
    Int,
    Uint,
    Float,
    Double,
    Str,
    Bin,
    Array,
    Map,
    Ext,
}
