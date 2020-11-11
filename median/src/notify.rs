//! Wrapper for Max object notifications.
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

/// Wrap a notification into an object.
pub struct Notification {
    sender_name: SymbolRef,
    message: SymbolRef,
    sender: *mut c_void,
    data: *mut c_void,
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
