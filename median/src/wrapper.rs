//! External Max and MSP object wrappers
//! TODO: Jitter

use crate::{
    atom::Atom,
    buffer::BufferRef,
    builder::{
        Builder, BuilderKind, Iolets, MSPBuilder, ManagedBufferRefInternal, Max, MaxBuilder, Msp,
    },
    class::{Class, ClassType},
    clock::ClockShared,
    defer::{Defer, DeferQueue, DeferredCall},
    error::{MaxResult, NewError},
    method::{MaxFree, MaxMethod},
    notify::Notification,
    object::{MSPObj, MaxObj, ObjBox, ObjRef},
    symbol::SymbolRef,
};

use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::{CStr, c_char, c_void},
    fmt::Display,
    marker::PhantomData,
    mem::MaybeUninit,
    os::raw::c_long,
    sync::{
        Arc, LazyLock, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

//type name -> the registered class
static CLASSES: LazyLock<Mutex<HashMap<&'static str, ClassEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The longest assist text, in bytes, that is given to Max (`ASSIST_MAX_STRING_LEN` in the SDK's
/// `ext_assist.h`). Max's assist buffers are at least 512 bytes.
pub const ASSIST_MAX_STRING_LEN: usize = 500;

pub type MaxObjWrapper<T> = Wrapper<max_sys::t_object, MaxWrapperInternal<T>, T>;
pub type MSPObjWrapper<T> = Wrapper<max_sys::t_pxobject, MSPWrapperInternal<T>, T>;

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

//we only use ClassMaxObjWrapper in CLASSES after we've registered the class, for max's usage this is
//Send
#[repr(transparent)]
struct ClassMaxObjWrapper(*mut max_sys::t_class);
unsafe impl Send for ClassMaxObjWrapper {}

/// A registered class, in `CLASSES`.
struct ClassEntry {
    class: ClassMaxObjWrapper,
    /// The class has deferred methods: each object gets a deferred-call queue.
    deferred: bool,
}

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
///
/// It runs while median holds its class table lock: registering a class (any wrapper's
/// `register()`) or creating a [`Clock`](crate::clock::Clock) in it deadlocks.
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
///
/// It runs while median holds its class table lock: registering a class (any wrapper's
/// `register()`) or creating a [`Clock`](crate::clock::Clock) in it deadlocks.
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
    ///
    /// Return `Err` to refuse to make the object (bad arguments, a missing resource): the
    /// message is posted to the Max console for the object and Max shows the box as broken.
    /// Everything made with the builder is released. `?` works on many error types, see
    /// [`NewError`].
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError>;

    /// Apply creation attributes after the object is fully constructed. Normally uses Max's
    /// `attr_args_process`; the generated Jitter glue uses `max_jit_attr_args` instead.
    #[doc(hidden)]
    fn process_creation_attrs(&self, obj: &ObjRef, args: &[Atom]) {
        unsafe {
            max_sys::attr_args_process(
                obj.as_ptr().cast(),
                args.len().min(i16::MAX as usize) as _,
                args.as_ptr().cast_mut().cast(),
            );
        }
    }

    /// Register anything else your class needs (aliases, methods with computed names, ...),
    /// after the field attributes and the methods.
    ///
    /// It runs while median holds its class table lock: registering a class (any wrapper's
    /// `register()`) or creating a [`Clock`](crate::clock::Clock) in it deadlocks.
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
    /// [`hot`](crate::inlet::Inlet::hot) are hot, unless marked [`cold`](crate::inlet::Inlet::cold). The same rules as for
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
    ///
    /// Return `Err` to refuse to make the object, as for [`MaxObjWrapped::new`].
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError>;

    /// Perform DSP.
    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], nframes: usize);

    /// Register anything else your class needs (aliases, methods with computed names, ...),
    /// after the field attributes and the methods.
    ///
    /// It runs while median holds its class table lock: registering a class (any wrapper's
    /// `register()`) or creating a [`Clock`](crate::clock::Clock) in it deadlocks.
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

    /// Give a notification to the `buffer~` references of the object (`MaxBuilder::buffer_ref`),
    /// which median's `notify` handling does before the user's. For the `notify` trampoline;
    /// the wrappers implement it, the default does nothing.
    #[doc(hidden)]
    fn notify_buffer_refs(&self, notification: &Notification) {
        let _ = notification;
    }

    /// Queue a deferred call on the object's deferred-call queue (see [`crate::defer`]). For
    /// the deferring trampolines; the wrappers implement it, the default panics.
    #[doc(hidden)]
    fn defer_call(&self, mode: Defer, call: DeferredCall<T>) {
        let _ = (mode, call);
        panic!("median: this wrapper has no deferred-call queue");
    }
}

/// The actual struct that is given to max
#[repr(C)]
pub struct Wrapper<O, I, T> {
    s_obj: O,
    // SDK-owned extension storage, located with class_obexoffset_set.
    obex: MaybeUninit<*mut c_void>,
    // Jitter's max_jit_obex is a different allocation from Max's object obex table.
    // Never register the same slot with both class_obexoffset_set and
    // max_jit_class_obex_setup: Jitter would overwrite Max's table pointer.
    jitter_obex: MaybeUninit<*mut c_void>,
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
    //the callback targets of the clocks and queues made with the builder: disabled by
    //`free_wrapped` before `wrapped` is dropped (and again in `Drop`)
    targets: Vec<Arc<ClockShared>>,
    //the pending deferred calls and their two qelems, if the class has deferred methods (the
    //qelems' targets are in `targets`)
    deferred: Option<DeferQueue<T>>,
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
    //the callback targets of the clocks and queues made with the builder: disabled by
    //`free_wrapped` before `wrapped` is dropped (and again in `Drop`)
    targets: Vec<Arc<ClockShared>>,
    //the pending deferred calls and their two qelems, if the class has deferred methods (the
    //qelems' targets are in `targets`)
    deferred: Option<DeferQueue<T>>,
}

/// Disable the clocks and queues created for an object. Must run before the object's `T` is
/// dropped (and before anything else about the object is torn down or moved): it waits for any
/// in-flight callback and makes later ones no-ops. Idempotent.
fn release_targets(targets: &[Arc<ClockShared>]) {
    for c in targets {
        c.clear();
    }
}

impl<T> Drop for MaxWrapperInternal<T> {
    fn drop(&mut self) {
        //`free_wrapped` has already done this, before it started to tear the object down; this
        //is a harmless second clear that keeps the invariant local to the type
        release_targets(&self.targets);
    }
}

impl<T> Drop for MSPWrapperInternal<T> {
    fn drop(&mut self) {
        //`free_wrapped` has already done this, before it started to tear the object down; this
        //is a harmless second clear that keeps the invariant local to the type
        release_targets(&self.targets);
    }
}

/// Sealed: `Wrapper` needs `WrapperInternal` as a bound, so the trait has to be `pub`, but it
/// lives in this private module and is not re-exported, so other crates can neither name nor
/// implement it.
mod sealed {
    use super::*;

    pub trait WrapperInternal<O, T>: Sized {
        fn wrapped(&self) -> &T;
        fn process_creation_attrs(&self, obj: &ObjRef, args: &[Atom]) {
            unsafe {
                max_sys::attr_args_process(
                    obj.as_ptr().cast(),
                    args.len().min(i16::MAX as usize) as _,
                    args.as_ptr().cast_mut().cast(),
                );
            }
        }
        /// The callback targets of the clocks and queues created for this object by the builder.
        fn targets(&self) -> &[Arc<ClockShared>];
        /// The deferred-call queue, if the class has deferred methods.
        fn defer_queue(&self) -> Option<&DeferQueue<T>>;
        /// The builder `T::new` gets.
        type Kind: BuilderKind;
        /// Run `T::new` and make the inlets: the internals, or why the object was refused.
        /// Whatever the builder made is released when the builder is dropped, or (once the
        /// caller commits it) when the object is freed.
        fn build(builder: &mut Builder<T, Self::Kind>) -> Result<Self, String>;
        fn class_setup(class: &mut Class<Wrapper<O, Self, T>>);

        fn call_float(&self, index: usize, value: f64);
        fn call_int(&self, index: usize, value: max_sys::t_atom_long);

        /// Give the notification to the object's `buffer~` references.
        fn notify_buffer_refs(&self, notification: &Notification);
        fn report_error(&self, obj: &ObjRef, err: &dyn Display);
        /// # Safety
        /// `s` must be null or point to a writable buffer of at least 512 bytes, as Max passes to
        /// an assist method.
        unsafe fn assist(&self, io: c_long, index: c_long, s: *mut c_char);
        /// Whether inlet `index` is hot.
        fn inlet_hot(&self, index: usize) -> bool;
        /// # Safety
        /// All arguments must be the live arguments supplied to Max's inletinfo callback.
        unsafe fn host_inletinfo(
            &self,
            _owner: *mut max_sys::t_object,
            _b: *mut c_void,
            _index: c_long,
            _cold: *mut c_char,
        ) -> bool {
            false
        }
    }
}
use sealed::WrapperInternal;

unsafe impl<I, T> MaxObj for Wrapper<max_sys::t_object, I, T> {}
unsafe impl<I, T> MaxObj for Wrapper<max_sys::t_pxobject, I, T> {
    const IS_MSP: bool = true;
}
unsafe impl<I, T> MSPObj for Wrapper<max_sys::t_pxobject, I, T> {}

impl<T> WrapperInternal<max_sys::t_object, T> for MaxWrapperInternal<T>
where
    T: MaxObjWrapped,
{
    fn wrapped(&self) -> &T {
        &self.wrapped
    }
    fn targets(&self) -> &[Arc<ClockShared>] {
        &self.targets
    }
    fn defer_queue(&self) -> Option<&DeferQueue<T>> {
        self.deferred.as_ref()
    }
    type Kind = Max;
    fn build(builder: &mut MaxBuilder<T>) -> Result<Self, String> {
        let wrapped = T::new(builder).map_err(|e| e.to_string())?;
        let iolets = builder.finalize()?;
        Ok(Self {
            wrapped,
            iolets,
            buffer_refs: builder.take_buffer_refs(),
            targets: builder.take_targets(),
            deferred: builder.take_defer_queue().map(DeferQueue::new),
        })
    }
    fn process_creation_attrs(&self, obj: &ObjRef, args: &[Atom]) {
        self.wrapped.process_creation_attrs(obj, args);
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
    fn notify_buffer_refs(&self, notification: &Notification) {
        handle_buffer_ref_notifications(&self.buffer_refs, notification);
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
    unsafe fn host_inletinfo(
        &self,
        owner: *mut max_sys::t_object,
        b: *mut c_void,
        index: c_long,
        cold: *mut c_char,
    ) -> bool {
        unsafe { self.iolets.host_inletinfo(owner, b, index, cold) }
    }
}

impl<T> WrapperInternal<max_sys::t_pxobject, T> for MSPWrapperInternal<T>
where
    T: MSPObjWrapped,
{
    fn wrapped(&self) -> &T {
        &self.wrapped
    }
    fn targets(&self) -> &[Arc<ClockShared>] {
        &self.targets
    }
    fn defer_queue(&self) -> Option<&DeferQueue<T>> {
        self.deferred.as_ref()
    }
    type Kind = Msp;
    fn build(builder: &mut MSPBuilder<T>) -> Result<Self, String> {
        let wrapped = T::new(builder).map_err(|e| e.to_string())?;
        let iolets = builder.finalize()?;
        #[cfg(test)]
        tests::maybe_panic_after_finalize();
        // Always forbid in-place processing: `perform` is handed shared input slices and
        // mutable output slices at the same time, which must never overlap.
        // SAFETY: the builder's object is a valid `t_pxobject` under construction
        unsafe {
            (*builder.msp_obj()).z_misc |= 1; //Z_NO_INPLACE;
        }
        Ok(Self {
            wrapped,
            panicked: AtomicBool::new(false),
            iolets,
            buffer_refs: builder.take_buffer_refs(),
            targets: builder.take_targets(),
            deferred: builder.take_defer_queue().map(DeferQueue::new),
        })
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
    fn notify_buffer_refs(&self, notification: &Notification) {
        handle_buffer_ref_notifications(&self.buffer_refs, notification);
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
        panic!(
            "{}: failed to register the field attributes: {:?}",
            key::<T>(),
            e
        );
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
                r.notify_if_unchecked(notification);
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
        let (signal_inlets, signal_outlets) =
            (self.iolets.signal_inlets, self.iolets.signal_outlets);
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
    #[allow(clippy::too_many_arguments)] // Mirrors the host DSP callback ABI.
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

        self.wrapped().perform(
            &in_slices[..ins.len()],
            &mut out_slices[..outs.len()],
            nframes,
        );
    }
}

/// The registered class for `key`, if there is one, and whether it has deferred methods.
fn registered_class(key: &'static str) -> Option<(*mut max_sys::t_class, bool)> {
    //the lock is released before the object is made, so its `new` may register classes
    //a panic during registration can poison the lock, but the map is still consistent
    let g = CLASSES.lock().unwrap_or_else(PoisonError::into_inner);
    g.get(key).map(|c| (c.class.0, c.deferred))
}

/// Why an object wasn't made.
enum CreateError {
    /// The class isn't registered, or Max didn't allocate the object.
    Unavailable,
    /// `T::new` returned `Err` (or the builder's description had a mistake); the message has
    /// been posted for the object.
    Refused(String),
}

impl<O, I, T> Wrapper<O, I, T>
where
    I: WrapperInternal<O, T>,
    Self: MaxObj + 'static,
{
    /// Allocate an object of the class registered for `T`, run `T::new` with a builder, and
    /// make the inlets.
    ///
    /// If `T::new` returns `Err` (or panics) the half-built object is freed: what the builder
    /// made is dropped (clocks, buffer references, proxies; for MSP, `z_dsp_free` if
    /// `z_dsp_setup` ran), then Max frees the object, whose `free` sees that it isn't
    /// constructed. On `Err` the message is posted for the object first.
    fn construct(sym: SymbolRef, args: &[Atom]) -> Result<ObjBox<Self>, CreateError> {
        let (max_class, deferred) = registered_class(key::<T>()).ok_or(CreateError::Unavailable)?;
        // SAFETY: `max_class` is the class registered for this wrapper type
        let o: ObjBox<Self> =
            unsafe { ObjBox::alloc(max_class) }.ok_or(CreateError::Unavailable)?;
        // SAFETY: `o` is a live, newly allocated wrapper that Max has not seen yet; its fields
        // are uninitialised, so write without dropping the old values. If `T::new` panics or
        // fails, `o` is dropped (after the builder) while `constructed` is false.
        unsafe { std::ptr::addr_of_mut!((*o.as_ptr()).constructed).write(AtomicBool::new(false)) };
        // SAFETY: the object is valid and under construction, and outlives the builder, which
        // is declared after `o` so it is dropped first; `args` outlives it too
        let mut builder: Builder<T, I::Kind> =
            unsafe { Builder::new(std::ptr::NonNull::new_unchecked(o.max_obj()), sym, args) };
        if deferred {
            //the qelems of the deferred-call queue, armed and disabled with the clocks
            builder.make_defer_queue(|obj| {
                // SAFETY: the targets are armed after construction and disabled before the
                // object is torn down, so `obj` is this live, constructed wrapper
                let w: &Self = unsafe { &*obj.cast::<Self>() };
                w.run_deferred();
            });
        }
        let internal = match I::build(&mut builder) {
            Ok(internal) => internal,
            Err(msg) => {
                // SAFETY: the object is still alive
                unsafe { crate::object::error(o.max_obj(), msg.as_str()) };
                return Err(CreateError::Refused(msg));
            }
        };
        // SAFETY: as above
        unsafe {
            std::ptr::addr_of_mut!((*o.as_ptr()).wrapped).write(MaybeUninit::new(internal));
            (*o.as_ptr()).constructed.store(true, Ordering::Release);
        }
        //from here the object's `free` releases what the builder made
        builder.commit();
        drop(builder);
        // SAFETY: `wrapped` is initialised; only now may clock and queue callbacks run
        unsafe { Self::arm_targets(o.as_ptr()) };
        // Process attributes only after construction and callback targets are ready.
        let obj = unsafe { ObjRef::from_raw(std::ptr::NonNull::new_unchecked(o.max_obj())) };
        o.internal().process_creation_attrs(&obj, args);
        Ok(o)
    }

    /// The `new` method Max calls.
    ///
    /// # Safety
    /// `sym` must be a symbol and `argv` null or valid for `argc` atoms, as Max passes them.
    unsafe fn new_tramp_common(
        sym: *mut max_sys::t_symbol,
        argc: c_long,
        argv: *const max_sys::t_atom,
    ) -> *mut c_void {
        //a panic (in `T::new` or elsewhere) is posted to the console and Max gets null, which
        //makes it report that the object couldn't be created. Unwinding drops the builder
        //(releasing what it made) and the `ObjBox`, which frees the half-built object;
        //`free_wrapped` knows not to drop the internals.
        crate::ffi::guard_global(key::<T>(), std::ptr::null_mut(), || {
            // SAFETY: the caller's contract
            let sym: SymbolRef = unsafe { SymbolRef::from_raw(sym) };
            let args: &[Atom] = if argc > 0 && !argv.is_null() {
                // SAFETY: the caller's contract; `Atom` is a transparent `t_atom`
                unsafe { std::slice::from_raw_parts(argv.cast::<Atom>(), argc as usize) }
            } else {
                &[]
            };
            match Self::construct(sym, args) {
                Ok(o) => ObjBox::into_raw(o).cast::<c_void>(),
                //posted for the object already
                Err(CreateError::Refused(_)) => std::ptr::null_mut(),
                Err(CreateError::Unavailable) => {
                    crate::max::error(format!("median: failed to create {}", key::<T>()));
                    std::ptr::null_mut()
                }
            }
        })
    }

    /// `construct`, panicking on failure.
    fn new_or_panic(sym: SymbolRef, args: &[Atom]) -> ObjBox<Self> {
        match Self::construct(sym, args) {
            Ok(o) => o,
            Err(CreateError::Unavailable) => panic!("failed to create {}", key::<T>()),
            Err(CreateError::Refused(msg)) => panic!("failed to create {}: {}", key::<T>(), msg),
        }
    }
}

impl<O, I, T> WrapperWrapped<T> for Wrapper<O, I, T>
where
    I: WrapperInternal<O, T>,
{
    fn wrapped(&self) -> &T {
        self.internal().wrapped()
    }

    fn defer_call(&self, mode: Defer, call: DeferredCall<T>) {
        match self.internal().defer_queue() {
            Some(q) => q.push(mode, call),
            None => panic!(
                "{}: a deferred method was called, but the class was registered without one",
                key::<T>()
            ),
        }
    }

    fn notify_buffer_refs(&self, notification: &Notification) {
        self.internal().notify_buffer_refs(notification);
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
    ( $( $i:literal => ($int:ident, $float:ident) ),+ ) => {
        $(
            pub extern "C" fn $int(&self, value: max_sys::t_atom_long) {
                self.guard_or(concat!("in", $i), (), || self.internal().call_int($i, value));
            }
            pub extern "C" fn $float(&self, value: f64) {
                self.guard_or(concat!("ft", $i), (), || self.internal().call_float($i, value));
            }
        )*
        fn register_ft_in(class: *mut max_sys::t_class) {
            // SAFETY: registration supplies a live class; each erased method has the signature
            // declared by its A_FLOAT/A_LONG marker and contains its own panic guard.
            unsafe {
                $(
                    let name = crate::__private::cstr(concat!("ft", $i, "\0").as_bytes());
                    max_sys::class_addmethod(class,
                        Some(std::mem::transmute::<extern "C" fn(&Self, f64), crate::method::MaxMethod>(Self::$float)),
                        name.as_ptr(), max_sys::e_max_atomtypes::A_FLOAT, 0);
                    mark_undocumented(class, name);
                    let name = crate::__private::cstr(concat!("in", $i, "\0").as_bytes());
                    max_sys::class_addmethod(class,
                        Some(std::mem::transmute::<extern "C" fn(&Self, max_sys::t_atom_long), crate::method::MaxMethod>(Self::$int)),
                        name.as_ptr(), max_sys::e_max_atomtypes::A_LONG, 0);
                    mark_undocumented(class, name);
                )*
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

    /// Run the object's pending deferred calls, in order, each inside the panic guard (called
    /// by either qelem of its deferred-call queue, on the main thread).
    ///
    /// The calls are taken off the queue one at a time and nothing is held while one runs, so a
    /// call that makes Max run the other qelem (a nested event loop, such as a modal dialog's)
    /// drains the rest of the queue there, in order, and this drain then finds it empty. At most
    /// as many calls run as were pending at the start, so a deferred method that queues itself
    /// again runs once per run of a qelem instead of looping here: its push set the other qelem
    /// (this one, still running, may stay unset: `ext_proto.h` says "A Qelem object is unset
    /// after its queue function has been called"). A call queued while both qelems were running
    /// (one nested inside the other) set neither, so a drain that leaves calls behind sets both
    /// again; the inner drain's set is lost too, but the outer one's, after the inner qelem has
    /// finished, is not.
    fn run_deferred(&self) {
        let Some(q) = self.internal().defer_queue() else {
            return;
        };
        for _ in 0..q.len() {
            let Some(call) = q.pop() else {
                break;
            };
            let what = call.what();
            self.guard_or(what, (), || {
                call.run(self.internal().wrapped(), &mut |err| {
                    WrapperWrapped::report_error(self, err)
                })
            });
        }
        q.set_if_pending();
    }

    /// How many deferred calls are pending.
    #[cfg(test)]
    pub(crate) fn pending_deferred(&self) -> usize {
        self.internal().defer_queue().map_or(0, |q| q.len())
    }

    /// Let the object's clocks and queues run. Called once the wrapped value is initialised.
    ///
    /// # Safety
    /// `this` must point to a wrapper whose `wrapped` field is initialised.
    unsafe fn arm_targets(this: *mut Self) {
        // SAFETY: guaranteed by the caller; `MaybeUninit<I>` has the layout of `I`
        let internal = unsafe { &*std::ptr::addr_of!((*this).wrapped).cast::<I>() };
        for c in internal.targets() {
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
                //(a) shut out the clocks and queues first: this waits for a callback that is
                //running (it holds `&T` into the object) and makes every later one a no-op. Only a
                //shared borrow of the internals exists meanwhile.
                {
                    let internal = &*std::ptr::addr_of!((*this).wrapped).cast::<I>();
                    release_targets(internal.targets());
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
            let (max_class, deferred) = if existing.is_null() {
                let mut c = creator();
                unsafe {
                    max_sys::class_obexoffset_set(c.inner(), std::mem::offset_of!(Self, obex) as _);
                }
                let notify = c"notify";
                let assist = c"assist";
                let inletinfo = c"inletinfo";
                unsafe {
                    // Only for a class without a `notify` method: keep the one the user added
                    // (`Class::add_notify`, which runs in `creator`, with the class' methods and
                    // `class_setup`) or the SDK's Jitter wrapper installed.
                    if max_sys::class_method(c.inner(), crate::sym!("notify").as_ptr()).is_none() {
                        //the default: only the `buffer~` references get notifications
                        max_sys::class_addmethod(
                            c.inner(),
                            Some(std::mem::transmute::<
                                extern "C" fn(
                                    &Self,
                                    *mut max_sys::t_symbol,
                                    *mut max_sys::t_symbol,
                                    *mut c_void,
                                    *mut c_void,
                                ),
                                MaxMethod,
                            >(notification_handler)),
                            notify.as_ptr(),
                            max_sys::e_max_atomtypes::A_CANT,
                            0,
                        );
                    }
                    //assist
                    max_sys::class_addmethod(
                        c.inner(),
                        Some(std::mem::transmute::<
                            extern "C" fn(&Self, *mut c_void, c_long, c_long, *mut c_char),
                            MaxMethod,
                        >(assist_tramp)),
                        assist.as_ptr(),
                        max_sys::e_max_atomtypes::A_CANT,
                        0,
                    );
                    // SDK-owned positions delegate to the saved host method; custom inlets
                    // use the same guarded table lookup as ordinary Max/MSP objects.
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
                    .unwrap_or_else(|_| panic!("failed to register {}", key));
                (c.inner(), c.has_deferred())
            } else {
                (existing, false)
            };

            h.insert(
                key,
                ClassEntry {
                    class: ClassMaxObjWrapper(max_class),
                    deferred,
                },
            );
        }
    }

    int_float_tramps!(
        1 => (call_in1, call_ft1),
        2 => (call_in2, call_ft2),
        3 => (call_in3, call_ft3),
        4 => (call_in4, call_ft4),
        5 => (call_in5, call_ft5),
        6 => (call_in6, call_ft6),
        7 => (call_in7, call_ft7),
        8 => (call_in8, call_ft8),
        9 => (call_in9, call_ft9)
    );

    /// The `inletinfo` method: `*t` is 0 for a hot inlet and 1 for a cold one (as the SDK
    /// examples write it).
    extern "C" fn inletinfo_tramp(&self, b: *mut c_void, index: c_long, t: *mut c_char) {
        self.guard_or("inletinfo", (), || {
            if !t.is_null()
                // SAFETY: s_obj is this SDK wrapper's live header; preserve all callback args.
                && unsafe { self.internal().host_inletinfo(
                    std::ptr::from_ref(&self.s_obj).cast_mut().cast(), b, index, t) }
            {
                return;
            }
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
    /// Registration holds median's class table lock while it runs `T::register_attrs`,
    /// `T::register` (the `Methods`) and `T::class_setup`. Inside any of them, registering a
    /// class (this `register()` or another wrapper's) or creating a `Clock` (which
    /// registers median's clock class) deadlocks.
    ///
    /// # Panics
    /// If Max fails to register the class, or one of its attributes or methods.
    pub(crate) fn jitter_obex_offset() -> usize {
        std::mem::offset_of!(Self, jitter_obex)
    }

    pub fn register() {
        Self::register_with(false)
    }

    /// `register`; with `lookup_class`, an existing class of the same name in Max is used
    /// instead of making one (for median's internal classes, which other externals built with
    /// the same median may have registered already).
    pub(crate) fn register_with(lookup_class: bool) {
        Self::register_common(
            lookup_class,
            Self::default_notify_tramp,
            Self::assist_tramp,
            || {
                let mut c: Class<Self> = Class::new(
                    T::class_name(),
                    Self::new_tramp,
                    Some(Self::free_wrapped as MaxFree<Self>),
                );
                //TODO somehow pass the lock so that classes can register additional classes
                MaxWrapperInternal::<T>::class_setup(&mut c);
                c
            },
        );
    }

    /// A method for Max to create an instance of your class. Null if `T::new` returned `Err`
    /// (the error is posted for the object) or panicked (the panic is posted).
    ///
    /// # Safety
    /// `sym` must be a symbol and `argv` null or valid for `argc` atoms, as Max passes them.
    pub unsafe extern "C" fn new_tramp(
        sym: *mut max_sys::t_symbol,
        argc: c_long,
        argv: *const max_sys::t_atom,
    ) -> *mut c_void {
        // SAFETY: the caller's contract
        unsafe { Self::new_tramp_common(sym, argc, argv) }
    }

    /// Create an instance of the wrapper, on the heap, with no arguments.
    ///
    /// # Panics
    /// As [`new`](Self::new).
    pub fn new_noargs() -> ObjBox<Self> {
        Self::new(SymbolRef::default(), &[])
    }

    /// Create an instance of the wrapper, on the heap.
    ///
    /// # Panics
    /// If the class isn't registered, Max fails to allocate the object, or `T::new` returns
    /// `Err` (which is also posted for the object).
    pub fn new(sym: SymbolRef, args: &[Atom]) -> ObjBox<Self> {
        Self::new_or_panic(sym, args)
    }

    /// The `notify` method of a class that has none of its own: the object's `buffer~`
    /// references only.
    extern "C" fn default_notify_tramp(
        &self,
        sender_name: *mut max_sys::t_symbol,
        message: *mut max_sys::t_symbol,
        sender: *mut c_void,
        data: *mut c_void,
    ) {
        self.guard_or("notify", (), || {
            // SAFETY: Max passes valid (or null) symbols to a notify method
            let notification = unsafe { Notification::new(sender_name, message, sender, data) };
            self.internal().notify_buffer_refs(&notification);
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
    /// Registration holds median's class table lock while it runs `T::register_attrs`,
    /// `T::register` (the `Methods`) and `T::class_setup`. Inside any of them, registering a
    /// class (this `register()` or another wrapper's) or creating a `Clock` (which
    /// registers median's clock class) deadlocks.
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
            Self::default_notify_tramp,
            Self::assist_tramp,
            || {
                let mut c: Class<Self> = Class::new(
                    T::class_name(),
                    Self::new_tramp,
                    Some(Self::free_msp as MaxFree<Self>),
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

    /// A method for Max to create an instance of your class. Null if `T::new` returned `Err`
    /// (the error is posted for the object) or panicked (the panic is posted).
    ///
    /// # Safety
    /// `sym` must be a symbol and `argv` null or valid for `argc` atoms, as Max passes them.
    pub unsafe extern "C" fn new_tramp(
        sym: *mut max_sys::t_symbol,
        argc: c_long,
        argv: *const max_sys::t_atom,
    ) -> *mut c_void {
        // SAFETY: the caller's contract
        unsafe { Self::new_tramp_common(sym, argc, argv) }
    }

    /// Create an instance of the wrapper, on the heap.
    ///
    /// # Panics
    /// If the class isn't registered, Max fails to allocate the object, or `T::new` returns
    /// `Err` (which is also posted for the object).
    pub fn new(sym: SymbolRef, args: &[Atom]) -> ObjBox<Self> {
        Self::new_or_panic(sym, args)
    }

    extern "C" fn free_msp(this: *mut Self) {
        //releasing the clocks and queues drops user closures, so keep it inside the panic guard
        crate::ffi::guard_global("free", (), || Self::free_msp_inner(this));
        Self::free_wrapped(this);
    }

    fn free_msp_inner(this: *mut Self) {
        // SAFETY: Max passes the object it is freeing
        let constructed = unsafe { (*this).constructed.load(Ordering::Acquire) };
        //shut out the clocks and queues first: a running callback holds `&W` over the whole
        //object, including the pxobject header `z_dsp_free` writes. Then free dsp (this takes
        //the object out of the dsp chain, so `perform` can't run afterwards), unless construction
        //didn't get as far as setting it up. `free_wrapped` clears them again, which is a no-op.
        //(The qelems themselves are freed when `T` is dropped, after `z_dsp_free`: `perform`
        //may set a queue until then.)
        if constructed {
            unsafe {
                let internal =
                    &*std::ptr::addr_of!((*this).wrapped).cast::<MSPWrapperInternal<T>>();
                release_targets(internal.targets());
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
            internal.perform64(
                dsp64,
                ins,
                numins,
                outs,
                numouts,
                sampleframes,
                flags,
                userparam,
            )
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

    /// The `notify` method of a class that has none of its own: the object's `buffer~`
    /// references only.
    extern "C" fn default_notify_tramp(
        &self,
        sender_name: *mut max_sys::t_symbol,
        message: *mut max_sys::t_symbol,
        sender: *mut c_void,
        data: *mut c_void,
    ) {
        self.guard_or("notify", (), || {
            // SAFETY: Max passes valid (or null) symbols to a notify method
            let notification = unsafe { Notification::new(sender_name, message, sender, data) };
            self.internal().notify_buffer_refs(&notification);
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

    /// Set by a test to make the next MSP construction panic right after the builder has made
    /// the inlets (so after `z_dsp_setup`), which no user code can do.
    static PANIC_AFTER_FINALIZE: AtomicBool = AtomicBool::new(false);

    pub(super) fn maybe_panic_after_finalize() {
        if PANIC_AFTER_FINALIZE.swap(false, Ordering::SeqCst) {
            panic!("injected after z_dsp_setup");
        }
    }

    /// An MSP class with two signal inlets, a proxy and a clock.
    struct R {
        _clock: crate::clock::Clock,
    }

    impl ObjClass for R {
        type Wrapper = MSPObjWrapper<Self>;
        fn class_name() -> &'static CStr {
            c"median.test.r~"
        }
    }
    impl Methods for R {}
    impl FieldAttrs for R {}

    impl MSPObjWrapped for R {
        fn new(b: &mut MSPBuilder<R>) -> Result<Self, NewError> {
            b.inlet(crate::inlet::Inlet::signal());
            b.inlet(crate::inlet::Inlet::signal());
            b.inlet(crate::inlet::Inlet::proxy());
            Ok(R {
                _clock: b.clock(|_: &R| {}),
            })
        }
        fn perform(&self, _: &[&[f64]], _: &mut [&mut [f64]], _: usize) {}
    }

    #[test]
    fn a_panic_after_dsp_setup_frees_dsp_and_everything_else() {
        let k = median_mock::lock();
        MSPObjWrapper::<R>::register();
        let clocks = k.live_clock_count();
        PANIC_AFTER_FINALIZE.store(true, Ordering::SeqCst);
        let obj = k.new_object("median.test.r~", &[]);
        assert!(obj.is_null());
        let log = k.take_console_log();
        assert_eq!(log.len(), 1, "{log:?}");
        assert!(
            log[0].text.contains("injected after z_dsp_setup"),
            "{log:?}"
        );
        //`z_dsp_setup` was paired with `z_dsp_free`, the proxy and the clock were freed (the
        //kernel reports an unpaired setup or a leaked proxy on the console)
        assert_eq!(k.dsp_frees().len(), 1);
        assert_eq!(k.live_object_count(), 0);
        assert_eq!(k.live_clock_count(), clocks);

        //and without the panic: the object's `free` does it, once
        let obj = k.new_object("median.test.r~", &[]);
        assert!(!obj.is_null());
        assert_eq!(k.inlets(obj).len(), 2);
        assert!(k.free_object(obj));
        assert_eq!(k.dsp_frees().len(), 2);
        assert_eq!(k.dsp_frees()[1], obj as usize);
        assert!(k.console_log().is_empty(), "{:?}", k.console_log());
        assert_eq!(k.live_clock_count(), clocks);
    }

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
        fn new(_builder: &mut MaxBuilder<P>) -> Result<Self, NewError> {
            Ok(P)
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
        fn new(_builder: &mut MSPBuilder<Q>) -> Result<Self, NewError> {
            Ok(Q)
        }
        fn perform(&self, _: &[&[f64]], _: &mut [&mut [f64]], _: usize) {
            RAN.fetch_add(1, Ordering::SeqCst);
        }
        fn dsp_setup(&self, _: f64) {
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

        assert_eq!(
            errors_during(&k, &*w, || W::call_in1(&w, 3)),
            refused("in1")
        );
        assert_eq!(
            errors_during(&k, &*w, || W::call_ft1(&w, 3.)),
            refused("ft1")
        );
        assert_eq!(
            errors_during(&k, &*w, || {
                // SAFETY: null symbols are valid for a notification
                W::default_notify_tramp(
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
            errors_during(&k, &*w, || W::inletinfo_tramp(
                &w,
                std::ptr::null_mut(),
                1,
                &mut t
            )),
            refused("inletinfo")
        );
        assert_eq!(t, 0);
        //the deferring trampolines, queuing (`Low`) and running at once (`Front` on the main
        //thread): neither queues nor runs anything
        for mode in [Defer::Low, Defer::Front] {
            let m = crate::method::bang_deferred_method::<W, P, _, _>(
                |_: &P| {
                    RAN.fetch_add(1, Ordering::SeqCst);
                },
                mode,
            );
            // SAFETY: that is the type of the trampoline `bang_deferred_method` returns
            let f =
                unsafe { std::mem::transmute::<crate::method::MaxMethod, extern "C" fn(&W)>(m) };
            assert_eq!(
                errors_during(&k, &*w, || f(&w)),
                refused("bang"),
                "{mode:?}"
            );
        }
        assert_eq!(k.main_queue_len(), 0);
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
        assert_eq!(
            errors_during(&k, &*w, || W::call_in1(&w, 3)),
            refused_by::<Q>("in1")
        );
        assert_eq!(RAN.load(Ordering::SeqCst), ran);
    }
}
