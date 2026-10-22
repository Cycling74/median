//! A `new` that returns `Err`: Max gets null, the error is posted for the object, and everything
//! the builder made (outlets, inlets, proxies, clocks, buffer references; for MSP the dsp setup)
//! is released, through the `median-mock` kernel. (A panic after `z_dsp_setup`, which user code
//! can't cause, is tested in the wrapper's unit tests.)

use median::prelude::*;
use median_mock::{Atom as MockAtom, Kernel, Level};
use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How many `Picky`s have been dropped.
static DROPPED: AtomicUsize = AtomicUsize::new(0);

/// The first argument, which must be a non-negative int.
fn count(args: &[Atom]) -> Result<t_atom_long, NewError> {
    let Some(a) = args.first() else {
        return Err("needs a count".into());
    };
    if !matches!(a.get_type(), Some(median::atom::AtomType::Int)) {
        return Err(format!("the count must be an int, not {}", a.get_symbol()).into());
    }
    //`?` on a std error: a negative count does not fit a u32
    let n: u32 = u32::try_from(a.get_int())?;
    Ok(n as t_atom_long)
}

/// Makes everything the builder can make, then checks its argument.
#[derive(MaxObject)]
#[max(name = "picky", no_methods)]
pub struct Picky {
    _obj: ObjRef,
    _out: Outlet<List>,
    _clock: Clock,
    _buf: median::builder::ManagedBufferRef,
    _n: t_atom_long,
}

impl MaxObjWrapped for Picky {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::proxy());
        b.inlet(Inlet::int(|_: &Self, _| {}));
        let obj = b.obj();
        let out = b.outlet(Outlet::list());
        let clock = b.clock(|_| {});
        let buf = b.with_buffer(None);
        let n = count(b.creation_args())?;
        Ok(Self {
            _obj: obj,
            _out: out,
            _clock: clock,
            _buf: buf,
            _n: n,
        })
    }
}

impl Drop for Picky {
    fn drop(&mut self) {
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }
}

/// The MSP twin: signal inlets declared before it refuses, so `z_dsp_setup` never runs.
#[derive(MaxObject)]
#[max(name = "picky~", kind = msp, no_methods)]
pub struct PickyDsp {
    _clock: Clock,
}

impl MSPObjWrapped for PickyDsp {
    fn new(b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::signal());
        b.inlet(Inlet::signal());
        b.inlet(Inlet::proxy());
        b.outlet(Outlet::signal());
        let clock = b.clock(|_| {});
        count(b.creation_args())?;
        Ok(Self { _clock: clock })
    }

    fn perform(&self, _ins: &[&[f64]], _outs: &mut [&mut [f64]], _nframes: usize) {}
}

/// Panics in `new` after making things (the panic path frees them too).
#[derive(MaxObject)]
#[max(name = "panicky~", kind = msp, no_methods)]
pub struct Panicky;

impl MSPObjWrapped for Panicky {
    fn new(b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::signal());
        b.inlet(Inlet::proxy());
        let _clock = b.clock(|_| {});
        let _buf = b.with_buffer(None);
        panic!("no");
    }

    fn perform(&self, _ins: &[&[f64]], _outs: &mut [&mut [f64]], _nframes: usize) {}
}

median::ext_main! { Picky, PickyDsp, Panicky }

fn setup() -> Kernel {
    static ONCE: Once = Once::new();
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
    k
}

/// Make `class` with `args`, expect it refused with `msg`, and check that nothing is left.
fn refused(k: &Kernel, class: &str, args: &[MockAtom], msg: &str) {
    let clocks = k.live_clock_count();
    let bufs = k.live_buffer_ref_count();
    let obj = k.new_object(class, args);
    assert!(obj.is_null(), "{class} {args:?}");
    let log = k.take_console_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].level, Level::Error);
    assert_eq!(log[0].text, msg);
    //posted for the object (which is gone now), not globally
    assert_ne!(log[0].object, 0);
    assert_eq!(k.live_object_count(), 0);
    assert_eq!(k.live_clock_count(), clocks);
    assert_eq!(k.live_buffer_ref_count(), bufs);
}

#[test]
fn an_err_from_new_gives_max_null_and_frees_everything() {
    let k = setup();
    let dropped = DROPPED.load(Ordering::SeqCst);
    refused(&k, "picky", &[], "needs a count");
    refused(
        &k,
        "picky",
        &["x".into()],
        "the count must be an int, not x",
    );
    refused(
        &k,
        "picky",
        &[MockAtom::Long(-1)],
        "out of range integral type conversion attempted",
    );
    //`T` was never made, so never dropped
    assert_eq!(DROPPED.load(Ordering::SeqCst), dropped);

    //a good one is made, and freed with everything it has
    let clocks = k.live_clock_count();
    let obj = k.new_object("picky", &[MockAtom::Long(3)]);
    assert!(!obj.is_null());
    assert_eq!(k.live_clock_count(), clocks + 1);
    assert!(k.free_object(obj));
    assert_eq!(DROPPED.load(Ordering::SeqCst), dropped + 1);
    assert_eq!(k.live_clock_count(), clocks);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn an_err_from_an_msp_new_never_sets_up_dsp() {
    let k = setup();
    refused(&k, "picky~", &[], "needs a count");
    assert!(k.dsp_frees().is_empty());
    //made: `z_dsp_setup` ran, and `free` pairs it
    let obj = k.new_object("picky~", &[MockAtom::Long(0)]);
    assert_eq!(k.signal_inlets(obj), Some(2));
    assert!(k.free_object(obj));
    assert_eq!(k.dsp_frees(), [obj as usize]);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn a_panic_in_an_msp_new_frees_everything() {
    let k = setup();
    let clocks = k.live_clock_count();
    let bufs = k.live_buffer_ref_count();
    let obj = k.new_object("panicky~", &[]);
    assert!(obj.is_null());
    let log = k.take_console_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(
        log[0].text.contains("panic") && log[0].text.contains("no"),
        "{log:?}"
    );
    assert_eq!(k.live_object_count(), 0);
    assert_eq!(k.live_clock_count(), clocks);
    assert_eq!(k.live_buffer_ref_count(), bufs);
    assert!(k.dsp_frees().is_empty());
}
