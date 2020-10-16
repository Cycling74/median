//! Atoms, typed datum.
//!
//! see [cycling 74 docs](https://cycling74.com/sdk/max-sdk-8.0.3/html/group__atom.html)

use crate::symbol::SymbolRef;
use core::ffi::c_void;
use std::cell::UnsafeCell;

#[derive(Copy, Clone, Debug)]
pub enum AtomType {
    Int,
    Float,
    Symbol,
    Object,
}

pub enum AtomValue {
    Int(i64),
    Float(f64),
    Symbol(SymbolRef),
    Object(*mut c_void),
    None,
}

#[repr(transparent)]
pub struct AtomRef {
    pub(crate) value: UnsafeCell<*mut max_sys::t_atom>,
}

impl AtomRef {
    pub fn get_type(&self) -> Option<AtomType> {
        unsafe {
            let t = max_sys::atom_gettype(self.value.get() as _) as max_sys::e_max_atomtypes::Type;
            match t {
                max_sys::e_max_atomtypes::A_LONG => Some(AtomType::Int),
                max_sys::e_max_atomtypes::A_FLOAT => Some(AtomType::Float),
                max_sys::e_max_atomtypes::A_SYM => Some(AtomType::Symbol),
                max_sys::e_max_atomtypes::A_OBJ => Some(AtomType::Object),
                _ => None,
            }
        }
    }

    pub fn get_value(&self) -> AtomValue {
        match self.get_type() {
            Some(AtomType::Int) => AtomValue::Int(self.get_int()),
            Some(AtomType::Float) => AtomValue::Float(self.get_float()),
            Some(AtomType::Symbol) => AtomValue::Symbol(self.get_symbol()),
            Some(AtomType::Object) => AtomValue::Object(self.get_obj()),
            None => AtomValue::None,
        }
    }

    pub fn get_int(&self) -> i64 {
        unsafe { max_sys::atom_getlong(self.value.get() as _) }
    }

    pub fn get_float(&self) -> f64 {
        unsafe { max_sys::atom_getfloat(self.value.get() as _) }
    }

    pub fn get_symbol(&self) -> SymbolRef {
        unsafe { max_sys::atom_getsym(self.value.get() as _).into() }
    }

    pub fn get_obj(&self) -> *mut c_void {
        unsafe { max_sys::atom_getobj(self.value.get() as _) }
    }

    pub fn set_int(&mut self, v: i64) {
        unsafe {
            max_sys::atom_setlong(self.value.get() as _, v);
        }
    }

    pub fn set_float(&mut self, v: f64) {
        unsafe {
            max_sys::atom_setfloat(self.value.get() as _, v);
        }
    }

    pub fn set_symbol(&mut self, v: SymbolRef) {
        unsafe {
            max_sys::atom_setsym(self.value.get() as _, v.into());
        }
    }

    pub fn set_obj(&mut self, v: *mut c_void) {
        unsafe {
            max_sys::atom_setobj(self.value.get() as _, v);
        }
    }
}
