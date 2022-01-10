//! Numeric type wrappers.

//re-export
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub use self::atomic64::*;

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
mod atomic64 {

    macro_rules! impl_atomic {
        ($t:ty, $a:ident) => {
            #[derive(Default)]
            #[repr(transparent)]
            pub struct $a {
                pub(crate) value: ::std::cell::UnsafeCell<$t>,
            }

            impl $a {
                pub fn new(v: $t) -> Self {
                    Self {
                        value: ::std::cell::UnsafeCell::new(v),
                    }
                }
                pub fn get(&self) -> $t {
                    unsafe { *self.value.get() }
                }
                pub fn set(&self, v: $t) {
                    unsafe {
                        *self.value.get() = v;
                    }
                }
            }

            impl ::std::convert::From<$t> for $a {
                fn from(v: $t) -> Self {
                    Self::new(v)
                }
            }

            impl ::std::convert::Into<$t> for &$a {
                fn into(self) -> $t {
                    unsafe { *self.value.get() }
                }
            }

            impl ::std::fmt::Display for $a {
                fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                    write!(f, "{}", self.get())
                }
            }

            impl Clone for $a {
                fn clone(&self) -> Self {
                    Self::new(self.get())
                }
            }
            unsafe impl Send for $a {}
            unsafe impl Sync for $a {}
        };
    }

    //we assume that the following types are atomic on the platform we run on so we wrap them in a
    //type that codifies that for rust
    impl_atomic!(f64, Float64);
    impl_atomic!(f32, Float32);
    impl_atomic!(max_sys::t_atom_long, Int64);

    impl From<i64> for Int64 {
        fn from(v: i64) -> Self {
            Self::new(v as max_sys::t_atom_long)
        }
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::atomic64::*;
    use std::cell::UnsafeCell;
    use std::sync::Arc;

    #[derive(Default)]
    pub struct A {
        pub f: Float64,
    }

    impl A {
        pub fn new() -> Self {
            Self {
                f: Float64::new(0f64),
            }
        }
    }

    static BLAH: A = A {
        f: Float64 {
            value: UnsafeCell::new(0f64),
        },
    };

    #[test]
    fn sizes() {
        assert_eq!(std::mem::size_of::<f32>(), std::mem::size_of::<Float32>());
        assert_eq!(std::mem::size_of::<f64>(), std::mem::size_of::<Float64>());
        assert_eq!(std::mem::size_of::<i64>(), std::mem::size_of::<Int64>());
    }

    #[test]
    fn align() {
        assert_eq!(std::mem::align_of::<f32>(), std::mem::align_of::<Float32>());
        assert_eq!(std::mem::align_of::<f64>(), std::mem::align_of::<Float64>());
        assert_eq!(std::mem::align_of::<i64>(), std::mem::align_of::<Int64>());
    }

    #[test]
    fn can_from() {
        let x: Int64 = 4i64.into();
        assert_eq!(x.get(), 4);
    }

    #[test]
    fn can_into() {
        let x = Int64::new(12);
        let y = &x;
        let z: max_sys::t_atom_long = y.into();
        assert_eq!(z, 12);
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
