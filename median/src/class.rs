//! Class registration.

use crate::atom::Atom;
use crate::attr::{
    Attr, AttrBuilder, AttrField, AttrOptions, AttrTrampGetMethod, AttrTrampSetMethod, AttrValue,
};
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
///     atom::Atom, attr::AttrOptions, builder::MaxWrappedBuilder, class::Class,
///     error::MaxResult, max_sys::t_atom_long, num::Int64, object::MaxObj, symbol::SymbolRef,
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
///     fn new(_builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
///         Example { value: Int64::new(0) }
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
/// # use median::{builder::MaxWrappedBuilder, class::Class, wrapper::*};
/// # struct Example;
/// # impl ObjClass for Example {
/// #     type Wrapper = MaxObjWrapper<Self>;
/// #     fn class_name() -> &'static std::ffi::CStr { c"example" }
/// # }
/// # impl Methods for Example {}
/// # impl FieldAttrs for Example {}
/// # impl MaxObjWrapped for Example {
/// #     fn new(_builder: &mut dyn MaxWrappedBuilder<Self>) -> Self { Example }
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
/// # use median::{builder::MaxWrappedBuilder, class::Class, wrapper::*};
/// # struct Example;
/// # impl ObjClass for Example {
/// #     type Wrapper = MaxObjWrapper<Self>;
/// #     fn class_name() -> &'static std::ffi::CStr { c"example" }
/// # }
/// # impl Methods for Example {}
/// # impl FieldAttrs for Example {}
/// # impl MaxObjWrapped for Example {
/// #     fn new(_builder: &mut dyn MaxWrappedBuilder<Self>) -> Self { Example }
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
    class: *mut max_sys::t_class,
    _phantom: PhantomData<T>,
}

#[derive(Debug, Copy, Clone)]
pub enum ClassType {
    Box,
    NoBox,
}

impl Into<*mut max_sys::t_symbol> for ClassType {
    fn into(self) -> *mut max_sys::t_symbol {
        let t = match self {
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

    ///
    pub unsafe fn new_registered(class: *mut max_sys::t_class) -> Self {
        Self {
            class,
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

    pub fn add_attribute(&mut self, attr: Attr<T>) -> MaxResult<()> {
        MaxError::from(
            unsafe { max_sys::class_addattr(self.inner(), attr.into()) as _ },
            (),
        )
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
    /// # use median::{builder::MaxWrappedBuilder, class::Class, max_sys::t_atom_long, wrapper::*};
    /// # struct Example;
    /// # impl ObjClass for Example {
    /// #     type Wrapper = MaxObjWrapper<Self>;
    /// #     fn class_name() -> &'static std::ffi::CStr { c"example" }
    /// # }
    /// # impl Methods for Example {}
    /// # impl FieldAttrs for Example {}
    /// # impl MaxObjWrapped for Example {
    /// #     fn new(_builder: &mut dyn MaxWrappedBuilder<Self>) -> Self { Example }
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
    /// than the number of arguments.
    pub fn add_sel<T, F, A, R>(&mut self, sel: &CStr, _f: F, defaults: usize) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: SelFn<T, A, R>,
    {
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
    /// gets the selector as well, so that one can serve several selectors.
    pub fn add_sel_gimme<T, F, R>(&mut self, sel: &CStr, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: Fn(&T, &SymbolRef, &[Atom]) -> R + Copy + Send + Sync + 'static,
        R: MethodResult,
    {
        let m = sel_gimme_method::<W, T, F, R>(f);
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
        if options.is_readonly() {
            return self.add_attr_readonly(name, _get, options);
        }
        let get: AttrTrampGetMethod<W> = crate::attr::get_tramp::<W, T, G, V>;
        let set: AttrTrampSetMethod<W> = crate::attr::set_tramp::<W, T, S, V, R>;
        let name = name.to_str().map_err(|_| MaxError::Generic)?;
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
        let get: AttrTrampGetMethod<W> = crate::attr::get_tramp::<W, T, G, V>;
        let name = name.to_str().map_err(|_| MaxError::Generic)?;
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

    fn add_built_attr(&mut self, b: &mut AttrBuilder<W>, options: AttrOptions) -> MaxResult<()> {
        b.clip(options.clip_value());
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
    ) -> MaxResult<()> {
        // SAFETY: guaranteed by the caller
        let r = unsafe { self.add_method_types(sel, Some(m), types) };
        MaxError::from(r as _, ())
    }
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

