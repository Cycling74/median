//! Inlets: described in your `new` with the builder, left to right.
//!
//! ```
//! # use median::prelude::*;
//! # #[derive(MaxObj)]
//! # #[max(no_methods)]
//! pub struct Adder {
//!     right: Int64,
//! }
//!
//! impl MaxObjWrapped for Adder {
//!     fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
//!         //the leftmost inlet is always there; this only gives it an assist text
//!         b.inlet(Inlet::default().assist("left operand, outputs the sum"));
//!         //an `int` inlet (`in1` in C), with a closure that may capture
//!         b.inlet(Inlet::int(|s: &Self, v| s.right.set(v)).assist("right operand"));
//!         //a proxy inlet: messages go to the usual methods, `ObjRef::proxy_inlet` says it was
//!         //inlet 2. Hot: Max draws it like the leftmost.
//!         let _id = b.inlet(Inlet::proxy().hot());
//!         Ok(Self { right: Int64::new(0) })
//!     }
//! }
//! ```
//!
//! * [`Inlet::default`] describes inlet 0, which every object has; it must come first.
//! * [`Inlet::proxy`] makes a proxy inlet whose id (what
//!   [`ObjRef::proxy_inlet`](crate::object::ObjRef::proxy_inlet) returns) is its position.
//! * [`Inlet::int`] and [`Inlet::float`] make an inlet whose `int` or `float` is handed to the
//!   callback (at most 9 of each). Other messages to it are an error in Max.
//! * [`Inlet::signal`] (MSP objects only) is a signal inlet. Signal inlets come first: the
//!   first one is inlet 0 and the others follow it.
//!
//! Each may have an assist text ([`Inlet::assist`], at most 500 bytes are shown; without one,
//! Max is shown its kind: "int", "float", "signal", or "inlet N" for a proxy) and may be marked
//! hot ([`Inlet::hot`]). Inlet 0 is hot and the others are cold unless marked, which is what
//! Max's `stdinletinfo` does. Both can also be computed at run time with the `assist` and
//! `inlet_hot` hooks of [`MaxObjWrapped`](crate::wrapper::MaxObjWrapped).
//!
//! A mistake in the description (an `Inlet::default()` that is not first, a signal inlet after
//! another kind, more than 9 `int` or `float` inlets) makes the object fail to be created, with
//! an error in the Max console.
use std::ffi::c_void;
use std::marker::PhantomData;

use crate::builder::{BuilderKind, Msp};

///Callback method for Float inlet
pub type FloatCB<T> = Box<dyn Fn(&T, f64) + Send + Sync>;
///Callback method for Int inlet
pub type IntCB<T> = Box<dyn Fn(&T, max_sys::t_atom_long) + Send + Sync>;

pub(crate) enum InletKind<T> {
    Default,
    Proxy,
    Int(IntCB<T>),
    Float(FloatCB<T>),
    Signal,
}

/// The description of an inlet, given to the builder's `inlet`. See the [module](self)
/// documentation.
pub struct Inlet<T> {
    pub(crate) kind: InletKind<T>,
    pub(crate) assist: Option<Box<str>>,
    /// `None`: the default, inlet 0 hot and the others cold
    pub(crate) hot: Option<bool>,
}

/// The description of a signal inlet, from [`Inlet::signal`]. MSP objects only: a Max object's
/// builder does not take it.
pub struct SignalInlet<T> {
    assist: Option<Box<str>>,
    hot: Option<bool>,
    _t: PhantomData<fn(&T)>,
}

impl<T> Default for Inlet<T> {
    /// Inlet 0, which every object has: describing it only sets its assist text and hot flag.
    /// It must be the first inlet described.
    fn default() -> Self {
        Self::new(InletKind::Default)
    }
}

impl<T> Inlet<T> {
    fn new(kind: InletKind<T>) -> Self {
        Self {
            kind,
            assist: None,
            hot: None,
        }
    }

    /// A proxy inlet: messages arriving there go to the object's usual methods, during which
    /// [`ObjRef::proxy_inlet`](crate::object::ObjRef::proxy_inlet) returns the inlet's position.
    pub fn proxy() -> Self {
        Self::new(InletKind::Proxy)
    }

    /// An inlet whose `int` messages go to `f`.
    pub fn int<F>(f: F) -> Self
    where
        F: Fn(&T, max_sys::t_atom_long) + Send + Sync + 'static,
    {
        Self::new(InletKind::Int(Box::new(f)))
    }

    /// An inlet whose `float` messages go to `f`.
    pub fn float<F>(f: F) -> Self
    where
        F: Fn(&T, f64) + Send + Sync + 'static,
    {
        Self::new(InletKind::Float(Box::new(f)))
    }

    /// A signal inlet (MSP objects only). Signal inlets must come before the other inlets.
    pub fn signal() -> SignalInlet<T> {
        SignalInlet {
            assist: None,
            hot: None,
            _t: PhantomData,
        }
    }

    /// The assist text Max shows for the inlet (at most 500 bytes are shown).
    pub fn assist(mut self, text: &str) -> Self {
        self.assist = Some(text.into());
        self
    }

    /// Mark the inlet hot: Max draws it as an inlet that makes the object output (inlet 0 is
    /// hot without this).
    pub fn hot(mut self) -> Self {
        self.hot = Some(true);
        self
    }

    /// Mark the inlet cold: Max draws it as an inlet that doesn't make the object output (every
    /// inlet but inlet 0 is cold without this). For inlet 0, e.g. a control inlet like `gate`'s
    /// left one: `Inlet::default().cold()`. Only what Max reports and draws changes, not which
    /// messages the inlet accepts. To change it at run time, see the `inlet_hot` hook.
    pub fn cold(mut self) -> Self {
        self.hot = Some(false);
        self
    }

    /// [`hot`](Self::hot) if `hot`, otherwise [`cold`](Self::cold): for a hot/cold choice
    /// computed in `new`, e.g. every other inlet hot:
    /// `b.inlet(Inlet::proxy().hot_if(i % 2 == 0))`.
    pub fn hot_if(mut self, hot: bool) -> Self {
        self.hot = Some(hot);
        self
    }
}

impl<T> SignalInlet<T> {
    /// As [`Inlet::assist`].
    pub fn assist(mut self, text: &str) -> Self {
        self.assist = Some(text.into());
        self
    }

    /// As [`Inlet::hot`].
    pub fn hot(mut self) -> Self {
        self.hot = Some(true);
        self
    }

    /// As [`Inlet::cold`].
    pub fn cold(mut self) -> Self {
        self.hot = Some(false);
        self
    }

    /// As [`Inlet::hot_if`].
    pub fn hot_if(mut self, hot: bool) -> Self {
        self.hot = Some(hot);
        self
    }
}

mod sealed {
    pub trait Sealed {}
}
impl<T> sealed::Sealed for Inlet<T> {}
impl<T> sealed::Sealed for SignalInlet<T> {}

/// The inlet descriptions a builder of kind `K` for `T` accepts: [`Inlet`] for every object,
/// [`SignalInlet`] for MSP objects only.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an inlet description for this builder",
    note = "signal inlets (`Inlet::signal()`) are for MSP objects only (`MSPBuilder`)"
)]
pub trait InletFor<T, K>: sealed::Sealed {
    #[doc(hidden)]
    fn into_inlet(self) -> Inlet<T>;
}

impl<T, K: BuilderKind> InletFor<T, K> for Inlet<T> {
    fn into_inlet(self) -> Inlet<T> {
        self
    }
}

impl<T> InletFor<T, Msp> for SignalInlet<T> {
    fn into_inlet(self) -> Inlet<T> {
        Inlet {
            kind: InletKind::Signal,
            assist: self.assist,
            hot: self.hot,
        }
    }
}

/// An inlet's position, from the builder's `inlet`: 0 is the leftmost. For a proxy inlet it is
/// also what [`ObjRef::proxy_inlet`](crate::object::ObjRef::proxy_inlet) returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InletId(pub(crate) usize);

impl InletId {
    /// The position, counted from 0 at the left.
    pub fn index(self) -> usize {
        self.0
    }
}

/// Encapsulation of a Max Proxy inlet
pub struct Proxy {
    inner: *mut c_void,
}

impl Proxy {
    /// Create a proxy inlet with the given id.
    ///
    /// # Safety
    /// `owner` must point to a valid Max object that is still being constructed (proxy inlets
    /// must be added from the object's `new`), and must outlive the returned `Proxy`.
    pub unsafe fn new(owner: *mut max_sys::t_object, id: usize) -> Self {
        Self {
            inner: unsafe {
                max_sys::proxy_new(owner as _, id as std::os::raw::c_long, std::ptr::null_mut())
            },
        }
    }

    /// Get the index of the inlet that received the message currently being handled.
    ///
    /// # Safety
    /// * `owner` must convert to a pointer to a valid Max object that has proxy inlets.
    /// * Must only be called from within a message method that Max invoked on that object, since
    ///   Max only tracks the receiving inlet for the duration of that call.
    pub unsafe fn get_inlet<I: Into<*mut max_sys::t_object>>(owner: I) -> usize {
        unsafe { max_sys::proxy_getinlet(owner.into()) as _ }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        if !self.inner.is_null() {
            unsafe {
                max_sys::object_free(self.inner);
            }
        }
    }
}
