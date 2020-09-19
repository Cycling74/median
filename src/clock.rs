use crate::class::{ClassType, MaxMethod};
use crate::object::MaxObj;
use crate::wrapper::{Wrapped, Wrapper};
use std::ffi::c_void;

struct ClockInner {
    func: Option<Box<dyn Fn()>>,
}

//since we know that T is Send and Sync, we should be able to call a method on it and be Send and
//Sync
unsafe impl Send for ClockInner {}
unsafe impl Sync for ClockInner {}

impl ClockInner {
    //tramp actually calls wrapper
    extern "C" fn call_tramp(s: *const Wrapper<Self>) {
        let wrapper = unsafe { &(*s) };
        wrapper.wrapped().call();
    }
    pub fn set(&mut self, func: Box<dyn Fn()>) {
        self.func = Some(func);
    }
    fn call(&self) {
        if let Some(func) = &self.func {
            (func)()
        }
    }
}

impl Wrapped for ClockInner {
    fn new(_o: *mut max_sys::t_object) -> Self {
        Self { func: None }
    }

    fn class_name() -> &'static str {
        //store version in class name so that other externals compliled with other versions won't
        //conflict
        std::concat!(
            env!("CARGO_CRATE_NAME"),
            "ClockInner",
            env!("CARGO_PKG_VERSION")
        )
    }

    fn class_type() -> ClassType {
        ClassType::NoBox
    }
}

pub struct ClockHandle {
    _target: crate::object::ObjBox<Wrapper<ClockInner>>,
    clock: *mut c_void,
}

unsafe impl Send for ClockHandle {}
unsafe impl Sync for ClockHandle {}

impl ClockHandle {
    /// Execute the callback as soon as possible.
    pub fn trigger(&self) {
        self.delay(0);
    }

    /// Execute the callback after the given milliseconds
    pub fn delay(&self, milliseconds: i64) {
        unsafe {
            max_sys::clock_delay(self.clock, milliseconds);
        }
    }

    /// Execute the callback after the given milliseconds
    pub fn fdelay(&self, milliseconds: f64) {
        unsafe {
            max_sys::clock_fdelay(self.clock, milliseconds);
        }
    }

    pub fn cancel(&self) {
        unsafe {
            max_sys::clock_unset(self.clock);
        }
    }

    /// Find out the current logical time of the scheduler in milliseconds as a floating-point number.
    pub fn ftime() -> f64 {
        let mut v = 0f64;
        unsafe {
            max_sys::clock_getftime(&mut v);
        }
        return v;
    }

    /// Find out the current logical time of the scheduler in milliseconds.
    pub fn time() -> i64 {
        unsafe { max_sys::gettime() }
    }

    pub unsafe fn new(func: Box<dyn Fn()>) -> Self {
        //register wraper if it hasn't already been
        //XXX what if there is another instance of this library that has already registered
        //this clock?
        Wrapper::<ClockInner>::register();
        let mut target = Wrapper::<ClockInner>::new();
        target.wrapped_mut().set(func);
        let clock = max_sys::clock_new(
            std::mem::transmute::<_, _>(target.max_obj()),
            Some(std::mem::transmute::<
                extern "C" fn(*const Wrapper<ClockInner>),
                MaxMethod,
            >(ClockInner::call_tramp)),
        );
        Self {
            _target: target,
            clock,
        }
    }
}

impl Drop for ClockHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}
