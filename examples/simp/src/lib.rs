use median::{
    attr::{AttrTrampGetMethod, AttrTrampSetMethod},
    builder::MaxWrappedBuilder,
    class::Class,
    clock::ClockHandle,
    inlet::MaxInlet,
    method::MaxMethod,
    num::Long,
    object::MaxObj,
    outlet::OutList,
    post,
    symbol::SymbolRef,
    wrapper::{MaxObjWrapped, MaxObjWrapper, ObjWrapped, WrapperWrapped},
};

use std::convert::{From, TryFrom};

use std::ffi::c_void;
use std::ffi::CString;
use std::os::raw::c_long;

pub struct Simp {
    pub value: Long,
    _v: String,
    clock: ClockHandle,
    list_out: OutList,
}

impl MaxObjWrapped<Simp> for Simp {
    fn new(builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
        //can call closure
        builder.add_inlet(MaxInlet::Float(Box::new(|_s, v| {
            post!("got float {}", v);
        })));
        //also can call method
        builder.add_inlet(MaxInlet::Int(Box::new(Self::int)));
        let _ = builder.add_inlet(MaxInlet::Proxy);
        Self {
            value: Long::new(0),
            _v: String::from("blah"),
            clock: builder.with_clockfn(Self::clocked),
            list_out: builder.add_list_outlet(),
        }
    }

    /// Register any methods you need for your class
    fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
        pub extern "C" fn bang_trampoline(s: *const MaxObjWrapper<Simp>) {
            unsafe {
                let obj = &*(s as *const MaxObjWrapper<Simp>);
                obj.wrapped().bang();
            }
        }

        pub extern "C" fn int_trampoline(s: *const MaxObjWrapper<Simp>, v: i64) {
            unsafe {
                let obj = &*(s as *const MaxObjWrapper<Simp>);
                obj.wrapped().int(v);
            }
        }

        pub extern "C" fn attr_get_trampoline(
            s: *mut MaxObjWrapper<Simp>,
            _attr: c_void,
            ac: *mut c_long,
            av: *mut *mut max_sys::t_atom,
        ) {
            unsafe {
                let obj = &*(s as *const MaxObjWrapper<Simp>);
                median::attr::get(ac, av, || obj.wrapped().value.get());
            }
        }

        pub extern "C" fn attr_set_trampoline(
            s: *mut MaxObjWrapper<Simp>,
            _attr: c_void,
            ac: c_long,
            av: *mut max_sys::t_atom,
        ) {
            unsafe {
                let obj = &*(s as *const MaxObjWrapper<Simp>);
                median::attr::set(ac, av, |v: i64| obj.wrapped().value.set(v));
            }
        }

        c.add_method(median::method::Method::Int(int_trampoline));
        c.add_method(median::method::Method::Bang(bang_trampoline));

        //TODO encapsulate in a safe method
        unsafe {
            let attr = max_sys::attribute_new(
                CString::new("blah").unwrap().as_ptr(),
                SymbolRef::try_from("long").unwrap().inner(),
                0,
                Some(std::mem::transmute::<
                    AttrTrampGetMethod<MaxObjWrapper<Self>>,
                    MaxMethod,
                >(attr_get_trampoline)),
                Some(std::mem::transmute::<
                    AttrTrampSetMethod<MaxObjWrapper<Self>>,
                    MaxMethod,
                >(attr_set_trampoline)),
            );
            max_sys::class_addattr(c.inner(), attr);
        }
    }
}

impl ObjWrapped<Simp> for Simp {
    fn class_name() -> &'static str {
        &"simp"
    }
}

impl Simp {
    pub fn bang(&self) {
        let i = median::inlet::Proxy::get_inlet(self.max_obj());
        post!("from rust {} inlet {}", self.value, i);
        self.clock.delay(10);
    }

    pub fn int(&self, v: i64) {
        let i = median::inlet::Proxy::get_inlet(self.max_obj());
        self.value.set(v);
        //XXX won't compile, needs mutex
        //self._v = format!("from rust {}", self.value);
        post!("from rust {} inlet {}", self.value, i);
    }

    pub fn clocked(&self) {
        post("clocked".to_string());
        let _ = self.list_out.send(&[
            1i64.into(),
            12f64.into(),
            SymbolRef::try_from("foo").unwrap().into(),
        ]);
    }
}

#[no_mangle]
pub unsafe extern "C" fn ext_main(_r: *mut c_void) {
    MaxObjWrapper::<Simp>::register(false)
}
