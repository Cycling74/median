//! A port of the SDK's `audio/index~`: plays the sample of a `buffer~` channel at the index its
//! signal gives.

use median::prelude::*;

#[derive(MSPObj)]
#[max(name = "median.index~")]
pub struct IndexTilde {
    buffer: ManagedBufferRef,
    // Zero-based.
    chan: Int64,
}

impl MSPObjWrapped for IndexTilde {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let name: SymbolRef = builder.args().at(0).unwrap_or_default()?;
        let chan = builder.args().at(1).unwrap_or(0)?;
        builder.inlet(Inlet::signal().assist("(signal) Sample Index"));
        builder.inlet(Inlet::int(Self::in1).assist("Audio Channel In buffer~"));
        builder.outlet(Outlet::signal().assist("(signal) Sample Value at Index"));
        let this = Self {
            buffer: builder.with_buffer(Some(name)),
            chan: Int64::new(0),
        };
        this.in1(chan);
        Ok(this)
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let out = &mut outs[0];
        let Ok(buf) = self.buffer.try_lock() else {
            out.fill(0.);
            return;
        };
        let (frames, channels) = (buf.frames(), buf.channels());
        if frames == 0 || channels == 0 {
            out.fill(0.);
            return;
        }
        // The SDK clamps to the channel count, which reads one channel past the last.
        let chan = (self.chan.get().max(0) as usize).min(channels - 1);
        for (o, &i) in out.iter_mut().zip(ins[0]) {
            let index = ((i + 0.5) as i64).clamp(0, frames as i64 - 1) as usize;
            *o = buf.sample(index, chan).get() as f64;
        }
    }
}

#[median::methods]
impl IndexTilde {
    // `buffer_ref_set` is main-thread only; the SDK's `index~` calls it from any thread.
    #[sel("set")]
    #[defer]
    pub fn set(&self, name: SymbolRef) {
        self.buffer.set(name);
    }

    #[dblclick]
    pub fn dblclick(&self) {
        self.buffer.view();
    }

    pub fn in1(&self, n: t_atom_long) {
        self.chan.set(n.max(1) - 1);
    }
}

median::ext_main! { IndexTilde }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn run(k: &median_mock::Kernel, x: *mut std::ffi::c_void, index: &[f64]) -> Vec<f64> {
        let adds = k.dsp64(x, 44100., index.len()).unwrap();
        let mut ins = vec![index.to_vec()];
        let mut outs = vec![vec![9.; index.len()]];
        adds[0].run(&mut ins, &mut outs);
        outs.remove(0)
    }

    #[test]
    fn reads_the_buffer_at_the_index() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        k.create_buffer("tab", 2, 3, 44100.);
        k.set_buffer_samples("tab", &[1., 10., 2., 20., 3., 30.]);
        let x = k.new_object("median.index~", &[Atom::Sym("tab".into()), Atom::Long(2)]);
        assert!(!x.is_null());
        assert_eq!(run(&k, x, &[-4., 0.6, 1.4, 99.]), [10., 20., 20., 30.]);
        k.send_inlet(x, 1, "int", &[Atom::Long(0)]).unwrap();
        assert_eq!(run(&k, x, &[0., 2.]), [1., 3.]);
        // Past the last channel reads the last.
        k.send_inlet(x, 1, "int", &[Atom::Long(5)]).unwrap();
        assert_eq!(run(&k, x, &[0.]), [10.]);
        assert!(k.dblclick(x));
        assert_eq!(k.buffer_views(), ["tab"]);
        assert_eq!(k.buffer_state("tab").unwrap().0, 0);
        assert_eq!(
            k.assist(x, false, 1).as_deref(),
            Some("Audio Channel In buffer~")
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn silent_without_a_buffer_and_set_follows_another() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.index~", &[Atom::Sym("nothing".into())]);
        assert_eq!(run(&k, x, &[0., 1.]), [0., 0.]);
        k.create_buffer("other", 1, 2, 44100.);
        k.set_buffer_samples("other", &[0.5, 0.25]);
        k.send(x, "set", &[Atom::Sym("other".into())]).unwrap();
        k.run_main_queue();
        assert_eq!(run(&k, x, &[0., 1.]), [0.5, 0.25]);
        assert!(k.free_object(x));
    }
}
