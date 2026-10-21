//! `post`, `error`, `object_post`, `object_warn`, `object_error`.
//!
//! The variadic entry points are in `c/variadic.c`; they format the message and call
//! [`median_mock_console`], which appends to the console log.

use crate::state::{ConsoleEntry, Level, state};
use std::ffi::{CStr, c_char, c_int, c_void};

/// # Safety
/// `text` must be a NUL-terminated string. Called by the C side only.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn median_mock_console(level: c_int, obj: *mut c_void, text: *const c_char) {
    let level = match level {
        0 => Level::Post,
        1 => Level::Warn,
        _ => Level::Error,
    };
    let text = if text.is_null() {
        String::new()
    } else {
        // SAFETY: the caller's contract
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    };
    state().console.push(ConsoleEntry {
        level,
        object: obj as usize,
        text,
    });
}
