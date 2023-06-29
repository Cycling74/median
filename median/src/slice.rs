//! Max memory allocated slice
use std::convert::{From, Into};
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
            let ptr = max_sys::sysmem_newptr((std::mem::size_of::<T>() * len) as _);
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

    pub fn from_raw_parts_mut(ptr: *mut T, len: usize) -> Self {
        assert!(
            !(len > 0 && ptr.is_null()),
            "cannot have null ptr and non zero length"
        );
        Self {
            inner: unsafe { slice::from_raw_parts_mut(ptr, len) },
        }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn as_ref(&self) -> &[T] {
        self.inner
    }

    pub fn as_mut(&mut self) -> &mut [T] {
        self.inner
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

impl<T, I, U> From<U> for Slice<T>
where
    I: Into<T>,
    U: ExactSizeIterator<Item = I>,
{
    fn from(iter: U) -> Self {
        unsafe {
            let len = iter.len();
            let ptr = max_sys::sysmem_newptr((std::mem::size_of::<T>() * len) as _);
            if ptr.is_null() {
                panic!("max_sys::sysmem_newptr returned null");
            }
            let inner = slice::from_raw_parts_mut(std::mem::transmute::<_, *mut T>(ptr), len);
            for (i, o) in inner.iter_mut().zip(iter) {
                *i = o.into();
            }
            Self { inner }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::atom::Atom;
    use std::convert::From;
    #[test]
    fn can_create() {
        let s: Slice<Atom> = Slice::from([0i64, 1i64].iter());
        assert_eq!(2, s.len());

        let (p, l) = s.into_raw();
        assert!(!p.is_null());
        assert_eq!(2, l);

        let s = Slice::from_raw_parts_mut(p, l);
        assert_eq!(2, s.len());
    }
}
