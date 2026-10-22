//! The main thread queue: `defer`, `defer_low`, `qelem_*` (`qelem_set` queues at the back,
//! `qelem_front` at the front).
//!
//! Queued work runs when a test calls [`crate::Kernel::run_main_queue`].

use crate::atoms::{Atom, atoms_from_raw};
use crate::class::{Raw, call_raw};
use crate::state::{Deferred, SubKind, state};
use crate::symbols::intern;
use crate::thread::{Role, current_role, with_role};
use max_sys::{method, t_atom, t_qelem, t_symbol};
use std::ffi::{c_short, c_void};

fn run_call(obj: usize, fun: usize, sym: usize, atoms: &mut [t_atom]) {
    // SAFETY: `fun` was registered as a method with the `(x, sym, argc, argv)` signature
    unsafe {
        call_raw(
            fun,
            &[
                Raw::I(obj),
                Raw::I(sym),
                Raw::I(atoms.len()),
                Raw::I(atoms.as_mut_ptr() as usize),
            ],
        );
    }
}

fn queue_call(ob: *mut c_void, fn_: method, sym: *mut t_symbol, argc: c_short, argv: *mut t_atom) {
    let Some(fun) = fn_ else { return };
    // SAFETY: the caller of `defer*` guarantees `argv` is valid for `argc` atoms
    let atoms = unsafe { atoms_from_raw(argc.max(0) as usize, argv) };
    state().queue.push_back(Deferred::Call {
        obj: ob as usize,
        fun: fun as usize,
        sym: sym as usize,
        atoms,
    });
}

/// From the scheduler thread the call is queued for the main thread; from anywhere else it runs
/// immediately, with the caller's arguments (as the SDK documents).
///
/// # Safety
/// `argv` must be null or valid for `argc` atoms; `fn_` must be a method taking `(x, s, argc,
/// argv)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn defer(
    ob: *mut c_void,
    fn_: method,
    sym: *mut t_symbol,
    argc: c_short,
    argv: *mut t_atom,
) -> *mut c_void {
    if current_role() == Role::Timer {
        queue_call(ob, fn_, sym, argc, argv);
    } else if let Some(fun) = fn_ {
        let n = argc.max(0) as usize;
        let args: &mut [t_atom] = if argv.is_null() {
            &mut []
        } else {
            // SAFETY: the caller's contract
            unsafe { std::slice::from_raw_parts_mut(argv, n) }
        };
        run_call(ob as usize, fun as usize, sym as usize, args);
    }
    std::ptr::null_mut()
}

/// Always queues.
///
/// # Safety
/// As `defer`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn defer_low(
    ob: *mut c_void,
    fn_: method,
    sym: *mut t_symbol,
    argc: c_short,
    argv: *mut t_atom,
) -> *mut c_void {
    queue_call(ob, fn_, sym, argc, argv);
    std::ptr::null_mut()
}

/// # Safety
/// `obj` is passed to `fn_` when the qelem runs and must outlive the qelem.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qelem_new(obj: *mut c_void, fn_: method) -> *mut c_void {
    let Some(fun) = fn_ else {
        return std::ptr::null_mut();
    };
    state().alloc_sub(SubKind::Qelem {
        obj: obj as usize,
        fun: fun as usize,
        set: false,
    }) as *mut c_void
}

/// Queues the qelem unless it is already queued.
///
/// # Safety
/// `x` must be a qelem from `qelem_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qelem_set(x: *mut t_qelem) {
    let mut st = state();
    let addr = x as usize;
    let Some(s) = st.subs.get_mut(&addr) else {
        return;
    };
    if let SubKind::Qelem { set, .. } = &mut s.kind {
        if s.alive && !*set {
            *set = true;
            st.queue.push_back(Deferred::Qelem(addr));
        }
    }
}

/// Like `qelem_set`, but the qelem goes to the front of the main queue. An already queued qelem
/// stays where it is (as `qelem_set`, it is not set again).
///
/// # Safety
/// As `qelem_set`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qelem_front(x: *mut t_qelem) {
    let mut st = state();
    let addr = x as usize;
    let Some(s) = st.subs.get_mut(&addr) else {
        return;
    };
    if let SubKind::Qelem { set, .. } = &mut s.kind {
        if s.alive && !*set {
            *set = true;
            st.queue.push_front(Deferred::Qelem(addr));
        }
    }
}

/// # Safety
/// As `qelem_set`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qelem_unset(x: *mut t_qelem) {
    if let Some(s) = state().subs.get_mut(&(x as usize)) {
        if let SubKind::Qelem { set, .. } = &mut s.kind {
            *set = false;
        }
    }
}

/// Freeing something that is not a live qelem (twice, say) is reported on the console.
///
/// # Safety
/// As `qelem_set`; `x` must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qelem_free(x: *mut t_qelem) {
    let live = matches!(
        state().subs.get(&(x as usize)),
        Some(s) if s.alive && matches!(s.kind, SubKind::Qelem { .. })
    );
    if live {
        crate::class::kill_sub(x as usize);
    } else {
        state().post_error(
            0,
            format!("median-mock: qelem_free of {:#x}, which is not a live qelem", x as usize),
        );
    }
}

/// Run queued calls and qelems on the calling thread (with the main role), first in first out,
/// including ones queued while running, until the queue is empty. Returns how many ran.
pub(crate) fn run_main_queue() -> usize {
    let mut ran = 0;
    with_role(Role::Main, || {
        loop {
            let Some(item) = state().queue.pop_front() else {
                break;
            };
            match item {
                Deferred::Call {
                    obj,
                    fun,
                    sym,
                    atoms,
                } => {
                    let mut raw: Vec<t_atom> = atoms.iter().map(Atom::to_raw).collect();
                    let sym = if sym == 0 { intern(b"") as usize } else { sym };
                    run_call(obj, fun, sym, &mut raw);
                    ran += 1;
                }
                Deferred::Qelem(addr) => {
                    let due = {
                        let mut st = state();
                        match st.subs.get_mut(&addr) {
                            Some(s) if s.alive => match &mut s.kind {
                                SubKind::Qelem { obj, fun, set } if *set => {
                                    *set = false;
                                    Some((*obj, *fun))
                                }
                                _ => None,
                            },
                            _ => None,
                        }
                    };
                    if let Some((obj, fun)) = due {
                        // SAFETY: `fun` was registered as a qelem method, which takes the object
                        unsafe {
                            std::mem::transmute::<
                                usize,
                                unsafe extern "C" fn(*mut c_void) -> *mut c_void,
                            >(fun)(obj as *mut c_void);
                        }
                        ran += 1;
                    }
                }
            }
        }
    });
    ran
}
