//! Registration refuses what would collide (an attribute name used twice, a selector the wrapper
//! registers itself), and attribute getters cope with the null pointers Max may pass. Run
//! through the `median-mock` kernel.

use median::class::{RESERVED_SELECTORS, is_reserved_selector};
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
        rec("sel in1", c.add_sel(c"in1", |_: &Self, _v: t_atom_long| {}, 0));
        rec("sel assist", c.add_sel(c"assist", |_: &Self| {}, 0));
        rec("gimme notify", c.add_sel_gimme(c"notify", |_: &Self, _: &SymbolRef, _: &[Atom]| {}));
        rec("gimme dsp64", c.add_sel_gimme(c"dsp64", |_: &Self, _: &SymbolRef, _: &[Atom]| {}));
        rec("sel ft10", c.add_sel(c"ft10", |_: &Self| {}, 0));
        rec(
            "attr value",
            c.add_attr(c"value", |s: &Self| s.value.get(), |s: &Self, v| s.value.set(v), AttrOptions::new()),
        );
        rec(
            "readonly value",
            c.add_attr_readonly(c"value", |s: &Self| s.value.get(), AttrOptions::new()),
        );
        rec("field value", c.add_field_attr(c"value", |s: &Self| &s.value, AttrOptions::new()));
        rec("attr other", c.add_attr_readonly(c"other", |s: &Self| s.value.get(), AttrOptions::new()));
        rec("attr other again", c.add_attr_readonly(c"other", |s: &Self| s.value.get(), AttrOptions::new()));
    }
}

#[median::methods]
impl Setup {
    #[bang]
    pub fn bang(&self) {}
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

median::ext_main! { DupAttr, Setup, Bare, BareDsp }

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
    let errors = EXT_MAIN_ERRORS.lock().unwrap().clone();
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
    assert!(is_reserved_selector(c"ft9") && !is_reserved_selector(c"ft10"));
}

#[test]
fn the_macro_mirrors_the_reserved_list() {
    let src = include_str!("../../median_macros/src/reserved.rs");
    let list = src
        .split_once("RESERVED_SELECTORS: &[&str] = &[")
        .and_then(|(_, rest)| rest.split_once("];"))
        .map(|(list, _)| list)
        .expect("the list in median_macros/src/reserved.rs");
    let names: Vec<&str> = list
        .split(',')
        .map(|s| s.trim().trim_matches('"'))
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(names, RESERVED_SELECTORS);
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
    let r = k.attr_get_with(obj, "value", GetterArgs::Buffer(1)).unwrap();
    assert_eq!((r.code, r.atoms.as_slice(), r.used_buffer), (0, &[MockAtom::Long(3)][..], true));
    //none: the getter allocates
    let r = k.attr_get_with(obj, "value", GetterArgs::Buffer(0)).unwrap();
    assert_eq!((r.atoms.as_slice(), r.used_buffer), (&[MockAtom::Long(3)][..], false));
    //a setter given a count but no atoms does nothing
    assert_eq!(k.attr_set_null_argv(obj, "value", 1), Ok(0));
    assert_eq!(k.attr_get(obj, "value").unwrap(), [MockAtom::Long(3)]);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
    assert!(k.free_object(obj));
}
