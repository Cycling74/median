//! Registration refuses what would collide (an attribute name used twice, a selector the wrapper
//! registers itself), and attribute getters cope with the null pointers Max may pass. Run
//! through the `median-mock` kernel.

use median::class::{MSP_RESERVED_SELECTORS, RESERVED_SELECTORS, is_reserved_selector};
use median::error::MaxError;
use median::prelude::*;
use median_mock::{Atom as MockAtom, GetterArgs, Kernel, Level};
use std::sync::{Mutex, Once};

/// A field attribute and a getter attribute with the same name: registration fails.
#[derive(MaxObject)]
#[max(name = "reg.dup")]
pub struct DupAttr {
    #[attr]
    foo: Float64,
}

impl MaxObjWrapped for DupAttr {
    fn new(_b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            foo: Float64::new(0.),
        })
    }
}

#[median::methods]
impl DupAttr {
    #[attr(name = "foo")]
    pub fn other(&self) -> f64 {
        self.foo.get()
    }
}

/// What `class_setup` of `Setup` got from the registration calls, by name.
static SETUP: Mutex<Vec<(String, Result<(), String>)>> = Mutex::new(Vec::new());

/// Hand written `class_setup` that tries every collision; the results are recorded (and the
/// class still registers, since `class_setup` ignores them).
#[derive(MaxObject)]
#[max(name = "reg.setup")]
pub struct Setup {
    #[attr]
    value: Int64,
}

impl MaxObjWrapped for Setup {
    fn new(_b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            value: Int64::new(3),
        })
    }

    fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
        let mut r = SETUP.lock().unwrap();
        let mut rec = |what: &str, res: MaxResult<()>| {
            r.push((what.to_string(), res.map_err(|e| format!("{e:?}"))));
        };
        rec(
            "sel in1",
            c.add_sel(c"in1", |_: &Self, _v: t_atom_long| {}, 0),
        );
        rec("sel assist", c.add_sel(c"assist", |_: &Self| {}, 0));
        rec(
            "gimme notify",
            c.add_sel_gimme(c"notify", |_: &Self, _: &SymbolRef, _: &[Atom]| {}),
        );
        rec(
            "gimme dsp64",
            c.add_sel_gimme(c"dsp64", |_: &Self, _: &SymbolRef, _: &[Atom]| {}),
        );
        rec("sel ft10", c.add_sel(c"ft10", |_: &Self| {}, 0));
        rec("sel signal", c.add_sel(c"signal", |_: &Self| {}, 0));
        rec(
            "gimme enable",
            c.add_sel_gimme(c"enable", |_: &Self, _: &SymbolRef, _: &[Atom]| {}),
        );
        rec(
            "bang userconnect",
            c.add_sel(c"userconnect", |_: &Self| {}, 0),
        );
        rec("sel nsiginlets", c.add_sel(c"nsiginlets", |_: &Self| {}, 0));
        rec(
            "attr value",
            c.add_attr(
                c"value",
                |s: &Self| s.value.get(),
                |s: &Self, v| s.value.set(v),
                AttrOptions::new(),
            ),
        );
        rec(
            "readonly value",
            c.add_attr_readonly(c"value", |s: &Self| s.value.get(), AttrOptions::new()),
        );
        rec(
            "field value",
            c.add_field_attr(c"value", |s: &Self| &s.value, AttrOptions::new()),
        );
        rec(
            "attr other",
            c.add_attr_readonly(c"other", |s: &Self| s.value.get(), AttrOptions::new()),
        );
        rec(
            "attr other again",
            c.add_attr_readonly(c"other", |s: &Self| s.value.get(), AttrOptions::new()),
        );
    }
}

#[median::methods]
impl Setup {
    #[bang]
    pub fn bang(&self) {}
}

/// A Max class may have the methods `class_dspinit` adds to MSP classes.
#[derive(MaxObject)]
#[max(name = "reg.enable")]
pub struct Enable {
    obj: ObjRef,
}

impl MaxObjWrapped for Enable {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self { obj: b.obj() })
    }
}

#[median::methods]
impl Enable {
    #[sel("enable")]
    pub fn enable(&self, v: t_atom_long) {
        self.obj.post(format!("enable {v}"));
    }

    #[sel("signal")]
    pub fn signal(&self) {}
}

/// An MSP class may not: its registration fails.
#[derive(MaxObject)]
#[max(name = "reg.enable~", kind = msp)]
pub struct EnableDsp;

impl MSPObjWrapped for EnableDsp {
    fn new(_b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        Ok(EnableDsp)
    }

    fn perform(&self, _ins: &[&[f64]], _outs: &mut [&mut [f64]], _nframes: usize) {}
}

#[median::methods]
impl EnableDsp {
    #[sel("enable")]
    pub fn enable(&self, _v: t_atom_long) {}
}

/// What `class_setup` of `SetupDsp` got from the registration calls.
static SETUP_DSP: Mutex<Vec<(String, Result<(), String>)>> = Mutex::new(Vec::new());

/// `class_setup` of an MSP class trying `class_dspinit`'s selectors.
#[derive(MaxObject)]
#[max(name = "reg.setup~", kind = msp, no_methods)]
pub struct SetupDsp;

impl MSPObjWrapped for SetupDsp {
    fn new(_b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        Ok(SetupDsp)
    }

    fn perform(&self, _ins: &[&[f64]], _outs: &mut [&mut [f64]], _nframes: usize) {}

    fn class_setup(c: &mut Class<MSPObjWrapper<Self>>) {
        let mut r = SETUP_DSP.lock().unwrap();
        let mut rec = |what: &str, res: MaxResult<()>| {
            r.push((what.to_string(), res.map_err(|e| format!("{e:?}"))));
        };
        rec("sel signal", c.add_sel(c"signal", |_: &Self| {}, 0));
        rec(
            "gimme enable",
            c.add_sel_gimme(c"enable", |_: &Self, _: &SymbolRef, _: &[Atom]| {}),
        );
        rec(
            "bang userconnect",
            c.add_sel(c"userconnect", |_: &Self| {}, 0),
        );
        rec("sel nsiginlets", c.add_sel(c"nsiginlets", |_: &Self| {}, 0));
        rec(
            "sel in1",
            c.add_sel(c"in1", |_: &Self, _v: t_atom_long| {}, 0),
        );
        rec("sel enabled", c.add_sel(c"enabled", |_: &Self| {}, 0));
    }
}

/// No methods of its own: what the class has is what the wrapper registers.
#[derive(MaxObject)]
#[max(name = "reg.bare", no_methods)]
pub struct Bare;

impl MaxObjWrapped for Bare {
    fn new(_b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Bare)
    }
}

#[derive(MaxObject)]
#[max(name = "reg.bare~", kind = msp, no_methods)]
pub struct BareDsp;

impl MSPObjWrapped for BareDsp {
    fn new(_b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        Ok(BareDsp)
    }

    fn perform(&self, _ins: &[&[f64]], _outs: &mut [&mut [f64]], _nframes: usize) {}
}

median::ext_main! { DupAttr, Setup, Bare, BareDsp, Enable, EnableDsp, SetupDsp }

/// The console errors `ext_main` posted.
static EXT_MAIN_ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn setup() -> Kernel {
    static ONCE: Once = Once::new();
    let k = median_mock::lock();
    ONCE.call_once(|| {
        // SAFETY: that is how Max calls it, on its main thread
        unsafe { ext_main(std::ptr::null_mut()) };
        *EXT_MAIN_ERRORS.lock().unwrap() = k
            .take_console_log()
            .into_iter()
            .filter(|e| e.level == Level::Error)
            .map(|e| e.text)
            .collect();
    });
    k
}

#[test]
fn a_duplicate_attribute_name_fails_the_registration() {
    let k = setup();
    assert!(!k.class_exists("reg.dup"));
    let errors: Vec<String> = EXT_MAIN_ERRORS
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e.contains("reg.dup"))
        .cloned()
        .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].starts_with("median: failed to register reg.dup: ")
            && errors[0].contains("failed to register the methods: Duplicate"),
        "{errors:?}"
    );
}

#[test]
fn class_setup_is_refused_reserved_selectors_and_duplicate_attributes() {
    let k = setup();
    assert!(k.class_exists("reg.setup"));
    let dup = Err(format!("{:?}", MaxError::Duplicate));
    let expected: Vec<(String, Result<(), String>)> = [
        ("sel in1", dup.clone()),
        ("sel assist", dup.clone()),
        ("gimme notify", dup.clone()),
        ("gimme dsp64", dup.clone()),
        ("sel ft10", Ok(())),
        //`class_dspinit`'s selectors are fine on a Max class
        ("sel signal", Ok(())),
        ("gimme enable", Ok(())),
        ("bang userconnect", Ok(())),
        ("sel nsiginlets", Ok(())),
        ("attr value", dup.clone()),
        ("readonly value", dup.clone()),
        ("field value", dup.clone()),
        ("attr other", Ok(())),
        ("attr other again", dup.clone()),
    ]
    .into_iter()
    .map(|(w, r)| (w.to_string(), r))
    .collect();
    assert_eq!(*SETUP.lock().unwrap(), expected);
    //the wrapper's own methods are still its own, and the attributes are there once
    let obj = k.new_object("reg.setup", &[]);
    assert_eq!(k.attr_names(obj), ["value", "other"]);
    assert_eq!(k.method(obj, "in1").unwrap().1, [max_sys_long()]);
    assert!(k.free_object(obj));
}

fn max_sys_long() -> i32 {
    median::max_sys::e_max_atomtypes::A_LONG as i32
}

#[test]
fn the_reserved_list_is_what_the_wrapper_registers() {
    let k = setup();
    let mut reserved: Vec<String> = RESERVED_SELECTORS.iter().map(|s| s.to_string()).collect();
    reserved.sort();
    let obj = k.new_object("reg.bare~", &[]);
    assert_eq!(k.method_names(obj), reserved);
    assert!(k.free_object(obj));
    let obj = k.new_object("reg.bare", &[]);
    reserved.retain(|s| s != "dsp64");
    assert_eq!(k.method_names(obj), reserved);
    assert!(k.free_object(obj));
    assert!(is_reserved_selector(c"ft9", false) && !is_reserved_selector(c"ft10", true));
    assert!(MSP_RESERVED_SELECTORS.iter().all(|s| {
        let s = std::ffi::CString::new(*s).unwrap();
        is_reserved_selector(&s, true) && !is_reserved_selector(&s, false)
    }));
}

#[test]
fn msp_selectors_are_reserved_for_msp_classes_only() {
    let k = setup();
    //a Max class has them
    let obj = k.new_object("reg.enable", &[]);
    assert!(!obj.is_null());
    k.send(obj, "enable", &[MockAtom::Long(1)]).unwrap();
    assert_eq!(k.take_console_log()[0].text, "enable 1");
    assert!(k.method(obj, "signal").is_some());
    assert!(k.free_object(obj));
    //an MSP class's `#[median::methods]` registration fails
    assert!(!k.class_exists("reg.enable~"));
    let errors: Vec<String> = EXT_MAIN_ERRORS
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e.contains("reg.enable~"))
        .cloned()
        .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("failed to register the methods: Duplicate"),
        "{errors:?}"
    );
    //and `Class::add_*` refuses them in an MSP class's `class_setup`
    assert!(k.class_exists("reg.setup~"));
    let dup = Err(format!("{:?}", MaxError::Duplicate));
    let got = SETUP_DSP.lock().unwrap().clone();
    let expected: Vec<(String, Result<(), String>)> = [
        ("sel signal", dup.clone()),
        ("gimme enable", dup.clone()),
        ("bang userconnect", dup.clone()),
        ("sel nsiginlets", dup.clone()),
        ("sel in1", dup.clone()),
        ("sel enabled", Ok(())),
    ]
    .into_iter()
    .map(|(w, r)| (w.to_string(), r))
    .collect();
    assert_eq!(got, expected);
}

#[test]
fn the_macro_mirrors_the_reserved_lists() {
    let src = include_str!("../../median_macros/src/reserved.rs");
    let list = |name: &str| -> Vec<&'static str> {
        let start = format!("pub(crate) const {name}: &[&str] = &[");
        src.split_once(start.as_str())
            .and_then(|(_, rest)| rest.split_once("];"))
            .map(|(list, _)| list)
            .unwrap_or_else(|| panic!("{name} in median_macros/src/reserved.rs"))
            .split(',')
            .map(|s| s.trim().trim_matches('"'))
            .filter(|s| !s.is_empty())
            .collect()
    };
    assert_eq!(list("RESERVED_SELECTORS"), RESERVED_SELECTORS);
    //the MSP ones are not mirrored: the macro can't know the class's kind
    assert!(!src.contains("pub(crate) const MSP_RESERVED_SELECTORS"));
}

#[test]
fn attribute_getters_ignore_null_pointers_and_use_a_given_array() {
    let k = setup();
    let obj = k.new_object("reg.setup", &[]);
    for args in [GetterArgs::NullAc, GetterArgs::NullAv, GetterArgs::NullBoth] {
        let r = k.attr_get_with(obj, "value", args).unwrap();
        assert_eq!((r.code, r.atoms.len()), (0, 0), "{args:?}");
    }
    //an array Max supplies is filled in place
    let r = k
        .attr_get_with(obj, "value", GetterArgs::Buffer(1))
        .unwrap();
    assert_eq!(
        (r.code, r.atoms.as_slice(), r.used_buffer),
        (0, &[MockAtom::Long(3)][..], true)
    );
    //none: the getter allocates
    let r = k
        .attr_get_with(obj, "value", GetterArgs::Buffer(0))
        .unwrap();
    assert_eq!(
        (r.atoms.as_slice(), r.used_buffer),
        (&[MockAtom::Long(3)][..], false)
    );
    //a setter given a count but no atoms does nothing
    assert_eq!(k.attr_set_null_argv(obj, "value", 1), Ok(0));
    assert_eq!(k.attr_get(obj, "value").unwrap(), [MockAtom::Long(3)]);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
    assert!(k.free_object(obj));
}
