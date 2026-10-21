//! `sysmem_*` and `strncpy_zero`.
//!
//! Like Max's, blocks are 16 byte aligned and `sysmem_freeptr` needs no size. The size lives in a
//! 16 byte header in front of the block, so that every block is released and resized with the
//! `Layout` it was allocated with (the old stub in `median/src/test/sysmem.rs` freed with the wrong
//! one, review M11).

use max_sys::{t_handle, t_ptr, t_ptr_size};
use std::alloc::{Layout, alloc, alloc_zeroed, dealloc, realloc};
use std::ffi::{c_char, c_long, c_void};

const HEADER: usize = 16;
const ALIGN: usize = 16;

fn layout_for(user_size: usize) -> Option<Layout> {
    Layout::from_size_align(user_size.checked_add(HEADER)?, ALIGN).ok()
}

fn new_block(size: usize, zeroed: bool) -> *mut c_void {
    let Some(layout) = layout_for(size) else {
        return std::ptr::null_mut();
    };
    // SAFETY: the layout has a non-zero size (the header)
    let base = unsafe {
        if zeroed {
            alloc_zeroed(layout)
        } else {
            alloc(layout)
        }
    };
    if base.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: `base` is `HEADER + size` bytes, aligned to 16
    unsafe {
        (base as *mut usize).write(size);
        base.add(HEADER) as *mut c_void
    }
}

/// The size of a block returned by `sysmem_newptr`, or `None` for a null pointer.
///
/// # Safety
/// `ptr` must be null or a live block from this module.
pub unsafe fn block_size(ptr: *const c_void) -> Option<usize> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees `ptr` is a live block, so the header precedes it
    Some(unsafe { ((ptr as *const u8).sub(HEADER) as *const usize).read() })
}

/// # Safety
/// As the SDK: none beyond what the returned block requires of its user.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysmem_newptr(size: t_ptr_size) -> t_ptr {
    new_block(size, false) as t_ptr
}

/// # Safety
/// As `sysmem_newptr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysmem_newptrclear(size: t_ptr_size) -> t_ptr {
    new_block(size, true) as t_ptr
}

/// # Safety
/// `ptr` must be null or a live block from `sysmem_newptr`/`sysmem_newptrclear`/`sysmem_resizeptr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysmem_ptrsize(ptr: *mut c_void) -> t_ptr_size {
    // SAFETY: the caller's contract
    unsafe { block_size(ptr) }.unwrap_or(0)
}

/// # Safety
/// As `sysmem_ptrsize`; the block must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysmem_freeptr(ptr: *mut c_void) {
    // SAFETY: the caller's contract
    let Some(size) = (unsafe { block_size(ptr) }) else {
        return;
    };
    let layout = layout_for(size).expect("the size was valid when the block was allocated");
    // SAFETY: the block was allocated by `new_block` with exactly this layout
    unsafe { dealloc((ptr as *mut u8).sub(HEADER), layout) };
}

/// The contents are kept up to the smaller of the old and new size. A null `ptr` allocates. On
/// failure null is returned and the old block is left alone.
///
/// # Safety
/// As `sysmem_ptrsize`; if the result is non-null the old pointer must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysmem_resizeptr(ptr: *mut c_void, newsize: t_ptr_size) -> t_ptr {
    // SAFETY: the caller's contract
    let Some(old) = (unsafe { block_size(ptr) }) else {
        return new_block(newsize, false) as t_ptr;
    };
    let Some(new_layout) = layout_for(newsize) else {
        return std::ptr::null_mut();
    };
    let old_layout = layout_for(old).expect("the size was valid when the block was allocated");
    // SAFETY: the block was allocated with `old_layout`; `new_layout.size()` is non-zero and does
    // not overflow when rounded up to the alignment (checked by `layout_for`)
    unsafe {
        let base = realloc((ptr as *mut u8).sub(HEADER), old_layout, new_layout.size());
        if base.is_null() {
            return std::ptr::null_mut();
        }
        (base as *mut usize).write(newsize);
        base.add(HEADER) as t_ptr
    }
}

/// A handle is a pointer to a master pointer to the data.
///
/// # Safety
/// As `sysmem_newptr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysmem_newhandle(size: t_ptr_size) -> t_handle {
    let master = new_block(size_of::<*mut c_char>(), true) as *mut *mut c_char;
    if master.is_null() {
        return std::ptr::null_mut();
    }
    let data = new_block(size, true) as *mut c_char;
    if data.is_null() {
        // SAFETY: `master` was just allocated by `new_block`
        unsafe { sysmem_freeptr(master as *mut c_void) };
        return std::ptr::null_mut();
    }
    // SAFETY: `master` is a live block of one pointer
    unsafe { master.write(data) };
    master
}

/// # Safety
/// `handle` must be null or a live handle from `sysmem_newhandle`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysmem_freehandle(handle: t_handle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: the caller's contract; both the data and the master pointer are blocks of ours
    unsafe {
        sysmem_freeptr(*handle as *mut c_void);
        sysmem_freeptr(handle as *mut c_void);
    }
}

/// Copies at most `size - 1` bytes of the C string `src`, always NUL terminating `dst`.
///
/// # Safety
/// `dst` must be valid for `size` bytes and `src` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncpy_zero(
    dst: *mut c_char,
    src: *const c_char,
    size: c_long,
) -> *mut c_char {
    if dst.is_null() || size <= 0 {
        return dst;
    }
    let max = size as usize - 1;
    let mut i = 0;
    // SAFETY: `i < size`, and `src` is read only up to and including its NUL
    unsafe {
        while i < max && !src.is_null() && *src.add(i) != 0 {
            *dst.add(i) = *src.add(i);
            i += 1;
        }
        *dst.add(i) = 0;
    }
    dst
}
