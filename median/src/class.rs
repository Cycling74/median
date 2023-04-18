//! Class registration.

use crate::attr::Attr;
use crate::error::{MaxError, MaxResult};
use crate::method::*;
use std::ffi::CString;
use std::marker::PhantomData;
use std::os::raw::c_long;

pub struct Class<T> {
    class: *mut max_sys::t_class,
    _phantom: PhantomData<T>,
}

#[derive(Debug, Copy, Clone)]
pub enum ClassType {
    Box,
    NoBox,
}

impl From<ClassType> for *mut max_sys::t_symbol {
    fn from(val: ClassType) -> Self {
        let t = CString::new(match val {
            ClassType::NoBox => "nobox",
            ClassType::Box => "box",
        })
        .unwrap();
        unsafe { max_sys::gensym(t.as_ptr()) }
    }
}

impl<T> Class<T> {
    pub fn exists_in_max(name: &str, class_type: ClassType) -> bool {
        !Self::find_in_max(name, class_type).is_null()
    }

    pub fn find_in_max(name: &str, class_type: ClassType) -> *mut max_sys::t_class {
        let name = CString::new(name).expect("couldn't convert name to CString");
        unsafe { max_sys::class_findbyname(class_type.into(), max_sys::gensym(name.as_ptr())) }
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
        let name = CString::new(name).expect("couldn't convert name to CString");
        let class = unsafe {
            max_sys::class_new(
                name.as_ptr(),
                Some(std::mem::transmute::<MaxNew, MaxMethod>(new)),
                std::mem::transmute::<Option<MaxFree<T>>, Option<MaxMethod>>(free),
                std::mem::size_of::<T>() as c_long,
                None,
                max_sys::e_max_atomtypes::A_GIMME as _,
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

    pub fn add_attribute(&mut self, attr: Attr<T>) -> MaxResult<()> {
        MaxError::from(
            unsafe { max_sys::class_addattr(self.inner(), attr.into()) as _ },
            (),
        )
    }

    fn add_sel_method(
        &self,
        sel: &str,
        m: Option<MaxMethod>,
        types: &mut [max_sys::e_max_atomtypes::Type],
        defaults: usize,
    ) -> max_sys::t_max_err {
        //fill in defaults
        let l = types.len();
        assert!(l >= defaults);
        for i in l - defaults..l {
            match types[i] {
                max_sys::e_max_atomtypes::A_FLOAT | max_sys::e_max_atomtypes::A_DEFFLOAT => {
                    types[i] = max_sys::e_max_atomtypes::A_DEFFLOAT
                }
                max_sys::e_max_atomtypes::A_LONG | max_sys::e_max_atomtypes::A_DEFLONG => {
                    types[i] = max_sys::e_max_atomtypes::A_DEFLONG
                }
                max_sys::e_max_atomtypes::A_SYM | max_sys::e_max_atomtypes::A_DEFSYM => {
                    types[i] = max_sys::e_max_atomtypes::A_DEFSYM
                }
                _ => panic!("type cannot be made default"),
            }
        }

        //register
        unsafe {
            let sel =
                std::ffi::CString::new(sel).expect("failed to create CString from selector &str");

            match types.len() {
                0 => max_sys::class_addmethod(self.class, m, sel.as_ptr(), 0),
                1 => max_sys::class_addmethod(self.class, m, sel.as_ptr(), types[0], 0),
                2 => max_sys::class_addmethod(self.class, m, sel.as_ptr(), types[0], types[1], 0),
                3 => max_sys::class_addmethod(
                    self.class,
                    m,
                    sel.as_ptr(),
                    types[0],
                    types[1],
                    types[2],
                    0,
                ),
                4 => max_sys::class_addmethod(
                    self.class,
                    m,
                    sel.as_ptr(),
                    types[0],
                    types[1],
                    types[2],
                    types[3],
                    0,
                ),
                5 => max_sys::class_addmethod(
                    self.class,
                    m,
                    sel.as_ptr(),
                    types[0],
                    types[1],
                    types[2],
                    types[3],
                    types[4],
                    0,
                ),
                6 => max_sys::class_addmethod(
                    self.class,
                    m,
                    sel.as_ptr(),
                    types[0],
                    types[1],
                    types[2],
                    types[3],
                    types[4],
                    types[5],
                    0,
                ),
                7 => max_sys::class_addmethod(
                    self.class,
                    m,
                    sel.as_ptr(),
                    types[0],
                    types[1],
                    types[2],
                    types[3],
                    types[4],
                    types[5],
                    types[6],
                    0,
                ),
                _ => unimplemented!(),
            }
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/class-gen.rs"));
