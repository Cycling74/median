//! `#[gimmeback]` in `#[median::methods]`, called the way `js` and `object_method_typed` do
//! through the `median-mock` kernel (`tests/gimmeback.rs` registers by hand).

use median::dict::{Dict, Ownership};
use median::gimmeback::Reply;
use median::prelude::*;
use median_mock::{CallReply, Kernel, Level, SendError, containers};
use std::ffi::c_void;
use std::sync::Once;
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(MaxObject)]
#[max(name = "gimmeback.macro")]
pub struct Gm {
    calls: AtomicI64,
}

impl MaxObjWrapped for Gm {
    fn new(_b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            calls: AtomicI64::new(0),
        })
    }
}

#[median::methods]
impl Gm {
    /// the atoms only: the marker names the selector
    #[gimmeback("count")]
    pub fn count(&self, atoms: &[Atom]) -> t_atom_long {
        self.calls.fetch_add(1, Ordering::SeqCst);
        atoms.len() as t_atom_long
    }

    /// a `Reply` by hand
    #[gimmeback("one")]
    pub fn one(&self, atoms: &[Atom]) -> Reply {
        Reply::Int(atoms.len() as t_atom_long)
    }

    /// the selector too: the explicit API says which was sent, so one method can have several
    #[gimmeback("which")]
    pub fn which(&self, sel: &SymbolRef, _atoms: &[Atom]) -> Reply {
        Reply::Symbol(*sel)
    }

    #[gimmeback("getstate")]
    pub fn state(&self, _atoms: &[Atom]) -> Result<Dict, DictError> {
        let d = Dict::new()?;
        d.insert(sym!("count"), self.calls.load(Ordering::SeqCst))?;
        Ok(d)
    }

    #[gimmeback("fail")]
    pub fn fail(&self, _atoms: &[Atom]) -> Result<t_atom_long, String> {
        Err("it failed".into())
    }

    /// a gimme method on a patcher, for contrast
    #[gimme("plain")]
    pub fn plain(&self, _atoms: &[Atom]) {
        self.calls.fetch_add(100, Ordering::SeqCst);
    }
}

median::ext_main! { Gm }

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
    let o = k.new_object("gimmeback.macro", &[]);
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
fn a_method_with_the_atoms_returns_a_plain_value() {
    let k = setup();
    let o = obj(&k);
    assert_eq!(
        k.call(o, "count", &[1.into(), 2.into(), 3.into()]),
        Ok(CallReply::Int(3))
    );
    assert_eq!(k.call(o, "count", &[]), Ok(CallReply::Int(0)));
    assert!(errors(&k, o).is_empty());
    assert!(k.free_object(o));
}

#[test]
fn a_method_with_the_selector_is_told_which_one_arrived() {
    let k = setup();
    let o = obj(&k);
    assert_eq!(
        k.call(o, "which", &[]),
        Ok(CallReply::Symbol("which".into()))
    );
    assert_eq!(k.call(o, "one", &[5.into()]), Ok(CallReply::Int(1)));
    assert!(k.free_object(o));
}

#[test]
fn a_dictionary_result_is_the_callers_to_free() {
    let k = setup();
    let o = obj(&k);
    k.call(o, "count", &[]).unwrap();
    let Ok(CallReply::Object(p)) = k.call(o, "getstate", &[]) else {
        panic!("not an object reply");
    };
    assert_eq!(containers::live_count(), 1);
    {
        // SAFETY: the reply is a dictionary whose one reference is ours
        let d = unsafe { Dict::from_raw(p.cast(), Ownership::Retained) }.unwrap();
        assert!(matches!(
            d.get(sym!("count")).unwrap(),
            Some(median::dict::DictValue::Int(1))
        ));
    }
    assert_eq!(containers::live_count(), 0);
    assert!(k.free_object(o));
}

#[test]
fn an_error_is_posted_and_the_caller_gets_no_value() {
    let k = setup();
    let o = obj(&k);
    assert_eq!(
        k.call(o, "fail", &[]),
        Ok(CallReply::Error(
            max_sys::e_max_errorcodes::MAX_ERR_GENERIC as _
        ))
    );
    assert_eq!(errors(&k, o), ["it failed"]);
    assert!(k.free_object(o));
}

#[test]
fn a_patcher_message_does_not_reach_the_method() {
    let k = setup();
    let o = obj(&k);
    assert_eq!(
        k.send(o, "count", &[]),
        Err(SendError::TypedOnly("count".into()))
    );
    assert_eq!(
        k.send(o, "which", &[]),
        Err(SendError::TypedOnly("which".into()))
    );
    //neither ran (a run of `count` adds one)
    k.send(o, "plain", &[]).unwrap();
    let Ok(CallReply::Object(p)) = k.call(o, "getstate", &[]) else {
        panic!("not an object reply");
    };
    // SAFETY: a dictionary reply, the reference is ours
    let d = unsafe { Dict::from_raw(p.cast(), Ownership::Retained) }.unwrap();
    assert!(matches!(
        d.get(sym!("count")).unwrap(),
        Some(median::dict::DictValue::Int(100))
    ));
    drop(d);
    assert_eq!(errors(&k, o).len(), 2);
    assert!(k.free_object(o));
}
