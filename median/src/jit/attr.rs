//! Jitter Attributes

use crate::atom::Atom;
use std::{ffi::c_void, os::raw::c_long};

pub struct Attr {
    inner: *mut c_void,
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

impl Attr {
    /// Creation
    pub unsafe fn new(inner: *mut c_void) -> Self {
        Self { inner }
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
pub fn get<T, F>(
    attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
    getter: F,
) -> max_sys::t_jit_err
where
    F: Fn(&Attr) -> T,
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
pub fn set<'a, T, F>(
    attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
    setter: F,
) -> max_sys::t_jit_err
where
    F: Fn(&Attr, T),
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
