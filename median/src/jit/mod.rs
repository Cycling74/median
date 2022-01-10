//! Jitter

use std::{collections::HashMap, sync::Mutex};

pub mod matrix;
pub mod ob3d;

/// Wrap the Jitter class pointer so we can use it across threads
#[repr(transparent)]
pub struct Class {
    pub inner: *mut std::ffi::c_void,
}

unsafe impl Sync for Class {}
unsafe impl Send for Class {}

lazy_static::lazy_static! {
    pub(crate) static ref CLASSES: Mutex<HashMap<&'static str, Class>> = Mutex::new(HashMap::new());
}
