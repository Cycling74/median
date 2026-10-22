//! Object handles and traits.

use crate::{
    atom::Atom,
    error::{MaxError, MaxResult},
    notify::{Attachment, AttachmentError, Registration, RegistrationError, Subscription},
    symbol::SymbolRef,
    wrapper::DeferMethodWrapped,
};
use std::convert::TryInto;
use std::fmt::Display;
use std::marker::PhantomData;

/// A handle to the Max object that wraps your struct.
///
/// Get one from the builder in your `new` (`builder.obj()`) and store it in your struct. It is how
/// the object posts to the Max console, touches its attributes, registers for notifications and
/// so on. Because it is only ever reachable from `&self`, all of its methods take `&self`.
///
/// ```
/// use median::object::ObjRef;
///
/// struct MyObj {
///     obj: ObjRef,
/// }
///
/// impl MyObj {
///     fn bang(&self) {
///         self.obj.post("bang");
///         median::object_post!(&self.obj, "or with a format: {}", 2084);
///     }
/// }
/// ```
///
/// # Limits
///
/// * `ObjRef` is deliberately not `Clone` or `Copy`, so it can't be duplicated and handed to
///   something that outlives the object. It is `Sync` (the wrapped struct must be) but
///   deliberately **not** `Send`, which keeps it out of statics such as
///   `static Mutex<Option<ObjRef>>`.
/// * The handle is valid until Max frees the object, which is after your struct is dropped.
///   The borrow checker enforces that for normal use, but it can't stop you from deliberately
///   leaking the handle out of the object, for example with `Box::leak` (which gives a
///   `&'static ObjRef`) or by parking it in a `thread_local!`. **Don't do that**: using the
///   handle after the object has been deleted is a use-after-free.
/// * The handle exists before your struct does: the builder hands it out while `new` is still
///   running. A call on it during `new` that makes Max call back into the object (touching an
///   attribute, notifying, deferring from the main thread) reaches a method of an object that
///   is not built yet. The wrapper refuses such a call with a console error, without running
///   your code, rather than letting it reach the half-built object, so don't do that in `new`.
///
/// `ObjRef` is not `Clone`:
///
/// ```compile_fail,E0277
/// fn needs_clone<T: Clone>() {}
/// needs_clone::<median::object::ObjRef>();
/// ```
///
/// `ObjRef` is not `Send`:
///
/// ```compile_fail,E0277
/// fn needs_send<T: Send>() {}
/// needs_send::<median::object::ObjRef>();
/// ```
///
/// (and, so that the two examples above can't be failing for some other reason, it is `Sync`:)
///
/// ```
/// fn needs_sync<T: Sync>() {}
/// needs_sync::<median::object::ObjRef>();
/// ```
pub struct ObjRef(
    std::ptr::NonNull<max_sys::t_object>,
    // makes the type `!Send` and `!Sync`, `Sync` is added back below
    PhantomData<*const ()>,
);

// SAFETY: Max may call into an object from any thread, and everything `ObjRef` does is a Max SDK
// call on the object that Max documents as callable from any thread (post, error, notify,
// attr_touch, ...). It is deliberately not `Send`, see the type's documentation.
unsafe impl Sync for ObjRef {}

impl ObjRef {
    /// Wrap a pointer to a Max object.
    ///
    /// # Safety
    /// `obj` must point to a valid Max object that stays valid, and is not freed, for as long as
    /// the returned handle (or any reference to it) is used. In practice that means the handle
    /// must be owned by the object it points to, or be dropped before the object is freed.
    pub unsafe fn from_raw(obj: std::ptr::NonNull<max_sys::t_object>) -> Self {
        Self(obj, PhantomData)
    }

    /// The raw pointer to the Max object.
    ///
    /// The pointer is only valid for as long as the object is alive.
    pub fn as_ptr(&self) -> *mut max_sys::t_object {
        self.0.as_ptr()
    }

    /// Post a message to the Max console, associated with this object.
    pub fn post(&self, msg: impl Display) {
        // SAFETY: `self` is a live object, see `from_raw`
        unsafe { post(self.as_ptr(), msg.to_string()) }
    }

    /// Post an error message to the Max console, associated with this object.
    pub fn error(&self, msg: impl Display) {
        // SAFETY: `self` is a live object, see `from_raw`
        unsafe { error(self.as_ptr(), msg.to_string()) }
    }

    /// Post a warning to the Max console, associated with this object.
    pub fn warn(&self, msg: impl Display) {
        // SAFETY: `self` is a live object, see `from_raw`
        unsafe { warn(self.as_ptr(), msg.to_string()) }
    }

    /// Get the index of the proxy inlet that received the message currently being handled.
    ///
    /// Only meaningful inside a message method that Max invoked on this object, and for an
    /// object that has proxy inlets (see [`crate::inlet::Inlet::proxy`]). Anywhere else the
    /// value is whatever Max last recorded (typically 0).
    pub fn proxy_inlet(&self) -> usize {
        // SAFETY: `self` is a live object, see `from_raw`
        unsafe { max_sys::proxy_getinlet(self.as_ptr()) as _ }
    }

    /// Try to register this object with the given namespace and name.
    pub fn try_register(
        &self,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<Registration, RegistrationError> {
        // SAFETY: `self` is a live object, see `from_raw`
        unsafe { Registration::try_register(self.as_ptr(), namespace, name) }
    }

    /// Try to attach to the namespace and name.
    pub fn try_attach(
        &self,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<Attachment, AttachmentError> {
        // SAFETY: `self` is a live object, see `from_raw`
        unsafe { Attachment::try_attach(self.as_ptr(), namespace, name) }
    }

    /// Subscribe to be attached to the namespace, name and optionally filter by class name.
    pub fn subscribe(
        &self,
        namespace: SymbolRef,
        name: SymbolRef,
        class_name: Option<SymbolRef>,
    ) -> Subscription {
        // SAFETY: `self` is a live object, see `from_raw`
        unsafe { Subscription::new(self.as_ptr(), namespace, name, class_name) }
    }

    /// Broadcast a message from a registered object to any attached client objects.
    pub fn notify(&self, msg: SymbolRef) -> MaxResult<()> {
        MaxError::from(
            // SAFETY: `self` is a live object, see `from_raw`
            unsafe {
                max_sys::object_notify(self.as_ptr() as _, msg.inner(), std::ptr::null_mut()) as _
            },
            (),
        )
    }

    /// Indicate that an attribute has had a change (outside of its setter).
    ///
    /// # Arguments
    /// * `name` - the name of the attribute
    pub fn attr_touch_with_name<I: Into<SymbolRef>>(&self, name: I) -> MaxResult<()> {
        // SAFETY: `self` is a live object, see `from_raw`
        unsafe { crate::attr::touch_with_name(self.as_ptr(), name) }
    }

    /// Indicate that an attribute has had a change (outside of its setter).
    ///
    /// # Arguments
    /// * `name` - the name of the attribute
    pub fn attr_try_touch_with_name<I: TryInto<SymbolRef>>(&self, name: I) -> MaxResult<()> {
        if let Ok(name) = name.try_into() {
            // SAFETY: `self` is a live object, see `from_raw`
            unsafe { crate::attr::touch_with_name(self.as_ptr(), name) }
        } else {
            Err(MaxError::Generic)
        }
    }

    /// Defer a trampoline of this object's wrapper to the main thread, if (and only if) you are
    /// calling from the scheduler thread. See [`crate::thread::defer`].
    ///
    /// # Safety
    /// * `W` must be the wrapper type of this object (`MaxObjWrapper<T>` or `MSPObjWrapper<T>`
    ///   for the `T` that holds this handle) and `meth` must be a trampoline of it (see the
    ///   `defer_tramp` attribute).
    /// * The object must stay alive until the deferred call has run: Max does not keep it alive,
    ///   and the deferred call receives a pointer to it. For example, the object must not be
    ///   freed (the box deleted) before then.
    /// * The object must be fully constructed, so don't call this from inside `new`. From the
    ///   main thread `defer` runs `meth` immediately, which calls back into the half-built
    ///   object. (The wrapper refuses that call with a console error instead of running the
    ///   method, but it is still not what you wanted.)
    pub unsafe fn defer<W>(&self, meth: DeferMethodWrapped<W>, sym: SymbolRef, atoms: &[Atom]) {
        // SAFETY: `self` is a live object, `meth` is a trampoline for it and the caller
        // guarantees that the object outlives the deferred call
        unsafe {
            crate::thread::defer(
                std::mem::transmute::<DeferMethodWrapped<W>, crate::thread::DeferMethod>(meth),
                self.as_ptr(),
                sym,
                atoms,
            );
        }
    }

    /// Defer a trampoline of this object's wrapper to the back of the main thread's queue. Always
    /// defers. See [`crate::thread::defer_low`].
    ///
    /// # Safety
    /// As [`ObjRef::defer`]: `W` must be this object's wrapper type, `meth` a trampoline of it,
    /// the object must outlive the deferred call, and it must be fully constructed (not called
    /// from inside `new`).
    pub unsafe fn defer_low<W>(&self, meth: DeferMethodWrapped<W>, sym: SymbolRef, atoms: &[Atom]) {
        // SAFETY: as for `defer`
        unsafe {
            crate::thread::defer_low(
                std::mem::transmute::<DeferMethodWrapped<W>, crate::thread::DeferMethod>(meth),
                self.as_ptr(),
                sym,
                atoms,
            );
        }
    }
}

/// Indicates that your struct can be safely cast to a max_sys::t_object this means your struct
/// must be `#[repr(C)]` and have a `max_sys::t_object` as its first member.
///
/// This is implemented for the wrapper types (and not for your own struct): to post, touch
/// attributes and so on from your own struct, keep an [`ObjRef`] in it.
pub unsafe trait MaxObj: Sized {
    fn max_obj(&self) -> *mut max_sys::t_object {
        unsafe { std::mem::transmute::<_, *mut max_sys::t_object>(self) }
    }
}

/// Indicates that your struct can be safely cast to a max_sys::t_pxobject this means your struct
/// must be `#[repr(C)]` and have a `max_sys::t_pxobject` as its first member.
///
/// This is implemented for the wrapper types (and not for your own struct): to post, touch
/// attributes and so on from your own struct, keep an [`ObjRef`] in it.
pub unsafe trait MSPObj: Sized {
    fn msp_obj(&self) -> *mut max_sys::t_pxobject {
        unsafe { std::mem::transmute::<_, *mut max_sys::t_pxobject>(self) }
    }
    /// any MSP object can be safely cast to and used as a max_sys::t_object
    fn as_max_obj(&self) -> *mut max_sys::t_object {
        unsafe { std::mem::transmute::<_, *mut max_sys::t_object>(self.msp_obj()) }
    }
}

use std::ffi::{CStr, CString};
use std::ops::Deref;
use std::ptr::NonNull;
use std::os::raw::c_char;

/// The printf format used to pass user text to the console; user text is never the format.
const FMT_S: *const c_char = c"%s".as_ptr();
const CSTRING_ERR: &CStr = c"failed to create CString";

/// Post a message to the Max console, associated with the given object.
///
/// For a safe version see [`ObjRef::post`].
///
/// # Safety
/// `obj` must point to a valid Max object (or be null, which Max treats as no object).
pub unsafe fn post<T: Into<Vec<u8>>>(obj: *mut max_sys::t_object, msg: T) {
    unsafe {
        match CString::new(msg) {
            Ok(p) => max_sys::object_post(obj, FMT_S, p.as_ptr()),
            Err(_) => max_sys::object_error(obj, FMT_S, CSTRING_ERR.as_ptr()),
        }
    }
}

/// Post an error to the Max console, associated with the given object
///
/// For a safe version see [`ObjRef::error`].
///
/// # Safety
/// `obj` must point to a valid Max object (or be null, which Max treats as no object).
pub unsafe fn error<T: Into<Vec<u8>>>(obj: *mut max_sys::t_object, msg: T) {
    unsafe {
        match CString::new(msg) {
            Ok(p) => max_sys::object_error(obj, FMT_S, p.as_ptr()),
            Err(_) => max_sys::object_error(obj, FMT_S, CSTRING_ERR.as_ptr()),
        }
    }
}

/// Post a warning to the Max console, associated with the given object
///
/// For a safe version see [`ObjRef::warn`].
///
/// # Safety
/// `obj` must point to a valid Max object (or be null, which Max treats as no object).
pub unsafe fn warn<T: Into<Vec<u8>>>(obj: *mut max_sys::t_object, msg: T) {
    unsafe {
        match CString::new(msg) {
            Ok(p) => max_sys::object_warn(obj, FMT_S, p.as_ptr()),
            Err(_) => max_sys::object_error(obj, FMT_S, CSTRING_ERR.as_ptr()),
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
