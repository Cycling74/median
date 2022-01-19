//! Jitter

use std::{collections::HashMap, sync::Mutex};

pub mod attr;
pub mod matrix;
pub mod ob3d;

pub type JitError = max_sys::t_jit_error_code::Type;
pub type JitResult<T> = Result<T, JitError>;

///trampoline for attribute getters
pub use median_macros::wrapped_jit_attr_get_tramp as attr_get_tramp;
///trampoline for attribute setters
pub use median_macros::wrapped_jit_attr_set_tramp as attr_set_tramp;

/// Wrap the Jitter class pointer so we can use it across threads
#[repr(transparent)]
pub struct Class {
    pub inner: *mut std::ffi::c_void,
}

impl Class {
    pub fn inner(&self) -> *mut std::ffi::c_void {
        self.inner
    }
}

unsafe impl Sync for Class {}
unsafe impl Send for Class {}

lazy_static::lazy_static! {
    pub(crate) static ref CLASSES: Mutex<HashMap<&'static str, Class>> = Mutex::new(HashMap::new());
}

pub fn result_wrap<T>(code: max_sys::t_jit_error_code::Type, v: T) -> JitResult<T> {
    if code == max_sys::t_jit_error_code::JIT_ERR_NONE {
        Ok(v)
    } else {
        Err(code)
    }
}
