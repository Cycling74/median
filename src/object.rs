//! Object traits.

/// Indicates that your struct can be safely cast to a max_sys::t_object this means your struct
/// must be `#[repr(C)]` and have a `max_sys::t_object` as its first member.
pub unsafe trait MaxObj: Sized {
    unsafe fn max_obj(&mut self) -> *mut max_sys::t_object {
        std::mem::transmute::<_, *mut max_sys::t_object>(self)
    }
}

/// Indicates that your struct can be safely cast to a max_sys::t_pxobject this means your struct
/// must be `#[repr(C)]` and have a `max_sys::t_pxobject` as its first member.
///
/// This automatically implements MaxObj.
pub unsafe trait MSPObj: MaxObj {
    unsafe fn msp_obj(&mut self) -> *mut max_sys::t_pxobject {
        std::mem::transmute::<_, *mut max_sys::t_pxobject>(self)
    }
}

use std::ops::{Deref, DerefMut};

/// A smart pointer for an object that max allocated
pub struct ObjBox<T: MaxObj> {
    pub value: Option<Box<T>>, //option box so that we can drop if the value still exists
}

impl<T: MaxObj> ObjBox<T> {
    pub unsafe fn alloc(class: *mut max_sys::t_class) -> Self {
        let value = max_sys::object_alloc(class);
        let value = std::mem::transmute::<_, *mut T>(value);
        Self::from_raw(value)
    }

    pub unsafe fn from_raw(value: *mut T) -> Self {
        Self {
            value: Some(Box::from_raw(value)),
        }
    }

    pub fn into_raw(mut self) -> *mut T {
        let value = self.value.take().unwrap();
        Box::into_raw(value)
    }
}

impl<T: MaxObj> Deref for ObjBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value.as_ref().unwrap().deref()
    }
}

impl<T: MaxObj> DerefMut for ObjBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.as_mut().unwrap().deref_mut()
    }
}

impl<T: MaxObj> Drop for ObjBox<T> {
    fn drop(&mut self) {
        if let Some(v) = self.value.take() {
            unsafe {
                max_sys::object_free(std::mem::transmute::<_, _>(v));
            }
        }
    }
}
