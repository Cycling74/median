//! Class registration.

use crate::atom::Atom;
use crate::attr::{
    Attr, AttrBuilder, AttrField, AttrOptions, AttrTrampGetMethod, AttrTrampSetMethod, AttrValue,
};
use crate::defer::Defer;
use crate::error::{MaxError, MaxResult};
use crate::method::*;
use crate::object::MaxObj;
use crate::symbol::SymbolRef;
use crate::wrapper::WrapperWrapped;
use std::ffi::CStr;
use std::marker::PhantomData;
use std::os::raw::c_long;

/// A Max class, being set up in `class_setup` (or `ext_main`).
///
/// # Registering methods
///
/// Each `add_*` method takes a zero-sized `Copy` callable (a fn item such as `Self::bang`, or a
/// closure that captures nothing) and builds the `extern "C"` trampoline that Max calls from the
/// callable's type, so nothing needs to be written (or generated) per method. Capturing closures
/// are rejected with a compile error when the method is registered. The callable may return `()`
/// or a `Result<(), E>` where `E: Display`; an `Err` is given to the type's `report_error` hook
/// ([`MaxObjWrapped::report_error`]), which posts it to the Max console by default.
///
/// `#[median::methods]` generates these calls from marked methods. Written by hand they go in
/// [`Methods::register`] (or `class_setup`, for anything registered conditionally or under a
/// computed name), and attributes in [`FieldAttrs::register_attrs`] or `class_setup`:
///
/// ```
/// use median::{
///     atom::Atom, attr::AttrOptions, builder::MaxBuilder, class::Class,
///     error::{MaxResult, NewError}, max_sys::t_atom_long, num::Int64, object::MaxObj, symbol::SymbolRef,
///     wrapper::*,
/// };
/// use std::ffi::CStr;
///
/// struct Example {
///     value: Int64,
/// }
///
/// impl ObjClass for Example {
///     type Wrapper = MaxObjWrapper<Self>;
///     fn class_name() -> &'static CStr {
///         c"example"
///     }
/// }
///
/// impl MaxObjWrapped for Example {
///     fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
///         Ok(Example { value: Int64::new(0) })
///     }
/// }
///
/// impl FieldAttrs for Example {
///     fn register_attrs<W>(c: &mut Class<W>) -> MaxResult<()>
///     where
///         W: WrapperWrapped<Self> + MaxObj,
///     {
///         //an attribute "value" backed by the field, clipped to 0..=10
///         c.add_field_attr(c"value", |s: &Self| &s.value, AttrOptions::new().clip(0., 10.))
///     }
/// }
///
/// impl Methods for Example {
///     fn register<W>(c: &mut Class<W>) -> MaxResult<()>
///     where
///         W: WrapperWrapped<Self> + MaxObj,
///     {
///         c.add_bang(Self::bang)?;
///         c.add_int(Self::int)?;
///         c.add_list(Self::list)?;
///         c.add_any(Self::anything)?;
///         //a "set" message with a symbol argument that defaults if left out
///         c.add_sel(c"set", Self::set, 1)?;
///         //two floats
///         c.add_sel(c"range", |_: &Self, _lo: f64, _hi: f64| {}, 0)?;
///         //a method can fail: the error is posted to the console
///         c.add_float(|_: &Self, v: f64| if v < 0. { Err("negative") } else { Ok(()) })?;
///         //an attribute with a getter and a setter
///         c.add_attr(c"double", Self::double, Self::set_double, AttrOptions::new())?;
///         Ok(())
///     }
/// }
///
/// impl Example {
///     fn bang(&self) {}
///     fn int(&self, _v: t_atom_long) {}
///     fn list(&self, _atoms: &[Atom]) {}
///     fn anything(&self, _selector: &SymbolRef, _atoms: &[Atom]) {}
///     fn set(&self, _name: SymbolRef) {}
///     fn double(&self) -> t_atom_long {
///         self.value.get() * 2
///     }
///     fn set_double(&self, v: t_atom_long) {
///         self.value.set(v / 2)
///     }
/// }
/// ```
///
/// A closure that captures something can't work, because there is nowhere to keep the capture. It
/// is rejected at compile time:
///
/// ```compile_fail,E0080
/// # use median::{builder::MaxBuilder, class::Class, error::NewError, wrapper::*};
/// # struct Example;
/// # impl ObjClass for Example {
/// #     type Wrapper = MaxObjWrapper<Self>;
/// #     fn class_name() -> &'static std::ffi::CStr { c"example" }
/// # }
/// # impl Methods for Example {}
/// # impl FieldAttrs for Example {}
/// # impl MaxObjWrapped for Example {
/// #     fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> { Ok(Example) }
/// # }
/// # use median_mock as _;
/// # fn main() {
/// # let _k = median_mock::lock();
/// # let mut c: Class<MaxObjWrapper<Example>> =
/// #     Class::new(c"example", MaxObjWrapper::<Example>::new_tramp, None);
/// let n = std::hint::black_box(3);
/// c.add_bang(move |_: &Example| { std::hint::black_box(n); }).unwrap();
/// # }
/// ```
///
/// The same without the capture compiles and runs. (The hidden code in both examples makes the
/// class in `median-mock`, which stands in for the Max functions that the library calls, so that
/// they can be linked and run without Max. Without it the first example could also "fail" for
/// lack of them.)
///
/// ```
/// # use median::{builder::MaxBuilder, class::Class, error::NewError, wrapper::*};
/// # struct Example;
/// # impl ObjClass for Example {
/// #     type Wrapper = MaxObjWrapper<Self>;
/// #     fn class_name() -> &'static std::ffi::CStr { c"example" }
/// # }
/// # impl Methods for Example {}
/// # impl FieldAttrs for Example {}
/// # impl MaxObjWrapped for Example {
/// #     fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> { Ok(Example) }
/// # }
/// # use median_mock as _;
/// # fn main() {
/// # let _k = median_mock::lock();
/// # let mut c: Class<MaxObjWrapper<Example>> =
/// #     Class::new(c"example", MaxObjWrapper::<Example>::new_tramp, None);
/// let n = std::hint::black_box(3);
/// c.add_bang(|_: &Example| { std::hint::black_box(3); }).unwrap();
/// # let _ = n;
/// # }
/// ```
///
/// `Class<W>` is a class of the wrapper `W`, which is [`MaxObjWrapper<T>`] or [`MSPObjWrapper<T>`]
/// for your type `T`; `W` and `T` are inferred.
///
/// [`MaxObjWrapper<T>`]: crate::wrapper::MaxObjWrapper
/// [`MSPObjWrapper<T>`]: crate::wrapper::MSPObjWrapper
/// [`MaxObjWrapped::report_error`]: crate::wrapper::MaxObjWrapped::report_error
/// [`Methods::register`]: crate::wrapper::Methods::register
/// [`FieldAttrs::register_attrs`]: crate::wrapper::FieldAttrs::register_attrs
pub struct Class<T> {
    pub(crate) jitter: bool,
    pub(crate) attributes_only: bool,
    pub(crate) messages_only: bool,
    class: *mut max_sys::t_class,
    /// The attributes added through this `Class`, to refuse a duplicate name.
    attr_names: Vec<String>,
    /// A deferred method was added: the wrapper gives each object a deferred-call queue.
    deferred: bool,
    /// `add_notify` registered the `notify` method through this `Class`.
    notify_added: bool,
    _phantom: PhantomData<T>,
}

#[derive(Debug, Copy, Clone)]
pub enum ClassType {
    Box,
    NoBox,
}

impl From<ClassType> for *mut max_sys::t_symbol {
    fn from(value: ClassType) -> Self {
        let t = match value {
            ClassType::NoBox => c"nobox",
            ClassType::Box => c"box",
        };
        unsafe { max_sys::gensym(t.as_ptr()) }
    }
}

impl<T> Class<T> {
    pub fn exists_in_max(name: &CStr, class_type: ClassType) -> bool {
        !Self::find_in_max(name, class_type).is_null()
    }

    pub fn find_in_max(name: &CStr, class_type: ClassType) -> *mut max_sys::t_class {
        unsafe { max_sys::class_findbyname(class_type.into(), max_sys::gensym(name.as_ptr())) }
    }

    /// Wrap an existing Max class.
    ///
    /// # Safety
    /// `class` must be a live class whose instances have layout `T`. The caller
    /// must arrange registration/mutation on a thread where the host permits it.
    pub unsafe fn new_registered(class: *mut max_sys::t_class) -> Self {
        Self {
            jitter: false,
            attributes_only: false,
            messages_only: false,
            class,
            attr_names: Vec::new(),
            deferred: false,
            notify_added: false,
            _phantom: PhantomData,
        }
    }

    /// Create a new max class with the given name, new trampoline and optional freem trampoline.
    pub fn new(name: &CStr, new: MaxNew, free: Option<MaxFree<T>>) -> Self {
        let class = unsafe {
            max_sys::class_new(
                name.as_ptr(),
                Some(std::mem::transmute::<MaxNew, MaxMethod>(new)),
                std::mem::transmute::<Option<MaxFree<T>>, Option<MaxMethod>>(free),
                std::mem::size_of::<T>() as c_long,
                None,
                max_sys::e_max_atomtypes::A_GIMME as _,
                0,
            )
        };
        unsafe { Self::new_registered(class) }
    }

    /// Register the max class.
    pub fn register(&mut self, class_type: ClassType) -> MaxResult<()> {
        unsafe {
            MaxError::from(
                max_sys::class_register(class_type.into(), self.class) as _,
                (),
            )
        }
    }

    /// Get the inner class object.
    pub fn inner(&mut self) -> *mut max_sys::t_class {
        self.class
    }

    /// Whether a deferred method (`add_*_deferred`) was added, so that the objects of the class
    /// need a deferred-call queue.
    pub(crate) fn set_deferred(&mut self, value: bool) {
        self.deferred |= value;
    }

    pub(crate) fn clear_deferred(&mut self) {
        self.deferred = false;
    }

    pub(crate) fn has_deferred(&self) -> bool {
        self.deferred
    }

    /// Add a built attribute to the class, then the attributes of the attribute its options ask
    /// for (label, save ..., see [`AttrOptions`]). `Err(MaxError::Duplicate)` (and the attribute
    /// is freed) if an attribute of the same name was added through this `Class` already: field
    /// attributes, `#[attr]` getters and `class_setup` all add to the same class.
    pub fn add_attribute(&mut self, attr: Attr<T>) -> MaxResult<()> {
        if self.jitter != attr.jitter {
            return Err(MaxError::Generic);
        }
        if self.attr_names.iter().any(|n| n == attr.name()) {
            return Err(MaxError::Duplicate);
        }
        let name = attr.name().to_string();
        let (val_type, options) = (attr.val_type(), *attr.options());
        let r = MaxError::from(
            unsafe {
                if self.jitter {
                    max_sys::jit_class_addattr(self.inner().cast(), attr.inner()) as _
                } else {
                    max_sys::class_addattr(self.inner(), attr.inner()) as _
                }
            },
            (),
        );
        if r.is_ok() {
            let _: *mut max_sys::t_object = attr.into(); // class takes ownership
            self.attr_names.push(name.clone());
            //the attributes of the attribute, now that it is in the class
            let name = std::ffi::CString::new(name).map_err(|_| MaxError::Generic)?;
            // SAFETY: the class is valid and has the attribute `name`, added above
            unsafe {
                crate::attr::add_attr_attrs(self.class, &name, val_type, &options, self.jitter)?
            };
        }
        r
    }

    /// `Err(MaxError::Duplicate)` if the attribute `name` was added already.
    fn check_attr_name(&self, name: &str) -> MaxResult<()> {
        if self.attr_names.iter().any(|n| n == name) {
            Err(MaxError::Duplicate)
        } else {
            Ok(())
        }
    }

    /// `class_addmethod` with the argument types given as a slice.
    ///
    /// # Safety
    /// `m` must be a method that takes the object followed by arguments of `types`, with at most
    /// 7 of them (more is an error, and nothing is registered).
    unsafe fn add_method_types(
        &self,
        sel: &CStr,
        m: Option<MaxMethod>,
        types: &[max_sys::e_max_atomtypes::Type],
    ) -> max_sys::t_max_err {
        if self.jitter {
            // Jitter's variadic type list uses SDK longs, including its terminator.
            let types: Vec<c_long> = types.iter().map(|&t| t as c_long).collect();
            let code = unsafe {
                match types.as_slice() {
                    [] => max_sys::jit_class_addmethod(
                        self.class.cast(),
                        m,
                        sel.as_ptr(),
                        0 as c_long,
                    ),
                    [t0] => max_sys::jit_class_addmethod(
                        self.class.cast(),
                        m,
                        sel.as_ptr(),
                        *t0,
                        0 as c_long,
                    ),
                    [t0, t1] => max_sys::jit_class_addmethod(
                        self.class.cast(),
                        m,
                        sel.as_ptr(),
                        *t0,
                        *t1,
                        0 as c_long,
                    ),
                    [t0, t1, t2] => max_sys::jit_class_addmethod(
                        self.class.cast(),
                        m,
                        sel.as_ptr(),
                        *t0,
                        *t1,
                        *t2,
                        0 as c_long,
                    ),
                    [t0, t1, t2, t3] => max_sys::jit_class_addmethod(
                        self.class.cast(),
                        m,
                        sel.as_ptr(),
                        *t0,
                        *t1,
                        *t2,
                        *t3,
                        0 as c_long,
                    ),
                    [t0, t1, t2, t3, t4] => max_sys::jit_class_addmethod(
                        self.class.cast(),
                        m,
                        sel.as_ptr(),
                        *t0,
                        *t1,
                        *t2,
                        *t3,
                        *t4,
                        0 as c_long,
                    ),
                    [t0, t1, t2, t3, t4, t5] => max_sys::jit_class_addmethod(
                        self.class.cast(),
                        m,
                        sel.as_ptr(),
                        *t0,
                        *t1,
                        *t2,
                        *t3,
                        *t4,
                        *t5,
                        0 as c_long,
                    ),
                    [t0, t1, t2, t3, t4, t5, t6] => max_sys::jit_class_addmethod(
                        self.class.cast(),
                        m,
                        sel.as_ptr(),
                        *t0,
                        *t1,
                        *t2,
                        *t3,
                        *t4,
                        *t5,
                        *t6,
                        0 as c_long,
                    ),
                    _ => return max_sys::e_max_errorcodes::MAX_ERR_GENERIC as _,
                }
            };
            return if code == 0 {
                0
            } else {
                max_sys::e_max_errorcodes::MAX_ERR_GENERIC as _
            };
        }
        unsafe {
            match types.len() {
                0 => max_sys::class_addmethod(self.class, m, sel.as_ptr(), 0),
                1 => max_sys::class_addmethod(self.class, m, sel.as_ptr(), types[0], 0),
                2 => max_sys::class_addmethod(self.class, m, sel.as_ptr(), types[0], types[1], 0),
                3 => max_sys::class_addmethod(
                    self.class,
                    m,
                    sel.as_ptr(),
                    types[0],
                    types[1],
                    types[2],
                    0,
                ),
                4 => max_sys::class_addmethod(
                    self.class,
                    m,
                    sel.as_ptr(),
                    types[0],
                    types[1],
                    types[2],
                    types[3],
                    0,
                ),
                5 => max_sys::class_addmethod(
                    self.class,
                    m,
                    sel.as_ptr(),
                    types[0],
                    types[1],
                    types[2],
                    types[3],
                    types[4],
                    0,
                ),
                6 => max_sys::class_addmethod(
                    self.class,
                    m,
                    sel.as_ptr(),
                    types[0],
                    types[1],
                    types[2],
                    types[3],
                    types[4],
                    types[5],
                    0,
                ),
                7 => max_sys::class_addmethod(
                    self.class,
                    m,
                    sel.as_ptr(),
                    types[0],
                    types[1],
                    types[2],
                    types[3],
                    types[4],
                    types[5],
                    types[6],
                    0,
                ),
                _ => max_sys::e_max_errorcodes::MAX_ERR_GENERIC as _,
            }
        }
    }
}

/// Typed method registration, without trampoline macros: see the documentation of [`Class`].
impl<W> Class<W> {
    /// Receive a named dictionary (`A_SYM`). The trampoline retains it for the call and reports
    /// an unknown name without calling the method. The received handle shares the sender's data.
    pub fn add_dictionary<T, F, R>(&mut self, _f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T, crate::dict::Dict) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        let m = unsafe {
            erase(dictionary_tramp::<W, T, F, R> as extern "C" fn(&W, *mut max_sys::t_symbol))
        };
        unsafe { self.add_typed(c"dictionary", m, &[max_sys::e_max_atomtypes::A_SYM]) }
    }

    /// Save state into the patcher's dictionary (`appendtodictionary`, `A_CANT`).
    /// Insert a deep copy of private state to keep the patcher's saved value independent.
    pub fn add_savestate<T, F, R>(&mut self, _f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T, &crate::dict::Dict) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        let m = unsafe {
            erase(savestate_tramp::<W, T, F, R> as extern "C" fn(&W, *mut max_sys::t_dictionary))
        };
        unsafe {
            self.add_typed(
                c"appendtodictionary",
                m,
                &[max_sys::e_max_atomtypes::A_CANT],
            )
        }
    }

    /// Register the `bang` method.
    pub fn add_bang<T, F, R>(&mut self, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        let m = bang_method::<W, T, F, R>(f);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"bang", m, &[]) }
    }

    /// Register the `dblclick` method (`A_CANT`), which Max calls when the box is double-clicked
    /// in a locked patcher (on the main thread).
    pub fn add_dblclick<T, F, R>(&mut self, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        let m = dblclick_method::<W, T, F, R>(f);
        // SAFETY: `m` is a trampoline taking the object, as Max calls `dblclick`
        unsafe { self.add_typed(c"dblclick", m, &[max_sys::e_max_atomtypes::A_CANT]) }
    }

    /// Register the `notify` method (`A_CANT`), which Max calls when an object this one is
    /// attached or subscribed to (or a `buffer~` it references) sends a notification. `f` gets
    /// the object and the [`Notification`], valid for the call only: its `sender` and `data`
    /// pointers must not be kept.
    ///
    /// median's own `notify` handling (the `buffer~` references of the object, see
    /// `MaxBuilder::buffer_ref`) runs before `f`. A class without a `notify` method gets a
    /// wrapper's own that does only that, so `notify` stays in [`RESERVED_SELECTORS`]:
    /// [`Class::add_sel`] refuses it. `Err(MaxError::Duplicate)` if the class has a `notify`
    /// method already (added through this `Class` before, or by the SDK's Jitter wrapper).
    ///
    /// A Jitter class (`kind = matrix_op` or `gl`) cannot use this (nor `#[notify]`): the SDK's
    /// wrapper, or median's glue, owns `notify` there, so a `#[notify]` method on such a type
    /// is a compile error. (The glue forwards the notification to `JitterObject::notify`, which
    /// the matrix and GL kinds implement themselves.)
    ///
    /// The wrapper's own `notify` is installed only when the class has none after `class_setup`
    /// has run, so a `class_setup` that installs `notify` directly through `class_addmethod`
    /// (instead of this method) also turns off the wrapper's `buffer~` reference forwarding.
    ///
    /// [`Notification`]: crate::notify::Notification
    pub fn add_notify<T, F, R>(&mut self, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T, &crate::notify::Notification) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        const {
            assert!(
                W::SUPPORTS_NOTIFY,
                "a #[notify] method or Class::add_notify on a Jitter object: the SDK wrapper owns `notify` (the glue forwards it to JitterObject::notify)"
            )
        };
        if self.attributes_only {
            return Ok(());
        }
        // SAFETY: the class is valid. (A Jitter class is only checked through `notify_added`:
        // `jit_class_method` is not part of what the mock implements.)
        if self.notify_added
            || (!self.jitter
                && unsafe { max_sys::class_method(self.class, crate::sym!("notify").as_ptr()) }
                    .is_some())
        {
            return Err(MaxError::Duplicate);
        }
        let m = notify_method::<W, T, F, R>(f);
        // SAFETY: `m` is a trampoline taking the object and the four arguments of `notify`
        unsafe { self.add_typed_inner(c"notify", m, &[max_sys::e_max_atomtypes::A_CANT], true) }?;
        self.notify_added = true;
        Ok(())
    }

    /// Register the `loadbang` method (`A_CANT`), which Max calls on the main thread when the
    /// patcher the object is in has finished loading, as the `loadbang` object fires. Max passes
    /// only the object. (The SDK headers don't declare the signature; `(x)` is what C externals
    /// and min register. Unverified here.)
    pub fn add_loadbang<T, F, R>(&mut self, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        let m = loadbang_method::<W, T, F, R>(f);
        // SAFETY: `m` is a trampoline taking the object, as Max calls `loadbang`
        unsafe { self.add_typed(c"loadbang", m, &[max_sys::e_max_atomtypes::A_CANT]) }
    }

    /// Register the `int` method.
    pub fn add_int<T, F, R>(&mut self, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T, max_sys::t_atom_long) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        let m = int_method::<W, T, F, R>(f);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"int", m, &[max_sys::e_max_atomtypes::A_LONG]) }
    }

    /// Register the `float` method.
    pub fn add_float<T, F, R>(&mut self, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T, f64) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        let m = float_method::<W, T, F, R>(f);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"float", m, &[max_sys::e_max_atomtypes::A_FLOAT]) }
    }

    /// Register the `symbol` method.
    pub fn add_sym<T, F, R>(&mut self, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T, SymbolRef) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        let m = sym_method::<W, T, F, R>(f);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"symbol", m, &[max_sys::e_max_atomtypes::A_SYM]) }
    }

    /// Register the `list` method. The selector (`list`) is not passed on.
    pub fn add_list<T, F, R>(&mut self, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T, &[Atom]) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        let m = list_method::<W, T, F, R>(f);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"list", m, &[max_sys::e_max_atomtypes::A_GIMME]) }
    }

    /// Register the `anything` method, which gets the message selector and its arguments.
    pub fn add_any<T, F, R>(&mut self, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T, &SymbolRef, &[Atom]) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        let m = any_method::<W, T, F, R>(f);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"anything", m, &[max_sys::e_max_atomtypes::A_GIMME]) }
    }

    /// Register the method `sel` with typed arguments, taken from the callable's signature: up
    /// to 7 arguments that are each `f64`, `t_atom_long` or [`SymbolRef`]; either all of them
    /// `f64` or none. Max passes floats and other arguments of a typed method in different
    /// registers, so a mix is rejected at compile time; use [`Class::add_sel_gimme`] for it:
    ///
    /// ```compile_fail,E0080
    /// # use median::{builder::MaxBuilder, class::Class, error::NewError, max_sys::t_atom_long, wrapper::*};
    /// # struct Example;
    /// # impl ObjClass for Example {
    /// #     type Wrapper = MaxObjWrapper<Self>;
    /// #     fn class_name() -> &'static std::ffi::CStr { c"example" }
    /// # }
    /// # impl Methods for Example {}
    /// # impl FieldAttrs for Example {}
    /// # impl MaxObjWrapped for Example {
    /// #     fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> { Ok(Example) }
    /// # }
    /// # use median_mock as _;
    /// # fn main() {
    /// # let _k = median_mock::lock();
    /// # let mut c: Class<MaxObjWrapper<Example>> =
    /// #     Class::new(c"example", MaxObjWrapper::<Example>::new_tramp, None);
    /// c.add_sel(c"range", |_: &Example, _lo: f64, _hi: t_atom_long| {}, 0).unwrap();
    /// # }
    /// ```
    ///
    /// (The same with `_hi: f64` compiles; see the [`Class`] documentation, whose examples do.)
    ///
    /// The last `defaults` arguments are optional (`A_DEFFLOAT` and so on): Max passes 0, 0.0 or
    /// the empty symbol when the message leaves them out. It is an error if `defaults` is more
    /// than the number of arguments, and `Err(MaxError::Duplicate)` if `sel` is one of
    /// [`RESERVED_SELECTORS`], [`UNTYPED_SELECTORS`] or, for an MSP class,
    /// [`MSP_RESERVED_SELECTORS`].
    pub fn add_sel<T, F, A, R>(&mut self, sel: &CStr, _f: F, defaults: usize) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: SelFn<T, A, R>,
    {
        if is_untyped_selector(sel) {
            return Err(MaxError::Duplicate);
        }
        let mut types = [0; 7];
        //`SelFn` is sealed and its impls have at most 7 arguments; checked anyway rather than
        //relied on
        if F::TYPES.len() > types.len() {
            return Err(MaxError::Generic);
        }
        let types = &mut types[..F::TYPES.len()];
        types.copy_from_slice(F::TYPES);
        default_types(types, defaults)?;
        // SAFETY: the trampoline takes the object and arguments of `F::TYPES`
        unsafe { self.add_typed(sel, F::trampoline::<W>(), types) }
    }

    /// Register the method `sel` with the arguments as a list of atoms (`A_GIMME`). The callable
    /// either takes the selector as well, so that one function can serve several selectors, or
    /// does not (see [`GimmeFn`]; which shape it is, is inferred):
    ///
    /// ```ignore
    /// c.add_sel_gimme(c"foo", Self::foo)?;   // fn foo(&self, sel: &SymbolRef, atoms: &[Atom])
    /// c.add_sel_gimme(c"bar", Self::foo)?;   // the same function answers two selectors
    /// c.add_sel_gimme(c"read", Self::read)?; // fn read(&self, atoms: &[Atom])
    /// ```
    ///
    /// `sel` must not be one of [`RESERVED_SELECTORS`], [`UNTYPED_SELECTORS`] or, for an MSP
    /// class, [`MSP_RESERVED_SELECTORS`] (`Err(MaxError::Duplicate)`).
    pub fn add_sel_gimme<T, F, R, A>(&mut self, sel: &CStr, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: GimmeFn<T, R, A>,
        R: MethodResult,
    {
        if is_untyped_selector(sel) {
            return Err(MaxError::Duplicate);
        }
        let m = sel_gimme_method::<W, T, F, R, A>(f);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(sel, m, &[max_sys::e_max_atomtypes::A_GIMME]) }
    }

    /// [`add_bang`](Self::add_bang), deferred to the main thread as `mode` says (see
    /// [`crate::defer`]; what `#[bang] #[defer]` and `#[bang] #[defer_low]` generate). Never send
    /// a deferred message from `perform`.
    pub fn add_bang_deferred<T, F, R>(&mut self, f: F, mode: Defer) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        T: 'static,
        F: Fn(&T) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        self.deferred = true;
        let m = bang_deferred_method::<W, T, F, R>(f, mode);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"bang", m, &[]) }
    }

    /// [`add_int`](Self::add_int), deferred (see [`add_bang_deferred`](Self::add_bang_deferred)).
    pub fn add_int_deferred<T, F, R>(&mut self, f: F, mode: Defer) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        T: 'static,
        F: Fn(&T, max_sys::t_atom_long) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        self.deferred = true;
        let m = int_deferred_method::<W, T, F, R>(f, mode);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"int", m, &[max_sys::e_max_atomtypes::A_LONG]) }
    }

    /// [`add_float`](Self::add_float), deferred (see
    /// [`add_bang_deferred`](Self::add_bang_deferred)).
    pub fn add_float_deferred<T, F, R>(&mut self, f: F, mode: Defer) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        T: 'static,
        F: Fn(&T, f64) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        self.deferred = true;
        let m = float_deferred_method::<W, T, F, R>(f, mode);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"float", m, &[max_sys::e_max_atomtypes::A_FLOAT]) }
    }

    /// [`add_sym`](Self::add_sym), deferred (see [`add_bang_deferred`](Self::add_bang_deferred)).
    pub fn add_sym_deferred<T, F, R>(&mut self, f: F, mode: Defer) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        T: 'static,
        F: Fn(&T, SymbolRef) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        self.deferred = true;
        let m = sym_deferred_method::<W, T, F, R>(f, mode);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"symbol", m, &[max_sys::e_max_atomtypes::A_SYM]) }
    }

    /// [`add_list`](Self::add_list), deferred (see
    /// [`add_bang_deferred`](Self::add_bang_deferred)). The atoms are copied for the call.
    pub fn add_list_deferred<T, F, R>(&mut self, f: F, mode: Defer) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        T: 'static,
        F: Fn(&T, &[Atom]) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        self.deferred = true;
        let m = list_deferred_method::<W, T, F, R>(f, mode);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"list", m, &[max_sys::e_max_atomtypes::A_GIMME]) }
    }

    /// [`add_any`](Self::add_any), deferred (see [`add_bang_deferred`](Self::add_bang_deferred)).
    /// The atoms are copied for the call.
    pub fn add_any_deferred<T, F, R>(&mut self, f: F, mode: Defer) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        T: 'static,
        F: Fn(&T, &SymbolRef, &[Atom]) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        self.deferred = true;
        let m = any_deferred_method::<W, T, F, R>(f, mode);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(c"anything", m, &[max_sys::e_max_atomtypes::A_GIMME]) }
    }

    /// [`add_sel`](Self::add_sel), deferred (see [`add_bang_deferred`](Self::add_bang_deferred));
    /// it refuses the same selectors.
    pub fn add_sel_deferred<T, F, A, R>(
        &mut self,
        sel: &CStr,
        _f: F,
        defaults: usize,
        mode: Defer,
    ) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        T: 'static,
        F: SelFn<T, A, R>,
    {
        if is_untyped_selector(sel) {
            return Err(MaxError::Duplicate);
        }
        let mut types = [0; 7];
        if F::TYPES.len() > types.len() {
            return Err(MaxError::Generic);
        }
        let types = &mut types[..F::TYPES.len()];
        types.copy_from_slice(F::TYPES);
        default_types(types, defaults)?;
        self.deferred = true;
        // SAFETY: the trampoline takes the object and arguments of `F::TYPES`
        unsafe { self.add_typed(sel, F::deferred_trampoline::<W>(mode), types) }
    }

    /// [`add_sel_gimme`](Self::add_sel_gimme), deferred (see
    /// [`add_bang_deferred`](Self::add_bang_deferred)). The atoms are copied for the call. Takes
    /// either shape of callable, as `add_sel_gimme` does (see [`GimmeFn`]).
    pub fn add_sel_gimme_deferred<T, F, R, A>(
        &mut self,
        sel: &CStr,
        f: F,
        mode: Defer,
    ) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        T: 'static,
        F: GimmeFn<T, R, A>,
        R: MethodResult,
    {
        if is_untyped_selector(sel) {
            return Err(MaxError::Duplicate);
        }
        self.deferred = true;
        let m = sel_gimme_deferred_method::<W, T, F, R, A>(f, mode);
        // SAFETY: `m` is a trampoline taking the object and the registered arguments
        unsafe { self.add_typed(sel, m, &[max_sys::e_max_atomtypes::A_GIMME]) }
    }

    /// Register the attribute `name`, read with `get` and written with `set`, both zero-sized
    /// `Copy` callables as for the methods (fn items such as `Self::foo` and `Self::set_foo`, or
    /// closures that capture nothing). The value type `V` (`f64`, `f32`, `t_atom_long` or
    /// [`SymbolRef`]) decides the Max attribute type.
    ///
    /// `set` may return `Result<(), E>` where `E: Display`: an `Err` goes to the type's
    /// `report_error` hook and Max is told the set failed. A panic in either accessor is posted to
    /// the console. With [`AttrOptions::readonly`] `set` is not registered (see
    /// [`Class::add_attr_readonly`]).
    pub fn add_attr<T, V, G, S, R>(
        &mut self,
        name: &CStr,
        _get: G,
        _set: S,
        options: AttrOptions,
    ) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        V: AttrValue,
        G: Fn(&T) -> V + Copy + Send + Sync + 'static,
        S: Fn(&T, V) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        if self.messages_only {
            return Ok(());
        }
        if options.is_readonly() {
            return self.add_attr_readonly(name, _get, options);
        }
        if self.messages_only {
            return Ok(());
        }
        let get: AttrTrampGetMethod<W> = crate::attr::get_tramp::<W, T, G, V>;
        let set: AttrTrampSetMethod<W> = crate::attr::set_tramp::<W, T, S, V, R>;
        let name = name.to_str().map_err(|_| MaxError::Generic)?;
        self.check_attr_name(name)?;
        let mut b = AttrBuilder::new_accessors(name, V::ATTR_TYPE, get, set);
        self.add_built_attr(&mut b, options)
    }

    /// Register the read-only attribute `name`, read with `get` (as [`Class::add_attr`]). Users
    /// can't set it (`ATTR_SET_OPAQUE_USER`), and a set from code does nothing.
    pub fn add_attr_readonly<T, V, G>(
        &mut self,
        name: &CStr,
        _get: G,
        options: AttrOptions,
    ) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        V: AttrValue,
        G: Fn(&T) -> V + Copy + Send + Sync + 'static,
    {
        if self.messages_only {
            return Ok(());
        }
        let get: AttrTrampGetMethod<W> = crate::attr::get_tramp::<W, T, G, V>;
        let name = name.to_str().map_err(|_| MaxError::Generic)?;
        self.check_attr_name(name)?;
        let mut b = AttrBuilder::new_get(name, V::ATTR_TYPE, get);
        self.add_built_attr(&mut b, options)
    }

    /// Register the attribute `name` backed by a field of the object, given as a zero-sized
    /// callable that returns a reference to it (`|s: &Self| &s.value`). The field's type
    /// implements [`AttrField`] (the [`num`](crate::num) atomics do), which gives the accessors
    /// and the value type. This is what `#[attr]` on a field generates.
    pub fn add_field_attr<T, A, F>(
        &mut self,
        name: &CStr,
        field: F,
        options: AttrOptions,
    ) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        A: AttrField,
        F: Fn(&T) -> &A + Copy + Send + Sync + 'static,
    {
        //these capture only `field`, which is zero-sized and `Copy`, so they are too: the
        //trampolines can make them from nothing as they can `field`
        self.add_attr(
            name,
            move |t: &T| field(t).get(),
            move |t: &T, v: A::Value| field(t).set(v),
            options,
        )
    }

    fn add_built_attr(&mut self, b: &mut AttrBuilder<W>, options: AttrOptions) -> MaxResult<()>
    where
        W: MaxObj,
    {
        b.options(options);
        let attr = b.build().map_err(|_| MaxError::Generic)?;
        self.add_attribute(attr)
    }

    /// # Safety
    /// `m` must be a method that takes the object followed by arguments of `types`.
    unsafe fn add_typed(
        &mut self,
        sel: &CStr,
        m: MaxMethod,
        types: &[max_sys::e_max_atomtypes::Type],
    ) -> MaxResult<()>
    where
        W: MaxObj,
    {
        // SAFETY: guaranteed by the caller
        unsafe { self.add_typed_inner(sel, m, types, false) }
    }

    /// `add_typed`; with `allow_reserved` it doesn't refuse the selectors the wrapper reserves
    /// (for the methods that have their own `add_*` for one, `Class::add_notify`).
    ///
    /// # Safety
    /// `m` must be a method that takes the object followed by arguments of `types`.
    unsafe fn add_typed_inner(
        &mut self,
        sel: &CStr,
        m: MaxMethod,
        types: &[max_sys::e_max_atomtypes::Type],
        allow_reserved: bool,
    ) -> MaxResult<()>
    where
        W: MaxObj,
    {
        if self.attributes_only {
            return Ok(());
        }
        if self.jitter && self.deferred {
            // A standalone Jitter core has no owner-bound Max deferred-call queue.
            return Err(MaxError::Generic);
        }
        if !allow_reserved && is_reserved_selector(sel, W::IS_MSP) {
            return Err(MaxError::Duplicate);
        }
        // SAFETY: guaranteed by the caller
        let r = unsafe { self.add_method_types(sel, Some(m), types) };
        MaxError::from(r as _, ())
    }
}

/// The selectors the wrapper registers for every class (`notify`, `assist`, `inletinfo`, the
/// `int`/`float` inlet messages `in1`..`in9` and `ft1`..`ft9`, and `dsp64` for MSP objects).
/// [`Class::add_sel`], [`Class::add_sel_gimme`] and `#[sel]` refuse them: registering one would
/// replace or collide with the wrapper's own method. `notify` has its own method,
/// [`Class::add_notify`] (`#[notify]`), which the wrapper's default gives way to.
pub const RESERVED_SELECTORS: &[&str] = &[
    "notify",
    "assist",
    "inletinfo",
    "dsp64",
    "ft1",
    "ft2",
    "ft3",
    "ft4",
    "ft5",
    "ft6",
    "ft7",
    "ft8",
    "ft9",
    "in1",
    "in2",
    "in3",
    "in4",
    "in5",
    "in6",
    "in7",
    "in8",
    "in9",
];

/// The untyped (`A_CANT`) messages the SDK headers and examples show Max calling through
/// `object_method` with its own fixed arguments, whatever argument types the class registered
/// (`ext_obex.h`, "untyped message"): `dblclick`, `loadbang`, `appendtodictionary`,
/// `getvalueof`/`setvalueof` (pattr, `ext_obex.h`) and `multichanneloutputs`/`inputchanged`
/// (MC, `z_dsp.h`). A typed `#[sel]`, [`Class::add_sel`] or [`Class::add_sel_gimme`] for one
/// would misread what Max passes (a `t_dictionary*` or a `long*` as a symbol or an atom list,
/// say), so they refuse these selectors (`Err(MaxError::Duplicate)`). Use
/// [`Class::add_dblclick`], [`Class::add_loadbang`] or [`Class::add_savestate`] (the
/// `#[dblclick]`, `#[loadbang]` and `#[savestate]` markers); the other four have no median
/// hook yet. The list is what the SDK shows, not a complete census of Max's untyped messages.
pub const UNTYPED_SELECTORS: &[&str] = &[
    "dblclick",
    "loadbang",
    "appendtodictionary",
    "getvalueof",
    "setvalueof",
    "multichanneloutputs",
    "inputchanged",
];

fn is_untyped_selector(sel: &CStr) -> bool {
    sel.to_str().is_ok_and(|s| UNTYPED_SELECTORS.contains(&s))
}

/// The selectors `class_dspinit` adds to an MSP class (`z_dsp.h`: "you should avoid binding
/// anything to their names"). The wrapper calls `class_dspinit` after the user's methods are
/// registered, so a user method of one of these names would be silently replaced (or replace
/// MSP's). `Class` refuses them for an MSP class (its wrapper is `MSPObjWrapper`) when the
/// method is registered; a Max class may use them. (`#[median::methods]` can't tell the kind, so
/// for it the refusal is the registration error.)
pub const MSP_RESERVED_SELECTORS: &[&str] = &["signal", "userconnect", "nsiginlets", "enable"];

/// Whether `sel` is one of [`RESERVED_SELECTORS`], or, for an MSP class (`msp`), of
/// [`MSP_RESERVED_SELECTORS`]: what [`Class::add_sel`], [`Class::add_sel_gimme`] (any `add_*` in
/// fact) refuse.
pub fn is_reserved_selector(sel: &CStr, msp: bool) -> bool {
    sel.to_str().is_ok_and(|s| {
        RESERVED_SELECTORS.contains(&s) || (msp && MSP_RESERVED_SELECTORS.contains(&s))
    })
}

/// Turn the last `defaults` of `types` into their `A_DEF*` variants. An error if there are fewer
/// types than `defaults` or one of them can't be defaulted (only float, long and symbol can).
/// `types` is not modified in the error case.
pub(crate) fn default_types(
    types: &mut [max_sys::e_max_atomtypes::Type],
    defaults: usize,
) -> MaxResult<()> {
    use max_sys::e_max_atomtypes::*;
    let l = types.len();
    if defaults > l {
        return Err(MaxError::Generic);
    }
    let converted = |t: Type| match t {
        A_FLOAT | A_DEFFLOAT => Some(A_DEFFLOAT),
        A_LONG | A_DEFLONG => Some(A_DEFLONG),
        A_SYM | A_DEFSYM => Some(A_DEFSYM),
        _ => None,
    };
    if types[l - defaults..]
        .iter()
        .any(|t| converted(*t).is_none())
    {
        return Err(MaxError::Generic);
    }
    for t in types[l - defaults..].iter_mut() {
        *t = converted(*t).unwrap_or(*t);
    }
    Ok(())
}
