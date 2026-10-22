//! Deferred methods: message methods that run on Max's main thread.
//!
//! Max calls message methods on the thread the message was sent from: the main thread for a
//! message from the patcher, the scheduler thread for one sent by a `metro` or a clock (with
//! Overdrive on). Some work belongs on the main thread: opening a dialog or a window, reading a
//! file, anything that touches the user interface. Mark such a method `#[defer]` or `#[defer_low]`
//! next to its message marker:
//!
//! ```no_run
//! use median::prelude::*;
//!
//! #[derive(MaxObject)]
//! #[max(name = "reader")]
//! pub struct Reader {
//!     obj: ObjRef,
//! }
//!
//! impl MaxObjWrapped for Reader {
//!     fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
//!         Ok(Self { obj: b.obj() })
//!     }
//! }
//!
//! #[median::methods]
//! impl Reader {
//!     /// `read` or `read <file>`: always later, on the main thread
//!     #[sel("read", gimme)]
//!     #[defer_low]
//!     pub fn read(&self, _sel: &SymbolRef, atoms: &[Atom]) {
//!         self.obj.post(format!("reading {} file(s)", atoms.len()));
//!     }
//!
//!     /// a bang from the scheduler thread is moved to the main thread
//!     #[bang]
//!     #[defer]
//!     pub fn bang(&self) {
//!         self.obj.post("bang, on the main thread");
//!     }
//! }
//! ```
//!
//! By hand, the `Class` methods `add_bang_deferred`, `add_int_deferred`, `add_float_deferred`,
//! `add_sym_deferred`, `add_list_deferred`, `add_any_deferred`, `add_sel_deferred` and
//! `add_sel_gimme_deferred` take the same arguments as their undeferred twins plus a [`Defer`].
//!
//! The two modes are those of the SDK's `defer_low` and `defer`, with one difference:
//!
//! * [`Defer::Low`] (`#[defer_low]`): always queued, at the back of the main thread's queue,
//!   even when the message arrives on the main thread. The SDK recommends this for messages that
//!   open a dialog (`read`, `write`).
//! * [`Defer::Front`] (`#[defer]`): run at once when the message arrives on the main thread,
//!   otherwise queued at the front. (Max's `defer` only queues from the scheduler thread; this
//!   queues from any thread that isn't the main one.)
//!
//! How it works, and why it is safe: each object whose class has deferred methods has its own
//! queue of pending calls and one qelem, owned by the wrapper. A deferred call copies its
//! arguments (numbers, symbols, atoms) into the queue and sets the qelem; when the qelem runs, on
//! the main thread, the pending calls run in order. Nothing is handed to Max's `defer`, so a
//! pending call can't outlive its object: when the object is freed, the qelem is disabled and
//! freed and the pending calls are dropped without running. Pending `Front` calls run before
//! pending `Low` ones of the same object, latest first (as several of Max's `defer`s are); the
//! qelem is set with `qelem_front` for them, but if it is already waiting at the back it stays
//! there.
//!
//! Rules:
//!
//! * **Never send a deferred message from `perform`** (the audio thread). Queuing allocates
//!   (the copy of the arguments and the boxed call) and takes a lock, on the thread that sends
//!   the message. Use a [`fifo`](crate::fifo) and a [`Queue`](crate::queue::Queue) to get data
//!   from the audio thread to the main thread.
//! * A deferred method must not free its own object (by deleting its box through scripting,
//!   say): it runs inside the object's queue callback, and freeing the object waits for that
//!   callback to finish, which deadlocks (the same rule as for clocks and queues).
//! * An `Err` from a deferred method goes to `report_error`, and a panic is posted for the
//!   object, when it runs.

use crate::atom::Atom;
use crate::queue::Queue;
use std::{collections::VecDeque, fmt::Display};

/// When a deferred method runs; see the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Defer {
    /// `#[defer]`: at once on the main thread, otherwise queued at the front.
    Front,
    /// `#[defer_low]`: always queued at the back.
    Low,
}

/// The function of a pending call: the user's method with its arguments, given the object and
/// where to report an `Err`.
type CallFn<T> = Box<dyn FnOnce(&T, &mut dyn FnMut(&dyn Display)) + Send>;

/// A call of a deferred method, with its arguments copied, waiting to run on the main thread.
/// Made by the deferring trampolines; not part of the API.
#[doc(hidden)]
pub struct DeferredCall<T> {
    what: &'static str,
    call: CallFn<T>,
}

impl<T> DeferredCall<T> {
    pub(crate) fn new(
        what: &'static str,
        call: impl FnOnce(&T, &mut dyn FnMut(&dyn Display)) + Send + 'static,
    ) -> Self {
        Self {
            what,
            call: Box::new(call),
        }
    }

    /// The method's name, for a panic message.
    pub(crate) fn what(&self) -> &'static str {
        self.what
    }

    /// Run it with the object, giving an `Err` to `report`.
    pub(crate) fn run(self, t: &T, report: &mut dyn FnMut(&dyn Display)) {
        (self.call)(t, report)
    }
}

/// `DeferQueue` is in the signature of the sealed `WrapperInternal` trait, which is `pub` (in a
/// private module), so it has to be `pub` too; it is declared in this private module and only
/// re-exported at `pub(crate)`, as `ClockShared` is.
mod pending {
    use super::*;

    /// The pending deferred calls of one object and the qelem that runs them. Owned by the wrapper:
    /// dropped (pending calls with it, then the qelem freed) when the object is freed, after the
    /// qelem's callback has been disabled.
    pub struct DeferQueue<T> {
        calls: parking_lot::Mutex<VecDeque<DeferredCall<T>>>,
        queue: Queue,
    }

    impl<T> DeferQueue<T> {
        /// `queue` is the object's qelem, whose callback runs `take()`'s calls.
        pub(crate) fn new(queue: Queue) -> Self {
            Self {
                calls: parking_lot::Mutex::new(VecDeque::new()),
                queue,
            }
        }

        /// Queue a call and set the qelem.
        pub(crate) fn push(&self, mode: Defer, call: DeferredCall<T>) {
            {
                let mut calls = self.calls.lock();
                match mode {
                    Defer::Front => calls.push_front(call),
                    Defer::Low => calls.push_back(call),
                }
            }
            match mode {
                Defer::Front => self.queue.front(),
                Defer::Low => self.queue.set(),
            }
        }

        /// The pending calls, to run now. Calls queued while they run wait for the next run of the
        /// qelem (which was unset before its callback was called, so the push sets it again).
        pub(crate) fn take(&self) -> VecDeque<DeferredCall<T>> {
            std::mem::take(&mut *self.calls.lock())
        }

        #[cfg(test)]
        pub(crate) fn len(&self) -> usize {
            self.calls.lock().len()
        }
    }
}
pub(crate) use pending::DeferQueue;

/// A copy of the atoms of a message, kept for a deferred call.
pub(crate) struct OwnedAtoms(Vec<Atom>);

// SAFETY: the atoms are plain values. Numbers are copied; symbols are interned by Max and never
// freed (`SymbolRef` is `Send` for the same reason); an `A_OBJ` atom's pointer is only copied,
// as Max's own `defer` copies atoms, and can only be dereferenced through `unsafe` code
unsafe impl Send for OwnedAtoms {}

#[cfg(test)]
pub(crate) static LIVE_OWNED_ATOMS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

impl OwnedAtoms {
    pub(crate) fn copy(atoms: &[Atom]) -> Self {
        #[cfg(test)]
        LIVE_OWNED_ATOMS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(atoms.iter().map(|a| Atom { value: a.value }).collect())
    }

    pub(crate) fn as_slice(&self) -> &[Atom] {
        &self.0
    }
}

#[cfg(test)]
impl Drop for OwnedAtoms {
    fn drop(&mut self) {
        LIVE_OWNED_ATOMS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        builder::MaxBuilder,
        class::Class,
        error::{MaxResult, NewError},
        object::{MaxObj, ObjRef},
        wrapper::{FieldAttrs, MaxObjWrapped, MaxObjWrapper, Methods, ObjClass, WrapperWrapped},
    };
    use median_mock::Atom as MockAtom;
    use std::ffi::CStr;
    use std::sync::{Once, atomic::Ordering};

    struct D {
        obj: ObjRef,
    }

    impl ObjClass for D {
        type Wrapper = MaxObjWrapper<Self>;
        fn class_name() -> &'static CStr {
            c"defer.unit"
        }
    }
    impl FieldAttrs for D {}
    impl Methods for D {
        fn register<W>(c: &mut Class<W>) -> MaxResult<()>
        where
            W: WrapperWrapped<Self> + MaxObj,
        {
            c.add_list_deferred(Self::list, Defer::Low)
        }
    }
    impl MaxObjWrapped for D {
        fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
            Ok(Self { obj: b.obj() })
        }
    }
    impl D {
        fn list(&self, atoms: &[Atom]) {
            self.obj.post(format!("list of {}", atoms.len()));
        }
    }

    fn register() {
        static ONCE: Once = Once::new();
        ONCE.call_once(MaxObjWrapper::<D>::register);
    }

    fn pending(obj: *mut std::ffi::c_void) -> usize {
        // SAFETY: `obj` is a live `D`
        let w = unsafe { &*obj.cast::<MaxObjWrapper<D>>() };
        w.pending_deferred()
    }

    #[test]
    fn a_pending_call_is_dropped_with_its_atoms_at_free() {
        let k = median_mock::lock();
        register();
        let live = LIVE_OWNED_ATOMS.load(Ordering::SeqCst);
        let obj = k.new_object("defer.unit", &[]);
        assert!(!obj.is_null());
        k.send_list(obj, &[MockAtom::Long(1), MockAtom::Sym("a".into())]).unwrap();
        k.send_list(obj, &[MockAtom::Float(2.)]).unwrap();
        assert_eq!(pending(obj), 2);
        assert_eq!(LIVE_OWNED_ATOMS.load(Ordering::SeqCst), live + 2);
        assert!(k.free_object(obj));
        //dropped, not run
        assert_eq!(LIVE_OWNED_ATOMS.load(Ordering::SeqCst), live);
        assert_eq!(k.run_main_queue(), 0);
        assert!(k.console_log().is_empty(), "{:?}", k.console_log());

        //and when they run, the copies are freed afterwards
        let obj = k.new_object("defer.unit", &[]);
        k.send_list(obj, &[MockAtom::Long(1)]).unwrap();
        assert_eq!(k.run_main_queue(), 1);
        assert_eq!(pending(obj), 0);
        assert_eq!(LIVE_OWNED_ATOMS.load(Ordering::SeqCst), live);
        assert_eq!(k.take_console_log()[0].text, "list of 1");
        assert!(k.free_object(obj));
    }
}
