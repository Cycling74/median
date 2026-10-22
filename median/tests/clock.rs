//! `Clock`, made with the builder's `clock` and `clock_with`, driven by the `median-mock`
//! scheduler (`Kernel::advance`).
//!
//! Every test takes `median_mock::lock()` for its whole body.

use median::prelude::*;
use median_mock::{Atom as MockAtom, Level, Role};
use std::sync::{
    Arc, Once,
    atomic::{AtomicUsize, Ordering},
};

/// A clock from a method, one from a capturing closure, and one whose callback fails.
#[derive(MaxObject)]
#[max(name = "clock.ticker")]
pub struct Ticker {
    obj: ObjRef,
    tick: Clock,
    counted: Clock,
    failing: Clock,
    nested: Clock,
    out: Outlet<Int>,
    ticks: Int64,
    nested_runs: Int64,
}

/// What the `clock_with` closure of the last `Ticker` counts.
static COUNTED: AtomicUsize = AtomicUsize::new(0);

impl MaxObjWrapped for Ticker {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        Ok(Self {
            obj: b.obj(),
            tick: b.clock(Self::tick),
            counted: b.clock_with(move |_: &Self| {
                let n = c.fetch_add(1, Ordering::SeqCst) + 1;
                COUNTED.store(n, Ordering::SeqCst);
            }),
            failing: b.clock(|_: &Self| -> Result<(), String> { Err("the clock failed".into()) }),
            nested: b.clock(Self::nested_tick),
            out: b.outlet(Outlet::int()),
            ticks: Int64::new(0),
            nested_runs: Int64::new(0),
        })
    }
}

#[median::methods]
impl Ticker {
    #[int]
    pub fn int(&self, ms: t_atom_long) {
        self.tick.delay(ms as _);
    }

    #[float]
    pub fn float(&self, ms: f64) {
        self.tick.fdelay(ms);
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

    #[sel("nested")]
    pub fn nested(&self) {
        self.nested.delay(1);
    }

    /// The first time, fires itself again nested: it reschedules and runs the scheduler from
    /// inside the callback (with Overdrive off, a modal dialog's event loop does).
    pub fn nested_tick(&self) {
        let n = self.nested_runs.get() + 1;
        self.nested_runs.set(n);
        self.obj.post(format!("nested {n} start"));
        if n == 1 {
            self.nested.delay(1);
            median_mock::Kernel::advance_nested(1.0);
        }
        self.obj.post(format!("nested {n} end"));
    }

    pub fn tick(&self) {
        let n = self.ticks.get() + 1;
        self.ticks.set(n);
        let _ = self.out.send(n);
        self.obj.post(format!("tick at {}", Clock::ftime()));
    }
}

median::ext_main! { Ticker }

fn register() {
    static ONCE: Once = Once::new();
    // SAFETY: called once, as Max does
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
}

#[test]
fn a_clock_fires_after_advance_on_the_scheduler_thread() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("clock.ticker", &[]);
    assert!(!obj.is_null());
    k.send_int(obj, 10).unwrap();
    assert_eq!(k.advance(9.), 0);
    assert!(k.outlet_log().is_empty());
    assert_eq!(k.advance(1.), 1);
    let log = k.take_outlet_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].atoms, [MockAtom::Long(1)]);
    assert_eq!((log[0].time, log[0].role), (10., Role::Timer));
    let console = k.take_console_log();
    assert_eq!(console.len(), 1);
    assert_eq!(console[0].text, "tick at 10");

    //a fractional delay; a later schedule replaces an earlier one
    k.send_float(obj, 2.5).unwrap();
    k.send_float(obj, 4.5).unwrap();
    assert_eq!(k.advance(4.), 0);
    assert_eq!(k.advance(1.), 1);
    assert_eq!(k.take_outlet_log()[0].time, 14.5);
    assert!(k.free_object(obj));
}

#[test]
fn cancel_unsets_the_clock() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("clock.ticker", &[]);
    k.send_int(obj, 10).unwrap();
    k.send(obj, "stop", &[]).unwrap();
    assert_eq!(k.advance(100.), 0);
    assert!(k.outlet_log().is_empty());
    //and it can be scheduled again
    k.send_int(obj, 1).unwrap();
    assert_eq!(k.advance(1.), 1);
    assert!(k.free_object(obj));
}

#[test]
fn a_clock_pending_at_free_does_not_fire_and_is_freed() {
    let k = median_mock::lock();
    register();
    let clocks = k.live_clock_count();
    let obj = k.new_object("clock.ticker", &[]);
    assert_eq!(k.live_clock_count(), clocks + 4);
    k.send_int(obj, 10).unwrap();
    k.send(obj, "count", &[]).unwrap();
    assert!(k.free_object(obj));
    assert_eq!(k.live_clock_count(), clocks);
    assert_eq!(k.advance(100.), 0);
    assert!(k.outlet_log().is_empty());
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn clock_with_runs_a_capturing_closure() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("clock.ticker", &[]);
    COUNTED.store(0, Ordering::SeqCst);
    k.send(obj, "count", &[]).unwrap();
    assert_eq!(k.advance(1.), 1);
    k.send(obj, "count", &[]).unwrap();
    assert_eq!(k.advance(1.), 1);
    assert_eq!(COUNTED.load(Ordering::SeqCst), 2);
    assert!(k.free_object(obj));
}

#[test]
fn an_err_from_a_clock_callback_is_reported_for_the_object() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("clock.ticker", &[]);
    k.send(obj, "fail", &[]).unwrap();
    assert_eq!(k.advance(1.), 1);
    let log = k.take_console_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(
        (log[0].level, log[0].object, log[0].text.as_str()),
        (Level::Error, obj as usize, "the clock failed")
    );
    assert!(k.free_object(obj));
}

#[test]
fn a_clock_callback_can_run_again_nested_inside_itself() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("clock.ticker", &[]);
    k.send(obj, "nested", &[]).unwrap();
    //the outer advance fires it once; the second run is nested inside the first
    assert_eq!(k.advance(10.0), 1);
    let log: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
    assert_eq!(
        log,
        [
            "nested 1 start",
            "nested 2 start",
            "nested 2 end",
            "nested 1 end"
        ]
    );
    //and the object can still be freed: nothing is left held
    assert!(k.free_object(obj));
}
