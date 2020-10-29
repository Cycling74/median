use crate::class::ClassType;
use crate::method::MaxMethod;
use crate::{
    builder::MaxWrappedBuilder,
    object::MaxObj,
    wrapper::{MaxObjWrapped, MaxObjWrapper, ObjWrapped, WrapperWrapped},
};

use std::ffi::c_void;

struct ClockInner {
    target: Option<(*mut max_sys::t_object, Box<dyn Fn(*mut max_sys::t_object)>)>,
}

unsafe impl Sync for ClockInner {}

impl ClockInner {
    //tramp actually calls wrapper
    extern "C" fn call_tramp(s: *const MaxObjWrapper<Self>) {
        let wrapper = unsafe { &(*s) };
        wrapper.wrapped().call();
    }
    pub(crate) fn set(
        &mut self,
        target: *mut max_sys::t_object,
        func: Box<dyn Fn(*mut max_sys::t_object)>,
    ) {
        self.target = Some((target, func));
    }
    fn call(&self) {
        if let Some((target, func)) = &self.target {
            (func)(*target)
        }
    }
}

impl ObjWrapped<ClockInner> for ClockInner {
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

impl MaxObjWrapped<ClockInner> for ClockInner {
    fn new(_builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
        Self { target: None }
    }
}

pub struct ClockHandle {
    _target: crate::object::ObjBox<MaxObjWrapper<ClockInner>>,
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

    pub unsafe fn new(
        target: *mut max_sys::t_object,
        func: Box<dyn Fn(*mut max_sys::t_object)>,
    ) -> Self {
        //register wraper if it hasn't already been
        //XXX what if there is another instance of this library that has already registered
        //this clock?
        MaxObjWrapper::<ClockInner>::register(true);
        let mut clock_target = MaxObjWrapper::<ClockInner>::new();
        clock_target.wrapped_mut().set(target, func);
        let clock = max_sys::clock_new(
            std::mem::transmute::<_, _>(clock_target.max_obj()),
            Some(std::mem::transmute::<
                extern "C" fn(*const MaxObjWrapper<ClockInner>),
                MaxMethod,
            >(ClockInner::call_tramp)),
        );
        Self {
            _target: clock_target,
            clock,
        }
    }
}

impl Drop for ClockHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}
