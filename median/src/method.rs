//! Method encapsulation and type aliases
use crate::{atom::Atom, symbol::SymbolRef};
use std::{ffi::c_void, os::raw::c_long};

pub type MaxNew = unsafe extern "C" fn(
    sym: *mut max_sys::t_symbol,
    argc: c_long,
    argv: *const max_sys::t_atom,
) -> *mut c_void;
pub type MaxFree<T> = unsafe extern "C" fn(obj: *mut T);

#[cfg(not(target_os = "windows"))]
pub type MaxMethod = unsafe extern "C" fn(arg1: *mut c_void) -> *mut c_void;

//windows doesnt' use "safe call"
#[cfg(target_os = "windows")]
pub type MaxMethod = unsafe extern "C" fn(arg1: *mut c_void, ...) -> *mut c_void;

pub type B<T> = unsafe extern "C" fn(&T);
pub type SelList<T> =
    unsafe extern "C" fn(&T, *mut max_sys::t_symbol, c_long, *const max_sys::t_atom);

/// helper method to convert between max and median calls selector list method calls
///
/// # Safety
/// * `sym` must be null or point to a valid `t_symbol`.
/// * If `ac > 0`, `av` must point to `ac` valid, initialized atoms that live for the duration of
///   the call.
///
/// These are the guarantees Max makes when it calls a method registered with `A_GIMME`, which is
/// the only intended caller.
pub unsafe fn sel_list<F>(
    sym: *mut max_sys::t_symbol,
    ac: ::std::os::raw::c_long,
    av: *const ::max_sys::t_atom,
    f: F,
) where
    F: Fn(SymbolRef, &[Atom]),
{
    // SAFETY: the caller guarantees `sym` is null or valid
    let sym = unsafe { SymbolRef::from_raw(sym) };
    let atoms = if ac > 0 && !av.is_null() {
        // SAFETY: the caller guarantees `av` points to `ac` atoms, and Atom is transparent
        unsafe {
            std::slice::from_raw_parts(
                std::mem::transmute::<*const ::max_sys::t_atom, *const Atom>(av),
                ac as _,
            )
        }
    } else {
        &[]
    };
    f(sym, atoms);
}

include!(concat!(env!("OUT_DIR"), "/method-gen.rs"));
