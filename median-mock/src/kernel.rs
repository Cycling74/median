//! The safe harness API: [`lock`] and [`Kernel`].

use crate::atoms::Atom;
use crate::attr::{self, AttrError};
use crate::class::{self, Raw, SendError, call_raw, with_inlet};
use crate::state::{ConsoleEntry, DspAdd, InletKind, OutletEvent, SubKind, state};
use crate::thread::{Role, with_role};
use max_sys::t_buffer_obj;
use std::ffi::{c_long, c_void};
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Take the exclusive right to use the kernel and reset it.
///
/// The kernel is one process-wide piece of state (Max has one too, and the C functions median
/// calls have nowhere to find another), but `cargo test` runs tests on parallel threads. Every
/// test that creates objects through the kernel must therefore hold the guard returned here for
/// its whole body; the next test blocks in `lock()` until the guard is dropped. A test that
/// panics does not poison the kernel for the others.
pub fn lock() -> Kernel {
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let kernel = Kernel { _guard: guard };
    kernel.reset();
    kernel
}

/// The exclusive handle to the kernel, from [`lock`]. All the harness operations are methods.
///
/// Objects are identified by their raw pointers, as in Max. The methods that take one are safe
/// because they check it against the kernel's table of live objects (`object_alloc` ... `object_free`)
/// and refuse anything else instead of dereferencing it.
pub struct Kernel {
    _guard: MutexGuard<'static, ()>,
}

impl Kernel {
    /// Forget everything a previous test did: objects, registrations, queued work, the logs, the
    /// clock (back to time 0), buffers. Classes and symbols are kept, because code such as
    /// `ext_main` registers its classes once per process and keeps their pointers.
    pub fn reset(&self) {
        state().reset();
    }

    // ----- classes and objects

    /// Whether a class of that name has been registered (in any namespace).
    pub fn class_exists(&self, name: &str) -> bool {
        class::find_class(name).is_some()
    }

    /// Create an object the way Max does for `[name args...]`: look the class up and call its
    /// `new` method, with `(sym, argc, argv)` for an `A_GIMME` one. Returns null if there is no
    /// such class or the `new` method returned null.
    pub fn new_object(&self, class_name: &str, args: &[Atom]) -> *mut c_void {
        self.try_new_object(class_name, args)
            .unwrap_or(std::ptr::null_mut())
    }

    /// Like [`new_object`](Self::new_object) but says why it failed. A `new` method that returns
    /// null is still `Ok(null)`.
    pub fn try_new_object(
        &self,
        class_name: &str,
        args: &[Atom],
    ) -> Result<*mut c_void, SendError> {
        class::create(class_name, args)
    }

    /// `object_free`. Returns whether it was a live object (or sub-object).
    // `object_free` looks `obj` up in the kernel's tables before it uses it, so an arbitrary
    // pointer is reported on the console and refused rather than dereferenced.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn free_object(&self, obj: *mut c_void) -> bool {
        // SAFETY: `object_free` checks the pointer against the live tables before using it
        unsafe { class::object_free(obj) == 0 }
    }

    /// How many objects made by `object_alloc` are alive (to check for leaks).
    pub fn live_object_count(&self) -> usize {
        state().objects.len()
    }

    /// The class name of a live object.
    pub fn class_name_of(&self, obj: *mut c_void) -> Option<String> {
        let st = state();
        st.objects
            .get(&(obj as usize))
            .map(|o| st.classes[&o.class].name.clone())
    }

    // ----- messages

    /// Send `selector atoms...` to an object as Max would from a patcher, to its leftmost inlet.
    ///
    /// The method registered for the selector is called: an `A_GIMME` method with
    /// `(x, selector, argc, argv)` and a typed one with the converted arguments (an int atom
    /// converts to a float argument and a float atom to an int argument by truncation; a symbol for
    /// a number or a missing non-default argument is [`SendError::BadArgs`]; extra atoms are
    /// ignored). If there is none: `int` and `float` fall back to each other, then to `list`; a
    /// one element `list` falls back to `int` or `float`; and any message falls back to an
    /// `anything` method. A missing method is [`SendError::NoMethod`] and an error entry in the
    /// console log. `A_CANT` methods cannot be sent to ([`SendError::NotSendable`]); see
    /// [`method`](Self::method).
    ///
    /// Methods run on the calling thread with no kernel lock held, so they may call back into the
    /// kernel freely.
    pub fn send(&self, obj: *mut c_void, selector: &str, atoms: &[Atom]) -> Result<(), SendError> {
        class::dispatch(obj as usize, selector, atoms).map(|_| ())
    }

    /// `send(obj, "bang", [])`.
    pub fn send_bang(&self, obj: *mut c_void) -> Result<(), SendError> {
        self.send(obj, "bang", &[])
    }

    /// `send(obj, "int", [n])`.
    pub fn send_int(&self, obj: *mut c_void, n: i64) -> Result<(), SendError> {
        self.send(obj, "int", &[Atom::Long(n)])
    }

    /// `send(obj, "float", [f])`.
    pub fn send_float(&self, obj: *mut c_void, f: f64) -> Result<(), SendError> {
        self.send(obj, "float", &[Atom::Float(f)])
    }

    /// `send(obj, "list", atoms)`.
    pub fn send_list(&self, obj: *mut c_void, atoms: &[Atom]) -> Result<(), SendError> {
        self.send(obj, "list", atoms)
    }

    /// Send to an inlet, counted from the left (0 is the leftmost, which is what [`send`](Self::send)
    /// does). What happens depends on how the inlet was made:
    /// * `proxy_new`: the message goes to the usual method, `proxy_getinlet` returns the proxy's id
    ///   while it runs, and the `stuffloc` the proxy was given holds the id during the call and 0
    ///   after;
    /// * `intin(x, n)`/`floatin(x, n)`: the atoms go to the method `in<n>`/`ft<n>`;
    /// * `inlet_new(x, "sel")`: the atoms go to the method `sel`; with a null `sel` as usual;
    /// * a signal inlet made by `z_dsp_setup`: as a proxy whose id is the inlet number.
    ///
    /// Inlets are created right to left, so the last inlet created is inlet 1.
    pub fn send_inlet(
        &self,
        obj: *mut c_void,
        inlet: usize,
        selector: &str,
        atoms: &[Atom],
    ) -> Result<(), SendError> {
        let addr = obj as usize;
        if inlet == 0 {
            return self.send(obj, selector, atoms);
        }
        let kind = {
            let st = state();
            let Some(o) = st.objects.get(&addr) else {
                return Err(SendError::NotLive);
            };
            let total = o.inlets.len();
            if inlet > total {
                return Err(SendError::NoInlet(inlet));
            }
            match st.subs.get(&o.inlets[total - inlet]) {
                Some(s) if s.alive => match &s.kind {
                    SubKind::Inlet { kind, .. } => kind.clone(),
                    _ => return Err(SendError::NoInlet(inlet)),
                },
                _ => return Err(SendError::NoInlet(inlet)),
            }
        };
        let res = match kind {
            InletKind::Proxy { id, stuffloc } => {
                if stuffloc != 0 {
                    // SAFETY: `proxy_new`'s contract: `stuffloc` is writable while the object lives
                    unsafe { (stuffloc as *mut c_long).write(id as c_long) };
                }
                let res = with_inlet(id, || class::dispatch(addr, selector, atoms));
                if stuffloc != 0 {
                    // SAFETY: as above
                    unsafe { (stuffloc as *mut c_long).write(0) };
                }
                res
            }
            InletKind::Int(n) => class::dispatch(addr, &format!("in{n}"), atoms),
            InletKind::Float(n) => class::dispatch(addr, &format!("ft{n}"), atoms),
            InletKind::Named(Some(sel)) => class::dispatch(addr, &sel, atoms),
            InletKind::Named(None) => class::dispatch(addr, selector, atoms),
            InletKind::Signal => {
                with_inlet(inlet as i64, || class::dispatch(addr, selector, atoms))
            }
        };
        res.map(|_| ())
    }

    /// The method registered for `selector` on the object's class as `(function pointer, argument
    /// types)`, e.g. to call an `A_CANT` method such as `assist` or `notify` directly (transmute
    /// the pointer to the real signature). The types are the `A_*` values without the final 0.
    pub fn method(&self, obj: *mut c_void, selector: &str) -> Option<(usize, Vec<i32>)> {
        let st = state();
        let o = st.objects.get(&(obj as usize))?;
        let m = st.classes[&o.class].methods.get(selector)?;
        Some((m.fun, m.types.clone()))
    }

    // ----- inlets and outlets

    /// How many outlets the object has.
    pub fn outlet_count(&self, obj: *mut c_void) -> usize {
        state()
            .objects
            .get(&(obj as usize))
            .map_or(0, |o| o.outlets.len())
    }

    /// The type string of each outlet of the object, left to right (`None`: sends anything).
    pub fn outlet_types(&self, obj: *mut c_void) -> Vec<Option<String>> {
        let st = state();
        let Some(o) = st.objects.get(&(obj as usize)) else {
            return Vec::new();
        };
        o.outlets
            .iter()
            .rev()
            .filter_map(|p| match &st.subs.get(p)?.kind {
                SubKind::Outlet { ty, .. } => Some(ty.clone()),
                _ => None,
            })
            .collect()
    }

    /// How many inlets were made (with `proxy_new`, `intin`, `floatin`, `inlet_new` and
    /// `z_dsp_setup`), not counting the leftmost.
    pub fn extra_inlet_count(&self, obj: *mut c_void) -> usize {
        state()
            .objects
            .get(&(obj as usize))
            .map_or(0, |o| o.inlets.len())
    }

    /// The `n` of `z_dsp_setup(x, n)`, if it was called.
    pub fn signal_inlets(&self, obj: *mut c_void) -> Option<usize> {
        state()
            .objects
            .get(&(obj as usize))
            .and_then(|o| o.signal_inlets)
    }

    /// Everything sent through outlets since the last reset or [`take_outlet_log`](Self::take_outlet_log),
    /// oldest first.
    pub fn outlet_log(&self) -> Vec<OutletEvent> {
        state().outlet_log.clone()
    }

    /// Like [`outlet_log`](Self::outlet_log) and empties the log.
    pub fn take_outlet_log(&self) -> Vec<OutletEvent> {
        std::mem::take(&mut state().outlet_log)
    }

    /// Everything posted with `post`, `error`, `object_post`, `object_warn`, `object_error` (and
    /// the kernel's own diagnostics: unknown message, bad arguments, bad frees) since the last reset
    /// or [`take_console_log`](Self::take_console_log).
    pub fn console_log(&self) -> Vec<ConsoleEntry> {
        state().console.clone()
    }

    /// Like [`console_log`](Self::console_log) and empties the log.
    pub fn take_console_log(&self) -> Vec<ConsoleEntry> {
        std::mem::take(&mut state().console)
    }

    // ----- time and threads

    /// The logical time in ms (what `clock_getftime` returns).
    pub fn time(&self) -> f64 {
        state().now
    }

    /// Move logical time forward `ms` and fire the clocks that fall due, in time order (ties in
    /// the order they were scheduled), each on the calling thread with the timer thread role, with
    /// the logical time set to the clock's due time while it runs. Clocks scheduled by a callback
    /// that fall due before the end are fired too. Returns how many fired.
    pub fn advance(&self, ms: f64) -> usize {
        crate::clock::advance(ms)
    }

    /// Run work queued by `defer_low` (and `defer` from the timer thread) and `qelem_set`, first in
    /// first out, with the main thread role, including work queued while running. Returns how many
    /// calls ran.
    pub fn run_main_queue(&self) -> usize {
        crate::queue::run_main_queue()
    }

    /// How many calls are waiting in the main queue.
    pub fn main_queue_len(&self) -> usize {
        state().queue.len()
    }

    /// Run `f` with the calling thread's role (what `systhread_ismainthread` and friends report)
    /// set to `role`. Threads are [`Role::Main`] by default.
    pub fn with_role<R>(&self, role: Role, f: impl FnOnce() -> R) -> R {
        with_role(role, f)
    }

    /// What `sys_getsr` returns (default 44100).
    pub fn set_sample_rate(&self, sr: f64) {
        state().sample_rate = sr;
    }

    // ----- dsp

    /// Call the object's `dsp64` method as MSP does, and return the perform routines it added
    /// with `dsp_add64` in the order it added them. `count` is all ones (every signal connected).
    pub fn dsp64(
        &self,
        obj: *mut c_void,
        sample_rate: f64,
        max_vector_size: usize,
    ) -> Result<Vec<DspAdd>, SendError> {
        let addr = obj as usize;
        let (fun, ins, outs) = {
            let st = state();
            let Some(o) = st.objects.get(&addr) else {
                return Err(SendError::NotLive);
            };
            let Some(m) = st.classes[&o.class].methods.get("dsp64") else {
                return Err(SendError::NoMethod("dsp64".into()));
            };
            let outs = o
                .outlets
                .iter()
                .filter(|p| {
                    matches!(
                        st.subs.get(p).map(|s| &s.kind),
                        Some(SubKind::Outlet { ty: Some(t), .. }) if t == "signal"
                    )
                })
                .count();
            (m.fun, o.signal_inlets.unwrap_or(0), outs)
        };
        state().dsp_adds.clear();
        let dsp64 = Box::into_raw(Box::new([0u64; 8])) as usize;
        let mut count = vec![1i16; ins + outs];
        // SAFETY: `dsp64` is an `A_CANT` method `(x, dsp64, short *count, double sr, long vs, long flags)`
        unsafe {
            call_raw(
                fun,
                &[
                    Raw::I(addr),
                    Raw::I(dsp64),
                    Raw::I(count.as_mut_ptr() as usize),
                    Raw::F(sample_rate),
                    Raw::I(max_vector_size),
                    Raw::I(0),
                ],
            );
        }
        Ok(std::mem::take(&mut state().dsp_adds))
    }

    // ----- attributes

    /// The names of the attributes of the object's class.
    pub fn attr_names(&self, obj: *mut c_void) -> Vec<String> {
        attr::names(obj as usize)
    }

    /// Get an attribute as `object_attr_getvalueof` would: through its getter, or, for an
    /// attribute made with `attr_offset_new` without one, by reading the object's memory. The
    /// array the getter allocates is freed with `sysmem_freeptr`, as the caller must in Max.
    pub fn attr_get(&self, obj: *mut c_void, name: &str) -> Result<Vec<Atom>, AttrError> {
        attr::get(obj as usize, name)
    }

    /// Set an attribute through its setter, or by writing the object's memory (see
    /// [`attr_get`](Self::attr_get)); the clip filters added with `attr_addfilter*_clip` apply.
    pub fn attr_set(&self, obj: *mut c_void, name: &str, atoms: &[Atom]) -> Result<(), AttrError> {
        attr::set(obj as usize, name, atoms)
    }

    /// The flags an attribute was created with (`ATTR_GET_OPAQUE_USER` and so on).
    pub fn attr_flags(&self, obj: *mut c_void, name: &str) -> Option<i64> {
        attr::flags(obj as usize, name)
    }

    /// `(object, attribute)` of every `object_attr_touch` since the last reset.
    pub fn attr_touches(&self) -> Vec<(usize, String)> {
        state().touches.clone()
    }

    // ----- buffers

    /// Make a buffer~ with zeroed interleaved samples and bind it to its subscribers (`buffer_ref`s
    /// and `object_subscribe` clients get `globalsymbol_binding`).
    pub fn create_buffer(
        &self,
        name: &str,
        channels: usize,
        frames: usize,
        sample_rate: f64,
    ) -> *mut t_buffer_obj {
        crate::buffer::create(name, channels, frames, sample_rate) as *mut t_buffer_obj
    }

    /// Remove a buffer~ (its clients get `globalsymbol_unbinding`).
    pub fn destroy_buffer(&self, name: &str) {
        crate::buffer::destroy(name);
    }

    /// The interleaved samples of a buffer~.
    pub fn buffer_samples(&self, name: &str) -> Option<Vec<f32>> {
        state().buffers.get(name).map(|b| b.data.clone())
    }

    /// Replace the interleaved samples of a buffer~; the length must be unchanged.
    ///
    /// # Panics
    /// If the buffer does not exist or `samples` has a different length.
    pub fn set_buffer_samples(&self, name: &str, samples: &[f32]) {
        let mut st = state();
        let b = st.buffers.get_mut(name).expect("no such buffer");
        assert_eq!(b.data.len(), samples.len(), "buffer size cannot change");
        b.data.copy_from_slice(samples);
    }

    /// `(locks outstanding, dirty)` of a buffer~.
    pub fn buffer_state(&self, name: &str) -> Option<(usize, bool)> {
        state().buffers.get(name).map(|b| (b.locks, b.dirty))
    }

    /// The `buffer_ref_notify` calls so far as `(buffer ref, sender name, message)`.
    pub fn buffer_ref_notifications(&self) -> Vec<(usize, String, String)> {
        state().buffer_ref_notifications.clone()
    }
}
