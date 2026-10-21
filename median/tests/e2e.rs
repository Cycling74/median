//! End to end tests of the wrapper: externals defined with the current API are registered, made
//! and sent messages through the `median-mock` kernel, which stands in for Max, and what they do
//! is read from the kernel's outlet and console logs.
//!
//! Four externals cover the ways of defining one:
//! * `Thing` uses `median::external!` (macro generated trampolines, `ext_main`);
//! * `Typed` is hand written: `MaxObjWrapped` + `Class::add_*`, registered with
//!   `MaxObjWrapper::<T>::register(false)`;
//! * `Ticker` is hand written too, and has a clock;
//! * `Dsp` is the same for MSP, with `MSPObjWrapper::<T>::register(false)`.
//!
//! Every test takes `median_mock::lock()` for its whole body. Classes stay registered in the
//! kernel (and in median) between tests, so registering is done once per process.

use median::{
    atom::Atom,
    attr::{AttrBuilder, AttrType},
    builder::{MSPWrappedBuilder, MaxWrappedBuilder},
    class::Class,
    clock::ClockHandle,
    inlet::MaxInlet,
    max_sys::t_atom_long,
    num::{Float64, Int64},
    object::ObjRef,
    outlet::{OutBang, OutInt, OutList},
    symbol::SymbolRef,
    wrapper::{
        MSPObjWrapped, MSPObjWrapper, MaxObjWrapped, MaxObjWrapper, ObjWrapped, attr_get_tramp,
        attr_set_tramp,
    },
};
use median_mock::{Atom as MockAtom, AttrError, Level, Role, SendError};
use std::sync::{
    Once,
    atomic::{AtomicUsize, Ordering},
};

/// How many `Thing`s and `Typed`s have been dropped.
static DROPPED: AtomicUsize = AtomicUsize::new(0);

median::external! {
    #[name = "e2e.thing"]
    pub struct Thing {
        obj: ObjRef,
        value: Int64,
        fvalue: Float64,
                int_out: OutInt,
        list_out: OutList,
    }

    impl MaxObjWrapped<Thing> for Thing {
        fn new(builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
            let start = builder.creation_args().first().map_or(0, |a| a.get_int());
            let _ = builder.add_inlet(MaxInlet::Proxy);
            Self {
                obj: builder.obj(),
                value: Int64::new(start),
                fvalue: Float64::new(0.),
                int_out: builder.add_int_outlet(),
                list_out: builder.add_list_outlet(),
            }
        }

        fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
            c.add_attribute(
                AttrBuilder::new_accessors(
                    "foo",
                    AttrType::Float64,
                    Self::foo_tramp,
                    Self::set_foo_tramp,
                )
                .build()
                .unwrap(),
            )
            .unwrap();
            c.add_attribute(
                AttrBuilder::new_accessors(
                    "blah",
                    AttrType::Int64,
                    Self::blah_tramp,
                    Self::set_blah_tramp,
                )
                .build()
                .unwrap(),
            )
            .unwrap();
        }
    }

    impl Thing {
        #[bang]
        pub fn bang(&self) {
            median::object_post!(&self.obj, "bang from inlet {}", self.obj.proxy_inlet());
        }

        #[int]
        pub fn int(&self, v: t_atom_long) {
            self.value.set(v);
            let _ = self.int_out.send(v * 2);
        }

        #[list]
        pub fn list(&self, atoms: &[Atom]) {
            let rev: Vec<Atom> = atoms
                .iter()
                .rev()
                .map(|a| Atom::from(a.get_value().unwrap()))
                .collect();
            let _ = self.list_out.send(&rev);
        }

        #[attr_get_tramp]
        pub fn foo(&self) -> f64 {
            self.fvalue.get()
        }

        #[attr_set_tramp]
        pub fn set_foo(&self, v: f64) {
            self.fvalue.set(v);
        }

        #[attr_get_tramp]
        pub fn blah(&self) -> t_atom_long {
            self.value.get()
        }

        #[attr_set_tramp]
        pub fn set_blah(&self, v: t_atom_long) {
            self.value.set(v);
        }
    }
}

impl Drop for Thing {
    fn drop(&mut self) {
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }
}

/// Hand written, with typed methods, errors and a sel method.
pub struct Typed {
    obj: ObjRef,
    out: OutList,
}

impl Drop for Typed {
    fn drop(&mut self) {
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }
}

impl ObjWrapped<Typed> for Typed {
    fn class_name() -> &'static str {
        "e2e.typed"
    }
}

impl MaxObjWrapped<Typed> for Typed {
    fn new(builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
        if builder.creation_args().first().map(|a| a.get_int()) == Some(-1) {
            panic!("no -1 for you");
        }
        Self {
            obj: builder.obj(),
            out: builder.add_list_outlet(),
        }
    }

    fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
        c.add_bang(Self::bang).unwrap();
        c.add_int(Self::int).unwrap();
        c.add_float(Self::float).unwrap();
        c.add_list(Self::list).unwrap();
        c.add_any(Self::anything).unwrap();
        //"set" has a symbol argument that defaults if left out
        c.add_sel(c"set", Self::set, 1).unwrap();
        c.add_sel(c"range", Self::range, 0).unwrap();
    }
}

impl Typed {
    fn bang(&self) {
        let _ = self.out.send(&[]);
    }

    /// Fails for a negative value, which the default `report_error` posts to the console.
    fn int(&self, v: t_atom_long) -> Result<(), String> {
        if v < 0 {
            return Err(format!("negative: {v}"));
        }
        let _ = self.out.send(&[v.into()]);
        Ok(())
    }

    fn float(&self, v: f64) {
        let _ = self.out.send(&[v.into()]);
    }

    fn list(&self, atoms: &[Atom]) {
        let _ = self.out.send(atoms);
    }

    fn anything(&self, sel: &SymbolRef, atoms: &[Atom]) {
        median::object_warn!(&self.obj, "anything {} {}", sel, atoms.len());
    }

    fn set(&self, name: SymbolRef) {
        median::object_post!(&self.obj, "set {}", name);
    }

    fn range(&self, lo: f64, hi: t_atom_long) -> Result<(), &'static str> {
        if (hi as f64) < lo {
            Err("empty range")
        } else {
            let _ = self.out.send(&[lo.into(), hi.into()]);
            Ok(())
        }
    }
}

/// Has a clock: `bang` starts it, and it sends a bang and a list when it fires.
pub struct Ticker {
    clock: ClockHandle,
    bang_out: OutBang,
    list_out: OutList,
    ticks: Int64,
}

impl ObjWrapped<Ticker> for Ticker {
    fn class_name() -> &'static str {
        "e2e.ticker"
    }
}

impl MaxObjWrapped<Ticker> for Ticker {
    fn new(builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
        Self {
            clock: builder.with_clockfn(Self::clocked),
            bang_out: builder.add_bang_outlet(),
            list_out: builder.add_list_outlet(),
            ticks: Int64::new(0),
        }
    }

    fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
        c.add_bang(|t: &Self| t.clock.delay(10)).unwrap();
    }
}

impl Ticker {
    fn clocked(&self) {
        let n = self.ticks.get() + 1;
        self.ticks.set(n);
        let _ = self.bang_out.send(());
        let _ = self.list_out.send(&[n.into(), 1.5f64.into()]);
    }
}

/// An MSP external like `hello_dsp`: two signal inlets, one signal outlet.
pub struct Dsp {
    obj: ObjRef,
}

impl ObjWrapped<Dsp> for Dsp {
    fn class_name() -> &'static str {
        "e2e.dsp~"
    }
}

impl MSPObjWrapped<Dsp> for Dsp {
    fn new(builder: &mut dyn MSPWrappedBuilder<Self>) -> Self {
        builder.add_signal_inlets_with_assist(&["in left", "in right"]);
        builder.add_signal_outlets_with_assist(&["out"]);
        Self { obj: builder.obj() }
    }

    fn dsp_setup(&self, sample_rate: f64) {
        median::object_post!(&self.obj, "sample rate: {}", sample_rate);
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], nframes: usize) {
        for i in 0..nframes {
            outs[0][i] = ins[0][i] * 2. + ins[1][i];
        }
    }
}

fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // `external!` made `ext_main`, which registers `Thing`
        // SAFETY: that is how Max calls it, on its main thread
        unsafe { ext_main(std::ptr::null_mut()) };
        // SAFETY: as above, for the hand written ones
        unsafe {
            MaxObjWrapper::<Typed>::register(false);
            MaxObjWrapper::<Ticker>::register(false);
            MSPObjWrapper::<Dsp>::register(false);
        }
    });
}

/// The text of the console entries of a level, in order.
fn console(k: &median_mock::Kernel, level: Level) -> Vec<String> {
    k.console_log()
        .into_iter()
        .filter(|e| e.level == level)
        .map(|e| e.text)
        .collect()
}

fn long(v: i64) -> MockAtom {
    MockAtom::Long(v)
}

#[test]
fn registration_makes_the_classes_with_their_methods_and_attributes() {
    let k = median_mock::lock();
    register();
    assert!(k.class_exists("e2e.thing"));
    assert!(k.class_exists("e2e.typed"));
    assert!(k.class_exists("e2e.dsp~"));

    let thing = k.new_object("e2e.thing", &[]);
    assert_eq!(k.class_name_of(thing).as_deref(), Some("e2e.thing"));
    for sel in ["bang", "int", "list", "notify", "assist", "in1", "ft1"] {
        assert!(k.method(thing, sel).is_some(), "{sel}");
    }
    let mut attrs = k.attr_names(thing);
    attrs.sort();
    assert_eq!(attrs, ["blah", "foo"]);
    //the outlets, left to right
    assert_eq!(k.outlet_count(thing), 2);
    assert!(k.free_object(thing));

    //registering again is a no-op: the class is the same
    // SAFETY: as in `register`
    unsafe { MaxObjWrapper::<Typed>::register(false) };
    let typed = k.new_object("e2e.typed", &[]);
    for sel in ["bang", "int", "float", "list", "anything", "set", "range"] {
        assert!(k.method(typed, sel).is_some(), "{sel}");
    }
    assert!(k.free_object(typed));
    assert_eq!(k.live_object_count(), 0);
}

#[test]
fn external_macro_object_handles_messages_and_sends_to_outlets() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("e2e.thing", &[long(5)]);
    assert!(!obj.is_null());

    //int: stored, and sent doubled to the int outlet, which is the left one (the last added, as Max makes outlets right to left)
    k.send_int(obj, 21).unwrap();
    let log = k.take_outlet_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].owner, obj as usize);
    assert_eq!(log[0].selector, "int");
    assert_eq!(log[0].atoms, [long(42)]);
    assert_eq!(log[0].index, 0);
    assert_eq!(k.attr_get(obj, "blah").unwrap(), [long(21)]);

    //list: reversed, to the right outlet
    k.send_list(obj, &[1.into(), 2.5.into(), "x".into()]).unwrap();
    let log = k.take_outlet_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].selector, "list");
    assert_eq!(log[0].index, 1);
    assert_eq!(
        log[0].atoms,
        [MockAtom::Sym("x".into()), 2.5.into(), long(1)]
    );

    //bang: posted for the object, from the leftmost inlet (0), with nothing sent yet
    k.send_bang(obj).unwrap();
    let c = k.console_log();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].level, Level::Post);
    assert_eq!(c[0].object, obj as usize);
    assert_eq!(c[0].text, "bang from inlet 0");
    assert!(k.outlet_log().is_empty());

    //the proxy inlet: the same method, `proxy_inlet` says which inlet it came in at
    k.send_inlet(obj, 1, "bang", &[]).unwrap();
    assert_eq!(console(&k, Level::Post)[1], "bang from inlet 1");

    //a message the class doesn't have is refused and the kernel says so
    assert!(matches!(
        k.send(obj, "nonesuch", &[]),
        Err(SendError::NoMethod(_))
    ));
    assert_eq!(console(&k, Level::Error).len(), 1);

    //dropping the object runs the wrapped value's `Drop`
    let dropped = DROPPED.load(Ordering::SeqCst);
    assert!(k.free_object(obj));
    assert_eq!(DROPPED.load(Ordering::SeqCst), dropped + 1);
    assert_eq!(k.live_object_count(), 0);
}

#[test]
fn creation_arguments_reach_new() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("e2e.thing", &[long(7)]);
    assert_eq!(k.attr_get(obj, "blah").unwrap(), [long(7)]);
    k.free_object(obj);
    let obj = k.new_object("e2e.thing", &[]);
    assert_eq!(k.attr_get(obj, "blah").unwrap(), [long(0)]);
    k.free_object(obj);
}

#[test]
fn typed_methods_send_and_post() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("e2e.typed", &[]);

    k.send_bang(obj).unwrap();
    k.send_int(obj, 3).unwrap();
    k.send_float(obj, 0.25).unwrap();
    k.send_list(obj, &[1.into(), "a".into()]).unwrap();
    let log = k.take_outlet_log();
    let sent: Vec<_> = log.iter().map(|e| (e.selector.as_str(), &e.atoms[..])).collect();
    assert_eq!(
        sent,
        [
            ("list", &[][..]),
            ("list", &[long(3)][..]),
            ("list", &[MockAtom::Float(0.25)][..]),
            ("list", &[long(1), MockAtom::Sym("a".into())][..]),
        ]
    );

    //"anything" is what an unknown selector falls back to
    k.send(obj, "frob", &[1.into(), 2.into()]).unwrap();
    let c = k.take_console_log();
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].level, c[0].object), (Level::Warn, obj as usize));
    assert_eq!(c[0].text, "anything frob 2");

    //a typed selector, with a defaulted symbol
    k.send(obj, "set", &["hello".into()]).unwrap();
    k.send(obj, "set", &[]).unwrap();
    assert_eq!(console(&k, Level::Post), ["set hello", "set "]);

    //two typed arguments: an int atom is a valid float argument
    k.send(obj, "range", &[1.into(), 9.into()]).unwrap();
    assert_eq!(
        k.take_outlet_log()[0].atoms,
        [MockAtom::Float(1.), long(9)]
    );
    //a symbol where a number is wanted never reaches the method
    assert!(matches!(
        k.send(obj, "range", &["a".into(), 9.into()]),
        Err(SendError::BadArgs { .. })
    ));
    assert!(k.outlet_log().is_empty());
    k.free_object(obj);
}

#[test]
fn an_err_result_is_reported_to_the_console_for_the_object() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("e2e.typed", &[]);

    k.send_int(obj, 4).unwrap();
    assert!(k.console_log().is_empty());

    //the method ran and returned `Err`: the send itself worked, and the default `report_error`
    //posted the error for the object. Nothing was sent.
    k.take_outlet_log();
    k.send_int(obj, -4).unwrap();
    let c = k.take_console_log();
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].level, c[0].object), (Level::Error, obj as usize));
    assert_eq!(c[0].text, "negative: -4");
    assert!(k.outlet_log().is_empty());

    //a `&'static str` error, from a method with two arguments
    k.send(obj, "range", &[5.into(), 1.into()]).unwrap();
    let c = k.take_console_log();
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].level, c[0].object), (Level::Error, obj as usize));
    assert_eq!(c[0].text, "empty range");

    //the object works afterwards
    k.send_int(obj, 1).unwrap();
    assert_eq!(k.take_outlet_log()[0].atoms, [long(1)]);
    k.free_object(obj);
}

#[test]
fn a_panic_in_new_is_posted_and_max_gets_no_object() {
    let k = median_mock::lock();
    register();
    //`new` panics for -1: the object is not made, the panic is on the console
    let obj = k.new_object("e2e.typed", &[long(-1)]);
    assert!(obj.is_null());
    let errors = console(&k, Level::Error);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("no -1 for you"), "{errors:?}");
    //and nothing is left behind: the half made object was freed
    assert_eq!(k.live_object_count(), 0);
}

#[test]
fn attributes_get_and_set_through_the_kernel() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("e2e.thing", &[]);

    assert_eq!(k.attr_get(obj, "foo").unwrap(), [MockAtom::Float(0.)]);
    k.attr_set(obj, "foo", &[2.5.into()]).unwrap();
    assert_eq!(k.attr_get(obj, "foo").unwrap(), [MockAtom::Float(2.5)]);

    k.attr_set(obj, "blah", &[9.into()]).unwrap();
    assert_eq!(k.attr_get(obj, "blah").unwrap(), [long(9)]);
    //and the setter's value is what `int` sees too (one value behind both)
    k.send_int(obj, 10).unwrap();
    assert_eq!(k.attr_get(obj, "blah").unwrap(), [long(10)]);

    assert!(matches!(
        k.attr_get(obj, "nonesuch"),
        Err(AttrError::NoAttr(_))
    ));
    k.free_object(obj);
}

//median bug (found by this test): `ClockHandle::new_unarmed` in `src/clock.rs` passes a C string
//(`c"#P".as_ptr()`) where `object_obex_lookup` takes a `t_symbol *`, so any object that makes a
//clock reads a symbol from a `char *`. The kernel dereferences it and the process crashes. With
//`gensym(name.as_ptr())` as the key (as the `object_obex_storeflags` call next to it has) this
//test passes.
#[test]
fn a_clock_fires_when_the_kernel_advances() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("e2e.ticker", &[]);
    assert!(!obj.is_null());

    //bang starts the clock for 10 ms
    k.send_bang(obj).unwrap();
    assert!(k.outlet_log().is_empty());
    assert_eq!(k.advance(9.), 0);
    assert!(k.outlet_log().is_empty());
    assert_eq!(k.advance(1.), 1);
    let log = k.take_outlet_log();
    assert_eq!(log.len(), 2);
    //the bang outlet (left), then the list outlet, at the due time on the scheduler thread
    assert_eq!((log[0].selector.as_str(), log[0].index), ("bang", 0));
    assert_eq!((log[1].selector.as_str(), log[1].index), ("list", 1));
    assert_eq!(log[1].atoms, [long(1), MockAtom::Float(1.5)]);
    assert!(log.iter().all(|e| e.time == 10. && e.role == Role::Timer));

    //it fired once
    assert_eq!(k.advance(100.), 0);

    //freeing the object takes its clock with it
    k.send_bang(obj).unwrap();
    assert!(k.free_object(obj));
    assert_eq!(k.advance(100.), 0);
    assert!(k.outlet_log().is_empty());
}

#[test]
fn msp_object_runs_its_perform_routine() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("e2e.dsp~", &[]);
    assert!(!obj.is_null());
    assert_eq!(k.signal_inlets(obj), Some(2));
    assert_eq!(k.extra_inlet_count(obj), 1);
    assert_eq!(k.outlet_types(obj), [Some("signal".to_string())]);

    let adds = k.dsp64(obj, 48000., 64).unwrap();
    assert_eq!(adds.len(), 1);
    //`dsp_setup` ran with the sample rate
    let c = k.console_log();
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].level, c[0].object), (Level::Post, obj as usize));
    assert_eq!(c[0].text, "sample rate: 48000");

    let mut ins = vec![vec![1., 2., 3., 4.], vec![10., 20., 30., 40.]];
    let mut outs = vec![vec![-1.; 4]];
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(outs[0], [12., 24., 36., 48.]);

    //again, with different buffers: the perform routine keeps working
    let mut ins = vec![vec![0.5; 4], vec![0.; 4]];
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(outs[0], [1.; 4]);

    assert!(k.free_object(obj));
    //nothing was posted as an error
    assert!(console(&k, Level::Error).is_empty());
}

#[test]
fn msp_object_is_a_normal_object_for_the_kernel() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("e2e.dsp~", &[]);
    //it has no `bang`: refused with a message on the console
    assert!(matches!(k.send_bang(obj), Err(SendError::NoMethod(_))));
    assert_eq!(console(&k, Level::Error).len(), 1);
    assert!(k.free_object(obj));
    assert_eq!(k.live_object_count(), 0);
}
