use std::convert::Into;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// The printf format used to pass user text to the console; user text is never the format.
const FMT_S: *const c_char = c"%s".as_ptr();
const CSTRING_ERR: &CStr = c"failed to create CString";

/// Get a reference to the common symbols table
pub fn common_symbols() -> &'static max_sys::_common_symbols_table {
    unsafe {
        let t = max_sys::common_symbols_gettable();
        assert!(!t.is_null(), "common symbols table hasn't been initialized");
        &*t
    }
}

/// Post a message to the Max console.
pub fn post<T: Into<Vec<u8>>>(msg: T) {
    unsafe {
        match CString::new(msg) {
            Ok(p) => max_sys::post(FMT_S, p.as_ptr()),
            Err(_) => max_sys::error(FMT_S, CSTRING_ERR.as_ptr()),
        }
    }
}

/// Post an error to the Max console.
pub fn error<T: Into<Vec<u8>>>(msg: T) {
    unsafe {
        match CString::new(msg) {
            Ok(p) => max_sys::error(FMT_S, p.as_ptr()),
            Err(_) => max_sys::error(FMT_S, CSTRING_ERR.as_ptr()),
        }
    }
}
