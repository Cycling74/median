//! Atoms, typed datum.
//!
//! see [cycling 74 docs](https://cycling74.com/sdk/max-sdk-8.0.3/html/group__atom.html)

use crate::symbol::SymbolRef;
use core::ffi::c_void;

/// The type of data that an atom stores.
#[derive(Copy, Clone, Debug)]
pub enum AtomType {
    Int,
    Float,
    Symbol,
    Object,
}

/// Typed atom data.
pub enum AtomValue {
    Int(i64),
    Float(f64),
    Symbol(SymbolRef),
    Object(*mut c_void),
}

#[repr(transparent)]
pub struct Atom {
    pub(crate) value: max_sys::t_atom,
}

impl Atom {
    pub fn get_type(&self) -> Option<AtomType> {
        unsafe {
            let t = max_sys::atom_gettype(&self.value as _) as max_sys::e_max_atomtypes::Type;
            match t {
                max_sys::e_max_atomtypes::A_LONG => Some(AtomType::Int),
                max_sys::e_max_atomtypes::A_FLOAT => Some(AtomType::Float),
                max_sys::e_max_atomtypes::A_SYM => Some(AtomType::Symbol),
                max_sys::e_max_atomtypes::A_OBJ => Some(AtomType::Object),
                _ => None,
            }
        }
    }

    pub fn get_value(&self) -> Option<AtomValue> {
        match self.get_type() {
            Some(AtomType::Int) => Some(AtomValue::Int(self.get_int())),
            Some(AtomType::Float) => Some(AtomValue::Float(self.get_float())),
            Some(AtomType::Symbol) => Some(AtomValue::Symbol(self.get_symbol())),
            Some(AtomType::Object) => Some(AtomValue::Object(self.get_obj())),
            None => None,
        }
    }

    pub fn assign(&mut self, other: Self) {
        self.value = other.value;
    }

    pub fn get_int(&self) -> i64 {
        unsafe { max_sys::atom_getlong(&self.value) }
    }

    pub fn get_float(&self) -> f64 {
        unsafe { max_sys::atom_getfloat(&self.value) }
    }

    pub fn get_symbol(&self) -> SymbolRef {
        unsafe { max_sys::atom_getsym(&self.value).into() }
    }

    pub fn get_obj(&self) -> *mut c_void {
        unsafe { max_sys::atom_getobj(&self.value) }
    }

    pub fn set_int(&mut self, v: i64) {
        unsafe {
            max_sys::atom_setlong(&mut self.value, v);
        }
    }

    pub fn set_float(&mut self, v: f64) {
        unsafe {
            max_sys::atom_setfloat(&mut self.value, v);
        }
    }

    pub fn set_symbol(&mut self, v: SymbolRef) {
        unsafe {
            max_sys::atom_setsym(&mut self.value, v.into());
        }
    }

    pub fn set_obj(&mut self, v: *mut c_void) {
        unsafe {
            max_sys::atom_setobj(&mut self.value, v);
        }
    }

    unsafe fn zeroed() -> Self {
        Self {
            value: std::mem::MaybeUninit::<max_sys::t_atom>::zeroed().assume_init(),
        }
    }
}

impl From<i64> for Atom {
    fn from(v: i64) -> Self {
        unsafe {
            let mut s = Self::zeroed();
            s.set_int(v);
            s
        }
    }
}

impl From<&i64> for Atom {
    fn from(v: &i64) -> Self {
        unsafe {
            let mut s = Self::zeroed();
            s.set_int(*v);
            s
        }
    }
}

impl From<f64> for Atom {
    fn from(v: f64) -> Self {
        unsafe {
            let mut s = Self::zeroed();
            s.set_float(v);
            s
        }
    }
}

impl From<&f64> for Atom {
    fn from(v: &f64) -> Self {
        unsafe {
            let mut s = Self::zeroed();
            s.set_float(*v);
            s
        }
    }
}

impl From<SymbolRef> for Atom {
    fn from(v: SymbolRef) -> Self {
        unsafe {
            let mut s = Self::zeroed();
            s.set_symbol(v);
            s
        }
    }
}

impl From<&SymbolRef> for Atom {
    fn from(v: &SymbolRef) -> Self {
        unsafe {
            let mut s = Self::zeroed();
            s.set_symbol(v.clone());
            s
        }
    }
}

impl From<*mut c_void> for Atom {
    fn from(v: *mut c_void) -> Self {
        unsafe {
            let mut s = Self::zeroed();
            s.set_obj(v);
            s
        }
    }
}

impl From<AtomValue> for Atom {
    fn from(v: AtomValue) -> Self {
        unsafe {
            let mut s = Self::zeroed();
            match v {
                AtomValue::Int(v) => s.set_int(v),
                AtomValue::Float(v) => s.set_float(v),
                AtomValue::Symbol(v) => s.set_symbol(v),
                AtomValue::Object(v) => s.set_obj(v),
            }
            s
        }
    }
}

impl From<&Atom> for i64 {
    fn from(a: &Atom) -> i64 {
        a.get_int()
    }
}

impl From<&Atom> for f64 {
    fn from(a: &Atom) -> f64 {
        a.get_float()
    }
}

impl From<&Atom> for SymbolRef {
    fn from(a: &Atom) -> SymbolRef {
        a.get_symbol()
    }
}

impl Default for Atom {
    fn default() -> Self {
        Self::from(0i64)
    }
}
