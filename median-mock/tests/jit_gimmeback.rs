//! `max_jit_class_wrap_standard` with and without `MAX_JIT_CLASS_FLAGS_GIMMEBACK_WRAP`: with it,
//! the Jitter class's `A_GIMMEBACK` methods get a Max method that sends the reply out dumpout.
//! This is median's assumption about the SDK wrap, modelled by the mock.
//!
//! Every test takes `median_mock::lock()`, which serialises them.

#![allow(clippy::missing_safety_doc)]
// `c_long` is `i64` here but 32 bit on Windows
#![allow(clippy::unnecessary_cast)]

use max_sys::{self as sys, e_max_atomtypes as ty, e_max_errorcodes as err};
use max_sys::{t_atom, t_atom_long, t_max_err, t_object, t_symbol};
use median_mock::{Atom, SendError, containers};
use std::ffi::{c_long, c_void};
use std::sync::Once;
use std::sync::atomic::{AtomicPtr, Ordering};

static REGISTER: Once = Once::new();
static JIT_CLASS: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

fn sym(s: &std::ffi::CStr) -> *mut t_symbol {
    unsafe { sys::gensym(s.as_ptr()) }
}

type Back =
    extern "C" fn(*mut t_object, *mut t_symbol, c_long, *mut t_atom, *mut t_atom) -> t_max_err;

/// `cell`: a fresh atomarray of `1, 2.5, foo` that the caller owns.
extern "C" fn cell(
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

/// `count`: the number of arguments times ten, as an int.
extern "C" fn count(
    _x: *mut t_object,
    _s: *mut t_symbol,
    ac: c_long,
    _av: *mut t_atom,
    rv: *mut t_atom,
) -> t_max_err {
    unsafe { sys::atom_setlong(rv, ac as t_atom_long * 10) };
    0
}

/// `name`: a symbol.
extern "C" fn name(
    _x: *mut t_object,
    _s: *mut t_symbol,
    _ac: c_long,
    _av: *mut t_atom,
    rv: *mut t_atom,
) -> t_max_err {
    unsafe { sys::atom_setsym(rv, sym(c"hello")) };
    0
}

/// `nothing`: no value (`rv` stays `A_NOTHING`).
extern "C" fn nothing(
    _x: *mut t_object,
    _s: *mut t_symbol,
    _ac: c_long,
    _av: *mut t_atom,
    _rv: *mut t_atom,
) -> t_max_err {
    0
}

/// `fail`: an error code and no value.
extern "C" fn fail(
    _x: *mut t_object,
    _s: *mut t_symbol,
    _ac: c_long,
    _av: *mut t_atom,
    _rv: *mut t_atom,
) -> t_max_err {
    err::MAX_ERR_GENERIC as t_max_err
}

/// `dict`: a fresh dictionary (an object that is not an atomarray).
extern "C" fn dict(
    _x: *mut t_object,
    _s: *mut t_symbol,
    _ac: c_long,
    _av: *mut t_atom,
    rv: *mut t_atom,
) -> t_max_err {
    unsafe { sys::atom_setobj(rv, sys::dictionary_new().cast()) };
    0
}

/// `plain`: an `A_GIMME` method of the Jitter class, which the wrap leaves alone.
extern "C" fn plain(_x: *mut t_object, _s: *mut t_symbol, _ac: c_long, _av: *mut t_atom) {}

/// The Jitter class of both Max classes: five gimmeback methods and a gimme one.
unsafe fn jitter_class(name_: &std::ffi::CStr) -> *mut c_void {
    unsafe {
        let jc = sys::jit_class_new(
            name_.as_ptr(),
            None,
            None,
            std::mem::size_of::<t_object>() as c_long,
            0 as c_long,
        );
        macro_rules! add {
            ($f:expr, $ty:ty, $name:expr, $t:expr) => {
                sys::jit_class_addmethod(
                    jc,
                    Some(std::mem::transmute::<
                        $ty,
                        unsafe extern "C" fn(*mut c_void) -> *mut c_void,
                    >($f)),
                    $name.as_ptr(),
                    $t as c_long,
                    0 as c_long,
                )
            };
        }
        add!(cell as Back, Back, c"cell", ty::A_GIMMEBACK);
        add!(count as Back, Back, c"count", ty::A_GIMMEBACK);
        add!(name as Back, Back, c"name", ty::A_GIMMEBACK);
        add!(nothing as Back, Back, c"nothing", ty::A_GIMMEBACK);
        add!(fail as Back, Back, c"fail", ty::A_GIMMEBACK);
        add!(dict as Back, Back, c"dict", ty::A_GIMMEBACK);
        type Gimme = extern "C" fn(*mut t_object, *mut t_symbol, c_long, *mut t_atom);
        add!(plain as Gimme, Gimme, c"plain", ty::A_GIMME);
        sys::jit_class_register(jc.cast());
        jc
    }
}

/// A Max class that wraps `jc` with `flags`, the way median's glue does.
unsafe fn max_class(name_: &std::ffi::CStr, jc: *mut c_void, flags: c_long) {
    unsafe {
        let c = sys::class_new(
            name_.as_ptr(),
            None,
            None,
            std::mem::size_of::<t_object>() as c_long,
            None,
            0,
        );
        sys::max_jit_class_obex_setup(c, 40);
        sys::max_jit_class_wrap_standard(c, jc.cast(), flags);
        sys::class_register(sym(c"box"), c);
    }
}

fn classes() {
    REGISTER.call_once(|| unsafe {
        let jc = jitter_class(c"median_mock.gimmeback");
        max_class(c"mock.gimmeback.on", jc, 1);
        max_class(c"mock.gimmeback.off", jc, 0);
        JIT_CLASS.store(jc, Ordering::SeqCst);
    });
}

/// A Max object of `class` with its Jitter object attached and the dumpout outlet; the outlet.
fn object(class: &str) -> (*mut c_void, *mut c_void, *mut c_void) {
    let class = std::ffi::CString::new(class).unwrap();
    // SAFETY: the classes were registered by `classes`
    unsafe {
        let class_ptr = sys::class_findbyname(sym(c"box"), sys::gensym(class.as_ptr()));
        assert!(!class_ptr.is_null());
        let obj = sys::object_alloc(class_ptr);
        let core = sys::jit_object_alloc(JIT_CLASS.load(Ordering::SeqCst));
        let dump = sys::outlet_new(obj.cast(), std::ptr::null());
        sys::max_jit_obex_dumpout_set(obj.cast(), dump);
        sys::max_jit_object_wrap_complete(obj.cast(), core.cast(), 0);
        sys::max_jit_obex_jitob_set(obj.cast(), core);
        (obj.cast(), core.cast(), dump)
    }
}

#[test]
fn the_flag_is_recorded_and_installs_a_max_method_per_gimmeback_method() {
    let k = median_mock::lock();
    classes();
    assert_eq!(median_mock::jit::wrap_flags("mock.gimmeback.on"), Some(1));
    assert_eq!(median_mock::jit::wrap_flags("mock.gimmeback.off"), Some(0));
    assert_eq!(median_mock::jit::wrap_flags("no.such.class"), None);
    let (on, core_on, _) = object("mock.gimmeback.on");
    for sel in ["cell", "count", "name", "nothing", "fail", "dict"] {
        let (_, types) = k.method(on, sel).unwrap_or_else(|| panic!("{sel}"));
        assert_eq!(types, [ty::A_GIMME as i32], "{sel}");
    }
    // the Jitter class's own gimme method is not wrapped
    assert!(k.method(on, "plain").is_none());
    let (off, core_off, _) = object("mock.gimmeback.off");
    for sel in ["cell", "count", "name", "nothing", "fail", "dict", "plain"] {
        assert!(k.method(off, sel).is_none(), "{sel}");
    }
    assert!(matches!(
        k.send(off, "cell", &[]),
        Err(SendError::NoMethod(_))
    ));
    for o in [on, core_on, off, core_off] {
        assert!(k.free_object(o));
    }
}

#[test]
fn an_atomarray_reply_goes_out_dumpout_as_a_list_and_is_freed() {
    let k = median_mock::lock();
    classes();
    let (on, core, dump) = object("mock.gimmeback.on");
    let live = containers::live_count();
    k.take_outlet_log();
    k.send(on, "cell", &[Atom::Long(9)]).unwrap();
    let log = k.take_outlet_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].outlet, dump as usize);
    assert_eq!(log[0].owner, on as usize);
    assert_eq!(log[0].selector, "cell");
    assert_eq!(
        log[0].atoms,
        [Atom::Long(1), Atom::Float(2.5), Atom::Sym("foo".into())]
    );
    assert_eq!(containers::live_count(), live, "the atomarray leaked");
    assert!(k.take_console_log().is_empty());
    assert!(k.free_object(on) && k.free_object(core));
}

#[test]
fn a_scalar_reply_is_a_one_atom_list_and_the_arguments_reach_the_method() {
    let k = median_mock::lock();
    classes();
    let (on, core, _) = object("mock.gimmeback.on");
    k.take_outlet_log();
    k.send(on, "count", &[Atom::Long(1), Atom::Long(2)])
        .unwrap();
    k.send(on, "name", &[]).unwrap();
    let log = k.take_outlet_log();
    assert_eq!(log.len(), 2, "{log:?}");
    assert_eq!(
        (log[0].selector.as_str(), &log[0].atoms[..]),
        ("count", &[Atom::Long(20)][..])
    );
    assert_eq!(
        (log[1].selector.as_str(), &log[1].atoms[..]),
        ("name", &[Atom::Sym("hello".into())][..])
    );
    assert!(k.free_object(on) && k.free_object(core));
}

#[test]
fn no_reply_an_error_or_a_non_atomarray_object_sends_nothing() {
    let k = median_mock::lock();
    classes();
    let (on, core, _) = object("mock.gimmeback.on");
    let live = containers::live_count();
    k.take_outlet_log();
    k.send(on, "nothing", &[]).unwrap();
    k.send(on, "fail", &[]).unwrap();
    k.send(on, "dict", &[]).unwrap();
    assert!(k.take_outlet_log().is_empty());
    // the dictionary was freed by the wrap
    assert_eq!(containers::live_count(), live);
    assert!(k.free_object(on) && k.free_object(core));
}
