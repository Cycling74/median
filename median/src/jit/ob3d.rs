//! Jitter OpenGL, which we refer to as OB3Ds.

use crate::{jit::Class, method::MaxMethod};
use max_sys::{t_jit_err, t_jit_ob3d_flags::Type as FlagType};

use std::{
    collections::HashMap,
    ffi::{c_void, CString},
    mem::MaybeUninit,
    os::raw::c_long,
    sync::Mutex,
};

/// Trait to implement Jitter OB3D object.
pub trait Draw {
    /// Creation
    fn new() -> Self;

    /// Define your OB3D draw method. Called in automatic mode by jit.gl.render or otherwise
    /// through ob3d when banged.
    fn draw(&self) -> t_jit_err;

    /// The name of your jitter class.
    fn class_name() -> &'static str;

    /// Customize your object, see [`max_sys::jit_ob3d_setup`]
    fn flags() -> FlagType {
        0
    }

    fn dest_changed(&self) -> t_jit_err {
        max_sys::t_jit_error_code::JIT_ERR_NONE as _
    }
    fn dest_closing(&self) -> t_jit_err {
        max_sys::t_jit_error_code::JIT_ERR_NONE as _
    }
}

lazy_static::lazy_static! {
    static ref CLASSES: Mutex<HashMap<&'static str, Class>> = Mutex::new(HashMap::new());
}

#[repr(C)]
pub struct Wrapper<T> {
    ob: max_sys::t_jit_object,
    ob3d: *mut c_void,
    wrapped: MaybeUninit<T>,
}

impl<T> Wrapper<T>
where
    T: Draw,
{
    pub unsafe fn init() -> max_sys::t_jit_err {
        let name = CString::new(T::class_name()).expect("couldn't convert name to CString");
        let class = max_sys::jit_class_new(
            name.as_ptr(),
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut max_sys::t_symbol) -> *mut Self,
                MaxMethod,
            >(Self::new)),
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut Self),
                MaxMethod,
            >(Self::free)),
            std::mem::size_of::<Self>() as c_long,
            max_sys::e_max_atomtypes::A_DEFSYM as c_long,
            0,
        );

        let _ob3d = max_sys::jit_ob3d_setup(
            class,
            field_offset::offset_of!(Self => ob3d).get_byte_offset() as _,
            T::flags() as _,
        );

        let name = CString::new("ob3d_draw").unwrap();
        max_sys::jit_class_addmethod(
            class,
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut Self) -> max_sys::t_jit_err,
                MaxMethod,
            >(Self::draw)),
            name.as_ptr(),
            max_sys::e_max_atomtypes::A_CANT as c_long,
            0,
        );

        let name = CString::new("dest_closing").unwrap();
        max_sys::jit_class_addmethod(
            class,
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut Self) -> max_sys::t_jit_err,
                MaxMethod,
            >(Self::dest_closing)),
            name.as_ptr(),
            max_sys::e_max_atomtypes::A_CANT as c_long,
            0,
        );

        let name = CString::new("dest_changed").unwrap();
        max_sys::jit_class_addmethod(
            class,
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut Self) -> max_sys::t_jit_err,
                MaxMethod,
            >(Self::dest_changed)),
            name.as_ptr(),
            max_sys::e_max_atomtypes::A_CANT as c_long,
            0,
        );

        let name = CString::new("register").unwrap();
        max_sys::jit_class_addmethod(
            class,
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut c_void, *mut max_sys::t_symbol) -> *mut c_void,
                MaxMethod,
            >(max_sys::jit_object_register)),
            name.as_ptr(),
            max_sys::e_max_atomtypes::A_CANT as c_long,
            0,
        );

        max_sys::jit_class_register(class);

        let mut g = CLASSES.lock().expect("couldn't lock CLASSES mutex");
        g.insert(T::class_name(), Class { inner: class });

        max_sys::t_jit_error_code::JIT_ERR_NONE as _
    }

    pub unsafe extern "C" fn new(dest_name: *mut max_sys::t_symbol) -> *mut Self {
        let g = CLASSES.lock().expect("couldn't lock CLASSES mutex");
        let c = g.get(T::class_name()).expect("couldn't find class by name");

        let x = max_sys::jit_object_alloc(c.inner);
        if !x.is_null() {
            // create and attach ob3d
            max_sys::jit_ob3d_new(x, dest_name);

            //initialize
            let x: &mut Self = std::mem::transmute(x as *mut Self);
            x.wrapped = MaybeUninit::new(T::new());
        }
        x as _
    }

    pub unsafe extern "C" fn free(x: *mut Self) {
        max_sys::jit_ob3d_free(x as _);
    }

    pub unsafe fn wrapped<'a>(x: *mut Self) -> &'a T {
        let x: &mut Self = std::mem::transmute(x as *mut Self);
        &*x.wrapped.as_ptr()
    }

    pub unsafe extern "C" fn draw(x: *mut Self) -> max_sys::t_jit_err {
        Self::wrapped(x).draw()
    }

    pub unsafe extern "C" fn dest_closing(x: *mut Self) -> max_sys::t_jit_err {
        Self::wrapped(x).dest_closing()
    }

    pub unsafe extern "C" fn dest_changed(x: *mut Self) -> max_sys::t_jit_err {
        Self::wrapped(x).dest_changed()
    }
}
