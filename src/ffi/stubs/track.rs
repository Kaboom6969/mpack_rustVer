//! Read/write track stack matching C `mpack-common` tracking.

use std::ffi::{c_char, c_int, c_void};
use std::ptr;

use crate::ffi::types::{
    MpackError, MpackTrack, MpackTrackElement, MPACK_ERROR_BUG, MPACK_ERROR_MEMORY, MPACK_OK,
};

const TRACKING_INITIAL_CAPACITY: usize = 8;

const TYPE_STR: c_int = 7;
const TYPE_BIN: c_int = 8;
const TYPE_ARRAY: c_int = 9;
const TYPE_MAP: c_int = 10;
const TYPE_EXT: c_int = 11;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn realloc(pointer: *mut c_void, size: usize) -> *mut c_void;
    fn free(pointer: *mut c_void);
    /// Provided by the frozen suite under `MPACK_CUSTOM_ASSERT` / debug builds.
    fn mpack_break_hit(message: *const c_char);
}

fn break_hit(message: &[u8]) {
    // SAFETY: Suite (or test shim) provides `mpack_break_hit`; message is a
    // NUL-terminated static or local C string.
    unsafe {
        mpack_break_hit(message.as_ptr().cast());
    }
}

/// Initializes an empty growable track stack (C `mpack_track_init`).
pub(crate) fn track_init(track: &mut MpackTrack) -> MpackError {
    track.count = 0;
    track.capacity = TRACKING_INITIAL_CAPACITY;
    let bytes = TRACKING_INITIAL_CAPACITY.saturating_mul(std::mem::size_of::<MpackTrackElement>());
    // SAFETY: libc malloc returns aligned storage or null.
    let elements = unsafe { malloc(bytes.max(1)) }.cast::<MpackTrackElement>();
    if elements.is_null() {
        track.capacity = 0;
        track.elements = ptr::null_mut();
        return MPACK_ERROR_MEMORY;
    }
    track.elements = elements;
    MPACK_OK
}

fn track_grow(track: &mut MpackTrack) -> MpackError {
    let new_capacity = track.capacity.saturating_mul(2).max(TRACKING_INITIAL_CAPACITY);
    let new_bytes = new_capacity.saturating_mul(std::mem::size_of::<MpackTrackElement>());
    // SAFETY: `elements` came from malloc/realloc; size is the new capacity.
    let new_elements =
        unsafe { realloc(track.elements.cast(), new_bytes.max(1)) }.cast::<MpackTrackElement>();
    if new_elements.is_null() {
        return MPACK_ERROR_MEMORY;
    }
    track.elements = new_elements;
    track.capacity = new_capacity;
    MPACK_OK
}

/// Pushes a typed element/byte count onto the track (C `mpack_track_push`).
pub(crate) fn track_push(track: &mut MpackTrack, type_: c_int, count: u32) -> MpackError {
    track_push_impl(track, type_, count, false)
}

/// Pushes a builder-owned compound entry.
pub(crate) fn track_push_builder(track: &mut MpackTrack, type_: c_int) -> MpackError {
    track_push_impl(track, type_, 0, true)
}

fn track_push_impl(
    track: &mut MpackTrack,
    type_: c_int,
    count: u32,
    builder: bool,
) -> MpackError {
    if track.elements.is_null() {
        return MPACK_ERROR_BUG;
    }
    if track.count == track.capacity {
        let error = track_grow(track);
        if error != MPACK_OK {
            return error;
        }
    }
    // SAFETY: count < capacity and elements is a live allocation.
    unsafe {
        *track.elements.add(track.count) = MpackTrackElement {
            type_,
            left: count,
            key_needs_value: false,
            builder,
        };
    }
    track.count += 1;
    MPACK_OK
}

fn track_pop_impl(track: &mut MpackTrack, type_: c_int, builder: bool) -> MpackError {
    if track.elements.is_null() {
        return MPACK_ERROR_BUG;
    }
    if track.count == 0 {
        break_hit(b"attempting to close a type but nothing was opened!\0");
        return MPACK_ERROR_BUG;
    }
    // SAFETY: count > 0 and elements is live.
    let element = unsafe { &mut *track.elements.add(track.count - 1) };
    if element.type_ != type_ {
        break_hit(b"attempting to close a type but the open element differs!\0");
        return MPACK_ERROR_BUG;
    }
    if element.key_needs_value {
        break_hit(b"attempting to close a map with an odd number of elements\0");
        return MPACK_ERROR_BUG;
    }
    if element.left != 0 {
        break_hit(b"attempting to close a type but elements/bytes remain\0");
        return MPACK_ERROR_BUG;
    }
    if element.builder != builder {
        break_hit(b"attempting to pop builder/non-builder mismatch\0");
        return MPACK_ERROR_BUG;
    }
    track.count -= 1;
    MPACK_OK
}

/// Pops a non-builder track entry (C `mpack_track_pop`).
pub(crate) fn track_pop(track: &mut MpackTrack, type_: c_int) -> MpackError {
    track_pop_impl(track, type_, false)
}

/// Pops a builder-owned compound entry.
pub(crate) fn track_pop_builder(track: &mut MpackTrack, type_: c_int) -> MpackError {
    track_pop_impl(track, type_, true)
}

fn track_peek_element_impl(track: &MpackTrack, read: bool) -> MpackError {
    let _ = read;
    if track.elements.is_null() {
        return MPACK_ERROR_BUG;
    }
    if track.count == 0 {
        return MPACK_OK;
    }
    // SAFETY: count > 0 and elements is live.
    let element = unsafe { &*track.elements.add(track.count - 1) };
    if element.type_ != TYPE_MAP && element.type_ != TYPE_ARRAY {
        break_hit(b"elements cannot be read/written within a str/bin/ext\0");
        return MPACK_ERROR_BUG;
    }
    if !element.builder && element.left == 0 && !element.key_needs_value {
        break_hit(b"too many elements read/written for map/array\0");
        return MPACK_ERROR_BUG;
    }
    MPACK_OK
}

fn track_element_impl(track: &mut MpackTrack, read: bool) -> MpackError {
    let error = track_peek_element_impl(track, read);
    if track.count == 0 || error != MPACK_OK {
        return error;
    }
    // SAFETY: count > 0 after peek succeeded with non-empty track.
    let element = unsafe { &mut *track.elements.add(track.count - 1) };
    if element.type_ == TYPE_MAP {
        if !element.key_needs_value {
            element.key_needs_value = true;
            return MPACK_OK;
        }
        element.key_needs_value = false;
    }
    if !element.builder {
        element.left = element.left.saturating_sub(1);
    }
    MPACK_OK
}

fn track_bytes_impl(track: &mut MpackTrack, read: bool, count: usize) -> MpackError {
    let _ = read;
    if track.elements.is_null() {
        return MPACK_ERROR_BUG;
    }
    if count > u32::MAX as usize {
        break_hit(b"reading more bytes than could possibly fit in a str/bin/ext!\0");
        return MPACK_ERROR_BUG;
    }
    if track.count == 0 {
        break_hit(b"bytes cannot be read with no open bin, str or ext\0");
        return MPACK_ERROR_BUG;
    }
    let element = unsafe { &mut *track.elements.add(track.count - 1) };
    if element.type_ == TYPE_MAP || element.type_ == TYPE_ARRAY {
        break_hit(b"bytes cannot be read within a map/array\0");
        return MPACK_ERROR_BUG;
    }
    if (element.left as usize) < count {
        break_hit(b"too many bytes read for str/bin/ext\0");
        return MPACK_ERROR_BUG;
    }
    element.left -= count as u32;
    MPACK_OK
}

fn track_str_bytes_all_impl(track: &mut MpackTrack, read: bool, count: usize) -> MpackError {
    let error = track_bytes_impl(track, read, count);
    if error != MPACK_OK {
        return error;
    }
    let element = unsafe { &*track.elements.add(track.count - 1) };
    if element.type_ != TYPE_STR {
        break_hit(b"the open type must be a string\0");
        return MPACK_ERROR_BUG;
    }
    if element.left != 0 {
        break_hit(b"not all bytes were read for a string read\0");
        return MPACK_ERROR_BUG;
    }
    MPACK_OK
}

/// Returns bug if any track entries remain (C `mpack_track_check_empty`).
pub(crate) fn track_check_empty(track: &MpackTrack) -> MpackError {
    if track.count != 0 {
        break_hit(b"unclosed type on reader destroy\0");
        return MPACK_ERROR_BUG;
    }
    MPACK_OK
}

/// Frees track storage; optionally checks emptiness (C `mpack_track_destroy`).
pub(crate) fn track_destroy(track: &mut MpackTrack, cancel: bool) -> MpackError {
    let error = if cancel {
        MPACK_OK
    } else {
        track_check_empty(track)
    };
    if !track.elements.is_null() {
        // SAFETY: elements came from malloc/realloc in this module.
        unsafe { free(track.elements.cast()) };
        track.elements = ptr::null_mut();
    }
    track.count = 0;
    track.capacity = 0;
    error
}

/// Safe wrapper around track element decrement (for Rust callers).
pub(crate) fn track_element(track: &mut MpackTrack) -> MpackError {
    track_element_impl(track, true)
}

/// Safe wrapper around track peek element (for Rust callers).
pub(crate) fn track_peek_element(track: &MpackTrack) -> MpackError {
    track_peek_element_impl(track, true)
}

/// Safe wrapper around track bytes (for Rust callers).
pub(crate) fn track_bytes(track: &mut MpackTrack, count: usize) -> MpackError {
    track_bytes_impl(track, true, count)
}

/// Safe wrapper around track_str_bytes_all (for Rust callers).
pub(crate) fn track_str_bytes_all(track: &mut MpackTrack, count: usize) -> MpackError {
    track_str_bytes_all_impl(track, true, count)
}

/// Track-push helper for compound/str/bin/ext tags after a successful read.
pub(crate) fn track_push_for_tag(
    track: &mut MpackTrack,
    type_: c_int,
    count: u32,
) -> MpackError {
    match type_ {
        TYPE_MAP | TYPE_ARRAY | TYPE_STR | TYPE_BIN | TYPE_EXT => track_push(track, type_, count),
        _ => MPACK_OK,
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_track_bytes(
    track: *mut MpackTrack,
    read: bool,
    count: usize,
) -> MpackError {
    if track.is_null() {
        return MPACK_ERROR_BUG;
    }
    // SAFETY: Non-null track is live writable storage from a reader/writer.
    track_bytes_impl(unsafe { &mut *track }, read, count)
}

#[no_mangle]
pub unsafe extern "C" fn mpack_track_element(track: *mut MpackTrack, read: bool) -> MpackError {
    if track.is_null() {
        return MPACK_ERROR_BUG;
    }
    track_element_impl(unsafe { &mut *track }, read)
}

#[no_mangle]
pub unsafe extern "C" fn mpack_track_peek_element(
    track: *mut MpackTrack,
    read: bool,
) -> MpackError {
    if track.is_null() {
        return MPACK_ERROR_BUG;
    }
    track_peek_element_impl(unsafe { &*track }, read)
}

#[no_mangle]
pub unsafe extern "C" fn mpack_track_str_bytes_all(
    track: *mut MpackTrack,
    read: bool,
    count: usize,
) -> MpackError {
    if track.is_null() {
        return MPACK_ERROR_BUG;
    }
    track_str_bytes_all_impl(unsafe { &mut *track }, read, count)
}
