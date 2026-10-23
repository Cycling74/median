//! Ergonomic bindings for the [Max/MSP](https://cycling74.com/) [SDK](https://github.com/Cycling74/max-sdk).
//!
//! # Example
//!
//! A very basic external that has `bang`, `int`, `list`, and `any` methods, a "set" message and
//! two attributes. *See the examples folder for more detailed examples.*
//!
//! ```no_run
//! use median::prelude::*;
//!
//! #[derive(MaxObject)]
//! #[max(name = "example")]
//! pub struct Example {
//!     obj: ObjRef,
//!     #[attr(clip(0, 100))]
//!     value: Int64,
//!     ratio: Float64,
//!     out: Outlet<List>,
//! }
//!
//! impl MaxObjWrapped for Example {
//!     fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
//!         builder.inlet(Inlet::default().assist("bang, int, list, set"));
//!         builder.inlet(Inlet::proxy().assist("bang, int"));
//!         Ok(Self {
//!             obj: builder.obj(),
//!             value: Int64::new(0),
//!             ratio: Float64::new(1.),
//!             out: builder.outlet(Outlet::list().assist("the list, reversed")),
//!         })
//!     }
//! }
//!
//! #[median::methods]
//! impl Example {
//!     #[bang]
//!     pub fn bang(&self) {
//!         let i = self.obj.proxy_inlet();
//!         median::object_post!(&self.obj, "bang from inlet {}", i);
//!     }
//!
//!     #[int]
//!     pub fn int(&self, v: t_atom_long) {
//!         self.value.set(v);
//!         post!("int {} from inlet {}", v, self.obj.proxy_inlet());
//!     }
//!
//!     #[list]
//!     pub fn list(&self, atoms: &[Atom]) {
//!         let rev: Vec<Atom> = atoms.iter().rev().filter_map(Atom::get_value).map(Atom::from).collect();
//!         let _ = self.out.send(&rev);
//!     }
//!
//!     #[any]
//!     pub fn baz(&self, sel: &SymbolRef, atoms: &[Atom]) {
//!         post!("got any with sel {} and length {}", sel, atoms.len());
//!     }
//!
//!     #[sel("set", defaults = 1)]
//!     pub fn set(&self, name: SymbolRef) -> Result<(), String> {
//!         if name == SymbolRef::default() {
//!             return Err("set what?".into());
//!         }
//!         post!("set {}", name);
//!         Ok(())
//!     }
//!
//!     #[attr]
//!     pub fn ratio(&self) -> f64 {
//!         self.ratio.get()
//!     }
//!
//!     pub fn set_ratio(&self, v: f64) {
//!         self.ratio.set(v);
//!     }
//! }
//!
//! median::ext_main! { Example }
//! ```
//!
//! Three ordinary items and a line: the struct with `#[derive(MaxObject)]`, the
//! [`MaxObjWrapped`](wrapper::MaxObjWrapped) impl (or [`MSPObjWrapped`](wrapper::MSPObjWrapped)
//! for an MSP object, which adds `perform`), an inherent impl with `#[median::methods]`, and
//! [`ext_main!`]. They can live in different modules.
//!
//! # The derive
//!
//! `#[derive(MaxObject)]` implements [`ObjClass`](wrapper::ObjClass) (the class name, box type
//! and wrapper), [`FieldAttrs`](wrapper::FieldAttrs) (the field attributes) and an inherent
//! `register()` that [`ext_main!`] calls. Options, in `#[max(..)]` on the struct:
//!
//! * `name = "foo.bar"`: the Max class name; by default the type name in lower case.
//! * `kind = max` (default) or `kind = msp`: which wrapper, and so which of `MaxObjWrapped` and
//!   `MSPObjWrapped` the type must implement (a mismatch is a type error).
//! * `kind = matrix_op` or `kind = gl`: implement [`jit::MatrixOp`] or [`jit::Ob3d`];
//!   the derive registers both the Jitter class and its Max forwarding class.
//! * `class_type = box` (default) or `class_type = nobox`.
//! * `no_methods`: also implement [`Methods`](wrapper::Methods) with no methods, for a type
//!   without a `#[median::methods]` block.
//!
//! `#[attr]` on a field registers an attribute backed by it. The field's type must implement
//! [`AttrField`](attr::AttrField), as the [`num`] atomics do. Options:
//!
//! * `name = ".."`: the attribute's name, by default the field name;
//! * `clip(min, max)`: values are limited to the range; `readonly`: users can't set it;
//! * for the inspector and the patcher file (see [`AttrOptions`](attr::AttrOptions), each is the
//!   SDK's `CLASS_ATTR_*` macro of the same name): `label = ".."`, `style = ".."`,
//!   `category = ".."`, `order = n`, `default = value` (a number, or a parse string),
//!   `save` (stored with the patcher), `enum = "a b c"` (a menu: `enum` style for a symbol
//!   attribute, `enumindex` for a number), `range(min, max)` (shown, not enforced), `hidden`.
//!
//! # Construction
//!
//! `new` returns `Result<Self, NewError>`. An `Err` refuses to make the object, as a C external's
//! `new` returning null does: the message is posted to the Max console for the object, Max shows
//! the box as broken, and everything made with the builder (outlets, inlets, clocks, buffer
//! references) is released. `?` works on strings and many error types, see
//! [`NewError`](error::NewError). A panic in `new` is caught and posted the same way.
//!
//! # Inlets and outlets
//!
//! `new` describes the inlets and makes the outlets with the builder it is given
//! ([`MaxBuilder`](builder::MaxBuilder), or [`MSPBuilder`](builder::MSPBuilder) which also takes
//! signal inlets and outlets), left to right: `builder.inlet(Inlet::proxy().assist(".."))`,
//! `builder.outlet(Outlet::list())`. An outlet is a typed handle, [`Outlet<List>`](outlet::Outlet)
//! and so on, kept in the struct. Each has an assist text (or its kind is shown); inlets can be
//! marked hot. The `assist` and `inlet_hot` hooks of [`MaxObjWrapped`](wrapper::MaxObjWrapped)
//! compute either at run time instead. See [`inlet`] and [`outlet`].
//!
//! # Methods
//!
//! `#[median::methods]` on an inherent impl emits the impl as written, without its markers, and
//! implements [`Methods`](wrapper::Methods) with one [`Class`](class::Class) registration call per
//! marked method. A method without a marker is an ordinary method. The markers and the signature
//! each needs:
//!
//! | marker | Max message | method |
//! |---|---|---|
//! | `#[bang]` | `bang` | `fn(&self)` |
//! | `#[int]` | `int` | `fn(&self, t_atom_long)` |
//! | `#[float]` | `float` | `fn(&self, f64)` |
//! | `#[sym]` | `symbol` | `fn(&self, SymbolRef)` |
//! | `#[list]` | `list` | `fn(&self, &[Atom])` |
//! | `#[any]` | `anything` | `fn(&self, &SymbolRef, &[Atom])` |
//! | `#[sel("name")]`, `#[sel("name", defaults = n)]` | `name` | `fn(&self, A1, .., An)`, up to 7 arguments that are each `f64`, `t_atom_long` or `SymbolRef`, either all `f64` or none (Max can't pass a mix: use `gimme`); the last `n` may be left out (they are then 0 or the empty symbol) |
//! | `#[sel("name", gimme)]` | `name` | `fn(&self, &SymbolRef, &[Atom])` |
//! | `#[attr]`, `#[attr(name = "..", ..)]` with the options of a field attribute | an attribute | a getter `fn(&self) -> V`, `V` one of `f64`, `f32`, `t_atom_long`, `SymbolRef`; a method `set_<getter name>(&self, V)` in the same impl is its setter, without one (or with `readonly`) the attribute is read-only |
//! | `#[dblclick]` | `dblclick` (`A_CANT`): the box was double-clicked | `fn(&self)` |
//! | `#[loadbang]` | `loadbang` (`A_CANT`): the patcher finished loading | `fn(&self)` |
//! | `#[dictionary]` | a retained registered dictionary | `fn(&self, Dict)` |
//! | `#[savestate]` | `appendtodictionary` (`A_CANT`): patcher save | `fn(&self, &Dict)` |
//!
//! A message marker (except `#[attr]`, `#[dblclick]`, `#[loadbang]`, `#[dictionary]`, and
//! `#[savestate]`) can be paired with
//! `#[defer_low]` (always run later on the main thread) or `#[defer]` (run now on the main thread,
//! otherwise later on it): `#[sel("read", gimme)] #[defer_low]`. See [`defer`]; never send a
//! deferred message from `perform`.
//!
//! A message method (and an attribute setter) may return `()` or `Result<(), E>` for any
//! `E: Display`. An `Err` goes to the type's `report_error` hook
//! ([`MaxObjWrapped::report_error`](wrapper::MaxObjWrapped::report_error)), which posts it to the
//! Max console for the object unless you override it, so `?` works in methods. A panic in a
//! method is caught and posted too: it never unwinds into Max.
//!
//! # Without the macros
//!
//! Everything the macros generate is a short, ordinary trait impl calling public API:
//! [`Class::add_bang`](class::Class::add_bang) and the other `add_*` methods take the method
//! itself (`c.add_bang(Self::bang)`) and build what Max calls from its type. Writing those impls
//! by hand is a supported style, and the way to register methods conditionally or under computed
//! names; the `simp_explicit` example is `simp` written that way. See [`class::Class`].
//!
//! # Threads
//!
//! Max calls message methods on the main and scheduler threads while `perform` runs on the
//! audio thread, so every method takes `&self` and the struct must be `Sync`: keep state in
//! atomics (the [`num`] types) or locks, and don't lock or allocate in `perform`.
//!
//! To run code later or elsewhere: a [`Clock`](clock::Clock) calls a method on the scheduler
//! thread after a delay, a [`Queue`](queue::Queue) calls one on the main thread soon (once per
//! burst of `set`s), `#[defer]`/`#[defer_low]` move a message method to the main thread
//! ([`defer`]), and a [`fifo`] carries values out of `perform`.
//!
pub mod alloc;
pub mod atom;
pub mod attr;
pub mod buffer;
pub mod builder;
pub mod class;
pub mod clock;
pub mod defer;
pub mod dict;
pub mod error;
pub mod ffi;
pub mod fifo;
pub mod file;
pub mod inlet;
pub mod jit;
pub mod method;
pub mod notify;
pub mod num;
pub mod object;
pub mod outlet;
pub mod queue;
pub mod slice;
pub mod symbol;
pub mod thread;
pub mod wrapper;

//re-exports
mod max;
pub use self::max::*;

pub use max_sys;

/// Derive the class identity, a `register()` function and the field attributes of a Max or MSP
/// object. See the [crate documentation](crate#the-derive) for the options.
pub use median_macros::MaxObject;

/// Register the marked methods of an inherent impl with the class: implements
/// [`Methods`](crate::wrapper::Methods). See the [crate documentation](crate#methods) for the
/// markers.
pub use median_macros::methods;

/// The `ext_main` function Max calls when it loads the external: registers each class given, in
/// order, each inside its own panic guard (a class that fails to register is posted to the Max
/// console and the others are still registered).
///
/// Each class is a type with a `register()` function, which `#[derive(MaxObject)]` generates:
///
/// ```no_run
/// # use median::prelude::*;
/// # #[derive(MaxObject)]
/// # #[max(no_methods)]
/// # pub struct Simp;
/// # impl MaxObjWrapped for Simp {
/// #     fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> { Ok(Simp) }
/// # }
/// median::ext_main! { Simp }
/// ```
///
/// Two classes in one external: `median::ext_main! { Base, Sig }`.
#[macro_export]
macro_rules! ext_main {
    ($($class:ty),+ $(,)?) => {
        /// Called by Max when it loads the external: registers the classes.
        ///
        /// # Safety
        /// Only Max calls this, once, on its main thread.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn ext_main(_r: *mut ::std::ffi::c_void) {
            $(
                $crate::ffi::guard_ext_main(::std::stringify!($class), || <$class>::register());
            )+
        }
    };
}

/// The common imports for writing an external: `use median::prelude::*;`.
pub mod prelude {
    pub use crate::{
        MaxObject,
        atom::Atom,
        attr::{AttrField, AttrOptions},
        builder::{MSPBuilder, ManagedBufferRef, MaxBuilder},
        class::{Class, ClassType},
        clock::Clock,
        defer::Defer,
        dict::{AtomArray, Dict, DictError, DictValue},
        error::{MaxResult, NewError},
        ext_main,
        inlet::{Inlet, InletId},
        max_sys::t_atom_long,
        methods,
        num::{Float32, Float64, Int64},
        object::{MaxObj, ObjRef},
        object_error, object_post, object_warn,
        outlet::{Any, Bang, Float, Int, List, Outlet, Signal, StackOverflow},
        post,
        queue::Queue,
        sym,
        symbol::SymbolRef,
        wrapper::{
            FieldAttrs, IoLet, MSPObjWrapped, MSPObjWrapper, MaxObjWrapped, MaxObjWrapper, Methods,
            ObjClass, WrapperWrapped,
        },
    };
}

/// Support for the code the macros generate; not part of the API.
#[doc(hidden)]
pub mod __private {
    use std::ffi::CStr;

    /// The `CStr` of a NUL-terminated byte string that has no other NUL (the macros check).
    pub const fn cstr(bytes: &'static [u8]) -> &'static CStr {
        match CStr::from_bytes_with_nul(bytes) {
            Ok(c) => c,
            Err(_) => panic!("median: a name is not a valid C string"),
        }
    }
}

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
