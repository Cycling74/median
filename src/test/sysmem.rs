use core::ffi::c_void;
use std::os::raw::c_long;

#[no_mangle]
pub unsafe extern "C" fn sysmem_newptr(size: c_long) -> max_sys::t_ptr {
    let vec: Vec<i8> = vec![0; size as _];
    let mut slice = vec.into_boxed_slice();
    let ptr = slice.as_mut_ptr();
    std::mem::forget(slice);
    ptr
}

#[no_mangle]
pub unsafe extern "C" fn sysmem_freeptr(ptr: *mut c_void) {
    Box::from_raw(ptr);
}
