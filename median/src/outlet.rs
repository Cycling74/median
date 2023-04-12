//! Routines for creating and sending data through outlets.

use crate::atom::Atom;
use crate::max::common_symbols;
use crate::symbol::SymbolRef;
use std::ffi::c_void;

/// Result type alias from sending data through an outlet.
pub type SendResult = Result<(), SendError>;
pub type OutBang = Box<dyn SendValue<()> + Sync>;
pub type OutInt = Box<dyn SendValue<max_sys::t_atom_long> + Sync>;
pub type OutFloat = Box<dyn SendValue<f64> + Sync>;
pub type OutList = Box<dyn for<'a> SendValue<&'a [Atom]> + Sync + Send>;
pub type OutAnything = Box<dyn for<'a> SendAnything<'a> + Sync + Send>;

pub enum SendError {
    StackOverflow,
}

/// Send data through an outlet.
pub trait SendValue<T> {
    fn send(&self, value: T) -> SendResult;
}

/// Send any data through an outlet.
pub trait SendAnything<'a>:
    SendValue<()> + SendValue<max_sys::t_atom_long> + SendValue<f64> + SendValue<&'a [Atom]>
{
    fn send_anything(&self, selector: SymbolRef, value: &'a [Atom]) -> SendResult;
}

/// A safe wrapper for a Max outlet.
///
/// # Remarks
/// This type is marked as Send and Sync but technically it can only be
pub struct Outlet {
    inner: *mut c_void,
}

/// Technically outlets are only Sync in the scheduler or main Max thread.
unsafe impl Send for Outlet {}
unsafe impl Sync for Outlet {}

impl Outlet {
    /// Create an outlet that can send anything Max allows.
    fn append(owner: *mut max_sys::t_object, type_sym: *mut max_sys::t_symbol) -> Box<Self> {
        Box::new(Self {
            inner: unsafe { max_sys::outlet_append(owner, std::ptr::null_mut(), type_sym) },
        })
    }

    /// Create an outlet that can send anything Max allows.
    pub fn append_anything(owner: *mut max_sys::t_object) -> OutAnything {
        Self::append(owner, std::ptr::null_mut())
    }

    /// Create an outlet that will only send bangs.
    pub fn append_bang(owner: *mut max_sys::t_object) -> OutBang {
        Self::append(owner, common_symbols().s_bang)
    }

    /// Create an outlet that will only send ints.
    pub fn append_int(owner: *mut max_sys::t_object) -> OutInt {
        Self::append(owner, common_symbols().s_long)
    }

    /// Create an outlet that will only send floats.
    pub fn append_float(owner: *mut max_sys::t_object) -> OutFloat {
        Self::append(owner, common_symbols().s_float)
    }

    /// Create an outlet that will only send floats.
    pub fn append_list(owner: *mut max_sys::t_object) -> OutList {
        Self::append(owner, common_symbols().s_list)
    }

    /// Add a signal output.
    pub fn append_signal(owner: *mut max_sys::t_object) {
        unsafe {
            let _ = max_sys::outlet_append(owner, std::ptr::null_mut(), common_symbols().s_signal);
        }
    }
}

/// wrap the result, all the outlet methods return 1 for success, null for stack overflow
fn res_wrap<F: FnOnce() -> *mut c_void>(func: F) -> SendResult {
    if func().is_null() {
        Err(SendError::StackOverflow)
    } else {
        Ok(())
    }
}

impl SendValue<()> for Outlet {
    /// Send a bang.
    fn send(&self, _v: ()) -> SendResult {
        res_wrap(|| unsafe { max_sys::outlet_bang(self.inner) })
    }
}

impl SendValue<f64> for Outlet {
    /// Send a float.
    fn send(&self, v: f64) -> SendResult {
        res_wrap(|| unsafe { max_sys::outlet_float(self.inner, v) })
    }
}

impl SendValue<max_sys::t_atom_long> for Outlet {
    /// Send an int.
    fn send(&self, v: max_sys::t_atom_long) -> SendResult {
        res_wrap(|| unsafe { max_sys::outlet_int(self.inner, v) })
    }
}

impl SendValue<&[Atom]> for Outlet {
    /// Send a list.
    fn send(&self, list: &[Atom]) -> SendResult {
        res_wrap(|| unsafe {
            max_sys::outlet_list(
                self.inner,
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
        res_wrap(|| unsafe {
            max_sys::outlet_anything(
                self.inner,
                selector.inner(),
                list.len() as _,
                //Atom is transparent, so it can be cast to t_atom
                std::mem::transmute::<_, *mut max_sys::t_atom>(list.as_ptr()),
            )
        })
    }
}
