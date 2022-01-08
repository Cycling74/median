//! Jitter

pub mod ob3d;

/// Wrap the Jitter class pointer so we can use it across threads
#[repr(transparent)]
pub struct Class {
    pub inner: *mut std::ffi::c_void,
}

unsafe impl Sync for Class {}
unsafe impl Send for Class {}
