use median::attr::{AttrTrampGetMethod, AttrTrampSetMethod};
use median::builder::MaxWrappedBuilder;
use median::class::{Class, MaxMethod};
use median::clock::ClockHandle;
use median::num::Long;
use median::post;
use median::symbol::SymbolRef;
use median::wrapper::{MaxObjWrapped, MaxObjWrapper, ObjWrapped, WrapperWrapped};

use std::convert::{From, TryFrom};

use std::ffi::c_void;
use std::ffi::CString;
use std::os::raw::c_long;

pub struct Simp {
    pub value: Long,
    _v: String,
    clock: ClockHandle,
}

impl MaxObjWrapped<Simp> for Simp {
    fn new(builder: &mut MaxWrappedBuilder<Self>) -> Self {
        Self {
            value: Long::new(0),
            _v: String::from("blah"),
            clock: builder.with_clockfn(Self::clocked),
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
                median::attr::set(ac, av, |v: i64| {
                    post!("attr_set_trampoline {}", v);
                    obj.wrapped().value.set(v);
                });
            }
        }

        c.add_method_int("int", int_trampoline);
        c.add_method_bang(bang_trampoline);

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
    MaxObjWrapper::<Simp>::register()
}
