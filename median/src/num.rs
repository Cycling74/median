//! Numeric type wrappers.
//!
//! These are lock-free cells for sharing a single scalar between threads, for example an attribute
//! value written on the main or scheduler thread and read on the audio thread.
//!
//! # Memory ordering
//!
//! All accessors use [`Ordering::Relaxed`]. That is enough for the intended use: one scalar that
//! readers want as "the latest value", where no other data is read on the strength of having seen
//! it. It does not order other memory accesses. If a value is used to publish anything else (a
//! pointer, a buffer, a second value that must be consistent with it), use `Release` on the writer
//! and `Acquire` on the reader with your own atomics, or a lock off the audio thread.

use std::sync::atomic::Ordering;

//re-export
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub use self::atomic64::*;

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
mod atomic64 {
    use super::Ordering;
    use std::sync::atomic::{AtomicI64, AtomicU64};

    macro_rules! impl_atomic {
        ($t:ty, $a:ident, $store:ty, $atomic:ty, $to:expr, $from:expr) => {
            #[derive(Default)]
            #[repr(transparent)]
            pub struct $a {
                pub(crate) value: $atomic,
            }

            impl $a {
                pub fn new(v: $t) -> Self {
                    Self {
                        value: <$atomic>::new(($to)(v)),
                    }
                }
                pub fn get(&self) -> $t {
                    let v: $store = self.value.load(Ordering::Relaxed);
                    ($from)(v)
                }
                pub fn set(&self, v: $t) {
                    self.value.store(($to)(v), Ordering::Relaxed)
                }
            }

            impl ::std::convert::From<$t> for $a {
                fn from(v: $t) -> Self {
                    Self::new(v)
                }
            }

            #[allow(clippy::from_over_into)]
            impl ::std::convert::Into<$t> for &$a {
                fn into(self) -> $t {
                    self.get()
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
        };
    }

    //`Send` and `Sync` come from the atomic fields; there is no `unsafe impl`.
    impl_atomic!(f64, Float64, u64, AtomicU64, f64::to_bits, f64::from_bits);
    const fn long_to_i64(v: max_sys::t_atom_long) -> i64 {
        v as i64
    }
    const fn i64_to_long(v: i64) -> max_sys::t_atom_long {
        v as max_sys::t_atom_long
    }
    impl_atomic!(
        max_sys::t_atom_long,
        Int64,
        i64,
        AtomicI64,
        long_to_i64,
        i64_to_long
    );

    impl From<i64> for Int64 {
        fn from(v: i64) -> Self {
            Self::new(v as max_sys::t_atom_long)
        }
    }
}

#[cfg(all(test, any(target_arch = "x86_64", target_arch = "aarch64")))]
mod tests {
    use super::atomic64::*;
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
            value: std::sync::atomic::AtomicU64::new(0),
        },
    };

    #[test]
    fn sizes() {
        assert_eq!(std::mem::size_of::<f64>(), std::mem::size_of::<Float64>());
        assert_eq!(std::mem::size_of::<i64>(), std::mem::size_of::<Int64>());
    }

    #[test]
    fn align() {
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
    fn float_roundtrip() {
        let f = Float64::new(-1.5);
        assert_eq!(f.get(), -1.5);
        f.set(f64::INFINITY);
        assert_eq!(f.get(), f64::INFINITY);
        for bits in [(-0.0f64).to_bits(), 0x7ff8_0000_0000_0042] {
            f.set(f64::from_bits(bits));
            assert_eq!(f.get().to_bits(), bits);
            assert_eq!(f.clone().get().to_bits(), bits);
        }
        assert_eq!(Float64::default().get(), 0.0);
    }

    #[test]
    fn can_share() {
        fn is_send_sync<T: Send + Sync>() {}
        is_send_sync::<Float64>();
        is_send_sync::<Int64>();

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
