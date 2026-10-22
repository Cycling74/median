//! `Queue` (qelem), made with the builder's `queue` and `queue_with`, run by the `median-mock`
//! main queue (`Kernel::run_main_queue`).
//!
//! Every test takes `median_mock::lock()` for its whole body.

use median::prelude::*;
use median_mock::{Atom as MockAtom, Level, Role};
use std::sync::{
    Once,
    atomic::{AtomicUsize, Ordering},
};

#[derive(MaxObject)]
#[max(name = "queue.thing")]
pub struct Thing {
    obj: ObjRef,
    redraw: Queue,
    counted: Queue,
    failing: Queue,
    nested: Queue,
    out: Outlet<Int>,
    value: Int64,
    nested_runs: Int64,
}

static COUNTED: AtomicUsize = AtomicUsize::new(0);

impl MaxObjWrapped for Thing {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let step = std::hint::black_box(1);
        Ok(Self {
            obj: b.obj(),
            redraw: b.queue(Self::redraw),
            counted: b.queue_with(move |_: &Self| {
                COUNTED.fetch_add(step, Ordering::SeqCst);
            }),
            failing: b.queue(|_: &Self| -> Result<(), &'static str> { Err("the queue failed") }),
            nested: b.queue(Self::nested_run),
            out: b.outlet(Outlet::int()),
            value: Int64::new(0),
            nested_runs: Int64::new(0),
        })
    }
}

#[median::methods]
impl Thing {
    #[int]
    pub fn int(&self, v: t_atom_long) {
        self.value.set(v);
        self.redraw.set();
    }

    #[sel("first")]
    pub fn first(&self) {
        self.redraw.front();
    }

    #[sel("stop")]
    pub fn stop(&self) {
        self.redraw.cancel();
    }

    #[sel("count")]
    pub fn count(&self) {
        self.counted.set();
    }

    #[sel("fail")]
    pub fn fail(&self) {
        self.failing.set();
    }

    #[sel("nested")]
    pub fn nested(&self) {
        self.nested.set();
    }

    /// The first time, sets itself again and runs the main queue nested (as a modal dialog's
    /// event loop would). The `set` does nothing: Max unsets a qelem after its function returns.
    pub fn nested_run(&self) {
        let n = self.nested_runs.get() + 1;
        self.nested_runs.set(n);
        self.obj.post(format!("nested {n} start"));
        if n == 1 {
            self.nested.set();
            median_mock::Kernel::run_main_queue_nested();
        }
        self.obj.post(format!("nested {n} end"));
    }

    pub fn redraw(&self) {
        let _ = self.out.send(self.value.get());
        self.obj.post(format!(
            "main thread: {}",
            median::thread::SysThread::is_main()
        ));
    }
}

median::ext_main! { Thing }

fn register() {
    static ONCE: Once = Once::new();
    // SAFETY: called once, as Max does
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
}

#[test]
fn a_queue_set_many_times_runs_once_on_the_main_thread() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("queue.thing", &[]);
    assert!(!obj.is_null());
    //from the timer thread too
    k.with_role(Role::Timer, || {
        for v in 1..=5 {
            k.send_int(obj, v).unwrap();
        }
    });
    k.send_int(obj, 6).unwrap();
    assert!(k.outlet_log().is_empty());
    assert_eq!(k.run_main_queue(), 1);
    let log = k.take_outlet_log();
    assert_eq!(log.len(), 1);
    assert_eq!(
        (log[0].atoms.as_slice(), log[0].role),
        (&[MockAtom::Long(6)][..], Role::Main)
    );
    assert_eq!(k.take_console_log()[0].text, "main thread: true");
    //it is unset once it has run, so it can be set again
    k.send_int(obj, 7).unwrap();
    assert_eq!(k.run_main_queue(), 1);
    assert!(k.free_object(obj));
}

#[test]
fn front_goes_ahead_and_cancel_unsets() {
    let k = median_mock::lock();
    register();
    let a = k.new_object("queue.thing", &[]);
    let b = k.new_object("queue.thing", &[]);
    k.send_int(a, 1).unwrap();
    k.send_int(b, 2).unwrap();
    //b's queue is already set: front leaves it where it is. This tests median-mock's model of
    //`qelem_front` on a set qelem, not Max (the SDK doesn't say; unverified)
    k.send(b, "first", &[]).unwrap();
    assert_eq!(k.run_main_queue(), 2);
    let order: Vec<_> = k
        .take_outlet_log()
        .iter()
        .map(|e| e.atoms.clone())
        .collect();
    assert_eq!(order, [[MockAtom::Long(1)], [MockAtom::Long(2)]]);
    //unset, front goes ahead of a's
    k.send_int(a, 3).unwrap();
    k.send(b, "first", &[]).unwrap();
    assert_eq!(k.run_main_queue(), 2);
    let order: Vec<_> = k
        .take_outlet_log()
        .iter()
        .map(|e| e.atoms.clone())
        .collect();
    assert_eq!(order, [[MockAtom::Long(2)], [MockAtom::Long(3)]]);
    //cancel
    k.send_int(a, 4).unwrap();
    k.send(a, "stop", &[]).unwrap();
    k.run_main_queue();
    assert!(k.outlet_log().is_empty());
    assert!(k.free_object(a));
    assert!(k.free_object(b));
}

#[test]
fn a_queue_pending_at_free_never_runs_and_is_freed() {
    let k = median_mock::lock();
    register();
    let qelems = k.live_qelem_count();
    let obj = k.new_object("queue.thing", &[]);
    assert_eq!(k.live_qelem_count(), qelems + 4);
    k.send_int(obj, 1).unwrap();
    k.send(obj, "count", &[]).unwrap();
    COUNTED.store(0, Ordering::SeqCst);
    assert!(k.free_object(obj));
    assert_eq!(k.live_qelem_count(), qelems);
    assert_eq!(k.run_main_queue(), 0);
    assert!(k.outlet_log().is_empty());
    assert_eq!(COUNTED.load(Ordering::SeqCst), 0);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn queue_with_runs_a_capturing_closure_and_errs_are_reported() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("queue.thing", &[]);
    COUNTED.store(0, Ordering::SeqCst);
    k.send(obj, "count", &[]).unwrap();
    k.send(obj, "count", &[]).unwrap();
    assert_eq!(k.run_main_queue(), 1);
    assert_eq!(COUNTED.load(Ordering::SeqCst), 1);

    k.send(obj, "fail", &[]).unwrap();
    assert_eq!(k.run_main_queue(), 1);
    let log = k.take_console_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(
        (log[0].level, log[0].object, log[0].text.as_str()),
        (Level::Error, obj as usize, "the queue failed")
    );
    assert!(k.free_object(obj));
}

#[test]
fn a_queue_set_inside_its_own_callback_does_nothing() {
    //`ext_proto.h`: "A Qelem object is unset after its queue function has been called"
    let k = median_mock::lock();
    register();
    let obj = k.new_object("queue.thing", &[]);
    k.send(obj, "nested", &[]).unwrap();
    assert_eq!(k.run_main_queue(), 1);
    //neither the nested event loop nor a later run runs it again
    let log: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
    assert_eq!(log, ["nested 1 start", "nested 1 end"]);
    assert_eq!(k.main_queue_len(), 0);
    assert_eq!(k.run_main_queue(), 0);
    //a set after the callback returned runs it again
    k.send(obj, "nested", &[]).unwrap();
    assert_eq!(k.run_main_queue(), 1);
    let log: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
    assert_eq!(log, ["nested 2 start", "nested 2 end"]);
    assert!(k.free_object(obj));
}
