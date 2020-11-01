use std::ffi::c_void;

pub type FloatCB<T> = Box<dyn Fn(&T, f64)>;
pub type IntCB<T> = Box<dyn Fn(&T, i64)>;

pub enum MaxInlet<T> {
    Float(FloatCB<T>),
    Int(IntCB<T>),
    Proxy,
}

pub enum MSPInlet<T> {
    Float(FloatCB<T>),
    Int(IntCB<T>),
    Proxy,
    Signal,
}

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
