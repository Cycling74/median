//! `#[defer]` and `#[defer_low]` (and the `Class::add_*_deferred` they generate), through the
//! `median-mock` kernel: thread roles with `Kernel::with_role`, the main queue with
//! `Kernel::run_main_queue`.
//!
//! Every test takes `median_mock::lock()` for its whole body.

use median::prelude::*;
use median_mock::{Atom as MockAtom, Kernel, Level, Role};
use std::sync::Once;

#[derive(MaxObject)]
#[max(name = "defer.thing")]
pub struct Thing {
    obj: ObjRef,
}

impl MaxObjWrapped for Thing {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self { obj: b.obj() })
    }
}

impl Thing {
    fn log(&self, what: String) {
        let main = median::thread::SysThread::is_main();
        self.obj.post(format!("{what}{}", if main { "" } else { " (not main)" }));
    }
}

#[median::methods]
impl Thing {
    #[bang]
    #[defer]
    pub fn bang(&self) {
        self.log("bang".into());
    }

    #[int]
    #[defer_low]
    pub fn int(&self, v: t_atom_long) {
        self.log(format!("int {v}"));
    }

    #[float]
    #[defer]
    pub fn float(&self, v: f64) {
        self.log(format!("float {v}"));
    }

    #[sym]
    #[defer_low]
    pub fn sym(&self, s: SymbolRef) {
        self.log(format!("symbol {s}"));
    }

    #[list]
    #[defer_low]
    pub fn list(&self, atoms: &[Atom]) {
        self.log(format!("list {}", describe(atoms)));
    }

    #[any]
    #[defer]
    pub fn anything(&self, sel: &SymbolRef, atoms: &[Atom]) {
        self.log(format!("anything {sel} {}", describe(atoms)));
    }

    #[sel("typed", defaults = 1)]
    #[defer_low]
    pub fn typed(&self, a: t_atom_long, b: SymbolRef) {
        self.log(format!("typed {a} {b}"));
    }

    #[sel("floats")]
    #[defer]
    pub fn floats(&self, a: f64, b: f64) {
        self.log(format!("floats {a} {b}"));
    }

    #[sel("open", gimme)]
    #[defer_low]
    pub fn open(&self, sel: &SymbolRef, atoms: &[Atom]) {
        self.log(format!("{sel} {}", describe(atoms)));
    }

    #[sel("fail")]
    #[defer_low]
    pub fn fail(&self) -> Result<(), String> {
        Err("deferred failure".into())
    }

    #[sel("boom")]
    #[defer_low]
    pub fn boom(&self) {
        panic!("deferred panic");
    }

    /// not deferred, for comparison
    #[sel("now")]
    pub fn now(&self) {
        self.log("now".into());
    }
}

fn describe(atoms: &[Atom]) -> String {
    atoms
        .iter()
        .map(|a| match a.get_value() {
            Some(median::atom::AtomValue::Int(i)) => format!("{i}"),
            Some(median::atom::AtomValue::Float(f)) => format!("{f:?}"),
            Some(median::atom::AtomValue::Symbol(s)) => format!("'{s}'"),
            _ => "?".into(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

median::ext_main! { Thing }

fn register() {
    static ONCE: Once = Once::new();
    // SAFETY: called once, as Max does
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
}

fn posts(k: &Kernel) -> Vec<String> {
    k.take_console_log().into_iter().map(|e| e.text).collect()
}

#[test]
fn defer_low_queues_even_on_the_main_thread() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("defer.thing", &[]);
    assert!(!obj.is_null());
    k.send_int(obj, 3).unwrap();
    k.send(obj, "now", &[]).unwrap();
    assert_eq!(posts(&k), ["now"]);
    assert_eq!(k.main_queue_len(), 1);
    assert_eq!(k.run_main_queue(), 1);
    assert_eq!(posts(&k), ["int 3"]);
    assert!(k.free_object(obj));
}

#[test]
fn defer_runs_at_once_on_main_and_queues_from_the_timer_thread() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("defer.thing", &[]);
    k.send_bang(obj).unwrap();
    assert_eq!(posts(&k), ["bang"]);
    assert_eq!(k.main_queue_len(), 0);

    k.with_role(Role::Timer, || {
        k.send_bang(obj).unwrap();
        k.send_float(obj, 1.5).unwrap();
        //not deferred: runs on the timer thread
        k.send(obj, "now", &[]).unwrap();
    });
    assert_eq!(posts(&k), ["now (not main)"]);
    //from another thread too
    k.with_role(Role::Other, || k.send_bang(obj).unwrap());
    assert!(posts(&k).is_empty());
    //one qelem for the object, however many calls
    assert_eq!(k.main_queue_len(), 1);
    assert_eq!(k.run_main_queue(), 1);
    //`Front` calls are queued at the front of the object's calls: latest first
    assert_eq!(posts(&k), ["bang", "float 1.5", "bang"]);
    assert!(k.free_object(obj));
}

#[test]
fn front_calls_run_before_low_calls_of_the_same_object() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("defer.thing", &[]);
    k.with_role(Role::Timer, || {
        k.send_int(obj, 1).unwrap();
        k.send_bang(obj).unwrap();
        k.send_int(obj, 2).unwrap();
    });
    k.run_main_queue();
    assert_eq!(posts(&k), ["bang", "int 1", "int 2"]);
    assert!(k.free_object(obj));
}

#[test]
fn deferred_typed_and_gimme_arguments_arrive_intact() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("defer.thing", &[]);
    k.with_role(Role::Timer, || {
        k.send(obj, "symbol", &[MockAtom::Sym("hello".into())]).unwrap();
        k.send_list(obj, &[MockAtom::Long(1), MockAtom::Float(2.5), MockAtom::Sym("x".into())])
            .unwrap();
        k.send(obj, "typed", &[MockAtom::Long(7), MockAtom::Sym("seven".into())]).unwrap();
        k.send(obj, "typed", &[MockAtom::Long(8)]).unwrap();
        k.send(obj, "open", &[MockAtom::Sym("a file".into()), MockAtom::Long(-1)]).unwrap();
        k.send(obj, "open", &[]).unwrap();
    });
    assert!(posts(&k).is_empty());
    k.run_main_queue();
    assert_eq!(
        posts(&k),
        [
            "symbol hello",
            "list 1,2.5,'x'",
            "typed 7 seven",
            "typed 8 ",
            "open 'a file',-1",
            "open ",
        ]
    );
    //`Front` ones, with the main thread running them at once
    k.send(obj, "floats", &[MockAtom::Float(0.25), MockAtom::Long(3)]).unwrap();
    k.send(obj, "whatever", &[MockAtom::Long(4), MockAtom::Sym("y".into())]).unwrap();
    assert_eq!(posts(&k), ["floats 0.25 3", "anything whatever 4,'y'"]);
    k.with_role(Role::Timer, || {
        k.send(obj, "whatever", &[MockAtom::Long(5)]).unwrap();
    });
    k.run_main_queue();
    assert_eq!(posts(&k), ["anything whatever 5"]);
    assert!(k.free_object(obj));
}

#[test]
fn a_deferred_call_pending_at_free_is_dropped_without_running() {
    let k = median_mock::lock();
    register();
    let qelems = k.live_qelem_count();
    let obj = k.new_object("defer.thing", &[]);
    assert_eq!(k.live_qelem_count(), qelems + 1);
    k.send_list(obj, &[MockAtom::Long(1), MockAtom::Sym("s".into())]).unwrap();
    k.send(obj, "open", &[MockAtom::Sym("f".into())]).unwrap();
    assert!(k.free_object(obj));
    assert_eq!(k.live_qelem_count(), qelems);
    assert_eq!(k.run_main_queue(), 0);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn errors_and_panics_of_deferred_methods_are_reported_when_they_run() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("defer.thing", &[]);
    k.send(obj, "fail", &[]).unwrap();
    k.send(obj, "boom", &[]).unwrap();
    k.send_int(obj, 9).unwrap();
    assert!(k.console_log().is_empty());
    k.run_main_queue();
    let log = k.take_console_log();
    let got: Vec<_> = log.iter().map(|e| (e.level, e.object, e.text.as_str())).collect();
    assert_eq!(
        got,
        [
            (Level::Error, obj as usize, "deferred failure"),
            (Level::Error, obj as usize, "median: panic in method: deferred panic"),
            //the panic didn't stop the next call
            (Level::Post, obj as usize, "int 9"),
        ]
    );
    assert!(k.free_object(obj));
}
