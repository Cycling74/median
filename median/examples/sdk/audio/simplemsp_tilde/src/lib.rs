//! A port of the SDK's `audio/simplemsp~`: adds a float offset to its signal input.

use median::prelude::*;

#[derive(MSPObj)]
#[max(name = "median.simplemsp~")]
pub struct SimpleMsp {
    offset: Float64,
}

impl MSPObjWrapped for SimpleMsp {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::signal().assist("I am inlet 0"));
        builder.outlet(Outlet::signal().assist("I am outlet 0"));
        Ok(Self {
            offset: Float64::new(0.),
        })
    }

    fn dsp_setup(&self, sample_rate: f64) {
        post!("my sample rate is: {sample_rate:.6}");
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let offset = self.offset.get();
        for (o, i) in outs[0].iter_mut().zip(ins[0]) {
            *o = i + offset;
        }
    }
}

#[median::methods]
impl SimpleMsp {
    #[float]
    pub fn float(&self, f: f64) {
        self.offset.set(f);
    }
}

median::ext_main! { SimpleMsp }

#[cfg(test)]
mod tests {
    #[test]
    fn adds_the_offset() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.simplemsp~", &[]);
        assert!(!x.is_null());
        k.send_float(x, 0.5).unwrap();
        let adds = k.dsp64(x, 44100., 3).unwrap();
        assert_eq!(
            k.take_console_log()[0].text,
            "my sample rate is: 44100.000000"
        );
        let mut ins = vec![vec![0., 1., -1.]];
        let mut outs = vec![vec![9.; 3]];
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs, [[0.5, 1.5, -0.5]]);
        assert!(k.free_object(x));
    }
}
