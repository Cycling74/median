//! Clocks - interface to Max’s scheduler.
use crate::class::ClassType;
use crate::method::MaxMethod;
use crate::{
    builder::MaxWrappedBuilder,
    object::MaxObj,
    wrapper::{MaxObjWrapped, MaxObjWrapper, ObjWrapped, WrapperWrapped},
};

use std::ffi::{c_void, CString};

/// A raw `t_object` pointer that may be stored in the (shared) clock target.
///
/// SAFETY (for the `Send + Sync` impls): the pointer is never dereferenced by this crate from
/// the thread that owns the `ClockInner`; it is only handed back to the stored callback, which
/// is `Send + Sync` and runs on the scheduler thread, and only while the owning Max object is
/// alive (see the lifetime invariant on `ClockHandle`).
#[repr(transparent)]
struct SyncPtr<T>(*mut T);

unsafe impl<T> Send for SyncPtr<T> {}
unsafe impl<T> Sync for SyncPtr<T> {}

type ClockFn = Box<dyn Fn(*mut max_sys::t_object) + Send + Sync>;

struct ClockInner {
    target: Option<(SyncPtr<max_sys::t_object>, ClockFn)>,
}

impl ClockInner {
    pub(crate) fn set(
        &mut self,
        target: *mut max_sys::t_object,
        func: ClockFn,
    ) {
        self.target = Some((SyncPtr(target), func));
    }

    pub extern "C" fn call_tramp(wrapper: &MaxObjWrapper<Self>) {
        wrapper.wrapped().call()
    }

    fn call(&self) {
        if let Some((target, func)) = &self.target {
            (func)(target.0)
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

/// A handle for a clock and the method it will trigger.
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
        func: ClockFn,
    ) -> Self {
        //register wraper if it hasn't already been
        //XXX what if there is another instance of this library that has already registered
        //this clock?
        MaxObjWrapper::<ClockInner>::register(true);
        let mut clock_target = MaxObjWrapper::<ClockInner>::new_noargs();
        clock_target.wrapped_mut().set(target, func);
        //Use the owner's scheduler at creation time; storing #S on an existing clock
        //does not initialize the clock with that scheduler.
        let sched = max_sys::scheduler_fromobject(target);
        let obj = std::mem::transmute::<_, *mut c_void>(clock_target.max_obj());
        let method = Some(std::mem::transmute::<
            extern "C" fn(&MaxObjWrapper<ClockInner>),
            MaxMethod,
        >(ClockInner::call_tramp));
        let clock = if sched.is_null() {
            max_sys::clock_new(obj, method)
        } else {
            max_sys::clock_new_withscheduler(obj, method, sched)
        };

        //set the patcher and box for the clock
        for lookup in &["#P", "#B"] {
            let name = CString::new(*lookup).unwrap();
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
        //Release the clock before its callback target is dropped.
        unsafe { max_sys::object_free(self.clock) };
    }
}
