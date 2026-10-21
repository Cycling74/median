//! Max memory allocated slice
use std::convert::{From, Into, TryFrom};
use std::ffi::c_void;
use std::mem::{align_of, size_of};
use std::ptr;
use std::slice;

/// Alignment `sysmem_newptr` guarantees (malloc alignment on the platforms Max runs on).
const SYSMEM_ALIGN: usize = 16;

///A slice allocated and freed using max_sys
///
///`T` must not require alignment greater than 16 bytes, which is all `sysmem_newptr` guarantees.
///This is checked at compile time, when a `Slice<T>` for such a `T` is first constructed.
///
///The constructors that allocate (`new_with_length`, `From<U>`) panic if the size of the
///allocation overflows or if `sysmem_newptr` fails.
pub struct Slice<T: 'static + Sized> {
    inner: &'static mut [T],
}

/// Frees a `sysmem_newptr` block on drop, including during unwinding.
struct FreePtr(*mut c_void);

impl Drop for FreePtr {
    fn drop(&mut self) {
        // SAFETY: only constructed around pointers obtained from `sysmem_newptr`
        unsafe { max_sys::sysmem_freeptr(self.0) }
    }
}

/// Allocation being filled in. Dropping it drops the elements pushed so far and frees the memory,
/// so a panicking `Default` / `Into` / iterator does not leak or free uninitialised elements.
struct Builder<T> {
    ptr: *mut T,
    cap: usize,
    len: usize,
}

impl<T> Builder<T> {
    /// `cap` must not be 0.
    fn with_capacity(cap: usize) -> Self {
        let bytes = size_of::<T>()
            .checked_mul(cap)
            .filter(|b| *b <= isize::MAX as usize)
            .expect("Slice size overflow");
        //zero sized types still get a (one byte) allocation so that there is something to free
        let bytes = max_sys::t_ptr_size::try_from(bytes.max(1)).expect("Slice size overflow");
        let ptr = unsafe { max_sys::sysmem_newptr(bytes) };
        if ptr.is_null() {
            panic!("max_sys::sysmem_newptr returned null");
        }
        Self {
            ptr: ptr as *mut T,
            cap,
            len: 0,
        }
    }

    /// Returns false (dropping nothing) if the builder is full.
    fn push(&mut self, v: T) -> bool {
        if self.len == self.cap {
            return false;
        }
        // SAFETY: `len < cap` and the allocation holds `cap` `T`s, aligned (checked statically)
        unsafe { ptr::write(self.ptr.add(self.len), v) };
        self.len += 1;
        true
    }

    fn finish(self) -> Slice<T> {
        if self.len == 0 {
            //dropped here, frees the allocation
            return Slice::default();
        }
        let (ptr, len) = (self.ptr, self.len);
        std::mem::forget(self);
        Slice {
            // SAFETY: `len` elements are initialised and the allocation is ours
            inner: unsafe { slice::from_raw_parts_mut(ptr, len) },
        }
    }
}

impl<T> Drop for Builder<T> {
    fn drop(&mut self) {
        let _free = FreePtr(self.ptr as _);
        // SAFETY: the first `len` elements are initialised
        unsafe { ptr::drop_in_place(ptr::slice_from_raw_parts_mut(self.ptr, self.len)) };
    }
}

impl<T> Slice<T>
where
    T: 'static + Sized,
{
    //evaluated when a Slice<T> is constructed (post monomorphisation compile error)
    const ALIGN_OK: () = assert!(
        align_of::<T>() <= SYSMEM_ALIGN,
        "Slice<T> requires align_of::<T>() <= 16"
    );
}

impl<T> Slice<T>
where
    T: 'static + Sized + Default,
{
    /// Allocate a slice of `len` default values. A `len` of 0 yields an empty slice.
    pub fn new_with_length(len: usize) -> Self {
        let () = Self::ALIGN_OK;
        if len == 0 {
            return Self::default();
        }
        let mut b = Builder::with_capacity(len);
        for _ in 0..len {
            b.push(Default::default());
        }
        b.finish()
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

    /// Create a `Slice` from a raw pointer and length, taking ownership of the allocation.
    ///
    /// A `len` of 0 yields an empty slice and `ptr` is ignored (it is not freed).
    ///
    /// # Safety
    /// If `len > 0`:
    /// * `ptr` must be non-null, aligned, and point to `len` initialized `T`s allocated with
    ///   `max_sys::sysmem_newptr` (it is freed with `sysmem_freeptr` on drop, after the elements
    ///   are dropped), for the `'static` lifetime of the slice.
    /// * Nothing else may access or free that memory while the `Slice` exists.
    pub unsafe fn from_raw_parts_mut(ptr: *mut T, len: usize) -> Self {
        let () = Self::ALIGN_OK;
        if len == 0 {
            return Self::default();
        }
        Self {
            // SAFETY: guaranteed by the caller
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
        Self { inner: &mut [] }
    }
}

impl<T> Drop for Slice<T> {
    fn drop(&mut self) {
        if !self.inner.is_empty() {
            let inner = std::mem::take(&mut self.inner);
            let _free = FreePtr(inner.as_mut_ptr() as _);
            // SAFETY: the elements are initialised and owned by this slice. The allocation is freed
            // by `_free` even if an element's drop panics.
            unsafe { ptr::drop_in_place(inner as *mut [T]) };
        }
    }
}

/// Collect the items of `iter` into Max allocated memory.
///
/// `ExactSizeIterator::len` is only used to size the allocation; it is not trusted. The slice
/// holds as many items as the iterator actually yields, up to that length.
impl<T, I, U> From<U> for Slice<T>
where
    T: 'static,
    I: Into<T>,
    U: ExactSizeIterator<Item = I>,
{
    fn from(iter: U) -> Self {
        let () = Self::ALIGN_OK;
        let len = iter.len();
        if len == 0 {
            return Self::default();
        }
        let mut b = Builder::with_capacity(len);
        for item in iter {
            if !b.push(item.into()) {
                break;
            }
        }
        b.finish()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::atom::Atom;
    use std::convert::From;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// An `ExactSizeIterator` whose `len()` does not match what it yields.
    struct Liar<I> {
        inner: I,
        claimed: usize,
    }

    impl<I: Iterator> Iterator for Liar<I> {
        type Item = I::Item;
        fn next(&mut self) -> Option<I::Item> {
            self.inner.next()
        }
    }

    impl<I: Iterator> ExactSizeIterator for Liar<I> {
        fn len(&self) -> usize {
            self.claimed
        }
    }

    struct CountDrop(Arc<AtomicUsize>);
    impl Drop for CountDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn can_create() {
        let s: Slice<Atom> = Slice::from([0i64, 1i64].iter());
        assert_eq!(2, s.len());

        let (p, l) = s.into_raw();
        assert!(!p.is_null());
        assert_eq!(2, l);

        let s = unsafe { Slice::from_raw_parts_mut(p, l) };
        assert_eq!(2, s.len());

        let s: Slice<Atom> = Default::default();
        assert_eq!(0, s.len());
    }

    #[test]
    fn new_with_length_drop_type() {
        let mut s = Slice::<String>::new_with_length(3);
        assert_eq!(3, s.len());
        assert!(s.as_ref().iter().all(|v| v.is_empty()));
        s.as_mut()[1].push_str("hello");
        assert_eq!("hello", s.as_ref()[1]);
        drop(s);

        let s = Slice::<String>::new_with_length(0);
        assert_eq!(0, s.len());
    }

    #[test]
    fn from_iter_drop_type() {
        let v = vec!["a".to_string(), "b".to_string()];
        let s: Slice<String> = Slice::from(v.into_iter());
        assert_eq!(&["a", "b"], s.as_ref());
    }

    #[test]
    fn drops_elements() {
        let n = Arc::new(AtomicUsize::new(0));
        let items: Vec<_> = (0..4).map(|_| CountDrop(n.clone())).collect();
        let s: Slice<CountDrop> = Slice::from(items.into_iter());
        assert_eq!(4, s.len());
        assert_eq!(0, n.load(Ordering::SeqCst));
        drop(s);
        assert_eq!(4, n.load(Ordering::SeqCst));
    }

    #[test]
    fn panic_while_filling_drops_initialised() {
        let n = Arc::new(AtomicUsize::new(0));
        let counter = n.clone();
        let r = std::panic::catch_unwind(move || {
            let items = (0..4usize).map(move |i| {
                if i == 2 {
                    panic!("boom");
                }
                CountDrop(counter.clone())
            });
            let _s: Slice<CountDrop> = Slice::from(Liar {
                inner: items,
                claimed: 4,
            });
        });
        assert!(r.is_err());
        //the two items pushed before the panic were dropped
        assert_eq!(2, n.load(Ordering::SeqCst));
    }

    #[test]
    fn iterator_overstates_len() {
        let it = Liar {
            inner: vec!["x".to_string(), "y".to_string()].into_iter(),
            claimed: 5,
        };
        let s: Slice<String> = Slice::from(it);
        assert_eq!(2, s.len());
        assert_eq!(&["x", "y"], s.as_ref());
    }

    #[test]
    fn iterator_understates_len() {
        let it = Liar {
            inner: vec!["x".to_string(), "y".to_string(), "z".to_string()].into_iter(),
            claimed: 1,
        };
        let s: Slice<String> = Slice::from(it);
        assert_eq!(1, s.len());
        assert_eq!(&["x"], s.as_ref());
    }

    #[test]
    fn iterator_claims_nothing() {
        let it = Liar {
            inner: vec!["x".to_string()].into_iter(),
            claimed: 0,
        };
        let s: Slice<String> = Slice::from(it);
        assert_eq!(0, s.len());
    }

    #[test]
    fn iterator_yields_nothing() {
        let it = Liar {
            inner: Vec::<String>::new().into_iter(),
            claimed: 3,
        };
        let s: Slice<String> = Slice::from(it);
        assert_eq!(0, s.len());
    }

    #[test]
    fn zero_sized() {
        let s = Slice::<()>::new_with_length(4);
        assert_eq!(4, s.len());
    }

    #[test]
    #[should_panic(expected = "Slice size overflow")]
    fn size_overflow_panics() {
        let _ = Slice::<u64>::new_with_length(usize::MAX / 4);
    }
}
