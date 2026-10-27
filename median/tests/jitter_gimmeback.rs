//! `A_GIMMEBACK` methods on a Jitter type: registered on the Jitter class in `class_setup` (the
//! wrap flag, the reply out dumpout) or with `#[gimmeback]` (on the Max wrapper, typed callers
//! only). The mock models the SDK's gimmeback wrap as median assumes it behaves
//! (`doc/plan-gimmeback.md`, "Host checks"), so these tests check median's side of it: which
//! flags it passes and where it registers.

use median::gimmeback::Reply;
use median::jit::{
    JitBuilder, JitObjRef, JitResult, MatrixOp,
    matrix::{Count, IOCount, Matrix},
    wrapper::{Core, MatrixKind},
};
use median::prelude::*;
use median_mock::{Atom as MockAtom, CallReply, Kernel, SendError, containers};
use std::sync::Once;

/// Both: two gimmeback methods on the Jitter class and one on the Max wrapper.
#[derive(MaxObject)]
#[max(name = "jit.test.gb.core", kind = matrix_op)]
struct Both {
    _obj: JitObjRef,
}
impl Both {
    fn cell(&self, atoms: &[Atom]) -> Reply {
        Reply::Atoms(
            atoms
                .iter()
                .map(|a| Atom::from(a.get_int() * 2))
                .chain([Atom::from(0.5)])
                .collect(),
        )
    }
    fn count(&self, atoms: &[Atom]) -> t_atom_long {
        atoms.len() as t_atom_long
    }
}
impl MatrixOp for Both {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self { _obj: b.obj() })
    }
    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }
    fn class_setup(class: &mut Class<Core<Self, MatrixKind>>) -> median::error::MaxResult<()> {
        class.add_gimmeback(c"getcell", Self::cell)?;
        class.add_gimmeback(c"getcount", Self::count)
    }
    fn calc(&self, _: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        Ok(())
    }
}
#[median::methods]
impl Both {
    #[gimmeback("maxback")]
    fn maxback(&self, atoms: &[Atom]) -> t_atom_long {
        atoms.len() as t_atom_long + 100
    }
}

/// A gimmeback method only on the Max wrapper (`#[gimmeback]`): no wrap flag.
#[derive(MaxObject)]
#[max(name = "jit.test.gb.max", kind = matrix_op)]
struct MaxOnly {
    _obj: JitObjRef,
}
impl MatrixOp for MaxOnly {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self { _obj: b.obj() })
    }
    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }
    fn calc(&self, _: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        Ok(())
    }
}
#[median::methods]
impl MaxOnly {
    #[gimmeback("maxback")]
    fn maxback(&self, atoms: &[Atom]) -> Result<Reply, String> {
        Ok(Reply::Atoms(
            atoms.iter().map(|a| Atom::from(a.get_int() + 1)).collect(),
        ))
    }
}

/// No gimmeback method anywhere: the flags stay 0.
#[derive(MaxObject)]
#[max(name = "jit.test.gb.none", kind = matrix_op, no_methods)]
struct NoGimmeback {
    _obj: JitObjRef,
}
impl MatrixOp for NoGimmeback {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self { _obj: b.obj() })
    }
    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }
    fn calc(&self, _: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        Ok(())
    }
}

median::ext_main! { Both, MaxOnly, NoGimmeback }

static REGISTER: Once = Once::new();
/// Register the classes once (they survive kernel resets); call with the kernel held.
fn setup(k: &Kernel) {
    // SAFETY: called as Max does
    REGISTER.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
    assert!(k.class_exists("jit.test.gb.core"), "{:?}", k.console_log());
}

#[test]
fn the_wrap_flag_is_passed_only_for_a_class_with_a_core_gimmeback_method() {
    let k = median_mock::lock();
    setup(&k);
    assert_eq!(median_mock::jit::wrap_flags("jit.test.gb.core"), Some(1));
    // a `#[gimmeback]` method is on the Max wrapper, not the Jitter class
    assert_eq!(median_mock::jit::wrap_flags("jit.test.gb.max"), Some(0));
    assert_eq!(median_mock::jit::wrap_flags("jit.test.gb.none"), Some(0));
}

#[test]
fn a_patcher_message_gets_the_atomarray_reply_as_a_list_out_dumpout() {
    let k = median_mock::lock();
    setup(&k);
    let obj = k.new_object("jit.test.gb.core", &[] as &[MockAtom]);
    assert!(!obj.is_null(), "{:?}", k.console_log());
    let live = containers::live_count();
    k.take_outlet_log();
    k.send(obj, "getcell", &[MockAtom::Long(3), MockAtom::Long(4)])
        .unwrap();
    let out = k.take_outlet_log();
    assert_eq!(out.len(), 1, "{out:?}");
    // dumpout is the rightmost outlet
    assert_eq!(out[0].index, k.outlet_count(obj) - 1);
    assert_eq!(out[0].selector, "cell");
    assert_eq!(
        out[0].atoms,
        [MockAtom::Long(6), MockAtom::Long(8), MockAtom::Float(0.5)]
    );
    assert_eq!(containers::live_count(), live, "an atomarray is left over");
    assert!(k.take_console_log().is_empty());
    assert!(k.free_object(obj));
}

#[test]
fn a_plain_integer_reply_is_a_one_atom_list() {
    let k = median_mock::lock();
    setup(&k);
    let obj = k.new_object("jit.test.gb.core", &[] as &[MockAtom]);
    assert!(!obj.is_null(), "{:?}", k.console_log());
    k.take_outlet_log();
    k.send(obj, "getcount", &[MockAtom::Long(1), MockAtom::Long(1)])
        .unwrap();
    let out = k.take_outlet_log();
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].index, k.outlet_count(obj) - 1);
    assert_eq!(out[0].selector, "count");
    assert_eq!(out[0].atoms, [MockAtom::Long(2)]);
    assert!(k.free_object(obj));
}

#[test]
fn a_sel_gimmeback_method_is_on_the_max_wrapper_for_typed_callers_only() {
    let k = median_mock::lock();
    setup(&k);
    for (class, expected) in [
        ("jit.test.gb.core", CallReply::Int(102)),
        (
            "jit.test.gb.max",
            // `+ 1` on each argument
            CallReply::Atoms(vec![MockAtom::Long(8), MockAtom::Long(9)]),
        ),
    ] {
        let obj = k.new_object(class, &[] as &[MockAtom]);
        assert!(!obj.is_null(), "{:?}", k.console_log());
        let live = containers::live_count();
        k.take_outlet_log();
        let reply = k.call(obj, "maxback", &[MockAtom::Long(7), MockAtom::Long(8)]);
        assert_eq!(reply, Ok(expected), "{class}");
        assert_eq!(containers::live_count(), live);
        // a patcher message is refused, and nothing goes out dumpout
        assert_eq!(
            k.send(obj, "maxback", &[MockAtom::Long(7)]),
            Err(SendError::TypedOnly("maxback".into())),
            "{class}"
        );
        assert!(k.take_outlet_log().is_empty());
        k.take_console_log();
        assert!(k.free_object(obj));
    }
}

#[test]
fn without_a_core_gimmeback_method_nothing_is_wrapped() {
    let k = median_mock::lock();
    setup(&k);
    let obj = k.new_object("jit.test.gb.none", &[] as &[MockAtom]);
    assert!(!obj.is_null(), "{:?}", k.console_log());
    for sel in ["getcell", "getcount", "maxback"] {
        assert!(matches!(k.send(obj, sel, &[]), Err(SendError::NoMethod(_))));
    }
    assert!(k.free_object(obj));
}

#[test]
fn only_a_method_median_owns_is_marked_undocumented() {
    let k = median_mock::lock();
    setup(&k);
    let undocumented = Some(("long".to_string(), "1".to_string()));
    let obj = k.new_object("jit.test.gb.core", &[] as &[MockAtom]);
    assert!(!obj.is_null(), "{:?}", k.console_log());
    // the core-class methods reach the patcher through the Max wrapper's own methods, which
    // median does not own and does not mark; `add_gimmeback` on the Jitter class skips the mark
    // (the mock cannot read a Jitter class's methods, so this checks the patcher's side)
    assert!(k.method(obj, "getcell").is_some());
    assert_eq!(k.method_attr(obj, "getcell", "undocumented"), None);
    assert_eq!(k.method_attr(obj, "getcount", "undocumented"), None);
    // a `#[gimmeback]` method is on the Max wrapper, a median-owned method: marked
    assert_eq!(k.method_attr(obj, "maxback", "undocumented"), undocumented);
    assert!(k.take_console_log().is_empty());
    assert!(k.free_object(obj));
}
