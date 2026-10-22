//! Attributes.

use crate::atom::Atom;
use crate::error::{MaxError, MaxResult};
use crate::max::common_symbols;
use crate::method::{MaxMethod, MethodResult, zst};
use crate::num::{Float32, Float64, Int64};
use crate::object::MaxObj;
use crate::symbol::SymbolRef;
use crate::wrapper::WrapperWrapped;

use std::ffi::{CStr, CString, c_void};
use std::marker::PhantomData;
use std::os::raw::c_long;

pub type AttrTrampGetMethod<T> = unsafe extern "C" fn(
    s: &T,
    attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> max_sys::t_max_err;
pub type AttrTrampSetMethod<T> = unsafe extern "C" fn(
    s: &T,
    attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
) -> max_sys::t_max_err;

/// A wrapper for a max attribute. `T` refers to the object that the attribute is attributed to.
pub struct Attr<T> {
    inner: *mut max_sys::t_object,
    name: String,
    val_type: AttrType,
    /// The attributes of the attribute (label, save ...), added by `Class::add_attribute` once
    /// the attribute is in the class.
    options: AttrOptions,
    _phantom: PhantomData<T>,
}

//could add scale but it doesn't look like anything in max uses it

/// A builder for building up attributes.
pub struct AttrBuilder<T> {
    name: String,
    val_type: AttrType,
    offset: Option<usize>,
    get: Option<AttrTrampGetMethod<T>>,
    set: Option<AttrTrampSetMethod<T>>,
    clip: AttrClip,
    get_vis: AttrVisiblity,
    set_vis: AttrVisiblity,
    options: AttrOptions,
}

impl<T> Attr<T> {
    pub fn inner(&self) -> *mut max_sys::t_object {
        self.inner
    }

    /// The attribute's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The attribute's type.
    pub fn val_type(&self) -> AttrType {
        self.val_type
    }

    /// The options to apply when it is added to a class.
    pub fn options(&self) -> &AttrOptions {
        &self.options
    }
}

/// No-op get method for attributes.
pub extern "C" fn get_nop<T>(
    _s: &T,
    _attr: *mut c_void,
    _ac: *mut c_long,
    _av: *mut *mut max_sys::t_atom,
) -> max_sys::t_max_err {
    max_sys::e_max_errorcodes::MAX_ERR_NONE as _
}
/// No-op set method for attributes.
pub extern "C" fn set_nop<T>(
    _s: &T,
    _attr: *mut c_void,
    _ac: c_long,
    _av: *mut max_sys::t_atom,
) -> max_sys::t_max_err {
    max_sys::e_max_errorcodes::MAX_ERR_NONE as _
}

impl<T> AttrBuilder<T> {
    // helper method used in public impls
    fn new<I: Into<String>>(name: I, val_type: AttrType) -> Self {
        Self {
            name: name.into(),
            val_type,
            offset: None,
            get: None,
            set: None,
            clip: AttrClip::None,
            get_vis: AttrVisiblity::Visible,
            set_vis: AttrVisiblity::Visible,
            options: AttrOptions::new(),
        }
    }

    /// Create a new builder with an offset pointing to a struct member.
    ///
    /// # Arguments
    /// * `name` - the name of the attribute.
    /// * `val_type` - the type of the attribute.
    /// * `offset_bytes` - a byte offset within the struct to the member that represents the attribute.
    pub unsafe fn new_offset<I: Into<String>>(
        name: I,
        val_type: AttrType,
        offset_bytes: usize,
    ) -> Self {
        let mut s = Self::new(name, val_type);
        s.offset = Some(offset_bytes);
        s
    }

    /// Create a new builder with an offset pointing to a struct member and a get method.
    ///
    /// # Arguments
    /// * `name` - the name of the attribute.
    /// * `val_type` - the type of the attribute.
    /// * `offset_bytes` - a byte offset within the struct to the member that represents the attribute.
    /// * `get` - a get method to use with the attribute.
    pub unsafe fn new_offset_get<I: Into<String>>(
        name: I,
        val_type: AttrType,
        offset_bytes: usize,
        get: AttrTrampGetMethod<T>,
    ) -> Self {
        // SAFETY: the caller upholds the contract of `new_offset`
        let mut s = unsafe { Self::new_offset(name, val_type, offset_bytes) };
        s.get = Some(get);
        s
    }

    /// Create a new builder with an offset pointing to a struct member and a set method.
    ///
    /// # Arguments
    /// * `name` - the name of the attribute.
    /// * `val_type` - the type of the attribute.
    /// * `offset_bytes` - a byte offset within the struct to the member that represents the attribute.
    /// * `set` - a set method to use with the attribute.
    pub unsafe fn new_offset_set<I: Into<String>>(
        name: I,
        val_type: AttrType,
        offset_bytes: usize,
        set: AttrTrampSetMethod<T>,
    ) -> Self {
        // SAFETY: the caller upholds the contract of `new_offset`
        let mut s = unsafe { Self::new_offset(name, val_type, offset_bytes) };
        s.set = Some(set);
        s
    }

    /// Create a new builder with accessor methods.
    ///
    /// # Arguments
    /// * `name` - the name of the attribute.
    /// * `val_type` - the type of the attribute.
    /// * `get` - a get method to use with the attribute.
    /// * `set` - a set method to use with the attribute.
    pub fn new_accessors<I: Into<String>>(
        name: I,
        val_type: AttrType,
        get: AttrTrampGetMethod<T>,
        set: AttrTrampSetMethod<T>,
    ) -> Self {
        let mut s = Self::new(name, val_type);
        s.get = Some(get);
        s.set = Some(set);
        s
    }

    /// Create a new builder with only a get method.
    ///
    /// # Arguments
    /// * `name` - the name of the attribute.
    /// * `val_type` - the type of the attribute.
    /// * `get` - a get method to use with the attribute.
    pub fn new_get<I: Into<String>>(
        name: I,
        val_type: AttrType,
        get: AttrTrampGetMethod<T>,
    ) -> Self {
        let mut s = Self::new(name, val_type);
        s.get = Some(get);
        s.set_vis = AttrVisiblity::UserVisible;
        s
    }

    /// Create a new builder with only a set method.
    ///
    /// # Arguments
    /// * `name` - the name of the attribute.
    /// * `val_type` - the type of the attribute.
    /// * `set` - a set method to use with the attribute.
    pub fn new_set<I: Into<String>>(
        name: I,
        val_type: AttrType,
        set: AttrTrampSetMethod<T>,
    ) -> Self {
        let mut s = Self::new(name, val_type);
        s.set = Some(set);
        s.get_vis = AttrVisiblity::UserVisible;
        s
    }

    /// Set the visiblity for the get for this attribute.
    ///
    /// # Remarks
    /// Defaults to `Visible`.
    ///
    /// # Panics
    /// Will panic if called when there is no offset or get method.
    pub fn get_vis(&mut self, v: AttrVisiblity) -> &mut Self {
        assert!(
            self.get.is_some() || self.offset.is_some(),
            "to set get visibilty you must have either a get method or an offset"
        );
        let n = self;
        n.get_vis = v;
        n
    }
    /// Set the visiblity for the set for this attribute.
    ///
    /// # Remarks
    /// Defaults to `Visible`.
    ///
    /// # Panics
    /// Will panic if called when there is no offset or set method.
    pub fn set_vis(&mut self, v: AttrVisiblity) -> &mut Self {
        assert!(
            self.set.is_some() || self.offset.is_some(),
            "to set set visibilty you must have either a set method or an offset"
        );
        let n = self;
        n.set_vis = v;
        n
    }
    /// Set the optional clip for this attribute.
    pub fn clip(&mut self, v: AttrClip) -> &mut Self {
        let n = self;
        n.clip = v;
        n
    }

    /// Take everything from `options`: the clip, read-only (users can't set it,
    /// `ATTR_SET_OPAQUE_USER`; a setter is still called from code) and the attributes of the
    /// attribute (label, style, category, order, default, save, enum, range, hidden), which
    /// [`Class::add_attribute`](crate::class::Class::add_attribute) adds once the attribute is in
    /// the class. Replaces what was set before.
    pub fn options(&mut self, options: AttrOptions) -> &mut Self {
        self.clip = options.clip;
        if options.readonly {
            self.set_vis = AttrVisiblity::UserVisible;
        }
        self.options = options;
        self
    }

    /// See [`AttrOptions::label`].
    pub fn label(&mut self, label: &'static CStr) -> &mut Self {
        self.options = self.options.label(label);
        self
    }

    /// See [`AttrOptions::style`].
    pub fn style(&mut self, style: &'static CStr) -> &mut Self {
        self.options = self.options.style(style);
        self
    }

    /// See [`AttrOptions::category`].
    pub fn category(&mut self, category: &'static CStr) -> &mut Self {
        self.options = self.options.category(category);
        self
    }

    /// See [`AttrOptions::order`].
    pub fn order(&mut self, order: max_sys::t_atom_long) -> &mut Self {
        self.options = self.options.order(order);
        self
    }

    /// See [`AttrOptions::default`].
    pub fn default(&mut self, value: impl Into<AttrDefault>) -> &mut Self {
        self.options = self.options.default(value);
        self
    }

    /// See [`AttrOptions::save`].
    pub fn save(&mut self) -> &mut Self {
        self.options = self.options.save();
        self
    }

    /// See [`AttrOptions::enum_vals`].
    pub fn enum_vals(&mut self, vals: &'static CStr) -> &mut Self {
        self.options = self.options.enum_vals(vals);
        self
    }

    /// See [`AttrOptions::range`].
    pub fn range(&mut self, min: f64, max: f64) -> &mut Self {
        self.options = self.options.range(min, max);
        self
    }

    /// See [`AttrOptions::hidden`].
    pub fn hidden(&mut self) -> &mut Self {
        self.options = self.options.hidden();
        self
    }

    pub fn build(&self) -> Result<Attr<T>, String> {
        if self.set.is_none() && self.get.is_none() && self.offset.is_none() {
            return Err("you must have at least 1 of get, set or offset".into());
        }
        let n = std::ffi::CString::new(self.name.clone())
            .map_err(|_| format!("{} failed to convert to a CString", self.name))?;
        let flags = match self.get_vis {
            AttrVisiblity::Visible => max_sys::e_max_attrflags::ATTR_FLAGS_NONE,
            //not actually used AttrVisiblity::Opaque => max_sys::e_max_attrflags::ATTR_GET_OPAQUE,
            AttrVisiblity::UserVisible => max_sys::e_max_attrflags::ATTR_GET_OPAQUE_USER,
        } | match self.set_vis {
            AttrVisiblity::Visible => max_sys::e_max_attrflags::ATTR_FLAGS_NONE,
            //not actually used AttrVisiblity::Opaque => max_sys::e_max_attrflags::ATTR_SET_OPAQUE,
            AttrVisiblity::UserVisible => max_sys::e_max_attrflags::ATTR_SET_OPAQUE_USER,
        };
        let inner = unsafe {
            //Absent accessors: for offset attributes pass None, so the SDK installs its default
            //accessors that read/write the field at `offset` in the object. For other attributes
            //keep the no-ops: with NULL accessors `attribute_new` would install the default
            //accessors, which store into memory owned by the (class-shared) attribute, so e.g.
            //`@x 5` on a get-only attribute would silently store a value.
            let is_offset = self.offset.is_some();
            let get = match self.get {
                Some(g) => Some(g),
                None if is_offset => None,
                None => Some(get_nop::<T> as AttrTrampGetMethod<T>),
            };
            let set = match self.set {
                Some(s) => Some(s),
                None if is_offset => None,
                None => Some(set_nop::<T> as AttrTrampSetMethod<T>),
            };
            let get = std::mem::transmute::<Option<AttrTrampGetMethod<T>>, Option<MaxMethod>>(get);
            let set = std::mem::transmute::<Option<AttrTrampSetMethod<T>>, Option<MaxMethod>>(set);
            if let Some(offset) = self.offset {
                max_sys::attr_offset_new(
                    n.as_ptr(),
                    self.val_type.into(),
                    flags as _,
                    get,
                    set,
                    offset as _,
                )
            } else {
                max_sys::attribute_new(
                    n.as_ptr(),
                    std::mem::transmute::<*const max_sys::t_symbol, *mut max_sys::t_symbol>(
                        self.val_type.into(),
                    ), //should have been const in max_sys
                    flags as _,
                    get,
                    set,
                )
            }
        };
        if inner.is_null() {
            return Err("failed to create attribute".into());
        }
        //apply clip
        MaxError::from(
            unsafe {
                match self.clip {
                    AttrClip::None => max_sys::e_max_errorcodes::MAX_ERR_NONE as max_sys::t_max_err,
                    AttrClip::Get(c) => {
                        let p: ClipParams = c.into();
                        max_sys::attr_addfilterget_clip(
                            inner as _, p.min, p.max, p.use_min, p.use_max,
                        )
                    }
                    AttrClip::Set(c) => {
                        let p: ClipParams = c.into();
                        max_sys::attr_addfilterset_clip(
                            inner as _, p.min, p.max, p.use_min, p.use_max,
                        )
                    }
                    AttrClip::GetSet(c) => {
                        let p: ClipParams = c.into();
                        max_sys::attr_addfilter_clip(inner as _, p.min, p.max, p.use_min, p.use_max)
                    }
                }
            } as _,
            (),
        )
        .map_err(|e| format!("error {:?} setting clip", e))?;
        Ok(Attr {
            inner,
            name: self.name.clone(),
            val_type: self.val_type,
            options: self.options,
            _phantom: PhantomData,
        })
    }
}

pub enum AttrAccess<T> {
    Offset(usize),
    OffsetGetMethod(usize, AttrTrampGetMethod<T>),
    OffsetSetMethod(usize, AttrTrampSetMethod<T>),
    GetSetMethod(AttrTrampGetMethod<T>, AttrTrampSetMethod<T>),
}

//p_sym_char (char), _sym_long (long), _sym_float32 (32-bit float), _sym_float64 (64-bit float), _sym_atom (Max t_atom pointer), _sym_symbol (Max t_symbol pointer), _sym_pointer (generic pointer) and _sym_object (Max t_object pointer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrType {
    Char,
    Int64, //long
    Float32,
    Float64,
    AtomPtr,
    SymbolRef,
    Ptr,
    ObjectPtr,
}

#[derive(Debug, Clone, Copy)]
pub enum AttrValClip {
    /// clip any value below the given to the given value.
    Min(f64),
    /// clip any value above the given to the given value.
    Max(f64),
    /// clip any value to be above the first and below the second, inclusive.
    MinMax(f64, f64),
}

#[derive(Debug, Clone, Copy)]
pub enum AttrClip {
    None,
    //Only clip get
    Get(AttrValClip),
    //Only clip set
    Set(AttrValClip),
    //Clip both get and set
    GetSet(AttrValClip),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrVisiblity {
    /// accessable from gui and code
    Visible,
    /// only accessable from code
    UserVisible,
    // not accessable from code or gui
    // not actually usedOpaque,
}

#[derive(Debug, Clone, Copy)]
struct ClipParams {
    pub min: f64,
    pub max: f64,
    pub use_min: c_long,
    pub use_max: c_long,
}

impl Into<*const max_sys::t_symbol> for AttrType {
    fn into(self) -> *const max_sys::t_symbol {
        let sym = common_symbols();
        match self {
            Self::Char => sym.s_char,
            Self::Int64 => sym.s_long,
            Self::Float32 => sym.s_float32,
            Self::Float64 => sym.s_float64,
            Self::AtomPtr => sym.s_atom,
            Self::SymbolRef => sym.s_symbol,
            Self::Ptr => sym.s_pointer,
            Self::ObjectPtr => sym.s_object,
        }
    }
}

impl Into<ClipParams> for AttrValClip {
    fn into(self) -> ClipParams {
        let mut p = ClipParams {
            min: 0f64,
            max: 0f64,
            use_min: 0,
            use_max: 0,
        };
        match self {
            Self::Min(v) => {
                p.min = v;
                p.use_min = 1;
            }
            Self::Max(v) => {
                p.max = v;
                p.use_max = 1;
            }
            Self::MinMax(min, max) => {
                p.min = min;
                p.max = max;
                p.use_min = 1;
                p.use_max = 1;
            }
        };
        p
    }
}

impl<T> Into<*mut max_sys::t_object> for Attr<T> {
    fn into(self) -> *mut max_sys::t_object {
        self.inner
    }
}

/// Indicate that an attribute has had a change (outside of its setter).
///
/// # Arguments
/// * `owner` - the object that owns the attribute
/// * `name` - the name of the attribute
///
/// # Safety
/// `owner` must point to a valid Max object. For a safe version that touches an attribute of
/// `self` see [`crate::object::ObjRef::attr_touch_with_name`].
pub unsafe fn touch_with_name<I: Into<SymbolRef>>(
    owner: *mut max_sys::t_object,
    name: I,
) -> MaxResult<()> {
    MaxError::from(
        unsafe { max_sys::object_attr_touch(owner, name.into().as_ptr()) as _ },
        (),
    )
}

/// handle the boiler plate of dealing with attribute atoms
///
/// If `ac` or `av` is null nothing is done (the getter isn't called) and `MAX_ERR_NONE` is
/// returned, as the SDK's own accessors do (`attrtester.c`).
///
/// # Safety
/// `ac` and `av` must be the out-parameters Max passes to an attribute getter: each null, or
/// `ac` pointing to a valid `c_long` and `av` to a valid `*mut t_atom` that is either null or
/// points to at least `*ac` atoms (if `*ac >= 1`) that Max or the caller owns. When a new atom
/// is allocated for the result, ownership passes to the caller as Max expects.
pub unsafe fn get<T, F>(
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
    getter: F,
) -> max_sys::t_max_err
where
    F: Fn() -> T,
    T: Into<Atom>,
{
    if ac.is_null() || av.is_null() {
        return max_sys::e_max_errorcodes::MAX_ERR_NONE as _;
    }
    //before touching `ac`/`av`, so a panicking getter leaves no uninitialised atom behind
    let value: Atom = getter().into();
    unsafe {
        if *ac < 1 || (*av).is_null() {
            *ac = 1;
            *av = max_sys::sysmem_newptr(std::mem::size_of::<max_sys::t_atom>() as _) as _;
            if (*av).is_null() {
                *ac = 0;
                return max_sys::e_max_errorcodes::MAX_ERR_OUT_OF_MEM as _;
            }
        }
        let s: &mut Atom = std::mem::transmute::<_, _>(*av);
        s.assign(value);
    }
    max_sys::e_max_errorcodes::MAX_ERR_NONE as _
}

/// handle the boiler plate of dealing with attribute atoms
///
/// # Safety
/// If `ac > 0`, `av` must point to at least one valid, initialized atom (as Max passes to an
/// attribute setter) that is valid for the duration of the call.
pub unsafe fn set<T, F>(ac: c_long, av: *mut max_sys::t_atom, setter: F) -> max_sys::t_max_err
where
    F: Fn(T),
    T: for<'b> From<&'b Atom>,
{
    unsafe {
        if ac > 0 && !av.is_null() {
            //transparent so this is okay
            let a: &Atom = std::mem::transmute::<_, _>(&*av);
            setter(a.into());
        }
    }
    max_sys::e_max_errorcodes::MAX_ERR_NONE as _
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for f64 {}
    impl Sealed for f32 {}
    impl Sealed for max_sys::t_atom_long {}
    impl Sealed for crate::symbol::SymbolRef {}
}

/// A value type an attribute can have: `f64` (`float64`), `f32` (`float32`), `t_atom_long`
/// (`long`) or [`SymbolRef`] (`symbol`). It decides the attribute type registered with Max and how
/// the value is converted to and from the atom that Max passes to the accessors.
///
/// This trait is sealed.
pub trait AttrValue: Sized + sealed::Sealed + 'static {
    /// The attribute type registered with Max.
    const ATTR_TYPE: AttrType;
    /// The value as an atom, for the getter.
    fn to_atom(&self) -> Atom;
    /// The value from the first atom given to the setter. Numbers convert as with
    /// `atom_getfloat`/`atom_getlong`.
    fn from_atom(atom: &Atom) -> Self;
}

impl AttrValue for f64 {
    const ATTR_TYPE: AttrType = AttrType::Float64;
    fn to_atom(&self) -> Atom {
        Atom::from(*self)
    }
    fn from_atom(atom: &Atom) -> Self {
        atom.get_float()
    }
}

impl AttrValue for f32 {
    const ATTR_TYPE: AttrType = AttrType::Float32;
    fn to_atom(&self) -> Atom {
        Atom::from(*self as f64)
    }
    fn from_atom(atom: &Atom) -> Self {
        atom.get_float() as f32
    }
}

impl AttrValue for max_sys::t_atom_long {
    const ATTR_TYPE: AttrType = AttrType::Int64;
    fn to_atom(&self) -> Atom {
        Atom::from(*self)
    }
    fn from_atom(atom: &Atom) -> Self {
        atom.get_int()
    }
}

impl AttrValue for SymbolRef {
    const ATTR_TYPE: AttrType = AttrType::SymbolRef;
    fn to_atom(&self) -> Atom {
        Atom::from(self)
    }
    fn from_atom(atom: &Atom) -> Self {
        atom.get_symbol()
    }
}

/// A field type that can back an attribute: `#[attr]` on a field with `#[derive(MaxObject)]`, or
/// [`Class::add_field_attr`](crate::class::Class::add_field_attr) by hand. The accessors take
/// `&self`, because Max reads and writes attributes on the main and scheduler threads while
/// other methods (and `perform`) run, so the field needs interior mutability that is safe to
/// share: the [`num`](crate::num) atomics implement this.
pub trait AttrField: Sync {
    /// The attribute's value type, which also decides its Max attribute type.
    type Value: AttrValue;
    /// The current value, for the attribute getter.
    fn get(&self) -> Self::Value;
    /// Store a value given to the attribute setter (after clipping, if the attribute clips).
    fn set(&self, value: Self::Value);
}

impl AttrField for Int64 {
    type Value = max_sys::t_atom_long;
    fn get(&self) -> Self::Value {
        Int64::get(self)
    }
    fn set(&self, value: Self::Value) {
        Int64::set(self, value)
    }
}

impl AttrField for Float64 {
    type Value = f64;
    fn get(&self) -> Self::Value {
        Float64::get(self)
    }
    fn set(&self, value: Self::Value) {
        Float64::set(self, value)
    }
}

impl AttrField for Float32 {
    type Value = f32;
    fn get(&self) -> Self::Value {
        Float32::get(self)
    }
    fn set(&self, value: Self::Value) {
        Float32::set(self, value)
    }
}

/// Options for an attribute registered with [`Class::add_attr`](crate::class::Class::add_attr),
/// [`Class::add_attr_readonly`](crate::class::Class::add_attr_readonly) or
/// [`Class::add_field_attr`](crate::class::Class::add_field_attr) (or given to
/// [`AttrBuilder::options`]), and what the `#[attr(..)]` options of the macros turn into.
///
/// ```
/// use median::attr::AttrOptions;
/// let o = AttrOptions::new().clip(0., 100.);
/// let ro = AttrOptions::new().readonly();
/// let inspector = AttrOptions::new()
///     .label(c"Gain")
///     .category(c"Levels")
///     .order(1)
///     .range(0., 2.)
///     .default(1.)
///     .save();
/// let menu = AttrOptions::new().enum_vals(c"sine saw \"square wave\"");
/// ```
///
/// Besides `clip` and `readonly` these are attributes of the attribute, for the inspector and the
/// patcher file, added the way the SDK's `CLASS_ATTR_*` macros (`ext_obex_util.h`) add them:
/// `class_attr_addattr_parse` (or `class_attr_addattr_atoms` for the label) once the attribute is
/// in the class.
#[derive(Debug, Clone, Copy)]
pub struct AttrOptions {
    clip: AttrClip,
    readonly: bool,
    pub(crate) label: Option<&'static CStr>,
    pub(crate) style: Option<&'static CStr>,
    pub(crate) category: Option<&'static CStr>,
    pub(crate) order: Option<max_sys::t_atom_long>,
    pub(crate) default: Option<AttrDefault>,
    pub(crate) save: bool,
    pub(crate) enum_vals: Option<&'static CStr>,
    pub(crate) range: Option<(f64, f64)>,
    pub(crate) hidden: bool,
}

/// The default value of an attribute, see [`AttrOptions::default`]: a number, or a parse string
/// (for a symbol attribute, say), from `5`, `0.5` or `c"sine"`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AttrDefault {
    Int(max_sys::t_atom_long),
    Float(f64),
    /// A string Max parses into atoms (`"a b"` is two symbols; quote to keep spaces).
    Parse(&'static CStr),
}

impl From<i32> for AttrDefault {
    fn from(v: i32) -> Self {
        Self::Int(v as max_sys::t_atom_long)
    }
}

impl From<isize> for AttrDefault {
    fn from(v: isize) -> Self {
        Self::Int(v as max_sys::t_atom_long)
    }
}

impl From<i64> for AttrDefault {
    fn from(v: i64) -> Self {
        Self::Int(v as max_sys::t_atom_long)
    }
}

impl From<f64> for AttrDefault {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<f32> for AttrDefault {
    fn from(v: f32) -> Self {
        Self::Float(v.into())
    }
}

impl From<&'static CStr> for AttrDefault {
    fn from(v: &'static CStr) -> Self {
        Self::Parse(v)
    }
}

impl AttrOptions {
    /// No clipping, readable and writable, nothing else.
    pub const fn new() -> Self {
        Self {
            clip: AttrClip::None,
            readonly: false,
            label: None,
            style: None,
            category: None,
            order: None,
            default: None,
            save: false,
            enum_vals: None,
            range: None,
            hidden: false,
        }
    }

    /// Clip values to `min..=max`, when set and when got (`attr_addfilter_clip`, what the SDK's
    /// `CLASS_ATTR_FILTER_CLIP` does).
    pub const fn clip(self, min: f64, max: f64) -> Self {
        self.clip_with(AttrClip::GetSet(AttrValClip::MinMax(min, max)))
    }

    /// Any [`AttrClip`].
    pub const fn clip_with(mut self, clip: AttrClip) -> Self {
        self.clip = clip;
        self
    }

    /// Users can't set the attribute (`ATTR_SET_OPAQUE_USER`); no setter is registered.
    pub const fn readonly(mut self) -> Self {
        self.readonly = true;
        self
    }

    /// The name shown in the inspector (`CLASS_ATTR_LABEL`: the attribute `label`, a symbol,
    /// translated with `gensym_tr` as the macro does).
    pub const fn label(mut self, label: &'static CStr) -> Self {
        self.label = Some(label);
        self
    }

    /// The inspector's editor (`CLASS_ATTR_STYLE`): `"text"`, `"onoff"`, `"rgba"`, `"enum"`,
    /// `"enumindex"`, `"rect"`, `"font"`, `"file"`. Wins over the style `enum_vals` sets.
    pub const fn style(mut self, style: &'static CStr) -> Self {
        self.style = Some(style);
        self
    }

    /// The inspector tab (`CLASS_ATTR_CATEGORY`; the string is parsed, translated with `str_tr`
    /// as the macro does, so quote a category with spaces).
    pub const fn category(mut self, category: &'static CStr) -> Self {
        self.category = Some(category);
        self
    }

    /// The position in the inspector (`CLASS_ATTR_ORDER`); 1 is first, 0 means unordered.
    pub const fn order(mut self, order: max_sys::t_atom_long) -> Self {
        self.order = Some(order);
        self
    }

    /// The default value (`CLASS_ATTR_DEFAULT`, typed as the attribute). The SDK says it is set
    /// automatically at creation only for objects with a dictionary constructor, which median's
    /// objects are not: give the field its initial value in `new` as well. Numbers are written
    /// as the attribute's type: a float attribute gets a float (`5` as `5.0`), a long attribute
    /// a whole number (`0.5` truncated), and an integer stays an integer for every other type
    /// (`atom`, `symbol`, pointers).
    pub fn default(mut self, value: impl Into<AttrDefault>) -> Self {
        self.default = Some(value.into());
        self
    }

    /// Save the attribute's value with the patcher (`CLASS_ATTR_SAVE`). Nothing else is needed
    /// from the class: the SDK's `attrtester` (a plain box class) uses the macro alone, and its
    /// `new` calls `attr_args_process`, as median's does.
    pub const fn save(mut self) -> Self {
        self.save = true;
        self
    }

    /// A menu of choices in the inspector (`CLASS_ATTR_ENUM` for a symbol attribute, whose value
    /// is the chosen symbol, `CLASS_ATTR_ENUMINDEX` for a number attribute, whose value is the
    /// index): the style `enum` or `enumindex` and the attribute `enumvals`, a parse string such
    /// as `c"zero one \"two words\""`.
    pub const fn enum_vals(mut self, vals: &'static CStr) -> Self {
        self.enum_vals = Some(vals);
        self
    }

    /// The range shown in the inspector (`CLASS_ATTR_MIN` and `CLASS_ATTR_MAX`, typed as the
    /// attribute). Values are not limited: that is [`clip`](Self::clip).
    pub const fn range(mut self, min: f64, max: f64) -> Self {
        self.range = Some((min, max));
        self
    }

    /// Hide the attribute from the inspector (`CLASS_ATTR_INVISIBLE`). It can still be set and
    /// got with messages.
    pub const fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    /// Whether the attribute is read-only.
    pub const fn is_readonly(&self) -> bool {
        self.readonly
    }

    /// The clip setting.
    pub const fn clip_value(&self) -> AttrClip {
        self.clip
    }
}

/// A number as a parse string for an attribute of type `ty`: whole for integer types, with a
/// decimal point or exponent for float types (so Max parses it as a float).
fn number_text(v: f64, ty: AttrType) -> String {
    match ty {
        AttrType::Int64 | AttrType::Char => format!("{}", v.trunc() as i64),
        _ => format!("{v:?}"),
    }
}

/// Add the attributes of the attribute `name` of `class` that `options` asks for, as the
/// `CLASS_ATTR_*` macros of `ext_obex_util.h` do (`CLASS_ATTR_LABEL`, `_STYLE`, `_ENUM`/
/// `_ENUMINDEX`, `_CATEGORY`, `_ORDER`, `_DEFAULT`, `_SAVE`, `_MIN`/`_MAX`, `_INVISIBLE`). The
/// macros that are typed as the attribute (`object_method(theattr, gettype)`) get `ty`'s symbol.
///
/// # Safety
/// `class` must be a valid class that has the attribute `name`.
pub(crate) unsafe fn add_attr_attrs(
    class: *mut max_sys::t_class,
    name: &CStr,
    ty: AttrType,
    o: &AttrOptions,
) -> MaxResult<()> {
    let sym = common_symbols();
    let attr_ty: *const max_sys::t_symbol = ty.into();
    let attr_ty = attr_ty as *mut max_sys::t_symbol;
    let parse = |attr2: &CStr, ty: *mut max_sys::t_symbol, text: &CStr| -> MaxResult<()> {
        // SAFETY: the caller's contract; the strings are NUL-terminated and outlive the call
        MaxError::from(
            unsafe {
                max_sys::class_attr_addattr_parse(
                    class,
                    name.as_ptr(),
                    attr2.as_ptr(),
                    ty,
                    0,
                    text.as_ptr(),
                )
            } as _,
            (),
        )
    };
    let text = |s: String| CString::new(s).map_err(|_| MaxError::Generic);
    if let Some(label) = o.label {
        //CLASS_ATTR_ATTR_FORMAT(c, name, "label", _sym_symbol, flags, "s", gensym_tr(label)):
        //one symbol atom
        // SAFETY: `label` is a NUL-terminated string
        let label = unsafe { SymbolRef::from_raw(max_sys::gensym_tr(label.as_ptr())) };
        let mut atom = Atom::from(label);
        // SAFETY: the caller's contract; one valid atom
        MaxError::from(
            unsafe {
                max_sys::class_attr_addattr_atoms(
                    class,
                    name.as_ptr(),
                    c"label".as_ptr(),
                    sym.s_symbol,
                    0,
                    1,
                    &mut atom.value,
                )
            } as _,
            (),
        )?;
    }
    if let Some(vals) = o.enum_vals {
        let style = if ty == AttrType::SymbolRef {
            c"enum"
        } else {
            c"enumindex"
        };
        parse(c"style", sym.s_symbol, style)?;
        parse(c"enumvals", sym.s_atom, vals)?;
    }
    if let Some(style) = o.style {
        parse(c"style", sym.s_symbol, style)?;
    }
    if let Some(category) = o.category {
        // SAFETY: `category` is a NUL-terminated string; `str_tr` returns one (it, or Max's
        // translation, which Max keeps)
        let category = unsafe { CStr::from_ptr(max_sys::str_tr(category.as_ptr())) };
        parse(c"category", sym.s_symbol, category)?;
    }
    if let Some(order) = o.order {
        parse(c"order", sym.s_long, &text(order.to_string())?)?;
    }
    if let Some(default) = o.default {
        let t = match default {
            //a float attribute gets a float ("5.0", so Max parses it as one); every other type
            //(long, char, atom, symbol, pointers) the whole number as written
            AttrDefault::Int(v) if matches!(ty, AttrType::Float64 | AttrType::Float32) => {
                text(number_text(v as f64, ty))?
            }
            AttrDefault::Int(v) => text(v.to_string())?,
            AttrDefault::Float(v) => text(number_text(v, ty))?,
            AttrDefault::Parse(s) => s.to_owned(),
        };
        parse(c"default", attr_ty, &t)?;
    }
    if o.save {
        parse(c"save", sym.s_long, c"1")?;
    }
    if let Some((min, max)) = o.range {
        parse(c"min", attr_ty, &text(number_text(min, ty))?)?;
        parse(c"max", attr_ty, &text(number_text(max, ty))?)?;
    }
    if o.hidden {
        parse(c"invisible", sym.s_long, c"1")?;
    }
    Ok(())
}

impl Default for AttrOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// The attribute getter trampoline for the zero-sized getter `G` (see `Class::add_attr`).
///
/// # Safety
/// Must only be called by Max as an attribute getter of the object `w`, with `ac` and `av` as Max
/// passes them (see [`get`]).
pub(crate) unsafe extern "C" fn get_tramp<W, T, G, V>(
    w: &W,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> max_sys::t_max_err
where
    W: WrapperWrapped<T> + MaxObj,
    G: Fn(&T) -> V + Copy,
    V: AttrValue,
{
    crate::ffi::guard_or(w, "attribute getter", crate::ffi::ERR_GENERIC, || {
        let t = w.wrapped();
        //nowhere to put the value: as the SDK's accessors, do nothing (and don't call the getter)
        if ac.is_null() || av.is_null() {
            return max_sys::e_max_errorcodes::MAX_ERR_NONE as _;
        }
        // SAFETY: `G` is a zero-sized `Copy` callable, see `zst`
        let v = (unsafe { zst::<G>() })(t);
        // SAFETY: the caller guarantees that `ac` and `av` are what Max passes to a getter
        unsafe { get(ac, av, || v.to_atom()) }
    })
}

/// The attribute setter trampoline for the zero-sized setter `S` (see `Class::add_attr`). An
/// `Err` from the setter goes to the type's `report_error` hook and Max gets
/// `MAX_ERR_GENERIC`.
///
/// # Safety
/// Must only be called by Max as an attribute setter of the object `w`, with `ac` and `av` as Max
/// passes them (see [`set`]).
pub(crate) unsafe extern "C" fn set_tramp<W, T, S, V, R>(
    w: &W,
    _attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
) -> max_sys::t_max_err
where
    W: WrapperWrapped<T> + MaxObj,
    S: Fn(&T, V) -> R + Copy,
    V: AttrValue,
    R: MethodResult,
{
    crate::ffi::guard_or(w, "attribute setter", crate::ffi::ERR_GENERIC, || {
        let t = w.wrapped();
        if ac < 1 || av.is_null() {
            return max_sys::e_max_errorcodes::MAX_ERR_NONE as _;
        }
        // SAFETY: the caller guarantees `av` points to `ac` (at least one) valid atoms, and
        // `Atom` is `repr(transparent)` over `t_atom`
        let v = V::from_atom(unsafe { &*av.cast::<Atom>() });
        let mut code = max_sys::e_max_errorcodes::MAX_ERR_NONE as max_sys::t_max_err;
        // SAFETY: `S` is a zero-sized `Copy` callable, see `zst`
        (unsafe { zst::<S>() })(t, v).report_if_err(&mut |err| {
            w.report_error(err);
            code = crate::ffi::ERR_GENERIC;
        });
        code
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panicking_getter_leaves_output_untouched() {
        let _lock = median_mock::lock();
        let mut count = 0;
        let mut atoms = std::ptr::null_mut();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // SAFETY: valid output pointers; the getter panics before allocation.
            unsafe {
                get(&mut count, &mut atoms, || -> Atom {
                    panic!("getter failed");
                });
            }
        }));
        assert!(result.is_err());
        assert_eq!(count, 0);
        assert!(atoms.is_null());
    }
}
