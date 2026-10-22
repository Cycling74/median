//! The audio-to-main-thread pattern of `median::fifo`: `perform` pushes into a ring buffer and
//! sets a `Queue`, whose callback drains it on the main thread. Run through `median-mock`
//! (`DspAdd::run` on the audio role, `Kernel::run_main_queue` on the main role).

use median::fifo::{Consumer, Producer, RingBuffer, Shared};
use median::prelude::*;
use median_mock::{Atom as MockAtom, Role};
use std::sync::Once;

#[derive(MaxObject)]
#[max(name = "fifo.peak~", kind = msp)]
pub struct Peak {
    tx: Shared<Producer<f64>>,
    rx: Shared<Consumer<f64>>,
    drain: Queue,
    out: Outlet<Float>,
    dropped: Int64,
}

impl MSPObjWrapped for Peak {
    fn new(b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::signal());
        let (tx, rx) = RingBuffer::new(2);
        Ok(Self {
            tx: Shared::new(tx),
            rx: Shared::new(rx),
            drain: b.queue(Self::drain),
            out: b.outlet(Outlet::float()),
            dropped: Int64::new(0),
        })
    }

    fn perform(&self, ins: &[&[f64]], _outs: &mut [&mut [f64]], _nframes: usize) {
        let peak = ins[0].iter().fold(0f64, |m, v| m.max(v.abs()));
        if !matches!(self.tx.try_with(|tx| tx.push(peak)), Some(Ok(()))) {
            self.dropped.set(self.dropped.get() + 1);
        }
        self.drain.set();
    }
}

#[median::methods]
impl Peak {
    pub fn drain(&self) {
        self.rx.try_with(|rx| {
            while let Ok(peak) = rx.pop() {
                let _ = self.out.send(peak);
            }
        });
    }
}

median::ext_main! { Peak }

fn register() {
    static ONCE: Once = Once::new();
    // SAFETY: called once, as Max does
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
}

#[test]
fn perform_pushes_on_the_audio_thread_and_the_queue_drains_on_main() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("fifo.peak~", &[]);
    assert!(!obj.is_null());
    let adds = k.dsp64(obj, 44100., 4).unwrap();
    assert_eq!(adds.len(), 1);
    let mut outs: Vec<Vec<f64>> = vec![];
    for block in [[0.1, -0.5, 0.2, 0.], [0.3, 0.3, -0.25, 0.]] {
        adds[0].run(&mut [block.to_vec()], &mut outs);
    }
    //nothing was sent from the audio thread; one queue run is pending
    assert!(k.outlet_log().is_empty());
    assert_eq!(k.main_queue_len(), 1);
    assert_eq!(k.run_main_queue(), 1);
    let log = k.take_outlet_log();
    let got: Vec<_> = log.iter().map(|e| (e.atoms.clone(), e.role)).collect();
    assert_eq!(
        got,
        [
            (vec![MockAtom::Float(0.5)], Role::Main),
            (vec![MockAtom::Float(0.3)], Role::Main)
        ]
    );

    //a full ring buffer drops values, it never blocks or grows
    for _ in 0..3 {
        adds[0].run(&mut [vec![1.; 4]], &mut outs);
    }
    assert_eq!(k.run_main_queue(), 1);
    assert_eq!(k.take_outlet_log().len(), 2);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
    assert!(k.free_object(obj));
}
