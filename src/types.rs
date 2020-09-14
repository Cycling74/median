//re-export
#[cfg(target_arch = "x86_64")]
pub use self::atomic64::*;

#[cfg(target_arch = "x86_64")]
mod atomic64 {
    //we assume that f64 and i64 are atomic on the platforms we run on
    //so, Float and Long are wrappers that codify that and allow interior mutability, Send and Sync
    use std::cell::UnsafeCell;
    use std::convert::From;
    use std::fmt::{Display, Formatter, Result};

    #[derive(Default)]
    #[repr(transparent)]
    pub struct Float {
        pub(crate) value: UnsafeCell<f64>,
    }

    #[derive(Default)]
    #[repr(transparent)]
    pub struct Long {
        pub(crate) value: UnsafeCell<i64>,
    }

    impl Float {
        pub fn new(v: f64) -> Self {
            Self {
                value: UnsafeCell::new(v),
            }
        }
        pub fn get(&self) -> f64 {
            unsafe { *self.value.get() }
        }
        pub fn set(&self, v: f64) {
            unsafe {
                *self.value.get() = v;
            }
        }
    }

    impl From<f64> for Float {
        fn from(v: f64) -> Self {
            Float::new(v)
        }
    }

    impl Display for Float {
        fn fmt(&self, f: &mut Formatter) -> Result {
            unsafe { write!(f, "{}", *self.value.get()) }
        }
    }

    impl Long {
        pub fn new(v: i64) -> Self {
            Self {
                value: UnsafeCell::new(v),
            }
        }
        pub fn get(&self) -> i64 {
            unsafe { *self.value.get() }
        }
        pub fn set(&self, v: i64) {
            unsafe {
                *self.value.get() = v;
            }
        }
    }

    impl From<i64> for Long {
        fn from(v: i64) -> Self {
            Long::new(v)
        }
    }

    impl Display for Long {
        fn fmt(&self, f: &mut Formatter) -> Result {
            unsafe { write!(f, "{}", *self.value.get()) }
        }
    }

    unsafe impl Send for Float {}
    unsafe impl Sync for Float {}
    unsafe impl Send for Long {}
    unsafe impl Sync for Long {}
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::atomic64::*;
    use std::cell::UnsafeCell;
    use std::sync::Arc;

    #[derive(Default)]
    pub struct A {
        pub f: Float,
    }

    impl A {
        pub fn new() -> Self {
            Self {
                f: Float::new(0f64),
            }
        }
    }

    static BLAH: A = A {
        f: Float {
            value: UnsafeCell::new(0f64),
        },
    };

    #[test]
    fn sizes() {
        assert_eq!(std::mem::size_of::<f64>(), std::mem::size_of::<Float>());
        assert_eq!(std::mem::size_of::<i64>(), std::mem::size_of::<Long>());
    }

    #[test]
    fn align() {
        assert_eq!(std::mem::align_of::<f64>(), std::mem::align_of::<Float>());
        assert_eq!(std::mem::align_of::<i64>(), std::mem::align_of::<Long>());
    }

    #[test]
    fn can_from() {
        let x: Long = 4i64.into();
        assert_eq!(x.get(), 4i64);
    }

    #[test]
    fn can_share() {
        let x = Arc::new(A::new());
        (*x).f.set(20f64);
        BLAH.f.set(1f64);
        let xc = x.clone();
        std::thread::spawn(move || {
            assert_eq!(1f64, BLAH.f.get());
            assert_eq!(20f64, (*xc).f.get());
            BLAH.f.set(2f64);
            (*xc).f.set(10f64);
        })
        .join()
        .unwrap();
        assert_eq!(2f64, BLAH.f.get());
        assert_eq!(10f64, (*x).f.get());
    }
}
