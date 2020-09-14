use std::ffi::c_void;
use std::ffi::CString;

use median::class::Class;
use median::wrapper::{Wrapped, Wrapper};

pub fn post(msg: String) {
    unsafe {
        max_sys::post(CString::new(msg.as_str()).unwrap().as_ptr());
    }
}

pub struct Simp {
    value: i64,
}

impl Wrapped for Simp {
    fn new(_o: *mut max_sys::t_object) -> Self {
        Self { value: 0 }
    }

    fn class_name() -> &'static str {
        &"simp"
    }

    /// Register any methods you need for your class
    fn class_setup(c: &mut Class<Wrapper<Self>>) {
        pub extern "C" fn bang_trampoline(s: *mut Wrapper<Simp>) {
            unsafe {
                let obj = &mut *(s as *mut Wrapper<Simp>);
                obj.wrapped().bang();
            }
        }

        pub extern "C" fn int_trampoline(s: *mut Wrapper<Simp>, v: i64) {
            unsafe {
                let obj = &mut *(s as *mut Wrapper<Simp>);
                obj.wrapped().int(v);
            }
        }
        c.add_method_int("int", int_trampoline);
        c.add_method_bang(bang_trampoline);
    }
}

impl Simp {
    pub fn bang(&mut self) {
        post(format!("from rust, value is {}", self.value));
    }

    pub fn int(&mut self, v: i64) {
        post(format!("from rust, value is {}", self.value));
        self.value = v
    }
}

#[no_mangle]
pub unsafe extern "C" fn ext_main(_r: *mut c_void) {
    Wrapper::<Simp>::register()
}
