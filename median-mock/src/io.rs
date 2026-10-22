//! Inlets and outlets: `outlet_new`, `outlet_append`, `outlet_bang/int/float/list/anything`,
//! `inlet_new`, `intin`, `floatin`, `proxy_new`, `proxy_getinlet`.
//!
//! Outlets record what is sent through them (see [`crate::OutletEvent`]); they are not connected
//! to anything. Like Max, inlets and `outlet_new` outlets are created right to left: the first one
//! created is the rightmost. `outlet_append` outlets are created left to right. The signal inlets
//! of `z_dsp_setup` are the exception: they stay next to the leftmost inlet.

use crate::atoms::{Atom, atoms_from_raw};
use crate::class::current_inlet;
use crate::state::{InletKind, OutletEvent, State, SubKind, state};
use crate::symbols::sym_name;
use crate::thread::current_role;
use max_sys::{t_atom, t_atom_long, t_object, t_outlet, t_symbol};
use std::ffi::{CStr, c_char, c_long, c_short, c_void};

fn cstr_opt(s: *const c_char) -> Option<String> {
    if s.is_null() {
        None
    } else {
        // SAFETY: callers pass NUL-terminated strings
        Some(unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned())
    }
}

fn add_outlet(st: &mut State, x: usize, ty: Option<String>, append: bool) -> *mut c_void {
    if !st.objects.contains_key(&x) {
        st.post_error(x, format!("median-mock: outlet created on {x:#x}, which is not a live object"));
        return std::ptr::null_mut();
    }
    let ptr = st.alloc_sub(SubKind::Outlet { owner: x, ty });
    let outlets = &mut st.objects.get_mut(&x).expect("checked above").outlets;
    if append {
        outlets.push(ptr);
    } else {
        outlets.insert(0, ptr);
    }
    // SAFETY: `x` is a live object, which starts with a `t_object`
    unsafe { (*(x as *mut t_object)).o_outlet = ptr as *mut c_void };
    ptr as *mut c_void
}

/// The inlets of `o` from left to right, not counting the leftmost: the signal inlets made by
/// `z_dsp_setup` first, then the others, the last one created first (see `z_dsp_setup`).
pub(crate) fn inlets_left_to_right(st: &State, o: &crate::state::ObjData) -> Vec<usize> {
    let is_signal = |p: &usize| {
        matches!(
            st.subs.get(p).map(|s| &s.kind),
            Some(SubKind::Inlet {
                kind: InletKind::Signal
            })
        )
    };
    let mut v: Vec<usize> = o.inlets.iter().copied().filter(is_signal).collect();
    v.extend(o.inlets.iter().rev().copied().filter(|p| !is_signal(p)));
    v
}

pub(crate) fn add_inlet(st: &mut State, x: usize, kind: InletKind) -> *mut c_void {
    if !st.objects.contains_key(&x) {
        st.post_error(x, format!("median-mock: inlet created on {x:#x}, which is not a live object"));
        return std::ptr::null_mut();
    }
    let ptr = st.alloc_sub(SubKind::Inlet { kind });
    st.objects.get_mut(&x).expect("checked above").inlets.push(ptr);
    ptr as *mut c_void
}

/// `s` is the outlet's type: `"bang"`, `"int"`, `"float"`, `"list"`, `"signal"`, or null for an
/// outlet that sends anything.
///
/// # Safety
/// `x` must be a live object (a bad one is reported on the console and gives null) and `s` null
/// or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn outlet_new(x: *mut c_void, s: *const c_char) -> *mut c_void {
    add_outlet(&mut state(), x as usize, cstr_opt(s), false)
}

/// The dynamic outlet constructor median uses. The SDK header only declares it; here it creates
/// an outlet whose type is the name of `s2` (or `s1` if `s2` is null), where null or the empty
/// symbol means anything. Unlike `outlet_new`, which makes each new outlet the leftmost, it adds
/// the new outlet to the right of the existing ones, which is how median calls it (its builder
/// specifies outlets from left to right). This follows how median calls it, not documented
/// behaviour.
///
/// # Safety
/// `op` must be a live object, `s1`/`s2` null or symbols.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn outlet_append(
    op: *mut t_object,
    s1: *mut t_symbol,
    s2: *mut t_symbol,
) -> *mut c_void {
    let s = if s2.is_null() { s1 } else { s2 };
    // SAFETY: the caller's contract
    let name = unsafe { sym_name(s) };
    let ty = if name.is_empty() { None } else { Some(name) };
    add_outlet(&mut state(), op as usize, ty, true)
}

fn log_outlet(x: *mut t_outlet, selector: &str, atoms: Vec<Atom>) -> *mut c_void {
    let mut st = state();
    let addr = x as usize;
    let owner = match st.subs.get(&addr) {
        Some(s) if s.alive => match &s.kind {
            SubKind::Outlet { owner, .. } => Some(*owner),
            _ => None,
        },
        _ => None,
    };
    let Some(owner) = owner else {
        st.post_error(0, format!("median-mock: {selector} sent through {addr:#x}, which is not a live outlet"));
        return std::ptr::null_mut();
    };
    let outlets = &st.objects[&owner].outlets;
    let index = outlets.iter().position(|&o| o == addr).unwrap_or(0);
    let event = OutletEvent {
        outlet: addr,
        owner,
        index,
        selector: selector.to_string(),
        atoms,
        role: current_role(),
        time: st.now,
    };
    st.outlet_log.push(event);
    std::ptr::null_mut()
}

/// # Safety
/// `x` must be an outlet from `outlet_new`/`outlet_append` (a bad one is reported on the console).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn outlet_bang(x: *mut t_outlet) -> *mut c_void {
    log_outlet(x, "bang", Vec::new())
}

/// # Safety
/// As `outlet_bang`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn outlet_int(x: *mut t_outlet, n: t_atom_long) -> *mut c_void {
    log_outlet(x, "int", vec![Atom::Long(n as i64)])
}

/// # Safety
/// As `outlet_bang`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn outlet_float(x: *mut t_outlet, f: f64) -> *mut c_void {
    log_outlet(x, "float", vec![Atom::Float(f)])
}

/// # Safety
/// As `outlet_bang`; `av` must be null or valid for `ac` atoms. The selector `s` is ignored, as
/// it is by Max (a list is a list).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn outlet_list(
    x: *mut t_outlet,
    _s: *mut t_symbol,
    ac: c_short,
    av: *mut t_atom,
) -> *mut c_void {
    // SAFETY: the caller's contract
    let atoms = unsafe { atoms_from_raw(ac.max(0) as usize, av) };
    log_outlet(x, "list", atoms)
}

/// # Safety
/// As `outlet_bang`; `s` must be a symbol and `av` null or valid for `ac` atoms.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn outlet_anything(
    x: *mut t_outlet,
    s: *mut t_symbol,
    ac: c_short,
    av: *mut t_atom,
) -> *mut c_void {
    // SAFETY: the caller's contract
    let (sel, atoms) = unsafe { (sym_name(s), atoms_from_raw(ac.max(0) as usize, av)) };
    log_outlet(x, &sel, atoms)
}

/// A generic inlet. The message `s` (null for any) is what messages arriving there are delivered
/// as; see [`crate::Kernel::send_inlet`].
///
/// # Safety
/// `x` must be a live object, `s` null or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn inlet_new(x: *mut c_void, s: *const c_char) -> *mut c_void {
    add_inlet(&mut state(), x as usize, InletKind::Named(cstr_opt(s)))
}

/// An inlet whose messages go to the method `in<n>`.
///
/// # Safety
/// `x` must be a live object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn intin(x: *mut c_void, n: c_short) -> *mut c_void {
    add_inlet(&mut state(), x as usize, InletKind::Int(n))
}

/// An inlet whose messages go to the method `ft<n>`.
///
/// # Safety
/// `x` must be a live object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn floatin(x: *mut c_void, n: c_short) -> *mut c_void {
    add_inlet(&mut state(), x as usize, InletKind::Float(n))
}

/// # Safety
/// `x` must be a live object; `stuffloc` null or valid for writing a `long` whenever a message
/// arrives at this inlet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn proxy_new(
    x: *mut c_void,
    id: c_long,
    stuffloc: *mut c_long,
) -> *mut c_void {
    add_inlet(
        &mut state(),
        x as usize,
        InletKind::Proxy {
            id: id as i64,
            stuffloc: stuffloc as usize,
        },
    )
}

/// The `id` of the proxy that is delivering the current message, 0 for the leftmost inlet. Only
/// meaningful inside a method called through [`crate::Kernel::send_inlet`] on the same thread.
///
/// # Safety
/// No requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn proxy_getinlet(_master: *mut t_object) -> c_long {
    current_inlet() as c_long
}
