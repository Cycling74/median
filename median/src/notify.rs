use crate::symbol::SymbolRef;
use core::ffi::c_void;

pub type NotifyMethod<T> = unsafe extern "C" fn(
    x: *mut T,
    sender_name: *mut max_sys::t_symbol,
    message: *mut max_sys::t_symbol,
    sender: *mut c_void,
    data: *mut c_void,
);

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
    pub fn sender(&self) -> *mut c_void {
        self.sender
    }

    pub fn sender_name(&self) -> &SymbolRef {
        &self.sender_name
    }

    pub fn data(&self) -> *mut c_void {
        self.data
    }

    pub fn message(&self) -> &SymbolRef {
        &self.message
    }
}
