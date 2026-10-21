//! Object traits.

use crate::{
    error::{MaxError, MaxResult},
    notify::{Attachment, AttachmentError, Registration, RegistrationError, Subscription},
    symbol::SymbolRef,
};
use std::convert::TryInto;

macro_rules! impl_obj_methods {
    ($o:expr_2021) => {
        /// Post a message to max.
        fn post<M: Into<Vec<u8>>>(&self, msg: M) {
            // SAFETY: `$o(self)` is a pointer to this live object
            unsafe { crate::object::post($o(self), msg) }
        }

        /// Post an error message to max.
        fn post_error<M: Into<Vec<u8>>>(&self, msg: M) {
            // SAFETY: `$o(self)` is a pointer to this live object
            unsafe { crate::object::error($o(self), msg) }
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
            // SAFETY: `$o(self)` is a pointer to this live object
            unsafe { crate::attr::touch_with_name($o(self), name) }
        }

        /// Indicate that an attribute has had a change (outside of its setter).
        ///
        /// # Arguments
        /// * `name` - the name of the attribute
        fn attr_try_touch_with_name<I: TryInto<SymbolRef>>(&self, name: I) -> MaxResult<()> {
            if let Ok(name) = name.try_into() {
                // SAFETY: `$o(self)` is a pointer to this live object
                unsafe { crate::attr::touch_with_name($o(self), name) }
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

    impl_obj_methods!(Self::as_max_obj);
}

use std::ffi::CString;
use std::ops::Deref;
use std::ptr::NonNull;
use std::os::raw::c_char;

/// The printf format used to pass user text to the console; user text is never the format.
const FMT_S: *const c_char = c"%s".as_ptr();
const CSTRING_ERR: &[u8] = b"failed to create CString\0";

/// Post a message to the Max console, associated with the given object.
///
/// For a safe version see [`MaxObj::post`] and [`MSPObj::post`], which post for `self`.
///
/// # Safety
/// `obj` must point to a valid Max object (or be null, which Max treats as no object).
pub unsafe fn post<T: Into<Vec<u8>>>(obj: *mut max_sys::t_object, msg: T) {
    unsafe {
        match CString::new(msg) {
            Ok(p) => max_sys::object_post(obj, FMT_S, p.as_ptr()),
            Err(_) => max_sys::object_error(obj, FMT_S, CSTRING_ERR.as_ptr() as *const c_char),
        }
    }
}

/// Post an error to the Max console, associated with the given object
///
/// For a safe version see [`MaxObj::post_error`] and [`MSPObj::post_error`], which post for
/// `self`.
///
/// # Safety
/// `obj` must point to a valid Max object (or be null, which Max treats as no object).
pub unsafe fn error<T: Into<Vec<u8>>>(obj: *mut max_sys::t_object, msg: T) {
    unsafe {
        match CString::new(msg) {
            Ok(p) => max_sys::object_error(obj, FMT_S, p.as_ptr()),
            Err(_) => max_sys::object_error(obj, FMT_S, CSTRING_ERR.as_ptr() as *const c_char),
        }
    }
}

/// A smart pointer for an object that max allocated.
///
/// Dropping it frees the object with `object_free` (which runs the class's `free` method and
/// releases Max's memory). Use [`ObjBox::into_raw`] to hand ownership to Max instead.
///
/// It only gives shared access (`Deref`): Max may call into the object from other threads at any
/// time, and the object's header is owned by Max, so `&mut T` is never available.
pub struct ObjBox<T: MaxObj> {
    value: NonNull<T>,
}

impl<T: MaxObj> ObjBox<T> {
    /// Allocate an instance of `class` with `object_alloc`. Returns `None` if Max returns null.
    ///
    /// # Safety
    /// `class` must be a valid, registered class whose instances are laid out as `T`.
    pub unsafe fn alloc(class: *mut max_sys::t_class) -> Option<Self> {
        // SAFETY: the caller guarantees `class` is valid
        let value = unsafe { max_sys::object_alloc(class) } as *mut T;
        NonNull::new(value).map(|value| Self { value })
    }

    /// Take ownership of a Max object.
    ///
    /// # Safety
    /// `value` must be non-null and point to a valid Max object laid out as `T`, allocated by
    /// Max (`object_alloc`), that is not owned by anything else.
    pub unsafe fn from_raw(value: *mut T) -> Self {
        Self {
            value: unsafe { NonNull::new_unchecked(value) },
        }
    }

    /// The raw pointer to the object. Ownership stays with `self`.
    pub fn as_ptr(&self) -> *mut T {
        self.value.as_ptr()
    }

    /// Give up ownership without freeing the object, e.g. to return it from `new` to Max.
    pub fn into_raw(self) -> *mut T {
        let value = self.value.as_ptr();
        std::mem::forget(self);
        value
    }
}

impl<T: MaxObj> Deref for ObjBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: `value` is valid for as long as `self` owns the object
        unsafe { self.value.as_ref() }
    }
}

impl<T: MaxObj> Drop for ObjBox<T> {
    fn drop(&mut self) {
        // SAFETY: `value` is a Max object we own; Max runs the class's `free` method and
        // releases the memory.
        unsafe {
            max_sys::object_free(self.value.as_ptr() as *mut std::ffi::c_void);
        }
    }
}

unsafe impl<T: MaxObj + Sync> Sync for ObjBox<T> {}
unsafe impl<T: MaxObj + Send> Send for ObjBox<T> {}
