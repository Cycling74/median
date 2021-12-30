use median::{
    builder::MaxWrappedBuilder,
    class::Class,
    max_sys,
    method::MaxMethod,
    object::MaxObj,
    wrapper::{MaxObjWrapped, MaxObjWrapper},
};

use std::{
    ffi::{c_void, CString},
    os::raw::{c_char, c_long},
};

#[repr(transparent)]
struct Hack(*mut c_void);

unsafe impl Send for Hack {}
unsafe impl Sync for Hack {}

//you need to wrap your external in this macro to get the system to register your object and
//automatically generate trampolines and what not.
median::external_no_main! {
    #[name="jit.gl.simple"]
    #[repr(C)]
    pub struct JitGLSimpleMax {
        obex: Hack
    }

    //implement the max object wrapper
    impl MaxObjWrapped<JitGLSimpleMax> for JitGLSimpleMax {
        //create an instance of your object
        //setup inlets/outlets and clocks
        fn new(_builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
            Self {
                obex: Hack(std::ptr::null_mut())
            }
        }

        fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
            unsafe {
            //TODO okay to do this after max class_register??

            let off1 = field_offset::offset_of!(median::wrapper::Wrapper::<max_sys::t_object, median::wrapper::MaxWrapperInternal<Self>, Self> => wrapped);
            let off2 = field_offset::offset_of!(median::wrapper::MaxWrapperInternal::<Self> => wrapped);
            let off3 = field_offset::offset_of!(Self => obex);
            max_sys::max_jit_class_obex_setup(c.inner(), (off1.get_byte_offset() + off2.get_byte_offset() + off3.get_byte_offset()) as _);

            let name = CString::new("jit_gl_simple").unwrap();
            let jitclass = max_sys::jit_class_findbyname(max_sys::gensym(name.as_ptr())) as _;
            max_sys::max_jit_class_wrap_standard(c.inner(), jitclass, 0);

            let name = CString::new("assist").unwrap();
            max_sys::class_addmethod(c.inner(), Some(std::mem::transmute::<
                    unsafe extern "C" fn ( x: *mut c_void, b: *mut c_void, m: c_long, a: c_long, s: *mut c_char) -> max_sys::t_jit_err,
                    MaxMethod>(max_sys::max_jit_ob3d_assist)), name.as_ptr(), max_sys::e_max_atomtypes::A_CANT as c_long, 0);

            max_sys::max_jit_class_ob3d_wrap(c.inner());
            }
        }
    }

    //implement any methods you might want for your object that aren't part of the wrapper
    impl JitGLSimpleMax {
        //XXX
    }
}

impl Drop for JitGLSimpleMax {
    fn drop(&mut self) {
        unsafe {
            let x = self.max_obj() as _;
            max_sys::jit_object_free(max_sys::max_jit_obex_jitob_get(x));

            // free resources associated with our obex entry
            max_sys::max_jit_object_free(x);
        }
    }
}

#[repr(C)]
pub struct JitGLSimple {
    ob: max_sys::t_jit_object,
    ob3d: *mut c_void,
}

static mut JIT_GL_SIMPLE_CLASS: *mut c_void = std::ptr::null_mut();

impl JitGLSimple {
    pub unsafe fn init() -> max_sys::t_jit_err {
        let ob3d_flags: c_long = max_sys::t_jit_ob3d_flags::JIT_OB3D_NO_MATRIXOUTPUT as _; // no matrix output

        let name = CString::new("jit_gl_simple").expect("couldn't convert name to CString");
        JIT_GL_SIMPLE_CLASS = max_sys::jit_class_new(
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
            JIT_GL_SIMPLE_CLASS,
            field_offset::offset_of!(Self => ob3d).get_byte_offset() as _,
            ob3d_flags,
        );

        let name = CString::new("ob3d_draw").unwrap();
        max_sys::jit_class_addmethod(
            JIT_GL_SIMPLE_CLASS,
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
            JIT_GL_SIMPLE_CLASS,
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
            JIT_GL_SIMPLE_CLASS,
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
            JIT_GL_SIMPLE_CLASS,
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut c_void, *mut max_sys::t_symbol) -> *mut c_void,
                MaxMethod,
            >(max_sys::jit_object_register)),
            name.as_ptr(),
            max_sys::e_max_atomtypes::A_CANT as c_long,
            0,
        );

        max_sys::jit_class_register(JIT_GL_SIMPLE_CLASS);

        max_sys::t_jit_error_code::JIT_ERR_NONE as _
    }

    pub unsafe extern "C" fn new(dest_name: *mut max_sys::t_symbol) -> *mut Self {
        let x = max_sys::jit_object_alloc(JIT_GL_SIMPLE_CLASS);
        if !x.is_null() {
            // create and attach ob3d
            max_sys::jit_ob3d_new(x, dest_name);
        }
        std::mem::transmute(x)
    }

    pub unsafe extern "C" fn free(x: *mut Self) {
        max_sys::jit_ob3d_free(x as _);
    }

    pub unsafe extern "C" fn draw(_x: *mut Self) -> max_sys::t_jit_err {
        max_sys::t_jit_error_code::JIT_ERR_NONE as _
    }

    pub unsafe extern "C" fn dest_closing(_x: *mut Self) -> max_sys::t_jit_err {
        max_sys::t_jit_error_code::JIT_ERR_NONE as _
    }

    pub unsafe extern "C" fn dest_changed(_x: *mut Self) -> max_sys::t_jit_err {
        max_sys::t_jit_error_code::JIT_ERR_NONE as _
    }
}

median::ext_main! {
    JitGLSimple::init();
    JitGLSimpleMax::register();
}
