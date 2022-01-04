use median::{
    builder::MaxWrappedBuilder,
    class::Class,
    max_sys,
    method::MaxMethod,
    object::MaxObj,
    symbol::SymbolRef,
    wrapper::{MaxObjWrapped, MaxObjWrapper, ObjWrapped},
};

use std::{
    ffi::{c_void, CString},
    os::raw::{c_char, c_long},
};

const JIT_CLASS_NAME: &str = "jit_gl_simple";

//you need to wrap your external in this macro to get the system to register your object and
//automatically generate trampolines and what not.
median::external_no_main! {
    #[name="jit.gl.simple"]
    #[repr(C)]
    pub struct JitGLSimpleMax;

    //implement the max object wrapper
    impl MaxObjWrapped<JitGLSimpleMax> for JitGLSimpleMax {
        //create an instance of your object
        //setup inlets/outlets and clocks
        fn new(builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
            unsafe {
                let name = CString::new(JIT_CLASS_NAME).unwrap();
                let x = builder.max_obj();
                {
                    assert_ne!(x, std::ptr::null_mut());

                    let mut dest_name: median::symbol::SymbolRef = Default::default();
                    let args = builder.creation_args();
                    if args.len() > 0 {
                        dest_name = args[0].get_symbol();
                    }

                    let jit_ob = max_sys::jit_object_new(max_sys::gensym(name.as_ptr()), dest_name);
                    assert_ne!(jit_ob, std::ptr::null_mut());

                    max_sys::max_jit_object_wrap_complete(x, jit_ob as _, 0);

                    {

                        // set internal jitter object instance
                        max_sys::max_jit_obex_jitob_set(x as _, jit_ob);
                        let o = max_sys::max_jit_obex_jitob_get(x as _);
                        assert_eq!(o, jit_ob);

                        // add a general purpose outlet (rightmost)
                        let out = max_sys::outlet_new(x as _, std::ptr::null_mut());
                        max_sys::max_jit_obex_dumpout_set(x as _, out);

                        // process attribute arguments
                        max_sys::max_jit_attr_args(x as _, args.len() as _, args.as_ptr() as _);

                        // attach the jit object's ob3d to a new outlet
                        // this outlet is used in matrixoutput mode
                        let name = CString::new("jit_matrix").unwrap();
                        let out = max_sys::outlet_new(x as _, name.as_ptr());
                        max_sys::max_jit_ob3d_attach(x as _, jit_ob as _, out);

                    }
                }
                Self
            }
        }

        fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
            unsafe {
                max_sys::max_jit_class_obex_setup(c.inner(), Self::obex_byte_offset() as _);

                let name = CString::new(JIT_CLASS_NAME).unwrap();
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

    impl ObjWrapped<JitGLSimpleMax> for JitGLSimpleMax {
        fn class_name() -> &'static str {
            "jit.gl.simple"
        }
        unsafe fn destroy(&mut self) {
            //have to do this before mem swap because otherwise the obex pointer isn't correct
            let x = self.max_obj() as _;

            // lookup our internal Jitter object instance and free
            let o = max_sys::max_jit_obex_jitob_get(x);
            assert_ne!(o, std::ptr::null_mut());
            max_sys::jit_object_free(o);

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

        let name = CString::new(JIT_CLASS_NAME).expect("couldn't convert name to CString");
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
            let s = SymbolRef::from(dest_name);
            println!("dest_name {}", s);
            // create and attach ob3d
            max_sys::jit_ob3d_new(x, dest_name);
        }
        x as _
    }

    pub unsafe extern "C" fn free(x: *mut Self) {
        max_sys::jit_ob3d_free(x as _);
    }

    pub unsafe extern "C" fn draw(_x: *mut Self) -> max_sys::t_jit_err {
        max_sys::jit_gl_immediate_begin(max_sys::e_jit_state::JIT_STATE_QUADS);
        max_sys::jit_gl_immediate_vertex3f(-1., -1., 0.);
        max_sys::jit_gl_immediate_vertex3f(-1., 1., 0.);
        max_sys::jit_gl_immediate_vertex3f(1., 1., 0.);
        max_sys::jit_gl_immediate_vertex3f(1., -1., 0.);
        max_sys::jit_gl_immediate_end();
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
