//! A port of the SDK's `audio/simpwave~`: a wavetable oscillator that reads a stretch of a
//! `buffer~`'s first channel, at a position from 0 to 1 given by its signal.
//!
//! The SDK object takes the start and end (in ms) from the signals of inlets 1 and 2 when they are
//! connected (`count[]` of `dsp64`), which median does not pass on. Here each comes from the first
//! sample of its inlet's signal until a float, an argument or `set` sets it, and from then on from
//! the value set; a `set` that leaves it out returns it to the signal. An unconnected signal inlet reads zeros, which is also the default, so the port
//! differs from the SDK only where an inlet that has had a value is also connected.
//!
//! The SDK caches the stretch and recomputes it on `buffer_modified` notifications; the port
//! computes it every vector from the claimed buffer, so it has no `notify` method.

use std::sync::atomic::{AtomicBool, Ordering};

use median::{atom::AtomValue, prelude::*};

#[derive(MSPObj)]
#[max(name = "median.simpwave~")]
pub struct SimpwaveTilde {
    obj: ObjRef,
    buffer: ManagedBufferRef,
    // In ms; `float` in the SDK.
    start: Float32,
    end: Float32,
    // Whether `start` and `end` replace the signals of inlets 1 and 2.
    replaced: [AtomicBool; 2],
}

impl MSPObjWrapped for SimpwaveTilde {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let args = builder.args();
        let name: SymbolRef = args.at(0).unwrap_or_default()?;
        let start: Option<f64> = args.at(1).get()?;
        let end: Option<f64> = args.at(2).get()?;
        builder.inlet(Inlet::signal().assist("(signal) Table Position (from 0 to 1)"));
        builder.inlet(Inlet::signal().assist("(signal/float) Starting Table Location in ms"));
        builder.inlet(Inlet::signal().assist("(signal/float) Ending Table Location in ms"));
        builder.outlet(Outlet::signal().assist("(signal) Output 1"));
        Ok(Self {
            obj: builder.obj(),
            buffer: builder.with_buffer(Some(name)),
            start: Float32::new(start.unwrap_or(0.) as f32),
            end: Float32::new(end.unwrap_or(0.) as f32),
            replaced: [
                AtomicBool::new(start.is_some()),
                AtomicBool::new(end.is_some()),
            ],
        })
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let out = &mut outs[0];
        let Ok(buf) = self.buffer.try_lock() else {
            out.fill(0.);
            return;
        };
        let frames = buf.frames() as i64;
        if frames == 0 || buf.channels() == 0 {
            out.fill(0.);
            return;
        }
        let limit = |i: usize, value: &Float32| {
            if self.replaced[i - 1].load(Ordering::Relaxed) {
                value.get() as f64
            } else {
                ins[i].first().copied().unwrap_or(0.)
            }
        };
        let min = limit(1, &self.start).max(0.);
        let max = limit(2, &self.end).max(0.);
        // An end of 0 is the rest of the table. Otherwise the SDK orders the two only when a
        // signal changed them, so after `set buf 100 50` it reads before the stretch.
        let max = if max == 0. { 0. } else { max.max(min) };

        let msr = buf.millisample_rate();
        let begin = ((min * msr) as i64).clamp(0, frames - 1);
        let mut len = if max == 0. {
            frames
        } else {
            ((max - min) * msr) as i64
        };
        if begin + len >= frames {
            len = frames - begin;
        }
        for (o, &v) in out.iter_mut().zip(ins[0]) {
            // `max(0)`: with `len` 0 the SDK reads one frame before `begin`.
            let dex = ((v.clamp(0., 1.) * len as f64) as i64).min(len - 1).max(0);
            *o = buf.sample((begin + dex) as usize, 0).get() as f64;
        }
    }
}

#[median::methods]
impl SimpwaveTilde {
    #[gimme("set")]
    #[defer]
    pub fn set(&self, atoms: &[Atom]) {
        let number = |i: usize| match atoms.get(i).and_then(Atom::get_value) {
            Some(AtomValue::Int(n)) => Some(n as f64),
            Some(AtomValue::Float(f)) => Some(f),
            Some(_) => Some(0.),
            None => None,
        };
        let (start, end) = (number(1), number(2));
        self.start.set(start.unwrap_or(0.).max(0.) as f32);
        self.end.set(end.unwrap_or(0.).max(0.) as f32);
        self.replaced[0].store(start.is_some(), Ordering::Relaxed);
        self.replaced[1].store(end.is_some(), Ordering::Relaxed);
        self.buffer
            .set(atoms.first().map(Atom::get_symbol).unwrap_or_default());
    }

    #[float]
    pub fn float(&self, f: f64) {
        let f = f.max(0.) as f32;
        match self.obj.proxy_inlet() {
            1 => {
                if f > self.end.get() {
                    self.end.set(f);
                    self.replaced[1].store(true, Ordering::Relaxed);
                }
                self.start.set(f);
                self.replaced[0].store(true, Ordering::Relaxed);
            }
            2 => {
                if f < self.start.get() {
                    self.start.set(f);
                    self.replaced[0].store(true, Ordering::Relaxed);
                }
                self.end.set(f);
                self.replaced[1].store(true, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.float(n as f64);
    }

    #[dblclick]
    pub fn dblclick(&self) {
        self.buffer.view();
    }
}

median::ext_main! { SimpwaveTilde }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn run(
        k: &median_mock::Kernel,
        x: *mut std::ffi::c_void,
        position: &[f64],
        limits: [f64; 2],
    ) -> Vec<f64> {
        let n = position.len();
        let adds = k.dsp64(x, 44100., n).unwrap();
        let mut ins = vec![position.to_vec(), vec![limits[0]; n], vec![limits[1]; n]];
        let mut outs = vec![vec![9.; n]];
        adds[0].run(&mut ins, &mut outs);
        outs.remove(0)
    }

    #[test]
    fn plays_a_stretch_of_the_table() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        // 1 frame per ms, 2 channels: frame i is (i, -i).
        k.create_buffer("wave", 2, 10, 1000.);
        let samples: Vec<f32> = (0..10).flat_map(|i| [i as f32, -(i as f32)]).collect();
        k.set_buffer_samples("wave", &samples);
        let x = k.new_object(
            "median.simpwave~",
            &[Atom::Sym("wave".into()), Atom::Long(2), Atom::Long(6)],
        );
        assert!(!x.is_null());
        // The limit signals are ignored: the arguments replaced them.
        assert_eq!(
            run(&k, x, &[-1., 0., 0.5, 0.99, 2.], [0., 0.]),
            [2., 2., 4., 5., 5.]
        );
        // Without an end the whole table from the start.
        k.send(x, "set", &[Atom::Sym("wave".into()), Atom::Long(8)])
            .unwrap();
        k.run_main_queue();
        assert_eq!(run(&k, x, &[0., 1.], [0., 0.]), [8., 9.]);
        k.send_inlet(x, 2, "float", &[Atom::Float(4.)]).unwrap();
        assert_eq!(run(&k, x, &[0., 1.], [0., 0.]), [4., 4.]);
        assert!(k.dblclick(x));
        assert_eq!(k.buffer_views(), ["wave"]);
        assert_eq!(k.assist(x, true, 0).as_deref(), Some("(signal) Output 1"));
        assert!(k.free_object(x));
    }

    #[test]
    fn limit_signals_and_no_buffer() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.simpwave~", &[Atom::Sym("wave".into())]);
        assert_eq!(run(&k, x, &[0.5], [1., 3.]), [0.]);
        k.create_buffer("wave", 1, 10, 1000.);
        let samples: Vec<f32> = (0..10).map(|i| i as f32).collect();
        k.set_buffer_samples("wave", &samples);
        assert_eq!(run(&k, x, &[0., 0.5, 1.], [1., 3.]), [1., 2., 2.]);
        // A start past the end plays one frame.
        assert_eq!(run(&k, x, &[0., 1.], [5., 3.]), [5., 5.]);
        assert!(k.free_object(x));
    }
}
