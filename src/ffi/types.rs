//! C ABI types for embed-writer (default) or full-suite configurations.

use std::ffi::{c_char, c_int, c_void};

use crate::common::Error;

pub type MpackError = c_int;

pub const MPACK_OK: MpackError = 0;
pub const MPACK_ERROR_IO: MpackError = 2;
pub const MPACK_ERROR_INVALID: MpackError = 3;
pub const MPACK_ERROR_UNSUPPORTED: MpackError = 4;
pub const MPACK_ERROR_TYPE: MpackError = 5;
pub const MPACK_ERROR_TOO_BIG: MpackError = 6;
pub const MPACK_ERROR_MEMORY: MpackError = 7;
pub const MPACK_ERROR_BUG: MpackError = 8;
pub const MPACK_ERROR_DATA: MpackError = 9;
pub const MPACK_ERROR_EOF: MpackError = 10;

/// Upstream `mpack_version_current` (`mpack_version_v5`).
#[cfg(feature = "full-suite-abi")]
pub const MPACK_VERSION_CURRENT: c_int = 5;

/// C `mpack_tag_t`. With `full-suite-abi`, matches `MPACK_EXTENSIONS=1`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MpackTag {
    pub type_: c_int,
    #[cfg(feature = "full-suite-abi")]
    pub exttype: i8,
    #[cfg(feature = "full-suite-abi")]
    pub _pad: [u8; 3],
    pub value: u64,
}

impl MpackTag {
    pub const fn zero() -> Self {
        Self {
            type_: 0,
            #[cfg(feature = "full-suite-abi")]
            exttype: 0,
            #[cfg(feature = "full-suite-abi")]
            _pad: [0; 3],
            value: 0,
        }
    }

    pub const fn nil() -> Self {
        Self {
            type_: 1,
            #[cfg(feature = "full-suite-abi")]
            exttype: 0,
            #[cfg(feature = "full-suite-abi")]
            _pad: [0; 3],
            value: 0,
        }
    }
}

#[cfg(feature = "full-suite-abi")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MpackTrackElement {
    pub type_: c_int,
    pub left: u32,
    pub key_needs_value: bool,
    pub builder: bool,
}

#[cfg(feature = "full-suite-abi")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MpackTrack {
    pub count: usize,
    pub capacity: usize,
    pub elements: *mut MpackTrackElement,
}

#[cfg(feature = "full-suite-abi")]
impl MpackTrack {
    pub const fn empty() -> Self {
        Self {
            count: 0,
            capacity: 0,
            elements: std::ptr::null_mut(),
        }
    }
}

#[cfg(feature = "full-suite-abi")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MpackBuilder {
    pub current_build: *mut c_void,
    pub latest_build: *mut c_void,
    pub current_page: *mut c_void,
    pub pages: *mut c_void,
    pub stash_buffer: *mut c_char,
    pub stash_position: *mut c_char,
    pub stash_end: *mut c_char,
}

#[cfg(feature = "full-suite-abi")]
impl MpackBuilder {
    pub const fn empty() -> Self {
        Self {
            current_build: std::ptr::null_mut(),
            latest_build: std::ptr::null_mut(),
            current_page: std::ptr::null_mut(),
            pages: std::ptr::null_mut(),
            stash_buffer: std::ptr::null_mut(),
            stash_position: std::ptr::null_mut(),
            stash_end: std::ptr::null_mut(),
        }
    }
}

pub type MpackWriterFlush = Option<unsafe extern "C" fn(*mut MpackWriter, *const c_char, usize)>;
pub type MpackWriterError = Option<unsafe extern "C" fn(*mut MpackWriter, MpackError)>;
pub type MpackWriterTeardown = Option<unsafe extern "C" fn(*mut MpackWriter)>;

/// `mpack_writer_t` layout selected by Cargo features.
#[repr(C)]
pub struct MpackWriter {
    #[cfg(feature = "full-suite-abi")]
    pub version: c_int,
    pub flush: MpackWriterFlush,
    pub error_fn: MpackWriterError,
    pub teardown: MpackWriterTeardown,
    pub context: *mut c_void,
    pub buffer: *mut c_char,
    pub position: *mut c_char,
    pub end: *mut c_char,
    pub error: MpackError,
    #[cfg(feature = "full-suite-abi")]
    pub track: MpackTrack,
    #[cfg(feature = "full-suite-abi")]
    pub reserved: [*mut c_void; 2],
    #[cfg(feature = "full-suite-abi")]
    pub builder: MpackBuilder,
}

impl MpackWriter {
    pub(crate) fn fixed_buffer(buffer: *mut c_char, size: usize) -> Self {
        let (end, error) = if buffer.is_null() {
            (buffer, MPACK_ERROR_BUG)
        } else {
            (buffer.wrapping_add(size), MPACK_OK)
        };

        Self {
            #[cfg(feature = "full-suite-abi")]
            version: MPACK_VERSION_CURRENT,
            flush: None,
            error_fn: None,
            teardown: None,
            context: std::ptr::null_mut(),
            buffer,
            position: buffer,
            end,
            error,
            #[cfg(feature = "full-suite-abi")]
            track: MpackTrack::empty(),
            #[cfg(feature = "full-suite-abi")]
            reserved: [std::ptr::null_mut(); 2],
            #[cfg(feature = "full-suite-abi")]
            builder: MpackBuilder::empty(),
        }
    }

    pub(crate) fn error_state(error: MpackError) -> Self {
        Self {
            #[cfg(feature = "full-suite-abi")]
            version: MPACK_VERSION_CURRENT,
            flush: None,
            error_fn: None,
            teardown: None,
            context: std::ptr::null_mut(),
            buffer: std::ptr::null_mut(),
            position: std::ptr::null_mut(),
            end: std::ptr::null_mut(),
            error,
            #[cfg(feature = "full-suite-abi")]
            track: MpackTrack::empty(),
            #[cfg(feature = "full-suite-abi")]
            reserved: [std::ptr::null_mut(); 2],
            #[cfg(feature = "full-suite-abi")]
            builder: MpackBuilder::empty(),
        }
    }
}

#[cfg(feature = "full-suite-abi")]
pub type MpackReaderFill = Option<unsafe extern "C" fn(*mut MpackReader, *mut c_char, usize) -> usize>;
#[cfg(feature = "full-suite-abi")]
pub type MpackReaderSkip = Option<unsafe extern "C" fn(*mut MpackReader, usize)>;
#[cfg(feature = "full-suite-abi")]
pub type MpackReaderError = Option<unsafe extern "C" fn(*mut MpackReader, MpackError)>;
#[cfg(feature = "full-suite-abi")]
pub type MpackReaderTeardown = Option<unsafe extern "C" fn(*mut MpackReader)>;

#[cfg(feature = "full-suite-abi")]
#[repr(C)]
pub struct MpackReader {
    pub context: *mut c_void,
    pub fill: MpackReaderFill,
    pub error_fn: MpackReaderError,
    pub teardown: MpackReaderTeardown,
    pub skip: MpackReaderSkip,
    pub buffer: *mut c_char,
    pub size: usize,
    pub data: *const c_char,
    pub end: *const c_char,
    pub error: MpackError,
    pub track: MpackTrack,
}

#[cfg(feature = "full-suite-abi")]
impl MpackReader {
    pub(crate) fn unsupported() -> Self {
        Self {
            context: std::ptr::null_mut(),
            fill: None,
            error_fn: None,
            teardown: None,
            skip: None,
            buffer: std::ptr::null_mut(),
            size: 0,
            data: std::ptr::null(),
            end: std::ptr::null(),
            error: MPACK_ERROR_UNSUPPORTED,
            track: MpackTrack::empty(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn error_state(error: MpackError) -> Self {
        let mut reader = Self::unsupported();
        reader.error = error;
        reader
    }
}

#[cfg(feature = "full-suite-abi")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MpackNodeData {
    pub type_: c_int,
    pub len: u32,
    pub value: u64,
}

#[cfg(feature = "full-suite-abi")]
impl MpackNodeData {
    pub const fn nil() -> Self {
        Self {
            type_: 1,
            len: 0,
            value: 0,
        }
    }

    pub const fn missing() -> Self {
        Self {
            type_: 0,
            len: 0,
            value: 0,
        }
    }
}

#[cfg(feature = "full-suite-abi")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MpackNode {
    pub data: *mut MpackNodeData,
    pub tree: *mut MpackTree,
}

#[cfg(feature = "full-suite-abi")]
impl MpackNode {
    pub const fn null() -> Self {
        Self {
            data: std::ptr::null_mut(),
            tree: std::ptr::null_mut(),
        }
    }
}

#[cfg(feature = "full-suite-abi")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MpackLevel {
    pub child: *mut MpackNodeData,
    pub left: usize,
}

#[cfg(feature = "full-suite-abi")]
#[repr(C)]
pub struct MpackTreeParser {
    pub state: c_int,
    pub possible_nodes_left: usize,
    pub nodes: *mut MpackNodeData,
    pub nodes_left: usize,
    pub current_node_reserved: usize,
    pub level: usize,
    pub stack: *mut MpackLevel,
    pub stack_capacity: usize,
    pub stack_owned: bool,
    pub stack_local: [MpackLevel; 3],
}

#[cfg(feature = "full-suite-abi")]
impl MpackTreeParser {
    pub const fn empty() -> Self {
        Self {
            state: 0,
            possible_nodes_left: 0,
            nodes: std::ptr::null_mut(),
            nodes_left: 0,
            current_node_reserved: 0,
            level: 0,
            stack: std::ptr::null_mut(),
            stack_capacity: 0,
            stack_owned: false,
            stack_local: [MpackLevel {
                child: std::ptr::null_mut(),
                left: 0,
            }; 3],
        }
    }
}

#[cfg(feature = "full-suite-abi")]
pub type MpackTreeError = Option<unsafe extern "C" fn(*mut MpackTree, MpackError)>;
#[cfg(feature = "full-suite-abi")]
pub type MpackTreeRead = Option<unsafe extern "C" fn(*mut MpackTree, *mut c_char, usize) -> usize>;
#[cfg(feature = "full-suite-abi")]
pub type MpackTreeTeardown = Option<unsafe extern "C" fn(*mut MpackTree)>;

#[cfg(feature = "full-suite-abi")]
#[repr(C)]
pub struct MpackTree {
    pub error_fn: MpackTreeError,
    pub read_fn: MpackTreeRead,
    pub teardown: MpackTreeTeardown,
    pub context: *mut c_void,
    pub nil_node: MpackNodeData,
    pub missing_node: MpackNodeData,
    pub error: MpackError,
    pub buffer: *mut c_char,
    pub buffer_capacity: usize,
    pub data: *const c_char,
    pub data_length: usize,
    pub size: usize,
    pub node_count: usize,
    pub max_size: usize,
    pub max_nodes: usize,
    pub parser: MpackTreeParser,
    pub root: *mut MpackNodeData,
    pub pool: *mut MpackNodeData,
    pub pool_count: usize,
    pub next: *mut c_void,
}

#[cfg(feature = "full-suite-abi")]
impl MpackTree {
    #[allow(dead_code)] // mirrors reader helper; not yet used on tree error paths
    pub(crate) fn unsupported() -> Self {
        Self {
            error_fn: None,
            read_fn: None,
            teardown: None,
            context: std::ptr::null_mut(),
            nil_node: MpackNodeData::nil(),
            missing_node: MpackNodeData::missing(),
            error: MPACK_ERROR_UNSUPPORTED,
            buffer: std::ptr::null_mut(),
            buffer_capacity: 0,
            data: std::ptr::null(),
            data_length: 0,
            size: 0,
            node_count: 0,
            max_size: 0,
            max_nodes: 0,
            parser: MpackTreeParser::empty(),
            root: std::ptr::null_mut(),
            pool: std::ptr::null_mut(),
            pool_count: 0,
            next: std::ptr::null_mut(),
        }
    }
}

#[cfg(feature = "full-suite-abi")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MpackTimestamp {
    pub seconds: i64,
    pub nanoseconds: u32,
}

#[cfg(feature = "full-suite-abi")]
#[repr(C)]
pub struct MpackPrint {
    pub buffer: *mut c_char,
    pub size: usize,
    pub count: usize,
    pub callback: Option<unsafe extern "C" fn(*mut c_void, *const c_char, usize)>,
    pub context: *mut c_void,
}

pub(crate) fn core_error_to_abi(error: Error) -> MpackError {
    match error {
        Error::Ok => MPACK_OK,
        Error::Io => MPACK_ERROR_IO,
        Error::Invalid => MPACK_ERROR_INVALID,
        Error::Unsupported => MPACK_ERROR_UNSUPPORTED,
        Error::Type => MPACK_ERROR_TYPE,
        Error::TooBig => MPACK_ERROR_TOO_BIG,
        Error::Memory => MPACK_ERROR_MEMORY,
        Error::Bug => MPACK_ERROR_BUG,
        Error::Data => MPACK_ERROR_DATA,
        Error::Eof => MPACK_ERROR_EOF,
    }
}
