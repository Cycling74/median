//! A port of the SDK's `audio/split~`: routes a signal to the left outlet when it is within a
//! range, otherwise to the middle one, with the comparison result on the right.
//!
//! The SDK object takes the limits from the signals of inlets 1 and 2 when they are connected
//! (`count[]` of `dsp64`), which median does not pass on. Here a limit comes from the first
//! sample of its inlet's signal until a float or an argument sets it, and from then on from the
//! value set. An unconnected signal inlet reads zeros, which is also the default limit, so the
//! port differs from the SDK only where an inlet that has had a float is also connected.

use std::sync::atomic::{AtomicBool, Ordering};

use median::prelude::*;

#[derive(MSPObj)]
#[max(name = "median.split~")]
pub struct SplitTilde {
    obj: ObjRef,
    // `float` in the SDK; kept, since the comparison is against the rounded value.
    low: Float32,
    high: Float32,
    // Whether `low` and `high` replace the signals of inlets 1 and 2.
    replaced: [AtomicBool; 2],
}

impl MSPObjWrapped for SplitTilde {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let low: Option<f64> = builder.args().at(0).get()?;
        let high: Option<f64> = builder.args().at(1).get()?;
        builder.inlet(Inlet::signal().assist("(signal) Value to be sent to one of the 2 outlets"));
        builder.inlet(Inlet::signal().assist("(signal/float) Set lower limit for left outlet"));
        builder.inlet(Inlet::signal().assist("(signal/float) Set upper limit for left outlet"));
        builder.outlet(Outlet::signal().assist("(signal) Input if within limits"));
        builder.outlet(Outlet::signal().assist("(signal) Input if not within limits"));
        builder.outlet(Outlet::signal().assist("(signal) Comparison result (1 or 0)"));
        Ok(Self {
            obj: builder.obj(),
            low: Float32::new(low.unwrap_or(0.) as f32),
            high: Float32::new(high.unwrap_or(0.) as f32),
            replaced: [
                AtomicBool::new(low.is_some()),
                AtomicBool::new(high.is_some()),
            ],
        })
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let limit = |i: usize, value: &Float32| {
            if self.replaced[i - 1].load(Ordering::Relaxed) {
                value.get() as f64
            } else {
                // As in the SDK, a limit signal is read once per vector.
                ins[i].first().copied().unwrap_or(0.)
            }
        };
        let (low, high) = (limit(1, &self.low), limit(2, &self.high));
        let [inside, outside, result] = outs else {
            return;
        };
        for (i, &value) in ins[0].iter().enumerate() {
            let within = value >= low && value <= high;
            inside[i] = if within { value } else { 0. };
            outside[i] = if within { 0. } else { value };
            result[i] = if within { 1. } else { 0. };
        }
    }
}

#[median::methods]
impl SplitTilde {
    #[float]
    pub fn float(&self, value: f64) {
        let inlet = self.obj.proxy_inlet();
        let limit = match inlet {
            1 => &self.low,
            2 => &self.high,
            _ => {
                median::object_error!(
                    &self.obj,
                    "oops -- maybe you sent a number to the wrong inlet?"
                );
                return;
            }
        };
        limit.set(value as f32);
        self.replaced[inlet - 1].store(true, Ordering::Relaxed);
    }

    #[int]
    pub fn int(&self, value: t_atom_long) {
        self.float(value as f64);
    }
}

median::ext_main! { SplitTilde }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn run(k: &median_mock::Kernel, x: *mut std::ffi::c_void, limits: [f64; 2]) -> Vec<Vec<f64>> {
        let adds = k.dsp64(x, 44100., 4).unwrap();
        let mut ins = vec![
            vec![-1., 0.25, 0.5, 2.],
            vec![limits[0]; 4],
            vec![limits[1]; 4],
        ];
        let mut outs = vec![vec![9.; 4]; 3];
        adds[0].run(&mut ins, &mut outs);
        outs
    }

    #[test]
    fn splits_by_argument_limits() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.split~", &[Atom::Float(0.), Atom::Float(1.)]);
        assert!(!x.is_null());
        // The limit signals are ignored: the arguments replaced them.
        let outs = run(&k, x, [5., 5.]);
        assert_eq!(outs[0], [0., 0.25, 0.5, 0.]);
        assert_eq!(outs[1], [-1., 0., 0., 2.]);
        assert_eq!(outs[2], [0., 1., 1., 0.]);
        k.send_inlet(x, 2, "float", &[Atom::Float(0.3)]).unwrap();
        assert_eq!(run(&k, x, [5., 5.])[2], [0., 1., 0., 0.]);
        assert_eq!(
            k.assist(x, true, 2).as_deref(),
            Some("(signal) Comparison result (1 or 0)")
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn limit_signals_and_wrong_inlet() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.split~", &[]);
        assert_eq!(run(&k, x, [0.5, 3.])[2], [0., 0., 1., 1.]);
        k.send_inlet(x, 1, "int", &[Atom::Long(-5)]).unwrap();
        assert_eq!(run(&k, x, [0.5, 3.])[2], [1., 1., 1., 1.]);
        k.send_float(x, 1.).unwrap();
        assert_eq!(
            k.take_console_log()[0].text,
            "oops -- maybe you sent a number to the wrong inlet?"
        );
        assert!(k.free_object(x));
    }
}
