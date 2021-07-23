///! Inlets
use std::ffi::c_void;

///Callback method for Float inlet
pub type FloatCB<T> = Box<dyn Fn(&T, f64)>;
///Callback method for Int inletk
pub type IntCB<T> = Box<dyn Fn(&T, max_sys::t_atom_long)>;

/// Inlets for Max objects
pub enum MaxInlet<T> {
    Float(FloatCB<T>),
    Int(IntCB<T>),
    Proxy,
}

/// Inlets for MSP objects
pub enum MSPInlet<T> {
    Float(FloatCB<T>),
    Int(IntCB<T>),
    Proxy,
    Signal,
}

/// Encapsulation of a Max Proxy inlet
pub struct Proxy {
    inner: *mut c_void,
}

impl Proxy {
    pub fn new(owner: *mut max_sys::t_object, id: usize) -> Self {
        Self {
            inner: unsafe { max_sys::proxy_new(owner as _, id as _, std::ptr::null_mut()) },
        }
    }

    pub fn get_inlet<I: Into<*mut max_sys::t_object>>(owner: I) -> usize {
        unsafe { max_sys::proxy_getinlet(owner.into()) as _ }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        unsafe {
            max_sys::object_free(self.inner);
        }
    }
}
