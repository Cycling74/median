//! Memory allocation.

use std::alloc::{GlobalAlloc, Layout};

/// An allocator that can be used as the Rust global allocator.
///
/// Rust allocations will use Max's sysmem_newptr and sysmem_freeptr to allocate and deallocate
/// memory.
///
/// `sysmem_newptr` only guarantees 16 byte alignment. Layouts that need more are served by
/// over-allocating and storing the pointer `sysmem_newptr` returned just in front of the aligned
/// block, so `dealloc` can give it back. `realloc` of a layout with alignment up to 16 uses
/// `sysmem_resizeptr` (which, like `realloc`, is assumed to leave the original block valid when
/// it fails).
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

/// Alignment `sysmem_newptr` guarantees.
const SYSMEM_ALIGN: usize = 16;
/// Room for the original pointer stored in front of an over-aligned block.
const HEADER: usize = core::mem::size_of::<*mut u8>();

unsafe impl GlobalAlloc for MaxAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= SYSMEM_ALIGN {
            return unsafe { max_sys::sysmem_newptr(layout.size() as _) as _ };
        }
        // The block starts at an address that is a multiple of 16, so rounding `raw + HEADER` up to
        // `align` moves at most `HEADER + align - 1` bytes. The aligned address is at least
        // `HEADER` past `raw`, which leaves room to store `raw` right before it.
        let align = layout.align();
        let total = match layout
            .size()
            .checked_add(align)
            .and_then(|n| n.checked_add(HEADER))
        {
            Some(t) => t,
            None => return core::ptr::null_mut(),
        };
        let raw = unsafe { max_sys::sysmem_newptr(total as _) as *mut u8 };
        if raw.is_null() {
            return raw;
        }
        let addr = raw as usize;
        let offset = (addr + HEADER).next_multiple_of(align) - addr;
        // SAFETY: offset + size <= total, and `offset - HEADER` is a multiple of 8 (`align` is a
        // power of two > 16, `raw` is 16 aligned) so the pointer slot is aligned.
        unsafe {
            let aligned = raw.add(offset);
            (aligned as *mut *mut u8).sub(1).write(raw);
            aligned
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if layout.align() <= SYSMEM_ALIGN {
            unsafe { max_sys::sysmem_freeptr(ptr as _) };
        } else {
            // SAFETY: `alloc` stored the original pointer in front of the block
            unsafe {
                let raw = (ptr as *mut *mut u8).sub(1).read();
                max_sys::sysmem_freeptr(raw as _);
            }
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if layout.align() <= SYSMEM_ALIGN {
            return unsafe { max_sys::sysmem_resizeptr(ptr as _, new_size as _) as _ };
        }
        // SAFETY: the caller guarantees `new_size` is valid for `layout.align()`
        unsafe {
            let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
            let new_ptr = self.alloc(new_layout);
            if !new_ptr.is_null() {
                core::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }
            new_ptr
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn small_alignment() {
        let a = MaxAllocator;
        unsafe {
            let layout = Layout::from_size_align(24, 8).unwrap();
            let p = a.alloc(layout);
            assert!(!p.is_null());
            assert_eq!(0, p as usize % 8);
            for i in 0..24 {
                p.add(i).write(i as u8);
            }
            let p = a.realloc(p, layout, 100);
            assert!(!p.is_null());
            for i in 0..24 {
                assert_eq!(i as u8, p.add(i).read());
            }
            a.dealloc(p, Layout::from_size_align(100, 8).unwrap());
        }
    }

    #[test]
    fn over_aligned() {
        let a = MaxAllocator;
        for &align in &[32usize, 64, 128, 256, 4096] {
            for &size in &[1usize, 17, 200] {
                unsafe {
                    let layout = Layout::from_size_align(size, align).unwrap();
                    let p = a.alloc(layout);
                    assert!(!p.is_null());
                    assert_eq!(0, p as usize % align, "align {}", align);
                    for i in 0..size {
                        p.add(i).write(i as u8);
                    }
                    //grow, content is preserved and alignment kept
                    let p = a.realloc(p, layout, size * 2);
                    assert!(!p.is_null());
                    assert_eq!(0, p as usize % align, "align {}", align);
                    for i in 0..size {
                        assert_eq!(i as u8, p.add(i).read());
                    }
                    //shrink
                    let p = a.realloc(p, Layout::from_size_align(size * 2, align).unwrap(), 1);
                    assert!(!p.is_null());
                    assert_eq!(0, p.read());
                    a.dealloc(p, Layout::from_size_align(1, align).unwrap());
                }
            }
        }
    }
}
