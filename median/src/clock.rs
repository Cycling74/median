use crate::class::ClassType;
use crate::method::MaxMethod;
use crate::{
    builder::MaxWrappedBuilder,
    object::MaxObj,
    wrapper::{tramp, MaxObjWrapped, MaxObjWrapper, ObjWrapped, WrapperWrapped},
};

use std::ffi::{c_void, CString};

struct ClockInner {
    target: Option<(*mut max_sys::t_object, Box<dyn Fn(*mut max_sys::t_object)>)>,
}

unsafe impl Sync for ClockInner {}

impl ClockInner {
    pub(crate) fn set(
        &mut self,
        target: *mut max_sys::t_object,
        func: Box<dyn Fn(*mut max_sys::t_object)>,
    ) {
        self.target = Some((target, func));
    }

    #[tramp(MaxObjWrapper<Self>)]
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
    pub fn delay(&self, milliseconds: i32) {
        unsafe {
            max_sys::clock_delay(self.clock, milliseconds as _);
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
    pub fn time() -> max_sys::t_atom_long {
        unsafe { max_sys::gettime() as _ }
    }

    pub unsafe fn new(
        target: *mut max_sys::t_object,
        func: Box<dyn Fn(*mut max_sys::t_object)>,
    ) -> Self {
        //register wraper if it hasn't already been
        //XXX what if there is another instance of this library that has already registered
        //this clock?
        MaxObjWrapper::<ClockInner>::register(true);
        let mut clock_target = MaxObjWrapper::<ClockInner>::new_noargs();
        clock_target.wrapped_mut().set(target, func);
        let clock = max_sys::clock_new(
            std::mem::transmute::<_, _>(clock_target.max_obj()),
            Some(std::mem::transmute::<
                extern "C" fn(&MaxObjWrapper<ClockInner>),
                MaxMethod,
            >(ClockInner::call_tramp)),
        );

        //set the scheduler for the clock to the scheduler for the owning object
        let sched = max_sys::scheduler_fromobject(target);
        if !sched.is_null() {
            let spound = CString::new("#S").unwrap();
            max_sys::object_obex_storeflags(
                clock,
                max_sys::gensym(spound.as_ptr()),
                sched as _,
                max_sys::e_max_datastore_flags::OBJ_FLAG_DATA as _,
            );
        }

        //set the patcher and box for the clock
        for lookup in ["#P", "#B"] {
            let name = CString::new(lookup).unwrap();
            let mut ob = std::ptr::null_mut();
            if max_sys::object_obex_lookup(
                target as _,
                std::mem::transmute::<_, _>(name.as_ptr()),
                &mut ob,
            ) == max_sys::e_max_errorcodes::MAX_ERR_NONE as max_sys::t_atom_long
            {
                let _ = max_sys::object_obex_storeflags(
                    clock,
                    max_sys::gensym(name.as_ptr()),
                    ob as _,
                    max_sys::e_max_datastore_flags::OBJ_FLAG_REF as _,
                );
            }
        }

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
