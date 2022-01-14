use median::{
    builder::MaxWrappedBuilder,
    class::Class,
    jit,
    jit::{
        attr,
        attr::{Attr, AttrBuilder, AttrClip, AttrType, AttrValClip},
        matrix::{Count, IOCount, JitObj, Matrix, WrappedMatrixOp, Wrapper},
    },
    max_sys,
    method::MaxMethod,
    num::Float32,
    object::MaxObj,
    symbol::SymbolRef,
    wrapper::{MaxObjWrapped, MaxObjWrapper, ObjWrapped},
};

use std::{
    convert::TryFrom,
    ffi::{c_void, CString},
    os::raw::{c_char, c_long},
    sync::atomic::{AtomicBool, Ordering},
};
lazy_static::lazy_static! {
    static ref SCALE: SymbolRef = SymbolRef::try_from("scale").unwrap();
    static ref A_SCALE: SymbolRef = SymbolRef::try_from("ascale").unwrap();
    static ref R_SCALE: SymbolRef = SymbolRef::try_from("rscale").unwrap();
    static ref G_SCALE: SymbolRef = SymbolRef::try_from("gscale").unwrap();
    static ref B_SCALE: SymbolRef = SymbolRef::try_from("bscale").unwrap();

    static ref BIAS: SymbolRef = SymbolRef::try_from("bias").unwrap();
    static ref A_BIAS: SymbolRef = SymbolRef::try_from("abias").unwrap();
    static ref R_BIAS: SymbolRef = SymbolRef::try_from("rbias").unwrap();
    static ref G_BIAS: SymbolRef = SymbolRef::try_from("gbias").unwrap();
    static ref B_BIAS: SymbolRef = SymbolRef::try_from("bbias").unwrap();

    static ref JIT_CLASS_NAME: SymbolRef = SymbolRef::try_from(JitScaleBias::class_name()).unwrap();
}

median::external_no_main! {
    pub struct JitScaleBiasMax;

    //implement the max object wrapper
    impl MaxObjWrapped<JitScaleBiasMax> for JitScaleBiasMax {
        //create an instance of your object
        //setup inlets/outlets and clocks
        fn new(builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
            unsafe {
                let x = builder.max_obj();
                {
                    assert_ne!(x, std::ptr::null_mut());

                    let args = builder.creation_args();

                    let jit_ob = max_sys::jit_object_new(JIT_CLASS_NAME.inner());
                    assert_ne!(jit_ob, std::ptr::null_mut());

                    max_sys::max_jit_object_wrap_complete(x, jit_ob as _, 0);

                    max_sys::max_jit_mop_setup_simple(x as _, jit_ob, args.len() as _, args.as_ptr() as _);
                    max_sys::max_jit_attr_args(x as _, args.len() as _, args.as_ptr() as _);

                    let o = max_sys::max_jit_obex_jitob_get(x as _);
                    assert_eq!(o, jit_ob);
                }
                Self
            }
        }

        fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
            unsafe {
                let jitclass = max_sys::jit_class_findbyname(JIT_CLASS_NAME.inner()) as _;

                max_sys::max_jit_class_obex_setup(c.inner(), Self::obex_byte_offset() as _);
                max_sys::max_jit_class_mop_wrap(c.inner(), jitclass, 0);
                max_sys::max_jit_class_wrap_standard(c.inner(), jitclass, 0);

                let name = CString::new("assist").unwrap();
                max_sys::class_addmethod(c.inner(), Some(std::mem::transmute::<
                        unsafe extern "C" fn (x: *mut c_void, b: *mut c_void, m: c_long, a: c_long, s: *mut c_char) -> max_sys::t_jit_err,
                        MaxMethod>(max_sys::max_jit_mop_assist)), name.as_ptr(), max_sys::e_max_atomtypes::A_CANT as c_long, 0);

            }
        }
    }

    impl ObjWrapped<JitScaleBiasMax> for JitScaleBiasMax {
        fn class_name() -> &'static str {
            "jit.median.scalebias"
        }

        unsafe fn destroy(&mut self) {
            //have to do this before mem swap because otherwise the obex pointer isn't correct
            let x = self.max_obj() as _;

            max_sys::max_jit_mop_free(x);

            // lookup our internal Jitter object instance and free
            let o = max_sys::max_jit_obex_jitob_get(x);
            assert_ne!(o, std::ptr::null_mut());
            max_sys::jit_object_free(o);

            // free resources associated with our obex entry
            max_sys::max_jit_object_free(x);
        }
    }
}

pub struct JitScaleBias {
    channels: [Channel; 4],
    mode: AtomicBool,
}

const A: usize = 0;
const R: usize = 1;
const G: usize = 2;
const B: usize = 3;

struct Channel {
    pub scale: Float32,
    pub bias: Float32,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            scale: 1.0.into(),
            bias: 0.0.into(),
        }
    }
}

impl WrappedMatrixOp for JitScaleBias {
    type Wrapper = Wrapper<Self>;

    fn new() -> Self {
        Self {
            channels: Default::default(),
            mode: AtomicBool::new(false),
        }
    }

    fn class_name() -> &'static str {
        &"jit_median_scalebias"
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }

    fn calc(
        &self,
        inputs: &[Matrix],
        outputs: &[Matrix],
    ) -> Result<(), max_sys::t_jit_error_code::Type> {
        let input = inputs
            .first()
            .ok_or(max_sys::t_jit_error_code::JIT_ERR_INVALID_INPUT)?;
        let output = outputs
            .first()
            .ok_or(max_sys::t_jit_error_code::JIT_ERR_INVALID_OUTPUT)?;

        let mut inputl = input.lock();
        let mut outputl = output.lock();

        let inputi = inputl.info();
        let outputi = outputl.info();

        if inputi.plane_count() != 4 || outputi.plane_count() != 4 {
            Err(max_sys::t_jit_error_code::JIT_ERR_MISMATCH_PLANE)
        } else {
            let mut scale: [c_long; 4] = [0; 4];
            let mut bias: [c_long; 4] = [0; 4];
            let mut sumbias: c_long = 0;
            let mode = self.mode.load(Ordering::Relaxed);

            for (s, b, i) in
                itertools::multizip((scale.iter_mut(), bias.iter_mut(), self.channels.iter()))
            {
                *s = (i.scale.get() * 256.0) as _;
                *b = (i.bias.get() * 256.0) as _;
                sumbias += *b;
            }

            let flags = [0, 0];

            if mode {
                jit::matrix::parallel::calc2_intersection2d::<_, u8, u8>(
                    &mut outputl,
                    &mut inputl,
                    &flags,
                    |outs, ins| {
                        for (o, i) in outs.zip(ins) {
                            // sum together, clamping to the range 0-255
                            // and set all output planes
                            for (o, i) in o.entry_iter().zip(i.entry_iter()) {
                                let mut tmp: c_long = 0;
                                for (x, s) in i.iter().zip(scale.iter()) {
                                    tmp = tmp.saturating_add((*x as c_long).saturating_mul(*s));
                                }
                                let tmp =
                                    num::clamp((tmp >> 8).saturating_add(sumbias), 0, 255) as u8;
                                for x in o.iter_mut() {
                                    *x = tmp;
                                }
                            }
                        }
                    },
                )
            } else {
                jit::matrix::parallel::calc2_intersection2d::<_, u8, u8>(
                    &mut outputl,
                    &mut inputl,
                    &flags,
                    |outs, ins| {
                        for (o, i) in outs.zip(ins) {
                            // apply to each plane individually
                            // clamping to the range 0-255
                            for (o, i) in o.entry_iter().zip(i.entry_iter()) {
                                for (o, i, s, b) in itertools::multizip((
                                    o.iter_mut(),
                                    i.iter(),
                                    scale.iter(),
                                    bias.iter(),
                                )) {
                                    *o = num::clamp(
                                        ((*i as c_long).saturating_mul(*s) >> 8) + b,
                                        0,
                                        255,
                                    ) as u8;
                                }
                            }
                        }
                    },
                )
            }
        }
    }

    fn mop_setup(mop: *mut max_sys::t_jit_object) {
        unsafe {
            max_sys::jit_mop_single_type(mop as _, max_sys::_jit_sym_char);
            max_sys::jit_mop_single_planecount(mop as _, 4);
        }
    }

    fn class_setup(class: &jit::Class) {
        for (name, label) in [
            ("ascale", "Alpha"),
            ("rscale", "Red"),
            ("gscale", "Green"),
            ("bscale", "Blue"),
        ] {
            let label = format!("\"{} Scale\"", label);
            let attr = AttrBuilder::new_accessors(
                name,
                AttrType::Float32,
                Self::attr_scale_tramp,
                Self::set_attr_scale_tramp,
            )
            .label(label.as_str())
            .build()
            .unwrap();
            unsafe {
                max_sys::jit_class_addattr(class.inner(), attr.inner() as _);
            }
        }

        for (name, label) in [
            ("abias", "Alpha"),
            ("rbias", "Red"),
            ("gbias", "Green"),
            ("bbias", "Blue"),
        ] {
            let label = format!("\"{} Bias\"", label);
            let attr = AttrBuilder::new_accessors(
                name,
                AttrType::Float32,
                Self::attr_bias_tramp,
                Self::set_attr_bias_tramp,
            )
            .label(label.as_str())
            .build()
            .unwrap();
            unsafe {
                max_sys::jit_class_addattr(class.inner(), attr.inner() as _);
            }
        }

        let attr = AttrBuilder::new_accessors(
            "mode",
            AttrType::Int64,
            Self::attr_mode_tramp,
            Self::set_attr_mode_tramp,
        )
        .clip(AttrClip::Set(AttrValClip::MinMax(0.0, 1.0)))
        .label("Mode")
        .build()
        .unwrap();
        unsafe {
            max_sys::jit_class_addattr(class.inner(), attr.inner() as _);
        }

        //setter only
        let attr = AttrBuilder::new_set("scale", AttrType::Float32, Self::set_attr_scale_tramp)
            .label("Scale")
            .build()
            .unwrap();
        unsafe {
            max_sys::jit_class_addattr(class.inner(), attr.inner() as _);
        }

        let attr = AttrBuilder::new_set("bias", AttrType::Float32, Self::set_attr_bias_tramp)
            .label("Bias")
            .build()
            .unwrap();
        unsafe {
            max_sys::jit_class_addattr(class.inner(), attr.inner() as _);
        }
    }
}

impl JitScaleBias {
    fn scale_index(name: SymbolRef) -> usize {
        if name == *R_SCALE {
            R
        } else if name == *G_SCALE {
            G
        } else if name == *B_SCALE {
            B
        } else {
            A
        }
    }
    fn bias_index(name: SymbolRef) -> usize {
        if name == *R_BIAS {
            R
        } else if name == *G_BIAS {
            G
        } else if name == *B_BIAS {
            B
        } else {
            A
        }
    }

    fn attr_scale(&self, attr: &Attr<Self>) -> f32 {
        let name = attr.name();
        let index = Self::scale_index(name.clone());
        self.channels[index].scale.get()
    }

    fn set_attr_scale(&self, attr: &Attr<Self>, v: f32) {
        let f = v as f32;
        let name = attr.name();
        if name == *SCALE {
            for c in self.channels.iter() {
                c.scale.set(f);
            }
            for n in &[&*R_SCALE, &*G_SCALE, &*B_SCALE] {
                let _ = attr::touch_with_name(self.jit_obj(), *n);
            }
        } else {
            let index = Self::scale_index(name.clone());
            self.channels[index].scale.set(f);
        }
    }

    fn attr_bias(&self, attr: &Attr<Self>) -> f32 {
        let name = attr.name();
        let index = Self::bias_index(name.clone());
        self.channels[index].bias.get()
    }

    fn set_attr_bias(&self, attr: &Attr<Self>, v: f32) {
        let f = v as f32;
        let name = attr.name();
        if name == *BIAS {
            for c in self.channels.iter() {
                c.bias.set(f);
            }
            for n in &[&*R_BIAS, &*G_BIAS, &*B_BIAS] {
                let _ = attr::touch_with_name(self.jit_obj(), *n);
            }
        } else {
            let index = Self::bias_index(name.clone());
            self.channels[index].bias.set(f);
        }
    }

    fn attr_mode(&self, _attr: &Attr<Self>) -> max_sys::t_atom_long {
        self.mode.load(Ordering::Acquire) as _
    }

    fn set_attr_mode(&self, _attr: &Attr<Self>, v: max_sys::t_atom_long) {
        self.mode.store(v != 0, Ordering::Release);
    }

    extern "C" fn attr_scale_tramp(
        x: *mut Wrapper<Self>,
        attr: *mut c_void,
        ac: *mut c_long,
        av: *mut *mut max_sys::t_atom,
    ) -> max_sys::t_jit_err {
        let x = unsafe { Wrapper::wrapped(x) };
        attr::get(attr, ac, av, |attr| x.attr_scale(attr))
    }

    extern "C" fn set_attr_scale_tramp(
        x: *mut Wrapper<Self>,
        attr: *mut c_void,
        ac: c_long,
        av: *mut max_sys::t_atom,
    ) -> max_sys::t_jit_err {
        let x = unsafe { Wrapper::wrapped(x) };
        attr::set(attr, ac, av, |attr, v: f32| {
            x.set_attr_scale(attr, v);
        })
    }

    extern "C" fn attr_bias_tramp(
        x: *mut Wrapper<Self>,
        attr: *mut c_void,
        ac: *mut c_long,
        av: *mut *mut max_sys::t_atom,
    ) -> max_sys::t_jit_err {
        let x = unsafe { Wrapper::wrapped(x) };
        attr::get(attr, ac, av, |attr| x.attr_bias(attr))
    }

    extern "C" fn set_attr_bias_tramp(
        x: *mut Wrapper<Self>,
        attr: *mut c_void,
        ac: c_long,
        av: *mut max_sys::t_atom,
    ) -> max_sys::t_jit_err {
        let x = unsafe { Wrapper::wrapped(x) };
        attr::set(attr, ac, av, |attr, v: f32| {
            x.set_attr_bias(attr, v);
        })
    }

    extern "C" fn attr_mode_tramp(
        x: *mut Wrapper<Self>,
        attr: *mut c_void,
        ac: *mut c_long,
        av: *mut *mut max_sys::t_atom,
    ) -> max_sys::t_jit_err {
        let x = unsafe { Wrapper::wrapped(x) };
        attr::get(attr, ac, av, |attr| x.attr_mode(attr))
    }

    extern "C" fn set_attr_mode_tramp(
        x: *mut Wrapper<Self>,
        attr: *mut c_void,
        ac: c_long,
        av: *mut max_sys::t_atom,
    ) -> max_sys::t_jit_err {
        let x = unsafe { Wrapper::wrapped(x) };
        attr::set(attr, ac, av, |attr, v: max_sys::t_atom_long| {
            x.set_attr_mode(attr, v);
        })
    }
}

median::ext_main! {
    JitScaleBias::init();
    JitScaleBiasMax::register();
}
