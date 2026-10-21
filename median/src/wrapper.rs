//! External Max and MSP object wrappers
//! TODO: Jitter

use crate::{
    atom::Atom,
    buffer::BufferRef,
    builder::{MSPWrappedBuilder, ManagedBufferRefInternal, MaxWrappedBuilder, WrappedBuilder},
    class::{Class, ClassType},
    clock::ClockShared,
    inlet::{FloatCB, IntCB},
    method::{MaxFree, MaxMethod},
    notify::Notification,
    object::{MSPObj, MaxObj, ObjBox},
    symbol::SymbolRef,
};

use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr, CString},
    marker::PhantomData,
    mem::MaybeUninit,
    os::raw::c_long,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, PoisonError,
    },
};

use lazy_static::lazy_static;

lazy_static! {
    //type name -> ClassMaxObjWrapper
    static ref CLASSES: Mutex<HashMap<&'static str, ClassMaxObjWrapper>> = Mutex::new(HashMap::new());
}

const ASSIST_MAX: i64 = 512;

pub type MaxObjWrapper<T> = Wrapper<max_sys::t_object, MaxWrapperInternal<T>, T>;
pub type MSPObjWrapper<T> = Wrapper<max_sys::t_pxobject, MSPWrapperInternal<T>, T>;

pub type FloatCBHash<T> = HashMap<usize, FloatCB<T>>;
pub type IntCBHash<T> = HashMap<usize, IntCB<T>>;

pub type DeferMethodWrapped<T> = unsafe extern "C" fn(
    wrapper: &T,
    sym: *mut max_sys::t_symbol,
    argc: std::os::raw::c_short,
    argv: *const max_sys::t_atom,
);

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum AssistIOlet {
    Inlet(usize),
    Outlet(usize),
}

//reexports
///trampoline for attribute getters
pub use median_macros::wrapped_attr_get_tramp as attr_get_tramp;
///trampoline for attribute setters
pub use median_macros::wrapped_attr_set_tramp as attr_set_tramp;
///trampoline for deffered calls
pub use median_macros::wrapped_defer_tramp as defer_tramp;
///trampoline for list methods (ditch selector)
pub use median_macros::wrapped_list_tramp as list_tramp;
///trampoline for seletor list methods
pub use median_macros::wrapped_sel_list_tramp as sel_list_tramp;
///general use trampoline
pub use median_macros::wrapped_tramp as tramp;

//we only use ClassMaxObjWrapper in CLASSES after we've registered the class, for max's usage this is
//Send
#[repr(transparent)]
struct ClassMaxObjWrapper(*mut max_sys::t_class);
unsafe impl Send for ClassMaxObjWrapper {}

/// A trait used by both Max and MSP objects.
///
/// # Remarks
/// If you're using the macro system to wrap your external and unless you need to override the
/// `ClassType` or `handle_notification`, you might not actually implement this explicitly.
pub trait ObjWrapped<T>: Sized + Sync + 'static {
    /// The name of your class, this is what you'll type into a box in Max if your class is a
    /// `ClassType::Box`.
    ///
    /// You can add additional aliases in the `class_setup` method.
    fn class_name() -> &'static str;

    /// The type of your class. Defaults to 'box' which creates visual objects in Max.
    fn class_type() -> ClassType {
        ClassType::Box
    }

    /// Handle notifications that your object gets
    fn handle_notification(&self, _notification: &Notification) {}

    /// Provide assistance in the case that you don't provide it along with your i/o creation
    fn assist<F: FnOnce(&CStr)>(&self, iolet: AssistIOlet, render: F) {
        let v = CString::new(match iolet {
            AssistIOlet::Inlet(i) => format!("inlet {}", i + 1),
            AssistIOlet::Outlet(i) => format!("outlet {}", i + 1),
        })
        .unwrap();
        render(v.as_c_str());
    }
}

/// The trait to implement for your object to be wrapped as a Max object.
pub trait MaxObjWrapped<T>: ObjWrapped<T> {
    /// A constructor for your object.
    ///
    /// # Arguments
    ///
    /// * `builder` - A builder for constructing inlets/oulets/etc.
    fn new(builder: &mut dyn MaxWrappedBuilder<T>) -> Self;

    /// Register any methods you need for your class.
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        //default, do nothing
    }
}

/// The trait to implement for your object to be wrapped as a MSP object.
pub trait MSPObjWrapped<T>: ObjWrapped<T> {
    /// A constructor for your object.
    ///
    /// # Arguments
    ///
    /// * `builder` - A builder for constructing inlets/oulets/etc.
    fn new(builder: &mut dyn MSPWrappedBuilder<T>) -> Self;

    /// Perform DSP.
    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], nframes: usize);

    /// Register any methods you need for your class.
    fn class_setup(_class: &mut Class<MSPObjWrapper<Self>>) {
        //default, do nothing
    }

    /// Optionally do any setup you need just before perform is called
    ///
    /// # Arguments
    /// * `sample_rate`: the audio sampling rate for the object in the DSP chain.
    ///
    /// # Remarks
    /// * This will be called every time DSP is toggled on.
    /// * Your best guess of the sample rate before DSP is toggled on is the value from [`max_sys::sys_getsr()`]
    #[allow(unused)]
    fn dsp_setup(&self, sample_rate: f64) {}
}

pub trait WrapperWrapped<T> {
    /// Retrieve a reference to your wrapped class.
    fn wrapped(&self) -> &T;
}

/// Defer methods for wrapped objects.
pub trait WrappedDefer<T> {
    ///defer a tramp method with the sym and atoms args
    ///
    /// # Safety
    /// The object must stay alive until the deferred call has run: Max does not keep it alive,
    /// and the deferred call receives a pointer to it. For example, the object must not be freed
    /// (the box deleted) before then.
    unsafe fn defer(&self, method: DeferMethodWrapped<T>, sym: SymbolRef, atoms: &[Atom]);
    ///defer_low a tramp method with the sym and atoms args
    ///
    /// # Safety
    /// The object must stay alive until the deferred call has run: Max does not keep it alive,
    /// and the deferred call receives a pointer to it. For example, the object must not be freed
    /// (the box deleted) before then.
    unsafe fn defer_low(&self, method: DeferMethodWrapped<T>, sym: SymbolRef, atoms: &[Atom]);
}

/// The actual struct that is given to max
#[repr(C)]
pub struct Wrapper<O, I, T> {
    s_obj: O,
    wrapped: MaybeUninit<I>,
    /// `wrapped` is initialised. Max can free the object before construction finished (a panic
    /// in `T::new`), in which case `free` must not drop `wrapped`. Only written while the
    /// object is being constructed or freed, when nothing else can reach it.
    constructed: bool,
    _phantom: PhantomData<T>,
}

/// Inner struct for wrapping [`MaxObjWrapped`]
pub struct MaxWrapperInternal<T> {
    wrapped: T,
    callbacks_float: FloatCBHash<T>,
    callbacks_int: IntCBHash<T>,
    buffer_refs: Vec<ManagedBufferRefInternal>,
    //disabled by `free_wrapped` before `wrapped` is dropped (and again in `Drop`)
    clocks: Vec<Arc<ClockShared>>,
    assist_ins: HashMap<usize, CString>,
    assist_outs: HashMap<usize, CString>,
    //we just hold onto these so they don't get deallocated until later
    _proxy_inlets: Vec<crate::inlet::Proxy>,
}

/// Inner struct for wrapping [`MSPObjWrapped`]
pub struct MSPWrapperInternal<T> {
    wrapped: T,
    signal_inlets: usize,
    signal_outlets: usize,
    /// `perform` panicked: the object stays silent (and `perform` is not called again, so the
    /// audio thread doesn't post an error every block) until the next `dsp64`.
    panicked: AtomicBool,
    callbacks_float: FloatCBHash<T>,
    callbacks_int: IntCBHash<T>,
    buffer_refs: Vec<ManagedBufferRefInternal>,
    //disabled by `free_wrapped` before `wrapped` is dropped (and again in `Drop`)
    clocks: Vec<Arc<ClockShared>>,
    assist_ins: HashMap<usize, CString>,
    assist_outs: HashMap<usize, CString>,
    //we just hold onto these so they don't get deallocated until later
    _proxy_inlets: Vec<crate::inlet::Proxy>,
}

/// Disable the clocks created for an object. Must run before the object's `T` is dropped (and
/// before anything else about the object is torn down or moved): it waits for any in-flight
/// clock callback and makes later ones no-ops. Idempotent.
fn release_clocks(clocks: &[Arc<ClockShared>]) {
    for c in clocks {
        c.clear();
    }
}

impl<T> Drop for MaxWrapperInternal<T> {
    fn drop(&mut self) {
        //`free_wrapped` has already done this, before it started to tear the object down; this
        //is a harmless second clear that keeps the invariant local to the type
        release_clocks(&self.clocks);
    }
}

impl<T> Drop for MSPWrapperInternal<T> {
    fn drop(&mut self) {
        //`free_wrapped` has already done this, before it started to tear the object down; this
        //is a harmless second clear that keeps the invariant local to the type
        release_clocks(&self.clocks);
    }
}

/// Sealed: `Wrapper` needs `WrapperInternal` as a bound, so the trait has to be `pub`, but it
/// lives in this private module and is not re-exported, so other crates can neither name nor
/// implement it.
mod sealed {
    use super::*;

    pub trait WrapperInternal<O, T>: Sized {
        fn wrapped(&self) -> &T;
        /// The clocks created for this object by the builder.
        fn clocks(&self) -> &[Arc<ClockShared>];
        /// # Safety
        /// `owner` must point to a valid, newly allocated object of type `O` that is being
        /// constructed and that will outlive the returned value.
        unsafe fn new(owner: *mut O, sym: SymbolRef, args: &[Atom]) -> Self;
        fn class_setup(class: &mut Class<Wrapper<O, Self, T>>);

        fn call_float(&self, index: usize, value: f64);
        fn call_int(&self, index: usize, value: max_sys::t_atom_long);

        fn handle_notification(&self, notification: &Notification);
        /// # Safety
        /// `s` must point to a writable buffer of at least `ASSIST_MAX` bytes, as Max passes to an
        /// assist method.
        unsafe fn assist(&self, io: c_long, index: c_long, s: *mut c_char);
    }
}
use sealed::WrapperInternal;

unsafe impl<I, T> MaxObj for Wrapper<max_sys::t_object, I, T> {}
unsafe impl<I, T> MaxObj for Wrapper<max_sys::t_pxobject, I, T> {}
unsafe impl<I, T> MSPObj for Wrapper<max_sys::t_pxobject, I, T> {}

impl<T> WrapperInternal<max_sys::t_object, T> for MaxWrapperInternal<T>
where
    T: MaxObjWrapped<T> + Sync + 'static,
{
    fn wrapped(&self) -> &T {
        &self.wrapped
    }
    fn clocks(&self) -> &[Arc<ClockShared>] {
        &self.clocks
    }
    unsafe fn new(owner: *mut max_sys::t_object, sym: SymbolRef, args: &[Atom]) -> Self {
        // SAFETY: the caller guarantees `owner` is valid and under construction
        let mut builder = unsafe { WrappedBuilder::new_max(owner, sym, args) };
        let wrapped = T::new(&mut builder);
        let mut f = builder.finalize();
        Self {
            wrapped,
            callbacks_float: std::mem::take(&mut f.callbacks_float),
            callbacks_int: std::mem::take(&mut f.callbacks_int),
            buffer_refs: std::mem::take(&mut f.buffer_refs),
            clocks: std::mem::take(&mut f.clocks),
            _proxy_inlets: std::mem::take(&mut f.proxy_inlets),
            assist_ins: std::mem::take(&mut f.assist_ins),
            assist_outs: std::mem::take(&mut f.assist_outs),
        }
    }
    fn class_setup(class: &mut Class<Wrapper<max_sys::t_object, Self, T>>) {
        T::class_setup(class);
    }
    fn call_float(&self, index: usize, value: f64) {
        if let Some(f) = self.callbacks_float.get(&index) {
            f(self.wrapped(), value);
        }
    }
    fn call_int(&self, index: usize, value: max_sys::t_atom_long) {
        if let Some(f) = self.callbacks_int.get(&index) {
            f(self.wrapped(), value);
        }
    }
    fn handle_notification(&self, notification: &Notification) {
        handle_buffer_ref_notifications(&self.buffer_refs, notification);
        self.wrapped().handle_notification(notification);
    }
    unsafe fn assist(&self, io: c_long, index: c_long, dest: *mut c_char) {
        let render = |src: &CStr| unsafe {
            let _ = ::max_sys::strncpy_zero(
                dest,
                src.to_bytes_with_nul().as_ptr() as _,
                ASSIST_MAX as _,
            );
        };

        //lookup assist string, if we don't have one, call wrapper method
        let iolet = match io {
            1 => {
                let index = index as usize;
                if let Some(s) = self.assist_ins.get(&index) {
                    render(s.as_c_str());
                    return;
                }
                AssistIOlet::Inlet(index)
            }
            2 => {
                let index = index as usize;
                if let Some(s) = self.assist_outs.get(&index) {
                    render(s.as_c_str());
                    return;
                }
                AssistIOlet::Outlet(index)
            }
            _ => return,
        };

        self.wrapped().assist(iolet, render);
    }
}

impl<T> WrapperInternal<max_sys::t_pxobject, T> for MSPWrapperInternal<T>
where
    T: MSPObjWrapped<T> + Sync + 'static,
{
    fn wrapped(&self) -> &T {
        &self.wrapped
    }
    fn clocks(&self) -> &[Arc<ClockShared>] {
        &self.clocks
    }
    unsafe fn new(owner: *mut max_sys::t_pxobject, sym: SymbolRef, args: &[Atom]) -> Self {
        // SAFETY: the caller guarantees `owner` is valid and under construction
        let mut builder = unsafe { WrappedBuilder::new_msp(owner, sym, args) };
        let wrapped = T::new(&mut builder);
        let mut f = builder.finalize();
        // Always forbid in-place processing: `perform` is handed shared input slices and
        // mutable output slices at the same time, which must never overlap.
        // SAFETY: the caller guarantees `owner` is valid and under construction
        unsafe {
            (*owner).z_misc |= 1; //Z_NO_INPLACE;
        }
        Self {
            wrapped,
            signal_inlets: f.signal_inlets,
            signal_outlets: f.signal_outlets,
            panicked: AtomicBool::new(false),
            callbacks_float: std::mem::take(&mut f.callbacks_float),
            callbacks_int: std::mem::take(&mut f.callbacks_int),
            buffer_refs: std::mem::take(&mut f.buffer_refs),
            clocks: std::mem::take(&mut f.clocks),
            _proxy_inlets: std::mem::take(&mut f.proxy_inlets),
            assist_ins: std::mem::take(&mut f.assist_ins),
            assist_outs: std::mem::take(&mut f.assist_outs),
        }
    }
    fn class_setup(class: &mut Class<Wrapper<max_sys::t_pxobject, Self, T>>) {
        T::class_setup(class);
    }
    fn call_float(&self, index: usize, value: f64) {
        if let Some(f) = self.callbacks_float.get(&index) {
            f(self.wrapped(), value);
        }
    }
    fn call_int(&self, index: usize, value: max_sys::t_atom_long) {
        if let Some(f) = self.callbacks_int.get(&index) {
            f(self.wrapped(), value);
        }
    }
    fn handle_notification(&self, notification: &Notification) {
        handle_buffer_ref_notifications(&self.buffer_refs, notification);
        self.wrapped().handle_notification(notification);
    }
    unsafe fn assist(&self, io: c_long, index: c_long, dest: *mut c_char) {
        let render = |src: &CStr| unsafe {
            let _ = ::max_sys::strncpy_zero(
                dest,
                src.to_bytes_with_nul().as_ptr() as _,
                ASSIST_MAX as _,
            );
        };

        //lookup assist string, if we don't have one, call wrapper method
        let iolet = match io {
            1 => {
                let index = index as usize;
                if let Some(s) = self.assist_ins.get(&index) {
                    render(s.as_c_str());
                    return;
                }
                AssistIOlet::Inlet(index)
            }
            2 => {
                let index = index as usize;
                if let Some(s) = self.assist_outs.get(&index) {
                    render(s.as_c_str());
                    return;
                }
                AssistIOlet::Outlet(index)
            }
            _ => return,
        };

        self.wrapped().assist(iolet, render);
    }
}

fn handle_buffer_ref_notifications(
    buffer_refs: &Vec<ManagedBufferRefInternal>,
    notification: &Notification,
) {
    if BufferRef::is_applicable(notification) {
        for r in buffer_refs {
            unsafe {
                r.notify_if_unchecked(&notification);
            }
        }
    }
}

/// Write silence to every output buffer.
///
/// # Safety
/// `outs` must be null or point to `numouts` pointers, each to `nframes` writable samples, as
/// Max passes to a perform routine.
unsafe fn silence_outputs(outs: *mut *mut f64, numouts: c_long, nframes: usize) {
    if numouts > 0 && !outs.is_null() {
        // SAFETY: guaranteed by the caller
        let outs = unsafe { std::slice::from_raw_parts(outs, numouts as usize) };
        for op in outs {
            unsafe { std::ptr::write_bytes(*op, 0, nframes) };
        }
    }
}

/// The most signal inlets or outlets for which `perform` can be handed slices. The slice tables
/// are built on the stack on every perform call (no allocation or locking on the audio thread),
/// so they need a fixed size.
const MAX_SIGNAL_IO: usize = 64;

impl<T> MSPWrapperInternal<T>
where
    T: MSPObjWrapped<T> + Sync + 'static,
{
    /// Report (once, at dsp64 time) if this object has more signal inlets/outlets than
    /// `perform64` can hand to user code.
    ///
    /// # Safety
    /// `obj` must point to this live object.
    unsafe fn check_signal_io(&self, obj: *mut max_sys::t_object) {
        if self.signal_inlets > MAX_SIGNAL_IO || self.signal_outlets > MAX_SIGNAL_IO {
            // SAFETY: the caller guarantees `obj` is valid
            unsafe {
                crate::object::error(
                    obj,
                    format!(
                        "median: at most {} signal inlets and outlets are supported, \
                         this object has {} and {}; its perform method will not be called \
                         and its outlets are silent",
                        MAX_SIGNAL_IO, self.signal_inlets, self.signal_outlets
                    ),
                );
            }
        }
    }

    /// Called by Max on the audio thread.
    ///
    /// Takes `&self`: other threads may hold `&self` (or `&T`) concurrently. It does not touch
    /// any shared mutable state, all slice tables live on this call's stack. It does not
    /// allocate or lock.
    ///
    /// Relies on `Z_NO_INPLACE` being set (see `new`), so the input and output buffers Max
    /// passes never alias.
    ///
    /// Not `extern "C"`: it is called from Rust inside the outer `perform64`'s panic guard, and a
    /// panic can't unwind out of an `extern "C"` fn (it aborts before the guard sees it).
    fn perform64(
        &self,
        _dsp64: *mut max_sys::t_object,
        ins: *const *const f64,
        numins: c_long,
        outs: *mut *mut f64,
        numouts: c_long,
        sampleframes: c_long,
        _flags: c_long,
        _userparam: *mut c_void,
    ) {
        let nframes = sampleframes.max(0) as usize;

        //convert into slices
        let ins = if numins > 0 && !ins.is_null() {
            // SAFETY: Max passes `numins` input buffer pointers
            unsafe { std::slice::from_raw_parts(ins, numins as usize) }
        } else {
            &[]
        };
        let outs = if numouts > 0 && !outs.is_null() {
            // SAFETY: Max passes `numouts` output buffer pointers
            unsafe { std::slice::from_raw_parts_mut(outs, numouts as usize) }
        } else {
            &mut []
        };

        //more i/o than we have room for: don't call user code at all (it would see fewer
        //buffers than it declared), silence every outlet. The error was posted by dsp64.
        if ins.len() > MAX_SIGNAL_IO || outs.len() > MAX_SIGNAL_IO {
            for op in outs.iter() {
                // SAFETY: Max passes `nframes` valid samples for each output
                unsafe { std::ptr::write_bytes(*op, 0, nframes) };
            }
            return;
        }

        let mut in_slices: [&[f64]; MAX_SIGNAL_IO] = [&[]; MAX_SIGNAL_IO];
        let mut out_slices: [&mut [f64]; MAX_SIGNAL_IO] = std::array::from_fn(|_| &mut [][..]);

        for (slice, ip) in in_slices.iter_mut().zip(ins.iter()) {
            // SAFETY: Max passes `nframes` valid samples for each input
            *slice = unsafe { std::slice::from_raw_parts(*ip, nframes) };
        }
        for (slice, op) in out_slices.iter_mut().zip(outs.iter()) {
            // SAFETY: Max passes `nframes` valid, unaliased (Z_NO_INPLACE) samples per output
            *slice = unsafe { std::slice::from_raw_parts_mut(*op, nframes) };
        }

        self.wrapped()
            .perform(&in_slices[..ins.len()], &mut out_slices[..outs.len()], nframes);
    }
}

/// Look up the registered class for `key` and call `func` with it. `None` if the class isn't
/// registered (or `func` returns `None`, e.g. because allocation failed).
fn new_common<F, O>(key: &'static str, func: F) -> Option<O>
where
    F: Fn(*mut max_sys::t_class) -> Option<O>,
{
    //unlock the mutex so we can register in the object init
    let max_class = {
        //a panic during registration can poison the lock, but the map is still consistent
        let g = CLASSES.lock().unwrap_or_else(PoisonError::into_inner);
        g.get(key)?.0
    };
    func(max_class)
}

impl<O, I, T> WrapperWrapped<T> for Wrapper<O, I, T>
where
    I: WrapperInternal<O, T>,
    T: ObjWrapped<T>,
{
    fn wrapped(&self) -> &T {
        self.internal().wrapped()
    }
}

//build up our float and int input trampolines, and the register fn
macro_rules! int_float_tramps {
    ( $( $i:literal ),+ ) => {
        $(
            paste::paste! {
                pub extern "C" fn [<call_in $i>](&self, value: max_sys::t_atom_long) {
                    self.guard_or(concat!("in", $i), (), || self.internal().call_int($i, value));
                }

                pub extern "C" fn [<call_ft $i>](&self, value: f64) {
                    self.guard_or(concat!("ft", $i), (), || self.internal().call_float($i, value));
                }
            }
        )*

        fn register_ft_in(class: *mut max_sys::t_class) {
            unsafe {
            $(
                paste::paste! {
                    let name = std::ffi::CString::new(concat!("ft", $i)).unwrap();
                    max_sys::class_addmethod(class,
                        Some(std::mem::transmute::<extern "C" fn(&Self, f64), crate::method::MaxMethod>(Self::[<call_ft $i>])),
                        name.as_ptr(),
                        max_sys::e_max_atomtypes::A_FLOAT, 0
                    );

                    let name = std::ffi::CString::new(concat!("in", $i)).unwrap();
                    max_sys::class_addmethod(class,
                        Some(std::mem::transmute::<extern "C" fn(&Self, max_sys::t_atom_long), crate::method::MaxMethod>(Self::[<call_in $i>])),
                        name.as_ptr(),
                        max_sys::e_max_atomtypes::A_LONG, 0
                    );
                })*
            }
        }
    };
}

impl<O, I, T> Wrapper<O, I, T>
where
    I: WrapperInternal<O, T>,
    T: ObjWrapped<T>,
{
    fn internal(&self) -> &I {
        unsafe { &*self.wrapped.as_ptr() }
    }

    /// Run `f`, the body of a method Max called on this object, containing any panic: it is
    /// posted as an error for this object and `neutral` is returned.
    fn guard_or<R, F: FnOnce() -> R>(&self, what: &str, neutral: R, f: F) -> R {
        // SAFETY: `s_obj` is the object header of this live object (a `t_object`, or a
        // `t_pxobject` which starts with one)
        unsafe {
            crate::ffi::guard_raw(
                &self.s_obj as *const O as *mut max_sys::t_object,
                what,
                neutral,
                f,
            )
        }
    }

    /// Let the object's clocks fire. Called once the wrapped value is initialised.
    ///
    /// # Safety
    /// `this` must point to a wrapper whose `wrapped` field is initialised.
    unsafe fn arm_clocks(this: *mut Self) {
        // SAFETY: guaranteed by the caller; `MaybeUninit<I>` has the layout of `I`
        let internal = unsafe { &*std::ptr::addr_of!((*this).wrapped).cast::<I>() };
        for c in internal.clocks() {
            c.arm();
        }
    }

    /// The `free` method Max calls when the object is deleted (also for an object whose
    /// construction failed). Last call Max makes on the object.
    ///
    /// Takes a raw pointer: clock callbacks may still be running on the scheduler thread with
    /// references into the object, so no `&mut` may exist until they are shut out.
    extern "C" fn free_wrapped(this: *mut Self) {
        crate::ffi::guard_global("free", (), || {
            // SAFETY: Max passes the object it is freeing, `this` is valid until we return
            unsafe {
                //construction didn't finish (`T::new` panicked), there is nothing to drop
                if !(*this).constructed {
                    return;
                }
                //(a) shut out the clocks first: this waits for a callback that is running (it
                //holds `&T` into the object) and makes every later one a no-op. Only a shared
                //borrow of the internals exists meanwhile.
                {
                    let internal = &*std::ptr::addr_of!((*this).wrapped).cast::<I>();
                    release_clocks(internal.clocks());
                }
                //(b) nothing else can reach the internals now, drop them in place (no moving,
                //no swapping with uninit: the memory stays initialised until dropped)
                (*this).constructed = false;
                std::ptr::drop_in_place(std::ptr::addr_of_mut!((*this).wrapped).cast::<I>());
            }
        });
    }

    fn register_common<F>(
        lookup_class: bool,
        notification_handler: extern "C" fn(
            &Wrapper<O, I, T>,
            sender_name: *mut max_sys::t_symbol,
            message: *mut max_sys::t_symbol,
            sender: *mut c_void,
            data: *mut c_void,
        ),
        assist_tramp: extern "C" fn(
            &Wrapper<O, I, T>,
            _b: *mut c_void,
            io: c_long,
            index: c_long,
            s: *mut c_char,
        ),
        creator: F,
    ) where
        F: Fn() -> Class<Self>,
    {
        let key = key::<T>();
        let mut h = CLASSES.lock().unwrap_or_else(PoisonError::into_inner);
        if !h.contains_key(key) {
            //don't lookup class unless we want to, because max might try to register it which
            //could cause a loop
            let existing = if lookup_class {
                Class::<T>::find_in_max(T::class_name(), T::class_type())
            } else {
                std::ptr::null_mut()
            };
            let max_class = if existing.is_null() {
                let mut c = creator();
                let notify = std::ffi::CString::new("notify").unwrap();
                let assist = std::ffi::CString::new("assist").unwrap();
                unsafe {
                    //register notifications
                    max_sys::class_addmethod(
                        c.inner(),
                        Some(std::mem::transmute::<_, MaxMethod>(notification_handler)),
                        notify.as_ptr(),
                        max_sys::e_max_atomtypes::A_CANT,
                        0,
                    );
                    //assist
                    max_sys::class_addmethod(
                        c.inner(),
                        Some(std::mem::transmute::<_, MaxMethod>(assist_tramp)),
                        assist.as_ptr(),
                        max_sys::e_max_atomtypes::A_CANT,
                        0,
                    );
                }
                c.register(T::class_type())
                    .expect(format!("failed to register {}", key).as_str());

                //register our ft1, ft2.. in1, in2.. tramps
                Self::register_ft_in(c.inner());
                c.inner()
            } else {
                existing
            };

            h.insert(key, ClassMaxObjWrapper(max_class));
        }
    }

    int_float_tramps!(1, 2, 3, 4, 5, 6, 7, 8, 9);
}

fn key<T>() -> &'static str {
    std::any::type_name::<T>()
}

impl<T> Wrapper<max_sys::t_object, MaxWrapperInternal<T>, T>
where
    T: MaxObjWrapped<T>,
{
    /// Register the class with Max.
    ///
    /// # Remarks
    ///
    /// This method expects to only be called from the main thread. Internally, it locks a mutex
    /// and looks up your class by type name. If your class has alrady been registered it won't
    /// re-register.
    ///
    /// This will deadlock if you call `register()` again inside your `T::class_setup()`.
    pub unsafe fn register(lookup_class: bool) {
        Self::register_common(
            lookup_class,
            Self::handle_notification_tramp,
            Self::assist_tramp,
            || {
                let mut c: Class<Self> = Class::new(
                    T::class_name(),
                    Self::new_tramp,
                    Some(
                        Self::free_wrapped as MaxFree<Self>,
                    ),
                );
                //TODO somehow pass the lock so that classes can register additional classes
                MaxWrapperInternal::<T>::class_setup(&mut c);
                c
            },
        );
    }

    /// A method for Max to create an instance of your class.
    pub unsafe extern "C" fn new_tramp(
        sym: *mut max_sys::t_symbol,
        argc: c_long,
        argv: *const max_sys::t_atom,
    ) -> *mut c_void {
        //a panic (in `T::new` or elsewhere) is posted to the console and Max gets null, which
        //makes it report that the object couldn't be created. Unwinding drops the `ObjBox`,
        //which frees the half-built object; `free_wrapped` knows not to drop the internals.
        crate::ffi::guard_global(key::<T>(), std::ptr::null_mut(), || unsafe {
            let sym: SymbolRef = SymbolRef::from_raw(sym);
            let args = if argc > 0 && !argv.is_null() {
                std::slice::from_raw_parts(std::mem::transmute::<_, _>(argv), argc as usize)
            } else {
                &[]
            };
            match Self::try_new(sym, &args) {
                Some(o) => {
                    //sanity check for the offset based `max_obj()` of the wrapped type
                    if o.max_obj() != o.wrapped().max_obj() {
                        crate::max::error(format!(
                            "median: failed to create {}: object pointer mismatch",
                            key::<T>()
                        ));
                        return std::ptr::null_mut();
                    }
                    std::mem::transmute::<_, _>(ObjBox::into_raw(o))
                }
                None => {
                    crate::max::error(format!("median: failed to create {}", key::<T>()));
                    std::ptr::null_mut()
                }
            }
        })
    }

    /// Create an instance of the wrapper, on the heap, with no arguments.
    ///
    /// # Panics
    /// If the class isn't registered or Max fails to allocate the object.
    pub fn new_noargs() -> ObjBox<Self> {
        Self::new(SymbolRef::default(), &[])
    }

    /// Create an instance of the wrapper, on the heap.
    ///
    /// # Panics
    /// If the class isn't registered or Max fails to allocate the object.
    pub fn new(sym: SymbolRef, args: &[Atom]) -> ObjBox<Self> {
        match Self::try_new(sym, args) {
            Some(o) => o,
            None => panic!("failed to create {}", key::<T>()),
        }
    }

    /// Like `new`, but `None` if the class isn't registered or Max fails to allocate.
    fn try_new(sym: SymbolRef, args: &[Atom]) -> Option<ObjBox<Self>> {
        new_common(key::<T>(), |max_class| unsafe {
            let o: ObjBox<Self> = ObjBox::alloc(max_class)?;
            // SAFETY: `o` is a live, newly allocated wrapper that Max has not seen yet; its
            // fields are uninitialised, so write without dropping the old values. If
            // `new` panics, `o` is dropped while `constructed` is false.
            std::ptr::addr_of_mut!((*o.as_ptr()).constructed).write(false);
            let internal = MaxWrapperInternal::<T>::new(o.max_obj(), sym.clone(), args);
            std::ptr::addr_of_mut!((*o.as_ptr()).wrapped).write(MaybeUninit::new(internal));
            std::ptr::addr_of_mut!((*o.as_ptr()).constructed).write(true);
            //only now that `wrapped` is initialised may clock callbacks run
            Self::arm_clocks(o.as_ptr());
            //process attribute arguments
            //TODO optionally don't process?
            max_sys::attr_args_process(
                o.max_obj() as _,
                args.len() as _,
                std::mem::transmute::<_, _>(args.as_ptr()), //casts to mutable but max doesn't mutate
            );
            Some(o)
        })
    }

    extern "C" fn handle_notification_tramp(
        &self,
        sender_name: *mut max_sys::t_symbol,
        message: *mut max_sys::t_symbol,
        sender: *mut c_void,
        data: *mut c_void,
    ) {
        self.guard_or("notify", (), || {
            // SAFETY: Max passes valid (or null) symbols to a notify method
            let notification = unsafe { Notification::new(sender_name, message, sender, data) };
            self.internal().handle_notification(&notification);
        });
    }

    extern "C" fn assist_tramp(&self, _b: *mut c_void, io: c_long, index: c_long, s: *mut c_char) {
        self.guard_or("assist", (), || {
            // SAFETY: Max passes a writable assist buffer
            unsafe { self.internal().assist(io, index, s) };
        });
    }
}

impl<T> MSPObjWrapper<T>
where
    T: MSPObjWrapped<T> + Sync + 'static,
{
    /// Register the class with Max.
    ///
    /// # Remarks
    ///
    /// This method expects to only be called from the main thread. Internally, it locks a mutex
    /// and looks up your class by type name. If your class has alrady been registered it won't
    /// re-register.
    ///
    /// This will deadlock if you call `register()` again inside your `T::class_setup()`.
    pub unsafe fn register(lookup_class: bool) {
        Self::register_common(
            lookup_class,
            Self::handle_notification_tramp,
            Self::assist_tramp,
            || {
                let mut c: Class<Self> = Class::new(
                    T::class_name(),
                    Self::new_tramp,
                    Some(
                        Self::free_msp as MaxFree<Self>,
                    ),
                );
                //TODO somehow pass the lock so that classes can register additional classes
                MSPWrapperInternal::<T>::class_setup(&mut c);
                let dsp64 = CString::new("dsp64").unwrap();
                // SAFETY: `c.inner()` is the valid class being registered, `dsp64` is a
                // NUL-terminated string that outlives the call (Max copies the name), and
                // `Self::dsp64` is an `extern "C"` function with the signature Max calls `dsp64` with
                unsafe {
                    max_sys::class_addmethod(
                        c.inner(),
                        Some(std::mem::transmute::<
                            extern "C" fn(
                                &Self,
                                dsp64: *mut max_sys::t_object,
                                count: *mut std::os::raw::c_short,
                                samplerate: f64,
                                maxvectorsize: c_long,
                                flags: c_long,
                            ),
                            MaxMethod,
                        >(Self::dsp64)),
                        dsp64.as_ptr(),
                        max_sys::e_max_atomtypes::A_CANT,
                        0,
                    );
                }
                // SAFETY: `c.inner()` is the valid class being registered, and its methods have
                // been added
                unsafe { max_sys::class_dspinit(c.inner()) };
                c
            },
        );
    }

    /// A method for Max to create an instance of your class.
    pub unsafe extern "C" fn new_tramp(
        sym: *mut max_sys::t_symbol,
        argc: c_long,
        argv: *const max_sys::t_atom,
    ) -> *mut c_void {
        //a panic (in `T::new` or elsewhere) is posted to the console and Max gets null, which
        //makes it report that the object couldn't be created. Unwinding drops the `ObjBox`,
        //which frees the half-built object; `free_wrapped` knows not to drop the internals.
        crate::ffi::guard_global(key::<T>(), std::ptr::null_mut(), || unsafe {
            let sym: SymbolRef = SymbolRef::from_raw(sym);
            let args = if argc > 0 && !argv.is_null() {
                std::slice::from_raw_parts(std::mem::transmute::<_, _>(argv), argc as usize)
            } else {
                &[]
            };
            match Self::try_new(sym, &args) {
                Some(o) => {
                    //sanity check for the offset based `max_obj()` of the wrapped type
                    if o.msp_obj() != o.wrapped().msp_obj() {
                        crate::max::error(format!(
                            "median: failed to create {}: object pointer mismatch",
                            key::<T>()
                        ));
                        return std::ptr::null_mut();
                    }
                    std::mem::transmute::<_, _>(ObjBox::into_raw(o))
                }
                None => {
                    crate::max::error(format!("median: failed to create {}", key::<T>()));
                    std::ptr::null_mut()
                }
            }
        })
    }

    /// Create an instance of the wrapper, on the heap.
    ///
    /// # Panics
    /// If the class isn't registered or Max fails to allocate the object.
    pub fn new(sym: SymbolRef, args: &[Atom]) -> ObjBox<Self> {
        match Self::try_new(sym, args) {
            Some(o) => o,
            None => panic!("failed to create {}", key::<T>()),
        }
    }

    /// Like `new`, but `None` if the class isn't registered or Max fails to allocate.
    fn try_new(sym: SymbolRef, args: &[Atom]) -> Option<ObjBox<Self>> {
        unsafe {
            new_common(key::<T>(), |max_class| {
                let o: ObjBox<Self> = ObjBox::alloc(max_class)?;
                // SAFETY: `o` is a live, newly allocated wrapper that Max has not seen yet; its
                // fields are uninitialised, so write without dropping the old values. If
                // `new` panics, `o` is dropped while `constructed` is false.
                std::ptr::addr_of_mut!((*o.as_ptr()).constructed).write(false);
                let internal = MSPWrapperInternal::<T>::new(o.msp_obj(), sym.clone(), args);
                std::ptr::addr_of_mut!((*o.as_ptr()).wrapped).write(MaybeUninit::new(internal));
                std::ptr::addr_of_mut!((*o.as_ptr()).constructed).write(true);
                //only now that `wrapped` is initialised may clock callbacks run
                Self::arm_clocks(o.as_ptr());
                //process attribute arguments
                //TODO optionally don't process?
                max_sys::attr_args_process(
                    o.max_obj() as _,
                    args.len() as _,
                    std::mem::transmute::<_, _>(args.as_ptr()), //casts to mutable but max doesn't mutate
                );
                Some(o)
            })
        }
    }

    extern "C" fn free_msp(this: *mut Self) {
        //releasing the clocks drops user closures, so keep it inside the panic guard
        crate::ffi::guard_global("free", (), || Self::free_msp_inner(this));
        Self::free_wrapped(this);
    }

    fn free_msp_inner(this: *mut Self) {
        // SAFETY: Max passes the object it is freeing
        let constructed = unsafe { (*this).constructed };
        //shut out the clocks first: a running callback holds `&W` over the whole object,
        //including the pxobject header `z_dsp_free` writes. Then free dsp (this takes the object
        //out of the dsp chain, so `perform` can't run afterwards), unless construction didn't get
        //as far as setting it up. `free_wrapped` clears the clocks again, which is a no-op.
        if constructed {
            unsafe {
                let internal = &*std::ptr::addr_of!((*this).wrapped).cast::<MSPWrapperInternal<T>>();
                release_clocks(internal.clocks());
                max_sys::z_dsp_free(this as *mut max_sys::t_pxobject);
            }
        }
    }

    extern "C" fn perform64(
        &self,
        dsp64: *mut max_sys::t_object,
        ins: *const *const f64,
        numins: c_long,
        outs: *mut *mut f64,
        numouts: c_long,
        sampleframes: c_long,
        flags: c_long,
        userparam: *mut c_void,
    ) {
        let internal = self.internal();
        let nframes = sampleframes.max(0) as usize;
        //the user's perform panicked earlier, stay silent until DSP is restarted
        if internal.panicked.load(Ordering::Relaxed) {
            // SAFETY: Max passes `numouts` output buffers of `sampleframes` samples
            unsafe { silence_outputs(outs, numouts, nframes) };
            return;
        }
        // no allocation unless `perform` panics
        if let Err(msg) = crate::ffi::catch(|| {
            internal.perform64(dsp64, ins, numins, outs, numouts, sampleframes, flags, userparam)
        }) {
            internal.panicked.store(true, Ordering::Relaxed);
            // SAFETY: as above
            unsafe { silence_outputs(outs, numouts, nframes) };
            MaxObj::post_error(self, format!(
                "{} (the object is silent until DSP is restarted)",
                crate::ffi::panic_text("perform", &msg)
            ));
        }
    }

    extern "C" fn dsp64(
        &self,
        dsp64: *mut max_sys::t_object,
        _count: *mut std::os::raw::c_short,
        samplerate: f64,
        _maxvectorsize: c_long,
        _flags: c_long,
    ) {
        let ok = self.guard_or("dsp64", false, || unsafe {
            self.internal().panicked.store(false, Ordering::Relaxed);
            self.internal().check_signal_io(self.max_obj());
            self.wrapped().dsp_setup(samplerate);
            true
        });
        if !ok {
            //`dsp_setup` panicked: still add the perform routine, which then silences the outlets
            self.internal().panicked.store(true, Ordering::Relaxed);
        }
        unsafe {
            max_sys::dsp_add64(
                dsp64,
                self.max_obj(),
                Some(std::mem::transmute::<
                    extern "C" fn(
                        &Self,
                        dsp64: *mut max_sys::t_object,
                        ins: *const *const f64,
                        numins: c_long,
                        outs: *mut *mut f64,
                        numouts: c_long,
                        sampleframes: c_long,
                        flags: c_long,
                        userparam: *mut c_void,
                    ),
                    unsafe extern "C" fn(
                        x: *mut max_sys::t_object,
                        dsp64: *mut max_sys::t_object,
                        ins: *mut *mut f64,
                        numins: c_long,
                        outs: *mut *mut f64,
                        numouts: c_long,
                        sampleframes: c_long,
                        flags: c_long,
                        userparam: *mut c_void,
                    ),
                >(Self::perform64)),
                0,
                std::ptr::null_mut(),
            );
        }
    }

    extern "C" fn handle_notification_tramp(
        &self,
        sender_name: *mut max_sys::t_symbol,
        message: *mut max_sys::t_symbol,
        sender: *mut c_void,
        data: *mut c_void,
    ) {
        self.guard_or("notify", (), || {
            // SAFETY: Max passes valid (or null) symbols to a notify method
            let notification = unsafe { Notification::new(sender_name, message, sender, data) };
            self.internal().handle_notification(&notification);
        });
    }

    extern "C" fn assist_tramp(&self, _b: *mut c_void, io: c_long, index: c_long, s: *mut c_char) {
        self.guard_or("assist", (), || {
            // SAFETY: Max passes a writable assist buffer
            unsafe { self.internal().assist(io, index, s) };
        });
    }
}

unsafe impl<T> MaxObj for T
where
    T: MaxObjWrapped<T>,
{
    fn max_obj(&self) -> *mut max_sys::t_object {
        //can't seem to get from wrapper to internal because of MaybeUninit?
        let off1 = field_offset::offset_of!(Wrapper::<max_sys::t_object, MaxWrapperInternal<T>, T> => wrapped);
        let off2 = field_offset::offset_of!(MaxWrapperInternal::<T> => wrapped);
        unsafe {
            let ptr: *mut u8 = std::mem::transmute::<_, *mut u8>(self as *const T);
            std::mem::transmute::<_, *mut max_sys::t_object>(
                ptr.offset(-((off1.get_byte_offset() + off2.get_byte_offset()) as isize)),
            )
        }
    }
}

unsafe impl<T> MSPObj for T
where
    T: MSPObjWrapped<T>,
{
    fn msp_obj(&self) -> *mut max_sys::t_pxobject {
        //can't seem to get from wrapper to internal because of MaybeUninit?
        let off1 = field_offset::offset_of!(Wrapper::<max_sys::t_pxobject, MSPWrapperInternal<T>, T> => wrapped);
        let off2 = field_offset::offset_of!(MSPWrapperInternal::<T> => wrapped);
        unsafe {
            let ptr: *mut u8 = std::mem::transmute::<_, *mut u8>(self as *const T);
            std::mem::transmute::<_, *mut max_sys::t_pxobject>(
                ptr.offset(-((off1.get_byte_offset() + off2.get_byte_offset()) as isize)),
            )
        }
    }
}

impl<T> WrappedDefer<MaxObjWrapper<T>> for T
where
    T: MaxObjWrapped<T>,
{
    unsafe fn defer(&self, meth: DeferMethodWrapped<MaxObjWrapper<T>>, sym: SymbolRef, atoms: &[Atom]) {
        let obj = self.max_obj();
        // SAFETY: `obj` is this live object and `meth` is a trampoline for it; the caller
        // guarantees that the object outlives the deferred call
        unsafe {
            crate::thread::defer(std::mem::transmute::<_, _>(meth), obj, sym, atoms);
        }
    }

    unsafe fn defer_low(
        &self,
        meth: DeferMethodWrapped<MaxObjWrapper<T>>,
        sym: SymbolRef,
        atoms: &[Atom],
    ) {
        let obj = self.max_obj();
        // SAFETY: `obj` is this live object and `meth` is a trampoline for it; the caller
        // guarantees that the object outlives the deferred call
        unsafe {
            crate::thread::defer_low(std::mem::transmute::<_, _>(meth), obj, sym, atoms);
        }
    }
}

impl<T> WrappedDefer<MSPObjWrapper<T>> for T
where
    T: MSPObjWrapped<T>,
{
    unsafe fn defer(&self, meth: DeferMethodWrapped<MSPObjWrapper<T>>, sym: SymbolRef, atoms: &[Atom]) {
        let obj = self.as_max_obj();
        // SAFETY: `obj` is this live object and `meth` is a trampoline for it; the caller
        // guarantees that the object outlives the deferred call
        unsafe {
            crate::thread::defer(std::mem::transmute::<_, _>(meth), obj, sym, atoms);
        }
    }

    unsafe fn defer_low(
        &self,
        meth: DeferMethodWrapped<MSPObjWrapper<T>>,
        sym: SymbolRef,
        atoms: &[Atom],
    ) {
        let obj = self.as_max_obj();
        // SAFETY: `obj` is this live object and `meth` is a trampoline for it; the caller
        // guarantees that the object outlives the deferred call
        unsafe {
            crate::thread::defer_low(std::mem::transmute::<_, _>(meth), obj, sym, atoms);
        }
    }
}
