//! `#[notify]` and `Class::add_notify`: the `notify` method of a class, called through
//! `median-mock` as Max calls it, and what the wrapper registers when there isn't one.

use median::builder::ManagedBufferRef;
use median::class::Class;
use median::error::MaxError;
use median::prelude::*;
use median_mock::{Atom as MockAtom, Kernel, Level};
use std::ffi::{CStr, c_void};
use std::sync::{
    Mutex, Once,
    atomic::{AtomicUsize, Ordering},
};

/// A class with a `#[notify]` method that posts the message and refuses `refuse`; it also keeps
/// a `buffer~` reference, which still gets its notifications.
#[derive(MaxObject)]
#[max(name = "notify.derived")]
pub struct Derived {
    obj: ObjRef,
    _buf: ManagedBufferRef,
}

impl MaxObjWrapped for Derived {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let name = b.creation_args().first().map(|a| a.get_symbol());
        Ok(Self {
            obj: b.obj(),
            _buf: b.with_buffer(name),
        })
    }
}

#[median::methods]
impl Derived {
    #[notify]
    pub fn notified(&self, n: &Notification) -> Result<(), String> {
        self.obj.post(format!("notified {}", n.message()));
        if *n.message() == median::sym!("refuse") {
            return Err("refused".to_string());
        }
        Ok(())
    }
}

/// What `class_setup` of `Twice` got from registering `notify` twice.
static TWICE: Mutex<Vec<Result<(), String>>> = Mutex::new(Vec::new());
/// What the `notify` methods of `Twice` added up: 1 for each call of the first, 100 of the second.
static TWICE_NOTIFIED: AtomicUsize = AtomicUsize::new(0);

/// Hand-written registration: the second `add_notify` is refused, and so is `add_sel`.
#[derive(MaxObject)]
#[max(name = "notify.twice", no_methods)]
pub struct Twice;

impl MaxObjWrapped for Twice {
    fn new(_b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self)
    }

    fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
        let mut r = TWICE.lock().unwrap();
        let mut rec = |res: MaxResult<()>| r.push(res.map_err(|e| format!("{e:?}")));
        rec(c.add_notify(|_: &Self, _: &Notification| {
            TWICE_NOTIFIED.fetch_add(1, Ordering::SeqCst);
        }));
        rec(c.add_notify(|_: &Self, _: &Notification| {
            TWICE_NOTIFIED.fetch_add(100, Ordering::SeqCst);
        }));
        rec(c.add_gimme(c"notify", |_: &Self, _: &SymbolRef, _: &[Atom]| {}));
    }
}

/// No `notify` method of its own.
#[derive(MaxObject)]
#[max(name = "notify.none")]
pub struct NoNotify;

impl MaxObjWrapped for NoNotify {
    fn new(_b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self)
    }
}

#[median::methods]
impl NoNotify {
    #[bang]
    pub fn bang(&self) {}
}

#[derive(MaxObject)]
#[max(name = "notify.via_methods", no_methods)]
pub struct ViaMethods {
    obj: ObjRef,
}

impl MaxObjWrapped for ViaMethods {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self { obj: b.obj() })
    }

    fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
        c.add_notify(Self::notified).unwrap();
    }
}

impl ViaMethods {
    fn notified(&self, n: &Notification) {
        self.obj.post(format!("hand-written {}", n.message()));
    }
}

median::ext_main! { Derived, Twice, NoNotify, ViaMethods }

fn setup() -> Kernel {
    static ONCE: Once = Once::new();
    let k = median_mock::lock();
    // SAFETY: called once, as Max does
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
    k
}

type NotifyFn = extern "C" fn(
    *mut c_void,
    *mut median::max_sys::t_symbol,
    *mut median::max_sys::t_symbol,
    *mut c_void,
    *mut c_void,
);

/// The object's `notify` method, as Max calls it.
fn notify_method(k: &Kernel, obj: *mut c_void) -> NotifyFn {
    let (f, types) = k.method(obj, "notify").unwrap();
    assert_eq!(types, [median::max_sys::e_max_atomtypes::A_CANT as i32]);
    // SAFETY: `notify` is the `A_CANT` method `(x, s, msg, sender, data)`
    unsafe { std::mem::transmute::<usize, NotifyFn>(f) }
}

/// Call it with `from` as the sender and the data.
fn send(k: &Kernel, obj: *mut c_void, sender: &CStr, msg: &CStr, from: *mut c_void) {
    notify_method(k, obj)(
        obj,
        SymbolRef::from(sender).as_ptr(),
        SymbolRef::from(msg).as_ptr(),
        from,
        from,
    );
}

fn log(k: &Kernel) -> Vec<(Level, String)> {
    k.take_console_log()
        .into_iter()
        .map(|e| (e.level, e.text))
        .collect()
}

#[test]
fn a_notify_method_gets_the_notification() {
    let k = setup();
    let obj = k.new_object("notify.derived", &[]);
    assert!(!obj.is_null());
    k.take_console_log();
    send(&k, obj, c"someone", c"changed", std::ptr::null_mut());
    assert_eq!(log(&k), [(Level::Post, "notified changed".to_string())]);
    assert!(k.free_object(obj));
}

#[test]
fn an_err_from_notify_is_reported_for_the_object() {
    let k = setup();
    let obj = k.new_object("notify.derived", &[]);
    k.take_console_log();
    send(&k, obj, c"someone", c"refuse", std::ptr::null_mut());
    assert_eq!(
        log(&k),
        [
            (Level::Post, "notified refuse".to_string()),
            (Level::Error, "refused".to_string())
        ]
    );
    assert!(k.free_object(obj));
}

#[test]
fn a_notification_the_mock_sends_reaches_it() {
    let k = setup();
    let obj = k.new_object("notify.derived", &[]);
    let (dict, _) = Dict::new()
        .unwrap()
        .register(Some(median::sym!("notify.observed")))
        .unwrap();
    // SAFETY: `obj` is a live object, which stays so while the subscription does
    let subscription = unsafe {
        median::notify::Subscription::new(
            obj.cast(),
            median::sym!("dictionary"),
            median::sym!("notify.observed"),
            None,
        )
    };
    k.take_console_log();
    dict.notify_modified().unwrap();
    assert_eq!(log(&k), [(Level::Post, "notified modified".to_string())]);
    drop(subscription);
    assert!(k.free_object(obj));
}

#[test]
fn the_buffer_reference_still_gets_its_notifications() {
    let k = setup();
    let obj = k.new_object("notify.derived", &[MockAtom::Sym("nb".into())]);
    let ours = k.create_buffer("nb", 1, 4, 48000.);
    let before = k.buffer_ref_notifications().len();
    k.take_console_log();
    send(&k, obj, c"nb", c"globalsymbol_binding", ours.cast());
    let notes = k.buffer_ref_notifications();
    assert_eq!(notes.len(), before + 1);
    assert_eq!(
        (notes[before].1.as_str(), notes[before].2.as_str()),
        ("nb", "globalsymbol_binding")
    );
    //and the method got it too
    assert_eq!(
        log(&k),
        [(Level::Post, "notified globalsymbol_binding".to_string())]
    );
    assert!(k.free_object(obj));
}

#[test]
fn add_notify_twice_is_a_duplicate_and_add_sel_still_refuses_notify() {
    let k = setup();
    assert!(k.class_exists("notify.twice"));
    let r = TWICE.lock().unwrap().clone();
    let dup = Err(format!("{:?}", MaxError::Duplicate));
    assert_eq!(r, [Ok(()), dup.clone(), dup]);
    //the first one is the one registered
    let obj = k.new_object("notify.twice", &[]);
    assert!(k.method(obj, "notify").is_some());
    assert!(k.free_object(obj));
}

#[test]
fn the_wrapper_registers_its_own_notify_only_when_there_is_none() {
    let k = setup();
    for name in ["notify.none", "notify.derived", "notify.twice"] {
        let obj = k.new_object(name, &[]);
        assert!(!obj.is_null(), "{name}");
        assert!(k.method(obj, "notify").is_some(), "{name}");
        k.take_console_log();
        TWICE_NOTIFIED.store(0, Ordering::SeqCst);
        send(&k, obj, c"someone", c"anything", std::ptr::null_mut());
        match name {
            //the default one takes a notification and does nothing
            "notify.none" => assert!(log(&k).is_empty()),
            //the installed one is the user's, not the wrapper's default
            "notify.derived" => {
                assert_eq!(log(&k), [(Level::Post, "notified anything".to_string())])
            }
            //and the first of two registrations
            _ => assert_eq!(TWICE_NOTIFIED.load(Ordering::SeqCst), 1),
        }
        assert!(k.free_object(obj));
    }
}

#[test]
fn a_hand_written_add_notify_works_as_the_marker() {
    let k = setup();
    let obj = k.new_object("notify.via_methods", &[]);
    k.take_console_log();
    send(&k, obj, c"someone", c"ping", std::ptr::null_mut());
    assert_eq!(log(&k), [(Level::Post, "hand-written ping".to_string())]);
    assert!(k.free_object(obj));
}

/// Max returns a non-null placeholder from `class_method` for a method a class does not have, and
/// the mock does as well; the presence test must use the class' method table. With the old test,
/// `Derived` (a `#[notify]` class) failed to register with `Duplicate`.
#[test]
fn classes_with_and_without_notify_register_without_errors_and_have_one() {
    let k = setup();
    for name in [
        "notify.derived",
        "notify.none",
        "notify.twice",
        "notify.via_methods",
    ] {
        assert!(k.class_exists(name), "{name}: {:?}", k.console_log());
        let obj = k.new_object(name, &[]);
        assert!(!obj.is_null(), "{name}");
        assert!(k.method(obj, "notify").is_some(), "{name}");
        assert!(k.free_object(obj));
    }
    assert!(
        !k.console_log()
            .iter()
            .any(|e| e.text.contains("failed to register")),
        "{:?}",
        k.console_log()
    );
}
