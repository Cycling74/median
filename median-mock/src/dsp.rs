//! MSP: `z_dsp_setup`, `z_dsp_free`, `dsp_add64`.
//!
//! `dsp_add64` records the perform routine (see [`crate::DspAdd`]); a test calls it with buffers
//! through [`crate::Kernel::dsp64`] and [`crate::DspAdd::run`].

use crate::io::add_inlet;
use crate::state::{DspAdd, InletKind, SubKind, state};
use crate::thread::{Role, with_role};
use max_sys::{t_object, t_perfroutine64, t_pxobject};
use std::ffi::{c_long, c_void};

/// Creates `nsignals - 1` signal inlets next to the object's leftmost, as Max does. They stay
/// there when more inlets are made afterwards: `z_dsp.h` says to create the non-signal inlets
/// after `dsp_setup`, which only makes sense if they then go to the right of the signal inlets.
/// (That is the reading of the header the kernel follows; where Max really puts them has not
/// been checked.) See [`crate::Kernel::inlets`].
///
/// # Safety
/// `x` must be a live object whose struct starts with a `t_pxobject`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn z_dsp_setup(x: *mut t_pxobject, nsignals: c_long) {
    let addr = x as usize;
    let mut st = state();
    let Some(obj) = st.objects.get_mut(&addr) else {
        st.post_error(addr, format!("median-mock: z_dsp_setup on {addr:#x}, which is not a live object"));
        return;
    };
    obj.signal_inlets = Some(nsignals.max(0) as usize);
    for _ in 1..nsignals {
        add_inlet(&mut st, addr, InletKind::Signal);
    }
    // SAFETY: `x` is a live object that starts with a `t_pxobject`, which the allocation covers
    unsafe {
        (*x).z_disabled = 0;
        (*x).z_in = 0;
    }
}

/// Recorded (see [`crate::Kernel::dsp_frees`]). Calling it on an object that `z_dsp_setup` was
/// not called on, or twice, is reported on the console.
///
/// # Safety
/// `x` must be an object `z_dsp_setup` was called on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn z_dsp_free(x: *mut t_pxobject) {
    let addr = x as usize;
    let mut st = state();
    st.dsp_frees.push(addr);
    let Some(obj) = st.objects.get_mut(&addr) else {
        st.post_error(addr, format!("median-mock: z_dsp_free on {addr:#x}, which is not a live object"));
        return;
    };
    if obj.signal_inlets.take().is_none() {
        st.post_error(addr, format!("median-mock: z_dsp_free on {addr:#x} without z_dsp_setup"));
        return;
    }
    let obj = st.objects.get_mut(&addr).expect("checked above");
    let inlets = obj.inlets.clone();
    for i in inlets {
        if let Some(s) = st.subs.get_mut(&i) {
            if matches!(
                s.kind,
                SubKind::Inlet {
                    kind: InletKind::Signal,
                    ..
                }
            ) {
                s.alive = false;
            }
        }
    }
}

/// # Safety
/// `chain` and `x` must be the objects passed to the `dsp64` method.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dsp_add64(
    chain: *mut t_object,
    x: *mut t_object,
    f: t_perfroutine64,
    flags: c_long,
    userparam: *mut c_void,
) {
    let Some(f) = f else { return };
    state().dsp_adds.push(DspAdd {
        object: x as usize,
        perform: f as usize,
        flags: flags as i64,
        userparam: userparam as usize,
        dsp64: chain as usize,
    });
}

impl DspAdd {
    /// Call the perform routine on the calling thread (with the audio role) with the given
    /// buffers, `sampleframes` being the length of the longest.
    ///
    /// # Panics
    /// If a buffer is shorter than the longest.
    pub fn run(&self, ins: &mut [Vec<f64>], outs: &mut [Vec<f64>]) {
        let frames = ins.iter().chain(outs.iter()).map(Vec::len).max().unwrap_or(0);
        assert!(
            ins.iter().chain(outs.iter()).all(|b| b.len() == frames),
            "all buffers passed to DspAdd::run must have the same length"
        );
        let mut in_ptrs: Vec<*mut f64> = ins.iter_mut().map(|b| b.as_mut_ptr()).collect();
        let mut out_ptrs: Vec<*mut f64> = outs.iter_mut().map(|b| b.as_mut_ptr()).collect();
        // SAFETY: `perform` was registered through `dsp_add64` as a `t_perfroutine64`, and the
        // pointers are `numins`/`numouts` buffers of `frames` samples
        unsafe {
            let f = std::mem::transmute::<
                usize,
                unsafe extern "C" fn(
                    *mut t_object,
                    *mut t_object,
                    *mut *mut f64,
                    c_long,
                    *mut *mut f64,
                    c_long,
                    c_long,
                    c_long,
                    *mut c_void,
                ),
            >(self.perform);
            with_role(Role::Audio, || {
                f(
                    self.object as *mut t_object,
                    self.dsp64 as *mut t_object,
                    in_ptrs.as_mut_ptr(),
                    in_ptrs.len() as c_long,
                    out_ptrs.as_mut_ptr(),
                    out_ptrs.len() as c_long,
                    frames as c_long,
                    0,
                    self.userparam as *mut c_void,
                )
            });
        }
    }
}
