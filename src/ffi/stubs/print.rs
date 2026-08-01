//! Temporary scaffolding; replace body with safe-core calls, do not grow unsafe here.

use std::ffi::c_char;
use std::ptr;

use crate::ffi::types::MpackPrint;

#[no_mangle]
pub unsafe extern "C" fn mpack_print_append(
    print: *mut MpackPrint,
    data: *const c_char,
    count: usize,
) {
    if print.is_null() || data.is_null() || count == 0 {
        return;
    }
    let state = unsafe { &mut *print };
    if state.buffer.is_null() || state.size == 0 {
        if let Some(callback) = state.callback {
            unsafe { callback(state.context, data, count) };
        }
        return;
    }
    let available = state.size.saturating_sub(state.count);
    let copy = count.min(available);
    if copy > 0 {
        unsafe {
            ptr::copy_nonoverlapping(
                data.cast::<u8>(),
                state.buffer.add(state.count).cast::<u8>(),
                copy,
            );
        }
        state.count += copy;
    }
}
