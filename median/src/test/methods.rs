//! Tests of the typed method registration: call the trampolines that `Class::add_*` registers
//! on a stand-in for the wrapper, as Max would.

use crate::{
    atom::Atom,
    class::Class,
    method::*,
    object::{MaxObj, ObjRef},
    symbol::SymbolRef,
    wrapper::{ObjWrapped, WrapperWrapped},
};
use max_sys::t_atom_long;
use std::{
    ffi::{CStr, c_char, c_long},
    fmt::Display,
    sync::Mutex,
};

/// The `t_object`s that the `object_error` stub was called with.
static ERRORS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Stand-in for the variadic `object_error` (the format arguments are ignored).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_error(x: *mut max_sys::t_object, _fmt: *const c_char) {
    ERRORS.lock().unwrap().push(x as usize);
}

/// Stand-in for the variadic `error`, which the library calls where there is no object (the
/// format arguments are ignored).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn error(_fmt: *const c_char) {}

/// Stand-in for `common_symbols_gettable`: a table of null symbols.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn common_symbols_gettable() -> *mut max_sys::t_common_symbols_table {
    static TABLE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    // SAFETY: the table is plain data (pointers)
    *TABLE.get_or_init(|| {
        Box::into_raw(Box::new(unsafe {
            std::mem::zeroed::<max_sys::t_common_symbols_table>()
        })) as usize
    }) as *mut _
}

/// (class, selector, method) of every `class_addmethod` call. The argument types, which are
/// variadic, are not recorded.
static METHODS: Mutex<Vec<(usize, String, usize)>> = Mutex::new(Vec::new());

/// Stand-in for the variadic `class_addmethod`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn class_addmethod(
    c: *mut max_sys::t_class,
    m: max_sys::method,
    name: *const c_char,
) -> max_sys::t_max_err {
    // SAFETY: `name` is a C string
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    METHODS
        .lock()
        .unwrap()
        .push((c as usize, name, m.map_or(0, |m| m as usize)));
    max_sys::e_max_errorcodes::MAX_ERR_NONE as _
}

/// How many errors were posted for `w`. Starts at 0 for every wrapper: an address can be reused by
/// a later test, so the entries a freed wrapper left behind are dropped.
fn errors_for<T>(w: &Wrap<T>) -> usize {
    let me = w.max_obj() as usize;
    ERRORS.lock().unwrap().iter().filter(|e| **e == me).count() - w.fresh
}

/// What Max allocates for an object: the header, then our data.
#[repr(C)]
struct Wrap<T> {
    obj: max_sys::t_object,
    t: T,
    /// errors already posted for this address when the wrapper was made
    fresh: usize,
}

impl<T> Wrap<T> {
    fn new(t: T) -> Box<Self> {
        let mut w = Box::new(Self {
            // SAFETY: a `t_object` is plain data (integers, pointers) that Max zeroes
            obj: unsafe { std::mem::zeroed() },
            t,
            fresh: 0,
        });
        w.fresh = errors_for(&w);
        w
    }
}

// SAFETY: `repr(C)` with a `t_object` first
unsafe impl<T> MaxObj for Wrap<T> {}

impl<T> WrapperWrapped<T> for Wrap<T> {
    fn wrapped(&self) -> &T {
        &self.t
    }
}

/// Records what is called, and the errors it is given.
#[derive(Default)]
struct Probe {
    log: Mutex<Vec<String>>,
    /// (object, message) of each `report_error`
    reported: Mutex<Vec<(usize, String)>>,
}

impl Probe {
    fn log(&self, s: impl Into<String>) {
        self.log.lock().unwrap().push(s.into());
    }
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut self.log.lock().unwrap())
    }
}

impl ObjWrapped<Probe> for Probe {
    fn class_name() -> &'static str {
        "probe"
    }
    fn report_error(&self, obj: &ObjRef, err: &dyn Display) {
        self.reported
            .lock()
            .unwrap()
            .push((obj.as_ptr() as usize, err.to_string()));
    }
}

/// Keeps the default `report_error`.
#[derive(Default)]
struct Plain;

impl ObjWrapped<Plain> for Plain {
    fn class_name() -> &'static str {
        "plain"
    }
}

type W = Wrap<Probe>;

fn probe() -> Box<W> {
    Wrap::new(Probe::default())
}

/// A `t_symbol` that lives on the stack (we don't have `gensym`).
fn symbol(name: &'static CStr) -> max_sys::t_symbol {
    // SAFETY: `t_symbol` is plain data, only `s_name` is read
    let mut s: max_sys::t_symbol = unsafe { std::mem::zeroed() };
    s.s_name = name.as_ptr() as *mut c_char;
    s
}

fn long_atom(v: t_atom_long) -> max_sys::t_atom {
    // SAFETY: `t_atom` is plain data
    let mut a: max_sys::t_atom = unsafe { std::mem::zeroed() };
    a.a_type = max_sys::e_max_atomtypes::A_LONG as _;
    a.a_w.w_long = v;
    a
}

fn describe(a: &[Atom]) -> String {
    a.iter()
        .map(|a| unsafe { a.value.a_w.w_long }.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn name(s: &SymbolRef) -> String {
    s.to_cstring().to_str().unwrap().to_string()
}

type Gimme = extern "C" fn(&W, *mut max_sys::t_symbol, c_long, *const max_sys::t_atom);

/// Turn the registered method back into the typed function Max would call.
unsafe fn cast<P: Copy>(m: MaxMethod) -> P {
    assert_eq!(std::mem::size_of::<P>(), std::mem::size_of::<MaxMethod>());
    unsafe { std::mem::transmute_copy::<MaxMethod, P>(&m) }
}

#[test]
fn bang() {
    let w = probe();
    let m = bang_method::<W, _, _, _>(|p: &Probe| p.log("bang"));
    let f: extern "C" fn(&W) = unsafe { cast(m) };
    f(&w);
    f(&w);
    assert_eq!(w.t.take(), ["bang", "bang"]);
}

#[test]
fn int_float() {
    let w = probe();
    let f: extern "C" fn(&W, t_atom_long) = unsafe {
        cast(int_method::<W, _, _, _>(|p: &Probe, v| {
            p.log(format!("int {v}"))
        }))
    };
    f(&w, -7);
    let f: extern "C" fn(&W, f64) = unsafe {
        cast(float_method::<W, _, _, _>(|p: &Probe, v| {
            p.log(format!("float {v}"))
        }))
    };
    f(&w, 1.5);
    assert_eq!(w.t.take(), ["int -7", "float 1.5"]);
}

#[test]
fn symbol_method() {
    let w = probe();
    let f: extern "C" fn(&W, *mut max_sys::t_symbol) = unsafe {
        cast(sym_method::<W, _, _, _>(|p: &Probe, s: SymbolRef| {
            p.log(format!("sym {}", name(&s)))
        }))
    };
    let mut s = symbol(c"hello");
    f(&w, &mut s);
    assert_eq!(w.t.take(), ["sym hello"]);
}

#[test]
fn list_any_gimme() {
    let w = probe();
    let atoms = [long_atom(3), long_atom(4)];
    let mut sel = symbol(c"foo");

    let f: Gimme = unsafe {
        cast(list_method::<W, _, _, _>(|p: &Probe, a: &[Atom]| {
            p.log(format!("list {}", describe(a)))
        }))
    };
    f(&w, &mut sel, 2, atoms.as_ptr());
    //no atoms: argv may be null
    f(&w, &mut sel, 0, std::ptr::null());

    let f: Gimme = unsafe {
        cast(any_method::<W, _, _, _>(
            |p: &Probe, s: &SymbolRef, a: &[Atom]| {
                p.log(format!("any {} {}", name(s), describe(a)))
            },
        ))
    };
    f(&w, &mut sel, 2, atoms.as_ptr());

    let f: Gimme = unsafe {
        cast(sel_gimme_method::<W, _, _, _>(
            |p: &Probe, s: &SymbolRef, a: &[Atom]| p.log(format!("gimme {} {}", name(s), a.len())),
        ))
    };
    f(&w, &mut sel, 1, atoms.as_ptr());
    assert_eq!(
        w.t.take(),
        ["list 3,4", "list ", "any foo 3,4", "gimme foo 1"]
    );
}

fn set(p: &Probe, s: SymbolRef) {
    p.log(format!("set {}", name(&s)));
}

#[test]
fn sel_args() {
    let w = probe();

    //arity 0
    fn zero(p: &Probe) {
        p.log("zero")
    }
    fn seven(
        p: &Probe,
        a: f64,
        b: t_atom_long,
        c: SymbolRef,
        d: f64,
        e: f64,
        f: t_atom_long,
        g: SymbolRef,
    ) {
        p.log(format!(
            "seven {a} {b} {} {d} {e} {f} {}",
            name(&c),
            name(&g)
        ))
    }
    fn mixed(p: &Probe, a: f64, b: t_atom_long, c: SymbolRef) -> Result<(), String> {
        p.log(format!("mixed {a} {b} {}", name(&c)));
        Ok(())
    }
    fn types<F: SelFn<Probe, A, R>, A, R>(_: F) -> &'static [max_sys::e_max_atomtypes::Type] {
        F::TYPES
    }
    use max_sys::e_max_atomtypes::{A_FLOAT, A_LONG, A_SYM};
    assert_eq!(types(zero), &[]);
    assert_eq!(types(set), &[A_SYM]);
    assert_eq!(types(mixed), &[A_FLOAT, A_LONG, A_SYM]);
    assert_eq!(
        types(seven),
        &[A_FLOAT, A_LONG, A_SYM, A_FLOAT, A_FLOAT, A_LONG, A_SYM]
    );
    fn tramp<F: SelFn<Probe, A, R>, A, R>(_: F) -> MaxMethod {
        F::trampoline::<W>()
    }

    let f: extern "C" fn(&W) = unsafe { cast(tramp(zero)) };
    f(&w);

    let f: extern "C" fn(&W, *mut max_sys::t_symbol) = unsafe { cast(tramp(set)) };
    let mut s = symbol(c"abc");
    f(&w, &mut s);
    //a null symbol (a defaulted argument) is the empty symbol
    let f: extern "C" fn(&W, *mut max_sys::t_symbol) = unsafe { cast(tramp(set)) };
    let mut empty = symbol(c"");
    f(&w, &mut empty);

    let f: extern "C" fn(&W, f64, t_atom_long, *mut max_sys::t_symbol) =
        unsafe { cast(tramp(mixed)) };
    f(&w, 0.25, 9, &mut s);

    let f: extern "C" fn(
        &W,
        f64,
        t_atom_long,
        *mut max_sys::t_symbol,
        f64,
        f64,
        t_atom_long,
        *mut max_sys::t_symbol,
    ) = unsafe { cast(tramp(seven)) };
    let mut t = symbol(c"t");
    f(&w, 1., 2, &mut s, 4., 5., 6, &mut t);

    //a closure works too
    let f: extern "C" fn(&W, f64, f64) = unsafe {
        cast(tramp(|p: &Probe, a: f64, b: f64| {
            p.log(format!("closure {}", a + b))
        }))
    };
    f(&w, 1., 2.);

    assert_eq!(
        w.t.take(),
        [
            "zero",
            "set abc",
            "set ",
            "mixed 0.25 9 abc",
            "seven 1 2 abc 4 5 6 t",
            "closure 3"
        ]
    );
}

#[test]
fn err_reaches_report_error() {
    let w = probe();
    let f: extern "C" fn(&W, t_atom_long) = unsafe {
        cast(int_method::<W, _, _, _>(|_: &Probe, v| {
            if v < 0 {
                Err(format!("bad value {v}"))
            } else {
                Ok(())
            }
        }))
    };
    f(&w, 1);
    assert!(w.t.reported.lock().unwrap().is_empty());
    f(&w, -2);
    assert_eq!(
        *w.t.reported.lock().unwrap(),
        [(w.max_obj() as usize, "bad value -2".to_string())]
    );
}

#[test]
fn default_report_error_posts_error_for_the_object() {
    let w = Wrap::new(Plain);
    let f: extern "C" fn(&Wrap<Plain>) = unsafe {
        cast(bang_method::<Wrap<Plain>, _, _, _>(|_: &Plain| {
            Err::<(), _>("nope")
        }))
    };
    assert_eq!(errors_for(&w), 0);
    f(&w);
    assert_eq!(errors_for(&w), 1);
}

#[test]
fn panic_is_contained() {
    let w = probe();
    let f: extern "C" fn(&W) = unsafe {
        cast(bang_method::<W, _, _, _>(|p: &Probe| -> () {
            p.log("before");
            panic!("boom");
        }))
    };
    assert_eq!(errors_for(&w), 0);
    //returns normally: the panic didn't unwind out of the `extern "C"` fn
    f(&w);
    //the guard posted it for the object, and no error was reported through `report_error`
    assert_eq!(errors_for(&w), 1);
    assert_eq!(w.t.take(), ["before"]);
    assert!(w.t.reported.lock().unwrap().is_empty());
    //and the object is still usable
    f(&w);
    assert_eq!(errors_for(&w), 2);
}

#[test]
fn panic_in_report_error_is_contained() {
    struct Bad;
    impl ObjWrapped<Bad> for Bad {
        fn class_name() -> &'static str {
            "bad"
        }
        fn report_error(&self, _obj: &ObjRef, _err: &dyn Display) {
            panic!("report failed");
        }
    }
    let w = Wrap::new(Bad);
    let f: extern "C" fn(&Wrap<Bad>) = unsafe {
        cast(bang_method::<Wrap<Bad>, _, _, _>(|_: &Bad| {
            Err::<(), _>("x")
        }))
    };
    f(&w);
    assert_eq!(errors_for(&w), 1);
}

#[test]
fn sel_defaults() {
    use max_sys::e_max_atomtypes::*;
    let mut t = [A_FLOAT, A_LONG, A_SYM];
    assert!(crate::class::default_types(&mut t, 0).is_ok());
    assert_eq!(t, [A_FLOAT, A_LONG, A_SYM]);
    assert!(crate::class::default_types(&mut t, 2).is_ok());
    assert_eq!(t, [A_FLOAT, A_DEFLONG, A_DEFSYM]);
    //already default is fine
    assert!(crate::class::default_types(&mut t, 3).is_ok());
    assert_eq!(t, [A_DEFFLOAT, A_DEFLONG, A_DEFSYM]);
    //more defaults than arguments
    let mut t = [A_FLOAT];
    assert!(crate::class::default_types(&mut t, 2).is_err());
    assert_eq!(t, [A_FLOAT]);
    //a type that has no default form, and nothing is changed
    let mut t = [A_FLOAT, A_GIMME];
    assert!(crate::class::default_types(&mut t, 2).is_err());
    assert_eq!(t, [A_FLOAT, A_GIMME]);
    assert!(crate::class::default_types(&mut t, 1).is_err());
}

#[test]
fn class_registers_the_methods() {
    //a class pointer that is only a key for the stub
    let key = Box::new(0u8);
    let id = &*key as *const u8 as usize;
    // SAFETY: the stub never dereferences the class
    let mut c: Class<W> = unsafe { Class::new_registered(id as *mut max_sys::t_class) };

    c.add_bang(|p: &Probe| p.log("bang")).unwrap();
    c.add_int(|p: &Probe, v| p.log(format!("int {v}"))).unwrap();
    c.add_float(|p: &Probe, v| p.log(format!("float {v}")))
        .unwrap();
    c.add_sym(|p: &Probe, s: SymbolRef| p.log(format!("symbol {}", name(&s))))
        .unwrap();
    c.add_list(|p: &Probe, a: &[Atom]| p.log(format!("list {}", a.len())))
        .unwrap();
    c.add_any(|p: &Probe, s: &SymbolRef, a: &[Atom]| {
        p.log(format!("anything {} {}", name(s), a.len()))
    })
    .unwrap();
    c.add_sel(c"set", set, 1).unwrap();
    c.add_sel(
        c"two",
        |p: &Probe, a: t_atom_long, b: f64| p.log(format!("two {a} {b}")),
        0,
    )
    .unwrap();
    c.add_sel_gimme(c"gimme", |p: &Probe, s: &SymbolRef, a: &[Atom]| {
        p.log(format!("gimme {} {}", name(s), a.len()))
    })
    .unwrap();

    let registered: Vec<_> = METHODS
        .lock()
        .unwrap()
        .iter()
        .filter(|(c, _, _)| *c == id)
        .cloned()
        .collect();
    let names: Vec<_> = registered.iter().map(|(_, n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "bang", "int", "float", "symbol", "list", "anything", "set", "two", "gimme"
        ]
    );
    assert!(registered.iter().all(|(_, _, m)| *m != 0));

    //and the methods that were registered are the ones that work
    let w = probe();
    let m = |i: usize| unsafe { std::mem::transmute::<usize, MaxMethod>(registered[i].2) };
    let f: extern "C" fn(&W) = unsafe { cast(m(0)) };
    f(&w);
    let f: extern "C" fn(&W, t_atom_long) = unsafe { cast(m(1)) };
    f(&w, 5);
    let f: extern "C" fn(&W, f64) = unsafe { cast(m(2)) };
    f(&w, 0.5);
    let f: extern "C" fn(&W, *mut max_sys::t_symbol) = unsafe { cast(m(3)) };
    let mut s = symbol(c"s");
    f(&w, &mut s);
    let atoms = [long_atom(1), long_atom(2)];
    let f: Gimme = unsafe { cast(m(4)) };
    f(&w, &mut s, 2, atoms.as_ptr());
    let f: Gimme = unsafe { cast(m(5)) };
    f(&w, &mut s, 1, atoms.as_ptr());
    let f: extern "C" fn(&W, *mut max_sys::t_symbol) = unsafe { cast(m(6)) };
    f(&w, &mut s);
    let f: extern "C" fn(&W, t_atom_long, f64) = unsafe { cast(m(7)) };
    f(&w, 3, 4.5);
    let f: Gimme = unsafe { cast(m(8)) };
    f(&w, &mut s, 2, atoms.as_ptr());
    assert_eq!(
        w.t.take(),
        [
            "bang",
            "int 5",
            "float 0.5",
            "symbol s",
            "list 2",
            "anything s 1",
            "set s",
            "two 3 4.5",
            "gimme s 2"
        ]
    );
}

#[test]
fn add_sel_with_too_many_defaults_is_an_error_not_a_panic() {
    // SAFETY: the class is never used, the call fails before it would be
    let mut c: Class<W> = unsafe { Class::new_registered(std::ptr::null_mut()) };
    assert!(c.add_sel(c"set", set, 2).is_err());
    assert!(c.add_sel(c"nothing", |_: &Probe| {}, 1).is_err());
}
