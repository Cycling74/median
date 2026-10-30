//! A port of the SDK's `audio/plussz~`: adds two signals, or a signal and a float.
//!
//! The SDK object picks its perform routine from which signal inlets are connected (`count[]` of
//! `dsp64`), which median does not pass on. Here a float replaces the signal of the inlet it
//! arrives at, from then on, and the argument replaces the right inlet's; without either, the two
//! signals are added. An unconnected signal inlet reads zeros, which is also the default value, so
//! the port differs from the SDK only where an inlet that has had a float is also connected.

use std::sync::atomic::{AtomicU8, Ordering};

use median::prelude::*;

// `replaced` when neither inlet's signal is replaced.
const SIGNALS: u8 = 2;

#[derive(MSPObj)]
#[max(name = "median.plussz~")]
pub struct PlusszTilde {
    obj: ObjRef,
    val: Float64,
    // The inlet (0 or 1) whose signal `val` replaces, or `SIGNALS`.
    replaced: AtomicU8,
}

impl MSPObjWrapped for PlusszTilde {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let val: Option<f64> = builder.args().at(0).get()?;
        builder.inlet(Inlet::signal().assist("(Signal) This + Right Inlet"));
        builder.inlet(Inlet::signal().assist("(Signal) Left Inlet + This"));
        builder.outlet(Outlet::signal().assist("(Signal) Addition Result"));
        Ok(Self {
            obj: builder.obj(),
            val: Float64::new(val.unwrap_or(0.)),
            replaced: AtomicU8::new(if val.is_some() { 1 } else { SIGNALS }),
        })
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let val = self.val.get();
        let out = outs[0].iter_mut();
        match self.replaced.load(Ordering::Relaxed) {
            0 => out.zip(ins[1]).for_each(|(o, r)| *o = val + r),
            1 => out.zip(ins[0]).for_each(|(o, l)| *o = l + val),
            _ => out
                .zip(ins[0].iter().zip(ins[1]))
                .for_each(|(o, (l, r))| *o = l + r),
        }
    }
}

#[median::methods]
impl PlusszTilde {
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

median::ext_main! { PlusszTilde }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn run(k: &median_mock::Kernel, x: *mut std::ffi::c_void) -> Vec<f64> {
        let adds = k.dsp64(x, 44100., 2).unwrap();
        let mut ins = vec![vec![1., 2.], vec![10., 20.]];
        let mut outs = vec![vec![0.; 2]];
        adds[0].run(&mut ins, &mut outs);
        outs.remove(0)
    }

    #[test]
    fn adds_signals_or_a_float() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.plussz~", &[]);
        assert!(!x.is_null());
        assert_eq!(run(&k, x), [11., 22.]);
        k.send_inlet(x, 1, "float", &[Atom::Float(0.5)]).unwrap();
        assert_eq!(run(&k, x), [1.5, 2.5]);
        k.send_int(x, 3).unwrap();
        assert_eq!(run(&k, x), [13., 23.]);
        assert_eq!(
            k.assist(x, false, 1).as_deref(),
            Some("(Signal) Left Inlet + This")
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn argument_replaces_the_right_inlet() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.plussz~", &[Atom::Float(100.)]);
        assert_eq!(run(&k, x), [101., 102.]);
        assert!(k.free_object(x));
    }
}
