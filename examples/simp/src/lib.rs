use median::class::Class;
use median::clock::ClockHandle;
use median::num::Long;
use median::post;
use median::symbol::SymbolRef;
use median::wrapper::{Wrapped, WrappedBuilder, Wrapper};

use std::convert::{Into, TryFrom};

use std::ffi::c_void;
use std::ffi::CString;

pub struct Simp {
    value: Long,
    _v: String,
    clock: ClockHandle,
}

impl Wrapped<Simp> for Simp {
    fn new(builder: &mut dyn WrappedBuilder<Self>) -> Self {
        Self {
            value: Long::new(0),
            _v: String::from("blah"),
            clock: builder.with_clockfn(Self::clocked),
        }
    }

    fn class_name() -> &'static str {
        &"simp"
    }

    /// Register any methods you need for your class
    fn class_setup(c: &mut Class<Wrapper<Self>>) {
        pub extern "C" fn bang_trampoline(s: *const Wrapper<Simp>) {
            unsafe {
                let obj = &*(s as *const Wrapper<Simp>);
                obj.wrapped().bang();
            }
        }

        pub extern "C" fn int_trampoline(s: *const Wrapper<Simp>, v: i64) {
            unsafe {
                let obj = &*(s as *const Wrapper<Simp>);
                obj.wrapped().int(v);
            }
        }
        c.add_method_int("int", int_trampoline);
        c.add_method_bang(bang_trampoline);

        //TODO encapsulate in a safe method
        unsafe {
            let attr = max_sys::attr_offset_new(
                CString::new("blah").unwrap().as_ptr(),
                SymbolRef::try_from("long").unwrap().into(),
                0,
                None,
                None,
                (std::mem::size_of::<max_sys::t_object>()
                    + field_offset::offset_of!(Self => value).get_byte_offset())
                    as _,
            );
            max_sys::class_addattr(c.inner(), attr);
        }
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
    Wrapper::<Simp>::register()
}
