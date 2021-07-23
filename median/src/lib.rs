//! Ergonomic bindings for the [Max/MSP](https://cycling74.com/) [SDK](https://github.com/Cycling74/max-sdk).
//!
//! # Example
//!
//! A very basic external the has `bang`, `int`, `list`, and `any` methods.
//! *See the examples folder for more detailed examples.*
//!
//! ```no_run
//! use median::{
//!     atom::Atom, builder::MaxWrappedBuilder, max_sys::t_atom_long, object::MaxObj, post,
//!     symbol::SymbolRef, wrapper::*,
//! };
//!
//! median::external! {
//!     pub struct Example;
//!
//!     impl MaxObjWrapped<Example> for Example {
//!         fn new(builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
//!             let _ = builder.add_inlet(median::inlet::MaxInlet::Proxy);
//!             Self
//!         }
//!     }
//!
//!     impl Example {
//!         #[bang]
//!         pub fn bang(&self) {
//!             let i = median::inlet::Proxy::get_inlet(self.max_obj());
//!             median::object_post!(self.max_obj(), "bang from inlet {}", i);
//!         }
//!
//!         #[int]
//!         pub fn int(&self, v: t_atom_long) {
//!             let i = median::inlet::Proxy::get_inlet(self.max_obj());
//!             post!("int {} from inlet {}", v, i);
//!         }
//!
//!         #[list]
//!         pub fn list(&self, atoms: &[Atom]) {
//!             post!("got list with length {}", atoms.len());
//!         }
//!
//!         #[any]
//!         pub fn baz(&self, sel: &SymbolRef, atoms: &[Atom]) {
//!             post!("got any with sel {} and length {}", sel, atoms.len());
//!         }
//!     }
//! }
//! ```
//!
pub mod alloc;
pub mod atom;
pub mod attr;
pub mod buffer;
pub mod builder;
pub mod class;
pub mod clock;
pub mod error;
pub mod file;
pub mod inlet;
pub mod method;
pub mod notify;
pub mod num;
pub mod object;
pub mod outlet;
pub mod slice;
pub mod symbol;
pub mod thread;
pub mod wrapper;

//re-exports
mod max;
pub use self::max::*;

pub use max_sys;

/// Wrap the given code in ext_main
pub use median_macros::ext_main;
/// Create an external with the wrapped contents.
pub use median_macros::external;
/// Create an external with the wrapped contents, don't register ext_main.
pub use median_macros::external_no_main;

/// Post a message to the Max console, using the same format as `std::format!`.
#[macro_export]
macro_rules! post {
    ($($arg:tt)*) => {{
        $crate::post(::std::format!($($arg)*))
    }}
}

/// Post an error to the Max console, using the same format as `std::format!`.
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {{
        $crate::error(::std::format!($($arg)*))
    }}
}

/// Post a message to the Max console, associated with the given object, using the same format as `std::format!`.
///
/// # Examples
///
/// Calling inside method for a struct that implements `MaxObjWrapped`.
/// ```ignore
/// use median::object::MaxObj;
///
/// pub fn bang(&self) {
///     median::object_post!(self.max_obj(), "from max obj");
/// }
/// ```
///
/// Calling inside method for a struct that implements `MSPObjWrapped`.
/// ```ignore
/// use median::object::MSPObj;
///
/// pub fn bang(&self) {
///     median::object_post!(self.as_max_obj(), "from msp obj {}", 2084);
/// }
/// ```
///
/// # Remarks
/// * `MaxObjWrapped` objects can use `self.max_obj()` as the first argument, but you must `use
/// median::object::MaxObj`
/// * `MSPObjWrapped` objects can use `self.as_max_obj()` as the first argument, but you must `use
/// median::object::MSPObj`
#[macro_export]
macro_rules! object_post {
    ($obj:expr, $($arg:tt)*) => {{
        $crate::object::post($obj, ::std::format!($($arg)*))
    }}
}

/// Post an error to the Max console, associated with the given object, using the same format as `std::format!`.
/// # Examples
///
/// Calling inside method for a struct that implements `MaxObjWrapped`.
/// ```ignore
/// use median::object::MaxObj;
///
/// pub fn bang(&self) {
///     median::object_error!(self.max_obj(), "from max obj");
/// }
/// ```
///
/// Calling inside method for a struct that implements `MSPObjWrapped`.
/// ```ignore
/// use median::object::MSPObj;
///
/// pub fn bang(&self) {
///     median::object_error!(self.as_max_obj(), "from msp obj {}", 2084);
/// }
/// ```
///
/// # Remarks
/// * `MaxObjWrapped` objects can use `self.max_obj()` as the first argument, but you must `use
/// median::object::MaxObj`
/// * `MSPObjWrapped` objects can use `self.as_max_obj()` as the first argument, but you must `use
/// median::object::MSPObj`
#[macro_export]
macro_rules! object_error {
    ($obj:expr, $($arg:tt)*) => {{
        $crate::object::error($obj, ::std::format!($($arg)*))
    }}
}

#[cfg(test)]
pub mod test;
