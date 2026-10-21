//! Clocks - interface to Max’s scheduler.
use crate::class::ClassType;
use crate::method::MaxMethod;
use crate::{
    builder::MaxWrappedBuilder,
    object::MaxObj,
    wrapper::{MaxObjWrapped, MaxObjWrapper, ObjWrapped, WrapperWrapped},
};

use parking_lot::Mutex;
use std::{
    ffi::{c_void, CString},
    sync::Arc,
};

/// A raw `t_object` pointer that may be stored in the (shared) clock target.
///
/// SAFETY (for the `Send + Sync` impls): the pointer is never dereferenced by this crate; it is
/// only handed back to the stored callback, which is `Send + Sync`. The callback only runs while
/// the owning Max object is alive, because the owner clears the target (under the target's lock)
/// before it drops its state, see [`ClockShared`].
#[repr(transparent)]
struct SyncPtr<T>(*mut T);

unsafe impl<T> Send for SyncPtr<T> {}
unsafe impl<T> Sync for SyncPtr<T> {}

/// The callback target of a clock: the owning object and what to call with it.
///
/// This is shared (`Arc`) between the clock's Max object, the [`ClockHandle`], and the owning
/// wrapper (which gets it from the builder). Lifetime protocol:
///
/// * The clock callback holds `target`'s lock for the whole call.
/// * The owning wrapper calls [`ClockShared::clear`] *before* it drops the user's `T`. That
///   waits for an in-flight callback to finish and makes every later callback a no-op, so
///   the callback can never run against a freed or half-dropped owner, even if the
///   `ClockHandle` was moved somewhere that outlives the object.
///
/// Invariant relied on: **a clock callback never (directly, or by messages it sends that Max
/// handles synchronously) frees its own owning object or drops its own `ClockHandle`.** Both
/// call `clear`, which would try to take the lock the callback already holds on the same thread
/// and deadlock. Defer such work (e.g. `defer_low`) instead.
pub(crate) struct ClockShared {
    target: Mutex<Option<(SyncPtr<max_sys::t_object>, ClockFn)>>,
}

impl ClockShared {
    fn new() -> Self {
        Self {
            target: Mutex::new(None),
        }
    }

    fn set(&self, target: *mut max_sys::t_object, func: ClockFn) {
        let old = self.target.lock().replace((SyncPtr(target), func));
        drop(old);
    }

    /// Run the callback, if the target is still set. Holds the lock for the duration.
    fn call(&self) {
        let guard = self.target.lock();
        if let Some((target, func)) = &*guard {
            (func)(target.0)
        }
    }

    /// Wait for an in-flight callback to finish and make all later callbacks no-ops.
    ///
    /// Must not be called from within a callback of this clock (see the type's invariant).
    pub(crate) fn clear(&self) {
        //drop the closure after releasing the lock
        let old = self.target.lock().take();
        drop(old);
    }
}

type ClockFn = Box<dyn Fn(*mut max_sys::t_object) + Send + Sync>;

struct ClockInner {
    shared: Arc<ClockShared>,
}

impl ClockInner {
    pub extern "C" fn call_tramp(wrapper: &MaxObjWrapper<Self>) {
        wrapper.wrapped().shared.call()
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
        Self {
            shared: Arc::new(ClockShared::new()),
        }
    }
}

/// A handle for a clock and the method it will trigger.
///
/// Dropping the handle unsets and frees the clock. Clocks made with the builder
/// ([`ObjBuilder::with_clock`](crate::builder::ObjBuilder::with_clock)) are additionally
/// disabled when the owning object is freed, so a handle that outlives its object does nothing
/// when it fires. A clock callback must not free its own object or drop its own handle, see
/// [`ClockShared`].
pub struct ClockHandle {
    _target: crate::object::ObjBox<MaxObjWrapper<ClockInner>>,
    clock: *mut c_void,
}

unsafe impl Send for ClockHandle {}
unsafe impl Sync for ClockHandle {}

impl ClockHandle {
    /// The callback target, so the owning wrapper can disable it before it is freed.
    pub(crate) fn shared(&self) -> Arc<ClockShared> {
        self._target.wrapped().shared.clone()
    }

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

    /// Create a clock that calls `func(target)` when it fires.
    ///
    /// # Safety
    /// `target` must be a valid Max object that outlives the returned handle, or the caller must
    /// ensure that the callback is disabled before `target` is freed (the builder does this
    /// for clocks it creates, by clearing [`ClockHandle::shared`] before the owner is dropped).
    pub unsafe fn new(
        target: *mut max_sys::t_object,
        func: Box<dyn Fn(*mut max_sys::t_object) + Send + Sync>,
    ) -> Self {
        //register wraper if it hasn't already been
        //XXX what if there is another instance of this library that has already registered
        //this clock?
        MaxObjWrapper::<ClockInner>::register(true);
        let clock_target = MaxObjWrapper::<ClockInner>::new_noargs();
        clock_target.wrapped().shared.set(target, func);
        //use the scheduler of the owning object if it has one, otherwise the current one
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
        // SAFETY: `clock` was created by `clock_new*` and is freed exactly once, here.
        unsafe {
            max_sys::clock_unset(self.clock);
            //the SDK releases clocks with freeobject(); this also guarantees the scheduler
            //doesn't start the callback again
            max_sys::object_free(self.clock);
        }
        //wait for a callback that is already running, so that `_target` (dropped after this,
        //by field drop) is not freed under it
        self._target.wrapped().shared.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn clear_disables_callback() {
        let count = Arc::new(AtomicUsize::new(0));
        let shared = ClockShared::new();
        let c = count.clone();
        shared.set(
            std::ptr::null_mut(),
            Box::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            }),
        );
        shared.call();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        shared.clear();
        shared.call();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn clear_waits_for_in_flight_callback() {
        let done = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let shared = Arc::new(ClockShared::new());
        let (d, s) = (done.clone(), started.clone());
        shared.set(
            std::ptr::null_mut(),
            Box::new(move |_| {
                s.store(1, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(100));
                d.store(1, Ordering::SeqCst);
            }),
        );
        let sh = shared.clone();
        let t = std::thread::spawn(move || sh.call());
        while started.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        shared.clear();
        //the in-flight callback had finished by the time clear returned
        assert_eq!(done.load(Ordering::SeqCst), 1);
        t.join().unwrap();
    }
}
