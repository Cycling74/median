//! Jitter Attributes

use crate::{
    atom::Atom,
    jit::{result_wrap, JitResult},
    method::MaxMethod,
    symbol::SymbolRef,
};
use std::{
    ffi::{c_void, CString},
    marker::PhantomData,
    os::raw::c_long,
};

lazy_static::lazy_static! {
    static ref LABEL_LIT: CString = CString::new("label").unwrap();
}

/// A wrapper for a jitter attribute. `T` refers to the object that the attribute is attributed to.
pub struct Attr<T> {
    inner: *mut c_void,
    _phantom: PhantomData<T>,
}

pub type AttrTrampGetMethod<T> = extern "C" fn(
    x: *mut T,
    attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> max_sys::t_jit_err;

pub type AttrTrampSetMethod<T> = extern "C" fn(
    x: *mut T,
    attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
) -> max_sys::t_jit_err;

impl<T> Attr<T> {
    /// Creation
    pub unsafe fn new(inner: *mut c_void) -> Self {
        Self {
            inner,
            _phantom: Default::default(),
        }
    }

    /// Get the raw pointer
    pub fn inner(&self) -> *mut c_void {
        self.inner
    }

    /// Get the name of this attribute
    pub fn name(&self) -> crate::symbol::SymbolRef {
        let s: *mut max_sys::t_symbol =
            unsafe { max_sys::jit_object_method(self.inner, max_sys::_jit_sym_getname) as _ };
        s.into()
    }
}

/// handle the boiler plate of dealing with attribute atoms
pub fn get<W, T, F>(
    attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
    getter: F,
) -> max_sys::t_jit_err
where
    F: Fn(&Attr<W>) -> T,
    T: Into<Atom>,
{
    unsafe {
        let attr = Attr::new(attr);
        if *ac < 1 || (*av).is_null() {
            *ac = 1;
            *av = max_sys::jit_getbytes(std::mem::size_of::<max_sys::t_atom>() as _) as _;
            if (*av).is_null() {
                *ac = 0;
                return max_sys::t_jit_error_code::JIT_ERR_OUT_OF_MEM as _;
            }
        }
        let s: &mut Atom = std::mem::transmute::<_, _>(*av);
        s.assign(getter(&attr).into());
    }
    max_sys::t_jit_error_code::JIT_ERR_NONE as _
}

/// handle the boiler plate of dealing with attribute atoms
pub fn set<'a, W, T, F>(
    attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
    setter: F,
) -> max_sys::t_jit_err
where
    F: Fn(&Attr<W>, T),
    T: From<&'a Atom>,
{
    unsafe {
        let attr = Attr::new(attr);
        if ac > 0 && !av.is_null() {
            //transparent so this is okay
            let a: &Atom = std::mem::transmute::<_, _>(&*av);
            setter(&attr, a.into());
        }
    }

    max_sys::t_jit_error_code::JIT_ERR_NONE as _
}

/// No-op get method for attributes
pub extern "C" fn get_nop<T>(
    _x: *mut T,
    _attr: *mut c_void,
    _ac: *mut c_long,
    _av: *mut *mut max_sys::t_atom,
) -> max_sys::t_jit_err {
    max_sys::t_jit_error_code::JIT_ERR_GENERIC as _
}

/// No-op set method for attributes
pub extern "C" fn set_nop<T>(
    _x: *mut T,
    _attr: *mut c_void,
    _ac: c_long,
    _av: *mut max_sys::t_atom,
) -> max_sys::t_jit_err {
    max_sys::t_jit_error_code::JIT_ERR_GENERIC as _
}

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
    get_sched: AttrSched,
    set_sched: AttrSched,
    label: Option<CString>,
}

impl<T> AttrBuilder<T> {
    // helper method used in public impls
    fn new(name: &str, val_type: AttrType) -> Self {
        Self {
            name: name.into(),
            val_type,
            offset: None,
            get: None,
            set: None,
            clip: AttrClip::None,
            get_vis: AttrVisiblity::Visible,
            set_vis: AttrVisiblity::Visible,
            get_sched: AttrSched::DeferLow,
            set_sched: AttrSched::UsurpLow,
            label: None,
        }
    }

    /// Create a new builder with an offset pointing to a struct member.
    ///
    /// # Arguments
    /// * `name` - the name of the attribute.
    /// * `val_type` - the type of the attribute.
    /// * `offset_bytes` - a byte offset within the struct to the member that represents the attribute.
    pub unsafe fn new_offset(name: &str, val_type: AttrType, offset_bytes: usize) -> Self {
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
    pub unsafe fn new_offset_get(
        name: &str,
        val_type: AttrType,
        offset_bytes: usize,
        get: AttrTrampGetMethod<T>,
    ) -> Self {
        let mut s = Self::new_offset(name, val_type, offset_bytes);
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
    pub unsafe fn new_offset_set(
        name: &str,
        val_type: AttrType,
        offset_bytes: usize,
        set: AttrTrampSetMethod<T>,
    ) -> Self {
        let mut s = Self::new_offset(name, val_type, offset_bytes);
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
    pub fn new_accessors(
        name: &str,
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
    pub fn new_get(name: &str, val_type: AttrType, get: AttrTrampGetMethod<T>) -> Self {
        let mut s = Self::new(name, val_type);
        s.get = Some(get);
        s.set_vis = AttrVisiblity::UserVisible;
        s.set_sched = AttrSched::None;
        s
    }

    /// Create a new builder with only a set method.
    ///
    /// # Arguments
    /// * `name` - the name of the attribute.
    /// * `val_type` - the type of the attribute.
    /// * `set` - a set method to use with the attribute.
    pub fn new_set(name: &str, val_type: AttrType, set: AttrTrampSetMethod<T>) -> Self {
        let mut s = Self::new(name, val_type);
        s.set = Some(set);
        s.get_vis = AttrVisiblity::UserVisible;
        s.get_sched = AttrSched::None;
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
        let mut n = self;
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
        let mut n = self;
        n.set_vis = v;
        n
    }

    /// Set the scheduler behavior for the get for this attribute.
    ///
    /// # Remarks
    /// Defaults to `AttrSched::DeferLow`.
    ///
    /// # Panics
    /// Will panic if called when there is no offset or get method.
    pub fn get_sched(&mut self, v: AttrSched) -> &mut Self {
        assert!(
            self.get.is_some() || self.offset.is_some(),
            "to set get schedule behavior you must have either a get method or an offset"
        );
        let mut n = self;
        n.get_sched = v;
        n
    }

    /// Set the scheduler behavior for the set for this attribute.
    ///
    /// # Remarks
    /// Defaults to `AttrSched::UsurpLow`.
    ///
    /// # Panics
    /// Will panic if called when there is no offset or set method.
    pub fn set_sched(&mut self, v: AttrSched) -> &mut Self {
        assert!(
            self.set.is_some() || self.offset.is_some(),
            "to set set schedule behavior you must have either a set method or an offset"
        );
        let mut n = self;
        n.set_sched = v;
        n
    }

    /// Set the optional clip for this attribute.
    pub fn clip(&mut self, v: AttrClip) -> &mut Self {
        let mut n = self;
        n.clip = v;
        n
    }

    /// Set the optional label for this attribute.
    pub fn label(&mut self, v: &str) -> &mut Self {
        let mut n = self;
        n.label = Some(CString::new(v).expect("label to be valid CString"));
        n
    }

    pub fn build(&self) -> Result<Attr<T>, String> {
        if self.set.is_none() && self.get.is_none() && self.offset.is_none() {
            return Err("you must have at least 1 of get, set or offset".into());
        }
        let n = std::ffi::CString::new(self.name.clone())
            .map_err(|_| format!("{} failed to convert to a CString", self.name))?;
        let flags = match self.get_vis {
            AttrVisiblity::Visible => 0,
            //not actually used AttrVisiblity::Opaque => max_sys::t_jit_attr_flags::JIT_ATTR_GET_OPAQUE,
            AttrVisiblity::UserVisible => max_sys::t_jit_attr_flags::JIT_ATTR_GET_OPAQUE_USER as _,
        } | match self.set_vis {
            AttrVisiblity::Visible => 0,
            //not actually used AttrVisiblity::Opaque => max_sys::t_jit_attr_flags::JIT_ATTR_SET_OPAQUE,
            AttrVisiblity::UserVisible => max_sys::t_jit_attr_flags::JIT_ATTR_SET_OPAQUE_USER as _,
        } | match self.get_sched {
            AttrSched::None => 0,
            AttrSched::DeferLow => max_sys::t_jit_attr_flags::JIT_ATTR_GET_DEFER_LOW as _,
            AttrSched::UsurpLow => max_sys::t_jit_attr_flags::JIT_ATTR_GET_USURP_LOW as _,
        } | match self.set_sched {
            AttrSched::None => 0,
            AttrSched::DeferLow => max_sys::t_jit_attr_flags::JIT_ATTR_SET_DEFER_LOW as _,
            AttrSched::UsurpLow => max_sys::t_jit_attr_flags::JIT_ATTR_SET_USURP_LOW as _,
        };
        let inner = unsafe {
            // set up offset and accessors, potentially no-ops of there is no offset
            let offset = self.offset.unwrap_or(0);
            let get = std::mem::transmute::<Option<AttrTrampGetMethod<T>>, Option<MaxMethod>>(
                self.get.or_else(|| match self.offset {
                    None => Some(get_nop),
                    Some(_) => None,
                }),
            );
            let set = std::mem::transmute::<Option<AttrTrampSetMethod<T>>, Option<MaxMethod>>(
                self.set.or_else(|| match self.offset {
                    None => Some(set_nop),
                    Some(_) => None,
                }),
            );

            let val_sym: *mut max_sys::t_symbol = self.val_type.into();
            max_sys::jit_object_new(
                max_sys::_jit_sym_jit_attr_offset,
                n.as_ptr(),
                val_sym,
                flags as c_long,
                get,
                set,
                offset,
            )
        };
        if inner.is_null() {
            return Err("failed to create attribute".into());
        }
        //apply clip
        result_wrap(
            unsafe {
                match self.clip {
                    AttrClip::None => max_sys::t_jit_error_code::JIT_ERR_NONE as _,
                    AttrClip::Get(c) => {
                        let p: ClipParams = c.into();
                        max_sys::jit_attr_addfilterget_clip(
                            inner as _, p.min, p.max, p.use_min, p.use_max,
                        )
                    }
                    AttrClip::Set(c) => {
                        let p: ClipParams = c.into();
                        max_sys::jit_attr_addfilterset_clip(
                            inner as _, p.min, p.max, p.use_min, p.use_max,
                        )
                    }
                    AttrClip::GetSet(c) => {
                        let p: ClipParams = c.into();
                        max_sys::jit_attr_addfilter_clip(
                            inner as _, p.min, p.max, p.use_min, p.use_max,
                        )
                    }
                }
            } as _,
            (),
        )
        .map_err(|e| format!("error {:?} setting clip", e))?;

        if let Some(label) = &self.label {
            unsafe {
                max_sys::object_addattr_parse(
                    inner as _,
                    LABEL_LIT.as_ptr(),
                    max_sys::_jit_sym_symbol,
                    0,
                    label.as_ptr(),
                );
            }
        }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrSched {
    /// No scheduler flag set
    None,
    /// Defer to low priority thread
    DeferLow,
    /// Defer to low priority thread and only queue 1 call
    UsurpLow,
}

#[derive(Debug, Clone, Copy)]
struct ClipParams {
    pub min: f64,
    pub max: f64,
    pub use_min: c_long,
    pub use_max: c_long,
}

impl Into<*mut max_sys::t_symbol> for AttrType {
    fn into(self) -> *mut max_sys::t_symbol {
        unsafe {
            match self {
                Self::Char => max_sys::_jit_sym_char,
                Self::Int64 => max_sys::_jit_sym_long,
                Self::Float32 => max_sys::_jit_sym_float32,
                Self::Float64 => max_sys::_jit_sym_float64,
                Self::AtomPtr => max_sys::_jit_sym_atom,
                Self::SymbolRef => max_sys::_jit_sym_symbol,
                Self::Ptr => max_sys::_jit_sym_pointer,
                Self::ObjectPtr => max_sys::_jit_sym_object,
            }
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

/// Indicate that an attribute has had a change (outside of its setter).
///
/// # Arguments
/// * `owner` - the object that owns the attribute
/// * `name` - the name of the attribute
pub fn touch_with_name<I: Into<SymbolRef>>(
    owner: *mut max_sys::t_object,
    name: I,
) -> JitResult<()> {
    result_wrap(
        unsafe { max_sys::jit_attr_user_touch(owner as _, name.into().inner()) as _ },
        (),
    )
}
