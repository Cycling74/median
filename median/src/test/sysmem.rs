//! Test stubs for the `sysmem_*` functions.
//!
//! Like the real thing these return 16 byte aligned blocks and `sysmem_freeptr` needs no size. The
//! size is kept in a 16 byte header in front of the block so that the Rust allocation can be
//! released (and resized) with the layout it was created with.
use core::ffi::c_void;
use std::alloc::{alloc, dealloc, realloc, Layout};

const HEADER: usize = 16;
const ALIGN: usize = 16;

fn layout_for(user_size: usize) -> Option<Layout> {
    Layout::from_size_align(user_size.checked_add(HEADER)?, ALIGN).ok()
}

#[no_mangle]
pub unsafe extern "C" fn sysmem_newptr(size: max_sys::t_ptr_size) -> max_sys::t_ptr {
    let size = size as usize;
    let layout = match layout_for(size) {
        Some(l) => l,
        None => return core::ptr::null_mut(),
    };
    let base = unsafe { alloc(layout) };
    if base.is_null() {
        return base as _;
    }
    unsafe {
        (base as *mut usize).write(size);
        base.add(HEADER) as _
    }
}

#[no_mangle]
pub unsafe extern "C" fn sysmem_ptrsize(ptr: *mut c_void) -> max_sys::t_ptr_size {
    if ptr.is_null() {
        return 0;
    }
    unsafe { ((ptr as *mut u8).sub(HEADER) as *mut usize).read() as _ }
}

#[no_mangle]
pub unsafe extern "C" fn sysmem_resizeptr(
    ptr: *mut c_void,
    newsize: max_sys::t_ptr_size,
) -> max_sys::t_ptr {
    if ptr.is_null() {
        return unsafe { sysmem_newptr(newsize) };
    }
    let newsize = newsize as usize;
    unsafe {
        let base = (ptr as *mut u8).sub(HEADER);
        let old = (base as *mut usize).read();
        let new_layout = match layout_for(newsize) {
            Some(l) => l,
            None => return core::ptr::null_mut(),
        };
        let new_base = realloc(base, layout_for(old).unwrap(), new_layout.size());
        if new_base.is_null() {
            return new_base as _;
        }
        (new_base as *mut usize).write(newsize);
        new_base.add(HEADER) as _
    }
}

#[no_mangle]
pub unsafe extern "C" fn sysmem_freeptr(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let base = (ptr as *mut u8).sub(HEADER);
        let size = (base as *mut usize).read();
        dealloc(base, layout_for(size).unwrap());
    }
}
