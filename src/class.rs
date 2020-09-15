//! Class registration.

use crate::error::{MaxError, MaxResult};
use std::ffi::c_void;
use std::ffi::CString;
use std::marker::PhantomData;

//TODO take args
pub type MaxNew = unsafe extern "C" fn() -> *mut c_void;
pub type MaxFree<T> = unsafe extern "C" fn(obj: *mut T);
pub type MaxMethod = unsafe extern "C" fn(arg1: *mut c_void, ...) -> *mut c_void;

pub struct Class<T> {
    class: *mut max_sys::t_class,
    _phantom: PhantomData<T>,
}

#[derive(Debug, Copy, Clone)]
pub enum ClassType {
    Box,
    NoBox,
}

impl<T> Class<T> {
    /// Create a new max class with the given name, new trampoline and optional freem trampoline.
    pub fn new(name: &str, new: MaxNew, free: Option<MaxFree<T>>) -> Self {
        let class = unsafe {
            max_sys::class_new(
                CString::new(name)
                    .expect("couldn't convert name to CString")
                    .as_ptr(),
                Some(std::mem::transmute::<
                    unsafe extern "C" fn() -> *mut c_void,
                    MaxMethod,
                >(new)),
                std::mem::transmute::<Option<MaxFree<T>>, Option<MaxMethod>>(free),
                std::mem::size_of::<T>() as i64,
                None,
                0,
            )
        };

        Self {
            class,
            _phantom: PhantomData,
        }
    }

    /// Register the max class.
    pub fn register(&mut self, class_type: ClassType) -> MaxResult<()> {
        let class_type = match class_type {
            ClassType::NoBox => "nobox",
            ClassType::Box => "box",
        };
        unsafe {
            MaxError::from(
                max_sys::class_register(
                    max_sys::gensym(CString::new(class_type).unwrap().as_ptr()),
                    self.class,
                ) as _,
                (),
            )
        }
    }

    /// Get the inner class object.
    pub fn inner(&mut self) -> *mut max_sys::t_class {
        self.class
    }

    pub fn add_method_int(&mut self, name: &str, cb: extern "C" fn(*const T, i64)) {
        unsafe {
            max_sys::class_addmethod(
                self.class,
                Some(std::mem::transmute::<_, MaxMethod>(cb)),
                CString::new(name).unwrap().as_ptr(),
                max_sys::e_max_atomtypes::A_LONG,
                0,
            );
        }
    }

    pub fn add_method_bang(&mut self, cb: extern "C" fn(*const T)) {
        unsafe {
            max_sys::class_addmethod(
                self.class,
                Some(std::mem::transmute::<_, MaxMethod>(cb)),
                CString::new("bang").unwrap().as_ptr(),
                0,
            );
        }
    }
}
