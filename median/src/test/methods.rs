//! Tests of the typed method registration: call the trampolines that `Class::add_*` builds on a
//! stand-in for the wrapper, as Max would, and send messages to a class made with `Class::new`
//! through the `median-mock` kernel. Every test holds `median_mock::lock()` to the end, which
//! also keeps the console log (where the errors are posted) its own.

use crate::{
    atom::Atom,
    class::Class,
    method::*,
    object::{MaxObj, ObjRef},
    symbol::SymbolRef,
    wrapper::WrapperWrapped,
};
use max_sys::t_atom_long;
use median_mock::{Kernel, Level};
use std::{
    ffi::{CStr, c_char, c_long, c_void},
    fmt::Display,
    sync::{
        Mutex,
        atomic::{AtomicPtr, Ordering},
    },
};

/// The messages posted as errors for `obj` (with `object_error`, which is what the default
/// `report_error` and the panic guards do), oldest first. `Kernel::reset` empties the log, so
/// each test sees only its own.
pub(crate) fn errors_for(k: &Kernel, obj: *mut max_sys::t_object) -> Vec<String> {
    k.console_log()
        .into_iter()
        .filter(|e| e.level == Level::Error && e.object == obj as usize)
        .map(|e| e.text)
        .collect()
}

/// What Max allocates for an object: the header, then our data.
#[repr(C)]
struct Wrap<T> {
    obj: max_sys::t_object,
    t: T,
}

impl<T> Wrap<T> {
    fn new(t: T) -> Box<Self> {
        Box::new(Self {
            // SAFETY: a `t_object` is plain data (integers, pointers) that Max zeroes
            obj: unsafe { std::mem::zeroed() },
            t,
        })
    }
}

// SAFETY: `repr(C)` with a `t_object` first
unsafe impl<T> MaxObj for Wrap<T> {}

/// The `report_error` hook of a wrapped type, as `MaxObjWrapped` has it.
trait Hook {
    fn report_error(&self, obj: &ObjRef, err: &dyn Display) {
        obj.error(err);
    }
}

impl<T: Hook> WrapperWrapped<T> for Wrap<T> {
    fn wrapped(&self) -> &T {
        &self.t
    }
    fn report_error(&self, err: &dyn Display) {
        //what the real wrappers do: a handle for the call only
        // SAFETY: `obj` is the header of this live object, the handle is dropped on return
        let obj = unsafe { ObjRef::from_raw(std::ptr::NonNull::from(&self.obj)) };
        self.t.report_error(&obj, err);
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

impl Hook for Probe {
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

impl Hook for Plain {}

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
    s.to_str().unwrap().to_string()
}

type Gimme = extern "C" fn(&W, *mut max_sys::t_symbol, c_long, *const max_sys::t_atom);

/// Turn the registered method back into the typed function Max would call.
unsafe fn cast<P: Copy>(m: MaxMethod) -> P {
    assert_eq!(std::mem::size_of::<P>(), std::mem::size_of::<MaxMethod>());
    unsafe { std::mem::transmute_copy::<MaxMethod, P>(&m) }
}

#[test]
fn bang() {
    let _k = median_mock::lock();
    let w = probe();
    let m = bang_method::<W, _, _, _>(|p: &Probe| p.log("bang"));
    let f: extern "C" fn(&W) = unsafe { cast(m) };
    f(&w);
    f(&w);
    assert_eq!(w.t.take(), ["bang", "bang"]);
}

#[test]
fn int_float() {
    let _k = median_mock::lock();
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
    let _k = median_mock::lock();
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
    let _k = median_mock::lock();
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
        cast(sel_gimme_method::<W, _, _, _, _>(
            |p: &Probe, s: &SymbolRef, a: &[Atom]| p.log(format!("gimme {} {}", name(s), a.len())),
        ))
    };
    f(&w, &mut sel, 1, atoms.as_ptr());
    //the selector-less shape of the same method: the selector is not passed on
    let f: Gimme = unsafe {
        cast(sel_gimme_method::<W, _, _, _, _>(
            |p: &Probe, a: &[Atom]| p.log(format!("gimme no selector {}", a.len())),
        ))
    };
    f(&w, &mut sel, 1, atoms.as_ptr());
    assert_eq!(
        w.t.take(),
        [
            "list 3,4",
            "list ",
            "any foo 3,4",
            "gimme foo 1",
            "gimme no selector 1"
        ]
    );
}

fn set(p: &Probe, s: SymbolRef) {
    p.log(format!("set {}", name(&s)));
}

fn gimme_with_sel(p: &Probe, s: &SymbolRef, a: &[Atom]) {
    p.log(format!("with {} {}", name(s), a.len()));
}

fn gimme_plain(p: &Probe, a: &[Atom]) {
    p.log(format!("plain {}", a.len()));
}

#[test]
fn sel_args() {
    let _k = median_mock::lock();
    let w = probe();

    //arity 0
    fn zero(p: &Probe) {
        p.log("zero")
    }
    #[allow(clippy::too_many_arguments)] // Exercises Max's maximum typed arity.
    fn seven(
        p: &Probe,
        a: t_atom_long,
        b: SymbolRef,
        c: t_atom_long,
        d: t_atom_long,
        e: SymbolRef,
        f: t_atom_long,
        g: SymbolRef,
    ) {
        p.log(format!(
            "seven {a} {} {c} {d} {} {f} {}",
            name(&b),
            name(&e),
            name(&g)
        ))
    }
    fn mixed(p: &Probe, b: t_atom_long, c: SymbolRef) -> Result<(), String> {
        p.log(format!("mixed {b} {}", name(&c)));
        Ok(())
    }
    //the float/other mix that `trampoline` (so `add_sel`) refuses to compile: its types are
    //still what they are
    fn floats_and_others(_: &Probe, _a: f64, _b: t_atom_long) {}
    fn types<F: SelFn<Probe, A, R>, A, R>(_: F) -> &'static [max_sys::e_max_atomtypes::Type] {
        F::TYPES
    }
    use max_sys::e_max_atomtypes::{A_FLOAT, A_LONG, A_SYM};
    assert!(types(zero).is_empty());
    assert_eq!(types(set), &[A_SYM]);
    assert_eq!(types(mixed), &[A_LONG, A_SYM]);
    assert_eq!(types(floats_and_others), &[A_FLOAT, A_LONG]);
    assert!(crate::method::mixes_float(types(floats_and_others)));
    assert!(!crate::method::mixes_float(types(mixed)));
    assert!(!crate::method::mixes_float(&[A_FLOAT, A_FLOAT]));
    assert!(!crate::method::mixes_float(&[]));
    assert_eq!(
        types(seven),
        &[A_LONG, A_SYM, A_LONG, A_LONG, A_SYM, A_LONG, A_SYM]
    );
    fn tramp<F: SelFn<Probe, A, R>, A, R>(_: F) -> MaxMethod {
        F::trampoline::<W>()
    }

    let f: extern "C" fn(&W) = unsafe { cast(tramp(zero)) };
    f(&w);

    let f: extern "C" fn(&W, *mut max_sys::t_symbol) = unsafe { cast(tramp(set)) };
    let mut s = symbol(c"abc");
    f(&w, &mut s);
    //a null symbol (a defaulted argument) is the empty symbol (`s_nothing`, from the kernel)
    let f: extern "C" fn(&W, *mut max_sys::t_symbol) = unsafe { cast(tramp(set)) };
    f(&w, std::ptr::null_mut());
    let f: extern "C" fn(&W, *mut max_sys::t_symbol) = unsafe {
        cast(tramp(|p: &Probe, s: SymbolRef| {
            p.log(format!("nothing {}", s == SymbolRef::default()))
        }))
    };
    f(&w, std::ptr::null_mut());
    //and that is a real symbol, not null
    assert!(!SymbolRef::default().as_ptr().is_null());
    assert_eq!(w.t.take(), ["zero", "set abc", "set ", "nothing true"]);

    let f: extern "C" fn(&W, t_atom_long, *mut max_sys::t_symbol) = unsafe { cast(tramp(mixed)) };
    f(&w, 9, &mut s);

    let f: extern "C" fn(
        &W,
        t_atom_long,
        *mut max_sys::t_symbol,
        t_atom_long,
        t_atom_long,
        *mut max_sys::t_symbol,
        t_atom_long,
        *mut max_sys::t_symbol,
    ) = unsafe { cast(tramp(seven)) };
    let mut t = symbol(c"t");
    f(&w, 1, &mut s, 3, 4, &mut t, 6, &mut s);

    //a closure works too
    let f: extern "C" fn(&W, f64, f64) = unsafe {
        cast(tramp(|p: &Probe, a: f64, b: f64| {
            p.log(format!("closure {}", a + b))
        }))
    };
    f(&w, 1., 2.);

    assert_eq!(
        w.t.take(),
        ["mixed 9 abc", "seven 1 abc 3 4 t 6 abc", "closure 3"]
    );
}

#[test]
fn err_reaches_report_error() {
    let k = median_mock::lock();
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
    //this `report_error` doesn't post
    assert!(k.console_log().is_empty());
}

#[test]
fn default_report_error_posts_error_for_the_object() {
    let k = median_mock::lock();
    let w = Wrap::new(Plain);
    let f: extern "C" fn(&Wrap<Plain>) = unsafe {
        cast(bang_method::<Wrap<Plain>, _, _, _>(|_: &Plain| {
            Err::<(), _>("nope")
        }))
    };
    assert!(errors_for(&k, w.max_obj()).is_empty());
    f(&w);
    assert_eq!(errors_for(&k, w.max_obj()), ["nope"]);
}

#[test]
fn panic_is_contained() {
    let k = median_mock::lock();
    let w = probe();
    let f: extern "C" fn(&W) = unsafe {
        cast(bang_method::<W, _, _, _>(|p: &Probe| -> () {
            p.log("before");
            panic!("boom");
        }))
    };
    assert!(errors_for(&k, w.max_obj()).is_empty());
    //returns normally: the panic didn't unwind out of the `extern "C"` fn
    f(&w);
    //the guard posted it for the object, and no error was reported through `report_error`
    let text = "median: panic in bang: boom";
    assert_eq!(errors_for(&k, w.max_obj()), [text]);
    assert_eq!(w.t.take(), ["before"]);
    assert!(w.t.reported.lock().unwrap().is_empty());
    //and the object is still usable
    f(&w);
    assert_eq!(errors_for(&k, w.max_obj()), [text, text]);
}

#[test]
fn panic_in_report_error_is_contained() {
    let k = median_mock::lock();
    struct Bad;
    impl Hook for Bad {
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
    assert_eq!(
        errors_for(&k, w.max_obj()),
        ["median: panic in bang: report failed"]
    );
}

#[test]
fn sel_defaults() {
    let _k = median_mock::lock();
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

/// The class `class_registers_the_methods` makes in the kernel.
static CLASS: AtomicPtr<max_sys::t_class> = AtomicPtr::new(std::ptr::null_mut());

/// The `new` method of that class: what `Wrapper::new_tramp` does, with `W` as the wrapper.
unsafe extern "C" fn new_w(
    _sym: *mut max_sys::t_symbol,
    _argc: c_long,
    _argv: *const max_sys::t_atom,
) -> *mut c_void {
    // SAFETY: the class was made by the kernel with the size of `W`, so this is zeroed memory for
    // a `W`, in which only `t` has to be written (the header is Max's)
    unsafe {
        let o = max_sys::object_alloc(CLASS.load(Ordering::SeqCst)) as *mut W;
        std::ptr::addr_of_mut!((*o).t).write(Probe::default());
        o as *mut c_void
    }
}

unsafe extern "C" fn free_w(w: *mut W) {
    // SAFETY: Max passes the object made by `new_w`, which is not used again
    unsafe { std::ptr::drop_in_place(std::ptr::addr_of_mut!((*w).t)) }
}

#[test]
fn class_registers_the_methods() {
    let k = median_mock::lock();
    let mut c: Class<W> = Class::new(c"median_test_methods", new_w, Some(free_w));
    CLASS.store(c.inner(), Ordering::SeqCst);

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
        |p: &Probe, a: f64, b: f64| p.log(format!("two {a} {b}")),
        0,
    )
    .unwrap();
    c.add_gimme(c"gimme", |p: &Probe, s: &SymbolRef, a: &[Atom]| {
        p.log(format!("gimme {} {}", name(s), a.len()))
    })
    .unwrap();
    //one function under two selectors, and the shape without the selector
    c.add_gimme(c"gimme_b", gimme_with_sel).unwrap();
    c.add_gimme(c"gimme_c", gimme_with_sel).unwrap();
    c.add_gimme(c"gimme_plain", gimme_plain).unwrap();
    c.register(crate::class::ClassType::Box).unwrap();

    let obj = k.new_object("median_test_methods", &[]);
    assert!(!obj.is_null());
    // SAFETY: `obj` is a live `W` made by `new_w`, which stays alive until `free_object`
    let w = unsafe { &*(obj as *const W) };

    //each method was registered under its selector with the argument types of its arguments
    use max_sys::e_max_atomtypes::{A_DEFSYM, A_FLOAT, A_GIMME, A_LONG, A_SYM};
    let types = |sel: &str| {
        let (fun, types) = k
            .method(obj, sel)
            .unwrap_or_else(|| panic!("no method {sel}"));
        assert_ne!(fun, 0);
        types
    };
    for (sel, t) in [
        ("bang", &[][..]),
        ("int", &[A_LONG as i32][..]),
        ("float", &[A_FLOAT as i32][..]),
        ("symbol", &[A_SYM as i32][..]),
        ("list", &[A_GIMME as i32][..]),
        ("anything", &[A_GIMME as i32][..]),
        ("set", &[A_DEFSYM as i32][..]),
        ("two", &[A_FLOAT as i32, A_FLOAT as i32][..]),
        ("gimme", &[A_GIMME as i32][..]),
        ("gimme_b", &[A_GIMME as i32][..]),
        ("gimme_plain", &[A_GIMME as i32][..]),
    ] {
        assert_eq!(types(sel), t, "{sel}");
    }

    //and the methods that were registered are the ones that work, as Max would call them
    k.send_bang(obj).unwrap();
    k.send_int(obj, 5).unwrap();
    k.send_float(obj, 0.5).unwrap();
    k.send(obj, "symbol", &["s".into()]).unwrap();
    k.send_list(obj, &[1.into(), 2.into()]).unwrap();
    k.send(obj, "other", &[1.into()]).unwrap();
    k.send(obj, "set", &["s".into()]).unwrap();
    //the default of a selector argument
    k.send(obj, "set", &[]).unwrap();
    k.send(obj, "two", &[3.into(), 4.5.into()]).unwrap();
    k.send(obj, "gimme", &[1.into(), 2.into()]).unwrap();
    k.send(obj, "gimme_b", &[1.into()]).unwrap();
    k.send(obj, "gimme_c", &[]).unwrap();
    k.send(obj, "gimme_plain", &[1.into(), 2.into(), 3.into()])
        .unwrap();
    assert_eq!(
        w.t.take(),
        [
            "bang",
            "int 5",
            "float 0.5",
            "symbol s",
            "list 2",
            "anything other 1",
            "set s",
            "set ",
            "two 3 4.5",
            "gimme gimme 2",
            "with gimme_b 1",
            "with gimme_c 0",
            "plain 3"
        ]
    );
    assert!(k.console_log().is_empty());
    assert!(k.free_object(obj));
}

#[test]
fn add_sel_with_too_many_defaults_is_an_error_not_a_panic() {
    let _k = median_mock::lock();
    // SAFETY: the class is never used, the call fails before it would be
    let mut c: Class<W> = unsafe { Class::new_registered(std::ptr::null_mut()) };
    assert!(c.add_sel(c"set", set, 2).is_err());
    assert!(c.add_sel(c"nothing", |_: &Probe| {}, 1).is_err());
}
