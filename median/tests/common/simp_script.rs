//! The same script run against `simp` (`tests/simp.rs`, written with the macros) and
//! `simp_explicit` (`tests/simp_explicit.rs`, written by hand): both must produce `EXPECTED`.

use median_mock::{Atom, Kernel, Level};
use std::ffi::c_void;

/// One line per console entry or outlet event, in order.
fn drain(k: &Kernel, obj: *mut c_void, out: &mut Vec<String>) {
    for e in k.take_console_log() {
        let who = if e.object == obj as usize {
            "obj"
        } else {
            "global"
        };
        let level = match e.level {
            Level::Post => "post",
            Level::Warn => "warn",
            Level::Error => "error",
        };
        out.push(format!("{level} {who}: {}", e.text));
    }
    for e in k.take_outlet_log() {
        out.push(format!("outlet {} {} {:?}", e.index, e.selector, e.atoms));
    }
}

/// Make an object of `class` (registered already) and put it through its paces.
pub fn run(k: &Kernel, class: &str) -> Vec<String> {
    let mut t = Vec::new();
    let obj = k.new_object(class, &[]);
    assert!(!obj.is_null());
    t.push(format!("outlets {}", k.outlet_count(obj)));
    t.push(format!("inlets {}", k.extra_inlet_count(obj)));
    t.push(format!("attributes {:?}", k.attr_names(obj)));
    for sel in [
        "bang", "int", "list", "anything", "set", "read", "symbol", "float",
    ] {
        t.push(format!("method {sel} {}", k.method(obj, sel).is_some()));
    }
    for name in ["label", "category", "save"] {
        t.push(format!(
            "value {name} {:?}",
            k.attr_attr(obj, "value", name)
        ));
    }

    k.send_bang(obj).unwrap();
    drain(k, obj, &mut t);
    t.push(format!("advance {}", k.advance(10.)));
    drain(k, obj, &mut t);

    k.send_int(obj, 5).unwrap();
    drain(k, obj, &mut t);
    t.push(format!("touches {:?}", k.attr_touches().len()));
    t.push(format!("value {:?}", k.attr_get(obj, "value").unwrap()));
    //set by the method, past the clip: the attribute getter clips
    k.send_int(obj, 500).unwrap();
    drain(k, obj, &mut t);
    t.push(format!("value {:?}", k.attr_get(obj, "value").unwrap()));
    k.attr_set(obj, "value", &[Atom::Long(-20)]).unwrap();
    t.push(format!("value {:?}", k.attr_get(obj, "value").unwrap()));

    t.push(format!("foo {:?}", k.attr_get(obj, "foo").unwrap()));
    k.attr_set(obj, "foo", &[Atom::Float(2.5)]).unwrap();
    t.push(format!("foo {:?}", k.attr_get(obj, "foo").unwrap()));

    k.send_list(obj, &[Atom::Long(1), Atom::Float(2.)]).unwrap();
    k.send(obj, "frob", &[Atom::Long(1)]).unwrap();
    k.send(obj, "set", &[Atom::Sym("abc".into())]).unwrap();
    k.send(obj, "set", &[]).unwrap();
    drain(k, obj, &mut t);

    //the inlets: float (`ft1`), int (`in2`), proxy
    k.send_inlet(obj, 1, "float", &[Atom::Float(1.5)]).unwrap();
    k.send_inlet(obj, 2, "int", &[Atom::Long(7)]).unwrap();
    k.send_inlet(obj, 3, "bang", &[]).unwrap();
    drain(k, obj, &mut t);
    t.push(format!("advance {}", k.advance(10.)));
    drain(k, obj, &mut t);

    //deferred: nothing until the main thread's queue runs, then the `changed` queue (set by
    //every `int` above, once) and the deferred `read` (one of the object's two deferred-call
    //qelems runs it, the other runs empty: 3 runs)
    k.send(obj, "read", &[Atom::Sym("file.txt".into())])
        .unwrap();
    drain(k, obj, &mut t);
    t.push(format!("main queue {}", k.run_main_queue()));
    drain(k, obj, &mut t);

    assert!(k.free_object(obj));
    t.push(format!("live {}", k.live_object_count()));
    t
}

pub const EXPECTED: &[&str] = &[
    "outlets 1",
    "inlets 3",
    "attributes [\"value\", \"foo\"]",
    "method bang true",
    "method int true",
    "method list true",
    "method anything true",
    "method set true",
    "method read true",
    "method symbol false",
    "method float false",
    "value label Some((\"symbol\", [Sym(\"Value\")]))",
    "value category Some((\"symbol\", [Sym(\"Simp\")]))",
    "value save Some((\"long\", [Long(1)]))",
    "post obj: from rust 0 inlet 0",
    "advance 1",
    "post global: clocked",
    "outlet 0 list [Long(1), Float(12.0), Sym(\"foo\")]",
    "post global: from rust 5 inlet 0",
    "touches 1",
    "value [Long(5)]",
    "post global: from rust 500 inlet 0",
    //the field holds 500, the attribute's clip applies when it is got
    "value [Long(100)]",
    "value [Long(0)]",
    "foo [Float(0.0)]",
    "foo [Float(2.5)]",
    "post global: got list with length 2",
    "post global: got any with sel frob and length 1",
    "post obj: set abc",
    //`set` without a name returns `Err`, posted by the default `report_error`
    "error obj: set needs a name",
    "post global: got float 1.5",
    "post global: from rust 7 inlet 0",
    "post obj: from rust 7 inlet 3",
    "advance 1",
    "post global: clocked",
    "outlet 0 list [Long(1), Float(12.0), Sym(\"foo\")]",
    "main queue 3",
    "post obj: value is now 7",
    "post obj: read 1 argument(s) on the main thread",
    "live 0",
];

/// The creation argument (the initial value): a number is taken, a symbol refuses the object.
pub fn run_args(k: &Kernel, class: &str) -> Vec<String> {
    let mut t = Vec::new();
    let obj = k.new_object(class, &[Atom::Long(42)]);
    t.push(format!("value {:?}", k.attr_get(obj, "value").unwrap()));
    assert!(k.free_object(obj));
    let obj = k.new_object(class, &[Atom::Float(7.9)]);
    t.push(format!("value {:?}", k.attr_get(obj, "value").unwrap()));
    assert!(k.free_object(obj));
    let obj = k.new_object(class, &[Atom::Sym("x".into())]);
    t.push(format!("null {}", obj.is_null()));
    for e in k.take_console_log() {
        t.push(format!(
            "{:?} for an object {}: {}",
            e.level,
            e.object != 0,
            e.text
        ));
    }
    t.push(format!("live {}", k.live_object_count()));
    t
}

pub const EXPECTED_ARGS: &[&str] = &[
    "value [Long(42)]",
    "value [Long(7)]",
    "null true",
    "Error for an object true: the initial value must be a number, not x",
    "live 0",
];

/// `doAbs` (`#[sel_gimmeback]` in `simp`, `add_sel_gimmeback` in `simp_explicit`) called the way
/// `js` does, and sent from a patcher, which is not expected to reach it.
pub fn run_do_abs(k: &Kernel, class: &str) -> Vec<String> {
    let mut t = Vec::new();
    let obj = k.new_object(class, &[]);
    assert!(!obj.is_null());
    k.take_console_log();
    for atoms in [
        vec![Atom::Long(-3)],
        vec![Atom::Float(-2.5)],
        vec![Atom::Long(1), Atom::Long(-2), Atom::Float(-0.5)],
        vec![Atom::Sym("x".into())],
        vec![],
    ] {
        t.push(format!("{atoms:?} -> {:?}", k.call(obj, "doAbs", &atoms)));
        drain(k, obj, &mut t);
    }
    t.push(format!(
        "send {:?}",
        k.send(obj, "doAbs", &[Atom::Long(-3)])
    ));
    drain(k, obj, &mut t);
    assert!(k.free_object(obj));
    t.push(format!("live {}", k.live_object_count()));
    //the mock's refusal names the class
    t.iter()
        .map(|l| l.replace(&format!("{class}: "), ""))
        .collect()
}

pub const EXPECTED_DO_ABS: &[&str] = &[
    "[Long(-3)] -> Ok(Int(3))",
    "[Float(-2.5)] -> Ok(Float(2.5))",
    "[Long(1), Long(-2), Float(-0.5)] -> Ok(Atoms([Long(1), Long(2), Float(0.5)]))",
    "[Sym(\"x\")] -> Ok(Nothing)",
    "[] -> Ok(Error(-1))",
    "error obj: missing argument for method doAbs()",
    "send Err(TypedOnly(\"doAbs\"))",
    "error obj: doAbs is a typed-call (A_GIMMEBACK) method; a patcher message cannot reach it",
    "live 0",
];
