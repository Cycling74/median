use median::{
    builder::MaxWrappedBuilder,
    class::Class,
    jit::ob3d::{JitObj, WrappedDraw, Wrapper},
    max_sys,
    method::MaxMethod,
    object::MaxObj,
    wrapper::{MaxObjWrapped, MaxObjWrapper, ObjWrapped},
};

use std::{
    ffi::{c_void, CString},
    os::raw::{c_char, c_long},
};

const JIT_CLASS_NAME: &str = "jit_gl_simple";

median::external_no_main! {
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

pub struct JitGLSimple;

impl WrappedDraw for JitGLSimple {
    type Wrapper = Wrapper<Self>;

    fn new() -> Self {
        Self
    }

    fn class_name() -> &'static str {
        &"jit_gl_simple"
    }

    fn draw(&self) -> max_sys::t_jit_err {
        unsafe {
            max_sys::jit_gl_immediate_begin(max_sys::e_jit_state::JIT_STATE_QUADS);
            max_sys::jit_gl_immediate_vertex3f(-1., -1., 0.);
            max_sys::jit_gl_immediate_vertex3f(-1., 1., 0.);
            max_sys::jit_gl_immediate_vertex3f(1., 1., 0.);
            max_sys::jit_gl_immediate_vertex3f(1., -1., 0.);
            max_sys::jit_gl_immediate_end();
        }
        max_sys::t_jit_error_code::JIT_ERR_NONE as _
    }
}

median::ext_main! {
    JitGLSimple::init();
    JitGLSimpleMax::register();
}
