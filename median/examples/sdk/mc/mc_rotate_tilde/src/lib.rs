//! A port of the SDK's `mc/mc.rotate~`: rotates the channels of a multichannel signal by one,
//! with as many output channels as input channels (MC auto-adapting).
//!
//! median has no multichannel outlet, so the outlet is made raw and is not in median's iolet table:
//! its assist text comes from the `assist` hook. `perform` is handed at most 64 channels each way
//! (median's `MAX_SIGNAL_IO`): with more, median posts an error at `dsp64` and the output is
//! silent, where the SDK object takes up to 512. Output channels the input does not reach (the
//! argument asks for more channels than arrive) are zeroed; the SDK leaves them unwritten.
//! Untested: everything Max does with the outlet type, `Z_MC_INLETS` and the channel protocol;
//! the tests call `inputchanged` and `multichanneloutputs` directly.

use std::{
    borrow::Cow,
    ffi::{c_int, c_long, c_short, c_void},
};

use median::{max_sys, prelude::*};

// From `z_dsp.h`, which max-sys binds without its macros.
const Z_MC_INLETS: c_short = 32;

#[derive(MSPObj)]
#[max(name = "median.mc.rotate~", no_methods)]
pub struct McRotateTilde {
    // The count of input channels, and so of output channels.
    size: Int64,
}

impl MSPObjWrapped for McRotateTilde {
    fn class_setup(class: &mut Class<MSPObjWrapper<Self>>) {
        let c = class.inner();
        // GAP: MC channel protocol (multichanneloutputs, inputchanged)
        // SAFETY: each function has the signature Max calls the `A_CANT` method of that name
        // with; the class is the live class being set up.
        unsafe {
            add_cant(
                c,
                c"multichanneloutputs",
                std::mem::transmute::<
                    unsafe extern "C" fn(*mut MSPObjWrapper<Self>, c_long) -> c_long,
                    unsafe extern "C" fn(*mut c_void) -> *mut c_void,
                >(multichanneloutputs),
            );
            add_cant(
                c,
                c"inputchanged",
                std::mem::transmute::<
                    unsafe extern "C" fn(*mut MSPObjWrapper<Self>, c_long, c_long) -> c_long,
                    unsafe extern "C" fn(*mut c_void) -> *mut c_void,
                >(inputchanged),
            );
        }
    }

    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let size = match builder.args().at(0).unwrap_or(0)? {
            0 => 2,
            n => n.clamp(1, 512),
        };
        builder.inlet(Inlet::signal().assist("(multi-channel signal) Input"));
        // GAP: multichannel signal outlets
        // SAFETY: the object is under construction; the symbol is interned.
        let out = unsafe {
            max_sys::outlet_append(
                builder.max_obj(),
                std::ptr::null_mut(),
                max_sys::gensym(c"multichannelsignal".as_ptr()),
            )
        };
        if out.is_null() {
            return Err(NewError::new("Max did not make the multichannel outlet"));
        }
        // GAP: z_misc flags
        // SAFETY: the object is under construction and nothing else touches its header now.
        unsafe { (*builder.msp_obj()).z_misc |= Z_MC_INLETS };
        Ok(Self {
            size: Int64::new(size),
        })
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let n = ins.len();
        for (ch, input) in ins.iter().enumerate() {
            if let Some(out) = outs.get_mut((ch + 1) % n) {
                out.copy_from_slice(input);
            }
        }
        for out in outs.iter_mut().skip(n) {
            out.fill(0.);
        }
    }

    fn assist(&self, iolet: IoLet) -> Option<Cow<'_, str>> {
        match iolet {
            IoLet::Outlet(0) => Some("(multi-channel signal) Input, Rotated".into()),
            _ => None,
        }
    }
}

impl McRotateTilde {
    fn inputchanged(&self, count: c_long) -> bool {
        let count = count as t_atom_long;
        let changed = count != self.size.get();
        self.size.set(count);
        changed
    }
}

/// # Safety
/// `class` must be a live class; `f` must have the signature Max calls `sel` with.
unsafe fn add_cant(
    class: *mut max_sys::t_class,
    sel: &std::ffi::CStr,
    f: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
) {
    // SAFETY: the caller's contract
    unsafe {
        max_sys::class_addmethod(
            class,
            Some(f),
            sel.as_ptr(),
            max_sys::e_max_atomtypes::A_CANT as c_int,
            0,
        );
    }
}

/// # Safety
/// `x` must be a live `median.mc.rotate~` object.
unsafe extern "C" fn multichanneloutputs(
    x: *mut MSPObjWrapper<McRotateTilde>,
    _index: c_long,
) -> c_long {
    // SAFETY: the caller's contract
    let w = unsafe { &*x };
    median::ffi::guard_or(w, "multichanneloutputs", 1, || {
        w.wrapped().size.get() as c_long
    })
}

/// # Safety
/// `x` must be a live `median.mc.rotate~` object.
unsafe extern "C" fn inputchanged(
    x: *mut MSPObjWrapper<McRotateTilde>,
    _index: c_long,
    count: c_long,
) -> c_long {
    // SAFETY: the caller's contract
    let w = unsafe { &*x };
    median::ffi::guard_or(w, "inputchanged", 0, || {
        w.wrapped().inputchanged(count) as c_long
    })
}

median::ext_main! { McRotateTilde }

#[cfg(test)]
mod tests {
    use super::Z_MC_INLETS;
    use median::max_sys;
    use median_mock::Atom::Long;
    use std::ffi::{c_long, c_void};

    type Outputs = unsafe extern "C" fn(*mut c_void, c_long) -> c_long;
    type Changed = unsafe extern "C" fn(*mut c_void, c_long, c_long) -> c_long;

    fn methods(k: &median_mock::Kernel, x: *mut c_void) -> (Outputs, Changed) {
        let (outputs, _) = k.method(x, "multichanneloutputs").unwrap();
        let (changed, _) = k.method(x, "inputchanged").unwrap();
        // SAFETY: the class registered these two `A_CANT` methods with these signatures.
        unsafe {
            (
                std::mem::transmute::<usize, Outputs>(outputs),
                std::mem::transmute::<usize, Changed>(changed),
            )
        }
    }

    #[test]
    fn makes_a_multichannel_outlet() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.mc.rotate~", &[]);
        assert!(!x.is_null());
        assert_eq!(k.outlet_types(x), [Some("multichannelsignal".to_string())]);
        assert_eq!(k.signal_inlets(x), Some(1));
        // SAFETY: an MSP object starts with its `t_pxobject`.
        let misc = unsafe { (*x.cast::<max_sys::t_pxobject>()).z_misc };
        assert_eq!(misc & (Z_MC_INLETS | 1), Z_MC_INLETS | 1);
        assert_eq!(
            k.assist(x, true, 0).as_deref(),
            Some("(multi-channel signal) Input, Rotated")
        );
        assert_eq!(
            k.assist(x, false, 0).as_deref(),
            Some("(multi-channel signal) Input")
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn adapts_to_the_input_channel_count() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.mc.rotate~", &[]);
        let y = k.new_object("median.mc.rotate~", &[Long(1000)]);
        let (outputs, changed) = methods(&k, x);
        // SAFETY: `x` and `y` are live objects of the class the methods belong to.
        unsafe {
            assert_eq!(outputs(x, 0), 2);
            assert_eq!(outputs(y, 0), 512);
            assert_eq!(changed(x, 0, 2), 0);
            assert_eq!(changed(x, 0, 5), 1);
            assert_eq!(changed(x, 0, 5), 0);
            assert_eq!(outputs(x, 0), 5);
        }
        assert!(k.free_object(x));
        assert!(k.free_object(y));
    }

    #[test]
    fn rotates_by_one() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.mc.rotate~", &[Long(3)]);
        let adds = k.dsp64(x, 44100., 2).unwrap();
        for n in [1, 3, 4] {
            let mut ins: Vec<_> = (0..n).map(|c| vec![c as f64; 2]).collect();
            let mut outs = vec![vec![9.; 2]; n];
            adds[0].run(&mut ins, &mut outs);
            let expected: Vec<_> = (0..n).map(|c| vec![((c + n - 1) % n) as f64; 2]).collect();
            assert_eq!(outs, expected);
        }
        // More outputs than inputs: the rest are silent.
        let mut ins = vec![vec![1.; 2], vec![2.; 2]];
        let mut outs = vec![vec![9.; 2]; 3];
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs, [[2.; 2], [1.; 2], [0.; 2]]);
        assert!(k.free_object(x));
    }
}
