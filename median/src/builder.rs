//! The builder your `new` gets: inlets, outlets, clocks, queues and buffer references are made
//! with it.
//!
//! [`MaxBuilder<T>`] for a Max object, [`MSPBuilder<T>`] for an MSP object (which also takes
//! signal inlets and outlets). Unlike the Max SDK, inlets and outlets are described from left to
//! right; see [`crate::inlet`] and [`crate::outlet`].
use crate::{
    atom::Atom,
    buffer::{BufferRef, BufferReference},
    clock::{Clock, ClockFn, ClockShared},
    inlet::{FloatCB, InletFor, InletId, InletKind, IntCB, Proxy},
    method::MethodResult,
    notify::{Attachment, AttachmentError, Registration, RegistrationError, Subscription},
    object::ObjRef,
    outlet::{Outlet, OutletDesc, OutletFor, OutletKind},
    queue::Queue,
    symbol::SymbolRef,
    wrapper::{IoLet, MSPObjWrapper, MaxObjWrapper, WrapperWrapped},
};
use std::{marker::PhantomData, ptr::NonNull, sync::Arc};

pub type ManagedBufferRef = Arc<dyn BufferReference>;
pub(crate) type ManagedBufferRefInternal = Arc<BufferRef>;

/// The most `int` (and, separately, `float`) inlets an object can have: Max has the messages
/// `in1`..`in9` and `ft1`..`ft9`.
pub const MAX_TYPED_INLETS: usize = 9;

mod sealed {
    pub trait Sealed {}
}

/// The kind of object a [`Builder`] builds: [`Max`] or [`Msp`].
pub trait BuilderKind: sealed::Sealed + 'static {
    #[doc(hidden)]
    const MSP: bool;
}

/// A Max object (a `t_object`).
pub enum Max {}
/// An MSP object (a `t_pxobject`).
pub enum Msp {}

impl sealed::Sealed for Max {}
impl sealed::Sealed for Msp {}
impl BuilderKind for Max {
    const MSP: bool = false;
}
impl BuilderKind for Msp {
    const MSP: bool = true;
}

/// The builder for a Max object's `new`.
pub type MaxBuilder<T> = Builder<T, Max>;
/// The builder for an MSP object's `new`.
pub type MSPBuilder<T> = Builder<T, Msp>;

/// The builder your `new` is given (as [`MaxBuilder<Self>`] or [`MSPBuilder<Self>`]).
///
/// It describes the object's inlets ([`inlet`](Self::inlet)) and makes its outlets
/// ([`outlet`](Self::outlet)), clocks and buffer references, and hands out the object's
/// [`ObjRef`] ([`obj`](Self::obj)). Outlets, clocks and buffer references exist as soon as they are
/// made; the inlets are made when `new` has returned, in the order Max needs.
pub struct Builder<T, K> {
    obj: NonNull<max_sys::t_object>,
    sym: SymbolRef,
    args: *const Atom,
    nargs: usize,
    inlets: Vec<InletEntry<T>>,
    outlets: Vec<IoletInfo>,
    signal_outlets: usize,
    int_inlets: usize,
    float_inlets: usize,
    /// SDK-owned iolets will be inserted to the left of these custom iolets.
    external_inlets: Option<usize>,
    external_outlets: usize,
    prebuilt: Option<Iolets<T>>,
    /// The first mistake in the description; construction fails with it.
    error: Option<String>,
    buffer_refs: Vec<ManagedBufferRefInternal>,
    /// The callback targets of the clocks and queues made with the builder: the wrapper arms them
    /// when construction has finished and disables them before the object is dropped.
    targets: Vec<Arc<ClockShared>>,
    /// The two qelems of the object's deferred-call queue, if its class has deferred methods.
    defer_queue: Option<[Queue; 2]>,
    /// `z_dsp_setup` was called and the builder still owes the `z_dsp_free`: until
    /// [`commit`](Self::commit), dropping the builder (construction failed or panicked) calls
    /// it. Afterwards the wrapper's `free` does.
    dsp_pending: bool,
    _kind: PhantomData<K>,
}

impl<T, K> Drop for Builder<T, K> {
    fn drop(&mut self) {
        if self.dsp_pending {
            // SAFETY: `z_dsp_setup` was called on `obj`, a valid `t_pxobject` (see `new`), and the
            // object did not get as far as being constructed, so its `free` won't call this
            unsafe { max_sys::z_dsp_free(self.obj.as_ptr().cast()) };
        }
    }
}

struct InletEntry<T> {
    position: usize,
    /// 1..=9 for `int` and `float` inlets
    id: usize,
    kind: InletKind<T>,
    assist: Option<Box<str>>,
    hot: Option<bool>,
}

impl<T, K: BuilderKind> Builder<T, K> {
    /// # Safety
    /// `owner` must point to a valid object (a `t_pxobject` if `K` is [`Msp`]) that is being
    /// constructed and outlives the builder; `args` must stay valid as long as the builder.
    pub(crate) unsafe fn new(
        owner: NonNull<max_sys::t_object>,
        sym: SymbolRef,
        args: &[Atom],
    ) -> Self {
        Self {
            obj: owner,
            sym,
            args: args.as_ptr(),
            nargs: args.len(),
            inlets: Vec::new(),
            outlets: Vec::new(),
            signal_outlets: 0,
            int_inlets: 0,
            float_inlets: 0,
            external_inlets: None,
            external_outlets: 0,
            prebuilt: None,
            error: None,
            buffer_refs: Vec::new(),
            targets: Vec::new(),
            defer_queue: None,
            dsp_pending: false,
            _kind: PhantomData,
        }
    }

    fn fail(&mut self, msg: impl Into<String>) {
        if self.error.is_none() {
            self.error = Some(msg.into());
        }
    }

    /// Describe the next inlet, left to right, and return its position. See [`crate::inlet`].
    ///
    /// A mistake (an [`Inlet::default`](crate::inlet::Inlet::default) that isn't first, a
    /// signal inlet after another kind, more than 9 `int` or `float` inlets) makes the object
    /// fail to be created, with an error in the Max console, when `new` returns.
    pub fn inlet<I: InletFor<T, K>>(&mut self, inlet: I) -> InletId {
        let inlet = inlet.into_inlet();
        //only an `Inlet::default()` or a signal inlet can describe position 0
        let zero_described = matches!(
            self.inlets.first().map(|e| &e.kind),
            Some(InletKind::Default | InletKind::Signal)
        );
        let n = self.inlets.len();
        let mut id = 0;
        let position = match &inlet.kind {
            InletKind::Default => {
                if self.external_inlets.is_some() {
                    self.fail(
                        "the SDK owns inlet 0; extra Jitter inlets must be int, float or proxy",
                    );
                } else if n != 0 {
                    self.fail("Inlet::default() describes inlet 0 and must be the first inlet");
                }
                0
            }
            InletKind::Signal => {
                if self
                    .inlets
                    .iter()
                    .any(|e| !matches!(e.kind, InletKind::Signal))
                {
                    self.fail("signal inlets must come before the other inlets");
                }
                n
            }
            kind => {
                let count = match kind {
                    InletKind::Int(_) => Some(&mut self.int_inlets),
                    InletKind::Float(_) => Some(&mut self.float_inlets),
                    _ => None,
                };
                if let Some(count) = count {
                    *count += 1;
                    id = *count;
                    if id > MAX_TYPED_INLETS {
                        self.fail(format!(
                            "at most {MAX_TYPED_INLETS} int and {MAX_TYPED_INLETS} float inlets are supported"
                        ));
                    }
                }
                if let Some(count) = self.external_inlets {
                    count + n
                } else if zero_described {
                    n
                } else {
                    n + 1
                }
            }
        };
        self.inlets.push(InletEntry {
            position,
            id,
            kind: inlet.kind,
            assist: inlet.assist,
            hot: inlet.hot,
        });
        InletId(position)
    }

    /// Make the next outlet, left to right, and return it: keep it in your struct to send
    /// through it. Signal outlets ([`Outlet::signal`]) are for MSP objects only.
    pub fn outlet<O: OutletFor<K>>(&mut self, desc: OutletDesc<O>) -> Outlet<O> {
        // SAFETY: `obj` is valid and under construction (see `new`)
        let outlet = unsafe { Outlet::<O>::append(self.obj.as_ptr()) };
        if outlet.is_null() {
            self.fail(format!(
                "Max did not make {} outlet {}",
                O::NAME,
                self.outlets.len()
            ));
        }
        if O::NAME == <crate::outlet::Signal as OutletKind>::NAME {
            self.signal_outlets += 1;
        }
        self.outlets.push(IoletInfo {
            assist: desc.assist,
            default: O::NAME.into(),
            hot: false,
        });
        outlet
    }

    /// Get the arguments that were passed to this object on creation.
    pub fn creation_args(&self) -> &[Atom] {
        if self.nargs == 0 {
            return &[];
        }
        // SAFETY: `args` is valid for `nargs` atoms while the builder lives (see `new`)
        unsafe { std::slice::from_raw_parts(self.args, self.nargs) }
    }

    /// Get the symbol that was used to create this object.
    pub fn creation_symbol(&self) -> SymbolRef {
        self.sym
    }

    /// Get a handle to the object under construction. Store it in your struct to post, touch
    /// attributes and so on later. It can be called any number of times, each call returns a
    /// handle to the same object.
    pub fn obj(&mut self) -> ObjRef {
        // SAFETY: `obj` is a valid object (for MSP, its `t_pxobject` header starts with the
        // `t_object`) that Max allocated before `T::new` ran, so it lives until Max frees it,
        // which is after the user's struct (the only place the handle is meant to be kept) is
        // dropped; see `new` and the limits documented on `ObjRef`
        unsafe { ObjRef::from_raw(self.obj) }
    }

    /// Get the Max object for the wrapper of this object.
    ///
    /// # Safety
    /// The object is under construction: it must not be freed, and Max must not be made to call
    /// its methods, through the pointer.
    pub unsafe fn max_obj(&mut self) -> *mut max_sys::t_object {
        self.obj.as_ptr()
    }

    /// Try to register your object in the given namespace with the given name.
    pub fn try_register(
        &self,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<Registration, RegistrationError> {
        // SAFETY: `obj` is valid (see `new`)
        unsafe { Registration::try_register(self.obj.as_ptr(), namespace, name) }
    }

    /// Attach to an object with the given name and namespace.
    pub fn attach(
        &mut self,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<Attachment, AttachmentError> {
        // SAFETY: `obj` is valid (see `new`)
        unsafe { Attachment::try_attach(self.obj.as_ptr(), namespace, name) }
    }

    /// Subscribe to attach to an object with the given name in the given namespace.
    pub fn subscribe(
        &mut self,
        namespace: SymbolRef,
        name: SymbolRef,
        class_name: Option<SymbolRef>,
    ) -> Subscription {
        // SAFETY: `obj` is valid (see `new`)
        unsafe { Subscription::new(self.obj.as_ptr(), namespace, name, class_name) }
    }

    /// Get a managed buffer reference.
    pub fn with_buffer(&mut self, name: Option<SymbolRef>) -> ManagedBufferRef {
        // SAFETY: `obj` is valid (see `new`), and the wrapper keeps the reference no longer than
        // the object
        let b = Arc::new(unsafe { BufferRef::new(self.obj.as_ptr(), name) });
        self.buffer_refs.push(b.clone());
        b
    }
}

macro_rules! callback_methods {
    ($kind:ty, $bound:path, $wrapper:ident) => {
        impl<T: $bound> Builder<T, $kind> {
            /// Make a [`Clock`] that calls `f` with the object when it fires (on the scheduler
            /// thread). `f` is a method (`Self::tick`) or a closure that captures nothing, as for
            /// the `Class::add_*` methods; [`clock_with`](Self::clock_with) takes any closure. It
            /// may return `Result<(), E>`: an `Err` goes to the type's `report_error`.
            ///
            /// The clock is only live once `new` has returned: if it fires earlier (a `delay(0)` in
            /// `new`), the callback is skipped. It is disabled before the object is dropped.
            ///
            /// A closure that captures something is a compile error here (use `clock_with`):
            ///
            /// ```compile_fail,E0080
            /// # use median::prelude::*;
            /// # #[derive(MaxObject)]
            /// # #[max(no_methods)]
            /// # pub struct Example;
            /// # impl MaxObjWrapped for Example {
            /// #     fn new(_b: &mut MaxBuilder<Self>) -> Result<Self, NewError> { Ok(Example) }
            /// # }
            /// fn make(b: &mut MaxBuilder<Example>) -> Clock {
            ///     let n = std::hint::black_box(3);
            ///     b.clock(move |_: &Example| { std::hint::black_box(n); })
            /// }
            /// # fn main() { let _ = make as fn(&mut MaxBuilder<Example>) -> Clock; }
            /// ```
            pub fn clock<F, R>(&mut self, f: F) -> Clock
            where
                F: Fn(&T) -> R + Copy + Send + Sync + 'static,
                R: MethodResult,
            {
                const {
                    assert!(
                        std::mem::size_of::<F>() == 0,
                        "Builder::clock takes a method or a closure that captures nothing; use clock_with for a closure that captures"
                    )
                };
                self.clock_with(f)
            }

            /// [`clock`](Self::clock) with any closure (one that captures state of its own, say).
            pub fn clock_with<F, R>(&mut self, f: F) -> Clock
            where
                F: Fn(&T) -> R + Send + Sync + 'static,
                R: MethodResult,
            {
                // SAFETY: the clock's target is this object, see `clock_for`
                let cb = unsafe { callback::<$wrapper<T>, T, F, R>(f) };
                self.clock_for(cb)
            }

            /// Like [`clock`](Self::clock), but the callback runs on the main thread, at low
            /// priority after the scheduler tick (min-devkit's `defer_delivery`). The clock's
            /// tick only sets a qelem, and Max runs `f` from it.
            ///
            /// Ticks that arrive while a call is already waiting for the main thread coalesce
            /// into one call. A tick that arrives while `f` is running is lost for that round,
            /// because `qelem_set` from inside the qelem's own function does nothing (see
            /// [`Queue`] and `ext_proto.h`, `qelem_set`): a deferred clock that re-arms itself
            /// from `f` with a delay shorter than `f`'s run time can miss ticks.
            /// [`Clock::cancel`] also cancels a tick that is waiting for the main thread (a
            /// tick the scheduler thread has already dispatched can still set the qelem after
            /// it, so one call can follow `cancel`). The clock's `delay`, `fdelay` and
            /// `trigger` can be called from any thread, `perform` included: they only schedule
            /// the Max clock, and the scheduler thread calls `qelem_set`. With Scheduler in
            /// Audio Interrupt that thread is the audio thread, so `qelem_set` then runs there
            /// (the same pattern as min-devkit's `defer_delivery`; its real-time safety is
            /// unverified, see `AGENTS.md`). `cancel` also calls `qelem_unset`: don't call it
            /// from `perform`, for the same reason.
            ///
            /// As for `clock`, the clock is live once `new` has returned and is disabled (with
            /// its pending call) before the object is dropped; `f` may return `Result<(), E>`.
            /// `f` is a method or a closure that captures nothing;
            /// [`clock_deferred_with`](Self::clock_deferred_with) takes any closure.
            pub fn clock_deferred<F, R>(&mut self, f: F) -> Clock
            where
                F: Fn(&T) -> R + Copy + Send + Sync + 'static,
                R: MethodResult,
            {
                const {
                    assert!(
                        std::mem::size_of::<F>() == 0,
                        "Builder::clock_deferred takes a method or a closure that captures nothing; use clock_deferred_with for a closure that captures"
                    )
                };
                self.clock_deferred_with(f)
            }

            /// [`clock_deferred`](Self::clock_deferred) with any closure.
            pub fn clock_deferred_with<F, R>(&mut self, f: F) -> Clock
            where
                F: Fn(&T) -> R + Send + Sync + 'static,
                R: MethodResult,
            {
                // SAFETY: the target of the queue is this object, see `clock_deferred_for`
                let cb = unsafe { callback::<$wrapper<T>, T, F, R>(f) };
                self.clock_deferred_for(cb)
            }

            /// Make a [`Queue`] that calls `f` with the object on the main thread after
            /// [`Queue::set`]. `f` is a method or a closure that captures nothing, as for
            /// [`clock`](Self::clock); [`queue_with`](Self::queue_with) takes any closure. It may
            /// return `Result<(), E>`: an `Err` goes to the type's `report_error`.
            ///
            /// The queue is only live once `new` has returned (a run before that is skipped), and
            /// is disabled before the object is dropped (a pending run is dropped).
            pub fn queue<F, R>(&mut self, f: F) -> Queue
            where
                F: Fn(&T) -> R + Copy + Send + Sync + 'static,
                R: MethodResult,
            {
                const {
                    assert!(
                        std::mem::size_of::<F>() == 0,
                        "Builder::queue takes a method or a closure that captures nothing; use queue_with for a closure that captures"
                    )
                };
                self.queue_with(f)
            }

            /// [`queue`](Self::queue) with any closure.
            pub fn queue_with<F, R>(&mut self, f: F) -> Queue
            where
                F: Fn(&T) -> R + Send + Sync + 'static,
                R: MethodResult,
            {
                // SAFETY: the queue's target is this object, see `queue_for`
                let cb = unsafe { callback::<$wrapper<T>, T, F, R>(f) };
                self.queue_for(cb)
            }
        }
    };
}

callback_methods!(Max, crate::wrapper::MaxObjWrapped, MaxObjWrapper);
callback_methods!(Msp, crate::wrapper::MSPObjWrapped, MSPObjWrapper);

/// The callback of a clock or queue of an object of wrapper `W`: calls `f` with the wrapped value
/// and gives an `Err` to the type's `report_error`.
///
/// # Safety
/// The callback must only be called with a pointer to a live, constructed `W` (the builder's
/// targets are armed after construction and disabled before the object is dropped).
pub(crate) unsafe fn callback<W, T, F, R>(f: F) -> ClockFn
where
    W: WrapperWrapped<T> + 'static,
    T: 'static,
    F: Fn(&T) -> R + Send + Sync + 'static,
    R: MethodResult,
{
    Box::new(move |obj| {
        // SAFETY: the caller's contract
        let w: &W = unsafe { &*obj.cast::<W>() };
        f(w.wrapped()).report_if_err(&mut |e| w.report_error(e));
    })
}

impl<T, K: BuilderKind> Builder<T, K> {
    /// A clock whose callback is `func`, called with this object.
    fn clock_for(&mut self, func: ClockFn) -> Clock {
        // SAFETY: `obj` is valid for the life of the object; the wrapper clears the clock's
        // target (via `targets`) before it drops `T`, so the callback never sees a freed owner.
        // The callback is only armed once construction finished, when `obj` is complete.
        let clock = unsafe { Clock::new_unarmed(self.obj.as_ptr(), func) };
        self.targets.push(clock.shared());
        clock
    }

    /// A clock whose tick sets a qelem whose callback is `func`, called with this object (on the
    /// main thread).
    fn clock_deferred_for(&mut self, func: ClockFn) -> Clock {
        let queue = self.queue_for(func);
        let handle = queue.handle();
        // the clock's callback only runs while its `Clock` is alive, and the `Clock` owns `queue`
        // (dropped after the clock's target is cleared), so `handle` is valid whenever it runs
        let tick: ClockFn = Box::new(move |_| {
            // SAFETY: as above
            unsafe { handle.set() }
        });
        self.clock_for(tick).with_deferred(queue)
    }

    /// A queue whose callback is `func`, called with this object.
    pub(crate) fn queue_for(&mut self, func: ClockFn) -> Queue {
        // SAFETY: as for `clock_for`: armed after construction, cleared (via `targets`) before
        // `T` is dropped
        let queue = unsafe { Queue::new_unarmed(self.obj.as_ptr(), func) };
        self.targets.push(queue.shared());
        queue
    }
}

impl<T> Builder<T, Msp> {
    /// Get the MSP object for the wrapper of this object.
    ///
    /// # Safety
    /// As [`max_obj`](Self::max_obj).
    pub unsafe fn msp_obj(&mut self) -> *mut max_sys::t_pxobject {
        self.obj.as_ptr().cast()
    }
}

/// The assist text and hot flag of one inlet or outlet.
pub(crate) struct IoletInfo {
    assist: Option<Box<str>>,
    /// What Max is shown without an assist text: the kind, or "inlet N"
    default: Box<str>,
    hot: bool,
}

/// What the builder made: the iolet table and the inlet callbacks, owned by the wrapper.
///
/// The table is filled once, at construction. (A later `dynlet` API, for iolets added after
/// construction, would make it updatable here.)
pub(crate) struct Iolets<T> {
    host_inletinfo: Option<(Option<usize>, InletInfoFn)>,
    ints: [Option<IntCB<T>>; MAX_TYPED_INLETS],
    floats: [Option<FloatCB<T>>; MAX_TYPED_INLETS],
    /// by position
    inlets: Vec<IoletInfo>,
    /// by position
    outlets: Vec<IoletInfo>,
    pub(crate) signal_inlets: usize,
    pub(crate) signal_outlets: usize,
    //freed (object_free) when the wrapper's internals are dropped, before Max frees the object
    _proxies: Vec<Proxy>,
}

pub(crate) type InletInfoFn = unsafe extern "C" fn(
    *mut max_sys::t_object,
    *mut core::ffi::c_void,
    core::ffi::c_long,
    *mut core::ffi::c_char,
);

impl<T> Iolets<T> {
    pub(crate) fn set_host_inletinfo(&mut self, count: Option<usize>, method: Option<InletInfoFn>) {
        self.host_inletinfo = method.map(|method| (count, method));
    }
    /// # Safety
    /// All arguments are from Max's live inletinfo callback, including its writable result.
    pub(crate) unsafe fn host_inletinfo(
        &self,
        owner: *mut max_sys::t_object,
        b: *mut core::ffi::c_void,
        index: core::ffi::c_long,
        cold: *mut core::ffi::c_char,
    ) -> bool {
        if let Some((count, method)) = self.host_inletinfo {
            if count.is_none_or(|count| index >= 0 && (index as usize) < count) {
                unsafe {
                    method(owner, b, index, cold);
                }
                return true;
            }
        }
        false
    }
    /// Call the callback of the `int` inlet `in<id>`.
    pub(crate) fn call_int(&self, t: &T, id: usize, v: max_sys::t_atom_long) {
        if let Some(Some(f)) = id.checked_sub(1).and_then(|i| self.ints.get(i)) {
            f(t, v);
        }
    }

    /// Call the callback of the `float` inlet `ft<id>`.
    pub(crate) fn call_float(&self, t: &T, id: usize, v: f64) {
        if let Some(Some(f)) = id.checked_sub(1).and_then(|i| self.floats.get(i)) {
            f(t, v);
        }
    }

    /// The assist text from the table: the text given, or the default.
    pub(crate) fn assist(&self, iolet: IoLet) -> std::borrow::Cow<'_, str> {
        let (table, index, what) = match iolet {
            IoLet::Inlet(i) => (&self.inlets, i, "inlet"),
            IoLet::Outlet(i) => (&self.outlets, i, "outlet"),
        };
        match table.get(index) {
            Some(info) => info.assist.as_deref().unwrap_or(&info.default).into(),
            None => format!("{} {}", what, index + 1).into(),
        }
    }

    /// Whether the table says the inlet at `index` is hot.
    pub(crate) fn hot(&self, index: usize) -> bool {
        self.inlets.get(index).map_or(index == 0, |i| i.hot)
    }
}

impl<T: 'static> Iolets<T> {
    /// Move a completed table into a forwarding wrapper without changing callback ownership.
    pub(crate) fn map<U: 'static>(self, target: fn(&U) -> &T) -> Iolets<U> {
        Iolets {
            host_inletinfo: self.host_inletinfo,
            ints: self
                .ints
                .map(|f| f.map(|f| Box::new(move |u: &U, v| f(target(u), v)) as IntCB<U>)),
            floats: self
                .floats
                .map(|f| f.map(|f| Box::new(move |u: &U, v| f(target(u), v)) as FloatCB<U>)),
            inlets: self.inlets,
            outlets: self.outlets,
            signal_inlets: self.signal_inlets,
            signal_outlets: self.signal_outlets,
            _proxies: self._proxies,
        }
    }
}

impl<T, K: BuilderKind> Builder<T, K> {
    /// Reserve positions for iolets the host will insert after custom inlets are finalized.
    pub(crate) fn reserve_host_iolets(&mut self, inlets: usize, outlets: usize) {
        self.external_inlets = Some(inlets.max(1));
        self.external_outlets = outlets;
    }
    pub(crate) fn use_prebuilt_iolets(&mut self, iolets: Iolets<T>) {
        self.prebuilt = Some(iolets);
    }
    /// Make the inlets (the non-signal inlets right to left, then, for MSP, `z_dsp_setup` last,
    /// so that the signal inlets end up leftmost) and hand over the table and the callbacks.
    /// Nothing is made if the description has a mistake.
    pub(crate) fn finalize(&mut self) -> Result<Iolets<T>, String> {
        if let Some(iolets) = self.prebuilt.take() {
            return Ok(iolets);
        }
        if let Some(e) = self.error.take() {
            return Err(format!("median: {e}"));
        }
        let entries = std::mem::take(&mut self.inlets);
        let signal_inlets = entries
            .iter()
            .filter(|e| matches!(e.kind, InletKind::Signal))
            .count();
        let obj = self.obj.as_ptr();

        //the table, by position; inlet 0 exists even if it wasn't described
        let count = entries
            .last()
            .map_or(self.external_inlets.unwrap_or(1), |e| e.position + 1);
        let mut inlets: Vec<IoletInfo> = (0..count)
            .map(|i| IoletInfo {
                assist: None,
                default: format!("inlet {}", i + 1).into(),
                hot: i == 0,
            })
            .collect();
        let mut ints: [Option<IntCB<T>>; MAX_TYPED_INLETS] = Default::default();
        let mut floats: [Option<FloatCB<T>>; MAX_TYPED_INLETS] = Default::default();
        let mut proxies = Vec::new();
        //Max makes inlets right to left
        for e in entries.into_iter().rev() {
            let info = &mut inlets[e.position];
            info.assist = e.assist;
            info.hot = e.hot.unwrap_or(info.hot);
            match e.kind {
                InletKind::Default => {}
                InletKind::Signal => info.default = "signal".into(),
                InletKind::Proxy => {
                    // SAFETY: `obj` is valid and under construction (see `new`)
                    proxies.push(unsafe { Proxy::new(obj, e.position) });
                }
                InletKind::Int(f) => {
                    info.default = "int".into();
                    // SAFETY: as above
                    unsafe { max_sys::intin(obj.cast(), e.id as _) };
                    ints[e.id - 1] = Some(f);
                }
                InletKind::Float(f) => {
                    info.default = "float".into();
                    // SAFETY: as above
                    unsafe { max_sys::floatin(obj.cast(), e.id as _) };
                    floats[e.id - 1] = Some(f);
                }
            }
        }
        //`z_dsp_setup` last: every new inlet is inserted just right of the leftmost (`intin` in
        //`ext_proto.h`), so making the others first and the signal inlets last puts the signal
        //inlets at 0..n whether or not Max keeps `z_dsp_setup`'s inlets next to the leftmost
        if K::MSP {
            // SAFETY: `obj` is a valid `t_pxobject` under construction (see `new`)
            unsafe { max_sys::z_dsp_setup(obj.cast(), signal_inlets as _) };
            self.dsp_pending = true;
        }
        let mut outlets: Vec<IoletInfo> = (0..self.external_outlets)
            .map(|i| IoletInfo {
                assist: None,
                default: format!("outlet {}", i + 1).into(),
                hot: false,
            })
            .collect();
        outlets.append(&mut self.outlets);
        Ok(Iolets {
            host_inletinfo: None,
            ints,
            floats,
            inlets,
            outlets,
            signal_inlets,
            signal_outlets: self.signal_outlets,
            _proxies: proxies,
        })
    }

    /// Construction finished: the object's `free` now pairs `z_dsp_setup` with `z_dsp_free`.
    pub(crate) fn commit(&mut self) {
        self.dsp_pending = false;
    }

    /// The callback targets of the clocks and queues made with the builder, for the wrapper.
    pub(crate) fn take_targets(&mut self) -> Vec<Arc<ClockShared>> {
        std::mem::take(&mut self.targets)
    }

    /// Give the object a deferred-call queue whose two qelems call `run` with the object (see
    /// `DeferQueue`). Their targets are armed and disabled with the clocks.
    pub(crate) fn make_defer_queue(&mut self, run: fn(*mut max_sys::t_object)) {
        let qs = [self.queue_for(Box::new(run)), self.queue_for(Box::new(run))];
        self.defer_queue = Some(qs);
    }

    /// The qelems of the deferred-call queue, for the wrapper.
    pub(crate) fn take_defer_queue(&mut self) -> Option<[Queue; 2]> {
        self.defer_queue.take()
    }

    /// The buffer references made with the builder, for the wrapper.
    pub(crate) fn take_buffer_refs(&mut self) -> Vec<ManagedBufferRefInternal> {
        std::mem::take(&mut self.buffer_refs)
    }
}
