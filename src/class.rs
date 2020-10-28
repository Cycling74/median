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

impl Into<*mut max_sys::t_symbol> for ClassType {
    fn into(self) -> *mut max_sys::t_symbol {
        unsafe {
            max_sys::gensym(
                CString::new(match self {
                    ClassType::NoBox => "nobox",
                    ClassType::Box => "box",
                })
                .unwrap()
                .as_ptr(),
            )
        }
    }
}

impl<T> Class<T> {
    pub fn exists_in_max(name: &str, class_type: ClassType) -> bool {
        !Self::find_in_max(name, class_type).is_null()
    }

    pub fn find_in_max(name: &str, class_type: ClassType) -> *mut max_sys::t_class {
        unsafe {
            max_sys::class_findbyname(
                class_type.into(),
                max_sys::gensym(
                    CString::new(name)
                        .expect("couldn't convert name to CString")
                        .as_ptr(),
                ),
            )
        }
    }

    ///
    pub unsafe fn new_registered(class: *mut max_sys::t_class) -> Self {
        Self {
            class,
            _phantom: PhantomData,
        }
    }

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
        unsafe { Self::new_registered(class) }
    }

    /// Register the max class.
    pub fn register(&mut self, class_type: ClassType) -> MaxResult<()> {
        unsafe {
            MaxError::from(
                max_sys::class_register(class_type.into(), self.class) as _,
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
