//! Max object notifications and infrastructure.
use crate::symbol::SymbolRef;
use core::ffi::c_void;

/// A type that encapsulates the Max notify method signature, can be cast to
/// `MaxMethod` and supplied as the `"notify"` class method for a class.
pub type NotifyMethod<T> = unsafe extern "C" fn(
    x: *mut T,
    sender_name: *mut max_sys::t_symbol,
    message: *mut max_sys::t_symbol,
    sender: *mut c_void,
    data: *mut c_void,
);

/// Encapsulated notification data.
pub struct Notification {
    sender_name: SymbolRef,
    message: SymbolRef,
    sender: *mut c_void,
    data: *mut c_void,
}

/// A max object registration object.
pub struct Registration {
    inner: *mut core::ffi::c_void,
}

/// A max object subscription object.
pub struct Subscription {
    namespace: SymbolRef,
    name: SymbolRef,
    client: *mut core::ffi::c_void,
    class_name: SymbolRef,
}

/// A max object notification attachment.
pub struct Attachment {
    namespace: SymbolRef,
    name: SymbolRef,
    client: *mut core::ffi::c_void,
}

/// Errors registering.
pub enum RegistrationError {
    NameCollision,
}

/// Errors attaching.
pub enum AttachmentError {
    NotFound,
}

impl Notification {
    pub fn new(
        sender_name: *mut max_sys::t_symbol,
        message: *mut max_sys::t_symbol,
        sender: *mut c_void,
        data: *mut c_void,
    ) -> Self {
        Self {
            sender_name: sender_name.into(),
            message: message.into(),
            sender,
            data,
        }
    }

    /// Get the notification message.
    pub fn message(&self) -> &SymbolRef {
        &self.message
    }

    /// Get a pointer to the sender of the notification.
    pub fn sender(&self) -> *mut c_void {
        self.sender
    }

    /// Get the name of the sender of the notification.
    pub fn sender_name(&self) -> &SymbolRef {
        &self.sender_name
    }

    /// Get the data from the notification.
    ///
    /// # Remarks
    /// * Might be null.
    pub fn data(&self) -> *mut c_void {
        self.data
    }
}

impl Registration {
    /// Try go register the given object with the namespace and name.
    pub unsafe fn try_register(
        obj: *mut max_sys::t_object,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<Registration, RegistrationError> {
        if max_sys::object_findregistered(namespace.inner(), name.inner()).is_null() {
            let inner = max_sys::object_register(namespace.inner(), name.inner(), obj as _);
            assert!(!inner.is_null());
            Ok(Self { inner })
        } else {
            Err(RegistrationError::NameCollision)
        }
    }
}

impl Subscription {
    /// Subscribe the given client to be attached to the namespace, name and optional class_name.
    pub unsafe fn new(
        client: *mut max_sys::t_object,
        namespace: SymbolRef,
        name: SymbolRef,
        class_name: Option<SymbolRef>,
    ) -> Self {
        let client: *mut core::ffi::c_void = client as _;
        let class_name = class_name.unwrap_or_default();
        let _ =
            max_sys::object_subscribe(namespace.inner(), name.inner(), class_name.inner(), client);
        Self {
            namespace,
            name,
            client,
            class_name,
        }
    }
}

impl Attachment {
    /// Try to attach the given client to the namespace and name.
    pub unsafe fn try_attach(
        client: *mut max_sys::t_object,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<Self, AttachmentError> {
        let p = max_sys::object_attach(namespace.inner(), name.inner(), client as _);
        if p.is_null() {
            Err(AttachmentError::NotFound)
        } else {
            Ok(Attachment {
                namespace,
                name,
                client: client as _,
            })
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        unsafe {
            let _ = max_sys::object_unregister(self.inner);
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        unsafe {
            let _ = max_sys::object_unsubscribe(
                self.namespace.inner(),
                self.name.inner(),
                self.class_name.inner(),
                self.client,
            );
        }
    }
}

impl Drop for Attachment {
    fn drop(&mut self) {
        unsafe {
            let _ =
                max_sys::object_detach(self.namespace.inner(), self.name.inner(), self.client as _);
        }
    }
}

unsafe impl Send for Registration {}
unsafe impl Send for Subscription {}
unsafe impl Send for Attachment {}
