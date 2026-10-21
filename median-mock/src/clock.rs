//! Clocks and the fake scheduler.
//!
//! Logical time starts at 0 and moves only when a test calls [`crate::Kernel::advance`], which
//! fires the due clocks in time order (ties in the order they were scheduled) on the calling
//! thread, with the thread's role set to [`crate::Role::Timer`]. A clock that is rescheduled
//! while it is firing, or by another clock that fires, is picked up in the same `advance` if it
//! is due by then.

use crate::state::{SubKind, state};
use crate::thread::{Role, with_role};
use max_sys::{method, t_clock, t_object, t_scheduler};
use std::ffi::{c_long, c_void};

fn make_clock(obj: *mut c_void, fun: method) -> *mut t_clock {
    let Some(fun) = fun else {
        return std::ptr::null_mut();
    };
    state().alloc_sub(SubKind::Clock {
        obj: obj as usize,
        fun: fun as usize,
        fire: None,
        seq: 0,
    }) as *mut t_clock
}

/// # Safety
/// `obj` is passed to `fn_` when the clock fires; it must be valid until the clock is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn clock_new(obj: *mut c_void, fn_: method) -> *mut t_clock {
    make_clock(obj, fn_)
}

/// There is one scheduler in the mock; `s` is ignored.
///
/// # Safety
/// As `clock_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn clock_new_withscheduler(
    obj: *mut c_void,
    fn_: method,
    _s: *mut t_scheduler,
) -> *mut t_clock {
    make_clock(obj, fn_)
}

/// Objects here have no scheduler of their own, so this is always null (the global scheduler).
///
/// # Safety
/// No requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scheduler_fromobject(_obj: *mut t_object) -> *mut t_scheduler {
    std::ptr::null_mut()
}

fn schedule(x: *mut t_clock, delay: f64) {
    let mut st = state();
    let now = st.now;
    st.seq += 1;
    let new_seq = st.seq;
    match st.subs.get_mut(&(x as usize)) {
        Some(s) if s.alive => {
            if let SubKind::Clock { fire, seq, .. } = &mut s.kind {
                *fire = Some(now + delay.max(0.0));
                *seq = new_seq;
                return;
            }
        }
        _ => {}
    }
    st.post_error(0, format!("median-mock: clock_delay on {:#x}, which is not a live clock", x as usize));
}

/// Schedules the clock `n` ms from now, replacing an earlier schedule.
///
/// # Safety
/// `x` must be a clock from `clock_new` (a bad one is reported on the console).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn clock_delay(x: *mut t_clock, n: c_long) {
    schedule(x, n as f64);
}

/// # Safety
/// As `clock_delay`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn clock_fdelay(x: *mut t_clock, f: f64) {
    schedule(x, f);
}

/// # Safety
/// As `clock_delay`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn clock_unset(x: *mut t_clock) {
    if let Some(s) = state().subs.get_mut(&(x as usize)) {
        if let SubKind::Clock { fire, .. } = &mut s.kind {
            *fire = None;
        }
    }
}

/// # Safety
/// `time` must be valid for writing a `double`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn clock_getftime(time: *mut f64) {
    if !time.is_null() {
        // SAFETY: the caller's contract
        unsafe { time.write(state().now) };
    }
}

/// # Safety
/// No requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gettime() -> c_long {
    state().now as c_long
}

/// Frees a clock (or anything else `object_free` frees).
///
/// # Safety
/// As `object_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn freeobject(op: *mut c_void) {
    // SAFETY: the caller's contract
    let _ = unsafe { crate::class::object_free(op) };
}

/// Move time forward by `ms`, firing everything that falls due. Returns how many clocks fired.
pub(crate) fn advance(ms: f64) -> usize {
    let target = state().now + ms.max(0.0);
    let mut fired = 0usize;
    loop {
        let next = {
            let mut st = state();
            let due = st
                .subs
                .iter()
                .filter_map(|(&addr, s)| match &s.kind {
                    SubKind::Clock {
                        fire: Some(t), seq, ..
                    } if s.alive && *t <= target => Some((*t, *seq, addr)),
                    _ => None,
                })
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            due.map(|(t, _, addr)| {
                if t > st.now {
                    st.now = t;
                }
                let Some(s) = st.subs.get_mut(&addr) else {
                    unreachable!("found above")
                };
                let SubKind::Clock { fire, obj, fun, .. } = &mut s.kind else {
                    unreachable!("found above")
                };
                *fire = None;
                (*obj, *fun)
            })
        };
        let Some((obj, fun)) = next else { break };
        fired += 1;
        assert!(
            fired < 10_000_000,
            "median-mock: more than 10 million clock callbacks in one advance; a clock that reschedules itself with delay 0?"
        );
        with_role(Role::Timer, || {
            // SAFETY: `fun` was registered as a clock method, which takes the object
            unsafe {
                std::mem::transmute::<usize, unsafe extern "C" fn(*mut c_void) -> *mut c_void>(fun)(
                    obj as *mut c_void,
                );
            }
        });
    }
    let mut st = state();
    if target > st.now {
        st.now = target;
    }
    fired
}
