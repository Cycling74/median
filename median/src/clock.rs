//! Clocks - interface to Max’s scheduler.
use crate::class::ClassType;
use crate::method::MaxMethod;
use crate::{
    builder::MaxBuilder,
    object::MaxObj,
    wrapper::{FieldAttrs, MaxObjWrapped, MaxObjWrapper, Methods, ObjClass, WrapperWrapped},
};

use parking_lot::Mutex;
use std::{
    ffi::{CStr, c_void},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
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

/// `ClockShared` is in the signature of the sealed `WrapperInternal` trait, which is `pub` (in a
/// private module), so it has to be `pub` too. It is declared in this private module and only
/// re-exported at `pub(crate)`, so other crates cannot name it.
mod shared {
    use super::*;

    /// The callback target of a clock: the owning object and what to call with it.
    ///
    /// This is shared (`Arc`) between the clock's Max object, the [`ClockHandle`], and the owning
    /// wrapper (which gets it from the builder). Lifetime protocol:
    ///
    /// * The callback does nothing until the target is *armed*. Clocks created by the builder run
    ///   inside `T::new`, before the wrapped value is initialised (a `delay` in `new` can fire on the
    ///   scheduler thread right away), so the owning wrapper arms them once construction is done.
    ///   A callback that fires before that is skipped.
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
    pub struct ClockShared {
        pub(super) armed: AtomicBool,
        pub(super) target: Mutex<Option<(SyncPtr<max_sys::t_object>, ClockFn)>>,
    }
}
pub(crate) use shared::ClockShared;

impl ClockShared {
    fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            target: Mutex::new(None),
        }
    }

    /// Allow the callback to run. Release pairs with the Acquire in `call`, so everything
    /// written before arming (the owner's initialised state) is visible to the callback.
    pub(crate) fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    fn set(&self, target: *mut max_sys::t_object, func: ClockFn) {
        let old = self.target.lock().replace((SyncPtr(target), func));
        drop(old);
    }

    /// Run the callback, if the target is armed and still set. Holds the lock for the duration,
    /// including reporting a panic against the target.
    fn call(&self) {
        self.call_with(|target, msg| {
            // SAFETY: called with the lock held, so the owner hasn't been freed (its free path
            // clears this target, which waits for the lock)
            unsafe { crate::object::error(target, crate::ffi::panic_text("clock", msg)) }
        })
    }

    /// `call`, with `report` called (still under the lock) if the callback panics.
    fn call_with<P: FnOnce(*mut max_sys::t_object, &str)>(&self, report: P) {
        if !self.armed.load(Ordering::Acquire) {
            return;
        }
        let guard = self.target.lock();
        if let Some((target, func)) = &*guard {
            let target = target.0;
            crate::ffi::guard_with((), || (func)(target), |msg| report(target, msg));
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
        //`call` catches and reports a panic itself, under the lock: after the lock is released
        //neither the owner nor this clock's object may be alive any more
        //the guard is for `wrapped`, which refuses an object that isn't constructed
        crate::ffi::guard(wrapper, "clock", || wrapper.wrapped().shared.call())
    }
}

/// The class name: stores the version so that externals compiled with other versions of median
/// won't conflict.
const CLOCK_CLASS_NAME: &CStr = match CStr::from_bytes_with_nul(
    concat!(
        env!("CARGO_CRATE_NAME"),
        "ClockInner",
        env!("CARGO_PKG_VERSION"),
        "\0"
    )
    .as_bytes(),
) {
    Ok(n) => n,
    Err(_) => panic!("the clock class name has a NUL"),
};

impl ObjClass for ClockInner {
    type Wrapper = MaxObjWrapper<Self>;

    fn class_name() -> &'static CStr {
        CLOCK_CLASS_NAME
    }

    fn class_type() -> ClassType {
        ClassType::NoBox
    }
}

impl Methods for ClockInner {}
impl FieldAttrs for ClockInner {}

impl MaxObjWrapped for ClockInner {
    fn new(_builder: &mut MaxBuilder<Self>) -> Self {
        Self {
            shared: Arc::new(ClockShared::new()),
        }
    }
}

/// A handle for a clock and the method it will trigger.
///
/// Dropping the handle unsets and frees the clock. Clocks made with the builder
/// ([`Builder::with_clock`](crate::builder::Builder::with_clock)) are additionally
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
        // SAFETY: the caller guarantees the contract of `new_unarmed` and, by calling this,
        // that `target` is ready to be called back immediately
        let handle = unsafe { Self::new_unarmed(target, func) };
        handle.shared().arm();
        handle
    }

    /// Like `new`, but the callback does nothing until `shared().arm()` is called. Used by the
    /// builder, whose owner arms its clocks when construction has finished.
    ///
    /// # Safety
    /// As `new`.
    pub(crate) unsafe fn new_unarmed(
        target: *mut max_sys::t_object,
        func: Box<dyn Fn(*mut max_sys::t_object) + Send + Sync>,
    ) -> Self {
        //register wraper if it hasn't already been
        //XXX what if there is another instance of this library that has already registered
        //this clock?
        //(only on Max's main thread, which clock creation already requires of the caller)
        MaxObjWrapper::<ClockInner>::register_with(true);
        let clock_target = MaxObjWrapper::<ClockInner>::new_noargs();
        clock_target.wrapped().shared.set(target, func);
        //use the scheduler of the owning object if it has one, otherwise the current one
        // SAFETY: the caller guarantees `target` is a valid object
        let sched = unsafe { max_sys::scheduler_fromobject(target) };
        // SAFETY: `max_obj()` is a pointer to the live wrapper object, which `_target` keeps alive
        let obj = unsafe { std::mem::transmute::<_, *mut c_void>(clock_target.max_obj()) };
        // SAFETY: `call_tramp` is an `extern "C"` function whose signature is what Max calls a
        // clock method with (the object pointer)
        let method = Some(unsafe {
            std::mem::transmute::<extern "C" fn(&MaxObjWrapper<ClockInner>), MaxMethod>(
                ClockInner::call_tramp,
            )
        });
        let clock = if sched.is_null() {
            // SAFETY: `obj` is a valid registered object and `method` a valid clock method
            unsafe { max_sys::clock_new(obj, method) }
        } else {
            // SAFETY: as above, and `sched` is non-null and came from `scheduler_fromobject`
            unsafe { max_sys::clock_new_withscheduler(obj, method, sched) }
        };

        //set the patcher and box for the clock
        for name in [c"#P", c"#B"] {
            let mut ob = std::ptr::null_mut();
            // SAFETY: `target` is valid per the caller's guarantee, `name` is a NUL-terminated
            // string that outlives the calls, and `clock` was just created above
            unsafe {
                if max_sys::object_obex_lookup(
                    target as _,
                    max_sys::gensym(name.as_ptr()),
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

    //`ClockShared::call` reports panics through Max's `object_error`, which the test binary
    //can't link
    fn call(shared: &ClockShared) {
        shared.call_with(|_, _| ())
    }

    #[test]
    fn unarmed_callback_does_not_run() {
        let count = Arc::new(AtomicUsize::new(0));
        let shared = ClockShared::new();
        let c = count.clone();
        shared.set(
            std::ptr::null_mut(),
            Box::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            }),
        );
        call(&shared);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        shared.arm();
        call(&shared);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn clear_disables_callback() {
        let count = Arc::new(AtomicUsize::new(0));
        let shared = ClockShared::new();
        shared.arm();
        let c = count.clone();
        shared.set(
            std::ptr::null_mut(),
            Box::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            }),
        );
        call(&shared);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        shared.clear();
        call(&shared);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn clear_waits_for_in_flight_callback() {
        let done = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let shared = Arc::new(ClockShared::new());
        shared.arm();
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
        let t = std::thread::spawn(move || call(&sh));
        while started.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        shared.clear();
        //the in-flight callback had finished by the time clear returned
        assert_eq!(done.load(Ordering::SeqCst), 1);
        t.join().unwrap();
    }

    #[test]
    fn panicking_callback_is_reported_under_lock() {
        let shared = ClockShared::new();
        shared.arm();
        shared.set(std::ptr::null_mut(), Box::new(|_| panic!("callback failed")));
        let mut reported = None;
        shared.call_with(|_, msg| {
            //the report runs while the lock is still held
            assert!(shared.target.try_lock().is_none());
            reported = Some(msg.to_string());
        });
        assert_eq!(reported.as_deref(), Some("callback failed"));
        //the lock was released afterwards: this would deadlock otherwise
        shared.clear();
    }
}
