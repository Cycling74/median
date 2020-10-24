use crate::wrapper::WrapperWrapped;

macro_rules! write_traits {
    ( $( $i:literal ),+ ) => {
        $(
            paste::paste! {
                pub trait [<FloatIn $i>] {
                    fn [<float_in $i>] (&self, value: f64);
                }

                pub trait [<IntIn $i>] {
                    fn [<int_in $i>] (&self, value: f64);
                }

                impl<O, I, T> crate::wrapper::Wrapper<O, I, T>
                    where
                        I: crate::wrapper::WrapperInternal<O, T>,
                        T: [<FloatIn $i>] + crate::wrapper::ObjWrapped<T> + Sync + 'static,
                        {
                            /// Trampoline for max to call a float method for a wrapped class.
                            pub extern "C" fn [<float_in $i _tramp>](&self, value: f64) {
                                self.wrapped().[<float_in $i>](value)
                            }

                            /// Register a floatin method for this class.
                            pub unsafe fn [<register_float_in $i>](class: *mut max_sys::t_class) {
                                max_sys::class_addmethod(class,
                                    Some(std::mem::transmute::<extern "C" fn(&Self, f64), crate::class::MaxMethod>(Self::[<float_in $i _tramp>])),
                                    std::ffi::CString::new(concat!("ft", $i)).unwrap().as_ptr(),
                                    max_sys::e_max_atomtypes::A_FLOAT, 0
                                    );
                            }
                        }

                impl<O, I, T> crate::wrapper::Wrapper<O, I, T>
                    where
                        I: crate::wrapper::WrapperInternal<O, T>,
                        T: [<IntIn $i>] + crate::wrapper::ObjWrapped<T> + Sync + 'static,
                        {
                            /// Trampoline for max to call an int method for a wrapped class.
                            pub extern "C" fn [<int_in $i _tramp>](&self, value: f64) {
                                self.wrapped().[<int_in $i>](value)
                            }

                            /// Register an intin method for this class.
                            pub unsafe fn [<register_int_in $i>](class: *mut max_sys::t_class) {
                                max_sys::class_addmethod(class,
                                    Some(std::mem::transmute::<extern "C" fn(&Self, f64), crate::class::MaxMethod>(Self::[<int_in $i _tramp>])),
                                    std::ffi::CString::new(concat!("in", $i)).unwrap().as_ptr(),
                                    max_sys::e_max_atomtypes::A_FLOAT, 0
                                    );
                            }
                        }
            }
        )*
    };
}

//write the traits and the automatic impls
write_traits!(1, 2, 3, 4, 5, 6, 7, 8, 9);
