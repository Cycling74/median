//! The lookahead limiter and DC blocker of min-lib (`c74_lib_limiter.h`, `c74_lib_dcblocker.h`).

/// A one-channel DC-blocking filter.
#[derive(Clone, Copy, Default, Debug)]
pub struct DcBlocker {
    x_1: f64,
    y_1: f64,
}

impl DcBlocker {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn process(&mut self, x: f64) -> f64 {
        let y = x - self.x_1 + self.y_1 * 0.9997;
        self.y_1 = y;
        self.x_1 = x;
        y
    }
}

/// The shape of the release.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResponseMode {
    Linear,
    Exponential,
}

fn db_to_linear(db: f64) -> f64 {
    10f64.powf(db * 0.05)
}

/// A lookahead limiter for a fixed number of channels.
///
/// Fixes three bugs of the min-lib original: a new limiter starts cleared (the original leaves
/// the release coefficient uninitialized and the gain at 0, so it fades in over seconds); the
/// default `1 / lookahead` is 0.01 (the original's `1/100` is integer division); and the output
/// is the sample `lookahead` samples old (the original reads the slot it has just written, so it
/// has no delay and its lookahead ramp only lowers gains already used). `reset` also keeps the
/// sample rate it is given, which the original forgets, and the lookahead is kept in
/// `1..=buffer_size`, which the original leaves to the caller.
#[derive(Debug)]
pub struct Limiter {
    buffer_size: usize,
    sample_rate: f64,
    dcblockers: Vec<DcBlocker>,
    dcblock: bool,
    bypass: bool,
    mode: ResponseMode,
    preamp: f64,
    linear_preamp: f64,
    postamp: f64,
    linear_postamp: f64,
    threshold: f64,
    linear_threshold: f64,
    release: f64,
    recover: f64,
    last: f64,
    lookahead: usize,
    lookahead_inv: f64,
    lookahead_index: usize,
    lookahead_buffers: Vec<Vec<f64>>,
    gain_buffer: Vec<f64>,
}

impl Limiter {
    /// Allocates `buffer_size` samples (at least 1) of lookahead for each channel.
    pub fn new(channels: usize, buffer_size: usize, sample_rate: f64) -> Self {
        let buffer_size = buffer_size.max(1);
        let lookahead = 100.min(buffer_size);
        let mut l = Self {
            buffer_size,
            sample_rate,
            dcblockers: vec![DcBlocker::default(); channels],
            dcblock: true,
            bypass: false,
            mode: ResponseMode::Exponential,
            preamp: 0.,
            linear_preamp: 1.,
            postamp: 0.,
            linear_postamp: 1.,
            threshold: 0.,
            linear_threshold: 1.,
            release: 1000.,
            recover: 0.,
            last: 1.,
            lookahead,
            lookahead_inv: 1. / lookahead as f64,
            lookahead_index: 0,
            lookahead_buffers: vec![vec![0.; buffer_size]; channels],
            gain_buffer: vec![1.; buffer_size],
        };
        l.clear();
        l
    }

    pub fn channels(&self) -> usize {
        self.lookahead_buffers.len()
    }

    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    pub fn set_dcblock(&mut self, dcblock: bool) {
        self.dcblock = dcblock;
    }

    pub fn mode(&self) -> ResponseMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: ResponseMode) {
        self.mode = mode;
        self.reset(self.release, mode, self.sample_rate);
    }

    pub fn lookahead(&self) -> usize {
        self.lookahead
    }

    /// Clamped to `1..=buffer_size`.
    pub fn set_lookahead(&mut self, samples: i64) {
        self.lookahead = samples.clamp(1, self.buffer_size as i64) as usize;
        self.lookahead_inv = 1. / self.lookahead as f64;
        if self.lookahead_index >= self.lookahead {
            self.lookahead_index = 0;
        }
    }

    /// In dB.
    pub fn set_preamp(&mut self, db: f64) {
        self.preamp = db;
        self.linear_preamp = db_to_linear(db);
    }

    /// In dB.
    pub fn set_postamp(&mut self, db: f64) {
        self.postamp = db;
        self.linear_postamp = db_to_linear(db);
    }

    /// In dB.
    pub fn set_threshold(&mut self, db: f64) {
        self.threshold = db;
        self.linear_threshold = db_to_linear(db);
    }

    pub fn release(&self) -> f64 {
        self.release
    }

    /// In milliseconds.
    pub fn set_release(&mut self, ms: f64) {
        self.release = ms;
        self.reset(ms, self.mode, self.sample_rate);
    }

    /// Forgets the filter, lookahead and gain history.
    pub fn clear(&mut self) {
        self.dcblockers.iter_mut().for_each(DcBlocker::clear);
        for b in &mut self.lookahead_buffers {
            b.fill(0.);
        }
        self.gain_buffer.fill(1.);
        self.last = 1.;
        self.lookahead_index = 0;
        self.reset(self.release, self.mode, self.sample_rate);
    }

    /// Recomputes the release coefficient.
    pub fn reset(&mut self, release: f64, mode: ResponseMode, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.recover = 1000. / (release * sample_rate)
            * match mode {
                ResponseMode::Linear => 0.5,
                ResponseMode::Exponential => 0.707,
            };
    }

    /// Processes `outs[0].len()` frames. Channels beyond the limiter's, or beyond `ins` or
    /// `outs`, are left alone.
    pub fn process(&mut self, ins: &[&[f64]], outs: &mut [&mut [f64]]) {
        if self.bypass {
            for (o, i) in outs.iter_mut().zip(ins) {
                o.copy_from_slice(i);
            }
            return;
        }
        let channels = self.channels().min(ins.len()).min(outs.len());
        let frames = outs.first().map_or(0, |o| o.len());
        let lookahead = self.lookahead;
        let is_linear = self.mode == ResponseMode::Linear;

        for i in 0..frames {
            let index = self.lookahead_index;
            let mut hot_sample = 0.;

            for ch in 0..channels {
                let x = ins[ch][i];
                let mut v = if self.dcblock {
                    self.dcblockers[ch].process(x)
                } else {
                    x
                };
                v *= self.linear_preamp;
                let slot = &mut self.lookahead_buffers[ch][index];
                outs[ch][i] = *slot;
                *slot = v * self.linear_postamp;
                let v = v.abs();
                if v > hot_sample {
                    hot_sample = v;
                }
            }

            let mut v = if is_linear || self.last <= 0.01 {
                self.last + self.recover
            } else {
                self.last + self.recover * self.last
            };
            if v > self.linear_threshold {
                v = self.linear_threshold;
            }
            let playback_gain = self.gain_buffer[index];
            self.gain_buffer[index] = v;

            if hot_sample * v > self.linear_threshold {
                let curgain = self.linear_threshold / hot_sample;
                let inc = self.linear_threshold - curgain;
                let mut acc = 0.;
                for j in 0..lookahead {
                    let k = (index + lookahead - j) % lookahead;
                    let newgain = if is_linear {
                        curgain + inc * acc
                    } else {
                        curgain + inc * (acc * acc)
                    };
                    if newgain < self.gain_buffer[k] {
                        self.gain_buffer[k] = newgain;
                    } else {
                        break;
                    }
                    acc += self.lookahead_inv;
                }
            }

            for out in outs.iter_mut().take(channels) {
                out[i] *= playback_gain;
            }

            self.last = self.gain_buffer[index];
            self.lookahead_index = if index + 1 >= lookahead { 0 } else { index + 1 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(l: &mut Limiter, input: &[Vec<f64>]) -> Vec<Vec<f64>> {
        let ins: Vec<&[f64]> = input.iter().map(Vec::as_slice).collect();
        let mut out = vec![vec![0.; input[0].len()]; input.len()];
        let mut outs: Vec<&mut [f64]> = out.iter_mut().map(Vec::as_mut_slice).collect();
        l.process(&ins, &mut outs);
        out
    }

    fn sine(amp: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| amp * (i as f64 * 0.05).sin()).collect()
    }

    #[test]
    fn dcblocker_matches_the_difference_equation() {
        let mut d = DcBlocker::default();
        assert_eq!(d.process(1.), 1.);
        assert_eq!(d.process(1.), 0.9997);
        d.clear();
        assert_eq!(d.process(0.5), 0.5);
    }

    #[test]
    fn quiet_input_is_delayed_by_the_lookahead() {
        let mut l = Limiter::new(1, 512, 48000.);
        l.set_dcblock(false);
        l.set_lookahead(10);
        let input = sine(0.5, 64);
        let out = run(&mut l, std::slice::from_ref(&input));
        assert_eq!(out[0][..10], [0.; 10]);
        assert_eq!(out[0][10..], input[..54]);
    }

    #[test]
    fn loud_input_is_limited_to_the_threshold() {
        for mode in [ResponseMode::Linear, ResponseMode::Exponential] {
            let mut l = Limiter::new(2, 512, 44100.);
            l.set_mode(mode);
            l.set_threshold(-6.);
            let input = vec![sine(4., 2000), sine(-0.25, 2000)];
            let out = run(&mut l, &input);
            let limit = db_to_linear(-6.) + 1e-12;
            let peak = out.iter().flatten().fold(0f64, |m, s| m.max(s.abs()));
            assert!(peak <= limit, "{mode:?}: {peak}");
            assert!(peak > 0.9 * limit, "{mode:?}: {peak}");
        }
    }

    #[test]
    fn postamp_scales_the_limited_output() {
        let mut l = Limiter::new(1, 512, 44100.);
        l.set_postamp(6.);
        let out = run(&mut l, &[sine(4., 2000)]);
        let peak = out[0].iter().fold(0f64, |m, s| m.max(s.abs()));
        assert!(peak <= db_to_linear(6.) + 1e-12);
    }

    #[test]
    fn bypass_copies_the_input() {
        let mut l = Limiter::new(2, 512, 44100.);
        l.set_bypass(true);
        let input = vec![sine(4., 64), sine(2., 64)];
        assert_eq!(run(&mut l, &input), input);
    }

    #[test]
    fn clear_forgets_the_history() {
        let mut l = Limiter::new(1, 512, 44100.);
        l.set_lookahead(4);
        run(&mut l, &[sine(8., 103)]);
        l.clear();
        let mut fresh = Limiter::new(1, 512, 44100.);
        fresh.set_lookahead(4);
        let input = [sine(0.3, 32)];
        assert_eq!(run(&mut l, &input), run(&mut fresh, &input));
    }

    #[test]
    fn lookahead_and_release_settings() {
        let mut l = Limiter::new(1, 200, 44100.);
        assert_eq!(l.lookahead(), 100);
        l.set_lookahead(1000);
        assert_eq!(l.lookahead(), 200);
        l.set_lookahead(-3);
        assert_eq!(l.lookahead(), 1);
        l.set_release(500.);
        assert_eq!(l.release(), 500.);
        l.reset(250., ResponseMode::Linear, 48000.);
        assert_eq!(l.recover, 1000. / (250. * 48000.) * 0.5);
    }
}
