//! Memory allocation.

use std::alloc::{GlobalAlloc, Layout};

/// An allocator that can be used as the Rust global allocator.
///
/// Rust allocations will use Max's sysmem_newptr and sysmem_freeptr to allocate and deallocate
/// memory.
///
/// See the [global allocators documentation](https://doc.rust-lang.org/edition-guide/rust-2018/platform-and-target-support/global-allocators.html)
/// for more details.
///
/// # Examples
///
/// ```no_run
/// use median::alloc::MaxAllocator;
///
/// #[global_allocator]
/// static GLOBAL: MaxAllocator = MaxAllocator;
///
/// pub unsafe extern "C" fn ext_main(_r: *mut core::ffi::c_void) {
///   //..register your class
/// }
/// ```
pub struct MaxAllocator;

unsafe impl GlobalAlloc for MaxAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        //XXX do we need to worry about alignment?
        max_sys::sysmem_newptr(layout.size() as _) as _
    }
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        max_sys::sysmem_freeptr(ptr as _);
    }
}
