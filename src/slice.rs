use std::convert::From;
use std::slice;

///A slice allocated and freed using max_sys
pub struct Slice<T: 'static + Sized> {
    inner: &'static mut [T],
}

impl<T> Slice<T>
where
    T: 'static + Sized + Default,
{
    pub fn new_with_length(len: usize) -> Self {
        let inner = unsafe {
            let ptr = max_sys::sysmem_newptr((std::mem::size_of::<T>() * len) as i64);
            if ptr.is_null() {
                panic!("max_sys::sysmem_newptr returned null");
            }
            let slice = slice::from_raw_parts_mut(std::mem::transmute::<_, *mut T>(ptr), len);
            for v in slice.iter_mut() {
                *v = Default::default();
            }
            slice
        };
        Self { inner }
    }
}

impl<T> Slice<T> {
    /// convert into raw ptr and size, most likely to be passed to, and managed by, max_sys
    pub fn into_raw(self) -> (*mut T, usize) {
        let len = self.inner.len();
        let ptr = self.inner.as_mut_ptr();
        std::mem::forget(self);
        (ptr, len)
    }
}

impl<T> Default for Slice<T>
where
    T: 'static + Sized,
{
    fn default() -> Self {
        unsafe {
            Self {
                inner: slice::from_raw_parts_mut(std::ptr::null_mut(), 0),
            }
        }
    }
}

impl<T> Drop for Slice<T> {
    fn drop(&mut self) {
        if self.inner.len() > 0 {
            unsafe {
                max_sys::sysmem_freeptr(self.inner.as_mut_ptr() as _);
                self.inner = slice::from_raw_parts_mut(std::ptr::null_mut(), 0);
            }
        }
    }
}

impl<T, U> From<U> for Slice<T>
where
    U: ExactSizeIterator<Item = T>,
{
    fn from(iter: U) -> Self {
        unsafe {
            let len = iter.len();
            let ptr = max_sys::sysmem_newptr((std::mem::size_of::<T>() * len) as i64);
            if ptr.is_null() {
                panic!("max_sys::sysmem_newptr returned null");
            }
            let inner = slice::from_raw_parts_mut(std::mem::transmute::<_, *mut T>(ptr), len);
            for (i, o) in inner.iter_mut().zip(iter) {
                *i = o;
            }
            Self { inner }
        }
    }
}
