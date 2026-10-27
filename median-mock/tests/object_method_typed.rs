//! `object_method_typed`, `A_GIMMEBACK` methods, `atomisatomarray`, `Kernel::call`, and the refusal
//! of a patcher message to a gimmeback method, through the C symbols max-sys declares.
//!
//! Every test takes `median_mock::lock()`, which serialises them.

#![allow(clippy::missing_safety_doc)]
// `c_long` is `i64` here but 32 bit on Windows
#![allow(clippy::unnecessary_cast)]

use max_sys::{self as sys, e_max_atomtypes as ty, e_max_errorcodes as err};
use max_sys::{t_atom, t_atom_long, t_class, t_max_err, t_object, t_symbol};
use median_mock::{Atom, CallReply, SendError, containers};
use std::ffi::{c_long, c_void};
use std::sync::Once;
use std::sync::atomic::{AtomicI64, AtomicPtr, Ordering};

static CLASS: AtomicPtr<t_class> = AtomicPtr::new(std::ptr::null_mut());
static REGISTER: Once = Once::new();
/// calls of `back`, `gimme`, `typed`
static BACK: AtomicI64 = AtomicI64::new(0);
static GIMME: AtomicI64 = AtomicI64::new(0);
static TYPED: AtomicI64 = AtomicI64::new(0);
/// `argc` of the last `back` call
static BACK_ARGC: AtomicI64 = AtomicI64::new(-1);
/// the selector of the last `gimme` call
static GIMME_SEL: AtomicPtr<t_symbol> = AtomicPtr::new(std::ptr::null_mut());

fn sym(s: &std::ffi::CStr) -> *mut t_symbol {
    unsafe { sys::gensym(s.as_ptr()) }
}

/// `back`: answers `argc * 10` as an int.
extern "C" fn back(
    _x: *mut t_object,
    _s: *mut t_symbol,
    ac: c_long,
    _av: *mut t_atom,
    rv: *mut t_atom,
) -> t_max_err {
    BACK.fetch_add(1, Ordering::SeqCst);
    BACK_ARGC.store(ac as i64, Ordering::SeqCst);
    unsafe { sys::atom_setlong(rv, ac as t_atom_long * 10) };
    0
}

/// `arr`: answers a fresh atomarray of `1, 2.5, foo`, that the caller owns.
extern "C" fn arr(
    _x: *mut t_object,
    _s: *mut t_symbol,
    _ac: c_long,
    _av: *mut t_atom,
    rv: *mut t_atom,
) -> t_max_err {
    unsafe {
        let mut a: [t_atom; 3] = std::mem::zeroed();
        sys::atom_setlong(&mut a[0], 1);
        sys::atom_setfloat(&mut a[1], 2.5);
        sys::atom_setsym(&mut a[2], sym(c"foo"));
        sys::atom_setobj(rv, sys::atomarray_new(3, a.as_mut_ptr()).cast());
    }
    0
}

/// `fail`: leaves `rv` alone and says `MAX_ERR_GENERIC`.
extern "C" fn fail(
    _x: *mut t_object,
    _s: *mut t_symbol,
    _ac: c_long,
    _av: *mut t_atom,
    _rv: *mut t_atom,
) -> t_max_err {
    err::MAX_ERR_GENERIC as t_max_err
}

extern "C" fn gimme(_x: *mut t_object, s: *mut t_symbol, _ac: c_long, _av: *mut t_atom) {
    GIMME.fetch_add(1, Ordering::SeqCst);
    GIMME_SEL.store(s, Ordering::SeqCst);
}

extern "C" fn typed(_x: *mut t_object, n: t_atom_long) {
    TYPED.store(n as i64, Ordering::SeqCst);
}

fn class() -> *mut t_class {
    REGISTER.call_once(|| unsafe {
        let c = sys::class_new(
            c"typed.test".as_ptr(),
            None,
            None,
            std::mem::size_of::<t_object>() as c_long,
            None,
            0,
        );
        macro_rules! add {
            ($f:expr, $ty:ty, $name:expr, $($t:expr),*) => {
                sys::class_addmethod(
                    c,
                    Some(std::mem::transmute::<$ty, unsafe extern "C" fn(*mut c_void) -> *mut c_void>($f)),
                    $name.as_ptr(),
                    $($t,)* 0,
                )
            };
        }
        type Back = extern "C" fn(*mut t_object, *mut t_symbol, c_long, *mut t_atom, *mut t_atom) -> t_max_err;
        add!(back as Back, Back, c"back", ty::A_GIMMEBACK);
        add!(arr as Back, Back, c"arr", ty::A_GIMMEBACK);
        add!(fail as Back, Back, c"fail", ty::A_GIMMEBACK);
        type Gimme = extern "C" fn(*mut t_object, *mut t_symbol, c_long, *mut t_atom);
        add!(gimme as Gimme, Gimme, c"gimme", ty::A_GIMME);
        type Typed = extern "C" fn(*mut t_object, t_atom_long);
        add!(typed as Typed, Typed, c"typed", ty::A_LONG);
        CLASS.store(c, Ordering::SeqCst);
    });
    CLASS.load(Ordering::SeqCst)
}

fn nothing() -> t_atom {
    unsafe {
        let mut a: t_atom = std::mem::zeroed();
        sys::atom_setlong(&mut a, 0x5eed);
        a
    }
}

#[test]
fn a_gimmeback_method_gets_the_arguments_and_the_reply_atom() {
    let k = median_mock::lock();
    let x = unsafe { sys::object_alloc(class()) };
    BACK.store(0, Ordering::SeqCst);
    unsafe {
        let mut args: [t_atom; 2] = std::mem::zeroed();
        sys::atom_setlong(&mut args[0], 1);
        sys::atom_setsym(&mut args[1], sym(c"x"));
        let mut rv = nothing();
        let code = sys::object_method_typed(x, sym(c"back"), 2, args.as_mut_ptr(), &mut rv);
        assert_eq!(code, 0);
        assert_eq!(rv.a_type as u32, ty::A_LONG);
        assert_eq!(rv.a_w.w_long, 20);
        assert_eq!(BACK_ARGC.load(Ordering::SeqCst), 2);
        // a method that answers an error and no value leaves A_NOTHING
        let mut rv = nothing();
        let code = sys::object_method_typed(x, sym(c"fail"), 0, std::ptr::null_mut(), &mut rv);
        assert_eq!(code, err::MAX_ERR_GENERIC as t_max_err);
        assert_eq!(rv.a_type as u32, ty::A_NOTHING);
    }
    assert_eq!(BACK.load(Ordering::SeqCst), 1);
    assert!(k.free_object(x));
}

#[test]
fn a_gimmeback_method_needs_a_reply_atom() {
    let k = median_mock::lock();
    let x = unsafe { sys::object_alloc(class()) };
    BACK.store(0, Ordering::SeqCst);
    let code = unsafe {
        sys::object_method_typed(
            x,
            sym(c"back"),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(code, err::MAX_ERR_GENERIC as t_max_err);
    assert_eq!(BACK.load(Ordering::SeqCst), 0);
    assert!(k.free_object(x));
}

#[test]
fn gimme_and_typed_methods_are_called_and_leave_a_nothing_atom() {
    let k = median_mock::lock();
    let x = unsafe { sys::object_alloc(class()) };
    GIMME.store(0, Ordering::SeqCst);
    TYPED.store(0, Ordering::SeqCst);
    unsafe {
        let mut rv = nothing();
        let code = sys::object_method_typed(x, sym(c"gimme"), 0, std::ptr::null_mut(), &mut rv);
        assert_eq!(code, 0);
        assert_eq!(rv.a_type as u32, ty::A_NOTHING);
        assert_eq!(GIMME.load(Ordering::SeqCst), 1);
        assert_eq!(GIMME_SEL.load(Ordering::SeqCst), sym(c"gimme"));

        let mut arg: t_atom = std::mem::zeroed();
        sys::atom_setlong(&mut arg, 7);
        let mut rv = nothing();
        let code = sys::object_method_typed(x, sym(c"typed"), 1, &mut arg, &mut rv);
        assert_eq!(code, 0);
        assert_eq!(rv.a_type as u32, ty::A_NOTHING);
        assert_eq!(TYPED.load(Ordering::SeqCst), 7);

        // arguments that don't fit the typed method
        sys::atom_setsym(&mut arg, sym(c"seven"));
        let code = sys::object_method_typed(x, sym(c"typed"), 1, &mut arg, std::ptr::null_mut());
        assert_eq!(code, err::MAX_ERR_GENERIC as t_max_err);
        assert_eq!(TYPED.load(Ordering::SeqCst), 7);
    }
    assert!(k.free_object(x));
}

#[test]
fn an_unknown_selector_is_an_error_with_a_nothing_atom() {
    let k = median_mock::lock();
    let x = unsafe { sys::object_alloc(class()) };
    unsafe {
        let mut rv = nothing();
        let code = sys::object_method_typed(x, sym(c"nope"), 0, std::ptr::null_mut(), &mut rv);
        assert_eq!(code, err::MAX_ERR_GENERIC as t_max_err);
        assert_eq!(rv.a_type as u32, ty::A_NOTHING);
    }
    assert!(k.free_object(x));
    // a receiver that is not live
    unsafe {
        let mut rv = nothing();
        let code = sys::object_method_typed(x, sym(c"back"), 0, std::ptr::null_mut(), &mut rv);
        assert_eq!(code, err::MAX_ERR_INVALID_PTR as t_max_err);
        assert_eq!(rv.a_type as u32, ty::A_NOTHING);
    }
}

#[test]
fn object_method_calls_a_gimmeback_method_with_its_four_arguments() {
    let k = median_mock::lock();
    let x = unsafe { sys::object_alloc(class()) };
    BACK.store(0, Ordering::SeqCst);
    unsafe {
        let mut args: [t_atom; 3] = std::mem::zeroed();
        let mut rv = nothing();
        // the result is the method's `t_max_err`, as a pointer
        let r = sys::object_method(
            x,
            sym(c"back"),
            sym(c"back"),
            3 as c_long,
            args.as_mut_ptr(),
            &mut rv as *mut t_atom,
        );
        assert!(r.is_null());
        assert_eq!(rv.a_w.w_long, 30);
    }
    assert_eq!(BACK.load(Ordering::SeqCst), 1);
    assert!(k.free_object(x));
}

#[test]
fn atomisatomarray_says_whether_the_atom_holds_an_atomarray() {
    let k = median_mock::lock();
    unsafe {
        let mut a: t_atom = std::mem::zeroed();
        sys::atom_setlong(&mut a, 3);
        assert_eq!(sys::atomisatomarray(&mut a), 0);
        sys::atom_setsym(&mut a, sym(c"foo"));
        assert_eq!(sys::atomisatomarray(&mut a), 0);

        let array = sys::atomarray_new(0, std::ptr::null_mut());
        sys::atom_setobj(&mut a, array.cast());
        assert_eq!(sys::atomisatomarray(&mut a), 1);
        assert_eq!(sys::atomisdictionary(&mut a), 0);

        let dict = sys::dictionary_new();
        sys::atom_setobj(&mut a, dict.cast());
        assert_eq!(sys::atomisatomarray(&mut a), 0);
        assert_eq!(sys::atomisdictionary(&mut a), 1);

        // an object atom that is not a container
        let x = sys::object_alloc(class());
        sys::atom_setobj(&mut a, x);
        assert_eq!(sys::atomisatomarray(&mut a), 0);
        assert!(k.free_object(x));

        sys::object_free(array.cast());
        sys::object_free(dict.cast());
        // once freed it is not one any more
        sys::atom_setobj(&mut a, array.cast());
        assert_eq!(sys::atomisatomarray(&mut a), 0);
    }
    assert_eq!(containers::live_count(), 0);
}

#[test]
fn call_reads_the_reply() {
    let k = median_mock::lock();
    let x = unsafe { sys::object_alloc(class()) };
    assert_eq!(
        k.call(x, "back", &[Atom::Long(1), Atom::Float(2.)]),
        Ok(CallReply::Int(20))
    );
    // an atomarray reply is read and freed
    assert_eq!(
        k.call(x, "arr", &[]),
        Ok(CallReply::Atoms(vec![
            Atom::Long(1),
            Atom::Float(2.5),
            Atom::Sym("foo".into())
        ]))
    );
    assert_eq!(containers::live_count(), 0);
    assert_eq!(
        k.call(x, "fail", &[]),
        Ok(CallReply::Error(err::MAX_ERR_GENERIC as t_max_err))
    );
    assert_eq!(k.call(x, "gimme", &[]), Ok(CallReply::Nothing));
    assert_eq!(k.call(x, "typed", &[Atom::Long(1)]), Ok(CallReply::Nothing));
    assert_eq!(
        k.call(x, "nope", &[]),
        Err(SendError::NoMethod("nope".into()))
    );
    assert!(k.free_object(x));
    assert_eq!(k.call(x, "back", &[]), Err(SendError::NotLive));
}

#[test]
fn a_patcher_message_does_not_reach_a_gimmeback_method() {
    let k = median_mock::lock();
    let x = unsafe { sys::object_alloc(class()) };
    BACK.store(0, Ordering::SeqCst);
    k.take_console_log();
    assert_eq!(
        k.send(x, "back", &[Atom::Long(1)]),
        Err(SendError::TypedOnly("back".into()))
    );
    assert_eq!(BACK.load(Ordering::SeqCst), 0);
    let log = k.take_console_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].object, x as usize);
    assert_eq!(
        log[0].text,
        "typed.test: back is a typed-call (A_GIMMEBACK) method; a patcher message cannot reach it"
    );
    // the typed call still works
    assert_eq!(k.call(x, "back", &[]), Ok(CallReply::Int(0)));
    assert_eq!(BACK.load(Ordering::SeqCst), 1);
    assert!(k.free_object(x));
}
