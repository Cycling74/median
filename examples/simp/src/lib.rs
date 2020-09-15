use std::ffi::c_void;
use std::ffi::CString;

use median::class::Class;
use median::num::Long;
use median::wrapper::{Wrapped, Wrapper};

pub fn post(msg: String) {
    unsafe {
        max_sys::post(CString::new(msg.as_str()).unwrap().as_ptr());
    }
}

pub struct Simp {
    value: Long,
    _v: String,
}

impl Wrapped for Simp {
    fn new(_o: *mut max_sys::t_object) -> Self {
        Self {
            value: Long::new(0),
            _v: String::from("blah"),
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
    }
}

impl Simp {
    pub fn bang(&self) {
        post(format!("from rust {}", self.value));
    }

    pub fn int(&self, v: i64) {
        self.value.set(v);
        //XXX won't compile, needs mutex
        //self._v = format!("from rust {}", self.value);
        post(format!("from rust {}", self.value));
    }
}

#[no_mangle]
pub unsafe extern "C" fn ext_main(_r: *mut c_void) {
    Wrapper::<Simp>::register()
}
