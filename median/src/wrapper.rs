//! External Max and MSP object wrappers
//! TODO: Jitter

use crate::{
    atom::Atom,
    buffer::BufferRef,
    builder::{Builder, Iolets, MSPBuilder, ManagedBufferRefInternal, MaxBuilder},
    class::{Class, ClassType},
    error::MaxResult,
    clock::ClockShared,
    method::{MaxFree, MaxMethod},
    notify::Notification,
    object::{MSPObj, MaxObj, ObjBox, ObjRef},
    symbol::SymbolRef,
};

use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::Display,
    ffi::{c_char, c_void, CStr},
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

/// The longest assist text, in bytes, that is given to Max (`ASSIST_MAX_STRING_LEN` in the SDK's
/// `ext_assist.h`). Max's assist buffers are at least 512 bytes.
pub const ASSIST_MAX_STRING_LEN: usize = 500;

pub type MaxObjWrapper<T> = Wrapper<max_sys::t_object, MaxWrapperInternal<T>, T>;
pub type MSPObjWrapper<T> = Wrapper<max_sys::t_pxobject, MSPWrapperInternal<T>, T>;

pub type DeferMethodWrapped<T> = unsafe extern "C" fn(
    wrapper: &T,
    sym: *mut max_sys::t_symbol,
    argc: std::os::raw::c_short,
    argv: *const max_sys::t_atom,
);

/// An inlet or an outlet of an object, by position (0 is the leftmost), as the `assist` hook
/// is asked about it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum IoLet {
    Inlet(usize),
    Outlet(usize),
}

impl IoLet {
    /// From the `io` (1 inlet, 2 outlet) and `index` Max passes to `assist`.
    fn from_raw(io: c_long, index: c_long) -> Option<Self> {
        let index = usize::try_from(index).ok()?;
        match io {
            1 => Some(Self::Inlet(index)),
            2 => Some(Self::Outlet(index)),
            _ => None,
        }
    }
}

/// Copy `text` into Max's assist buffer: at most [`ASSIST_MAX_STRING_LEN`] bytes, cut on a
/// character boundary, and a terminating NUL. (An interior NUL ends the text early.)
///
/// # Safety
/// `dest` must be null or writable for `ASSIST_MAX_STRING_LEN + 1` bytes.
unsafe fn write_assist(dest: *mut c_char, text: &str) {
    if dest.is_null() {
        return;
    }
    let mut n = text.len().min(ASSIST_MAX_STRING_LEN);
    while !text.is_char_boundary(n) {
        n -= 1;
    }
    // SAFETY: the caller's contract, and `n <= ASSIST_MAX_STRING_LEN`
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), dest.cast::<u8>(), n);
        dest.add(n).write(0);
    }
}

/// What `CLASS_METHOD_ATTR_PARSE(c, name, "undocumented", gensym("long"), 0, "1")` does: the
/// method is not shown to users (`ext_obex_util.h`).
///
/// # Safety
/// `class` must be a valid class that has the method `name`.
unsafe fn mark_undocumented(class: *mut max_sys::t_class, name: &CStr) {
    // SAFETY: the caller's contract; the names are NUL-terminated strings that outlive the calls
    unsafe {
        let methods = max_sys::class_extra_lookup(class, max_sys::gensym(c"methods".as_ptr()))
            as *mut max_sys::t_hashtab;
        if methods.is_null() {
            return;
        }
        let mut m: *mut max_sys::t_object = std::ptr::null_mut();
        let _ = max_sys::hashtab_lookup(methods, max_sys::gensym(name.as_ptr()), &mut m);
        if !m.is_null() {
            let _ = max_sys::object_addattr_parse(
                m,
                c"undocumented".as_ptr(),
                max_sys::gensym(c"long".as_ptr()),
                0,
                c"1".as_ptr(),
            );
        }
    }
}

//reexports
///trampoline for deffered calls
pub use median_macros::wrapped_defer_tramp as defer_tramp;

//we only use ClassMaxObjWrapper in CLASSES after we've registered the class, for max's usage this is
//Send
#[repr(transparent)]
struct ClassMaxObjWrapper(*mut max_sys::t_class);
unsafe impl Send for ClassMaxObjWrapper {}

/// The identity of a class: its name, its box type and which wrapper it lives in.
///
/// `#[derive(MaxObject)]` implements this; by hand it is:
///
/// ```
/// # use median::{class::ClassType, wrapper::*};
/// # use std::ffi::CStr;
/// # struct Simp;
/// impl ObjClass for Simp {
///     type Wrapper = MaxObjWrapper<Self>; // MSPObjWrapper<Self> for an MSP object
///     fn class_name() -> &'static CStr {
///         c"simp"
///     }
///     // optional, `ClassType::Box` by default
///     fn class_type() -> ClassType {
///         ClassType::Box
///     }
/// }
/// ```
///
/// [`MaxObjWrapped`] requires `Wrapper = MaxObjWrapper<Self>` and [`MSPObjWrapped`] requires
/// `Wrapper = MSPObjWrapper<Self>`, so a type can't be both, and a mismatch between the two
/// impls is a type error.
pub trait ObjClass: Sized {
    /// The wrapper that Max allocates for an object of this class: [`MaxObjWrapper<Self>`] or
    /// [`MSPObjWrapper<Self>`].
    type Wrapper;

    /// The name of your class, this is what you'll type into a box in Max if your class is a
    /// `ClassType::Box`.
    fn class_name() -> &'static CStr;

    /// The type of your class. Defaults to 'box' which creates visual objects in Max.
    fn class_type() -> ClassType {
        ClassType::Box
    }
}

/// The message methods of a class, registered with Max when the class is.
///
/// `#[median::methods]` on your inherent impl implements this from the marked methods
/// (`#[bang]`, `#[int]`, `#[sel("name")]` ...). A type without such a block uses
/// `#[max(no_methods)]` on the derive, or writes `impl Methods for Foo {}`. By hand, call the
/// [`Class`] registration methods:
///
/// ```
/// # use median::{class::Class, error::MaxResult, object::MaxObj, wrapper::*};
/// # struct Simp;
/// # impl Simp { fn bang(&self) {} }
/// impl Methods for Simp {
///     fn register<W>(c: &mut Class<W>) -> MaxResult<()>
///     where
///         W: WrapperWrapped<Self> + MaxObj,
///     {
///         c.add_bang(Self::bang)?;
///         Ok(())
///     }
/// }
/// ```
///
/// The wrapper calls it once, when the class is registered, after [`FieldAttrs::register_attrs`]
/// and before `class_setup`. An `Err` stops the registration: the error is posted to the Max
/// console and the class is not registered.
pub trait Methods: Sized {
    /// Register the methods. `W` is the wrapper type, it is inferred.
    fn register<W>(_class: &mut Class<W>) -> MaxResult<()>
    where
        W: WrapperWrapped<Self> + MaxObj,
    {
        Ok(())
    }
}

/// The attributes backed by fields of the struct (`#[attr]` on a field, with
/// `#[derive(MaxObject)]`), registered with Max when the class is.
///
/// The derive always implements this, with an empty body when no field has `#[attr]`. By hand,
/// call [`Class::add_field_attr`] (or write `impl FieldAttrs for Foo {}` for none):
///
/// ```
/// # use median::{attr::AttrOptions, class::Class, error::MaxResult, num::Int64, object::MaxObj, wrapper::*};
/// # struct Simp { value: Int64 }
/// impl FieldAttrs for Simp {
///     fn register_attrs<W>(c: &mut Class<W>) -> MaxResult<()>
///     where
///         W: WrapperWrapped<Self> + MaxObj,
///     {
///         c.add_field_attr(c"value", |s: &Self| &s.value, AttrOptions::new().clip(0., 100.))
///     }
/// }
/// ```
///
/// Called first when the class is registered, before [`Methods::register`] and `class_setup`.
pub trait FieldAttrs: Sized {
    /// Register the attributes. `W` is the wrapper type, it is inferred.
    fn register_attrs<W>(_class: &mut Class<W>) -> MaxResult<()>
    where
        W: WrapperWrapped<Self> + MaxObj,
    {
        Ok(())
    }
}

/// The trait to implement for your object to be wrapped as a Max object.
///
/// The supertraits are the class identity ([`ObjClass`], with `Wrapper = MaxObjWrapper<Self>`),
/// the message methods ([`Methods`]) and the field attributes ([`FieldAttrs`]); with
/// `#[derive(MaxObject)]` and `#[median::methods]` they are generated. Registration calls
/// [`FieldAttrs::register_attrs`], then [`Methods::register`], then [`MaxObjWrapped::class_setup`].
pub trait MaxObjWrapped:
    ObjClass<Wrapper = MaxObjWrapper<Self>> + Methods + FieldAttrs + Sync + 'static
{
    /// A constructor for your object.
    ///
    /// # Arguments
    ///
    /// * `builder` - describes the inlets, makes the outlets, clocks and buffer references, and
    ///   hands out the object's [`ObjRef`]; see [`MaxBuilder`].
    fn new(builder: &mut MaxBuilder<Self>) -> Self;

    /// Register anything else your class needs (aliases, methods with computed names, ...),
    /// after the field attributes and the methods.
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        //default, do nothing
    }

    /// Called when a method registered with `Class::add_*` (`add_bang`, `add_int`, `add_sel`,
    /// ..., which is what `#[median::methods]` uses) or an attribute setter returns `Err`, with
    /// the error. By default it is posted to the Max console as an error for the object.
    /// Override it to route errors elsewhere, to a status outlet or a logger for example.
    ///
    /// `obj` is the handle of the object the method was called on, valid for the call only. It is
    /// passed in because your struct may not keep an [`ObjRef`].
    fn report_error(&self, obj: &ObjRef, err: &dyn Display) {
        obj.error(err);
    }

    /// Handle notifications that your object gets
    fn handle_notification(&self, _notification: &Notification) {}

    /// The assist text of an inlet or outlet, computed when Max asks for it. `None` (the
    /// default) uses the text given to the builder, or the iolet's kind ("int", "list",
    /// "inlet 2", ...).
    ///
    /// Max calls this on the main thread on every hover, every repaint of the box (once per
    /// inlet) and when it lists connections, and never caches the result: it must be cheap, and
    /// must not take a lock that the audio or scheduler threads may hold (use atomics). It may
    /// allocate. At most [`ASSIST_MAX_STRING_LEN`] bytes are shown.
    fn assist(&self, _iolet: IoLet) -> Option<Cow<'_, str>> {
        None
    }

    /// Whether inlet `index` is hot (Max draws hot inlets differently), computed when Max asks.
    /// `None` (the default) uses the builder's: inlet 0 and the inlets marked
    /// [`hot`](crate::inlet::Inlet::hot) are hot. The same rules as for
    /// [`assist`](Self::assist) apply: main thread, often, no locks the other threads hold.
    fn inlet_hot(&self, _index: usize) -> Option<bool> {
        None
    }
}

/// The trait to implement for your object to be wrapped as a MSP object.
///
/// As [`MaxObjWrapped`], with `Wrapper = MSPObjWrapper<Self>`, plus `perform` and `dsp_setup`.
pub trait MSPObjWrapped:
    ObjClass<Wrapper = MSPObjWrapper<Self>> + Methods + FieldAttrs + Sync + 'static
{
    /// A constructor for your object.
    ///
    /// # Arguments
    ///
    /// * `builder` - as for [`MaxObjWrapped::new`], plus signal inlets and outlets; see
    ///   [`MSPBuilder`].
    fn new(builder: &mut MSPBuilder<Self>) -> Self;

    /// Perform DSP.
    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], nframes: usize);

    /// Register anything else your class needs (aliases, methods with computed names, ...),
    /// after the field attributes and the methods.
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

    /// As [`MaxObjWrapped::report_error`].
    fn report_error(&self, obj: &ObjRef, err: &dyn Display) {
        obj.error(err);
    }

    /// Handle notifications that your object gets
    fn handle_notification(&self, _notification: &Notification) {}

    /// As [`MaxObjWrapped::assist`].
    fn assist(&self, _iolet: IoLet) -> Option<Cow<'_, str>> {
        None
    }

    /// As [`MaxObjWrapped::inlet_hot`].
    fn inlet_hot(&self, _index: usize) -> Option<bool> {
        None
    }
}

/// What a wrapper type gives the generic trampolines (`Class::add_*`): the wrapped value, and the
/// hooks of the wrapped type that they call.
///
/// Implemented by [`MaxObjWrapper`] and [`MSPObjWrapper`].
pub trait WrapperWrapped<T> {
    /// Retrieve a reference to your wrapped class.
    fn wrapped(&self) -> &T;

    /// Give an error returned by a method of the wrapped value to its `report_error` hook
    /// ([`MaxObjWrapped::report_error`] or [`MSPObjWrapped::report_error`]).
    fn report_error(&self, err: &dyn Display);
}

/// The actual struct that is given to max
#[repr(C)]
pub struct Wrapper<O, I, T> {
    s_obj: O,
    wrapped: MaybeUninit<I>,
    /// `wrapped` is initialised. Max can free the object before construction finished (a panic
    /// in `T::new`), in which case `free` must not drop `wrapped`. Written (with `Release`) once
    /// after `wrapped` is written, and cleared in `free`. Safe code can make Max call back into
    /// the object while `T::new` is still running (through the `ObjRef` from `builder.obj()`), so
    /// every trampoline that reaches `wrapped` checks this first (with `Acquire`): see
    /// `Wrapper::internal`.
    constructed: AtomicBool,
    _phantom: PhantomData<T>,
}

/// Inner struct for wrapping [`MaxObjWrapped`]
pub struct MaxWrapperInternal<T> {
    wrapped: T,
    //the inlet callbacks, the assist/hot table, the proxies (freed when this is dropped)
    iolets: Iolets<T>,
    buffer_refs: Vec<ManagedBufferRefInternal>,
    //disabled by `free_wrapped` before `wrapped` is dropped (and again in `Drop`)
    clocks: Vec<Arc<ClockShared>>,
}

/// Inner struct for wrapping [`MSPObjWrapped`]
pub struct MSPWrapperInternal<T> {
    wrapped: T,
    /// `perform` panicked: the object stays silent (and `perform` is not called again, so the
    /// audio thread doesn't post an error every block) until the next `dsp64`.
    panicked: AtomicBool,
    //as in `MaxWrapperInternal`
    iolets: Iolets<T>,
    buffer_refs: Vec<ManagedBufferRefInternal>,
    //disabled by `free_wrapped` before `wrapped` is dropped (and again in `Drop`)
    clocks: Vec<Arc<ClockShared>>,
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
        fn report_error(&self, obj: &ObjRef, err: &dyn Display);
        /// # Safety
        /// `s` must be null or point to a writable buffer of at least 512 bytes, as Max passes to
        /// an assist method.
        unsafe fn assist(&self, io: c_long, index: c_long, s: *mut c_char);
        /// Whether inlet `index` is hot.
        fn inlet_hot(&self, index: usize) -> bool;
    }
}
use sealed::WrapperInternal;

unsafe impl<I, T> MaxObj for Wrapper<max_sys::t_object, I, T> {}
unsafe impl<I, T> MaxObj for Wrapper<max_sys::t_pxobject, I, T> {}
unsafe impl<I, T> MSPObj for Wrapper<max_sys::t_pxobject, I, T> {}

impl<T> WrapperInternal<max_sys::t_object, T> for MaxWrapperInternal<T>
where
    T: MaxObjWrapped,
{
    fn wrapped(&self) -> &T {
        &self.wrapped
    }
    fn clocks(&self) -> &[Arc<ClockShared>] {
        &self.clocks
    }
    unsafe fn new(owner: *mut max_sys::t_object, sym: SymbolRef, args: &[Atom]) -> Self {
        // SAFETY: the caller guarantees `owner` is valid and under construction
        let mut builder: MaxBuilder<T> =
            unsafe { Builder::new(std::ptr::NonNull::new_unchecked(owner), sym, args) };
        let wrapped = T::new(&mut builder);
        let iolets = builder
            .finalize()
            .unwrap_or_else(|e| panic!("{}", e));
        Self {
            wrapped,
            iolets,
            buffer_refs: builder.take_buffer_refs(),
            clocks: builder.take_clocks(),
        }
    }
    fn class_setup(class: &mut Class<Wrapper<max_sys::t_object, Self, T>>) {
        register_members::<_, T>(class);
        T::class_setup(class);
    }
    fn call_float(&self, index: usize, value: f64) {
        self.iolets.call_float(&self.wrapped, index, value);
    }
    fn call_int(&self, index: usize, value: max_sys::t_atom_long) {
        self.iolets.call_int(&self.wrapped, index, value);
    }
    fn handle_notification(&self, notification: &Notification) {
        handle_buffer_ref_notifications(&self.buffer_refs, notification);
        self.wrapped().handle_notification(notification);
    }
    fn report_error(&self, obj: &ObjRef, err: &dyn Display) {
        self.wrapped().report_error(obj, err);
    }
    unsafe fn assist(&self, io: c_long, index: c_long, dest: *mut c_char) {
        let Some(iolet) = IoLet::from_raw(io, index) else {
            return;
        };
        //the hook first, then the table
        let text = match self.wrapped.assist(iolet) {
            Some(text) => text,
            None => self.iolets.assist(iolet),
        };
        // SAFETY: the caller's contract
        unsafe { write_assist(dest, &text) };
    }
    fn inlet_hot(&self, index: usize) -> bool {
        self.wrapped
            .inlet_hot(index)
            .unwrap_or_else(|| self.iolets.hot(index))
    }
}

impl<T> WrapperInternal<max_sys::t_pxobject, T> for MSPWrapperInternal<T>
where
    T: MSPObjWrapped,
{
    fn wrapped(&self) -> &T {
        &self.wrapped
    }
    fn clocks(&self) -> &[Arc<ClockShared>] {
        &self.clocks
    }
    unsafe fn new(owner: *mut max_sys::t_pxobject, sym: SymbolRef, args: &[Atom]) -> Self {
        // SAFETY: the caller guarantees `owner` is valid and under construction
        let mut builder: MSPBuilder<T> =
            unsafe { Builder::new(std::ptr::NonNull::new_unchecked(owner.cast()), sym, args) };
        let wrapped = T::new(&mut builder);
        let iolets = builder
            .finalize()
            .unwrap_or_else(|e| panic!("{}", e));
        // Always forbid in-place processing: `perform` is handed shared input slices and
        // mutable output slices at the same time, which must never overlap.
        // SAFETY: the caller guarantees `owner` is valid and under construction
        unsafe {
            (*owner).z_misc |= 1; //Z_NO_INPLACE;
        }
        Self {
            wrapped,
            panicked: AtomicBool::new(false),
            iolets,
            buffer_refs: builder.take_buffer_refs(),
            clocks: builder.take_clocks(),
        }
    }
    fn class_setup(class: &mut Class<Wrapper<max_sys::t_pxobject, Self, T>>) {
        register_members::<_, T>(class);
        T::class_setup(class);
    }
    fn call_float(&self, index: usize, value: f64) {
        self.iolets.call_float(&self.wrapped, index, value);
    }
    fn call_int(&self, index: usize, value: max_sys::t_atom_long) {
        self.iolets.call_int(&self.wrapped, index, value);
    }
    fn handle_notification(&self, notification: &Notification) {
        handle_buffer_ref_notifications(&self.buffer_refs, notification);
        self.wrapped().handle_notification(notification);
    }
    fn report_error(&self, obj: &ObjRef, err: &dyn Display) {
        self.wrapped().report_error(obj, err);
    }
    unsafe fn assist(&self, io: c_long, index: c_long, dest: *mut c_char) {
        let Some(iolet) = IoLet::from_raw(io, index) else {
            return;
        };
        //the hook first, then the table
        let text = match self.wrapped.assist(iolet) {
            Some(text) => text,
            None => self.iolets.assist(iolet),
        };
        // SAFETY: the caller's contract
        unsafe { write_assist(dest, &text) };
    }
    fn inlet_hot(&self, index: usize) -> bool {
        self.wrapped
            .inlet_hot(index)
            .unwrap_or_else(|| self.iolets.hot(index))
    }
}

/// Register the field attributes and the methods of `T`, in that order, before its `class_setup`.
///
/// # Panics
/// If one of them fails: registration stops, and the class is not registered. (`register` runs
/// inside the panic guard of `ext_main`, or of the derived `register`, which posts the message.)
fn register_members<W, T>(class: &mut Class<W>)
where
    W: WrapperWrapped<T> + MaxObj,
    T: FieldAttrs + Methods,
{
    if let Err(e) = T::register_attrs(class) {
        panic!("{}: failed to register the field attributes: {:?}", key::<T>(), e);
    }
    if let Err(e) = <T as Methods>::register(class) {
        panic!("{}: failed to register the methods: {:?}", key::<T>(), e);
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
    T: MSPObjWrapped,
{
    /// Report (once, at dsp64 time) if this object has more signal inlets/outlets than
    /// `perform64` can hand to user code.
    ///
    /// # Safety
    /// `obj` must point to this live object.
    unsafe fn check_signal_io(&self, obj: *mut max_sys::t_object) {
        let (signal_inlets, signal_outlets) = (self.iolets.signal_inlets, self.iolets.signal_outlets);
        if signal_inlets > MAX_SIGNAL_IO || signal_outlets > MAX_SIGNAL_IO {
            // SAFETY: the caller guarantees `obj` is valid
            unsafe {
                crate::object::error(
                    obj,
                    format!(
                        "median: at most {} signal inlets and outlets are supported, \
                         this object has {} and {}; its perform method will not be called \
                         and its outlets are silent",
                        MAX_SIGNAL_IO, signal_inlets, signal_outlets
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
{
    fn wrapped(&self) -> &T {
        self.internal().wrapped()
    }

    fn report_error(&self, err: &dyn Display) {
        let internal = self.internal();
        // SAFETY: `s_obj` is the header of this live object, and the handle is dropped when this
        // call returns, so it can't outlive the object
        let obj = unsafe {
            ObjRef::from_raw(std::ptr::NonNull::from(&self.s_obj).cast::<max_sys::t_object>())
        };
        internal.report_error(&obj, err);
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
                    //wrapper-internal: hidden from the object's message list
                    mark_undocumented(class, &name);

                    let name = std::ffi::CString::new(concat!("in", $i)).unwrap();
                    max_sys::class_addmethod(class,
                        Some(std::mem::transmute::<extern "C" fn(&Self, max_sys::t_atom_long), crate::method::MaxMethod>(Self::[<call_in $i>])),
                        name.as_ptr(),
                        max_sys::e_max_atomtypes::A_LONG, 0
                    );
                    mark_undocumented(class, &name);
                })*
            }
        }
    };
}

impl<O, I, T> Wrapper<O, I, T>
where
    I: WrapperInternal<O, T>,
{
    /// The internals of an object whose construction has finished.
    ///
    /// # Panics
    /// If the object isn't constructed yet (or is being freed): Max called into it while `T::new`
    /// was still running, which safe code can cause through the `ObjRef` the builder gives out.
    /// Every trampoline runs this inside a panic guard, which posts the panic message as an
    /// error for the object and returns the neutral value, so the object is never touched.
    fn internal(&self) -> &I {
        if !self.constructed.load(Ordering::Acquire) {
            panic!("{}: object used before construction finished", key::<T>());
        }
        // SAFETY: `constructed` was set after `wrapped` was written, and is cleared before it
        // is dropped
        unsafe { self.internal_unchecked() }
    }

    /// Like `internal`, without the check, for the paths that run outside a panic guard.
    ///
    /// # Safety
    /// The object must be constructed and not being freed.
    unsafe fn internal_unchecked(&self) -> &I {
        // SAFETY: guaranteed by the caller
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
                if !(*this).constructed.load(Ordering::Acquire) {
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
                (*this).constructed.store(false, Ordering::Release);
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
        T: ObjClass,
    {
        let key = key::<T>();
        let mut h = CLASSES.lock().unwrap_or_else(PoisonError::into_inner);
        if !h.contains_key(key) {
            //don't lookup class unless we want to, because max might try to register it which
            //could cause a loop
            let existing = if lookup_class {
                Class::<Self>::find_in_max(T::class_name(), T::class_type())
            } else {
                std::ptr::null_mut()
            };
            let max_class = if existing.is_null() {
                let mut c = creator();
                let notify = c"notify";
                let assist = c"assist";
                let inletinfo = c"inletinfo";
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
                    //hot/cold inlets
                    max_sys::class_addmethod(
                        c.inner(),
                        Some(std::mem::transmute::<
                            extern "C" fn(&Self, *mut c_void, c_long, *mut c_char),
                            MaxMethod,
                        >(Self::inletinfo_tramp)),
                        inletinfo.as_ptr(),
                        max_sys::e_max_atomtypes::A_CANT,
                        0,
                    );
                }
                //register our ft1, ft2.. in1, in2.. tramps (before the class is registered, as
                //the SDK examples mark methods undocumented)
                Self::register_ft_in(c.inner());
                c.register(T::class_type())
                    .expect(format!("failed to register {}", key).as_str());
                c.inner()
            } else {
                existing
            };

            h.insert(key, ClassMaxObjWrapper(max_class));
        }
    }

    int_float_tramps!(1, 2, 3, 4, 5, 6, 7, 8, 9);

    /// The `inletinfo` method: `*t` is 0 for a hot inlet and 1 for a cold one (as the SDK
    /// examples write it).
    extern "C" fn inletinfo_tramp(&self, _b: *mut c_void, index: c_long, t: *mut c_char) {
        self.guard_or("inletinfo", (), || {
            let Ok(index) = usize::try_from(index) else {
                return;
            };
            let hot = self.internal().inlet_hot(index);
            if !t.is_null() {
                // SAFETY: Max passes a writable `char` to `inletinfo`
                unsafe { t.write(if hot { 0 } else { 1 }) };
            }
        });
    }
}

fn key<T>() -> &'static str {
    std::any::type_name::<T>()
}

impl<T> Wrapper<max_sys::t_object, MaxWrapperInternal<T>, T>
where
    T: MaxObjWrapped,
{
    /// Register the class with Max: its field attributes, its methods, then `T::class_setup`.
    /// `#[derive(MaxObject)]` generates a `T::register()` that calls this, and `ext_main!`
    /// calls that.
    ///
    /// # Remarks
    ///
    /// Call it from `ext_main`, on Max's main thread. Internally, it locks a mutex and looks up
    /// your class by type name. If your class has already been registered it won't re-register.
    ///
    /// This will deadlock if you call `register()` again inside your `T::class_setup()`.
    ///
    /// # Panics
    /// If Max fails to register the class, or one of its attributes or methods.
    pub fn register() {
        Self::register_with(false)
    }

    /// `register`; with `lookup_class`, an existing class of the same name in Max is used
    /// instead of making one (for median's internal classes, which other externals built with
    /// the same median may have registered already).
    pub(crate) fn register_with(lookup_class: bool) {
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
            std::ptr::addr_of_mut!((*o.as_ptr()).constructed).write(AtomicBool::new(false));
            let internal = MaxWrapperInternal::<T>::new(o.max_obj(), sym.clone(), args);
            std::ptr::addr_of_mut!((*o.as_ptr()).wrapped).write(MaybeUninit::new(internal));
            (*o.as_ptr()).constructed.store(true, Ordering::Release);
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
    T: MSPObjWrapped,
{
    /// Register the class with Max: its field attributes, its methods, then `T::class_setup`.
    /// `#[derive(MaxObject)]` generates a `T::register()` that calls this, and `ext_main!`
    /// calls that.
    ///
    /// # Remarks
    ///
    /// Call it from `ext_main`, on Max's main thread. Internally, it locks a mutex and looks up
    /// your class by type name. If your class has already been registered it won't re-register.
    ///
    /// This will deadlock if you call `register()` again inside your `T::class_setup()`.
    ///
    /// # Panics
    /// If Max fails to register the class, or one of its attributes or methods.
    pub fn register() {
        Self::register_with(false)
    }

    /// `register`; with `lookup_class`, an existing class of the same name in Max is used
    /// instead of making one (for median's internal classes, which other externals built with
    /// the same median may have registered already).
    pub(crate) fn register_with(lookup_class: bool) {
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
                let dsp64 = c"dsp64";
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
                std::ptr::addr_of_mut!((*o.as_ptr()).constructed).write(AtomicBool::new(false));
                let internal = MSPWrapperInternal::<T>::new(o.msp_obj(), sym.clone(), args);
                std::ptr::addr_of_mut!((*o.as_ptr()).wrapped).write(MaybeUninit::new(internal));
                (*o.as_ptr()).constructed.store(true, Ordering::Release);
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
        let constructed = unsafe { (*this).constructed.load(Ordering::Acquire) };
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
        // SAFETY: `perform64` is only added to the DSP chain by `dsp64`, which doesn't do that
        // for an object that isn't constructed, and `free_msp` takes the object out of the chain
        // before it is torn down. This runs outside a panic guard, so it can't use the checked
        // `internal`.
        let internal = unsafe { self.internal_unchecked() };
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
            // SAFETY: `self` is this live object
            unsafe {
                crate::object::error(
                    self.max_obj(),
                    format!(
                        "{} (the object is silent until DSP is restarted)",
                        crate::ffi::panic_text("perform", &msg)
                    ),
                );
            }
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
        //called while `T::new` is running (through an `ObjRef`): don't add `perform64`, which
        //relies on the object being constructed
        if !self.constructed.load(Ordering::Acquire) {
            // SAFETY: `self` is this live object
            unsafe {
                crate::object::error(
                    self.max_obj(),
                    crate::ffi::panic_text("dsp64", "object used before construction finished"),
                )
            };
            return;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::methods::errors_for;
    use median_mock::Kernel;
    use std::sync::atomic::AtomicUsize;

    /// Counts the calls of the user's code that got through.
    static RAN: AtomicUsize = AtomicUsize::new(0);

    struct P;

    impl ObjClass for P {
        type Wrapper = MaxObjWrapper<Self>;
        fn class_name() -> &'static CStr {
            c"p"
        }
    }
    impl Methods for P {}
    impl FieldAttrs for P {}

    impl MaxObjWrapped for P {
        fn new(_builder: &mut MaxBuilder<P>) -> Self {
            P
        }
        fn handle_notification(&self, _n: &Notification) {
            RAN.fetch_add(1, Ordering::SeqCst);
        }
        fn assist(&self, _iolet: IoLet) -> Option<Cow<'_, str>> {
            RAN.fetch_add(1, Ordering::SeqCst);
            None
        }
        fn inlet_hot(&self, _index: usize) -> Option<bool> {
            RAN.fetch_add(1, Ordering::SeqCst);
            None
        }
    }

    /// The MSP twin of `P`.
    struct Q;

    impl ObjClass for Q {
        type Wrapper = MSPObjWrapper<Self>;
        fn class_name() -> &'static CStr {
            c"q"
        }
    }
    impl Methods for Q {}
    impl FieldAttrs for Q {}

    impl MSPObjWrapped for Q {
        fn new(_builder: &mut MSPBuilder<Q>) -> Self {
            Q
        }
        fn perform(&self, _: &[&[f64]], _: &mut [&mut [f64]], _: usize) {
            RAN.fetch_add(1, Ordering::SeqCst);
        }
        fn dsp_setup(&self, _: f64) {
            RAN.fetch_add(1, Ordering::SeqCst);
        }
        fn handle_notification(&self, _n: &Notification) {
            RAN.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// What Max has allocated and `T::new` is still running for: header zeroed, `wrapped`
    /// uninitialised (here zero, which is just as unusable), not constructed.
    fn half_built<O, I, T>() -> Box<Wrapper<O, I, T>> {
        // SAFETY: the header is plain data, `wrapped` is a `MaybeUninit`, `constructed` is a
        // bool (false when zeroed)
        Box::new(unsafe { MaybeUninit::zeroed().assume_init() })
    }

    /// The refusal that every trampoline posts for a half-built object of type `T`.
    fn refused_by<T>(what: &str) -> Vec<String> {
        vec![format!(
            "median: panic in {what}: {}: object used before construction finished",
            key::<T>()
        )]
    }

    /// Run `f` and return the errors that were posted for `w` meanwhile, oldest first.
    fn errors_during<W: MaxObj>(k: &Kernel, w: &W, f: impl FnOnce()) -> Vec<String> {
        let before = errors_for(k, w.max_obj()).len();
        f();
        errors_for(k, w.max_obj()).split_off(before)
    }

    /// What `errors_during` should return when only the trampoline `what` of a `P` refused.
    fn refused(what: &str) -> Vec<String> {
        refused_by::<P>(what)
    }

    #[test]
    fn typed_methods_refuse_a_half_built_object() {
        let k = median_mock::lock();
        type W = MaxObjWrapper<P>;
        let w = half_built::<_, _, P>();
        assert!(!w.constructed.load(Ordering::SeqCst));
        let m = crate::method::bang_method::<W, P, _, _>(|_: &P| {
            RAN.fetch_add(1, Ordering::SeqCst);
        });
        // SAFETY: that is the type of the trampoline `bang_method` returns
        let f = unsafe { std::mem::transmute::<crate::method::MaxMethod, extern "C" fn(&W)>(m) };
        let ran = RAN.load(Ordering::SeqCst);
        //returns normally (neutral), the user's code didn't run, the refusal was posted
        assert_eq!(errors_during(&k, &*w, || f(&w)), refused("bang"));
        assert_eq!(RAN.load(Ordering::SeqCst), ran);
    }

    #[test]
    fn other_trampolines_refuse_a_half_built_object() {
        let k = median_mock::lock();
        type W = MaxObjWrapper<P>;
        let w = half_built::<_, _, P>();
        let ran = RAN.load(Ordering::SeqCst);

        assert_eq!(errors_during(&k, &*w, || W::call_in1(&w, 3)), refused("in1"));
        assert_eq!(errors_during(&k, &*w, || W::call_ft1(&w, 3.)), refused("ft1"));
        assert_eq!(
            errors_during(&k, &*w, || {
                // SAFETY: null symbols are valid for a notification
                W::handle_notification_tramp(
                    &w,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            }),
            refused("notify")
        );
        let mut s = [0 as std::ffi::c_char; 8];
        assert_eq!(
            errors_during(&k, &*w, || W::assist_tramp(
                &w,
                std::ptr::null_mut(),
                1,
                0,
                s.as_mut_ptr()
            )),
            refused("assist")
        );
        let mut t: std::ffi::c_char = 0;
        assert_eq!(
            errors_during(&k, &*w, || W::inletinfo_tramp(&w, std::ptr::null_mut(), 1, &mut t)),
            refused("inletinfo")
        );
        assert_eq!(t, 0);
        //what the `defer_tramp` trampolines do
        assert_eq!(
            errors_during(&k, &*w, || crate::ffi::guard(&*w, "method", || {
                let _: &P = WrapperWrapped::wrapped(&*w);
                RAN.fetch_add(1, Ordering::SeqCst);
            })),
            refused("method")
        );
        //the attribute accessors (`Class::add_attr`, `#[attr]`) return an error to Max
        fn get(_: &P) -> f64 {
            RAN.fetch_add(1, Ordering::SeqCst);
            1.
        }
        fn set(_: &P, _: f64) {
            RAN.fetch_add(1, Ordering::SeqCst);
        }
        fn getter<G: Fn(&P) -> f64 + Copy>(_: G) -> crate::attr::AttrTrampGetMethod<W> {
            crate::attr::get_tramp::<W, P, G, f64>
        }
        fn setter<S: Fn(&P, f64) + Copy>(_: S) -> crate::attr::AttrTrampSetMethod<W> {
            crate::attr::set_tramp::<W, P, S, f64, ()>
        }
        let mut ac: c_long = 0;
        let mut av: *mut max_sys::t_atom = std::ptr::null_mut();
        let mut code = 0;
        assert_eq!(
            errors_during(&k, &*w, || {
                // SAFETY: `ac` and `av` are what Max passes to a getter
                code = unsafe { getter(get)(&w, std::ptr::null_mut(), &mut ac, &mut av) };
            }),
            refused("attribute getter")
        );
        assert_eq!(code, crate::ffi::ERR_GENERIC);
        assert!(av.is_null());
        let mut atom: max_sys::t_atom = crate::atom::Atom::from(2.).value;
        assert_eq!(
            errors_during(&k, &*w, || {
                // SAFETY: one valid atom, as Max passes to a setter
                code = unsafe { setter(set)(&w, std::ptr::null_mut(), 1, &mut atom) };
            }),
            refused("attribute setter")
        );
        assert_eq!(code, crate::ffi::ERR_GENERIC);
        assert_eq!(RAN.load(Ordering::SeqCst), ran);
    }

    #[test]
    fn msp_object_refuses_a_half_built_object() {
        let k = median_mock::lock();
        type W = MSPObjWrapper<Q>;
        let w = half_built::<_, _, Q>();
        let ran = RAN.load(Ordering::SeqCst);

        assert_eq!(
            errors_during(&k, &*w, || W::dsp64(
                &w,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                44100.,
                64,
                0
            )),
            //`dsp64` has a refusal of its own, because it must not add `perform64`
            ["median: panic in dsp64: object used before construction finished"]
        );
        //no perform routine was added, so `perform64` can't run on it
        assert!(k.dsp_adds().is_empty());
        assert_eq!(errors_during(&k, &*w, || W::call_in1(&w, 3)), refused_by::<Q>("in1"));
        assert_eq!(RAN.load(Ordering::SeqCst), ran);
    }
}
