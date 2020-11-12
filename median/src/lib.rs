pub mod alloc;
pub mod atom;
pub mod attr;
pub mod buffer;
pub mod builder;
pub mod class;
pub mod clock;
pub mod error;
pub mod inlet;
pub mod method;
pub mod notify;
pub mod num;
pub mod object;
pub mod outlet;
pub mod slice;
pub mod symbol;
pub mod wrapper;

//re-exports
mod max;
pub use self::max::*;
pub use median_macros::external;

/// Post a message to the Max console, using the same format as `std::format!`.
#[macro_export]
macro_rules! post {
    ($($arg:tt)*) => {{
        $crate::post(std::format!($($arg)*))
    }}
}

/// Post an error to the Max console, using the same format as `std::format!`.
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{
        $crate::error(std::format!($($arg)*))
    }}
}

/// Post a message to the Max console, using the same format as `std::format!`.
#[macro_export]
macro_rules! object_post {
    ($obj:expr, $($arg:tt)*) => {{
        $crate::object::post($obj, std::format!($($arg)*))
    }}
}

/// Post an error to the Max console, using the same format as `std::format!`.
#[macro_export]
macro_rules! object_error {
    ($obj:tt, $($arg:tt)*) => {{
        $crate::object::error($obj, std::format!($($arg)*))
    }}
}

#[cfg(test)]
pub mod test;
