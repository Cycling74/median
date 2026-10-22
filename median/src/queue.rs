//! Queues (Max's qelems): run a method of your object on the main thread, soon, once.
//!
//! Make one in `new` with [`MaxBuilder::queue`](crate::builder::Builder::queue) (a method, or a
//! closure that captures nothing) or [`queue_with`](crate::builder::Builder::queue_with) (any
//! closure), keep the [`Queue`] in your struct, and call [`Queue::set`] whenever there is work
//! for the main thread:
//!
//! ```no_run
//! use median::prelude::*;
//!
//! #[derive(MaxObject)]
//! pub struct Redraw {
//!     redraw: Queue,
//!     value: Int64,
//! }
//!
//! impl MaxObjWrapped for Redraw {
//!     fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
//!         Ok(Self {
//!             redraw: b.queue(Self::redraw),
//!             value: Int64::new(0),
//!         })
//!     }
//! }
//!
//! #[median::methods]
//! impl Redraw {
//!     #[int]
//!     pub fn int(&self, v: t_atom_long) {
//!         self.value.set(v);
//!         //however often this is called, `redraw` runs once for the lot
//!         self.redraw.set();
//!     }
//!
//!     pub fn redraw(&self) {
//!         post!("value is now {}", self.value.get());
//!     }
//! }
//! ```
//!
//! Setting a queue that is already set does nothing (`qelem_set`: "if the Qelem object has
//! already been set, it will not be set again"), so a burst of `set`s from any thread runs the
//! callback once, on the main thread, after the burst. A queue is unset just before its callback
//! runs, so a `set` during the callback runs it again later.
//!
//! The callback may return `Result<(), E>`; an `Err` goes to the type's `report_error`. A queue
//! made with the builder does nothing until `new` has returned and is disabled before the object
//! is dropped (a pending run is dropped), as for a [`Clock`](crate::clock::Clock). A callback
//! must not free its own object (by deleting its box through scripting, say) or drop its own
//! `Queue`: that blocks the main thread forever (rather than free the object under the running
//! callback). A callback may make Max run its queues (a modal dialog's event loop does); if the
//! queue was set again meanwhile, its callback then runs again, nested inside the first.
//!
//! **Threads.** The callback always runs on the main thread. [`set`](Queue::set),
//! [`front`](Queue::front) and [`cancel`](Queue::cancel) may be called from any thread: the SDK's
//! `simplethread` example calls `qelem_set` from a thread of its own. The SDK headers do not say
//! whether `qelem_set` is real-time safe (lock- and allocation-free), so calling it from
//! `perform` (the audio thread) is **unverified**; see [`crate::fifo`] for the audio-to-main
//! pattern and its caveat.

use crate::clock::{ClockFn, ClockShared};
use std::{ffi::c_void, ptr::NonNull, sync::Arc};

/// A queue: calls a method of its object once on the main thread after [`set`](Self::set). See
/// the [module documentation](self).
pub struct Queue {
    qelem: NonNull<c_void>,
    shared: Arc<ClockShared>,
    /// The strong count of `shared` that the qelem holds (its `obj` argument). Released in
    /// `drop`, after the qelem is freed.
    qelem_ref: *const ClockShared,
}

// SAFETY: the qelem pointer is only passed to `qelem_*` functions, which may be called from any
// thread (the SDK's `simplethread` calls `qelem_set` from its own thread); `shared` is
// `Send + Sync`, and `qelem_ref` is only turned back into its `Arc` in `drop`
unsafe impl Send for Queue {}
// SAFETY: as above; every method takes `&self` and only calls `qelem_*` functions
unsafe impl Sync for Queue {}

/// What Max calls when the qelem runs (on the main thread), with the `obj` given to `qelem_new`.
extern "C" fn queue_tramp(shared: *const ClockShared) {
    crate::ffi::guard_global("queue", (), || {
        // SAFETY: `shared` is the strong count `Queue::new_unarmed` gave the qelem, released only
        // after `qelem_free`, which cancels a pending run (and the qelem runs on the main thread,
        // where the owner is freed)
        unsafe { (*shared).call("queue") }
    })
}

impl Queue {
    /// A queue whose callback is `func(target)`, which does nothing until `shared().arm()`.
    ///
    /// # Panics
    /// If Max does not make the qelem.
    ///
    /// # Safety
    /// `target` must be valid whenever the callback can run: the caller disables it (by clearing
    /// `shared()`) before `target` is freed, and arms it once `target` may be called back.
    pub(crate) unsafe fn new_unarmed(target: *mut max_sys::t_object, func: ClockFn) -> Self {
        let shared = Arc::new(ClockShared::new());
        shared.set(target, func);
        let qelem_ref = Arc::into_raw(shared.clone());
        // SAFETY: `queue_tramp` takes the `obj` pointer, which is how Max calls a qelem function
        let method = unsafe {
            crate::method::erase(queue_tramp as extern "C" fn(*const ClockShared))
        };
        // SAFETY: `qelem_ref` stays valid until `drop` frees the qelem
        let qelem = unsafe { max_sys::qelem_new(qelem_ref as *mut c_void, Some(method)) };
        let Some(qelem) = NonNull::new(qelem) else {
            // SAFETY: the qelem wasn't made, so nothing else holds this count
            drop(unsafe { Arc::from_raw(qelem_ref) });
            panic!("median: Max did not make a qelem");
        };
        Self {
            qelem,
            shared,
            qelem_ref,
        }
    }

    /// The callback target, so the owning wrapper can arm and disable it.
    pub(crate) fn shared(&self) -> Arc<ClockShared> {
        self.shared.clone()
    }

    /// Run the callback on the main thread, at the back of Max's queue, unless it is already set
    /// (`qelem_set`).
    pub fn set(&self) {
        // SAFETY: `qelem` lives until `drop`
        unsafe { max_sys::qelem_set(self.qelem.as_ptr().cast()) }
    }

    /// Like [`set`](Self::set), at the front of Max's queue (`qelem_front`). The SDK asks to use
    /// it "only for special time-critical applications". What it does to a queue that is already
    /// set is **unverified in Max**: `ext_proto.h` only says it is "identical to `qelem_set()`"
    /// apart from the position, so it is assumed (and modelled by `median-mock`) that the queue
    /// is not set again and stays where it is, rather than being moved to the front.
    pub fn front(&self) {
        // SAFETY: as in `set`
        unsafe { max_sys::qelem_front(self.qelem.as_ptr().cast()) }
    }

    /// Cancel the pending run, if any (`qelem_unset`).
    pub fn cancel(&self) {
        // SAFETY: as in `set`
        unsafe { max_sys::qelem_unset(self.qelem.as_ptr().cast()) }
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        // SAFETY: `qelem` was made by `qelem_new` and is freed exactly once, here; this cancels a
        // pending run
        unsafe { max_sys::qelem_free(self.qelem.as_ptr().cast()) };
        //wait for a callback that is running (on another thread than this one)
        self.shared.clear();
        // SAFETY: the qelem is gone, so its count can be released
        drop(unsafe { Arc::from_raw(self.qelem_ref) });
    }
}
