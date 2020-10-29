use std::ffi::c_void;

pub type MaxNew = unsafe extern "C" fn() -> *mut c_void;
pub type MaxFree<T> = unsafe extern "C" fn(obj: *mut T);
pub type MaxMethod = unsafe extern "C" fn(arg1: *mut c_void, ...) -> *mut c_void;

pub type B<T> = unsafe extern "C" fn(*const T);
pub type SelList<T> =
    unsafe extern "C" fn(*const T, *mut max_sys::t_symbol, i64, *const max_sys::t_atom);

include!(concat!(env!("OUT_DIR"), "/method-gen.rs"));
