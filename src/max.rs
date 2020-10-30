use std::convert::Into;
use std::ffi::CString;

/// Get a reference to the common symbols table
pub fn common_symbols() -> &'static max_sys::_common_symbols_table {
    unsafe {
        assert!(
            !max_sys::_common_symbols.is_null(),
            "common symbols table hasn't been initialized"
        );
        &*max_sys::_common_symbols
    }
}

/// Post a message to the Max console.
pub fn post<T: Into<Vec<u8>>>(msg: T) {
    unsafe {
        match CString::new(msg) {
            Ok(p) => max_sys::post(p.as_ptr()),
            //TODO make CString below a const static
            Err(_) => self::error("failed to create CString"),
        }
    }
}

/// Post an error to the Max console.
pub fn error<T: Into<Vec<u8>>>(msg: T) {
    unsafe {
        match CString::new(msg) {
            Ok(p) => max_sys::error(p.as_ptr()),
            //TODO make CString below a const static
            Err(_) => max_sys::error(CString::new("failed to create CString").unwrap().as_ptr()),
        }
    }
}

/// Post a message to the Max console, using the same format as `std::format!`.
#[macro_export]
macro_rules! post {
    ($($arg:tt)*) => {{
        crate::post(std::format!($($arg)*))
    }}
}

/// Post an error to the Max console, using the same format as `std::format!`.
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{
        crate::error(std::format!($($arg)*))
    }}
}
