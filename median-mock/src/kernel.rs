//! The safe harness API: [`lock`] and [`Kernel`].

use crate::atoms::Atom;
use crate::attr::{self, AttrError};
use crate::class::{self, Raw, SendError, call_raw, with_inlet};
use crate::state::{ConsoleEntry, DspAdd, InletKind, OutletEvent, SubKind, state};
use crate::thread::{Role, with_role};
use max_sys::t_buffer_obj;
use std::cell::Cell;
use std::ffi::{c_long, c_void};
use std::sync::{Mutex, MutexGuard};

static TEST_LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    /// Whether this thread holds the kernel (a live [`Kernel`]), for the `*_nested` functions.
    static HELD: Cell<bool> = const { Cell::new(false) };
}

/// Panic unless the calling thread holds the kernel.
fn assert_held(what: &str) {
    assert!(
        HELD.with(Cell::get),
        "median-mock: {what} called on a thread that does not hold the kernel (from `lock()`)"
    );
}

/// Take the exclusive right to use the kernel and reset it.
///
/// The kernel is one process-wide piece of state (Max has one too, and the C functions median
/// calls have nowhere to find another), but `cargo test` runs tests on parallel threads. Every
/// test that creates objects through the kernel must therefore hold the guard returned here for
/// its whole body; the next test blocks in `lock()` until the guard is dropped. A test that
/// panics does not poison the kernel for the others.
pub fn lock() -> Kernel {
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    HELD.with(|h| h.set(true));
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

impl Drop for Kernel {
    fn drop(&mut self) {
        //the guard is not `Send`, so this is the thread `lock` set it on
        HELD.with(|h| h.set(false));
    }
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

    /// How many clocks made by `clock_new` have not been freed (to check for leaks).
    pub fn live_clock_count(&self) -> usize {
        state()
            .subs
            .values()
            .filter(|s| s.alive && matches!(s.kind, SubKind::Clock { .. }))
            .count()
    }

    /// How many qelems made by `qelem_new` have not been freed (to check for leaks).
    pub fn live_qelem_count(&self) -> usize {
        state()
            .subs
            .values()
            .filter(|s| s.alive && matches!(s.kind, SubKind::Qelem { .. }))
            .count()
    }

    /// How many buffer references made by `buffer_ref_new` have not been freed (to check for
    /// leaks).
    pub fn live_buffer_ref_count(&self) -> usize {
        state()
            .subs
            .values()
            .filter(|s| s.alive && matches!(s.kind, SubKind::BufferRef { .. }))
            .count()
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
    /// ignored). If there is none: `int` falls back to `float` then `list` (not checked in Max);
    /// `float` has no fallback, as in Max (not `int`, `list` or `anything`); a one element `list`
    /// falls back to `int` or `float` (a float element only to `float`); and any other message
    /// falls back to an
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
    /// Inlets are created right to left, so the last inlet created is inlet 1, `z_dsp_setup`'s
    /// signal inlets included (see [`inlets`](Self::inlets)).
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
            let inlets = crate::io::inlets_left_to_right(&st, o);
            let Some(ptr) = inlets.get(inlet - 1) else {
                return Err(SendError::NoInlet(inlet));
            };
            match st.subs.get(ptr) {
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

    /// The selectors of the methods of the object's class, sorted.
    pub fn method_names(&self, obj: *mut c_void) -> Vec<String> {
        let st = state();
        let Some(o) = st.objects.get(&(obj as usize)) else {
            return Vec::new();
        };
        let mut v: Vec<String> = st.classes[&o.class].methods.keys().cloned().collect();
        v.sort();
        v
    }

    /// An attribute of the method `selector` of the object's class, added with
    /// `object_addattr_parse` (`CLASS_METHOD_ATTR_PARSE`), as `(type name, parse string)`.
    pub fn method_attr(
        &self,
        obj: *mut c_void,
        selector: &str,
        attr: &str,
    ) -> Option<(String, String)> {
        let st = state();
        let o = st.objects.get(&(obj as usize))?;
        st.classes[&o.class]
            .methods
            .get(selector)?
            .attrs
            .get(attr)
            .cloned()
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

    /// How each inlet was made, from left to right, not counting the leftmost (which is the
    /// object itself). Inlets are created right to left, so the last one created comes first;
    /// the signal inlets of `z_dsp_setup` are no exception. Freed inlets are included.
    pub fn inlets(&self, obj: *mut c_void) -> Vec<InletKind> {
        let st = state();
        let Some(o) = st.objects.get(&(obj as usize)) else {
            return Vec::new();
        };
        crate::io::inlets_left_to_right(&st, o)
            .iter()
            .filter_map(|p| match &st.subs.get(p)?.kind {
                SubKind::Inlet { kind } => Some(kind.clone()),
                _ => None,
            })
            .collect()
    }

    /// How each inlet was made, in the order they were made.
    pub fn inlets_created(&self, obj: *mut c_void) -> Vec<InletKind> {
        let st = state();
        let Some(o) = st.objects.get(&(obj as usize)) else {
            return Vec::new();
        };
        o.inlets
            .iter()
            .filter_map(|p| match &st.subs.get(p)?.kind {
                SubKind::Inlet { kind } => Some(kind.clone()),
                _ => None,
            })
            .collect()
    }

    /// Call the object's `assist` method (`A_CANT`, `(x, b, long io, long index, char *s)`) for
    /// inlet (`outlet == false`) or outlet `index`, with a 512 byte buffer (the smallest Max
    /// passes) that starts out empty, and return what it wrote up to the terminating NUL (invalid
    /// UTF-8 replaced). `None` if the object has no `assist` method. A string that fills the
    /// whole buffer without a terminator is reported on the console and cut at 512 bytes.
    pub fn assist(&self, obj: *mut c_void, outlet: bool, index: usize) -> Option<String> {
        let (fun, _) = self.method(obj, "assist")?;
        let mut buf = vec![0u8; 512];
        let io: c_long = if outlet { 2 } else { 1 };
        // SAFETY: `assist` methods are `(x, void *b, long io, long index, char *s)`, and `s` is
        // 512 writable bytes
        unsafe {
            call_raw(
                fun,
                &[
                    Raw::I(obj as usize),
                    Raw::I(0),
                    Raw::I(io as usize),
                    Raw::I(index),
                    Raw::I(buf.as_mut_ptr() as usize),
                ],
            );
        }
        let end = match buf.iter().position(|&b| b == 0) {
            Some(n) => n,
            None => {
                state().post_error(obj as usize, "median-mock: assist wrote no terminator in 512 bytes".into());
                buf.len()
            }
        };
        Some(String::from_utf8_lossy(&buf[..end]).into_owned())
    }

    /// Call the object's `inletinfo` method (`A_CANT`, `(x, b, long index, char *t)`) for inlet
    /// `index`, with `*t` starting at 0, and say whether the inlet is hot (`*t` still 0; the SDK
    /// examples set it to 1 for a cold inlet). `None` if the object has no `inletinfo` method.
    pub fn inlet_is_hot(&self, obj: *mut c_void, index: usize) -> Option<bool> {
        let (fun, _) = self.method(obj, "inletinfo")?;
        let mut t: std::ffi::c_char = 0;
        // SAFETY: `inletinfo` methods are `(x, void *b, long index, char *t)`
        unsafe {
            call_raw(
                fun,
                &[
                    Raw::I(obj as usize),
                    Raw::I(0),
                    Raw::I(index),
                    Raw::I(&raw mut t as usize),
                ],
            );
        }
        Some(t == 0)
    }

    /// Make every outlet call fail as on a stack overflow (return null, send nothing, log
    /// nothing) until it is turned off again or the kernel is reset.
    pub fn set_outlet_overflow(&self, overflow: bool) {
        state().outlet_overflow = overflow;
    }

    /// Call the object's `dblclick` method (`A_CANT`, `(x)`) as Max does when the box is
    /// double-clicked in a locked patcher. `false` if the object has no `dblclick` method.
    pub fn dblclick(&self, obj: *mut c_void) -> bool {
        let Some((fun, _)) = self.method(obj, "dblclick") else {
            return false;
        };
        // SAFETY: `dblclick` methods are `(x)`
        unsafe { call_raw(fun, &[Raw::I(obj as usize)]) };
        true
    }

    /// Call the object's `loadbang` method (`A_CANT`, `(x)`) as Max does when the patcher it is
    /// in has finished loading. `false` if the object has no `loadbang` method.
    pub fn loadbang(&self, obj: *mut c_void) -> bool {
        let Some((fun, _)) = self.method(obj, "loadbang") else {
            return false;
        };
        // SAFETY: `loadbang` methods are `(x)`
        unsafe { call_raw(fun, &[Raw::I(obj as usize)]) };
        true
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
    /// first out, with the main thread role, including work queued while running. A qelem is
    /// unset after its function returns (`ext_proto.h`), so a `qelem_set` of the qelem whose
    /// function is running does nothing. Returns how many calls ran.
    pub fn run_main_queue(&self) -> usize {
        crate::queue::run_main_queue()
    }

    /// Run the main queue from inside a callback, the way a nested event loop does in Max: a
    /// modal dialog opened by a method, or by a qelem or clock callback, services the main
    /// thread's queue until it closes, so other qelems and deferred calls run nested inside that
    /// callback. A running qelem is not among them: it stays set, and so can't be queued again,
    /// until its function returns. As
    /// [`run_main_queue`](Self::run_main_queue), but an associated function, for code that runs
    /// under the [`Kernel`] (a method, a clock or qelem callback) and has no handle to it.
    ///
    /// # Panics
    /// If the calling thread does not hold the kernel (a [`Kernel`] from [`lock`] that is alive).
    pub fn run_main_queue_nested() -> usize {
        assert_held("Kernel::run_main_queue_nested");
        crate::queue::run_main_queue()
    }

    /// Like [`run_main_queue_nested`](Self::run_main_queue_nested), for clocks: move logical time
    /// forward from inside a callback, firing what falls due, nested inside it (as Max's
    /// scheduler does when it is serviced by the main thread's event loop, with Overdrive off,
    /// and a modal dialog runs that loop). As [`advance`](Self::advance) otherwise.
    ///
    /// # Panics
    /// If the calling thread does not hold the kernel.
    pub fn advance_nested(ms: f64) -> usize {
        assert_held("Kernel::advance_nested");
        crate::clock::advance(ms)
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

    /// The objects `z_dsp_free` was called on since the last reset, in order (also calls the
    /// kernel refused).
    pub fn dsp_frees(&self) -> Vec<usize> {
        state().dsp_frees.clone()
    }

    /// The perform routines added with `dsp_add64` since the last reset, without removing them.
    /// [`dsp64`](Self::dsp64) takes the ones its call added (and clears the list first), so this is
    /// for code that gets a `dsp64` method called some other way, or that checks that none was
    /// added.
    pub fn dsp_adds(&self) -> Vec<DspAdd> {
        state().dsp_adds.clone()
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

    /// Call an attribute's getter as [`attr_get`](Self::attr_get) does, but with the pointers
    /// chosen by `args`: null ones (which Max may pass; a getter must not dereference them) or a
    /// caller supplied array. No clip filter is applied.
    pub fn attr_get_with(
        &self,
        obj: *mut c_void,
        name: &str,
        args: attr::GetterArgs,
    ) -> Result<attr::GetterResult, AttrError> {
        attr::get_with(obj as usize, name, args)
    }

    /// Call an attribute's setter with `ac` atoms and a null `av`, returning its code.
    pub fn attr_set_null_argv(&self, obj: *mut c_void, name: &str, ac: i64) -> Result<i64, AttrError> {
        attr::set_null_argv(obj as usize, name, ac)
    }

    /// The flags an attribute was created with (`ATTR_GET_OPAQUE_USER` and so on).
    pub fn attr_flags(&self, obj: *mut c_void, name: &str) -> Option<i64> {
        attr::flags(obj as usize, name)
    }

    /// An attribute of the attribute `attr` of the object's class (`label`, `save`, `enumvals`
    /// ...: what the `CLASS_ATTR_*` macros add with `class_attr_addattr_parse` and friends), as
    /// `(type name, atoms)`. Parse strings are split as [`attr::parse_atoms`] does.
    pub fn attr_attr(&self, obj: *mut c_void, attr: &str, name: &str) -> Option<(String, Vec<Atom>)> {
        attr::attr_attrs(obj as usize, attr)?.remove(name)
    }

    /// The names of the attributes of the attribute `attr` of the object's class, sorted.
    pub fn attr_attr_names(&self, obj: *mut c_void, attr: &str) -> Vec<String> {
        attr::attr_attrs(obj as usize, attr).map_or_else(Vec::new, |m| m.into_keys().collect())
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

    /// Whether a buffer~ is between `buffer_edit_begin` and `buffer_edit_end`.
    pub fn buffer_editing(&self, name: &str) -> Option<bool> {
        state().buffers.get(name).map(|b| b.editing > 0)
    }

    /// The `valid` argument of the last accepted `buffer_edit_end` on a buffer~ (`Some(None)`
    /// before any).
    pub fn buffer_last_edit_valid(&self, name: &str) -> Option<Option<i64>> {
        state().buffers.get(name).map(|b| b.last_edit_valid)
    }

    /// `(begun, ended)`: how many `buffer_edit_begin` and `buffer_edit_end` calls on a buffer~
    /// were accepted.
    pub fn buffer_edits(&self, name: &str) -> Option<(usize, usize)> {
        state().buffers.get(name).map(|b| (b.edits_begun, b.edits_ended))
    }

    /// Clear a buffer~'s dirty flag (set by `buffer_setdirty`).
    pub fn clear_buffer_dirty(&self, name: &str) {
        if let Some(b) = state().buffers.get_mut(name) {
            b.dirty = false;
        }
    }

    /// The names of the buffers `buffer_view` opened since the last reset, in order.
    pub fn buffer_views(&self) -> Vec<String> {
        state().buffer_views.clone()
    }

    /// The `buffer_ref_notify` calls so far as `(buffer ref, sender name, message)`.
    pub fn buffer_ref_notifications(&self) -> Vec<(usize, String, String)> {
        state().buffer_ref_notifications.clone()
    }
}
