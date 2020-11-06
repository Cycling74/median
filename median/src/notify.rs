use core::ffi::c_void;

pub type NotifyMethod<T> = unsafe extern "C" fn(
    x: *mut T,
    sender_name: *mut max_sys::t_symbol,
    msg: *mut max_sys::t_symbol,
    sender: *mut c_void,
    data: *mut c_void,
);
