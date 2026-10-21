//! Inlets
use std::ffi::c_void;

///Callback method for Float inlet
pub type FloatCB<T> = Box<dyn Fn(&T, f64) + Send + Sync>;
///Callback method for Int inlet
pub type IntCB<T> = Box<dyn Fn(&T, max_sys::t_atom_long) + Send + Sync>;

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
    /// Create a proxy inlet with the given id.
    ///
    /// # Safety
    /// `owner` must point to a valid Max object that is still being constructed (proxy inlets
    /// must be added from the object's `new`), and must outlive the returned `Proxy`.
    pub unsafe fn new(owner: *mut max_sys::t_object, id: usize) -> Self {
        Self {
            inner: unsafe { max_sys::proxy_new(owner as _, id as _, std::ptr::null_mut()) },
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
        unsafe {
            max_sys::object_free(self.inner);
        }
    }
}
