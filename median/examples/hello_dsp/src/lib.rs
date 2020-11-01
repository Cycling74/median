use median::{
    attr::{AttrBuilder, AttrType},
    builder::MSPWrappedBuilder,
    class::Class,
    clock::ClockHandle,
    num::Int64,
    post,
    wrapper::{MSPObjWrapped, MSPObjWrapper, ObjWrapped, WrapperWrapped},
};

use std::convert::From;

use std::ffi::c_void;
use std::os::raw::c_long;

pub struct HelloDSP {
    pub value: Int64,
    _v: String,
    clock: ClockHandle,
}

impl MSPObjWrapped<HelloDSP> for HelloDSP {
    fn new(builder: &mut dyn MSPWrappedBuilder<Self>) -> Self {
        builder.add_signal_inlets(2);
        builder.add_signal_outlets(2);
        Self {
            value: Int64::new(0),
            _v: String::from("blah"),
            clock: builder.with_clockfn(Self::clocked),
        }
    }

    fn perform(&self, _ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        for o in outs[0].iter_mut() {
            *o = 2f64;
        }
        for o in outs[1].iter_mut() {
            *o = 1f64;
        }
    }

    /// Register any methods you need for your class
    fn class_setup(c: &mut Class<MSPObjWrapper<Self>>) {
        pub extern "C" fn bang_trampoline(w: &MSPObjWrapper<HelloDSP>) {
            w.wrapped().bang();
        }

        pub extern "C" fn int_trampoline(w: &MSPObjWrapper<HelloDSP>, v: i64) {
            w.wrapped().int(v);
        }

        pub extern "C" fn attr_get_trampoline(
            w: &MSPObjWrapper<HelloDSP>,
            _attr: c_void,
            ac: *mut c_long,
            av: *mut *mut max_sys::t_atom,
        ) {
            median::attr::get(ac, av, || w.wrapped().value.get());
        }

        pub extern "C" fn attr_set_trampoline(
            w: &MSPObjWrapper<HelloDSP>,
            _attr: c_void,
            ac: c_long,
            av: *mut max_sys::t_atom,
        ) {
            median::attr::set(ac, av, |v: i64| {
                post!("attr_set_trampoline {}", v);
                w.wrapped().value.set(v);
            });
        }

        c.add_method(median::method::Method::Int(int_trampoline));
        c.add_method(median::method::Method::Bang(bang_trampoline));

        c.add_attribute(
            AttrBuilder::new_accessors(
                "blah",
                AttrType::Int64,
                attr_get_trampoline,
                attr_set_trampoline,
            )
            .build()
            .unwrap(),
        )
        .expect("failed to add attribute");
    }
}

impl ObjWrapped<HelloDSP> for HelloDSP {
    fn class_name() -> &'static str {
        &"hello_dsp~"
    }
}

impl HelloDSP {
    pub fn bang(&self) {
        post!("from rust {}", self.value);
        self.clock.delay(10);
    }

    pub fn int(&self, v: i64) {
        self.value.set(v);
        //XXX won't compile, needs mutex
        //self._v = format!("from rust {}", self.value);
        post!("from rust {}", self.value);
    }

    pub fn clocked(&self) {
        post("clocked".to_string());
    }
}

#[no_mangle]
pub unsafe extern "C" fn ext_main(_r: *mut c_void) {
    MSPObjWrapper::<HelloDSP>::register(false)
}
