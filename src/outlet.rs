//! Routines for creating and sending data through outlets.

use crate::atom::Atom;
use crate::symbol::SymbolRef;
use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::sync::Arc;

/// Result type alias from sending data through an outlet.
pub type SendResult = Result<(), SendError>;
pub type OutBang = Arc<dyn SendValue<()> + Sync>;
pub type OutInt = Arc<dyn SendValue<i64> + Sync>;
pub type OutFloat = Arc<dyn SendValue<f64> + Sync>;
pub type OutList = Arc<dyn for<'a> SendValue<&'a [Atom]> + Sync + Send>;
pub type OutAnything = Arc<dyn for<'a> SendAnything<'a> + Sync + Send>;

pub enum SendError {
    StackOverflow,
}

/// Send data through an outlet.
pub trait SendValue<T> {
    fn send(&self, value: T) -> SendResult;
}

/// Send any data through an outlet.
pub trait SendAnything<'a>:
    SendValue<()> + SendValue<i64> + SendValue<f64> + SendValue<&'a [Atom]>
{
    fn send_anything(&self, selector: SymbolRef, value: &'a [Atom]) -> SendResult;
}

/// A safe wrapper for a Max outlet.
///
/// # Remarks
/// This type is marked as Send and Sync but technically it can only be
pub struct Outlet {
    inner: UnsafeCell<*mut c_void>,
}

/// Technically outlets are only Sync in the scheduler or main Max thread.
unsafe impl Send for Outlet {}
unsafe impl Sync for Outlet {}

impl Outlet {
    /// Create an outlet that can send anything Max allows.
    pub fn new(owner: *mut max_sys::t_object) -> OutAnything {
        unsafe {
            let s = Self::new_null();
            s.init_anything(owner);
            s
        }
    }

    /// Create an outlet that will only send bangs.
    pub fn new_bang(owner: *mut max_sys::t_object) -> OutBang {
        unsafe {
            let s = Self::new_null();
            s.init_bang(owner);
            s
        }
    }

    /// Create an outlet that will only send ints.
    pub fn new_int(owner: *mut max_sys::t_object) -> OutInt {
        unsafe {
            let s = Self::new_null();
            s.init_int(owner);
            s
        }
    }

    /// Create an outlet that will only send floats.
    pub fn new_float(owner: *mut max_sys::t_object) -> OutFloat {
        unsafe {
            let s = Self::new_null();
            s.init_float(owner);
            s
        }
    }

    /// Create an outlet that will only send floats.
    pub fn new_list(owner: *mut max_sys::t_object) -> OutList {
        unsafe {
            let s = Self::new_null();
            s.init_list(owner);
            s
        }
    }

    //delayed initialization, allowing builders to allocate but then init later (for reordering)
    pub(crate) fn new_null() -> Arc<Self> {
        Arc::new(Self {
            inner: UnsafeCell::new(std::ptr::null_mut()),
        })
    }

    //these init functions will only be called right after creation, in the same thread, so it
    //should be safe

    pub(crate) unsafe fn init_anything(&self, owner: *mut max_sys::t_object) {
        assert!(self.inner.get().is_null(), "already initialized");
        *self.inner.get() = max_sys::outlet_new(owner as _, std::ptr::null());
    }

    /// Create an outlet that will only send bangs.
    pub(crate) unsafe fn init_bang(&self, owner: *mut max_sys::t_object) {
        assert!(self.inner.get().is_null(), "already initialized");
        *self.inner.get() = max_sys::bangout(owner as _);
    }

    /// Create an outlet that will only send ints.
    pub(crate) unsafe fn init_int(&self, owner: *mut max_sys::t_object) {
        assert!(self.inner.get().is_null(), "already initialized");
        *self.inner.get() = max_sys::intout(owner as _);
    }

    /// Create an outlet that will only send floats.
    pub(crate) unsafe fn init_float(&self, owner: *mut max_sys::t_object) {
        assert!(self.inner.get().is_null(), "already initialized");
        *self.inner.get() = max_sys::floatout(owner as _);
    }

    /// Create an outlet that will only send floats.
    pub(crate) unsafe fn init_list(&self, owner: *mut max_sys::t_object) {
        assert!(self.inner.get().is_null(), "already initialized");
        *self.inner.get() = max_sys::listout(owner as _);
    }
}

/// wrap the result, all the outlet methods return null for success, 1 for stack overflow
fn res_wrap<F: FnOnce() -> *mut c_void>(func: F) -> SendResult {
    if func().is_null() {
        Ok(())
    } else {
        Err(SendError::StackOverflow)
    }
}

impl SendValue<()> for Outlet {
    /// Send a bang.
    fn send(&self, _v: ()) -> SendResult {
        assert!(!self.inner.get().is_null(), "Uninitialized outlet");
        res_wrap(|| unsafe { max_sys::outlet_bang(*self.inner.get()) })
    }
}

impl SendValue<f64> for Outlet {
    /// Send a float.
    fn send(&self, v: f64) -> SendResult {
        assert!(!self.inner.get().is_null(), "Uninitialized outlet");
        res_wrap(|| unsafe { max_sys::outlet_float(*self.inner.get(), v) })
    }
}

impl SendValue<i64> for Outlet {
    /// Send an int.
    fn send(&self, v: i64) -> SendResult {
        assert!(!self.inner.get().is_null(), "Uninitialized outlet");
        res_wrap(|| unsafe { max_sys::outlet_int(*self.inner.get(), v) })
    }
}

impl SendValue<&[Atom]> for Outlet {
    /// Send a list.
    fn send(&self, list: &[Atom]) -> SendResult {
        assert!(!self.inner.get().is_null(), "Uninitialized outlet");
        res_wrap(|| unsafe {
            max_sys::outlet_list(
                *self.inner.get(),
                std::ptr::null_mut(),
                list.len() as _,
                //Atom is transparent, so it can be cast to t_atom
                std::mem::transmute::<_, *mut max_sys::t_atom>(list.as_ptr()),
            )
        })
    }
}

impl<'a> SendAnything<'a> for Outlet {
    /// Send a selector message.
    fn send_anything(&self, selector: SymbolRef, list: &'a [Atom]) -> SendResult {
        assert!(!self.inner.get().is_null(), "Uninitialized outlet");
        res_wrap(|| unsafe {
            max_sys::outlet_anything(
                *self.inner.get(),
                selector.inner(),
                list.len() as _,
                //Atom is transparent, so it can be cast to t_atom
                std::mem::transmute::<_, *mut max_sys::t_atom>(list.as_ptr()),
            )
        })
    }
}
