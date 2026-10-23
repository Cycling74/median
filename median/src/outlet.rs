//! Outlets: declared in your `new` with the builder, kept in your struct, sent through from your
//! methods.
//!
//! ```
//! # use median::prelude::*;
//! # #[derive(MaxObject)]
//! # #[max(no_methods)]
//! pub struct Counter {
//!     out: Outlet<Int>,
//!     done: Outlet<Bang>,
//! }
//!
//! impl MaxObjWrapped for Counter {
//!     fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
//!         Ok(Self {
//!             //left to right
//!             out: b.outlet(Outlet::int().assist("the count")),
//!             done: b.outlet(Outlet::bang().assist("bang when done")),
//!         })
//!     }
//! }
//!
//! impl Counter {
//!     fn count(&self, n: t_atom_long) {
//!         for i in 0..n {
//!             if self.out.send(i).is_err() {
//!                 return; //stack overflow: Max stopped the message
//!             }
//!         }
//!         let _ = self.done.send();
//!     }
//! }
//! ```
//!
//! The kind of an outlet is part of its type ([`Bang`], [`Int`], [`Float`], [`List`], [`Any`],
//! and [`Signal`] for MSP objects), and gives what `send` takes. It is also the type Max uses to
//! check connections, and the default assist text.
//!
//! # Threads
//!
//! Max protects outlets between its main and scheduler threads, so `send` may be called from
//! message methods and clock callbacks. **Do not send from `perform`** (the audio thread): the
//! SDK says the only supported way for a perform routine to reach Max is a clock.

use crate::atom::Atom;
use crate::builder::{BuilderKind, Msp};
use crate::max::common_symbols;
use crate::symbol::SymbolRef;
use std::ffi::c_void;
use std::fmt;
use std::marker::PhantomData;

/// Max refused to send because its stack overflowed (a feedback loop in the patch).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackOverflow;

impl fmt::Display for StackOverflow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("stack overflow")
    }
}

impl std::error::Error for StackOverflow {}

/// What sending through an outlet returns.
pub type SendResult = Result<(), StackOverflow>;

/// A dictionary must be registered before its name can be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DictSendError {
    Unregistered,
    StackOverflow(StackOverflow),
}
impl fmt::Display for DictSendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unregistered => f.write_str("dictionary is not registered"),
            Self::StackOverflow(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for DictSendError {}

mod sealed {
    pub trait Sealed {}
}

/// The kind of an outlet: [`Bang`], [`Int`], [`Float`], [`List`], [`Any`] or [`Signal`].
pub trait OutletKind: sealed::Sealed {
    /// The type name Max is given for the outlet, and the default assist text.
    const NAME: &'static str;
    #[doc(hidden)]
    fn type_sym() -> *mut max_sys::t_symbol;
}

/// The outlet kinds a builder of kind `K` accepts: all of them for MSP objects, all but
/// [`Signal`] for Max objects.
#[diagnostic::on_unimplemented(
    message = "a `{Self}` outlet can't be made by this builder",
    note = "signal outlets (`Outlet::signal()`) are for MSP objects only (`MSPBuilder`)"
)]
pub trait OutletFor<K>: OutletKind {}

macro_rules! kinds {
    ($( $(#[$m:meta])* $t:ident, $name:literal, $sym:expr; )*) => {
        $(
            $(#[$m])*
            pub enum $t {}
            impl sealed::Sealed for $t {}
            impl OutletKind for $t {
                const NAME: &'static str = $name;
                fn type_sym() -> *mut max_sys::t_symbol {
                    $sym
                }
            }
        )*
    };
}

kinds! {
    /// An outlet that sends `bang`: `send(&self)`.
    Bang, "bang", common_symbols().s_bang;
    /// An outlet that sends `int`: `send(&self, t_atom_long)`.
    Int, "int", common_symbols().s_int;
    /// An outlet that sends `float`: `send(&self, f64)`.
    Float, "float", common_symbols().s_float;
    /// An outlet that sends `list`: `send(&self, &[Atom])`.
    List, "list", common_symbols().s_list;
    /// An outlet that sends any message: `send(&self, &SymbolRef, &[Atom])`, and the
    /// `send_bang`/`send_int`/`send_float`/`send_list` shorthands.
    Any, "anything", std::ptr::null_mut();
    /// A signal outlet of an MSP object. Nothing is sent through it from Rust: `perform` writes
    /// its samples.
    Signal, "signal", common_symbols().s_signal;
}

impl<K: BuilderKind> OutletFor<K> for Bang {}
impl<K: BuilderKind> OutletFor<K> for Int {}
impl<K: BuilderKind> OutletFor<K> for Float {}
impl<K: BuilderKind> OutletFor<K> for List {}
impl<K: BuilderKind> OutletFor<K> for Any {}
impl OutletFor<Msp> for Signal {}

/// The description of an outlet, from [`Outlet::bang`], [`Outlet::int`] and so on, given to the
/// builder's `outlet`.
pub struct OutletDesc<K> {
    pub(crate) assist: Option<Box<str>>,
    _kind: PhantomData<K>,
}

impl<K> OutletDesc<K> {
    fn new() -> Self {
        Self {
            assist: None,
            _kind: PhantomData,
        }
    }

    /// The assist text Max shows for the outlet (at most 500 bytes are shown). Without it, the
    /// outlet's kind is shown (`"int"`, `"list"`, ...).
    pub fn assist(mut self, text: &str) -> Self {
        self.assist = Some(text.into());
        self
    }
}

/// An outlet of your object, from the builder's `outlet`. Keep it in your struct and send through
/// it from your methods.
///
/// # Limits
///
/// Like [`ObjRef`](crate::object::ObjRef), an outlet is only valid while its object is, and is
/// deliberately neither `Clone`/`Copy` nor `Send`, so the borrow checker keeps it inside the
/// object for normal use. It can't stop you from deliberately leaking it out of the object,
/// with `Box::leak` or a `thread_local!` for example. **Don't do that**: sending through an
/// outlet after its object has been deleted is a use-after-free.
///
/// `Outlet` is not `Clone`:
///
/// ```compile_fail,E0277
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<median::outlet::Outlet<median::outlet::List>>();
/// ```
///
/// `Outlet` is not `Send`:
///
/// ```compile_fail,E0277
/// fn needs_send<T: Send>() {}
/// needs_send::<median::outlet::Outlet<median::outlet::List>>();
/// ```
///
/// (and it is `Sync`, as your struct must be:)
///
/// ```
/// fn needs_sync<T: Sync>() {}
/// needs_sync::<median::outlet::Outlet<median::outlet::List>>();
/// ```
pub struct Outlet<K> {
    inner: *mut c_void,
    // `!Send` (and `!Sync`, added back below), see the type's documentation
    _kind: PhantomData<(K, *const ())>,
}

// SAFETY: Max protects outlets between its main and scheduler threads (see the module
// documentation for the audio thread); the handle itself is only a pointer. Deliberately not
// `Send`, like `ObjRef`.
unsafe impl<K> Sync for Outlet<K> {}

impl<K: OutletKind> Outlet<K> {
    /// Append an outlet of kind `K` to the right of the existing ones.
    ///
    /// # Safety
    /// `owner` must point to a valid Max object that is still being constructed, and the outlet
    /// must not be used after the object is freed.
    pub(crate) unsafe fn append(owner: *mut max_sys::t_object) -> Self {
        Self {
            // SAFETY: the caller's contract
            inner: unsafe { max_sys::outlet_append(owner, std::ptr::null_mut(), K::type_sym()) },
            _kind: PhantomData,
        }
    }

    /// Whether Max made the outlet.
    pub(crate) fn is_null(&self) -> bool {
        self.inner.is_null()
    }

    /// Send through the outlet with `f` (an `outlet_*` call), unless it was never made.
    fn send_with<F: FnOnce(*mut c_void) -> *mut c_void>(&self, f: F) -> SendResult {
        if self.inner.is_null() {
            return Ok(());
        }
        //the outlet functions return null for a stack overflow, non-null otherwise
        if f(self.inner).is_null() {
            Err(StackOverflow)
        } else {
            Ok(())
        }
    }
}

impl Outlet<Bang> {
    /// Describe an outlet that sends `bang`.
    pub fn bang() -> OutletDesc<Bang> {
        OutletDesc::new()
    }

    /// Send a bang.
    pub fn send(&self) -> SendResult {
        // SAFETY: `inner` is a live outlet of this live object
        self.send_with(|o| unsafe { max_sys::outlet_bang(o as _) })
    }
}

impl Outlet<Int> {
    /// Describe an outlet that sends `int`.
    pub fn int() -> OutletDesc<Int> {
        OutletDesc::new()
    }

    /// Send an int.
    pub fn send(&self, v: max_sys::t_atom_long) -> SendResult {
        // SAFETY: as in `Outlet<Bang>::send`
        self.send_with(|o| unsafe { max_sys::outlet_int(o as _, v) })
    }
}

impl Outlet<Float> {
    /// Describe an outlet that sends `float`.
    pub fn float() -> OutletDesc<Float> {
        OutletDesc::new()
    }

    /// Send a float.
    pub fn send(&self, v: f64) -> SendResult {
        // SAFETY: as in `Outlet<Bang>::send`
        self.send_with(|o| unsafe { max_sys::outlet_float(o as _, v) })
    }
}

/// `outlet_list`/`outlet_anything` take `short ac`.
fn list_len(atoms: &[Atom]) -> std::os::raw::c_short {
    atoms.len().min(std::os::raw::c_short::MAX as usize) as _
}

/// `Atom` is `#[repr(transparent)]` over `t_atom`; Max doesn't write to the atoms it is sent.
fn atoms_ptr(atoms: &[Atom]) -> *mut max_sys::t_atom {
    atoms.as_ptr() as *const max_sys::t_atom as *mut max_sys::t_atom
}

impl Outlet<List> {
    /// Describe an outlet that sends `list`.
    pub fn list() -> OutletDesc<List> {
        OutletDesc::new()
    }

    /// Send a list (at most 32767 atoms are sent).
    pub fn send(&self, atoms: &[Atom]) -> SendResult {
        // SAFETY: as in `Outlet<Bang>::send`; `atoms` is valid for `list_len(atoms)` atoms
        self.send_with(|o| unsafe {
            max_sys::outlet_list(
                o as _,
                std::ptr::null_mut(),
                list_len(atoms),
                atoms_ptr(atoms),
            )
        })
    }
}

impl Outlet<Any> {
    /// Send a dictionary's registered name. Register it explicitly with `Dict::register` first
    /// and keep its returned handle alive for downstream lookups. Receivers retaining a copy
    /// during this synchronous send may keep that copy independently.
    pub fn send_dict(&self, dict: &crate::dict::Dict) -> Result<SymbolRef, DictSendError> {
        let name = dict.name().ok_or(DictSendError::Unregistered)?;
        self.send(&crate::sym!("dictionary"), &[name.into()])
            .map_err(DictSendError::StackOverflow)?;
        Ok(name)
    }
    /// Describe an outlet that sends any message.
    pub fn any() -> OutletDesc<Any> {
        OutletDesc::new()
    }

    /// Send the message `sel atoms...` (at most 32767 atoms are sent).
    pub fn send(&self, sel: &SymbolRef, atoms: &[Atom]) -> SendResult {
        // SAFETY: as in `Outlet<List>::send`
        self.send_with(|o| unsafe {
            max_sys::outlet_anything(o as _, sel.as_ptr(), list_len(atoms), atoms_ptr(atoms))
        })
    }

    /// Send a bang.
    pub fn send_bang(&self) -> SendResult {
        // SAFETY: as in `Outlet<Bang>::send`
        self.send_with(|o| unsafe { max_sys::outlet_bang(o as _) })
    }

    /// Send an int.
    pub fn send_int(&self, v: max_sys::t_atom_long) -> SendResult {
        // SAFETY: as in `Outlet<Bang>::send`
        self.send_with(|o| unsafe { max_sys::outlet_int(o as _, v) })
    }

    /// Send a float.
    pub fn send_float(&self, v: f64) -> SendResult {
        // SAFETY: as in `Outlet<Bang>::send`
        self.send_with(|o| unsafe { max_sys::outlet_float(o as _, v) })
    }

    /// Send a list (at most 32767 atoms are sent).
    pub fn send_list(&self, atoms: &[Atom]) -> SendResult {
        // SAFETY: as in `Outlet<List>::send`
        self.send_with(|o| unsafe {
            max_sys::outlet_list(
                o as _,
                std::ptr::null_mut(),
                list_len(atoms),
                atoms_ptr(atoms),
            )
        })
    }
}

impl Outlet<Signal> {
    /// Describe a signal outlet (MSP objects only).
    pub fn signal() -> OutletDesc<Signal> {
        OutletDesc::new()
    }
}
