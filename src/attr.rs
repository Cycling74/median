//! Attributes.

use crate::atom::Atom;
use std::os::raw::c_long;

//p_sym_char (char), _sym_long (long), _sym_float32 (32-bit float), _sym_float64 (64-bit float), _sym_atom (Max t_atom pointer), _sym_symbol (Max t_symbol pointer), _sym_pointer (generic pointer) and _sym_object (Max t_object pointer).
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

pub trait ClippableAttribute {}

impl ClippableAttribute for f32 {}
impl ClippableAttribute for f64 {}
impl ClippableAttribute for i64 {}

#[derive(Debug, Clone, Copy)]
pub enum AttrClip {
    None,
    /// clip any value below the given to the given value.
    Min(f64),
    /// clip any value above the given to the given value.
    Max(f64),
    /// clip any value to be above the first and below the second, inclusive.
    MinMax(f64, f64),
}

#[derive(Debug, Clone, Copy)]
pub enum AttrVisiblity<T> {
    /// not accessable from code or gui
    Opaque,
    /// accessable from gui and code
    Visible(T),
    /// only accessable from code
    UserVisible(T),
}

/// handler the boiler plate of dealing with attribute atoms
pub fn get<T, F>(ac: *mut c_long, av: *mut *mut max_sys::t_atom, getter: F) -> max_sys::t_max_err
where
    F: Fn() -> T,
    T: Into<Atom>,
{
    unsafe {
        if *ac < 1 || av.is_null() || (*av).is_null() {
            *ac = 1;
            *av = max_sys::sysmem_newptr(std::mem::size_of::<max_sys::t_atom>() as _) as _;
            if (*av).is_null() {
                *ac = 0;
                return max_sys::e_max_errorcodes::MAX_ERR_OUT_OF_MEM as _;
            }
        }
        let s: &mut Atom = std::mem::transmute::<_, _>(*av);
        s.assign(getter().into());
    }
    max_sys::e_max_errorcodes::MAX_ERR_NONE as _
}

pub fn set<'a, T, F>(
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
    setter: F,
) -> max_sys::t_max_err
where
    F: Fn(T),
    T: From<&'a Atom>,
{
    unsafe {
        if *ac > 0 && !(*av).is_null() {
            //transparent so this is okay
            let a: &Atom = std::mem::transmute::<_, _>(*av);
            setter(a.into());
        }
    }
    max_sys::e_max_errorcodes::MAX_ERR_NONE as _
}
