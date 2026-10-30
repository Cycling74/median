//! A port of the SDK's `mc/limi~`: a lookahead limiter for N channels, the build without
//! `MC_VERSION` (one signal inlet and outlet per channel).
//!
//! The channel count (argument 1) is clamped to 1..=64 rather than the SDK's 1..=1024, because
//! median hands `perform` at most 64 signal inlets and outlets. The SDK's limiter takes the
//! sample rate from `sys_getsr` when it is made; here it starts at 48000 and takes the rate from
//! `dsp_setup`, which comes before any audio. The limiter fixes bugs of its min-lib original,
//! listed on [`limiter::Limiter`]; the one that shows is that the output is delayed by
//! `lookahead` samples. The SDK's inlet assist calls `printf` instead of `sprintf` and so shows
//! nothing; the port shows "(signal) Input".

mod limiter;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use median::prelude::*;

pub use limiter::{DcBlocker, Limiter, ResponseMode};

const MAX_CHANNELS: t_atom_long = 64;

#[derive(MSPObj)]
#[max(name = "median.limi~")]
pub struct LimiTilde {
    bypass: Int64,
    dcblock: Int64,
    mode: Int64,
    lookahead: Int64,
    preamp: Float64,
    postamp: Float64,
    threshold: Float64,
    release: Float64,
    sample_rate: Float64,
    // Set after a parameter above changes: perform passes them all to the limiter.
    changed: AtomicBool,
    clear: AtomicBool,
    // Only perform locks it, with `try_lock`: setters, `clear` and `dsp_setup` hand their changes
    // over through the atomics and flags above, so the lock is never contended and perform
    // never waits; the silence it outputs if the lock were ever taken is a fallback only.
    limiter: Mutex<Limiter>,
}

impl MSPObjWrapped for LimiTilde {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let args = builder.args();
        let positional = |i: usize| args.positional().get(i).map(Atom::get_int);
        let channels = positional(0).map_or(1, |n| n.clamp(1, MAX_CHANNELS)) as usize;
        let buffer_size = positional(1).map_or(512, |n| n.clamp(100, 96000)) as usize;
        for _ in 0..channels {
            builder.inlet(Inlet::signal().assist("(signal) Input"));
        }
        for _ in 0..channels {
            builder.outlet(Outlet::signal().assist("(signal) Output"));
        }
        let limiter = Limiter::new(channels, buffer_size, 48000.);
        Ok(Self {
            bypass: Int64::new(0),
            dcblock: Int64::new(1),
            mode: Int64::new(1),
            lookahead: Int64::new(100),
            preamp: Float64::new(0.),
            postamp: Float64::new(0.),
            threshold: Float64::new(0.),
            release: Float64::new(1000.),
            sample_rate: Float64::new(48000.),
            changed: AtomicBool::new(true),
            clear: AtomicBool::new(false),
            limiter: Mutex::new(limiter),
        })
    }

    fn dsp_setup(&self, sample_rate: f64) {
        self.sample_rate.set(sample_rate);
        self.changed.store(true, Ordering::Release);
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let Ok(mut limiter) = self.limiter.try_lock() else {
            outs.iter_mut().for_each(|o| o.fill(0.));
            return;
        };
        if self.changed.swap(false, Ordering::Acquire) {
            self.configure(&mut limiter);
        }
        if self.clear.swap(false, Ordering::Relaxed) {
            limiter.clear();
        }
        limiter.process(ins, outs);
    }
}

impl LimiTilde {
    fn configure(&self, l: &mut Limiter) {
        l.reset(l.release(), l.mode(), self.sample_rate.get());
        l.set_bypass(self.bypass.get() != 0);
        l.set_dcblock(self.dcblock.get() != 0);
        // The SDK casts any other value to its enum, where all but 0 act as exponential.
        l.set_mode(if self.mode.get() == 0 {
            ResponseMode::Linear
        } else {
            ResponseMode::Exponential
        });
        l.set_lookahead(self.lookahead.get() as i64);
        l.set_preamp(self.preamp.get());
        l.set_postamp(self.postamp.get());
        l.set_threshold(self.threshold.get());
        l.set_release(self.release.get());
    }

    fn changed(&self) {
        self.changed.store(true, Ordering::Release);
    }
}

#[median::methods]
impl LimiTilde {
    #[sel("clear")]
    pub fn clear(&self) {
        self.clear.store(true, Ordering::Relaxed);
    }

    #[attr(style = "onoff", label = "Bypass")]
    pub fn bypass(&self) -> t_atom_long {
        self.bypass.get()
    }

    pub fn set_bypass(&self, v: t_atom_long) {
        self.bypass.set(v);
        self.changed();
    }

    #[attr(style = "onoff", label = "Block DC offsets")]
    pub fn dcblock(&self) -> t_atom_long {
        self.dcblock.get()
    }

    pub fn set_dcblock(&self, v: t_atom_long) {
        self.dcblock.set(v);
        self.changed();
    }

    #[attr(enum = "Linear Exponential", label = "Response Mode")]
    pub fn mode(&self) -> t_atom_long {
        self.mode.get()
    }

    pub fn set_mode(&self, v: t_atom_long) {
        self.mode.set(v);
        self.changed();
    }

    #[attr(label = "Lookahead")]
    pub fn lookahead(&self) -> t_atom_long {
        self.lookahead.get()
    }

    pub fn set_lookahead(&self, v: t_atom_long) {
        self.lookahead.set(v);
        self.changed();
    }

    #[attr(label = "Preamp Gain")]
    pub fn preamp(&self) -> f64 {
        self.preamp.get()
    }

    pub fn set_preamp(&self, v: f64) {
        self.preamp.set(v);
        self.changed();
    }

    #[attr(label = "Postamp Gain")]
    pub fn postamp(&self) -> f64 {
        self.postamp.get()
    }

    pub fn set_postamp(&self, v: f64) {
        self.postamp.set(v);
        self.changed();
    }

    #[attr(label = "Threshold Level")]
    pub fn threshold(&self) -> f64 {
        self.threshold.get()
    }

    pub fn set_threshold(&self, v: f64) {
        self.threshold.set(v);
        self.changed();
    }

    #[attr(label = "Release Time")]
    pub fn release(&self) -> f64 {
        self.release.get()
    }

    pub fn set_release(&self, v: f64) {
        self.release.set(v);
        self.changed();
    }
}

median::ext_main! { LimiTilde }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn sine(amp: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| amp * (i as f64 * 0.05).sin()).collect()
    }

    fn peak(outs: &[Vec<f64>]) -> f64 {
        outs.iter().flatten().fold(0f64, |m, s| m.max(s.abs()))
    }

    #[test]
    fn limits_each_channel_after_the_lookahead() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object(
            "median.limi~",
            &[
                Atom::Long(2),
                Atom::Long(200),
                Atom::Sym("@lookahead".into()),
                Atom::Long(16),
                Atom::Sym("@dcblock".into()),
                Atom::Long(0),
            ],
        );
        assert!(!x.is_null());
        assert_eq!(k.assist(x, false, 1).as_deref(), Some("(signal) Input"));
        assert_eq!(k.assist(x, true, 1).as_deref(), Some("(signal) Output"));
        let adds = k.dsp64(x, 44100., 64).unwrap();

        // Quiet: each channel comes out unchanged, 16 samples late.
        let quiet = vec![sine(0.5, 64), sine(-0.25, 64)];
        let mut outs = vec![vec![0.; 64]; 2];
        adds[0].run(&mut quiet.clone(), &mut outs);
        for (o, i) in outs.iter().zip(&quiet) {
            assert_eq!(o[..16], [0.; 16]);
            assert_eq!(o[16..], i[..48]);
        }

        // Loud on one channel: both stay under the threshold.
        k.attr_set(x, "threshold", &[Atom::Float(-6.)]).unwrap();
        k.send(x, "clear", &[]).unwrap();
        let limit = 10f64.powf(-6. * 0.05) + 1e-12;
        for _ in 0..8 {
            let mut ins = vec![sine(4., 64), sine(0.5, 64)];
            adds[0].run(&mut ins, &mut outs);
            assert!(peak(&outs) <= limit, "{}", peak(&outs));
        }
        assert!(outs[0].iter().any(|s| s.abs() > 0.5 * limit));
        assert!(k.free_object(x));
    }

    #[test]
    fn bypass_and_clear() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object(
            "median.limi~",
            &[Atom::Sym("@bypass".into()), Atom::Long(1)],
        );
        let adds = k.dsp64(x, 48000., 32).unwrap();
        let input = vec![sine(5., 32)];
        let mut outs = vec![vec![0.; 32]];
        adds[0].run(&mut input.clone(), &mut outs);
        assert_eq!(outs, input);

        // Loud input leaves history behind; clear forgets it, so the first lookahead is silent.
        k.attr_set(x, "bypass", &[Atom::Long(0)]).unwrap();
        k.attr_set(x, "lookahead", &[Atom::Long(8)]).unwrap();
        adds[0].run(&mut [sine(5., 32)], &mut outs);
        k.send(x, "clear", &[]).unwrap();
        adds[0].run(&mut [vec![0.; 32]], &mut outs);
        assert_eq!(outs, [[0.; 32]]);
        assert!(k.free_object(x));
    }

    #[test]
    fn attributes_and_arguments() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.limi~", &[Atom::Long(500)]);
        assert!(!x.is_null());
        assert_eq!(k.assist(x, false, 63).as_deref(), Some("(signal) Input"));
        assert_ne!(k.assist(x, false, 64).as_deref(), Some("(signal) Input"));
        let get = |n: &str| k.attr_get(x, n).unwrap();
        assert_eq!(get("bypass"), [Atom::Long(0)]);
        assert_eq!(get("dcblock"), [Atom::Long(1)]);
        assert_eq!(get("mode"), [Atom::Long(1)]);
        assert_eq!(get("lookahead"), [Atom::Long(100)]);
        assert_eq!(get("preamp"), [Atom::Float(0.)]);
        assert_eq!(get("postamp"), [Atom::Float(0.)]);
        assert_eq!(get("threshold"), [Atom::Float(0.)]);
        assert_eq!(get("release"), [Atom::Float(1000.)]);
        for (name, v) in [
            ("bypass", Atom::Long(1)),
            ("dcblock", Atom::Long(0)),
            ("mode", Atom::Long(0)),
            ("lookahead", Atom::Long(50)),
            ("preamp", Atom::Float(3.)),
            ("postamp", Atom::Float(-3.)),
            ("threshold", Atom::Float(-1.5)),
            ("release", Atom::Float(250.)),
        ] {
            k.attr_set(x, name, std::slice::from_ref(&v)).unwrap();
            assert_eq!(get(name), [v]);
        }
        let attr = |a: &str, n: &str| k.attr_attr(x, a, n).map(|(_, atoms)| atoms);
        assert_eq!(
            attr("mode", "style"),
            Some(vec![Atom::Sym("enumindex".into())])
        );
        assert_eq!(
            attr("dcblock", "style"),
            Some(vec![Atom::Sym("onoff".into())])
        );
        assert_eq!(
            attr("threshold", "label"),
            Some(vec![Atom::Sym("Threshold Level".into())])
        );
        assert!(k.free_object(x));
    }
}
