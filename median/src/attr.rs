//! Attributes.

use crate::atom::Atom;
use crate::error::{MaxError, MaxResult};
use crate::max::common_symbols;
use crate::method::MaxMethod;
use crate::symbol::SymbolRef;

use std::ffi::c_void;
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
}

impl<T> Attr<T> {
    pub fn inner(&self) -> *mut max_sys::t_object {
        self.inner
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
#[derive(Debug, Clone, Copy)]
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
        unsafe { max_sys::object_attr_touch(owner, name.into().inner()) as _ },
        (),
    )
}

/// handle the boiler plate of dealing with attribute atoms
///
/// # Safety
/// `ac` and `av` must be the (non-null) out-parameters Max passes to an attribute getter: `ac`
/// must point to a valid `c_long`, `av` must point to a valid `*mut t_atom` that is either null
/// or points to at least `*ac` atoms (if `*ac >= 1`) that Max or the caller owns. When a new atom
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panicking_getter_leaves_output_untouched() {
        let mut count = 0;
        let mut atoms = std::ptr::null_mut();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            get(&mut count, &mut atoms, || -> Atom {
                panic!("getter failed");
            });
        }));
        assert!(result.is_err());
        assert_eq!(count, 0);
        assert!(atoms.is_null());
    }
}
