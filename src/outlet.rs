//! Routines for creating and sending data through outlets.

use crate::atom::Atom;
use crate::symbol::SymbolRef;
use std::ffi::c_void;

/// Result type alias from sending data through an outlet.
pub type SendResult = Result<(), SendError>;
pub type OutBang = Box<dyn Send<()>>;
pub type OutInt = Box<dyn Send<i64>>;
pub type OutFloat = Box<dyn Send<f64>>;
pub type OutList = Box<dyn for<'a> Send<&'a [Atom]>>;
pub type OutAnything = Box<dyn for<'a> SendAnything<'a>>;

pub enum SendError {
    StackOverflow,
}

/// Send data through an outlet.
pub trait Send<T> {
    fn send(&self, value: T) -> SendResult;
}

/// Send any data through an outlet.
pub trait SendAnything<'a>: Send<()> + Send<i64> + Send<f64> + Send<&'a [Atom]> {
    fn send_anything(&self, selector: SymbolRef, value: &'a [Atom]) -> SendResult;
}

/// A safe wrapper for a Max outlet.
pub struct Outlet {
    inner: *mut c_void,
}

impl Outlet {
    /// Create an outlet that can send anything Max allows.
    pub fn new(owner: *mut max_sys::t_object) -> OutAnything {
        Box::new(Self {
            inner: unsafe { max_sys::outlet_new(owner as _, std::ptr::null()) },
        })
    }

    /// Create an outlet that will only send bangs.
    pub fn new_bang(owner: *mut max_sys::t_object) -> OutBang {
        Box::new(Self {
            inner: unsafe { max_sys::bangout(owner as _) },
        })
    }

    /// Create an outlet that will only send ints.
    pub fn new_int(owner: *mut max_sys::t_object) -> OutInt {
        Box::new(Self {
            inner: unsafe { max_sys::intout(owner as _) },
        })
    }

    /// Create an outlet that will only send floats.
    pub fn new_float(owner: *mut max_sys::t_object) -> OutFloat {
        Box::new(Self {
            inner: unsafe { max_sys::floatout(owner as _) },
        })
    }

    /// Create an outlet that will only send floats.
    pub fn new_list(owner: *mut max_sys::t_object) -> OutList {
        Box::new(Self {
            inner: unsafe { max_sys::listout(owner as _) },
        })
    }

    //helper function used with list and anything
    fn send_anything_sym(&self, selector: *const max_sys::t_symbol, list: &[Atom]) -> SendResult {
        res_wrap(|| unsafe {
            max_sys::outlet_anything(
                self.inner,
                selector,
                list.len() as _,
                //Atom is transparent, so it can be cast to t_atom
                std::mem::transmute::<_, *mut max_sys::t_atom>(list.as_ptr()),
            )
        })
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

impl Send<()> for Outlet {
    /// Send a bang.
    fn send(&self, _v: ()) -> SendResult {
        res_wrap(|| unsafe { max_sys::outlet_bang(self.inner) })
    }
}

impl Send<f64> for Outlet {
    /// Send a float.
    fn send(&self, v: f64) -> SendResult {
        res_wrap(|| unsafe { max_sys::outlet_float(self.inner, v) })
    }
}

impl Send<i64> for Outlet {
    /// Send an int.
    fn send(&self, v: i64) -> SendResult {
        res_wrap(|| unsafe { max_sys::outlet_int(self.inner, v) })
    }
}

impl Send<&[Atom]> for Outlet {
    /// Send a list.
    fn send(&self, v: &[Atom]) -> SendResult {
        self.send_anything_sym(std::ptr::null(), v)
    }
}

impl<'a> SendAnything<'a> for Outlet {
    /// Send a selector message.
    fn send_anything(&self, selector: SymbolRef, list: &'a [Atom]) -> SendResult {
        self.send_anything_sym(unsafe { selector.inner() }, list)
    }
}
