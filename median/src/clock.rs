//! Clocks: run a method of your object later, on Max's scheduler thread.
//!
//! Make one in `new` with [`MaxBuilder::clock`](crate::builder::Builder::clock) (a method, or a
//! closure that captures nothing) or [`clock_with`](crate::builder::Builder::clock_with) (any
//! closure), keep the [`Clock`] in your struct, and schedule it with [`Clock::delay`]:
//!
//! ```no_run
//! use median::prelude::*;
//!
//! #[derive(MaxObject)]
//! pub struct Ticker {
//!     clock: Clock,
//!     out: Outlet<Bang>,
//! }
//!
//! impl MaxObjWrapped for Ticker {
//!     fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
//!         Ok(Self {
//!             clock: b.clock(Self::tick),
//!             out: b.outlet(Outlet::bang()),
//!         })
//!     }
//! }
//!
//! #[median::methods]
//! impl Ticker {
//!     #[bang]
//!     pub fn bang(&self) {
//!         self.clock.delay(100);
//!     }
//!
//!     pub fn tick(&self) {
//!         let _ = self.out.send();
//!         self.clock.delay(100); // again in 100 ms
//!     }
//! }
//! ```
//!
//! The callback runs on the scheduler thread (which may be the audio thread with Overdrive and
//! "Audio Interrupt" on), concurrently with message methods on the main thread. It may return
//! `Result<(), E>`; an `Err` goes to the type's `report_error`. A clock made with the builder
//! does nothing until `new` has returned, and is disabled before the object is dropped, so a
//! callback never sees a half-built or freed object. A callback must not free its own object (by
//! deleting its box through scripting, say) or drop its own `Clock`: that blocks the thread
//! forever (rather than free the object under the running callback), see `ClockShared`. A
//! callback that makes Max service its scheduler (with Overdrive off, the scheduler runs in the
//! main thread's event loop, which a modal dialog runs nested) may see the clock fire again,
//! nested inside it; that is allowed.
use crate::class::ClassType;
use crate::method::MaxMethod;
use crate::{
    builder::MaxBuilder,
    object::MaxObj,
    wrapper::{FieldAttrs, MaxObjWrapped, MaxObjWrapper, Methods, ObjClass, WrapperWrapped},
};

use parking_lot::{Mutex, ReentrantMutex};
use std::{
    ffi::{CStr, c_long, c_void},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

/// A raw `t_object` pointer that may be stored in the (shared) clock target.
///
/// SAFETY (for the `Send + Sync` impls): the pointer is never dereferenced by this crate; it is
/// only handed back to the stored callback, which is `Send + Sync`. The callback only runs while
/// the owning Max object is alive: a callback holds `running` for the whole call, and the owner
/// clears the target before it drops its state, which takes `running` (waiting for an
/// in-flight callback, or blocking forever on the callback's own thread), so no callback is
/// running or starts afterwards. See [`ClockShared`].
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
    /// This is shared (`Arc`) between the clock's Max object, the [`Clock`], and the owning
    /// wrapper (which gets it from the builder). [`Queue`](crate::queue::Queue)s, and the queue
    /// of deferred calls of an object, use the same type for their qelem, with the same protocol.
    /// Lifetime protocol:
    ///
    /// * The callback does nothing until the target is *armed*. Clocks created by the builder run
    ///   inside `T::new`, before the wrapped value is initialised (a `delay` in `new` can fire on the
    ///   scheduler thread right away), so the owning wrapper arms them once construction is done.
    ///   A callback that fires before that is skipped.
    /// * A callback holds `running`, a *reentrant* lock, for the whole call. `target`'s own lock
    ///   is only held to read or replace the target, never while the callback runs.
    /// * The owning wrapper calls [`ClockShared::clear`] *before* it drops the user's `T`. From
    ///   another thread, that waits (on `running`) for an in-flight callback to finish and makes
    ///   every later callback a no-op, so the callback can never run against a freed or
    ///   half-dropped owner, even if the `Clock` was moved somewhere that outlives the object.
    ///
    /// Re-entry: a callback may run again, nested, on the thread that is already running it.
    /// That happens when user code in a callback makes Max service its scheduler or queues (a
    /// deferred `read` that opens a modal file dialog runs a nested event loop, in which a clock
    /// can fire with Overdrive off). A qelem stays set while its function runs (`ext_proto.h`:
    /// "A Qelem object is unset after its queue function has been called"), so Max should not
    /// run the same qelem nested, but that is not relied on. The nested call takes `running`
    /// again (it is reentrant) and runs to completion inside the outer one; nothing else is held
    /// across the user's code, so it doesn't deadlock. Max's own `defer` behaves the same way.
    ///
    /// Invariant relied on: **a callback never (directly, or by messages it sends that Max
    /// handles synchronously) frees its own owning object or drops its own `Clock`/`Queue`.**
    /// Both call `clear`; on the thread that is running a callback of this target (outer or
    /// nested), `clear` cannot wait for that callback (it is further up the same stack) and
    /// cannot return either, because the owner's memory is freed when it does while the
    /// callback still holds a reference into it. So `clear` blocks that thread forever: the
    /// deadlock of the original design, now limited to this case, and never a use-after-free.
    /// A deferred method (`#[defer]`, `#[defer_low]`) runs inside the callback of one of its
    /// object's two deferred-call qelems, so the same holds for it.
    ///
    /// Known gap (not fixed): a clock callback that Max has dispatched but that has not taken the
    /// lock yet still reads the clock's target object, which `Clock::drop` can free in that
    /// window, and the callback holds `running` while it calls into Max, which can deadlock
    /// against a Max lock held by a thread that waits in `clear`. See the Status section of
    /// `doc/plan-soundness-fixes.md`.
    pub struct ClockShared {
        pub(super) armed: AtomicBool,
        /// Held by a running callback (taken again by a nested one on the same thread) and by
        /// `clear` while it disables the target.
        pub(super) running: ReentrantMutex<()>,
        /// The owner and its callback. The callback is an `Arc` so that a call can take its own
        /// reference and release this lock before it runs.
        pub(super) target: Mutex<Option<(SyncPtr<max_sys::t_object>, SharedFn)>>,
    }
}
pub(crate) use shared::ClockShared;

impl ClockShared {
    pub(crate) fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            running: ReentrantMutex::new(()),
            target: Mutex::new(None),
        }
    }

    /// Allow the callback to run. Release pairs with the Acquire in `call`, so everything
    /// written before arming (the owner's initialised state) is visible to the callback.
    pub(crate) fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    /// Set the target. Only called on a new target, before its clock or qelem exists, so no
    /// callback is running.
    pub(crate) fn set(&self, target: *mut max_sys::t_object, func: ClockFn) {
        let old = self.target.lock().replace((SyncPtr(target), Arc::from(func)));
        drop(old);
    }

    /// Run the callback, if the target is armed and still set. Holds `running` for the duration,
    /// including reporting a panic against the target (as a panic in `what`).
    pub(crate) fn call(&self, what: &str) {
        self.call_with(|target, msg| {
            // SAFETY: called with `running` held, so the owner hasn't been freed (its free path
            // clears this target, which waits for `running`, or blocks forever on this thread)
            unsafe { crate::object::error(target, crate::ffi::panic_text(what, msg)) }
        })
    }

    /// `call`, with `report` called (still under `running`) if the callback panics.
    fn call_with<P: FnOnce(*mut max_sys::t_object, &str)>(&self, report: P) {
        if !self.armed.load(Ordering::Acquire) {
            return;
        }
        let _running = self.running.lock();
        //take a reference to the callback and release `target` before running it: a nested call
        //on this thread needs to read it again. (Cloning the `Arc` doesn't allocate, and this
        //reference is never the last one: the target can only be replaced by `set`, before any
        //callback, or taken by `clear`, which waits for `running` or never returns.)
        let current = self.target.lock().as_ref().map(|(t, f)| (t.0, f.clone()));
        if let Some((target, func)) = current {
            crate::ffi::guard_with((), || (func)(target), |msg| report(target, msg));
        }
    }

    /// Wait for an in-flight callback to finish and make all later callbacks no-ops.
    ///
    /// Called on a thread that is running a callback of this target (the owner freed, or the
    /// `Clock`/`Queue` dropped, from inside its own callback), this posts an error for the owner
    /// and blocks that thread forever, see the type's invariant.
    pub(crate) fn clear(&self) {
        if self.running.is_owned_by_current_thread() {
            let owner = self.target.lock().as_ref().map(|(t, _)| t.0);
            clear_inside_own_callback(owner);
        }
        let old = {
            let _running = self.running.lock();
            self.target.lock().take()
        };
        //drop the closure after releasing the locks
        drop(old);
    }
}

/// How many threads `clear_inside_own_callback` has parked.
static PARKED_IN_OWN_CALLBACK: AtomicUsize = AtomicUsize::new(0);

/// How many threads have been parked forever because an object was freed, or its `Clock`/`Queue`
/// dropped, from inside its own callback. For median's tests; not part of the API.
#[doc(hidden)]
pub fn parked_in_own_callback() -> usize {
    PARKED_IN_OWN_CALLBACK.load(Ordering::SeqCst)
}

/// `clear` from inside the target's own callback: returning would let the caller free the owner
/// while the callback, further up this thread's stack, still uses it. Block instead (the
/// documented deadlock), after saying why on stderr (Max's system console) and, for the owner
/// (`owner`, the target), in the Max console, so the user sees why the object froze.
#[cold]
fn clear_inside_own_callback(owner: Option<*mut max_sys::t_object>) -> ! {
    use std::io::Write;
    const MSG: &str = "median: an object was freed, or its Clock/Queue dropped, from inside its \
                       own clock, queue or deferred callback; blocking this thread forever \
                       instead of using freed memory";
    let _ = writeln!(std::io::stderr(), "{MSG}");
    if let Some(owner) = owner.filter(|o| !o.is_null()) {
        // SAFETY: the owner is alive: this thread is running one of its callbacks (it holds
        // `running`), and its memory is not freed before `clear` returns, on this thread, or
        // gets `running`, on another; neither happens
        unsafe { crate::object::error(owner, MSG) };
    }
    PARKED_IN_OWN_CALLBACK.fetch_add(1, Ordering::SeqCst);
    loop {
        std::thread::park();
    }
}

/// What a [`ClockShared`] calls: the owner's callback, given the owner.
pub(crate) type ClockFn = Box<dyn Fn(*mut max_sys::t_object) + Send + Sync>;

/// A [`ClockFn`] as `ClockShared` keeps it.
type SharedFn = Arc<dyn Fn(*mut max_sys::t_object) + Send + Sync>;

struct ClockInner {
    shared: Arc<ClockShared>,
}

impl ClockInner {
    pub extern "C" fn call_tramp(wrapper: &MaxObjWrapper<Self>) {
        //`call` catches and reports a panic itself, under the lock: after the lock is released
        //neither the owner nor this clock's object may be alive any more
        //the guard is for `wrapped`, which refuses an object that isn't constructed
        crate::ffi::guard(wrapper, "clock", || wrapper.wrapped().shared.call("clock"))
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
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, crate::error::NewError> {
        Ok(Self {
            shared: Arc::new(ClockShared::new()),
        })
    }
}

/// A clock: calls a method of its object on the scheduler thread after a delay. See the
/// [module documentation](self).
///
/// Make one with the builder ([`clock`](crate::builder::Builder::clock),
/// [`clock_with`](crate::builder::Builder::clock_with)) and keep it in your struct. Dropping it
/// unsets and frees the clock. A clock made with the builder is also disabled when its object is
/// freed, so a `Clock` that outlives its object does nothing when it fires. A callback must not
/// free its own object or drop its own `Clock` (that blocks the thread forever).
///
/// Creating one registers median's clock class the first time, under median's class table lock:
/// don't create a clock during class registration (`FieldAttrs::register_attrs`,
/// `Methods::register`, `class_setup`), which already holds that lock and would deadlock.
pub struct Clock {
    _target: crate::object::ObjBox<MaxObjWrapper<ClockInner>>,
    clock: *mut c_void,
}

// SAFETY: the clock pointer is only passed to `clock_*` functions, which Max lets any thread call
// (the SDK's `pictmeter~` calls `clock_delay` from its perform routine); `_target` is an
// `ObjBox` of median's own clock object, whose state is an `Arc<ClockShared>` (`Send + Sync`)
unsafe impl Send for Clock {}
// SAFETY: as above; every method takes `&self` and only calls `clock_*` functions
unsafe impl Sync for Clock {}

impl Clock {
    /// The callback target, so the owning wrapper can disable it before it is freed.
    pub(crate) fn shared(&self) -> Arc<ClockShared> {
        self._target.wrapped().shared.clone()
    }

    /// Run the callback as soon as possible (`delay(0)`).
    pub fn trigger(&self) {
        self.delay(0);
    }

    /// Run the callback `milliseconds` from now (`clock_delay`), replacing an earlier schedule.
    pub fn delay(&self, milliseconds: c_long) {
        unsafe {
            max_sys::clock_delay(self.clock, milliseconds as _);
        }
    }

    /// Run the callback `milliseconds` from now, with a fractional delay (`clock_fdelay`),
    /// replacing an earlier schedule.
    pub fn fdelay(&self, milliseconds: f64) {
        unsafe {
            max_sys::clock_fdelay(self.clock, milliseconds);
        }
    }

    /// Cancel the scheduled run, if any (`clock_unset`).
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

    /// Create a clock that calls `func(target)` when it fires, outside the builder. (The builder's
    /// [`clock`](crate::builder::Builder::clock) is the safe way.)
    ///
    /// # Safety
    /// `target` must be a valid Max object that outlives the returned `Clock`, and `func` must be
    /// sound to call with it on the scheduler thread from now on. (The builder instead disables
    /// the callbacks of its clocks before the owner is dropped.)
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

impl Drop for Clock {
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

    //run the callback with a no-op error reporter, so these tests don't need an owner object
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
            //the report runs while `running` is still held, and `target` is not
            assert!(shared.running.is_owned_by_current_thread());
            assert!(shared.target.try_lock().is_some());
            reported = Some(msg.to_string());
        });
        assert_eq!(reported.as_deref(), Some("callback failed"));
        //`running` was released afterwards: this would block forever otherwise
        assert!(!shared.running.is_locked());
        shared.clear();
    }

    #[test]
    fn a_nested_callback_on_the_same_thread_runs() {
        //a callback that makes its own target run again on the same thread (Max servicing its
        //queues from inside the callback: a modal dialog's event loop) runs nested
        let depth = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let shared = Arc::new(ClockShared::new());
        shared.arm();
        let (d, c, sh) = (depth.clone(), calls.clone(), Arc::downgrade(&shared));
        shared.set(
            std::ptr::null_mut(),
            Box::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                if d.fetch_add(1, Ordering::SeqCst) < 2 {
                    call(&sh.upgrade().unwrap());
                }
            }),
        );
        call(&shared);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        //everything was released: a clear from here doesn't block
        shared.clear();
        call(&shared);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn clear_from_inside_the_callback_blocks_that_thread() {
        //documented: the callback's thread never returns from `clear` (returning would let the
        //owner be freed under the callback), and the callback never finishes
        let cleared = Arc::new(AtomicUsize::new(0));
        let shared = Arc::new(ClockShared::new());
        shared.arm();
        let (c, sh) = (cleared.clone(), Arc::downgrade(&shared));
        shared.set(
            std::ptr::null_mut(),
            Box::new(move |_| {
                sh.upgrade().unwrap().clear();
                c.store(1, Ordering::SeqCst);
            }),
        );
        let sh = shared.clone();
        //the thread is never joined: it stays parked until the test binary exits
        let t = std::thread::spawn(move || call(&sh));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!t.is_finished());
        assert_eq!(cleared.load(Ordering::SeqCst), 0);
        //the target is still set and `running` still held by the blocked thread
        assert!(shared.running.is_locked());
        assert!(shared.target.lock().is_some());
        //blocked where it should be, not somewhere else
        assert!(parked_in_own_callback() >= 1);
    }
}
