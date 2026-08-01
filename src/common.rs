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

/// MessagePack value category.
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

/// A decoded MessagePack tag.
///
/// String, binary, array, map, and extension variants contain the payload or
/// element count from the encoded header. Extension payload bytes follow the
/// tag in the reader just like string and binary payload bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tag {
    Nil,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f32),
    Double(f64),
    Str(u32),
    Bin(u32),
    Array(u32),
    Map(u32),
    Ext { extension_type: i8, length: u32 },
}

impl Tag {
    /// Returns the category of this tag.
    pub const fn kind(self) -> Type {
        match self {
            Self::Nil => Type::Nil,
            Self::Bool(_) => Type::Bool,
            Self::Int(_) => Type::Int,
            Self::Uint(_) => Type::Uint,
            Self::Float(_) => Type::Float,
            Self::Double(_) => Type::Double,
            Self::Str(_) => Type::Str,
            Self::Bin(_) => Type::Bin,
            Self::Array(_) => Type::Array,
            Self::Map(_) => Type::Map,
            Self::Ext { .. } => Type::Ext,
        }
    }
}
