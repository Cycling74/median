//! `attr_args_offset`, `attr_args_process` and `atom_gettext`, through the C symbols max-sys
//! declares, with a hand-written C style class whose `new` processes its attribute arguments as
//! the SDK documents.
//!
//! Every test takes `median_mock::lock()`, which serialises them.

#![allow(clippy::missing_safety_doc)]
// `c_long` is `i64` here but 32 bit on Windows
#![allow(clippy::unnecessary_cast)]

use max_sys::{e_max_atomtypes as ty, t_atom, t_class, t_max_err, t_object, t_symbol};
use median_mock::{Atom, Level};
use std::ffi::{CStr, c_char, c_long, c_short, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, Once};

#[repr(C)]
struct Thing {
    ob: t_object,
    gain: f64,
    level: i64,
    name: *mut t_symbol,
}

static CLASS: AtomicPtr<t_class> = AtomicPtr::new(null_mut());
/// `(argc)` of every call of the `level` setter
static LEVEL_SETS: Mutex<Vec<c_long>> = Mutex::new(Vec::new());

macro_rules! meth {
    ($f:expr, $ty:ty) => {{
        #[allow(unused_unsafe)]
        unsafe {
            std::mem::transmute::<$ty, max_sys::method>($f)
        }
    }};
}

fn sym(s: &CStr) -> *mut t_symbol {
    unsafe { max_sys::gensym(s.as_ptr()) }
}

extern "C" fn thing_new(_s: *mut t_symbol, argc: c_long, argv: *mut t_atom) -> *mut c_void {
    unsafe {
        let x = max_sys::object_alloc(CLASS.load(Ordering::SeqCst)) as *mut Thing;
        (*x).level = -1;
        (*x).name = sym(c"none");
        max_sys::attr_args_process(x as _, argc as c_short, argv);
        x as *mut c_void
    }
}

unsafe extern "C" fn level_get(
    x: *mut Thing,
    _attr: *mut t_object,
    argc: *mut c_long,
    argv: *mut *mut t_atom,
) -> t_max_err {
    unsafe {
        if *argc < 1 || (*argv).is_null() {
            *argc = 1;
            *argv = max_sys::sysmem_newptr(size_of::<t_atom>() as _) as *mut t_atom;
        }
        max_sys::atom_setlong(*argv, (*x).level as isize)
    }
}

unsafe extern "C" fn level_set(
    x: *mut Thing,
    _attr: *mut t_object,
    argc: c_long,
    argv: *mut t_atom,
) -> t_max_err {
    LEVEL_SETS.lock().unwrap().push(argc);
    unsafe {
        if argc > 0 && !argv.is_null() {
            (*x).level = max_sys::atom_getlong(argv) as i64;
        }
    }
    0
}

fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let c = max_sys::class_new(
            c"attr.args".as_ptr(),
            meth!(
                thing_new,
                extern "C" fn(*mut t_symbol, c_long, *mut t_atom) -> *mut c_void
            ),
            None,
            size_of::<Thing>() as c_long,
            None,
            ty::A_GIMME as c_short,
            0,
        );
        let gain = max_sys::attr_offset_new(
            c"gain".as_ptr(),
            sym(c"float64"),
            0,
            None,
            None,
            std::mem::offset_of!(Thing, gain) as c_long,
        );
        assert_eq!(max_sys::attr_addfilter_clip(gain as _, 0.0, 1.0, 1, 1), 0);
        assert_eq!(max_sys::class_addattr(c, gain), 0);
        let level = max_sys::attribute_new(
            c"level".as_ptr(),
            sym(c"long"),
            0,
            meth!(
                level_get,
                unsafe extern "C" fn(
                    *mut Thing,
                    *mut t_object,
                    *mut c_long,
                    *mut *mut t_atom,
                ) -> t_max_err
            ),
            meth!(
                level_set,
                unsafe extern "C" fn(*mut Thing, *mut t_object, c_long, *mut t_atom) -> t_max_err
            ),
        );
        assert_eq!(max_sys::class_addattr(c, level), 0);
        let name = max_sys::attr_offset_new(
            c"name".as_ptr(),
            sym(c"symbol"),
            0,
            None,
            None,
            std::mem::offset_of!(Thing, name) as c_long,
        );
        assert_eq!(max_sys::class_addattr(c, name), 0);
        assert_eq!(max_sys::class_register(sym(c"box"), c), 0);
        CLASS.store(c, Ordering::SeqCst);
    });
}

fn raw(atoms: &[Atom]) -> Vec<t_atom> {
    atoms.iter().map(Atom::to_raw).collect()
}

fn offset(atoms: &[Atom]) -> c_long {
    let mut raw = raw(atoms);
    unsafe { max_sys::attr_args_offset(raw.len() as c_short, raw.as_mut_ptr()) }
}

fn text(atoms: &[Atom]) -> String {
    let mut raw = raw(atoms);
    let mut size: c_long = -1;
    let mut text: *mut c_char = null_mut();
    unsafe {
        assert_eq!(
            max_sys::atom_gettext(
                raw.len() as c_long,
                raw.as_mut_ptr(),
                &mut size,
                &mut text,
                0
            ),
            0
        );
        let s = CStr::from_ptr(text).to_str().unwrap().to_owned();
        assert_eq!(size as usize, s.len());
        max_sys::sysmem_freeptr(text as _);
        s
    }
}

fn s(v: &str) -> Atom {
    Atom::Sym(v.into())
}

#[test]
fn attr_args_offset_is_the_first_at_symbol() {
    let _k = median_mock::lock();
    // the SDK header's example, `foo bar 3.0 @mode 6`
    assert_eq!(
        offset(&[
            s("foo"),
            s("bar"),
            Atom::Float(3.0),
            s("@mode"),
            Atom::Long(6)
        ]),
        3
    );
    assert_eq!(offset(&[s("@a"), Atom::Long(1)]), 0);
    // no attribute arguments: the count
    assert_eq!(offset(&[Atom::Long(1), s("a")]), 2);
    assert_eq!(offset(&[]), 0);
    // only a symbol counts, and the first one wins
    assert_eq!(offset(&[Atom::Long(4), s("a@b"), s("@x"), s("@y")]), 2);
    assert_eq!(unsafe { max_sys::attr_args_offset(0, null_mut()) }, 0);
}

#[test]
fn attr_args_process_sets_each_group() {
    let k = median_mock::lock();
    register();
    LEVEL_SETS.lock().unwrap().clear();
    let x = k.new_object(
        "attr.args",
        &[
            Atom::Long(9),
            s("positional"),
            s("@gain"),
            Atom::Float(0.5),
            s("@level"),
            Atom::Long(7),
            s("@name"),
            s("hello"),
        ],
    );
    assert!(!x.is_null());
    assert!(k.take_console_log().is_empty());
    assert_eq!(k.attr_get(x, "gain").unwrap(), [Atom::Float(0.5)]);
    assert_eq!(k.attr_get(x, "level").unwrap(), [Atom::Long(7)]);
    assert_eq!(k.attr_get(x, "name").unwrap(), [s("hello")]);
    // the setter ran once, with the group's atoms
    assert_eq!(*LEVEL_SETS.lock().unwrap(), [1]);
    assert!(k.free_object(x));

    // no attribute arguments, or only positional ones: nothing is set
    LEVEL_SETS.lock().unwrap().clear();
    let x = k.new_object("attr.args", &[Atom::Long(1), s("a")]);
    assert_eq!(k.attr_get(x, "level").unwrap(), [Atom::Long(-1)]);
    assert!(LEVEL_SETS.lock().unwrap().is_empty());
    assert!(k.free_object(x));

    // the setters see the clip filters and the whole group
    let x = k.new_object(
        "attr.args",
        &[
            s("@gain"),
            Atom::Float(7.0),
            s("@level"),
            Atom::Long(3),
            Atom::Long(4),
        ],
    );
    assert_eq!(k.attr_get(x, "gain").unwrap(), [Atom::Float(1.0)]);
    assert_eq!(k.attr_get(x, "level").unwrap(), [Atom::Long(3)]);
    assert_eq!(LEVEL_SETS.lock().unwrap().last(), Some(&2));
    assert!(k.free_object(x));
}

#[test]
fn attr_args_process_posts_a_failed_group_and_goes_on() {
    let k = median_mock::lock();
    register();
    let x = k.new_object(
        "attr.args",
        &[s("@nope"), Atom::Long(1), s("@level"), Atom::Long(5)],
    );
    assert!(!x.is_null());
    let log = k.take_console_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].level, Level::Error);
    assert_eq!(log[0].object, x as usize);
    assert!(log[0].text.contains("@nope"), "{log:?}");
    assert_eq!(k.attr_get(x, "level").unwrap(), [Atom::Long(5)]);
    assert!(k.free_object(x));
}

#[test]
fn atom_gettext_renders_atoms_as_max_shows_them() {
    let _k = median_mock::lock();
    assert_eq!(text(&[]), "");
    assert_eq!(text(&[Atom::Long(-3)]), "-3");
    assert_eq!(text(&[Atom::Float(3.0)]), "3.");
    assert_eq!(text(&[Atom::Float(2.5)]), "2.5");
    assert_eq!(text(&[Atom::Float(-0.125)]), "-0.125");
    assert_eq!(text(&[s("foo")]), "foo");
    assert_eq!(text(&[s("two words")]), "\"two words\"");
    assert_eq!(
        text(&[
            Atom::Long(1),
            Atom::Float(2.5),
            s("foo"),
            s("@value"),
            Atom::Long(7)
        ]),
        "1 2.5 foo @value 7"
    );
}

#[test]
fn atom_gettext_refuses_null_out_pointers() {
    let _k = median_mock::lock();
    let mut size: c_long = 0;
    let mut text: *mut c_char = null_mut();
    unsafe {
        assert_ne!(
            max_sys::atom_gettext(0, null_mut(), null_mut(), &mut text, 0),
            0
        );
        assert_ne!(
            max_sys::atom_gettext(0, null_mut(), &mut size, null_mut(), 0),
            0
        );
    }
}
