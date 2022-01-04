//! Object traits.

use crate::{
    error::{MaxError, MaxResult},
    notify::{Attachment, AttachmentError, Registration, RegistrationError, Subscription},
    symbol::SymbolRef,
};
use std::convert::TryInto;

macro_rules! impl_obj_methods {
    ($o:expr) => {
        /// Post a message to max.
        fn post<M: Into<Vec<u8>>>(&self, msg: M) {
            crate::object::post($o(self), msg)
        }

        /// Post an error message to max.
        fn post_error<M: Into<Vec<u8>>>(&self, msg: M) {
            crate::object::error($o(self), msg)
        }

        /// Try to register this object with the given namespace and name.
        fn try_register(
            &self,
            namespace: SymbolRef,
            name: SymbolRef,
        ) -> Result<Registration, RegistrationError> {
            unsafe { Registration::try_register($o(self), namespace, name) }
        }

        /// Try to attach to the namespace and name.
        fn try_attach(
            &self,
            namespace: SymbolRef,
            name: SymbolRef,
        ) -> Result<Attachment, AttachmentError> {
            unsafe { Attachment::try_attach($o(self), namespace, name) }
        }

        /// Subscribe to be attached to the namespace, name and optionally filter by class
        /// name.
        fn subscribe(
            &self,
            namespace: SymbolRef,
            name: SymbolRef,
            class_name: Option<SymbolRef>,
        ) -> Subscription {
            unsafe { Subscription::new($o(self), namespace, name, class_name) }
        }

        /// Broadcast a message from a registered object to any attached client objects.
        fn notify(&self, msg: SymbolRef) -> MaxResult<()> {
            crate::error::MaxError::from(
                unsafe {
                    max_sys::object_notify($o(self) as _, msg.inner(), std::ptr::null_mut()) as _
                },
                (),
            )
        }

        /// Indicate that an attribute has had a change (outside of its setter).
        ///
        /// # Arguments
        /// * `name` - the name of the attribute
        fn attr_touch_with_name<I: Into<SymbolRef>>(&self, name: I) -> MaxResult<()> {
            crate::attr::touch_with_name($o(self), name)
        }

        /// Indicate that an attribute has had a change (outside of its setter).
        ///
        /// # Arguments
        /// * `name` - the name of the attribute
        fn attr_try_touch_with_name<I: TryInto<SymbolRef>>(&self, name: I) -> MaxResult<()> {
            if let Ok(name) = name.try_into() {
                crate::attr::touch_with_name($o(self), name)
            } else {
                Err(MaxError::Generic)
            }
        }
    };
}

/// Indicates that your struct can be safely cast to a max_sys::t_object this means your struct
/// must be `#[repr(C)]` and have a `max_sys::t_object` as its first member.
pub unsafe trait MaxObj: Sized {
    fn max_obj(&self) -> *mut max_sys::t_object {
        unsafe { std::mem::transmute::<_, *mut max_sys::t_object>(self) }
    }

    /// Retrieve a byte offset for the start of your wrapped struct, relative to the start of the
    /// wrapper struct
    fn wrapped_byte_offset() -> usize;

    /// Retrieve a byte offset for the start of your wrapped struct, relative to the start of the
    /// wrapper struct
    fn obex_byte_offset() -> usize;

    impl_obj_methods!(Self::max_obj);
}

/// Indicates that your struct can be safely cast to a max_sys::t_pxobject this means your struct
/// must be `#[repr(C)]` and have a `max_sys::t_pxobject` as its first member.
pub unsafe trait MSPObj: Sized {
    fn msp_obj(&self) -> *mut max_sys::t_pxobject {
        unsafe { std::mem::transmute::<_, *mut max_sys::t_pxobject>(self) }
    }
    /// any MSP object can be safely cast to and used as a max_sys::t_object
    fn as_max_obj(&self) -> *mut max_sys::t_object {
        unsafe { std::mem::transmute::<_, *mut max_sys::t_object>(self.msp_obj()) }
    }

    /// Retrieve a byte offset for the start of your wrapped struct, relative to the start of the
    /// wrapper struct
    fn wrapped_byte_offset() -> usize;

    /// Retrieve a byte offset for the start of your wrapped struct, relative to the start of the
    /// wrapper struct
    fn obex_byte_offset() -> usize;

    impl_obj_methods!(Self::as_max_obj);
}

use std::ffi::CString;
use std::ops::{Deref, DerefMut};

/// Post a message to the Max console, associated with the given object.
pub fn post<T: Into<Vec<u8>>>(obj: *mut max_sys::t_object, msg: T) {
    unsafe {
        match CString::new(msg) {
            Ok(p) => max_sys::object_post(obj, p.as_ptr()),
            //TODO make CString below a const static
            Err(_) => self::error(obj, "failed to create CString"),
        }
    }
}

/// Post an error to the Max console, associated with the given object
pub fn error<T: Into<Vec<u8>>>(obj: *mut max_sys::t_object, msg: T) {
    unsafe {
        match CString::new(msg) {
            Ok(p) => max_sys::object_error(obj, p.as_ptr()),
            //TODO make CString below a const static
            Err(_) => {
                let m = CString::new("failed to create CString").unwrap();
                max_sys::object_error(obj, m.as_ptr())
            }
        }
    }
}

/// A smart pointer for an object that max allocated
pub struct ObjBox<T: MaxObj> {
    pub value: Option<Box<T>>, //option box so that we can drop if the value still exists
}

impl<T: MaxObj> ObjBox<T> {
    pub unsafe fn alloc(class: *mut max_sys::t_class) -> Self {
        //convert to t_object for debugging
        let value: *mut max_sys::t_object =
            std::mem::transmute::<_, _>(max_sys::object_alloc(class));

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

unsafe impl<T: MaxObj + Sync> Sync for ObjBox<T> {}
unsafe impl<T: MaxObj + Send> Send for ObjBox<T> {}
