//! Temporary scaffolding; replace body with safe-core calls, do not grow unsafe here.

use crate::ffi::types::{MpackError, MpackTrack, MPACK_OK};

#[no_mangle]
pub unsafe extern "C" fn mpack_track_bytes(
    _track: *mut MpackTrack,
    _read: bool,
    _count: usize,
) -> MpackError {
    MPACK_OK
}

#[no_mangle]
pub unsafe extern "C" fn mpack_track_element(_track: *mut MpackTrack, _read: bool) -> MpackError {
    MPACK_OK
}

#[no_mangle]
pub unsafe extern "C" fn mpack_track_peek_element(
    _track: *mut MpackTrack,
    _read: bool,
) -> MpackError {
    MPACK_OK
}

#[no_mangle]
pub unsafe extern "C" fn mpack_track_str_bytes_all(
    _track: *mut MpackTrack,
    _read: bool,
    _count: usize,
) -> MpackError {
    MPACK_OK
}
