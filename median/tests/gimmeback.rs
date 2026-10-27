//! `A_GIMMEBACK` methods registered by hand with `Class::add_sel_gimmeback` (`gimmeback_macro.rs`
//! uses the `#[gimmeback]` marker), called the way `js` and `object_method_typed` do, through
//! the `median-mock` kernel.

use median::atom::AtomValue;
use median::class::Class;
use median::dict::{AtomArray, Dict, Ownership};
use median::error::MaxError;
use median::gimmeback::Reply;
use median::prelude::*;
use median_mock::{CallReply, Kernel, Level, SendError, containers};
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, Once};

/// The absolute value of the number(s) it is given, like the SDK's `simplejs` `doAbs`.
#[derive(MaxObject)]
#[max(name = "gimmeback.test")]
pub struct Gb {
    /// calls of any method
    calls: AtomicI64,
}

impl MaxObjWrapped for Gb {
    fn new(_b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            calls: AtomicI64::new(0),
        })
    }

    fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
        Self::refusals(c);
    }
}

impl Gb {
    fn do_abs(&self, atoms: &[Atom]) -> Result<Reply, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match atoms {
            [] => Err("missing argument for method doAbs()".into()),
            [one] => Ok(match one.get_value() {
                Some(AtomValue::Int(i)) => Reply::Int(i.abs()),
                Some(AtomValue::Float(f)) => Reply::Float(f.abs()),
                _ => Reply::Nothing,
            }),
            many => Ok(Reply::Atoms(
                many.iter()
                    .map(|a| match a.get_value() {
                        Some(AtomValue::Int(i)) => Atom::from(i.abs()),
                        Some(AtomValue::Float(f)) => Atom::from(f.abs()),
                        _ => Atom::from(a.get_symbol()),
                    })
                    .collect(),
            )),
        }
    }

    fn count(&self, _atoms: &[Atom]) -> t_atom_long {
        self.calls.fetch_add(1, Ordering::SeqCst) as t_atom_long
    }

    fn half(&self, atoms: &[Atom]) -> f64 {
        atoms.first().map_or(0., |a| a.get_float() / 2.)
    }

    fn name(&self, _atoms: &[Atom]) -> SymbolRef {
        sym!("gimmeback")
    }

    fn unit(&self, _atoms: &[Atom]) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }

    /// A list as an `AtomArray`.
    fn array(&self, _atoms: &[Atom]) -> Result<AtomArray, DictError> {
        AtomArray::new(&[Atom::from(1 as t_atom_long), Atom::from(2.5)])
    }

    /// A list that the caller can't take: object atoms are refused.
    fn bad_list(&self, _atoms: &[Atom]) -> Vec<Atom> {
        vec![Atom::from(std::ptr::dangling_mut::<c_void>())]
    }

    fn state(&self, _atoms: &[Atom]) -> Result<Dict, DictError> {
        let d = Dict::new()?;
        d.insert(sym!("count"), self.calls.load(Ordering::SeqCst))?;
        Ok(d)
    }

    /// The same function under two selectors: it is told which one was sent.
    fn which(&self, sel: &SymbolRef, _atoms: &[Atom]) -> Reply {
        Reply::Symbol(*sel)
    }

    fn boom(&self, _atoms: &[Atom]) -> t_atom_long {
        panic!("boom in a gimmeback method")
    }
}

/// What `class_setup` got from the registrations that must be refused, by name.
static REFUSED: Mutex<Vec<(&'static str, Result<(), MaxError>)>> = Mutex::new(Vec::new());

impl Methods for Gb {
    fn register<W>(c: &mut Class<W>) -> MaxResult<()>
    where
        W: WrapperWrapped<Self> + MaxObj,
    {
        c.add_sel_gimmeback(c"doAbs", Self::do_abs)?;
        c.add_sel_gimmeback(c"getcount", Self::count)?;
        c.add_sel_gimmeback(c"half", Self::half)?;
        c.add_sel_gimmeback(c"name", Self::name)?;
        c.add_sel_gimmeback(c"unit", Self::unit)?;
        c.add_sel_gimmeback(c"array", Self::array)?;
        c.add_sel_gimmeback(c"badlist", Self::bad_list)?;
        c.add_sel_gimmeback(c"getstate", Self::state)?;
        //one function, two selectors; it is told which
        c.add_sel_gimmeback(c"one", Self::which)?;
        c.add_sel_gimmeback(c"two", Self::which)?;
        c.add_sel_gimmeback(c"boom", Self::boom)?;
        //a closure must annotate its parameters
        c.add_sel_gimmeback(c"closure", |_: &Self, atoms: &[Atom]| {
            atoms.len() as t_atom_long
        })?;
        //a message sent to a patcher
        c.add_sel_gimme(c"plain", |t: &Self, _: &[Atom]| {
            t.calls.fetch_add(100, Ordering::SeqCst);
        })?;
        Ok(())
    }
}

impl Gb {
    fn refusals(c: &mut Class<MaxObjWrapper<Self>>) {
        let mut r = REFUSED.lock().unwrap();
        //untyped (`A_CANT`) selectors: Max calls them with its own arguments
        r.push((
            "dblclick",
            c.add_sel_gimmeback(c"dblclick", |_: &Self, _: &[Atom]| {}),
        ));
        //reserved: the wrapper registers it
        r.push((
            "notify",
            c.add_sel_gimmeback(c"notify", |_: &Self, _: &[Atom]| {}),
        ));
        r.push((
            "assist",
            c.add_sel_gimmeback(c"assist", |_: &Self, _: &SymbolRef, _: &[Atom]| {
                0 as t_atom_long
            }),
        ));
    }
}

median::ext_main! { Gb }

fn setup() -> Kernel {
    static ONCE: Once = Once::new();
    let k = median_mock::lock();
    ONCE.call_once(|| {
        // SAFETY: that is how Max calls it, on its main thread
        unsafe { ext_main(std::ptr::null_mut()) };
    });
    k
}

fn obj(k: &Kernel) -> *mut c_void {
    let o = k.new_object("gimmeback.test", &[]);
    assert!(!o.is_null());
    o
}

fn errors(k: &Kernel, o: *mut c_void) -> Vec<String> {
    k.take_console_log()
        .into_iter()
        .filter(|e| e.level == Level::Error && e.object == o as usize)
        .map(|e| e.text)
        .collect()
}

#[test]
fn do_abs_answers_a_number_a_list_or_an_error() {
    let k = setup();
    let o = obj(&k);
    assert_eq!(k.call(o, "doAbs", &[(-3).into()]), Ok(CallReply::Int(3)));
    assert_eq!(
        k.call(o, "doAbs", &[(-2.5).into()]),
        Ok(CallReply::Float(2.5))
    );
    assert_eq!(
        k.call(o, "doAbs", &[1.into(), (-2).into(), (-0.5).into()]),
        Ok(CallReply::Atoms(vec![1.into(), 2.into(), 0.5.into()]))
    );
    //a symbol is no number: no value, and no error
    assert_eq!(k.call(o, "doAbs", &["x".into()]), Ok(CallReply::Nothing));
    assert!(errors(&k, o).is_empty());
    //no argument: an error on the console, MAX_ERR_GENERIC, no value
    assert_eq!(
        k.call(o, "doAbs", &[]),
        Ok(CallReply::Error(
            max_sys::e_max_errorcodes::MAX_ERR_GENERIC as _
        ))
    );
    assert_eq!(errors(&k, o), ["missing argument for method doAbs()"]);
    assert!(k.free_object(o));
}

#[test]
fn a_list_reply_leaves_no_atomarray_behind() {
    let k = setup();
    let o = obj(&k);
    assert_eq!(
        k.call(o, "doAbs", &[1.into(), 2.into()]),
        Ok(CallReply::Atoms(vec![1.into(), 2.into()]))
    );
    assert_eq!(containers::live_count(), 0);
    //an AtomArray as the return value goes over as it is
    assert_eq!(
        k.call(o, "array", &[]),
        Ok(CallReply::Atoms(vec![1.into(), 2.5.into()]))
    );
    assert_eq!(containers::live_count(), 0);
    assert!(k.free_object(o));
}

#[test]
fn a_dictionary_reply_is_the_callers_to_free() {
    let k = setup();
    let o = obj(&k);
    k.call(o, "unit", &[]).unwrap();
    k.call(o, "unit", &[]).unwrap();
    let Ok(CallReply::Object(p)) = k.call(o, "getstate", &[]) else {
        panic!("not an object reply");
    };
    assert_eq!(containers::live_count(), 1);
    {
        // SAFETY: the reply is a dictionary whose one reference is ours
        let d = unsafe { Dict::from_raw(p.cast(), Ownership::Retained) }.unwrap();
        assert!(d.contains_key(sym!("count")));
        assert!(matches!(
            d.get(sym!("count")).unwrap(),
            Some(median::dict::DictValue::Int(2))
        ));
    }
    assert_eq!(containers::live_count(), 0);
    assert!(k.free_object(o));
}

/// The method as Max calls it, with the reply atom a caller of its own choosing passes.
fn call_direct(
    k: &Kernel,
    o: *mut c_void,
    sel: &std::ffi::CStr,
    rv: *mut max_sys::t_atom,
) -> max_sys::t_max_err {
    type Back = unsafe extern "C" fn(
        *mut c_void,
        *mut max_sys::t_symbol,
        std::ffi::c_long,
        *const max_sys::t_atom,
        *mut max_sys::t_atom,
    ) -> max_sys::t_max_err;
    let (fun, types) = k.method(o, sel.to_str().unwrap()).unwrap();
    assert_eq!(types, [max_sys::e_max_atomtypes::A_GIMMEBACK as i32]);
    // SAFETY: registered `A_GIMMEBACK`, which is this signature
    let f = unsafe { std::mem::transmute::<usize, Back>(fun) };
    // SAFETY: a live object, the symbol is interned, no atoms
    unsafe { f(o, max_sys::gensym(sel.as_ptr()), 0, std::ptr::null(), rv) }
}

#[test]
fn a_null_reply_atom_runs_the_method_and_drops_the_reply() {
    let k = setup();
    let o = obj(&k);
    let none = std::ptr::null_mut();
    assert_eq!(call_direct(&k, o, c"getstate", none), 0);
    assert_eq!(call_direct(&k, o, c"array", none), 0);
    assert_eq!(call_direct(&k, o, c"getcount", none), 0);
    //a dictionary and an atomarray made and freed, nothing left
    assert_eq!(containers::live_count(), 0);
    assert_eq!(k.call(o, "getcount", &[]), Ok(CallReply::Int(1)));
    //an error and a panic are as they are with a reply atom
    assert_eq!(
        call_direct(&k, o, c"boom", none),
        max_sys::e_max_errorcodes::MAX_ERR_GENERIC as max_sys::t_max_err
    );
    assert!(k.free_object(o));
}

#[test]
fn the_reply_atom_is_nothing_when_the_method_has_no_value() {
    let k = setup();
    let o = obj(&k);
    let mut rv = median_mock::Atom::Long(7).to_raw();
    assert_eq!(call_direct(&k, o, c"unit", &mut rv), 0);
    assert_eq!(rv.a_type as u32, max_sys::e_max_atomtypes::A_NOTHING);
    assert!(k.free_object(o));
}

#[test]
fn plain_values_are_replies_too() {
    let k = setup();
    let o = obj(&k);
    assert_eq!(k.call(o, "getcount", &[]), Ok(CallReply::Int(0)));
    assert_eq!(k.call(o, "getcount", &[]), Ok(CallReply::Int(1)));
    assert_eq!(k.call(o, "half", &[5.into()]), Ok(CallReply::Float(2.5)));
    assert_eq!(
        k.call(o, "name", &[]),
        Ok(CallReply::Symbol("gimmeback".into()))
    );
    assert_eq!(k.call(o, "unit", &[]), Ok(CallReply::Nothing));
    assert_eq!(
        k.call(o, "closure", &[1.into(), 2.into(), 3.into()]),
        Ok(CallReply::Int(3))
    );
    assert!(errors(&k, o).is_empty());
    assert!(k.free_object(o));
}

#[test]
fn one_function_serves_two_selectors() {
    let k = setup();
    let o = obj(&k);
    assert_eq!(k.call(o, "one", &[]), Ok(CallReply::Symbol("one".into())));
    assert_eq!(k.call(o, "two", &[]), Ok(CallReply::Symbol("two".into())));
    assert!(k.free_object(o));
}

#[test]
fn a_panic_is_posted_and_the_caller_gets_an_error_and_no_value() {
    let k = setup();
    let o = obj(&k);
    let mut rv = median_mock::Atom::Long(7).to_raw();
    assert_eq!(
        call_direct(&k, o, c"boom", &mut rv),
        max_sys::e_max_errorcodes::MAX_ERR_GENERIC as max_sys::t_max_err
    );
    assert_eq!(rv.a_type as u32, max_sys::e_max_atomtypes::A_NOTHING);
    assert_eq!(
        errors(&k, o),
        ["median: panic in method: boom in a gimmeback method"]
    );
    assert_eq!(
        k.call(o, "boom", &[]),
        Ok(CallReply::Error(
            max_sys::e_max_errorcodes::MAX_ERR_GENERIC as _
        ))
    );
    assert!(k.free_object(o));
}

#[test]
fn a_list_that_cannot_be_made_is_an_error_and_no_value() {
    let k = setup();
    let o = obj(&k);
    //object atoms are refused: reported, MAX_ERR_GENERIC
    assert_eq!(
        k.call(o, "badlist", &[]),
        Ok(CallReply::Error(
            max_sys::e_max_errorcodes::MAX_ERR_GENERIC as _
        ))
    );
    assert_eq!(errors(&k, o).len(), 1);
    //allocation failure: MAX_ERR_OUT_OF_MEM
    containers::fail_next(containers::Failure::AtomArrayNew);
    assert_eq!(
        k.call(o, "doAbs", &[1.into(), 2.into()]),
        Ok(CallReply::Error(
            max_sys::e_max_errorcodes::MAX_ERR_OUT_OF_MEM as _
        ))
    );
    assert_eq!(containers::live_count(), 0);
    assert!(k.free_object(o));
}

#[test]
fn a_patcher_message_does_not_reach_a_gimmeback_method() {
    let k = setup();
    let o = obj(&k);
    assert_eq!(
        k.send(o, "getcount", &[]),
        Err(SendError::TypedOnly("getcount".into()))
    );
    assert_eq!(
        k.send(o, "doAbs", &[1.into()]),
        Err(SendError::TypedOnly("doAbs".into()))
    );
    //neither ran
    assert_eq!(k.call(o, "getcount", &[]), Ok(CallReply::Int(0)));
    let log = errors(&k, o);
    assert_eq!(log.len(), 2);
    assert!(log[0].contains("getcount is a typed-call (A_GIMMEBACK) method"));
    //a plain gimme method is reached
    k.send(o, "plain", &[]).unwrap();
    assert_eq!(k.call(o, "getcount", &[]), Ok(CallReply::Int(101)));
    assert!(k.free_object(o));
}

#[test]
fn untyped_and_reserved_selectors_are_refused() {
    let _k = setup();
    let r = REFUSED.lock().unwrap();
    assert_eq!(r.len(), 3);
    for (what, res) in r.iter() {
        assert!(matches!(res, Err(MaxError::Duplicate)), "{what}: {res:?}");
    }
}
