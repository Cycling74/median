//! A port of the SDK's `audio/times~`: multiplies two signals, or a signal and a float.
//!
//! The SDK object picks its perform routine from which signal inlets are connected (`count[]` of
//! `dsp64`), which median does not pass on. Here a float replaces the signal of the inlet it
//! arrives at, from then on, and the argument replaces the right inlet's; without either, the two
//! signals are multiplied. An unconnected signal inlet reads zeros, which is also the default
//! value, so the port differs from the SDK only where an inlet that has had a float is also
//! connected.
//!
//! The SDK calls `class_setname("*~", "times~")` to tie the object name `*~` to the file
//! `times~`. The port registers no `median.*~`: median has no safe `class_setname`, and such an
//! alias belongs in the package's object mappings, which Max reads before any file is loaded.

use std::sync::atomic::{AtomicU8, Ordering};

use median::prelude::*;

// `replaced` when neither inlet's signal is replaced.
const SIGNALS: u8 = 2;

#[derive(MSPObj)]
#[max(name = "median.times~")]
pub struct TimesTilde {
    obj: ObjRef,
    val: Float64,
    // The inlet (0 or 1) whose signal `val` replaces, or `SIGNALS`.
    replaced: AtomicU8,
}

impl MSPObjWrapped for TimesTilde {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let val: Option<f64> = builder.args().at(0).get()?;
        builder.inlet(Inlet::signal().assist("(Signal/Float) This * Right Inlet"));
        builder.inlet(Inlet::signal().assist("(Signal/Float) Left Inlet * This"));
        builder.outlet(Outlet::signal().assist("(Signal) Multiplication Result"));
        Ok(Self {
            obj: builder.obj(),
            val: Float64::new(val.unwrap_or(0.)),
            replaced: AtomicU8::new(if val.is_some() { 1 } else { SIGNALS }),
        })
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let out = outs[0].iter_mut();
        match self.replaced.load(Ordering::Relaxed) {
            replaced @ (0 | 1) => {
                let val = self.val.get();
                let signal = ins[1 - replaced as usize];
                out.zip(signal).for_each(|(o, s)| *o = fix_nan(val * s));
            }
            _ => out
                .zip(ins[0].iter().zip(ins[1]))
                .for_each(|(o, (l, r))| *o = fix_nan(l * r)),
        }
    }
}

// `FIX_DENORM_NAN_DOUBLE` as the SDK builds it (denormals are left alone): NaN, infinities and
// magnitudes from 2^1023 up become 0.
fn fix_nan(v: f64) -> f64 {
    const EXPONENT_TOP: u64 = 0x7fe << 52;
    if v.to_bits() & EXPONENT_TOP == EXPONENT_TOP {
        0.
    } else {
        v
    }
}

#[median::methods]
impl TimesTilde {
    #[float]
    pub fn float(&self, f: f64) {
        self.val.set(f);
        let inlet = self.obj.proxy_inlet().min(1) as u8;
        self.replaced.store(inlet, Ordering::Relaxed);
    }

    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.float(n as f64);
    }
}

median::ext_main! { TimesTilde }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn run(k: &median_mock::Kernel, x: *mut std::ffi::c_void, left: f64) -> Vec<f64> {
        let adds = k.dsp64(x, 44100., 2).unwrap();
        let mut ins = vec![vec![left, 2.], vec![10., 20.]];
        let mut outs = vec![vec![9.; 2]];
        adds[0].run(&mut ins, &mut outs);
        outs.remove(0)
    }

    #[test]
    fn multiplies_signals_or_a_float() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.times~", &[]);
        assert!(!x.is_null());
        assert_eq!(run(&k, x, 1.), [10., 40.]);
        k.send_inlet(x, 1, "int", &[Atom::Long(3)]).unwrap();
        assert_eq!(run(&k, x, 1.), [3., 6.]);
        k.send_float(x, 0.5).unwrap();
        assert_eq!(run(&k, x, 1.), [5., 10.]);
        assert_eq!(
            k.assist(x, true, 0).as_deref(),
            Some("(Signal) Multiplication Result")
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn argument_replaces_the_right_inlet_and_nan_is_zeroed() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.times~", &[Atom::Float(4.)]);
        assert_eq!(run(&k, x, f64::NAN), [0., 8.]);
        assert_eq!(run(&k, x, f64::INFINITY), [0., 8.]);
        assert!(k.free_object(x));
    }
}
