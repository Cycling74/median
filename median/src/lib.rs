//! Ergonomic bindings for the [Max/MSP](https://cycling74.com/) [SDK](https://github.com/Cycling74/max-sdk).
//!
//! # Example
//!
//! A very basic external the has `bang`, `int`, `list`, and `any` methods.
//! *See the examples folder for more detailed examples.*
//!
//! ```no_run
//! use median::{
//!     atom::Atom, builder::MaxWrappedBuilder, max_sys::t_atom_long, object::ObjRef, post,
//!     symbol::SymbolRef, wrapper::*,
//! };
//!
//! median::external! {
//!     pub struct Example {
//!         obj: ObjRef,
//!     }
//!
//!     impl MaxObjWrapped<Example> for Example {
//!         fn new(builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
//!             let _ = builder.add_inlet(median::inlet::MaxInlet::Proxy);
//!             Self { obj: builder.obj() }
//!         }
//!     }
//!
//!     impl Example {
//!         #[bang]
//!         pub fn bang(&self) {
//!             let i = self.obj.proxy_inlet();
//!             median::object_post!(&self.obj, "bang from inlet {}", i);
//!         }
//!
//!         #[int]
//!         pub fn int(&self, v: t_atom_long) {
//!             let i = self.obj.proxy_inlet();
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
pub mod ffi;
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
/// The first argument is a `&ObjRef`, the handle you get from `builder.obj()` in your `new`.
///
/// # Examples
///
/// ```no_run
/// use median::object::ObjRef;
///
/// struct MyObj {
///     obj: ObjRef,
/// }
///
/// impl MyObj {
///     pub fn bang(&self) {
///         median::object_post!(&self.obj, "from max obj {}", 2084);
///     }
/// }
/// ```
#[macro_export]
macro_rules! object_post {
    ($obj:expr_2021, $($arg:tt)*) => {{
        let obj: &$crate::object::ObjRef = $obj;
        obj.post(::std::format!($($arg)*))
    }}
}

/// Post an error to the Max console, associated with the given object, using the same format as `std::format!`.
///
/// The first argument is a `&ObjRef`, the handle you get from `builder.obj()` in your `new`.
///
/// # Examples
///
/// ```no_run
/// use median::object::ObjRef;
///
/// struct MyObj {
///     obj: ObjRef,
/// }
///
/// impl MyObj {
///     pub fn bang(&self) {
///         median::object_error!(&self.obj, "from max obj {}", 2084);
///     }
/// }
/// ```
#[macro_export]
macro_rules! object_error {
    ($obj:expr_2021, $($arg:tt)*) => {{
        let obj: &$crate::object::ObjRef = $obj;
        obj.error(::std::format!($($arg)*))
    }}
}

/// Post a warning to the Max console, associated with the given object, using the same format as `std::format!`.
///
/// The first argument is a `&ObjRef`, the handle you get from `builder.obj()` in your `new`.
///
/// # Examples
///
/// ```no_run
/// use median::object::ObjRef;
///
/// struct MyObj {
///     obj: ObjRef,
/// }
///
/// impl MyObj {
///     pub fn bang(&self) {
///         median::object_warn!(&self.obj, "from max obj {}", 2084);
///     }
/// }
/// ```
#[macro_export]
macro_rules! object_warn {
    ($obj:expr_2021, $($arg:tt)*) => {{
        let obj: &$crate::object::ObjRef = $obj;
        obj.warn(::std::format!($($arg)*))
    }}
}

// The tests call the Max C API, which `median-mock` implements. Nothing here names it, so say so
// to get it linked.
#[cfg(test)]
use median_mock as _;

#[cfg(test)]
pub mod test;
