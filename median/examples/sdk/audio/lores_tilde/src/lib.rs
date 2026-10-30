//! A port of the SDK's `audio/lores~`: a resonant lowpass filter with cutoff and resonance
//! controls.
//!
//! The SDK object takes cutoff and resonance from the signals of inlets 1 and 2 when they are
//! connected (`count[]` of `dsp64`), which median does not pass on. Here each comes from the first
//! sample of its inlet's signal until a float, an argument or the attribute sets it, and from then
//! on from the value set. An unconnected signal inlet reads zeros, which is also the default, so
//! the port differs from the SDK only where an inlet that has had a value is also connected.
//!
//! The SDK chooses between an unrolled and a plain perform routine by the maximum vector size,
//! which median does not pass on either; both compute the same filter, and the port has one loop.

use std::sync::atomic::{AtomicBool, Ordering};

use median::prelude::*;

// The SDK clamps to `1 - 1E-20`, which rounds to 1; this is the double just below 1.
const MAX_RESONANCE: f64 = 1. - f64::EPSILON / 2.;

#[derive(MSPObj)]
#[max(name = "median.lores~")]
pub struct LoresTilde {
    obj: ObjRef,
    freq: Float64,
    resonance: Float64,
    // Whether `freq` and `resonance` replace the signals of inlets 1 and 2.
    replaced: [AtomicBool; 2],
    // Set by `clear` and `dsp_setup`: perform zeroes its sample memory.
    clear: AtomicBool,
    // Set by `dsp_setup` after `two_pi_dsr`: perform recomputes its coefficients.
    recalc: AtomicBool,
    two_pi_dsr: Float64,
    filter: Filter,
}

// Written by perform only.
#[derive(Default)]
struct Filter {
    freq: Float64,
    resonance: Float64,
    a1: Float64,
    a2: Float64,
    ym1: Float64,
    ym2: Float64,
}

impl MSPObjWrapped for LoresTilde {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let args = builder.args();
        let freq: Option<f64> = args.at(0).get()?;
        let resonance: Option<f64> = args.at(1).get()?;
        builder.inlet(Inlet::signal().assist("(signal) Input"));
        builder.inlet(Inlet::signal().assist("(signal/float) Cutoff Frequency"));
        builder.inlet(Inlet::signal().assist("(signal/float) Resonance Control (0-1)"));
        builder.outlet(Outlet::signal().assist("(signal) Output"));
        Ok(Self {
            obj: builder.obj(),
            freq: Float64::new(freq.unwrap_or(0.)),
            resonance: Float64::new(resonance.unwrap_or(0.).min(MAX_RESONANCE)),
            replaced: [
                AtomicBool::new(freq.is_some()),
                AtomicBool::new(resonance.is_some()),
            ],
            clear: AtomicBool::new(true),
            recalc: AtomicBool::new(false),
            two_pi_dsr: Float64::new(0.),
            filter: Filter::default(),
        })
    }

    fn dsp_setup(&self, sample_rate: f64) {
        self.two_pi_dsr.set(std::f64::consts::TAU / sample_rate);
        self.recalc.store(true, Ordering::Release);
        self.clear.store(true, Ordering::Relaxed);
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let f = &self.filter;
        // The SDK keeps the values a signal sets, so the attributes read them.
        let control = |i: usize, value: &Float64, clamp: fn(f64) -> f64| {
            if self.replaced[i - 1].load(Ordering::Relaxed) {
                clamp(value.get())
            } else {
                let v = clamp(ins[i].first().copied().unwrap_or(0.));
                value.set(v);
                v
            }
        };
        let freq = control(1, &self.freq, |f| f);
        let resonance = control(2, &self.resonance, |r| r.clamp(0., MAX_RESONANCE));

        if self.clear.swap(false, Ordering::Relaxed) {
            f.ym1.set(0.);
            f.ym2.set(0.);
        }
        let recalc = self.recalc.swap(false, Ordering::Acquire);
        if recalc || freq != f.freq.get() || resonance != f.resonance.get() {
            let resterm = (resonance * 0.125).exp() * 0.882497;
            let fqterm = (self.two_pi_dsr.get() * freq).cos();
            f.a1.set(-2. * resterm * fqterm);
            f.a2.set(resterm * resterm);
            f.freq.set(freq);
            f.resonance.set(resonance);
        }

        let (a1, a2) = (f.a1.get(), f.a2.get());
        let scale = 1. + a1 + a2;
        let (mut ym1, mut ym2) = (f.ym1.get(), f.ym2.get());
        for (o, x) in outs[0].iter_mut().zip(ins[0]) {
            let y = scale * x - a1 * ym1 - a2 * ym2;
            ym2 = ym1;
            ym1 = y;
            *o = y;
        }
        f.ym1.set(ym1);
        f.ym2.set(ym2);
    }
}

#[median::methods]
impl LoresTilde {
    #[float]
    pub fn float(&self, f: f64) {
        match self.obj.proxy_inlet() {
            1 => self.set_cutoff(f),
            2 => self.set_resonance(f),
            _ => {}
        }
    }

    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.float(n as f64);
    }

    #[sel("clear")]
    pub fn clear(&self) {
        self.clear.store(true, Ordering::Relaxed);
    }

    #[attr(label = "Cutoff Frequency")]
    pub fn cutoff(&self) -> f64 {
        self.freq.get()
    }

    pub fn set_cutoff(&self, f: f64) {
        self.freq.set(f);
        self.replaced[0].store(true, Ordering::Relaxed);
    }

    #[attr(label = "Resonance")]
    pub fn resonance(&self) -> f64 {
        self.resonance.get()
    }

    pub fn set_resonance(&self, r: f64) {
        self.resonance.set(r.min(MAX_RESONANCE));
        self.replaced[1].store(true, Ordering::Relaxed);
    }
}

median::ext_main! { LoresTilde }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    // The SDK's filter, one sample at a time.
    fn reference(sr: f64, freq: f64, r: f64, input: &[f64]) -> Vec<f64> {
        let resterm = (r * 0.125).exp() * 0.882497;
        let a1 = -2. * resterm * (std::f64::consts::TAU / sr * freq).cos();
        let a2 = resterm * resterm;
        let scale = 1. + a1 + a2;
        let (mut ym1, mut ym2) = (0., 0.);
        input
            .iter()
            .map(|x| {
                let y = scale * x - a1 * ym1 - a2 * ym2;
                ym2 = ym1;
                ym1 = y;
                y
            })
            .collect()
    }

    #[test]
    fn filters_with_argument_settings() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.lores~", &[Atom::Long(1000), Atom::Float(0.5)]);
        assert!(!x.is_null());
        let adds = k.dsp64(x, 44100., 4).unwrap();
        let input = [1., 0., 0., 0., 0., 0., 0., 0.];
        let expected = reference(44100., 1000., 0.5, &input);
        let mut out = Vec::new();
        for block in input.chunks(4) {
            // The control signals are ignored: the arguments replaced them.
            let mut ins = vec![block.to_vec(), vec![50.; 4], vec![0.9; 4]];
            let mut outs = vec![vec![0.; 4]];
            adds[0].run(&mut ins, &mut outs);
            out.extend(outs.remove(0));
        }
        assert_eq!(out, expected);

        // `clear` forgets the sample memory: the next block starts again from rest.
        k.send(x, "clear", &[]).unwrap();
        let mut ins = vec![input[..4].to_vec(), vec![0.; 4], vec![0.; 4]];
        let mut outs = vec![vec![0.; 4]];
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs[0], expected[..4]);
        assert_eq!(k.attr_get(x, "cutoff").unwrap(), [Atom::Float(1000.)]);
        assert!(k.free_object(x));
    }

    #[test]
    fn signals_floats_and_attributes() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.lores~", &[]);
        let adds = k.dsp64(x, 48000., 4).unwrap();
        let input = [1., 0.5, 0., -0.5];
        let mut ins = vec![input.to_vec(), vec![2000.; 4], vec![0.25; 4]];
        let mut outs = vec![vec![0.; 4]];
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs[0], reference(48000., 2000., 0.25, &input));
        assert_eq!(k.attr_get(x, "cutoff").unwrap(), [Atom::Float(2000.)]);

        k.attr_set(x, "resonance", &[Atom::Float(3.)]).unwrap();
        k.send_inlet(x, 1, "float", &[Atom::Float(500.)]).unwrap();
        k.send(x, "clear", &[]).unwrap();
        let mut ins = vec![input.to_vec(), vec![2000.; 4], vec![0.25; 4]];
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(
            outs[0],
            reference(48000., 500., super::MAX_RESONANCE, &input)
        );
        assert_eq!(
            k.attr_get(x, "resonance").unwrap(),
            [Atom::Float(super::MAX_RESONANCE)]
        );
        assert_eq!(
            k.assist(x, false, 2).as_deref(),
            Some("(signal/float) Resonance Control (0-1)")
        );
        assert!(k.free_object(x));
    }
}
