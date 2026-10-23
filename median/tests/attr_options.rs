//! Attribute options (`AttrOptions`, `AttrBuilder`, `#[attr(..)]` on fields and getters): each
//! one adds the attribute of the attribute the SDK's `CLASS_ATTR_*` macro adds, which
//! `median-mock` records per attribute (`Kernel::attr_attr`).

use median::attr::{AttrBuilder, AttrOptions, AttrType};
use median::prelude::*;
use median_mock::{Atom as MockAtom, Kernel};
use std::sync::Once;

#[test]
fn unattached_attributes_release_their_allocation() {
    let k = median_mock::lock();
    let before = k.live_attribute_count();
    let attr = AttrBuilder::<MaxObjWrapper<Thing>>::new_get(
        "unused",
        AttrType::Float64,
        median::attr::get_nop,
    )
    .build()
    .unwrap();
    assert_eq!(k.live_attribute_count(), before + 1);
    drop(attr);
    assert_eq!(k.live_attribute_count(), before);
}

#[derive(MaxObject)]
#[max(name = "attropts.thing")]
pub struct Thing {
    /// every option
    #[attr(
        label = "Gain Level",
        style = "text",
        category = "Levels",
        order = 2,
        default = 1.5,
        save,
        range(0, 10),
        clip(0, 10)
    )]
    gain: Float64,
    /// an enumerated long (enumindex), hidden
    #[attr(
        enum = "sine saw \"square wave\"",
        default = 1,
        range(0.5, 2.9),
        hidden
    )]
    shape: Int64,
    /// none
    #[attr]
    plain: Int64,
    mode: Int64,
    built: Float64,
}

impl MaxObjWrapped for Thing {
    fn new(_b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            gain: Float64::new(1.5),
            shape: Int64::new(1),
            plain: Int64::new(0),
            mode: Int64::new(0),
            built: Float64::new(0.),
        })
    }

    fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
        //by hand, with the builder
        extern "C" fn get(
            s: &MaxObjWrapper<Thing>,
            _attr: *mut std::ffi::c_void,
            ac: *mut std::ffi::c_long,
            av: *mut *mut median::max_sys::t_atom,
        ) -> median::max_sys::t_max_err {
            median::ffi::guard_or(s, "built", median::ffi::ERR_GENERIC, || {
                // SAFETY: Max passes `ac` and `av` as for a getter
                unsafe { median::attr::get(ac, av, || s.wrapped().built.get()) }
            })
        }
        let mut b = AttrBuilder::new_get("built", AttrType::Float64, get);
        b.label(c"Built").save().order(7);
        c.add_attribute(b.build().unwrap()).unwrap();
        //an atom attribute with an integer default
        extern "C" fn get_atom(
            s: &MaxObjWrapper<Thing>,
            _attr: *mut std::ffi::c_void,
            ac: *mut std::ffi::c_long,
            av: *mut *mut median::max_sys::t_atom,
        ) -> median::max_sys::t_max_err {
            median::ffi::guard_or(s, "any", median::ffi::ERR_GENERIC, || {
                // SAFETY: Max passes `ac` and `av` as for a getter
                unsafe { median::attr::get(ac, av, || Atom::from(s.wrapped().plain.get())) }
            })
        }
        let mut b = AttrBuilder::new_get("any", AttrType::AtomPtr, get_atom);
        b.default(5);
        c.add_attribute(b.build().unwrap()).unwrap();
        //by hand, with `add_attr` and options, a symbol default
        c.add_attr(
            c"named",
            |_: &Self| SymbolRef::from(c"x"),
            |_: &Self, _v: SymbolRef| {},
            AttrOptions::new()
                .default(c"\"two words\"")
                .category(c"Names"),
        )
        .unwrap();
    }
}

#[median::methods]
impl Thing {
    /// a getter attribute: an enumerated symbol, with a label
    #[attr(enum = "fast slow", label = "Mode", save)]
    pub fn mode(&self) -> SymbolRef {
        if self.mode.get() == 0 {
            sym!("fast")
        } else {
            sym!("slow")
        }
    }

    pub fn set_mode(&self, v: SymbolRef) {
        self.mode.set((v == sym!("slow")) as t_atom_long);
    }
}

median::ext_main! { Thing }

fn register() {
    static ONCE: Once = Once::new();
    // SAFETY: called once, as Max does
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
}

fn s(v: &str) -> MockAtom {
    MockAtom::Sym(v.into())
}

fn aa(k: &Kernel, obj: *mut std::ffi::c_void, attr: &str, name: &str) -> (String, Vec<MockAtom>) {
    k.attr_attr(obj, attr, name)
        .unwrap_or_else(|| panic!("{attr} has no {name}: {:?}", k.attr_attr_names(obj, attr)))
}

#[test]
fn field_attribute_options_add_attribute_attributes() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("attropts.thing", &[]);
    assert!(!obj.is_null());
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());

    //CLASS_ATTR_LABEL: one symbol (format "s")
    assert_eq!(
        aa(&k, obj, "gain", "label"),
        ("symbol".into(), vec![s("Gain Level")])
    );
    //CLASS_ATTR_STYLE, _CATEGORY: parsed, symbol
    assert_eq!(
        aa(&k, obj, "gain", "style"),
        ("symbol".into(), vec![s("text")])
    );
    assert_eq!(
        aa(&k, obj, "gain", "category"),
        ("symbol".into(), vec![s("Levels")])
    );
    //CLASS_ATTR_ORDER, _SAVE: long
    assert_eq!(
        aa(&k, obj, "gain", "order"),
        ("long".into(), vec![MockAtom::Long(2)])
    );
    assert_eq!(
        aa(&k, obj, "gain", "save"),
        ("long".into(), vec![MockAtom::Long(1)])
    );
    //CLASS_ATTR_DEFAULT, _MIN, _MAX: typed as the attribute
    assert_eq!(
        aa(&k, obj, "gain", "default"),
        ("float64".into(), vec![MockAtom::Float(1.5)])
    );
    assert_eq!(
        aa(&k, obj, "gain", "min"),
        ("float64".into(), vec![MockAtom::Float(0.)])
    );
    assert_eq!(
        aa(&k, obj, "gain", "max"),
        ("float64".into(), vec![MockAtom::Float(10.)])
    );
    assert_eq!(
        k.attr_attr_names(obj, "gain"),
        [
            "category", "default", "label", "max", "min", "order", "save", "style"
        ]
    );
    //range is only a hint, clip limits
    k.attr_set(obj, "gain", &[MockAtom::Float(12.)]).unwrap();
    assert_eq!(k.attr_get(obj, "gain").unwrap(), [MockAtom::Float(10.)]);

    //CLASS_ATTR_ENUMINDEX for a long: style enumindex, enumvals atoms; numbers as longs
    assert_eq!(
        aa(&k, obj, "shape", "style"),
        ("symbol".into(), vec![s("enumindex")])
    );
    assert_eq!(
        aa(&k, obj, "shape", "enumvals"),
        ("atom".into(), vec![s("sine"), s("saw"), s("square wave")])
    );
    assert_eq!(
        aa(&k, obj, "shape", "default"),
        ("long".into(), vec![MockAtom::Long(1)])
    );
    assert_eq!(
        aa(&k, obj, "shape", "min"),
        ("long".into(), vec![MockAtom::Long(0)])
    );
    assert_eq!(
        aa(&k, obj, "shape", "max"),
        ("long".into(), vec![MockAtom::Long(2)])
    );
    //CLASS_ATTR_INVISIBLE
    assert_eq!(
        aa(&k, obj, "shape", "invisible"),
        ("long".into(), vec![MockAtom::Long(1)])
    );

    //no options, nothing added
    assert!(k.attr_attr_names(obj, "plain").is_empty());
    assert!(k.free_object(obj));
}

#[test]
fn getter_and_hand_written_attribute_options() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("attropts.thing", &[]);
    //CLASS_ATTR_ENUM for a symbol: style enum
    assert_eq!(
        aa(&k, obj, "mode", "style"),
        ("symbol".into(), vec![s("enum")])
    );
    assert_eq!(
        aa(&k, obj, "mode", "enumvals"),
        ("atom".into(), vec![s("fast"), s("slow")])
    );
    assert_eq!(
        aa(&k, obj, "mode", "label"),
        ("symbol".into(), vec![s("Mode")])
    );
    assert_eq!(
        aa(&k, obj, "mode", "save"),
        ("long".into(), vec![MockAtom::Long(1)])
    );
    k.attr_set(obj, "mode", &[s("slow")]).unwrap();
    assert_eq!(k.attr_get(obj, "mode").unwrap(), [s("slow")]);

    //`AttrBuilder` options
    assert_eq!(k.attr_attr_names(obj, "built"), ["label", "order", "save"]);
    assert_eq!(
        aa(&k, obj, "built", "order"),
        ("long".into(), vec![MockAtom::Long(7)])
    );
    //an integer default of an atom attribute is written as an integer ("5", not "5.0")
    assert_eq!(
        aa(&k, obj, "any", "default"),
        ("atom".into(), vec![MockAtom::Long(5)])
    );
    //`add_attr` with a parse-string default for a symbol attribute
    assert_eq!(
        aa(&k, obj, "named", "default"),
        ("symbol".into(), vec![s("two words")])
    );
    assert_eq!(
        aa(&k, obj, "named", "category"),
        ("symbol".into(), vec![s("Names")])
    );
    assert!(k.free_object(obj));
}
