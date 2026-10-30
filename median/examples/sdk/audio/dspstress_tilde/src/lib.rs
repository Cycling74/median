//! A port of the SDK's `audio/dspstress~`: spends a given percentage of each signal vector's
//! duration busy in its perform routine.
//!
//! The SDK object measures the duration of the maximum vector size, which median's `dsp64` does
//! not pass on; the port measures the vector it is given. It spins on `std::time::Instant`
//! instead of `systimer_gettime`.

use std::time::{Duration, Instant};

use median::prelude::*;

#[derive(MSPObj)]
#[max(name = "median.dspstress~")]
pub struct DspstressTilde {
    // 0 to 100.
    cpu_usage_target: Float64,
    sample_rate: Float64,
}

impl MSPObjWrapped for DspstressTilde {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let target = builder.args().at(0).unwrap_or(0.)?;
        builder.inlet(Inlet::signal().assist("Specify cpu usage % here"));
        Ok(Self {
            cpu_usage_target: Float64::new(target),
            sample_rate: Float64::new(0.),
        })
    }

    fn dsp_setup(&self, sample_rate: f64) {
        self.sample_rate.set(sample_rate);
    }

    fn perform(&self, _ins: &[&[f64]], _outs: &mut [&mut [f64]], nframes: usize) {
        let vector_secs = nframes as f64 / self.sample_rate.get();
        let spin = vector_secs * self.cpu_usage_target.get() / 100.;
        let Ok(spin) = Duration::try_from_secs_f64(spin) else {
            return;
        };
        let start = Instant::now();
        while start.elapsed() < spin {
            std::hint::spin_loop();
        }
    }
}

#[median::methods]
impl DspstressTilde {
    #[float]
    pub fn float(&self, f: f64) {
        self.cpu_usage_target.set(f);
    }

    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.float(n as f64);
    }
}

median::ext_main! { DspstressTilde }

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    #[test]
    fn spins_for_the_share_of_the_vector() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.dspstress~", &[50i64.into()]);
        assert!(!x.is_null());
        // 100 frames at 10 kHz are 10 ms; half of that is spent spinning.
        let adds = k.dsp64(x, 10_000., 100).unwrap();
        let mut ins = vec![vec![0.; 100]];
        let start = Instant::now();
        adds[0].run(&mut ins, &mut []);
        assert!(start.elapsed() >= Duration::from_millis(5));

        k.send_int(x, 0).unwrap();
        let start = Instant::now();
        adds[0].run(&mut ins, &mut []);
        assert!(start.elapsed() < Duration::from_millis(5));
        assert_eq!(
            k.assist(x, false, 0).as_deref(),
            Some("Specify cpu usage % here")
        );
        assert!(k.free_object(x));
    }
}
