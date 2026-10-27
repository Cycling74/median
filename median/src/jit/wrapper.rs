//! Shared Jitter construction and Max forwarding. The user value lives in the Jitter object;
//! the Max object uses median's existing constructor, panic guards and deferred-call queue.
//!
//! Gimmeback methods (`A_GIMMEBACK`, see `crate::gimmeback`) have two homes on a Jitter type:
//! `#[gimmeback]` registers on the Max wrapper (typed callers only), while
//! `Class::<Core<..>>::add_gimmeback` in `class_setup` registers on the Jitter class and makes
//! the Max class pass `MAX_JIT_CLASS_FLAGS_GIMMEBACK_WRAP` to `max_jit_class_wrap_standard`, so a
//! patcher message gets the reply out dumpout. How the wrap renders the reply (an atomarray as a
//! list) is assumed, pending the host check in `doc/plan-gimmeback.md`.

use super::{
    JitResult,
    matrix::{IOCount, MOP, Matrix},
};
use crate::{
    atom::Atom,
    builder::{InletInfoFn, Max, MaxBuilder},
    class::{Class, ClassType},
    defer::{Defer, DeferredCall},
    error::{MaxError, MaxResult, NewError},
    inlet::{Inlet, InletId},
    object::{MaxObj, ObjRef},
    outlet::{Outlet, OutletDesc, OutletFor},
    symbol::SymbolRef,
    wrapper::{FieldAttrs, IoLet, MaxObjWrapped, MaxObjWrapper, Methods, ObjClass, WrapperWrapped},
};
use core::ffi::{c_char, c_long, c_void};
use max_sys::{self as sys, t_jit_error_code as err};
use std::{
    any::TypeId,
    borrow::Cow,
    collections::HashMap,
    ffi::{CStr, CString},
    fmt::Display,
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::NonNull,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// A Jitter object handle. Like `ObjRef`, it is not cloneable or sendable. Keep it in its owner;
/// deliberately leaking it past the owner's deletion is unsupported. During construction, do
/// not invoke methods that require the finished user value.
pub struct JitObjRef(ObjRef);
impl JitObjRef {
    pub fn as_ptr(&self) -> *mut sys::t_object {
        self.0.as_ptr()
    }
    pub fn post(&self, text: impl Display) {
        self.0.post(text);
    }
    pub fn error(&self, text: impl Display) {
        self.0.error(text);
    }
}

/// Construction context for a Jitter value. MOP/ob3d creates the matrix iolets and dumpout;
/// they must not also be declared as ordinary Max iolets. Use `JitBuilder<Self>` in `new`.
/// Custom iolets follow the SDK iolets, including dumpout, and are described left to right.
/// Numeric callbacks receive `&Self`; proxy messages use the regular forwarded methods and
/// the Max handle's `proxy_inlet()` reports the final position. Extra iolets require a Max
/// wrapper and fixed matrix counts; standalone Jitter cores cannot create Max iolets.
pub struct JitBuilder<'a, T> {
    host: Option<&'a mut MaxBuilder<T>>,
    fixed_iolets: bool,
    obj: Option<JitObjRef>,
    destination: SymbolRef,
}
impl<T> JitBuilder<'_, T> {
    fn iolets(&mut self) -> Result<&mut MaxBuilder<T>, NewError> {
        if !self.fixed_iolets {
            return Err(
                "extra Jitter iolets require a Max wrapper with fixed matrix counts".into(),
            );
        }
        self.host
            .as_deref_mut()
            .ok_or_else(|| "this Jitter object has no Max wrapper".into())
    }
    /// Add a non-signal inlet after the SDK inlets. The returned id is its final position.
    /// `Inlet::default()` is reserved for the SDK; a variable-count MOP cannot add extra iolets.
    pub fn inlet(&mut self, inlet: Inlet<T>) -> Result<InletId, NewError> {
        Ok(self.iolets()?.inlet(inlet))
    }
    /// Append a message outlet after the SDK matrix outlets and dumpout.
    pub fn outlet<O: OutletFor<Max>>(
        &mut self,
        outlet: OutletDesc<O>,
    ) -> Result<Outlet<O>, NewError> {
        Ok(self.iolets()?.outlet(outlet))
    }
    /// A handle to the Max forwarding object, for sending messages or reading `proxy_inlet`.
    /// Unavailable when the core is constructed directly through `jit_object_new`.
    pub fn max_obj(&mut self) -> Result<ObjRef, NewError> {
        self.host
            .as_deref_mut()
            .map(|b| b.obj())
            .ok_or_else(|| "this Jitter object has no Max wrapper".into())
    }
    /// The Max creation arguments, or an empty slice for a standalone Jitter core.
    pub fn creation_args(&self) -> &[Atom] {
        self.host.as_deref().map_or(&[], |b| b.creation_args())
    }
    /// Take the object's handle once, for storage in the user value.
    pub fn obj(&mut self) -> JitObjRef {
        self.obj.take().expect("Jitter object handle already taken")
    }
    /// The GL destination, or the empty symbol for a matrix operator.
    pub fn destination(&self) -> SymbolRef {
        self.destination
    }
}

/// A matrix operator. `calc` may run concurrently: use atomic/locked state, and keep borrowed
/// matrices inside the call. Data references require the matrix view's unsafe access contract.
///
/// `inputs` and `outputs` can hold the same matrix (in-place calculation). Guards from
/// `Matrix::lock` on one matrix must be dropped last in, first out, or the matrix stays locked
/// after `calc` (see [`MatrixGuard`](crate::jit::matrix::MatrixGuard)); the reverse-declaration
/// drop order of locals does this unless a guard is dropped explicitly.
pub trait MatrixOp: ObjClass + Methods + FieldAttrs + Sync + 'static {
    fn new(builder: &mut JitBuilder<Self>) -> Result<Self, NewError>;
    fn io_count() -> IOCount;
    fn mop_setup(_mop: &mut MOP<'_>) -> JitResult<()> {
        Ok(())
    }
    fn class_setup(_class: &mut Class<Core<Self, MatrixKind>>) -> MaxResult<()> {
        Ok(())
    }
    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()>;
}

/// Proof that the host is calling a GL callback on this thread. It cannot be constructed,
/// cloned, or moved out of the callback by safe code. Do not close the context or delete the
/// owner from inside the callback. `Drop` on an object has no such context guarantee.
pub struct DrawContext {
    owner: NonNull<sys::t_jit_object>,
    _thread: PhantomData<*mut ()>,
}
impl DrawContext {
    fn current(owner: &impl MaxObj) -> Self {
        Self {
            owner: NonNull::new(owner.max_obj()).expect("live ob3d wrapper"),
            _thread: PhantomData,
        }
    }
    pub(crate) fn ob3d(&self) -> *mut c_void {
        unsafe { sys::jit_ob3d_get(self.owner.as_ptr().cast()) }
    }
}

/// A Jitter ob3d. OpenGL operations belong in `draw`, with the host's current context.
/// Release context-specific resources in `dest_closing`; `Drop` may run without a GL context.
pub trait Ob3d: ObjClass + Methods + FieldAttrs + Sync + 'static {
    fn new(builder: &mut JitBuilder<Self>) -> Result<Self, NewError>;
    fn flags() -> sys::t_jit_ob3d_flags::Type {
        0
    }
    fn class_setup(_class: &mut Class<Core<Self, GlKind>>) -> MaxResult<()> {
        Ok(())
    }
    fn draw(&self, context: &DrawContext) -> JitResult<()>;
    fn dest_changed(&self, _context: &DrawContext) -> JitResult<()> {
        Ok(())
    }
    fn dest_closing(&self, _context: &DrawContext) -> JitResult<()> {
        Ok(())
    }
}

#[doc(hidden)]
pub enum MatrixKind {}
#[doc(hidden)]
pub enum GlKind {}

/// The low-level lifecycle implemented by the built-in matrix/ob3d adapters.
///
/// # Safety
/// Implementations must match the SDK's class, object, MOP/ob3d and callback ownership contracts.
/// Pointer arguments are live instances of the matching registered classes. All new callbacks
/// must contain panics. Ordinary externals should implement `MatrixOp` or `Ob3d` instead.
#[doc(hidden)]
pub unsafe trait JitterObject<K>: ObjClass + Methods + FieldAttrs + Sync + 'static {
    fn make(b: &mut JitBuilder<Self>) -> Result<Self, NewError>;
    fn destination(_args: &[Atom]) -> SymbolRef {
        SymbolRef::default()
    }
    unsafe fn notify(_obj: *mut sys::t_object, _notification: &crate::notify::Notification) {}
    /// Return the SDK's physical inlet/outlet counts, including inlet 0 and dumpout, or None
    /// for a variable layout. They must match the subsequent `setup_max` implementation.
    unsafe fn setup_class(c: &mut Class<Core<Self, K>>) -> MaxResult<Option<(usize, usize)>>;
    unsafe fn setup_core(_p: *mut Core<Self, K>, _dest: SymbolRef) -> Result<(), NewError> {
        Ok(())
    }
    unsafe fn free_core(_p: *mut Core<Self, K>) {}
    /// `gimmeback`: the Jitter class has a method registered with `Class::add_gimmeback`
    /// (see `wrap_flags`).
    unsafe fn setup_max_class(
        c: *mut sys::t_class,
        jit: *mut sys::t_class,
        gimmeback: bool,
    ) -> MaxResult<()>;
    unsafe fn setup_max(
        obj: *mut sys::t_object,
        jit: *mut sys::t_object,
        args: &[Atom],
    ) -> Result<(), NewError>;
    unsafe fn free_max(obj: *mut sys::t_object);
    unsafe fn assist(obj: *mut sys::t_object, io: c_long, index: c_long, text: *mut c_char);
}

/// The Jitter-side wrapper. The user value is boxed to honor any Rust alignment requirement.
#[repr(C)]
pub struct Core<T, K> {
    header: sys::t_jit_object,
    ob3d: MaybeUninit<*mut c_void>,
    value: *mut T,
    ready: AtomicBool,
    ob3d_ready: bool,
    _kind: PhantomData<K>,
}
// SAFETY: only the SDK wrapper starts with a Jitter/Max object header; no impl exists for T.
unsafe impl<T, K> MaxObj for Core<T, K> {
    const IS_JITTER: bool = true;
}
impl<T, K> WrapperWrapped<T> for Core<T, K> {
    fn wrapped(&self) -> &T {
        assert!(
            self.ready.load(Ordering::Acquire),
            "Jitter object used before construction finished"
        );
        unsafe { &*self.value }
    }
    fn report_error(&self, error: &dyn Display) {
        unsafe {
            crate::object::error(self.max_obj(), error.to_string());
        }
    }
}

/// `MAX_JIT_CLASS_FLAGS_GIMMEBACK_WRAP` (`jit.max.h`: "uses standard dumpout `A_DEFER_LOW`
/// method"). The header's anonymous enum is not in the generated bindings.
const GIMMEBACK_WRAP: c_long = 0x0000_0001;

/// The `flags` for `max_jit_class_wrap_standard`. With `gimmeback` (the Jitter class has a method
/// registered through `Class::add_gimmeback`) they ask for the SDK's gimmeback wrap, which per
/// `jit.max.h` "uses standard dumpout `A_DEFER_LOW` method": a patcher message to such a method
/// calls it and the reply is sent out the dumpout outlet. The header does not say how the wrap
/// renders the reply; median assumes an atomarray goes out as a list and a scalar as one atom, and
/// that the caller side frees an object reply. That is unverified in a host
/// (`doc/plan-gimmeback.md`, "Host checks"). Without a gimmeback method the flags stay `0`.
fn wrap_flags(gimmeback: bool) -> c_long {
    if gimmeback { GIMMEBACK_WRAP } else { 0 }
}

#[derive(Clone, Copy)]
struct Entry {
    class: usize,
    /// The Jitter class has a gimmeback method (`class_setup` ran before the Max class is made).
    gimmeback: bool,
    iolets: Option<(usize, usize)>,
    inletinfo: Option<InletInfoFn>,
}
static CLASSES: LazyLock<Mutex<HashMap<TypeId, Entry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
fn entry<T: 'static, K: 'static>() -> Option<Entry> {
    CLASSES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&TypeId::of::<(T, K)>())
        .copied()
}
fn max_error(code: sys::t_jit_err) -> MaxResult<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(MaxError::Generic)
    }
}
fn jit_code(result: JitResult<()>) -> sys::t_jit_err {
    result.err().unwrap_or(0) as _
}

struct PendingClass(*mut c_void);
impl Drop for PendingClass {
    fn drop(&mut self) {
        unsafe {
            sys::jit_class_free(self.0);
        }
    }
}
struct PendingAdornment(*mut c_void);
impl Drop for PendingAdornment {
    fn drop(&mut self) {
        unsafe {
            sys::jit_object_free(self.0);
        }
    }
}

impl<T, K> Core<T, K>
where
    T: JitterObject<K>,
    K: 'static,
{
    unsafe extern "C" fn new_tramp(dest: *mut sys::t_symbol) -> *mut c_void {
        Self::create(dest, None, false)
    }
    fn create(
        dest: *mut sys::t_symbol,
        host: Option<&mut MaxBuilder<T>>,
        fixed_iolets: bool,
    ) -> *mut c_void {
        crate::ffi::guard_global("Jitter new", std::ptr::null_mut(), || {
            let Some(class) = entry::<T, K>() else {
                return std::ptr::null_mut();
            };
            let p = unsafe { sys::jit_object_alloc(class.class as _) }.cast::<Self>();
            if p.is_null() {
                return std::ptr::null_mut();
            }
            unsafe {
                std::ptr::addr_of_mut!((*p).value).write(std::ptr::null_mut());
                std::ptr::addr_of_mut!((*p).ready).write(AtomicBool::new(false));
                std::ptr::addr_of_mut!((*p).ob3d).write(MaybeUninit::new(std::ptr::null_mut()));
                std::ptr::addr_of_mut!((*p).ob3d_ready).write(false);
            }
            struct Pending(*mut c_void);
            impl Drop for Pending {
                fn drop(&mut self) {
                    unsafe {
                        sys::jit_object_free(self.0);
                    }
                }
            }
            let pending = Pending(p.cast());
            let destination = unsafe { SymbolRef::from_raw(dest) };
            if let Err(e) = unsafe { T::setup_core(p, destination) } {
                unsafe {
                    crate::object::error(p.cast(), e.to_string());
                }
                return std::ptr::null_mut();
            }
            let mut builder = JitBuilder {
                host,
                fixed_iolets,
                obj: Some(JitObjRef(unsafe {
                    ObjRef::from_raw(NonNull::new_unchecked(p.cast()))
                })),
                destination,
            };
            let value = match T::make(&mut builder) {
                Ok(value) => value,
                Err(e) => {
                    unsafe {
                        crate::object::error(p.cast(), e.to_string());
                    }
                    return std::ptr::null_mut();
                }
            };
            unsafe {
                (*p).value = Box::into_raw(Box::new(value));
                (*p).ready.store(true, Ordering::Release);
            }
            std::mem::forget(pending);
            p.cast()
        })
    }
    unsafe extern "C" fn free(p: *mut Self) {
        if p.is_null() {
            return;
        }
        // Keep ob3d teardown outside the user Drop guard so it runs even if Drop panics.
        crate::ffi::guard_global("Jitter free", (), || unsafe {
            if (*p).ready.swap(false, Ordering::AcqRel) {
                drop(Box::from_raw((*p).value));
            }
        });
        crate::ffi::guard_global("Jitter context free", (), || unsafe {
            T::free_core(p);
        });
    }
}

/// Max-side glue, automatically selected by `#[max(kind = matrix_op)]` / `kind = gl`.
pub struct MaxGlue<T: JitterObject<K>, K: 'static> {
    obj: ObjRef,
    jit: NonNull<Core<T, K>>,
    max_setup_started: bool,
    iolets: Option<(usize, usize)>,
}
// SAFETY: the pointer is immutable while the Max wrapper lives; its Jitter value is Sync and
// the SDK serializes destruction after callbacks. It is freed only by this wrapper's Drop.
unsafe impl<T: JitterObject<K>, K: 'static> Sync for MaxGlue<T, K> {}
impl<T: JitterObject<K>, K: 'static> ObjClass for MaxGlue<T, K> {
    type Wrapper = MaxObjWrapper<Self>;
    fn class_name() -> &'static CStr {
        T::class_name()
    }
    fn class_type() -> ClassType {
        T::class_type()
    }
    fn description() -> Option<&'static str> {
        T::description()
    }
    fn author() -> Option<&'static str> {
        T::author()
    }
    fn tags() -> &'static [&'static str] {
        T::tags()
    }
}
impl<T: JitterObject<K>, K: 'static> FieldAttrs for MaxGlue<T, K> {}

// The transparent adapter lets existing generic message trampolines reach T through the live
// Max wrapper. It forwards deferred calls into that wrapper's existing, owner-bound queue.
#[repr(transparent)]
struct Forward<W, T, K>(W, PhantomData<(T, K)>);
unsafe impl<W: MaxObj, T, K> MaxObj for Forward<W, T, K> {
    // `notify` is installed on the Max class before the user's methods are registered
    const SUPPORTS_NOTIFY: bool = false;
    fn max_obj(&self) -> *mut sys::t_object {
        self.0.max_obj()
    }
}
impl<W, T, K> WrapperWrapped<T> for Forward<W, T, K>
where
    W: WrapperWrapped<MaxGlue<T, K>> + MaxObj,
    T: JitterObject<K>,
    K: 'static,
{
    fn wrapped(&self) -> &T {
        unsafe { self.0.wrapped().jit.as_ref() }.wrapped()
    }
    fn report_error(&self, error: &dyn Display) {
        self.0.report_error(error);
    }
    fn defer_call(&self, mode: Defer, call: DeferredCall<T>) {
        let what = call.what();
        self.0.defer_call(
            mode,
            DeferredCall::new(what, move |glue: &MaxGlue<T, K>, report| {
                call.run(unsafe { glue.jit.as_ref() }.wrapped(), report);
            }),
        );
    }
}
/// The `notify` method of the glue of a Jitter class: `T::notify` on the Max wrapper's object.
fn forward_notify<T: JitterObject<K>, K: 'static>(
    glue: &MaxGlue<T, K>,
    notification: &crate::notify::Notification,
) {
    // SAFETY: `glue.obj` is the live Max object that is being notified
    unsafe { T::notify(glue.obj.as_ptr(), notification) };
}

impl<T: JitterObject<K>, K: 'static> Methods for MaxGlue<T, K> {
    fn register<W>(class: &mut Class<W>) -> MaxResult<()>
    where
        W: WrapperWrapped<Self> + MaxObj,
    {
        let jit = entry::<T, K>().ok_or(MaxError::InvalidPtr)?;
        unsafe {
            sys::max_jit_class_obex_setup(
                class.inner(),
                MaxObjWrapper::<Self>::jitter_obex_offset() as c_long,
            );
            T::setup_max_class(class.inner(), jit.class as _, jit.gimmeback)?;
            // Notifications go to `T::notify`, unless the SDK's wrapper installed its handler.
            if sys::class_method(class.inner(), crate::sym!("notify").as_ptr()).is_none() {
                class.add_notify(forward_notify::<T, K>)?;
            }
            let inletinfo = sys::class_method(class.inner(), crate::sym!("inletinfo").as_ptr())
                .map(|f| {
                    std::mem::transmute::<
                        sys::method,
                        unsafe extern "C" fn(*mut sys::t_object, *mut c_void, c_long, *mut c_char),
                    >(Some(f))
                });
            CLASSES
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_mut(&TypeId::of::<(T, K)>())
                .unwrap()
                .inletinfo = inletinfo;
        }
        let mut forwarded: Class<Forward<W, T, K>> =
            unsafe { Class::new_registered(class.inner()) };
        forwarded.messages_only = true;
        <T as Methods>::register(&mut forwarded)?;
        class.set_deferred(forwarded.has_deferred());
        Ok(())
    }
}
impl<T: JitterObject<K>, K: 'static> MaxObjWrapped for MaxGlue<T, K> {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let class = entry::<T, K>().ok_or_else(|| NewError::from("Jitter class is unavailable"))?;
        let destination = T::destination(b.creation_args());
        // Custom outlets append now; SDK outlets are later inserted to their left. Custom
        // inlets are finalized before SDK setup for the same ordering on the inlet side.
        let mut host = unsafe {
            MaxBuilder::<T>::new(
                NonNull::new_unchecked(b.max_obj()),
                b.creation_symbol(),
                b.creation_args(),
            )
        };
        if let Some((inputs, outputs)) = class.iolets {
            host.reserve_host_iolets(inputs, outputs);
        }
        let ptr = Core::<T, K>::create(
            destination.as_ptr(),
            Some(&mut host),
            class.iolets.is_some(),
        )
        .cast::<Core<T, K>>();
        let jit =
            NonNull::new(ptr).ok_or_else(|| NewError::from("unable to create Jitter object"))?;
        let mut glue = Self {
            obj: b.obj(),
            jit,
            max_setup_started: false,
            iolets: class.iolets,
        };
        unsafe {
            sys::max_jit_object_wrap_complete(glue.obj.as_ptr(), jit.as_ptr().cast(), 0);
            // wrap_complete initializes Jitter's wrapper storage but leaves jitob null.
            sys::max_jit_obex_jitob_set(glue.obj.as_ptr().cast(), jit.as_ptr().cast());
        }
        let mut iolets = host.finalize().map_err(NewError::from)?;
        iolets.set_host_inletinfo(class.iolets.map(|(inputs, _)| inputs), class.inletinfo);
        b.use_prebuilt_iolets(iolets.map(|glue: &Self| unsafe { glue.jit.as_ref().wrapped() }));
        glue.max_setup_started = true;
        unsafe {
            T::setup_max(glue.obj.as_ptr(), jit.as_ptr().cast(), b.creation_args())?;
        }
        Ok(glue)
    }
    fn process_creation_attrs(&self, _obj: &ObjRef, args: &[Atom]) {
        unsafe {
            sys::max_jit_attr_args(
                self.obj.as_ptr().cast(),
                args.len().min(i16::MAX as usize) as _,
                args.as_ptr().cast_mut().cast(),
            );
        }
    }
    fn assist(&self, io: IoLet) -> Option<Cow<'_, str>> {
        let (kind, index) = match io {
            IoLet::Inlet(i) => (1, i),
            IoLet::Outlet(i) => (2, i),
        };
        if self
            .iolets
            .is_some_and(|(ins, outs)| index >= if kind == 1 { ins } else { outs })
        {
            return None;
        }
        let mut text = [0 as c_char; 512];
        unsafe {
            T::assist(self.obj.as_ptr(), kind, index as c_long, text.as_mut_ptr());
        }
        text[511] = 0;
        Some(Cow::Owned(
            unsafe { CStr::from_ptr(text.as_ptr()) }
                .to_string_lossy()
                .into_owned(),
        ))
    }
}

impl<T: JitterObject<K>, K: 'static> Drop for MaxGlue<T, K> {
    fn drop(&mut self) {
        unsafe {
            if self.max_setup_started {
                T::free_max(self.obj.as_ptr());
            }
            sys::jit_object_free(self.jit.as_ptr().cast());
            sys::max_jit_object_free(self.obj.as_ptr().cast());
        }
    }
}

/// Register the Jitter class first, then the Max class. Called by the derive, also usable by hand.
pub fn register_matrix<T: MatrixOp>() {
    register::<T, MatrixKind>();
}
pub fn register_gl<T: Ob3d>() {
    register::<T, GlKind>();
}
fn register<T: JitterObject<K>, K: 'static>() {
    crate::ffi::guard_ext_main(T::class_name().to_string_lossy().as_ref(), || {
        if entry::<T, K>().is_none() {
            let mut name = b"median_".to_vec();
            name.extend_from_slice(T::class_name().to_bytes());
            let name = CString::new(name).expect("class name contains no NUL");
            let c = unsafe {
                sys::jit_class_new(
                    name.as_ptr(),
                    Some(crate::method::erase(
                        Core::<T, K>::new_tramp
                            as unsafe extern "C" fn(*mut sys::t_symbol) -> *mut c_void,
                    )),
                    Some(crate::method::erase(
                        Core::<T, K>::free as unsafe extern "C" fn(*mut Core<T, K>),
                    )),
                    std::mem::size_of::<Core<T, K>>() as c_long,
                    sys::e_max_atomtypes::A_DEFSYM as c_long,
                    0 as c_long,
                )
            };
            if c.is_null() {
                crate::max::error("unable to create Jitter class");
                return;
            }
            let pending = PendingClass(c);
            let mut class: Class<Core<T, K>> = unsafe { Class::new_registered(c.cast()) };
            class.jitter = true;
            let mut iolets = None;
            let result = (|| {
                T::register_attrs(&mut class)?;
                class.attributes_only = true;
                <T as Methods>::register(&mut class)?;
                class.attributes_only = false;
                class.clear_deferred();
                unsafe {
                    iolets = T::setup_class(&mut class)?;
                    max_error(sys::jit_class_register(c))
                }
            })();
            if let Err(e) = result {
                crate::max::error(format!("Jitter class setup failed: {e:?}"));
                return;
            }
            std::mem::forget(pending);
            CLASSES.lock().unwrap_or_else(|e| e.into_inner()).insert(
                TypeId::of::<(T, K)>(),
                Entry {
                    class: c as usize,
                    gimmeback: class.has_gimmeback(),
                    iolets,
                    inletinfo: None,
                },
            );
        }
        MaxObjWrapper::<MaxGlue<T, K>>::register();
    });
}

unsafe impl<T: MatrixOp> JitterObject<MatrixKind> for T {
    unsafe fn notify(obj: *mut sys::t_object, n: &crate::notify::Notification) {
        unsafe {
            sys::max_jit_mop_notify(obj.cast(), n.sender_name().as_ptr(), n.message().as_ptr());
        }
    }
    fn make(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        T::new(b)
    }
    unsafe fn setup_class(
        class: &mut Class<Core<Self, MatrixKind>>,
    ) -> MaxResult<Option<(usize, usize)>> {
        let counts = T::io_count();
        let inputs = counts.inputs.raw().map_err(|_| MaxError::Generic)?;
        let outputs = counts.outputs.raw().map_err(|_| MaxError::Generic)?;
        let p = unsafe { sys::jit_object_new(crate::sym!("jit_mop").as_ptr(), inputs, outputs) };
        let mut mop = unsafe { MOP::from_raw(p) }.map_err(|_| MaxError::OutOfMem)?;
        let pending = PendingAdornment(p);
        T::mop_setup(&mut mop).map_err(|_| MaxError::Generic)?;
        unsafe {
            max_error(sys::jit_class_addadornment(class.inner().cast(), p.cast()))?;
            std::mem::forget(pending);
            max_error(sys::jit_class_addmethod(
                class.inner().cast(),
                Some(crate::method::erase(
                    matrix_calc::<T>
                        as unsafe extern "C" fn(
                            &Core<T, MatrixKind>,
                            *mut c_void,
                            *mut c_void,
                        ) -> sys::t_jit_err,
                )),
                c"matrix_calc".as_ptr(),
                sys::e_max_atomtypes::A_CANT as c_long,
                0 as c_long,
            ))?;
        }
        T::class_setup(class)?;
        Ok(if inputs >= 0 && outputs >= 0 {
            Some((inputs.max(1) as usize, outputs as usize + 1))
        } else {
            None
        })
    }
    unsafe fn setup_max_class(
        c: *mut sys::t_class,
        jit: *mut sys::t_class,
        gimmeback: bool,
    ) -> MaxResult<()> {
        unsafe {
            max_error(sys::max_jit_class_mop_wrap(c, jit, 0))?;
            sys::max_jit_class_wrap_standard(c, jit, wrap_flags(gimmeback));
            Ok(())
        }
    }
    unsafe fn setup_max(
        obj: *mut sys::t_object,
        jit: *mut sys::t_object,
        args: &[Atom],
    ) -> Result<(), NewError> {
        unsafe {
            max_error(sys::max_jit_mop_setup_simple(
                obj.cast(),
                jit.cast(),
                args.len() as c_long,
                args.as_ptr().cast_mut().cast(),
            ))
            .map_err(NewError::from)
        }
    }
    unsafe fn free_max(obj: *mut sys::t_object) {
        unsafe {
            sys::max_jit_mop_free(obj.cast());
        }
    }
    unsafe fn assist(obj: *mut sys::t_object, io: c_long, index: c_long, text: *mut c_char) {
        unsafe {
            sys::max_jit_mop_assist(obj.cast(), std::ptr::null_mut(), io, index, text);
        }
    }
}

unsafe extern "C" fn matrix_calc<T: MatrixOp>(
    w: &Core<T, MatrixKind>,
    inputs: *mut c_void,
    outputs: *mut c_void,
) -> sys::t_jit_err {
    crate::ffi::guard_or(w, "matrix_calc", err::JIT_ERR_GENERIC as _, || {
        // Per-call stack storage; no shared mutable vectors and no assumption of nonzero iolets.
        let mut ins: [MaybeUninit<Matrix<'_>>; 32] = std::array::from_fn(|_| MaybeUninit::uninit());
        let mut outs: [MaybeUninit<Matrix<'_>>; 32] =
            std::array::from_fn(|_| MaybeUninit::uninit());
        unsafe {
            let mut run = || -> JitResult<()> {
                let ni = fill(inputs, &mut ins)?;
                let no = fill(outputs, &mut outs)?;
                w.wrapped().calc(
                    std::slice::from_raw_parts_mut(ins.as_mut_ptr().cast(), ni),
                    std::slice::from_raw_parts_mut(outs.as_mut_ptr().cast(), no),
                )
            };
            jit_code(run())
        }
    })
}
unsafe fn fill<'a>(
    list: *mut c_void,
    data: &mut [MaybeUninit<Matrix<'a>>; 32],
) -> JitResult<usize> {
    if list.is_null() {
        return Ok(0);
    }
    let n = unsafe { sys::jit_object_method(list, crate::sym!("getsize").as_ptr()) } as isize;
    if !(0..=32).contains(&n) {
        return Err(err::JIT_ERR_OUT_OF_BOUNDS);
    }
    for (i, slot) in data.iter_mut().enumerate().take(n as usize) {
        let p =
            unsafe { sys::jit_object_method(list, crate::sym!("getindex").as_ptr(), i as c_long) };
        slot.write(unsafe { Matrix::from_raw(p)? });
    }
    Ok(n as usize)
}

unsafe impl<T: Ob3d> JitterObject<GlKind> for T {
    fn destination(args: &[Atom]) -> SymbolRef {
        let s = args.first().map(Atom::get_symbol).unwrap_or_default();
        if s.name().to_bytes().starts_with(b"@") {
            SymbolRef::default()
        } else {
            s
        }
    }
    fn make(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        T::new(b)
    }
    unsafe fn setup_class(c: &mut Class<Core<T, GlKind>>) -> MaxResult<Option<(usize, usize)>> {
        unsafe {
            if sys::jit_ob3d_setup(
                c.inner().cast(),
                std::mem::offset_of!(Core<T, GlKind>, ob3d) as c_long,
                T::flags() as c_long,
            )
            .is_null()
            {
                return Err(MaxError::OutOfMem);
            }
            for (name, method) in [
                (
                    c"ob3d_draw",
                    gl_draw::<T> as extern "C" fn(&Core<T, GlKind>) -> sys::t_jit_err,
                ),
                (c"dest_changed", gl_changed::<T>),
                (c"dest_closing", gl_closing::<T>),
            ] {
                max_error(sys::jit_class_addmethod(
                    c.inner().cast(),
                    Some(crate::method::erase(method)),
                    name.as_ptr(),
                    sys::e_max_atomtypes::A_CANT as c_long,
                    0 as c_long,
                ))?;
            }
            max_error(sys::jit_class_addmethod(
                c.inner().cast(),
                Some(crate::method::erase(
                    sys::jit_object_register
                        as unsafe extern "C" fn(*mut c_void, *mut sys::t_symbol) -> *mut c_void,
                )),
                c"register".as_ptr(),
                sys::e_max_atomtypes::A_CANT as c_long,
                0 as c_long,
            ))?;
            T::class_setup(c)?;
            Ok(Some((1, 2)))
        }
    }
    unsafe fn setup_core(p: *mut Core<T, GlKind>, dest: SymbolRef) -> Result<(), NewError> {
        unsafe {
            if sys::jit_ob3d_new(p.cast(), dest.as_ptr()).is_null() {
                return Err("unable to create ob3d".into());
            }
            (*p).ob3d_ready = true;
            Ok(())
        }
    }
    unsafe fn free_core(p: *mut Core<T, GlKind>) {
        unsafe {
            if (*p).ob3d_ready {
                sys::jit_ob3d_free(p.cast());
            }
        }
    }
    unsafe fn setup_max_class(
        c: *mut sys::t_class,
        jit: *mut sys::t_class,
        gimmeback: bool,
    ) -> MaxResult<()> {
        unsafe {
            sys::max_jit_class_wrap_standard(c, jit, wrap_flags(gimmeback));
            sys::max_jit_class_ob3d_wrap(c);
            Ok(())
        }
    }
    unsafe fn setup_max(
        obj: *mut sys::t_object,
        jit: *mut sys::t_object,
        _args: &[Atom],
    ) -> Result<(), NewError> {
        unsafe {
            let dump = sys::outlet_new(obj.cast(), std::ptr::null());
            if dump.is_null() {
                return Err("unable to create dumpout".into());
            }
            sys::max_jit_obex_dumpout_set(obj.cast(), dump);
            let matrix = sys::outlet_new(obj.cast(), c"jit_matrix".as_ptr());
            if matrix.is_null() {
                return Err("unable to create matrix outlet".into());
            }
            sys::max_jit_ob3d_attach(obj.cast(), jit, matrix);
            Ok(())
        }
    }
    unsafe fn free_max(obj: *mut sys::t_object) {
        unsafe {
            sys::max_jit_ob3d_detach(obj.cast());
        }
    }
    unsafe fn assist(obj: *mut sys::t_object, io: c_long, index: c_long, text: *mut c_char) {
        unsafe {
            sys::max_jit_ob3d_assist(obj.cast(), std::ptr::null_mut(), io, index, text);
        }
    }
}
extern "C" fn gl_draw<T: Ob3d>(w: &Core<T, GlKind>) -> sys::t_jit_err {
    crate::ffi::guard_or(w, "ob3d_draw", err::JIT_ERR_GENERIC as _, || {
        jit_code(w.wrapped().draw(&DrawContext::current(w)))
    })
}
extern "C" fn gl_changed<T: Ob3d>(w: &Core<T, GlKind>) -> sys::t_jit_err {
    crate::ffi::guard_or(w, "dest_changed", err::JIT_ERR_GENERIC as _, || {
        jit_code(w.wrapped().dest_changed(&DrawContext::current(w)))
    })
}
extern "C" fn gl_closing<T: Ob3d>(w: &Core<T, GlKind>) -> sys::t_jit_err {
    crate::ffi::guard_or(w, "dest_closing", err::JIT_ERR_GENERIC as _, || {
        jit_code(w.wrapped().dest_closing(&DrawContext::current(w)))
    })
}
