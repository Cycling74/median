//! `#[derive(MaxObject)]`, `#[median::methods]` and `median::ext_main!`, end to end: the classes
//! are registered by the `ext_main` the macro defines, made and sent messages through the
//! `median-mock` kernel, and what they do is read from its console and outlet logs.
//!
//! Every test takes `median_mock::lock()` for its whole body. Classes stay registered between
//! tests, so `ext_main` runs once per process.

use median::prelude::*;
use median_mock::{Atom as MockAtom, AttrError, Level, SendError};
use std::fmt::Display;
use std::sync::OnceLock;

/// Every marker kind. Each method posts what it got, for the object.
#[derive(MaxObject)]
#[max(name = "derive.marks")]
pub struct Marks {
    obj: ObjRef,
    /// a field attribute, clipped
    #[attr(clip(-1, 10))]
    value: Int64,
    /// renamed and read-only
    #[attr(name = "level", readonly)]
    level: Float64,
    /// a `Float32` field
    #[attr]
    ratio: Float32,
    fvalue: Float64,
    out: Outlet<List>,
}

impl MaxObjWrapped for Marks {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            obj: builder.obj(),
            value: Int64::new(0),
            level: Float64::new(0.5),
            ratio: Float32::new(0.25),
            fvalue: Float64::new(0.),
            out: builder.outlet(Outlet::list()),
        })
    }
}

#[median::methods]
impl Marks {
    #[bang]
    pub fn bang(&self) {
        self.obj.post("bang");
    }

    #[int]
    pub fn int(&self, v: t_atom_long) -> Result<(), String> {
        if v < 0 {
            return Err(format!("negative int {v}"));
        }
        self.obj.post(format!("int {v}"));
        Ok(())
    }

    #[float]
    pub fn float(&self, v: f64) {
        self.obj.post(format!("float {v}"));
    }

    #[sym]
    pub fn sym(&self, s: SymbolRef) {
        self.obj.post(format!("symbol {s}"));
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        let _ = self.out.send(atoms);
    }

    #[any]
    pub fn anything(&self, sel: &SymbolRef, atoms: &[Atom]) {
        self.obj.post(format!("anything {sel} {}", atoms.len()));
    }

    /// typed arguments, the last two optional (no `f64` among them: Max can't pass a mix)
    #[sel("range", defaults = 2)]
    pub fn range(
        &self,
        lo: t_atom_long,
        hi: t_atom_long,
        label: SymbolRef,
    ) -> Result<(), &'static str> {
        if hi < lo {
            return Err("empty range");
        }
        self.obj.post(format!("range {lo} {hi} [{label}]"));
        Ok(())
    }

    /// all `f64`, the last optional
    #[sel("scale", defaults = 1)]
    pub fn scale(&self, a: f64, b: f64) {
        self.obj.post(format!("scale {a} {b}"));
    }

    /// no arguments
    #[sel("reset")]
    pub fn reset(&self) {
        self.value.set(0);
        self.obj.post("reset");
    }

    #[sel("gimme", gimme)]
    pub fn gimme(&self, sel: &SymbolRef, atoms: &[Atom]) {
        self.obj.post(format!("gimme {sel} {}", atoms.len()));
    }

    /// an attribute with a getter and setter (`set_foo`, found by name); the setter can fail
    #[attr]
    pub fn foo(&self) -> f64 {
        self.fvalue.get()
    }

    pub fn set_foo(&self, v: f64) -> Result<(), String> {
        if v.is_nan() || v > 1000. {
            return Err(format!("foo out of range: {v}"));
        }
        self.fvalue.set(v);
        Ok(())
    }

    /// a read-only attribute: no `set_twice`
    #[attr(name = "double")]
    pub fn twice(&self) -> t_atom_long {
        self.value.get() * 2
    }

    /// `readonly` although there is a setter: the setter is not registered
    #[attr(readonly, clip(0, 1))]
    pub fn locked(&self) -> f64 {
        self.fvalue.get()
    }

    pub fn set_locked(&self, v: f64) {
        self.fvalue.set(v);
    }

    /// not marked: not a method of the class
    pub fn helper(&self) -> t_atom_long {
        self.value.get()
    }
}

/// Overrides `report_error`: errors go to an outlet instead of the console.
#[derive(MaxObject)]
#[max(name = "derive.reporter")]
pub struct Reporter {
    errors: Outlet<Any>,
}

impl MaxObjWrapped for Reporter {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            errors: builder.outlet(Outlet::any()),
        })
    }

    fn report_error(&self, _obj: &ObjRef, err: &dyn Display) {
        let msg = SymbolRef::try_from(err.to_string().as_str()).unwrap();
        let _ = self.errors.send(&SymbolRef::from(c"error"), &[msg.into()]);
    }
}

#[median::methods]
impl Reporter {
    #[bang]
    pub fn bang(&self) -> Result<(), String> {
        Err("from bang".into())
    }
}

/// An MSP object: a gain set with `float`, applied in `perform`.
#[derive(MaxObject)]
#[max(name = "derive.gain~", kind = msp)]
pub struct Gain {
    #[attr(clip(0, 4))]
    gain: Float64,
}

impl MSPObjWrapped for Gain {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::signal());
        builder.outlet(Outlet::signal());
        Ok(Self {
            gain: Float64::new(1.),
        })
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let g = self.gain.get();
        for (o, i) in outs[0].iter_mut().zip(ins[0].iter()) {
            *o = i * g;
        }
    }
}

#[median::methods]
impl Gain {
    #[float]
    pub fn float(&self, v: f64) {
        self.gain.set(v);
    }
}

/// No methods, no attributes, the default name ("plain").
#[derive(MaxObject)]
#[max(no_methods)]
pub struct Plain;

impl MaxObjWrapped for Plain {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Plain)
    }
}

/// A `nobox` class.
#[derive(MaxObject)]
#[max(name = "derive.nobox", class_type = nobox, no_methods)]
pub struct NoBox;

impl MaxObjWrapped for NoBox {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(NoBox)
    }
}

/// Its registration fails (in `class_setup`), which must not stop the classes after it.
#[derive(MaxObject)]
#[max(name = "derive.broken", no_methods)]
pub struct Broken;

impl MaxObjWrapped for Broken {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Broken)
    }

    fn class_setup(_c: &mut Class<MaxObjWrapper<Self>>) {
        panic!("no class for you");
    }
}

median::ext_main! { Marks, Reporter, Broken, Gain, Plain, NoBox }

/// The console errors posted while `ext_main` ran.
static EXT_MAIN_ERRORS: OnceLock<Vec<String>> = OnceLock::new();

fn register(k: &median_mock::Kernel) {
    EXT_MAIN_ERRORS.get_or_init(|| {
        // SAFETY: that is how Max calls it, on its main thread
        unsafe { ext_main(std::ptr::null_mut()) };
        let errors = errors(k);
        k.take_console_log();
        errors
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

fn errors(k: &median_mock::Kernel) -> Vec<String> {
    console(k, Level::Error)
}

/// The posts for `obj`, taken from the log.
fn posts(k: &median_mock::Kernel, obj: *mut std::ffi::c_void) -> Vec<String> {
    k.take_console_log()
        .into_iter()
        .filter(|e| e.level == Level::Post && e.object == obj as usize)
        .map(|e| e.text)
        .collect()
}

fn long(v: i64) -> MockAtom {
    MockAtom::Long(v)
}

#[test]
fn ext_main_registers_every_class_and_survives_a_failing_one() {
    let k = median_mock::lock();
    register(&k);
    for name in ["derive.marks", "derive.reporter", "derive.gain~", "plain"] {
        assert!(k.class_exists(name), "{name}");
    }
    //`Broken` panicked in `class_setup`: posted, not registered, and the classes after it
    //(`Gain`, `Plain`) still were
    assert!(!k.class_exists("derive.broken"));
    //registered in the `nobox` namespace
    assert_eq!(
        <NoBox as ObjClass>::class_type() as u8,
        ClassType::NoBox as u8
    );
    assert!(Class::<MaxObjWrapper<NoBox>>::exists_in_max(
        c"derive.nobox",
        ClassType::NoBox
    ));
    assert!(!Class::<MaxObjWrapper<NoBox>>::exists_in_max(
        c"derive.nobox",
        ClassType::Box
    ));
    assert!(Class::<MaxObjWrapper<Plain>>::exists_in_max(
        c"plain",
        ClassType::Box
    ));
    let ext_errors = EXT_MAIN_ERRORS.get().unwrap();
    assert_eq!(ext_errors.len(), 1, "{ext_errors:?}");
    assert!(
        ext_errors[0].starts_with("median: failed to register derive.broken: ")
            && ext_errors[0].ends_with("no class for you"),
        "{ext_errors:?}"
    );
    let obj = k.new_object("plain", &[]);
    assert!(!obj.is_null());
    assert!(k.free_object(obj));
}

#[test]
fn every_marker_is_registered_under_its_selector() {
    let k = median_mock::lock();
    register(&k);
    let obj = k.new_object("derive.marks", &[]);
    for sel in [
        "bang", "int", "float", "symbol", "list", "anything", "range", "scale", "reset", "gimme",
    ] {
        assert!(k.method(obj, sel).is_some(), "{sel}");
    }
    //a method without a marker is not registered
    assert!(k.method(obj, "helper").is_none());
    //field attributes first, in field order, then the accessor attributes in method order
    assert_eq!(
        k.attr_names(obj),
        ["value", "level", "ratio", "foo", "double", "locked"]
    );
    assert!(k.free_object(obj));
}

#[test]
fn every_marker_dispatches_with_its_arguments() {
    let k = median_mock::lock();
    register(&k);
    let obj = k.new_object("derive.marks", &[]);

    k.send_bang(obj).unwrap();
    k.send_int(obj, 7).unwrap();
    k.send_float(obj, 0.5).unwrap();
    k.send(obj, "symbol", &["hey".into()]).unwrap();
    k.send(obj, "frob", &[1.into(), 2.into(), 3.into()])
        .unwrap();
    k.send(obj, "gimme", &[1.into(), "x".into()]).unwrap();
    k.send(obj, "reset", &[]).unwrap();
    assert_eq!(
        posts(&k, obj),
        [
            "bang",
            "int 7",
            "float 0.5",
            "symbol hey",
            "anything frob 3",
            "gimme gimme 2",
            "reset"
        ]
    );

    k.send_list(obj, &[1.into(), 2.5.into()]).unwrap();
    let log = k.take_outlet_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].atoms, [long(1), MockAtom::Float(2.5)]);
    assert!(errors(&k).is_empty());
    assert!(k.free_object(obj));
}

#[test]
fn sel_defaults_fill_in_left_out_arguments() {
    let k = median_mock::lock();
    register(&k);
    let obj = k.new_object("derive.marks", &[]);
    //the registered types: long, then the two defaulted ones
    use median::max_sys::e_max_atomtypes::{A_DEFFLOAT, A_DEFLONG, A_DEFSYM, A_FLOAT, A_LONG};
    assert_eq!(
        k.method(obj, "range").unwrap().1,
        [A_LONG as i32, A_DEFLONG as i32, A_DEFSYM as i32]
    );
    assert_eq!(
        k.method(obj, "scale").unwrap().1,
        [A_FLOAT as i32, A_DEFFLOAT as i32]
    );
    k.send(obj, "range", &[1.into(), 3.into(), "a".into()])
        .unwrap();
    k.send(obj, "range", &[(-2).into(), 3.into()]).unwrap();
    k.send(obj, "range", &[(-2).into()]).unwrap();
    k.send(obj, "scale", &[0.5.into(), 2.into()]).unwrap();
    k.send(obj, "scale", &[0.5.into()]).unwrap();
    assert_eq!(
        posts(&k, obj),
        [
            "range 1 3 [a]",
            "range -2 3 []",
            "range -2 0 []",
            "scale 0.5 2",
            "scale 0.5 0"
        ]
    );
    //the first argument is not optional
    assert!(matches!(
        k.send(obj, "range", &[]),
        Err(SendError::BadArgs { .. })
    ));
    assert!(k.free_object(obj));
}

#[test]
fn an_err_reaches_report_error() {
    let k = median_mock::lock();
    register(&k);

    //the default `report_error` posts it as an error for the object
    let obj = k.new_object("derive.marks", &[]);
    k.send_int(obj, -3).unwrap();
    k.send(obj, "range", &[5.into(), 1.into()]).unwrap();
    let c = k.take_console_log();
    let errs: Vec<_> = c
        .iter()
        .map(|e| (e.level, e.object, e.text.as_str()))
        .collect();
    assert_eq!(
        errs,
        [
            (Level::Error, obj as usize, "negative int -3"),
            (Level::Error, obj as usize, "empty range")
        ]
    );
    assert!(k.free_object(obj));

    //an overridden `report_error` gets it instead
    let obj = k.new_object("derive.reporter", &[]);
    k.send_bang(obj).unwrap();
    assert!(k.console_log().is_empty());
    let log = k.take_outlet_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].selector, "error");
    assert_eq!(log[0].atoms, [MockAtom::Sym("from bang".into())]);
    assert!(k.free_object(obj));
}

#[test]
fn field_attributes_get_set_and_clip() {
    let k = median_mock::lock();
    register(&k);
    let obj = k.new_object("derive.marks", &[]);

    //`Int64` field, clipped to -1..=10
    assert_eq!(k.attr_get(obj, "value").unwrap(), [long(0)]);
    k.attr_set(obj, "value", &[4.into()]).unwrap();
    assert_eq!(k.attr_get(obj, "value").unwrap(), [long(4)]);
    k.attr_set(obj, "value", &[99.into()]).unwrap();
    assert_eq!(k.attr_get(obj, "value").unwrap(), [long(10)]);
    k.attr_set(obj, "value", &[(-5).into()]).unwrap();
    assert_eq!(k.attr_get(obj, "value").unwrap(), [long(-1)]);
    //a float atom sets an int attribute
    k.attr_set(obj, "value", &[3.7.into()]).unwrap();
    assert_eq!(k.attr_get(obj, "value").unwrap(), [long(3)]);
    //the field is what the methods see: `double` reads it
    assert_eq!(k.attr_get(obj, "double").unwrap(), [long(6)]);

    //`Float32` field
    assert_eq!(k.attr_get(obj, "ratio").unwrap(), [MockAtom::Float(0.25)]);
    k.attr_set(obj, "ratio", &[0.75.into()]).unwrap();
    assert_eq!(k.attr_get(obj, "ratio").unwrap(), [MockAtom::Float(0.75)]);

    //read-only field under another name: users can't set it, a set does nothing
    let opaque_user = median::max_sys::e_max_attrflags::ATTR_SET_OPAQUE_USER as i64;
    assert_eq!(k.attr_get(obj, "level").unwrap(), [MockAtom::Float(0.5)]);
    assert_eq!(
        k.attr_flags(obj, "level").unwrap() & opaque_user,
        opaque_user
    );
    k.attr_set(obj, "level", &[2.into()]).unwrap();
    assert_eq!(k.attr_get(obj, "level").unwrap(), [MockAtom::Float(0.5)]);
    assert!(matches!(
        k.attr_get(obj, "nonesuch"),
        Err(AttrError::NoAttr(_))
    ));
    //the writable ones are not opaque
    assert_eq!(k.attr_flags(obj, "value").unwrap() & opaque_user, 0);
    assert!(errors(&k).is_empty());
    assert!(k.free_object(obj));
}

#[test]
fn getter_attributes_with_and_without_a_setter() {
    let k = median_mock::lock();
    register(&k);
    let obj = k.new_object("derive.marks", &[]);
    let opaque_user = median::max_sys::e_max_attrflags::ATTR_SET_OPAQUE_USER as i64;

    //`foo` / `set_foo`
    assert_eq!(k.attr_get(obj, "foo").unwrap(), [MockAtom::Float(0.)]);
    k.attr_set(obj, "foo", &[2.5.into()]).unwrap();
    assert_eq!(k.attr_get(obj, "foo").unwrap(), [MockAtom::Float(2.5)]);
    assert_eq!(k.attr_flags(obj, "foo").unwrap() & opaque_user, 0);
    //the setter's `Err` goes to `report_error` (posted) and Max is told the set failed
    assert_eq!(
        k.attr_set(obj, "foo", &[5000.into()]),
        Err(AttrError::Accessor(
            median::max_sys::e_max_errorcodes::MAX_ERR_GENERIC as i64
        ))
    );
    assert_eq!(errors(&k), ["foo out of range: 5000"]);
    k.take_console_log();
    assert_eq!(k.attr_get(obj, "foo").unwrap(), [MockAtom::Float(2.5)]);

    //`double`: no setter, read-only
    k.attr_set(obj, "value", &[2.into()]).unwrap();
    assert_eq!(k.attr_get(obj, "double").unwrap(), [long(4)]);
    assert_eq!(
        k.attr_flags(obj, "double").unwrap() & opaque_user,
        opaque_user
    );
    k.attr_set(obj, "double", &[100.into()]).unwrap();
    assert_eq!(k.attr_get(obj, "double").unwrap(), [long(4)]);

    //`locked`: `readonly` wins over `set_locked`, and the getter is clipped to 0..=1
    assert_eq!(
        k.attr_flags(obj, "locked").unwrap() & opaque_user,
        opaque_user
    );
    k.attr_set(obj, "locked", &[0.5.into()]).unwrap();
    assert_eq!(k.attr_get(obj, "foo").unwrap(), [MockAtom::Float(2.5)]);
    assert_eq!(k.attr_get(obj, "locked").unwrap(), [MockAtom::Float(1.)]);
    assert!(errors(&k).is_empty());
    assert!(k.free_object(obj));
}

#[test]
fn msp_derive_runs_perform_with_methods_and_attributes() {
    let k = median_mock::lock();
    register(&k);
    let obj = k.new_object("derive.gain~", &[]);
    assert!(!obj.is_null());
    assert_eq!(k.signal_inlets(obj), Some(1));
    assert_eq!(k.outlet_types(obj), [Some("signal".to_string())]);

    let adds = k.dsp64(obj, 44100., 4).unwrap();
    assert_eq!(adds.len(), 1);
    let mut ins = vec![vec![1., 2., 3., 4.]];
    let mut outs = vec![vec![0.; 4]];
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(outs[0], [1., 2., 3., 4.]);

    //the `#[float]` method sets the gain, perform uses it
    k.send_float(obj, 0.5).unwrap();
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(outs[0], [0.5, 1., 1.5, 2.]);

    //the field attribute (clipped to 0..=4) is the same value
    assert_eq!(k.attr_get(obj, "gain").unwrap(), [MockAtom::Float(0.5)]);
    k.attr_set(obj, "gain", &[10.into()]).unwrap();
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(outs[0], [4., 8., 12., 16.]);

    assert!(errors(&k).is_empty());
    assert!(k.free_object(obj));
    assert_eq!(k.live_object_count(), 0);
}
