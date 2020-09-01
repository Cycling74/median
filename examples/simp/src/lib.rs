use std::ffi::c_void;
use std::ffi::CString;

use median::class::Class;
use median::wrapper::{WrappedNew, Wrapper};

static mut SIMP_CLASS: Option<Class<Wrapper<Simp>>> = None;

pub fn post(msg: String) {
    unsafe {
        max_sys::post(CString::new(msg.as_str()).unwrap().as_ptr());
    }
}

pub struct Simp {
    value: i64,
}

impl WrappedNew for Simp {
    fn new(_o: *mut max_sys::t_object) -> Self {
        Self { value: 0 }
    }
}

impl Simp {
    pub unsafe extern "C" fn new_tramp() -> *mut c_void {
        Wrapper::new(&mut SIMP_CLASS)
    }

    pub fn bang(&mut self) {
        post(format!("from rust, value is {}", self.value));
    }

    pub fn int(&mut self, v: i64) {
        post(format!("from rust, value is {}", self.value));
        self.value = v
    }
}

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

#[no_mangle]
pub unsafe extern "C" fn ext_main(_r: *mut c_void) {
    let mut c = Wrapper::new_class("simp", Simp::new_tramp);
    c.add_method_int("int", int_trampoline);
    c.add_method_bang(bang_trampoline);
    c.register().expect("failed to register");
    SIMP_CLASS = Some(c);
}
