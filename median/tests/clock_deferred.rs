//! Deferred clocks (`clock_deferred`, `clock_deferred_with`): the tick on the `median-mock`
//! scheduler (`Kernel::advance`) only sets a qelem, and the callback runs when the mock runs the
//! main queue (`Kernel::run_main_queue`).
//!
//! Every test takes `median_mock::lock()` for its whole body.

use median::prelude::*;
use median_mock::{Atom as MockAtom, Level, Role};
use std::sync::{
    Once,
    atomic::{AtomicUsize, Ordering},
};

/// A deferred clock from a method, one from a capturing closure, one whose callback fails, and
/// one that ticks while its own callback runs.
#[derive(MaxObject)]
#[max(name = "clock.deferred")]
pub struct Deferred {
    obj: ObjRef,
    tick: Clock,
    counted: Clock,
    failing: Clock,
    lost: Clock,
    out: Outlet<Int>,
    ticks: Int64,
    lost_runs: Int64,
}

/// What the `clock_deferred_with` closure of the last `Deferred` counts.
static COUNTED: AtomicUsize = AtomicUsize::new(0);

impl MaxObjWrapped for Deferred {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let step = std::hint::black_box(1);
        Ok(Self {
            obj: b.obj(),
            tick: b.clock_deferred(Self::tick),
            counted: b.clock_deferred_with(move |_: &Self| {
                COUNTED.fetch_add(step, Ordering::SeqCst);
            }),
            failing: b.clock_deferred(|_: &Self| -> Result<(), String> {
                Err("the deferred clock failed".into())
            }),
            lost: b.clock_deferred(Self::lost_run),
            out: b.outlet(Outlet::int()),
            ticks: Int64::new(0),
            lost_runs: Int64::new(0),
        })
    }
}

#[median::methods]
impl Deferred {
    #[int]
    pub fn int(&self, ms: t_atom_long) {
        self.tick.delay(ms as _);
    }

    #[sel("stop")]
    pub fn stop(&self) {
        self.tick.cancel();
    }

    #[sel("count")]
    pub fn count(&self) {
        self.counted.delay(1);
    }

    #[sel("fail")]
    pub fn fail(&self) {
        self.failing.delay(1);
    }

    #[sel("lost")]
    pub fn lost(&self) {
        self.lost.delay(1);
    }

    /// The first time, ticks its own clock while it runs (the scheduler runs from inside the
    /// callback): `qelem_set` from inside the qelem's own function does nothing.
    pub fn lost_run(&self) {
        let n = self.lost_runs.get() + 1;
        self.lost_runs.set(n);
        if n == 1 {
            self.lost.delay(1);
            median_mock::Kernel::advance_nested(1.0);
        }
    }

    pub fn tick(&self) {
        let n = self.ticks.get() + 1;
        self.ticks.set(n);
        let _ = self.out.send(n);
        self.obj.post(format!(
            "main thread: {}",
            median::thread::SysThread::is_main()
        ));
    }
}

median::ext_main! { Deferred }

fn register() {
    static ONCE: Once = Once::new();
    // SAFETY: called once, as Max does
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
}

#[test]
fn a_tick_runs_the_callback_once_on_the_main_thread_when_the_queue_runs() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("clock.deferred", &[]);
    assert!(!obj.is_null());
    k.send_int(obj, 10).unwrap();
    assert_eq!(k.advance(10.), 1);
    //the scheduler ran the tick, which only set the qelem
    assert!(k.outlet_log().is_empty());
    assert_eq!(k.main_queue_len(), 1);
    assert_eq!(k.run_main_queue(), 1);
    let log = k.take_outlet_log();
    assert_eq!(log.len(), 1);
    assert_eq!(
        (log[0].atoms.as_slice(), log[0].role),
        (&[MockAtom::Long(1)][..], Role::Main)
    );
    assert_eq!(k.take_console_log()[0].text, "main thread: true");
    //nothing is left over, and it can tick again
    assert_eq!(k.run_main_queue(), 0);
    k.send_int(obj, 1).unwrap();
    assert_eq!(k.advance(1.), 1);
    assert_eq!(k.run_main_queue(), 1);
    assert_eq!(k.take_outlet_log().len(), 1);
    assert!(k.free_object(obj));
}

#[test]
fn ticks_before_the_queue_runs_coalesce_into_one_call() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("clock.deferred", &[]);
    k.send_int(obj, 1).unwrap();
    assert_eq!(k.advance(1.), 1);
    k.send_int(obj, 1).unwrap();
    assert_eq!(k.advance(1.), 1);
    assert_eq!(k.main_queue_len(), 1);
    assert_eq!(k.run_main_queue(), 1);
    assert_eq!(k.take_outlet_log().len(), 1);
    assert!(k.free_object(obj));
}

#[test]
fn cancel_after_a_tick_and_before_the_queue_runs_gives_no_call() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("clock.deferred", &[]);
    k.send_int(obj, 1).unwrap();
    assert_eq!(k.advance(1.), 1);
    k.send(obj, "stop", &[]).unwrap();
    assert_eq!(k.run_main_queue(), 0);
    assert!(k.outlet_log().is_empty());
    //cancel before the tick, too
    k.send_int(obj, 5).unwrap();
    k.send(obj, "stop", &[]).unwrap();
    assert_eq!(k.advance(10.), 0);
    assert_eq!(k.run_main_queue(), 0);
    assert!(k.outlet_log().is_empty());
    //and it is not stuck: it ticks again
    k.send_int(obj, 1).unwrap();
    assert_eq!(k.advance(1.), 1);
    assert_eq!(k.run_main_queue(), 1);
    assert_eq!(k.take_outlet_log().len(), 1);
    assert!(k.free_object(obj));
}

#[test]
fn freeing_the_object_with_a_tick_queued_gives_no_call_and_frees_both() {
    let k = median_mock::lock();
    register();
    let clocks = k.live_clock_count();
    let qelems = k.live_qelem_count();
    let obj = k.new_object("clock.deferred", &[]);
    //the four deferred clocks have a clock and a qelem each
    assert_eq!(k.live_clock_count(), clocks + 4);
    assert_eq!(k.live_qelem_count(), qelems + 4);
    COUNTED.store(0, Ordering::SeqCst);
    k.send_int(obj, 1).unwrap();
    k.send(obj, "count", &[]).unwrap();
    //both ticked: their qelems are set
    assert_eq!(k.advance(1.), 2);
    k.send(obj, "count", &[]).unwrap();
    //and the second `count` is still a pending clock
    assert!(k.free_object(obj));
    assert_eq!(k.live_clock_count(), clocks);
    assert_eq!(k.live_qelem_count(), qelems);
    assert_eq!(k.advance(100.), 0);
    assert_eq!(k.run_main_queue(), 0);
    assert!(k.outlet_log().is_empty());
    assert_eq!(COUNTED.load(Ordering::SeqCst), 0);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn clock_deferred_with_runs_a_capturing_closure_and_errs_are_reported() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("clock.deferred", &[]);
    COUNTED.store(0, Ordering::SeqCst);
    k.send(obj, "count", &[]).unwrap();
    assert_eq!(k.advance(1.), 1);
    assert_eq!(COUNTED.load(Ordering::SeqCst), 0);
    assert_eq!(k.run_main_queue(), 1);
    assert_eq!(COUNTED.load(Ordering::SeqCst), 1);

    k.send(obj, "fail", &[]).unwrap();
    assert_eq!(k.advance(1.), 1);
    assert!(k.console_log().is_empty());
    assert_eq!(k.run_main_queue(), 1);
    let log = k.take_console_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(
        (log[0].level, log[0].object, log[0].text.as_str()),
        (Level::Error, obj as usize, "the deferred clock failed")
    );
    assert!(k.free_object(obj));
}

#[test]
fn a_tick_while_the_callback_runs_is_lost() {
    //`ext_proto.h`: a qelem is unset only after its function returns, so `qelem_set` from inside
    //it (here, by a tick that the scheduler delivers from inside the callback) does nothing
    let k = median_mock::lock();
    register();
    let obj = k.new_object("clock.deferred", &[]);
    k.send(obj, "lost", &[]).unwrap();
    assert_eq!(k.advance(1.), 1);
    assert_eq!(k.run_main_queue(), 1);
    assert_eq!(k.main_queue_len(), 0);
    assert_eq!(k.run_main_queue(), 0);
    //a tick after the callback returned runs it again
    k.send(obj, "lost", &[]).unwrap();
    assert_eq!(k.advance(1.), 1);
    assert_eq!(k.run_main_queue(), 1);
    assert!(k.free_object(obj));
}
