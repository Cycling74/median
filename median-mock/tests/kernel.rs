//! Exercises the kernel the way a consumer does: through the C symbols max-sys declares, with a
//! hand-written C style class (an `extern "C"` new/free/method set over a `#[repr(C)]` struct with
//! a `t_object` header).
//!
//! Every test takes `median_mock::lock()`, which serialises them.

#![allow(clippy::missing_safety_doc)]
// `c_long` is `i64` here but 32 bit on Windows
#![allow(clippy::unnecessary_cast)]

use max_sys::{
    e_max_atomtypes as ty, t_atom, t_class, t_clock, t_max_err, t_object, t_pxobject, t_symbol,
};
use median_mock::{Atom, Level, Role, SendError};
use std::ffi::{c_long, c_short, c_void};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicI64, AtomicPtr, Ordering};
use std::sync::{Mutex, Once};

// ---------------------------------------------------------------------------------------------
// the class under test

#[repr(C)]
struct Thing {
    ob: t_object,
    out_left: *mut c_void,
    out_right: *mut c_void,
    proxy: *mut c_void,
    clock: *mut t_clock,
    id: i64,
    gain: f64,
    level: i64,
    /// `stuffloc` of the proxy
    stuff: c_long,
    /// what `proxy_getinlet` said in the last `int` method
    last_inlet: i64,
    /// the `stuff` seen in the last `int` method
    last_stuff: i64,
    qcalls: i64,
}

static THING_CLASS: AtomicPtr<t_class> = AtomicPtr::new(null_mut());
static DSP_CLASS: AtomicPtr<t_class> = AtomicPtr::new(null_mut());
static APPEND_CLASS: AtomicPtr<t_class> = AtomicPtr::new(null_mut());
static INLETS_CLASS: AtomicPtr<t_class> = AtomicPtr::new(null_mut());
static LEAKY_CLASS: AtomicPtr<t_class> = AtomicPtr::new(null_mut());
static FREED: AtomicI64 = AtomicI64::new(0);
/// `(thing id, logical time, on the timer thread)` for every clock callback
static TICKS: Mutex<Vec<(i64, f64, bool)>> = Mutex::new(Vec::new());
/// `(selector, atoms)` for every deferred call
static DEFERRED: Mutex<Vec<(String, Vec<Atom>)>> = Mutex::new(Vec::new());
/// `(sender name, message, data)` for every notification
static NOTES: Mutex<Vec<(String, String, usize)>> = Mutex::new(Vec::new());
static DEFER_FROM_TICK: AtomicI64 = AtomicI64::new(0);

macro_rules! meth {
    ($f:expr, $ty:ty) => {{
        #[allow(unused_unsafe)]
        unsafe {
            std::mem::transmute::<$ty, max_sys::method>($f)
        }
    }};
}

fn sym(s: &std::ffi::CStr) -> *mut t_symbol {
    unsafe { max_sys::gensym(s.as_ptr()) }
}

extern "C" fn thing_new(_s: *mut t_symbol, argc: c_long, argv: *mut t_atom) -> *mut c_void {
    unsafe {
        let x = max_sys::object_alloc(THING_CLASS.load(Ordering::SeqCst)) as *mut Thing;
        (*x).id = -1;
        if argc > 0 && max_sys::atom_gettype(argv) == ty::A_LONG as c_long {
            (*x).id = max_sys::atom_getlong(argv) as i64;
        }
        // right to left
        max_sys::intin(x as _, 2);
        (*x).proxy = max_sys::proxy_new(x as _, 1, &raw mut (*x).stuff);
        (*x).out_right = max_sys::outlet_new(x as _, null());
        (*x).out_left = max_sys::outlet_new(x as _, null());
        (*x).clock = max_sys::clock_new(x as _, meth!(thing_tick, extern "C" fn(*mut Thing)));
        x as *mut c_void
    }
}

extern "C" fn thing_free(x: *mut Thing) {
    unsafe {
        FREED.fetch_add(1, Ordering::SeqCst);
        max_sys::freeobject((*x).clock);
        max_sys::object_free((*x).proxy);
    }
}

extern "C" fn thing_bang(x: *mut Thing) {
    unsafe { max_sys::outlet_bang((*x).out_left) };
}

extern "C" fn thing_int(x: *mut Thing, n: isize) {
    unsafe {
        (*x).last_inlet = max_sys::proxy_getinlet(x as _) as i64;
        (*x).last_stuff = (*x).stuff as i64;
        max_sys::outlet_int((*x).out_left, n + 1);
    }
}

extern "C" fn thing_in2(x: *mut Thing, n: isize) {
    unsafe { max_sys::outlet_int((*x).out_right, n * 100) };
}

extern "C" fn thing_float(x: *mut Thing, f: f64) {
    unsafe { max_sys::outlet_float((*x).out_left, f * 2.0) };
}

extern "C" fn thing_list(x: *mut Thing, s: *mut t_symbol, argc: c_long, argv: *mut t_atom) {
    unsafe { max_sys::outlet_list((*x).out_left, s, argc as c_short, argv) };
}

extern "C" fn thing_anything(x: *mut Thing, s: *mut t_symbol, argc: c_long, argv: *mut t_atom) {
    unsafe { max_sys::outlet_anything((*x).out_right, s, argc as c_short, argv) };
}

/// A float, an int and a defaulted symbol: the register classes interleave.
extern "C" fn thing_mixed(x: *mut Thing, f: f64, n: isize, s: *mut t_symbol) {
    unsafe {
        let mut a: [t_atom; 3] = std::mem::zeroed();
        max_sys::atom_setfloat(&mut a[0], f);
        max_sys::atom_setlong(&mut a[1], n);
        max_sys::atom_setsym(&mut a[2], s);
        max_sys::outlet_anything((*x).out_left, sym(c"mixed"), 3, a.as_mut_ptr());
    }
}

extern "C" fn thing_sched(x: *mut Thing, ms: f64) {
    unsafe { max_sys::clock_fdelay((*x).clock, ms) };
}

extern "C" fn thing_unset(x: *mut Thing) {
    unsafe { max_sys::clock_unset((*x).clock) };
}

extern "C" fn thing_tick(x: *mut Thing) {
    unsafe {
        let mut t = 0.0;
        max_sys::clock_getftime(&mut t);
        TICKS
            .lock()
            .unwrap()
            .push(((*x).id, t, max_sys::systhread_istimerthread() != 0));
        match DEFER_FROM_TICK.load(Ordering::SeqCst) {
            1 => {
                // from the timer thread: queued
                max_sys::defer(
                    x as _,
                    meth!(
                        thing_deferred,
                        extern "C" fn(*mut Thing, *mut t_symbol, c_long, *mut t_atom)
                    ),
                    sym(c"from_timer"),
                    0,
                    null_mut(),
                );
            }
            2 => {
                // reschedule ourselves 5 ms on, three times in all
                if (*x).qcalls < 2 {
                    (*x).qcalls += 1;
                    max_sys::clock_fdelay((*x).clock, 5.0);
                }
            }
            3 if (*x).qcalls == 0 => {
                // the first time: reschedule ourselves and fire nested, as a modal dialog's
                // event loop would with Overdrive off
                (*x).qcalls += 1;
                max_sys::clock_fdelay((*x).clock, 1.0);
                assert_eq!(median_mock::Kernel::advance_nested(1.0), 1);
                TICKS.lock().unwrap().push(((*x).id, -1.0, true));
            }
            _ => {}
        }
    }
}

extern "C" fn thing_deferred(_x: *mut Thing, s: *mut t_symbol, argc: c_long, argv: *mut t_atom) {
    let atoms = unsafe { median_mock::atoms::atoms_from_raw(argc as usize, argv) };
    let name = unsafe { median_mock::symbols::sym_name(s) };
    DEFERRED.lock().unwrap().push((name, atoms));
}

extern "C" fn thing_loadbang(x: *mut Thing) {
    unsafe { (*x).qcalls += 100 };
}

extern "C" fn thing_qelem(x: *mut Thing) {
    unsafe { (*x).qcalls += 1 };
}

/// The qelem `thing_qelem_nested` and `thing_qelem_self` set again.
static NESTED_QELEM: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

/// The first time: queue a deferred call, set this qelem again (which does nothing: it is unset
/// only after this returns), and run the main queue nested (a modal dialog's event loop), which
/// runs the deferred call inside this call, and not this qelem.
extern "C" fn thing_qelem_nested(x: *mut Thing) {
    unsafe {
        (*x).qcalls += 1;
        if (*x).qcalls == 1 {
            let f = meth!(
                thing_deferred,
                extern "C" fn(*mut Thing, *mut t_symbol, c_long, *mut t_atom)
            );
            max_sys::defer_low(x as _, f, sym(c"nested"), 0, null_mut());
            max_sys::qelem_set(NESTED_QELEM.load(Ordering::SeqCst) as _);
            assert_eq!(median_mock::Kernel::run_main_queue_nested(), 1);
            // the deferred call ran before this returns; this qelem didn't run again
            assert_eq!((*x).qcalls, 1);
            assert_eq!(DEFERRED.lock().unwrap().len(), 1);
        }
    }
}

/// Sets its own qelem, at the back and at the front, every time it runs.
extern "C" fn thing_qelem_self(x: *mut Thing) {
    unsafe {
        (*x).qcalls += 1;
        let q = NESTED_QELEM.load(Ordering::SeqCst) as *mut max_sys::t_qelem;
        max_sys::qelem_set(q);
        max_sys::qelem_front(q);
    }
}

extern "C" fn thing_notify(
    _x: *mut Thing,
    s: *mut t_symbol,
    msg: *mut t_symbol,
    _sender: *mut c_void,
    data: *mut c_void,
) {
    unsafe {
        NOTES.lock().unwrap().push((
            median_mock::symbols::sym_name(s),
            median_mock::symbols::sym_name(msg),
            data as usize,
        ));
    }
}

unsafe extern "C" fn level_get(
    x: *mut Thing,
    _attr: *mut t_object,
    argc: *mut c_long,
    argv: *mut *mut t_atom,
) -> t_max_err {
    unsafe {
        // as the SDK's accessors do (attrtester.c)
        if argc.is_null() || argv.is_null() {
            return 0;
        }
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
    unsafe {
        if argc > 0 && !argv.is_null() {
            (*x).level = max_sys::atom_getlong(argv) as i64;
        }
    }
    0
}

#[repr(C)]
struct DspThing {
    px: t_pxobject,
}

extern "C" fn dsp_new(_s: *mut t_symbol, _argc: c_long, _argv: *mut t_atom) -> *mut c_void {
    unsafe {
        let x = max_sys::object_alloc(DSP_CLASS.load(Ordering::SeqCst)) as *mut DspThing;
        max_sys::z_dsp_setup(&raw mut (*x).px, 2);
        max_sys::outlet_new(x as _, c"signal".as_ptr());
        x as *mut c_void
    }
}

extern "C" fn dsp_free(x: *mut DspThing) {
    unsafe { max_sys::z_dsp_free(&raw mut (*x).px) };
}

/// An MSP object with two signal inlets and two more made after `z_dsp_setup`, right to left:
/// `floatin(x, 1)` (rightmost), then a proxy with id 2.
#[repr(C)]
struct InletsThing {
    px: t_pxobject,
    proxy: *mut c_void,
}

extern "C" fn inlets_new(_s: *mut t_symbol, _argc: c_long, _argv: *mut t_atom) -> *mut c_void {
    unsafe {
        let x = max_sys::object_alloc(INLETS_CLASS.load(Ordering::SeqCst)) as *mut InletsThing;
        max_sys::z_dsp_setup(&raw mut (*x).px, 2);
        max_sys::floatin(x as _, 1);
        (*x).proxy = max_sys::proxy_new(x as _, 2, null_mut());
        x as *mut c_void
    }
}

extern "C" fn inlets_free(x: *mut InletsThing) {
    unsafe {
        max_sys::z_dsp_free(&raw mut (*x).px);
        max_sys::object_free((*x).proxy);
    }
}

/// Breaks the rules: makes a proxy and sets up dsp, and has no free method.
extern "C" fn leaky_new(_s: *mut t_symbol, _argc: c_long, _argv: *mut t_atom) -> *mut c_void {
    unsafe {
        let x = max_sys::object_alloc(LEAKY_CLASS.load(Ordering::SeqCst)) as *mut InletsThing;
        max_sys::z_dsp_setup(&raw mut (*x).px, 1);
        (*x).proxy = max_sys::proxy_new(x as _, 1, null_mut());
        x as *mut c_void
    }
}

extern "C" fn thing_assist(_x: *mut Thing, _b: *mut c_void, io: c_long, index: c_long, s: *mut i8) {
    let text = if index == 9 {
        // longer than the buffer would be wrong; this is what an object may write: 500 bytes
        "x".repeat(500)
    } else {
        format!("{} {index}", if io == 1 { "in" } else { "out" })
    };
    let text = std::ffi::CString::new(text).unwrap();
    unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), s, text.as_bytes_with_nul().len()) };
}

extern "C" fn thing_inletinfo(_x: *mut Thing, _b: *mut c_void, index: c_long, t: *mut i8) {
    if index != 0 {
        unsafe { *t = 1 };
    }
}

static PERFORM_ROLE_AUDIO: AtomicI64 = AtomicI64::new(0);

unsafe extern "C" fn dsp_perform(
    _x: *mut t_object,
    _dsp64: *mut t_object,
    ins: *mut *mut f64,
    numins: c_long,
    outs: *mut *mut f64,
    numouts: c_long,
    frames: c_long,
    _flags: c_long,
    userparam: *mut c_void,
) {
    unsafe {
        assert_eq!((numins, numouts), (2, 1));
        PERFORM_ROLE_AUDIO.store(max_sys::systhread_isaudiothread() as i64, Ordering::SeqCst);
        for i in 0..frames as usize {
            *(*outs).add(i) = *(*ins).add(i) + *(*ins.add(1)).add(i) + userparam as usize as f64;
        }
    }
}

extern "C" fn dsp_dsp64(
    x: *mut DspThing,
    dsp64: *mut t_object,
    _count: *mut c_short,
    sample_rate: f64,
    max_vector: c_long,
    _flags: c_long,
) {
    assert_eq!((sample_rate, max_vector), (48000.0, 64));
    unsafe {
        max_sys::dsp_add64(dsp64, x as _, Some(dsp_perform), 0, 3 as *mut c_void);
    }
}

/// Makes its outlets with `outlet_append`, as median does: `int`, `list`, anything, left to right,
/// then one with `outlet_new`, which goes to the left of those.
#[repr(C)]
struct AppendThing {
    ob: t_object,
    outs: [*mut c_void; 4],
}

extern "C" fn append_new(_s: *mut t_symbol, _argc: c_long, _argv: *mut t_atom) -> *mut c_void {
    unsafe {
        let x = max_sys::object_alloc(APPEND_CLASS.load(Ordering::SeqCst)) as *mut AppendThing;
        let o = x as *mut t_object;
        (*x).outs[0] = max_sys::outlet_append(o, null_mut(), sym(c"int"));
        (*x).outs[1] = max_sys::outlet_append(o, null_mut(), sym(c"list"));
        (*x).outs[2] = max_sys::outlet_append(o, null_mut(), null_mut());
        (*x).outs[3] = max_sys::outlet_new(x as _, c"float".as_ptr());
        x as *mut c_void
    }
}

fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let a = max_sys::class_new(
            c"append.thing".as_ptr(),
            meth!(
                append_new,
                extern "C" fn(*mut t_symbol, c_long, *mut t_atom) -> *mut c_void
            ),
            None,
            size_of::<AppendThing>() as c_long,
            None,
            ty::A_GIMME as c_short,
            0,
        );
        assert_eq!(max_sys::class_register(sym(c"box"), a), 0);
        APPEND_CLASS.store(a, Ordering::SeqCst);

        let c = max_sys::class_new(
            c"thing".as_ptr(),
            meth!(
                thing_new,
                extern "C" fn(*mut t_symbol, c_long, *mut t_atom) -> *mut c_void
            ),
            meth!(thing_free, extern "C" fn(*mut Thing)),
            size_of::<Thing>() as c_long,
            None,
            ty::A_GIMME as c_short,
            0,
        );
        assert!(!c.is_null());
        let add = |name: &std::ffi::CStr, m: max_sys::method, types: &[i32]| match types {
            [] => max_sys::class_addmethod(c, m, name.as_ptr(), 0),
            [a] => max_sys::class_addmethod(c, m, name.as_ptr(), *a, 0),
            [a, b] => max_sys::class_addmethod(c, m, name.as_ptr(), *a, *b, 0),
            [a, b, d] => max_sys::class_addmethod(c, m, name.as_ptr(), *a, *b, *d, 0),
            _ => unreachable!(),
        };
        let (l, f, s, g, ct) = (
            ty::A_LONG as i32,
            ty::A_FLOAT as i32,
            ty::A_DEFSYM as i32,
            ty::A_GIMME as i32,
            ty::A_CANT as i32,
        );
        assert_eq!(
            add(c"bang", meth!(thing_bang, extern "C" fn(*mut Thing)), &[]),
            0
        );
        add(
            c"int",
            meth!(thing_int, extern "C" fn(*mut Thing, isize)),
            &[l],
        );
        add(
            c"in2",
            meth!(thing_in2, extern "C" fn(*mut Thing, isize)),
            &[l],
        );
        add(
            c"float",
            meth!(thing_float, extern "C" fn(*mut Thing, f64)),
            &[f],
        );
        add(
            c"list",
            meth!(
                thing_list,
                extern "C" fn(*mut Thing, *mut t_symbol, c_long, *mut t_atom)
            ),
            &[g],
        );
        add(
            c"anything",
            meth!(
                thing_anything,
                extern "C" fn(*mut Thing, *mut t_symbol, c_long, *mut t_atom)
            ),
            &[g],
        );
        add(
            c"mixed",
            meth!(
                thing_mixed,
                extern "C" fn(*mut Thing, f64, isize, *mut t_symbol)
            ),
            &[f, l, s],
        );
        add(
            c"sched",
            meth!(thing_sched, extern "C" fn(*mut Thing, f64)),
            &[f],
        );
        add(c"unset", meth!(thing_unset, extern "C" fn(*mut Thing)), &[]);
        add(
            c"deferred",
            meth!(
                thing_deferred,
                extern "C" fn(*mut Thing, *mut t_symbol, c_long, *mut t_atom)
            ),
            &[g],
        );
        add(
            c"notify",
            meth!(
                thing_notify,
                extern "C" fn(*mut Thing, *mut t_symbol, *mut t_symbol, *mut c_void, *mut c_void)
            ),
            &[ct],
        );
        add(
            c"assist",
            meth!(
                thing_assist,
                extern "C" fn(*mut Thing, *mut c_void, c_long, c_long, *mut i8)
            ),
            &[ct],
        );
        add(
            c"inletinfo",
            meth!(
                thing_inletinfo,
                extern "C" fn(*mut Thing, *mut c_void, c_long, *mut i8)
            ),
            &[ct],
        );

        // attributes: a float64 member with clip, and a long with accessors
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
        // attributes of attributes, the three ways
        assert_eq!(
            max_sys::class_attr_addattr_parse(
                c,
                c"level".as_ptr(),
                c"enumvals".as_ptr(),
                sym(c"atom"),
                0,
                c"zero one \"two three\" 4 5.5 -6".as_ptr()
            ),
            0
        );
        let mut label: t_atom = std::mem::zeroed();
        max_sys::atom_setsym(&mut label, max_sys::gensym_tr(c"The Level".as_ptr()));
        assert_eq!(
            max_sys::class_attr_addattr_atoms(
                c,
                c"level".as_ptr(),
                c"label".as_ptr(),
                sym(c"symbol"),
                0,
                1,
                &mut label
            ),
            0
        );
        assert_eq!(
            max_sys::object_addattr_parse(gain, c"save".as_ptr(), sym(c"long"), 0, c"1".as_ptr()),
            0
        );
        // replaced by a second add
        max_sys::object_addattr_parse(gain, c"order".as_ptr(), sym(c"long"), 0, c"1".as_ptr());
        max_sys::object_addattr_parse(gain, c"order".as_ptr(), sym(c"long"), 0, c"2".as_ptr());
        add(
            c"loadbang",
            meth!(thing_loadbang, extern "C" fn(*mut Thing)),
            &[ct],
        );
        assert_eq!(max_sys::class_register(sym(c"box"), c), 0);
        THING_CLASS.store(c, Ordering::SeqCst);

        // an MSP class
        let d = max_sys::class_new(
            c"dsp.thing".as_ptr(),
            meth!(
                dsp_new,
                extern "C" fn(*mut t_symbol, c_long, *mut t_atom) -> *mut c_void
            ),
            meth!(dsp_free, extern "C" fn(*mut DspThing)),
            size_of::<DspThing>() as c_long,
            None,
            ty::A_GIMME as c_short,
            0,
        );
        max_sys::class_addmethod(
            d,
            meth!(
                dsp_dsp64,
                extern "C" fn(*mut DspThing, *mut t_object, *mut c_short, f64, c_long, c_long)
            ),
            c"dsp64".as_ptr(),
            ty::A_CANT as i32,
            0,
        );
        max_sys::class_dspinit(d);
        assert_eq!(max_sys::class_register(sym(c"box"), d), 0);
        DSP_CLASS.store(d, Ordering::SeqCst);

        for (name, new, free, store) in [
            (
                c"inlets.thing",
                meth!(
                    inlets_new,
                    extern "C" fn(*mut t_symbol, c_long, *mut t_atom) -> *mut c_void
                ),
                meth!(inlets_free, extern "C" fn(*mut InletsThing)),
                &INLETS_CLASS,
            ),
            (
                c"leaky.thing",
                meth!(
                    leaky_new,
                    extern "C" fn(*mut t_symbol, c_long, *mut t_atom) -> *mut c_void
                ),
                None,
                &LEAKY_CLASS,
            ),
        ] {
            let c = max_sys::class_new(
                name.as_ptr(),
                new,
                free,
                size_of::<InletsThing>() as c_long,
                None,
                ty::A_GIMME as c_short,
                0,
            );
            max_sys::class_dspinit(c);
            assert_eq!(max_sys::class_register(sym(c"box"), c), 0);
            store.store(c, Ordering::SeqCst);
        }
    });
}

fn thing(k: &median_mock::Kernel, id: i64) -> *mut Thing {
    let o = k.new_object("thing", &[Atom::Long(id)]);
    assert!(!o.is_null());
    o as *mut Thing
}

// ---------------------------------------------------------------------------------------------
// symbols

#[test]
fn gensym_interns_and_is_thread_safe() {
    let _k = median_mock::lock();
    let a = unsafe { max_sys::gensym(c"interned".as_ptr()) };
    let b = unsafe { max_sys::gensym(c"interned".as_ptr()) };
    let c = unsafe { max_sys::gensym(c"other".as_ptr()) };
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(unsafe { median_mock::symbols::sym_name(a) }, "interned");
    assert_eq!(unsafe { max_sys::gensym(null()) }, unsafe {
        max_sys::gensym(c"".as_ptr())
    });
    // survives a reset
    _k.reset();
    assert_eq!(a, unsafe { max_sys::gensym(c"interned".as_ptr()) });

    let main = a as usize;
    let seen: Vec<usize> = (0..8)
        .map(|_| {
            std::thread::spawn(move || {
                (0..200)
                    .map(
                        |i| unsafe { max_sys::gensym(format!("t{}\0", i % 7).as_ptr() as _) }
                            as usize,
                    )
                    .chain([unsafe { max_sys::gensym(c"interned".as_ptr()) } as usize])
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    assert!(seen.iter().filter(|&&p| p == main).count() >= 8);
    // 7 distinct t-symbols across all threads
    let mut distinct: Vec<usize> = seen.into_iter().filter(|&p| p != main).collect();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 7);
}

#[test]
fn common_symbols_table() {
    let _k = median_mock::lock();
    let t = unsafe { &*max_sys::common_symbols_gettable() };
    assert_eq!(t.version, 500);
    unsafe {
        assert_eq!(t.s_bang, max_sys::gensym(c"bang".as_ptr()));
        assert_eq!(t.s_long, max_sys::gensym(c"long".as_ptr()));
        assert_eq!(t.s_float, max_sys::gensym(c"float".as_ptr()));
        assert_eq!(t.s_list, max_sys::gensym(c"list".as_ptr()));
        assert_eq!(t.s_signal, max_sys::gensym(c"signal".as_ptr()));
        assert_eq!(t.s_nothing, max_sys::gensym(c"".as_ptr()));
        assert_eq!(t.s__preset, max_sys::gensym(c"_preset".as_ptr()));
    }
}

// ---------------------------------------------------------------------------------------------
// classes, objects, dispatch

#[test]
fn class_is_registered_and_object_header_points_into_it() {
    let k = median_mock::lock();
    register();
    unsafe {
        let found = max_sys::class_findbyname(sym(c"box"), sym(c"thing"));
        assert_eq!(found, THING_CLASS.load(Ordering::SeqCst));
        assert!(max_sys::class_findbyname(sym(c"nobox"), sym(c"thing")).is_null());
        assert!(max_sys::class_findbyname(sym(c"box"), sym(c"nonexistent")).is_null());
    }
    assert!(k.class_exists("thing"));
    let x = thing(&k, 7);
    unsafe {
        let ob = x as *mut t_object;
        let ml = (*ob).o_messlist;
        assert!(!ml.is_null());
        // Max keeps the class at index -1 of the messlist
        assert_eq!(
            (*ml.sub(1)).m_sym as *mut t_class,
            THING_CLASS.load(Ordering::SeqCst)
        );
        assert_eq!((*x).id, 7);
        assert_eq!((*x).level, 0, "object memory is zeroed");
    }
    assert_eq!(k.class_name_of(x as _).as_deref(), Some("thing"));
    assert_eq!(k.live_object_count(), 1);
    assert!(k.free_object(x as _));
    assert_eq!(k.live_object_count(), 0);
}

#[test]
fn dispatch_bang_int_float_list_anything_and_outlets_are_recorded() {
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1) as *mut c_void;

    k.send_bang(x).unwrap();
    k.send_int(x, 41).unwrap();
    k.send_float(x, 1.25).unwrap();
    k.send_list(x, &[Atom::Long(1), Atom::Float(2.5), "three".into()])
        .unwrap();
    k.send(x, "unknownmsg", &[Atom::Long(9)]).unwrap(); // -> anything
    // typed with defaults and mixed register classes
    k.send(x, "mixed", &[Atom::Long(3), Atom::Float(4.9)])
        .unwrap();

    let log = k.outlet_log();
    let seen: Vec<(usize, &str, &[Atom])> = log
        .iter()
        .map(|e| (e.index, e.selector.as_str(), e.atoms.as_slice()))
        .collect();
    assert_eq!(
        seen,
        vec![
            (0, "bang", &[][..]),
            (0, "int", &[Atom::Long(42)][..]),
            (0, "float", &[Atom::Float(2.5)][..]),
            (
                0,
                "list",
                &[Atom::Long(1), Atom::Float(2.5), Atom::Sym("three".into())][..]
            ),
            (1, "unknownmsg", &[Atom::Long(9)][..]),
            // an int atom where a float is wanted converts; a float atom for a long truncates
            (
                0,
                "mixed",
                &[Atom::Float(3.0), Atom::Long(4), Atom::Sym("".into())][..]
            ),
        ]
    );
    assert!(
        log.iter()
            .all(|e| e.owner == x as usize && e.role == Role::Main)
    );
    // left outlet and right outlet are different pointers, in creation order right then left
    assert_ne!(log[0].outlet, log[4].outlet);
    assert_eq!(k.outlet_count(x), 2);
    assert_eq!(k.take_outlet_log().len(), 6);
    assert!(k.outlet_log().is_empty());
}

#[test]
fn outlet_calls_return_non_null_unless_a_stack_overflow_is_simulated() {
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1);
    unsafe {
        let out = (*x).out_left;
        assert!(!max_sys::outlet_bang(out).is_null());
        assert!(!max_sys::outlet_int(out, 1).is_null());
        assert!(!max_sys::outlet_float(out, 1.0).is_null());
        assert!(!max_sys::outlet_anything(out, sym(c"foo"), 0, null_mut()).is_null());
        assert_eq!(k.take_outlet_log().len(), 4);
        k.set_outlet_overflow(true);
        assert!(max_sys::outlet_bang(out).is_null());
        assert!(max_sys::outlet_list(out, null_mut(), 0, null_mut()).is_null());
        assert!(k.take_outlet_log().is_empty());
        k.set_outlet_overflow(false);
        assert!(!max_sys::outlet_bang(out).is_null());
        // a reset turns it off too
        k.set_outlet_overflow(true);
        k.reset();
        let x = thing(&k, 2);
        assert!(!max_sys::outlet_bang((*x).out_left).is_null());
    }
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn outlet_append_adds_to_the_right_and_outlet_new_to_the_left() {
    let k = median_mock::lock();
    register();
    let x = k.new_object("append.thing", &[]);
    assert!(!x.is_null());
    // left to right: the `outlet_new` one, then the three appended in the order they were made
    assert_eq!(
        k.outlet_types(x),
        [
            Some("float".to_string()),
            Some("int".to_string()),
            Some("list".to_string()),
            None
        ]
    );
    unsafe {
        let outs = (*(x as *mut AppendThing)).outs;
        max_sys::outlet_float(outs[3], 1.0);
        max_sys::outlet_int(outs[0], 2);
        max_sys::outlet_int(outs[1], 3);
        max_sys::outlet_anything(outs[2], sym(c"foo"), 0, null_mut());
    }
    let seen: Vec<_> = k
        .outlet_log()
        .iter()
        .map(|e| (e.index, e.selector.clone()))
        .collect();
    assert_eq!(
        seen,
        [
            (0, "float".to_string()),
            (1, "int".to_string()),
            (2, "int".to_string()),
            (3, "foo".to_string())
        ]
    );
    assert!(k.free_object(x));
}

#[test]
fn dispatch_fallbacks_and_errors() {
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1) as *mut c_void;

    // "int" with a symbol argument is rejected and reported
    let err = k.send(x, "int", &[Atom::Sym("nope".into())]).unwrap_err();
    assert!(
        matches!(err, SendError::BadArgs { ref selector, .. } if selector == "int"),
        "{err:?}"
    );
    // a missing required argument
    assert!(matches!(
        k.send(x, "int", &[]),
        Err(SendError::BadArgs { .. })
    ));
    // A_CANT methods cannot be sent to
    assert_eq!(
        k.send(x, "notify", &[]),
        Err(SendError::NotSendable("notify".into()))
    );
    // a one element list goes to int, as Max does when there is no list method... here there is
    // a list method, so it is used
    k.send(x, "list", &[Atom::Long(1)]).unwrap();
    assert_eq!(k.take_outlet_log()[0].selector, "list");
    let console = k.console_log();
    assert_eq!(
        console.iter().filter(|c| c.level == Level::Error).count(),
        2,
        "{console:?}"
    );
    assert!(console[0].text.contains("bad arguments for message"));
    assert_eq!(console[0].object, x as usize);

    // an unknown object is refused without being dereferenced
    assert_eq!(
        k.send(0x1000 as *mut c_void, "bang", &[]),
        Err(SendError::NotLive)
    );
    assert!(k.new_object("no such class", &[]).is_null());
    assert!(matches!(
        k.try_new_object("no such class", &[]),
        Err(SendError::NoClass(_))
    ));
}

#[test]
fn object_free_runs_the_free_method_and_releases_everything() {
    let k = median_mock::lock();
    register();
    let before = FREED.load(Ordering::SeqCst);
    let x = thing(&k, 1) as *mut c_void;
    assert_eq!(k.outlet_count(x), 2);
    assert_eq!(k.extra_inlet_count(x), 2);
    unsafe {
        let outlet = (*(x as *mut Thing)).out_left;
        assert!(k.free_object(x));
        assert_eq!(FREED.load(Ordering::SeqCst), before + 1);
        // the object is gone: messages are refused, its outlets are dead
        assert_eq!(k.send_bang(x), Err(SendError::NotLive));
        max_sys::outlet_bang(outlet);
    }
    let console = k.console_log();
    assert!(
        console.iter().any(|c| c.text.contains("not a live outlet")),
        "{console:?}"
    );
    // freeing twice is reported, not crashed on
    k.take_console_log();
    assert!(!k.free_object(x));
    assert!(k.console_log()[0].text.contains("not a live object"));
}

// ---------------------------------------------------------------------------------------------
// inlets

#[test]
fn proxy_and_intin_inlets() {
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1);
    let o = x as *mut c_void;
    // created right to left: intin(2) is inlet 2, the proxy (id 1) is inlet 1
    k.send_inlet(o, 2, "int", &[Atom::Long(3)]).unwrap();
    assert_eq!(k.take_outlet_log()[0].atoms, [Atom::Long(300)]);
    k.send_inlet(o, 1, "int", &[Atom::Long(5)]).unwrap();
    unsafe {
        assert_eq!(((*x).last_inlet, (*x).last_stuff), (1, 1));
        assert_eq!((*x).stuff, 0, "the proxy resets stuffloc after the message");
    }
    k.send_int(o, 5).unwrap();
    unsafe {
        assert_eq!(((*x).last_inlet, (*x).last_stuff), (0, 0));
    }
    assert_eq!(
        k.send_inlet(o, 3, "int", &[Atom::Long(1)]),
        Err(SendError::NoInlet(3))
    );
}

#[test]
fn dsp_setup_signal_inlets_are_ordinary_inserts() {
    use median_mock::InletKind;
    let k = median_mock::lock();
    register();
    let o = k.new_object("inlets.thing", &[]);
    assert_eq!(k.signal_inlets(o), Some(2));
    assert_eq!(
        k.inlets_created(o),
        [
            InletKind::Signal,
            InletKind::Float(1),
            InletKind::Proxy { id: 2, stuffloc: 0 }
        ]
    );
    // each inlet is inserted just right of the leftmost, `z_dsp_setup`'s too: the class calls it
    // first, so its signal inlet ends up rightmost
    assert_eq!(
        k.inlets(o),
        [
            InletKind::Proxy { id: 2, stuffloc: 0 },
            InletKind::Float(1),
            InletKind::Signal
        ]
    );
    assert!(k.free_object(o));
    assert_eq!(k.dsp_frees(), [o as usize]);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn an_unfreed_proxy_and_a_missing_dsp_free_are_reported() {
    let k = median_mock::lock();
    register();
    let o = k.new_object("leaky.thing", &[]);
    assert!(k.free_object(o));
    let errors: Vec<_> = k.console_log().into_iter().map(|e| e.text).collect();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].contains("without z_dsp_free"), "{errors:?}");
    assert!(
        errors[1].contains("was not freed before its owner"),
        "{errors:?}"
    );
    assert!(k.dsp_frees().is_empty());
    // z_dsp_free without z_dsp_setup is reported too
    let x = thing(&k, 1);
    k.take_console_log();
    unsafe { max_sys::z_dsp_free(x as *mut t_pxobject) };
    assert!(k.console_log()[0].text.contains("without z_dsp_setup"));
    assert_eq!(k.dsp_frees(), [x as usize]);
}

#[test]
fn assist_and_inletinfo_are_called_as_max_does() {
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1) as *mut c_void;
    assert_eq!(k.assist(x, false, 1).as_deref(), Some("in 1"));
    assert_eq!(k.assist(x, true, 0).as_deref(), Some("out 0"));
    assert_eq!(k.assist(x, false, 9).unwrap().len(), 500);
    assert_eq!(k.inlet_is_hot(x, 0), Some(true));
    assert_eq!(k.inlet_is_hot(x, 1), Some(false));
    // a class without the methods
    let d = k.new_object("dsp.thing", &[]);
    assert_eq!(k.assist(d, false, 0), None);
    assert_eq!(k.inlet_is_hot(d, 0), None);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn method_attributes_through_the_methods_hashtab() {
    let k = median_mock::lock();
    register();
    let c = THING_CLASS.load(Ordering::SeqCst);
    let x = thing(&k, 1) as *mut c_void;
    unsafe {
        // what CLASS_METHOD_ATTR_PARSE(c, "in2", "undocumented", gensym("long"), 0, "1") does
        assert!(max_sys::class_extra_lookup(c, sym(c"other")).is_null());
        let methods = max_sys::class_extra_lookup(c, sym(c"methods")) as *mut max_sys::t_hashtab;
        assert!(!methods.is_null());
        assert_eq!(
            max_sys::class_extra_lookup(c, sym(c"methods")),
            methods as *mut c_void
        );
        let mut m: *mut t_object = null_mut();
        assert_eq!(max_sys::hashtab_lookup(methods, sym(c"in2"), &mut m), 0);
        assert!(!m.is_null());
        let mut again: *mut t_object = null_mut();
        max_sys::hashtab_lookup(methods, sym(c"in2"), &mut again);
        assert_eq!(m, again);
        assert_eq!(
            max_sys::object_addattr_parse(
                m,
                c"undocumented".as_ptr(),
                sym(c"long"),
                0,
                c"1".as_ptr()
            ),
            0
        );
        // no such method: null and an error code
        let mut none: *mut t_object = std::ptr::dangling_mut();
        assert_ne!(
            max_sys::hashtab_lookup(methods, sym(c"nonesuch"), &mut none),
            0
        );
        assert!(none.is_null());
    }
    assert_eq!(
        k.method_attr(x, "in2", "undocumented"),
        Some(("long".to_string(), "1".to_string()))
    );
    assert_eq!(k.method_attr(x, "int", "undocumented"), None);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

/// A method that does nothing, for `class_addmethod`.
extern "C" fn nothing(_: *mut c_void) {}

#[test]
fn class_method_of_an_absent_method_is_the_non_null_method_false() {
    let _k = median_mock::lock();
    register();
    let fresh = unsafe {
        max_sys::class_new(
            c"methodless.thing".as_ptr(),
            None,
            None,
            size_of::<t_object>() as c_long,
            None,
            ty::A_GIMME as c_short,
            0,
        )
    };
    let dsp = DSP_CLASS.load(Ordering::SeqCst);
    // a class that has registered no method yet, one that has, and an MSP class
    for (what, c) in [
        ("fresh", fresh),
        ("thing", THING_CLASS.load(Ordering::SeqCst)),
        ("dsp", dsp),
    ] {
        unsafe {
            // like `object_getmethod`, `class_method` gives `method_false()` for a missing method
            let m = max_sys::class_method(c, sym(c"nonesuch")).expect(what);
            assert_eq!(m(null_mut()) as usize, 0, "{what}");
            assert!(
                m as usize == median_mock::class::method_false as *const () as usize,
                "{what}"
            );
            // the methods hashtab is what tells
            let methods =
                max_sys::class_extra_lookup(c, sym(c"methods")) as *mut max_sys::t_hashtab;
            assert!(!methods.is_null(), "{what}");
            let mut found: *mut t_object = null_mut();
            assert_ne!(
                max_sys::hashtab_lookup(methods, sym(c"nonesuch"), &mut found),
                0,
                "{what}"
            );
            assert!(found.is_null(), "{what}");
        }
    }
    // after `class_addmethod` the table has it, and `class_method` is the method itself
    unsafe {
        max_sys::class_addmethod(
            fresh,
            Some(std::mem::transmute::<
                extern "C" fn(*mut c_void),
                unsafe extern "C" fn(*mut c_void) -> *mut c_void,
            >(nothing)),
            c"notify".as_ptr(),
            ty::A_CANT as i32,
            0,
        );
        let methods =
            max_sys::class_extra_lookup(fresh, sym(c"methods")) as *mut max_sys::t_hashtab;
        let mut found: *mut t_object = null_mut();
        assert_eq!(
            max_sys::hashtab_lookup(methods, sym(c"notify"), &mut found),
            0
        );
        assert!(!found.is_null());
        let m = max_sys::class_method(fresh, sym(c"notify")).unwrap();
        assert!(m as usize != median_mock::class::method_false as *const () as usize);
    }
}

// ---------------------------------------------------------------------------------------------
// clocks

#[test]
fn live_clocks_are_counted() {
    let k = median_mock::lock();
    register();
    let before = k.live_clock_count();
    let x = thing(&k, 1);
    assert_eq!(k.live_clock_count(), before + 1);
    assert!(k.free_object(x as *mut c_void));
    assert_eq!(k.live_clock_count(), before);
}

#[test]
fn live_buffer_refs_are_counted() {
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1);
    let before = k.live_buffer_ref_count();
    let r = unsafe { max_sys::buffer_ref_new(x as *mut t_object, sym(c"buf")) };
    assert_eq!(k.live_buffer_ref_count(), before + 1);
    unsafe { max_sys::object_free(r as *mut c_void) };
    assert_eq!(k.live_buffer_ref_count(), before);
    assert!(k.free_object(x as *mut c_void));
}

#[test]
fn clocks_fire_in_time_order_on_advance() {
    let k = median_mock::lock();
    register();
    TICKS.lock().unwrap().clear();
    let a = thing(&k, 1) as *mut c_void;
    let b = thing(&k, 2) as *mut c_void;
    let c = thing(&k, 3) as *mut c_void;
    let d = thing(&k, 4) as *mut c_void;
    k.send_float(a, 0.0).unwrap(); // not a schedule, just checks the float method exists
    k.send(a, "sched", &[Atom::Float(30.0)]).unwrap();
    k.send(b, "sched", &[Atom::Float(10.0)]).unwrap();
    k.send(c, "sched", &[Atom::Float(20.0)]).unwrap();
    k.send(d, "sched", &[Atom::Float(10.0)]).unwrap(); // ties with b: scheduled later, fires later
    // rescheduling replaces
    k.send(c, "sched", &[Atom::Float(25.0)]).unwrap();
    // unset cancels
    k.send(a, "unset", &[]).unwrap();
    k.send(a, "sched", &[Atom::Float(40.0)]).unwrap();

    assert_eq!(k.advance(9.0), 0);
    assert_eq!(k.time(), 9.0);
    assert_eq!(k.advance(1.0), 2);
    assert_eq!(k.advance(100.0), 2);
    assert_eq!(k.time(), 110.0);
    let ticks = TICKS.lock().unwrap().clone();
    assert_eq!(
        ticks,
        vec![
            (2, 10.0, true),
            (4, 10.0, true),
            (3, 25.0, true),
            (1, 40.0, true)
        ]
    );
    // nothing is left
    assert_eq!(k.advance(1000.0), 0);
}

#[test]
fn a_clock_that_reschedules_itself_fires_repeatedly_within_one_advance() {
    let k = median_mock::lock();
    register();
    TICKS.lock().unwrap().clear();
    DEFER_FROM_TICK.store(2, Ordering::SeqCst);
    let a = thing(&k, 1) as *mut c_void;
    k.send(a, "sched", &[Atom::Float(5.0)]).unwrap();
    assert_eq!(k.advance(100.0), 3);
    DEFER_FROM_TICK.store(0, Ordering::SeqCst);
    let times: Vec<f64> = TICKS.lock().unwrap().iter().map(|t| t.1).collect();
    assert_eq!(times, [5.0, 10.0, 15.0]);
}

#[test]
fn a_freed_clock_does_not_fire_and_reset_cancels_everything() {
    let k = median_mock::lock();
    register();
    TICKS.lock().unwrap().clear();
    let a = thing(&k, 1) as *mut c_void;
    k.send(a, "sched", &[Atom::Float(5.0)]).unwrap();
    assert!(k.free_object(a));
    assert_eq!(k.advance(10.0), 0);
    let b = thing(&k, 2) as *mut c_void;
    k.send(b, "sched", &[Atom::Float(5.0)]).unwrap();
    k.reset();
    assert_eq!(k.time(), 0.0);
    assert_eq!(k.advance(10.0), 0);
    assert!(TICKS.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------------------------------------
// defer and qelem

#[test]
fn defer_and_qelem_run_from_the_main_queue() {
    let k = median_mock::lock();
    register();
    DEFERRED.lock().unwrap().clear();
    let x = thing(&k, 1);
    let o = x as *mut c_void;
    let f = meth!(
        thing_deferred,
        extern "C" fn(*mut Thing, *mut t_symbol, c_long, *mut t_atom)
    );
    unsafe {
        let mut args: [t_atom; 2] = std::mem::zeroed();
        max_sys::atom_setlong(&mut args[0], 7);
        max_sys::atom_setsym(&mut args[1], sym(c"s"));

        // on the main thread defer runs now, defer_low always queues
        max_sys::defer(o, f, sym(c"now"), 2, args.as_mut_ptr());
        assert_eq!(DEFERRED.lock().unwrap().len(), 1);
        max_sys::defer_low(o, f, sym(c"later"), 2, args.as_mut_ptr());
        // the queued call has its own copy of the atoms
        max_sys::atom_setlong(&mut args[0], 99);
        assert_eq!(DEFERRED.lock().unwrap().len(), 1);
        assert_eq!(k.main_queue_len(), 1);

        // a clock callback is the timer thread: defer queues there
        DEFER_FROM_TICK.store(1, Ordering::SeqCst);
        k.send(o, "sched", &[Atom::Float(1.0)]).unwrap();
        k.advance(1.0);
        DEFER_FROM_TICK.store(0, Ordering::SeqCst);
        assert_eq!(k.main_queue_len(), 2);

        // qelems: setting twice queues once
        let q = max_sys::qelem_new(o, meth!(thing_qelem, extern "C" fn(*mut Thing)));
        max_sys::qelem_set(q);
        max_sys::qelem_set(q);
        assert_eq!(k.main_queue_len(), 3);
        let q2 = max_sys::qelem_new(o, meth!(thing_qelem, extern "C" fn(*mut Thing)));
        max_sys::qelem_set(q2);
        max_sys::qelem_unset(q2);

        assert_eq!(k.run_main_queue(), 3);
        assert_eq!((*x).qcalls, 1);
        assert_eq!(k.run_main_queue(), 0);
        max_sys::qelem_free(q);
        max_sys::qelem_free(q2);
    }
    let d = DEFERRED.lock().unwrap().clone();
    assert_eq!(
        d,
        vec![
            (
                "now".to_string(),
                vec![Atom::Long(7), Atom::Sym("s".into())]
            ),
            (
                "later".to_string(),
                vec![Atom::Long(7), Atom::Sym("s".into())]
            ),
            ("from_timer".to_string(), vec![]),
        ]
    );
}

#[test]
fn a_nested_main_queue_runs_inside_the_callback() {
    let k = median_mock::lock();
    register();
    DEFERRED.lock().unwrap().clear();
    let x = thing(&k, 1);
    unsafe {
        let q = max_sys::qelem_new(x as _, meth!(thing_qelem_nested, extern "C" fn(*mut Thing)));
        NESTED_QELEM.store(q as _, Ordering::SeqCst);
        max_sys::qelem_set(q);
        // the outer run counts only what it ran itself
        assert_eq!(k.run_main_queue(), 1);
        assert_eq!((*x).qcalls, 1);
        assert_eq!(DEFERRED.lock().unwrap()[0].0, "nested");
        assert_eq!(k.main_queue_len(), 0);
        max_sys::qelem_free(q);
    }
    // only the thread that holds the kernel may run it
    let other = std::thread::spawn(median_mock::Kernel::run_main_queue_nested).join();
    assert!(other.is_err());
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn a_qelem_set_inside_its_own_function_stays_unset() {
    // `ext_proto.h`: "A Qelem object is unset after its queue function has been called"
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1);
    unsafe {
        let q = max_sys::qelem_new(x as _, meth!(thing_qelem_self, extern "C" fn(*mut Thing)));
        NESTED_QELEM.store(q as _, Ordering::SeqCst);
        max_sys::qelem_set(q);
        assert_eq!(k.main_queue_len(), 1);
        // its `qelem_set` and `qelem_front` did nothing: it ran once and is not queued
        assert_eq!(k.run_main_queue(), 1);
        assert_eq!((*x).qcalls, 1);
        assert_eq!(k.main_queue_len(), 0);
        assert_eq!(k.run_main_queue(), 0);
        // set after its function returned, it runs again
        max_sys::qelem_set(q);
        assert_eq!(k.run_main_queue(), 1);
        assert_eq!((*x).qcalls, 2);
        assert_eq!(k.main_queue_len(), 0);
        max_sys::qelem_free(q);
    }
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn a_nested_advance_fires_inside_the_clock_callback() {
    let k = median_mock::lock();
    register();
    TICKS.lock().unwrap().clear();
    DEFER_FROM_TICK.store(3, Ordering::SeqCst);
    let a = thing(&k, 1) as *mut c_void;
    k.send(a, "sched", &[Atom::Float(5.0)]).unwrap();
    assert_eq!(k.advance(10.0), 1);
    DEFER_FROM_TICK.store(0, Ordering::SeqCst);
    // outer tick at 5, nested tick at 6, then the outer callback finishes
    let ticks = TICKS.lock().unwrap().clone();
    assert_eq!(ticks, [(1, 5.0, true), (1, 6.0, true), (1, -1.0, true)]);
    assert_eq!(k.time(), 10.0);
    let other = std::thread::spawn(|| median_mock::Kernel::advance_nested(1.0)).join();
    assert!(other.is_err());
}

#[test]
fn qelem_front_queues_at_the_front_and_frees_are_checked() {
    let k = median_mock::lock();
    register();
    DEFERRED.lock().unwrap().clear();
    let x = thing(&k, 1);
    let o = x as *mut c_void;
    let before = k.live_qelem_count();
    unsafe {
        let f = meth!(
            thing_deferred,
            extern "C" fn(*mut Thing, *mut t_symbol, c_long, *mut t_atom)
        );
        max_sys::defer_low(o, f, sym(c"back"), 0, null_mut());
        let q = max_sys::qelem_new(o, meth!(thing_qelem, extern "C" fn(*mut Thing)));
        assert_eq!(k.live_qelem_count(), before + 1);
        max_sys::qelem_front(q);
        // already set: neither call queues it again (the mock's model of `qelem_front` on a set
        // qelem; not verified in Max)
        max_sys::qelem_front(q);
        max_sys::qelem_set(q);
        assert_eq!(k.main_queue_len(), 2);
        // the qelem runs first: when the deferred call runs, it has already counted
        assert_eq!(k.run_main_queue(), 2);
        assert_eq!((*x).qcalls, 1);
        assert_eq!(DEFERRED.lock().unwrap().len(), 1);

        // freeing a set qelem cancels it; freeing it again is reported
        max_sys::qelem_set(q);
        max_sys::qelem_free(q);
        assert_eq!(k.live_qelem_count(), before);
        assert_eq!(k.run_main_queue(), 0);
        assert!(k.console_log().is_empty());
        max_sys::qelem_free(q);
        let log = k.take_console_log();
        assert_eq!(log.len(), 1);
        assert!(log[0].text.contains("not a live qelem"), "{log:?}");
    }
}

#[test]
fn loadbang_calls_the_method() {
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1);
    assert!(k.loadbang(x as *mut c_void));
    unsafe { assert_eq!((*x).qcalls, 100) };
    let a = k.new_object("append.thing", &[]);
    assert!(!k.loadbang(a));
}

// ---------------------------------------------------------------------------------------------
// console

#[test]
fn console_formats_and_records() {
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1);
    unsafe {
        max_sys::post(
            c"hello %s %d %.2f".as_ptr(),
            c"world".as_ptr(),
            42i32,
            1.5f64,
        );
        max_sys::error(c"bad".as_ptr());
        max_sys::object_post(x as _, c"%s".as_ptr(), c"posted".as_ptr());
        max_sys::object_warn(x as _, c"warned %ld".as_ptr(), 7 as c_long);
        max_sys::object_error(x as _, c"%%s is %s".as_ptr(), c"not a format".as_ptr());
        let long = "x".repeat(5000);
        let long = std::ffi::CString::new(long).unwrap();
        max_sys::post(c"%s".as_ptr(), long.as_ptr());
    }
    let log = k.take_console_log();
    assert_eq!(log.len(), 6);
    assert_eq!(
        (log[0].level, log[0].object, log[0].text.as_str()),
        (Level::Post, 0, "hello world 42 1.50")
    );
    assert_eq!((log[1].level, log[1].text.as_str()), (Level::Error, "bad"));
    assert_eq!(
        (log[2].level, log[2].object, log[2].text.as_str()),
        (Level::Post, x as usize, "posted")
    );
    assert_eq!(
        (log[3].level, log[3].text.as_str()),
        (Level::Warn, "warned 7")
    );
    assert_eq!(
        (log[4].level, log[4].text.as_str()),
        (Level::Error, "%s is not a format")
    );
    assert_eq!(log[5].text.len(), 5000);
    assert!(k.console_log().is_empty());
}

// ---------------------------------------------------------------------------------------------
// sysmem and atoms

#[test]
fn sysmem_round_trip_including_resize() {
    let _k = median_mock::lock();
    unsafe {
        let p = max_sys::sysmem_newptr(100) as *mut u8;
        assert!(!p.is_null());
        assert_eq!(p as usize % 16, 0);
        assert_eq!(max_sys::sysmem_ptrsize(p as _), 100);
        for i in 0..100 {
            p.add(i).write(i as u8);
        }
        // grow: contents kept, size header updated
        let p = max_sys::sysmem_resizeptr(p as _, 10_000) as *mut u8;
        assert!(!p.is_null());
        assert_eq!(max_sys::sysmem_ptrsize(p as _), 10_000);
        for i in 0..100 {
            assert_eq!(p.add(i).read(), i as u8);
        }
        p.add(9_999).write(1);
        // shrink
        let p = max_sys::sysmem_resizeptr(p as _, 10) as *mut u8;
        assert_eq!(max_sys::sysmem_ptrsize(p as _), 10);
        assert_eq!(p.add(9).read(), 9);
        max_sys::sysmem_freeptr(p as _);

        // resizing null allocates
        let q = max_sys::sysmem_resizeptr(null_mut(), 8) as *mut u8;
        assert_eq!(max_sys::sysmem_ptrsize(q as _), 8);
        max_sys::sysmem_freeptr(q as _);
        max_sys::sysmem_freeptr(null_mut());

        // clear
        let z = max_sys::sysmem_newptrclear(256) as *mut u8;
        assert!((0..256).all(|i| z.add(i).read() == 0));
        max_sys::sysmem_freeptr(z as _);

        // zero sized blocks are valid
        let e = max_sys::sysmem_newptr(0);
        assert!(!e.is_null());
        assert_eq!(max_sys::sysmem_ptrsize(e as _), 0);
        max_sys::sysmem_freeptr(e as _);

        // handles
        let h = max_sys::sysmem_newhandle(32);
        assert!(!h.is_null() && !(*h).is_null());
        (*h).write(5);
        max_sys::sysmem_freehandle(h);

        let mut buf = [1 as std::ffi::c_char; 4];
        max_sys::strncpy_zero(buf.as_mut_ptr(), c"abcdef".as_ptr(), 4);
        assert_eq!(buf, [b'a' as _, b'b' as _, b'c' as _, 0]);
    }
}

#[test]
fn atom_accessors() {
    let _k = median_mock::lock();
    unsafe {
        let mut a: t_atom = std::mem::zeroed();
        assert_eq!(max_sys::atom_setlong(&mut a, 7), 0);
        assert_eq!(max_sys::atom_gettype(&a), ty::A_LONG as c_long);
        assert_eq!(
            (max_sys::atom_getlong(&a), max_sys::atom_getfloat(&a)),
            (7, 7.0)
        );
        max_sys::atom_setfloat(&mut a, 2.9);
        assert_eq!(max_sys::atom_gettype(&a), ty::A_FLOAT as c_long);
        assert_eq!(
            (max_sys::atom_getlong(&a), max_sys::atom_getfloat(&a)),
            (2, 2.9)
        );
        assert_eq!(max_sys::atom_getsym(&a), sym(c""));
        max_sys::atom_setsym(&mut a, sym(c"sym"));
        assert_eq!(max_sys::atom_gettype(&a), ty::A_SYM as c_long);
        assert_eq!(max_sys::atom_getsym(&a), sym(c"sym"));
        assert_eq!(max_sys::atom_getlong(&a), 0);
        let mut x = 5;
        max_sys::atom_setobj(&mut a, &raw mut x as _);
        assert_eq!(max_sys::atom_getobj(&a), &raw mut x as *mut c_void);
        assert_eq!(
            max_sys::atom_setlong(null_mut(), 1),
            max_sys::e_max_errorcodes::MAX_ERR_INVALID_PTR as t_max_err
        );
    }
    let atoms = [
        Atom::Long(-3),
        Atom::Float(0.5),
        Atom::Sym("x".into()),
        Atom::Obj(0x10),
        Atom::Other(10),
    ];
    for a in &atoms {
        assert_eq!(&Atom::from_raw(&a.to_raw()), a);
    }
}

// ---------------------------------------------------------------------------------------------
// msp

#[test]
fn dsp64_records_the_perform_routine_and_it_runs_on_the_audio_role() {
    let k = median_mock::lock();
    register();
    let o = k.new_object("dsp.thing", &[]);
    assert!(!o.is_null());
    assert_eq!(k.signal_inlets(o), Some(2));
    assert_eq!(k.extra_inlet_count(o), 1);
    assert_eq!(k.outlet_types(o), vec![Some("signal".to_string())]);
    let adds = k.dsp64(o, 48000.0, 64).unwrap();
    assert_eq!(adds.len(), 1);
    let mut ins = vec![vec![1.0; 8], (0..8).map(f64::from).collect()];
    let mut outs = vec![vec![0.0; 8]];
    PERFORM_ROLE_AUDIO.store(0, Ordering::SeqCst);
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(PERFORM_ROLE_AUDIO.load(Ordering::SeqCst), 1);
    assert_eq!(
        outs[0],
        (0..8).map(|i| 1.0 + i as f64 + 3.0).collect::<Vec<_>>()
    );
    unsafe {
        assert_eq!(max_sys::systhread_isaudiothread(), 0);
        assert_eq!(max_sys::systhread_ismainthread(), 1);
    }
    assert!(k.free_object(o));
}

#[test]
fn dsp_adds_lists_what_dsp64_was_called_without_the_harness() {
    let k = median_mock::lock();
    register();
    let o = k.new_object("dsp.thing", &[]);
    assert!(k.dsp_adds().is_empty());
    // `Kernel::dsp64` hands over what the method added, so nothing is left for `dsp_adds`
    assert_eq!(k.dsp64(o, 48000.0, 64).unwrap().len(), 1);
    assert!(k.dsp_adds().is_empty());
    // calling the method some other way is seen, and a read does not take them
    let method = k.method(o, "dsp64").unwrap().0;
    // SAFETY: `dsp64` is the `A_CANT` method `(x, dsp64, count, sr, vs, flags)` of `DspThing`
    unsafe {
        let f = std::mem::transmute::<
            usize,
            extern "C" fn(*mut c_void, *mut t_object, *mut c_short, f64, c_long, c_long),
        >(method);
        f(o, null_mut(), null_mut(), 48000.0, 64, 0);
        f(o, null_mut(), null_mut(), 48000.0, 64, 0);
    }
    assert_eq!(k.dsp_adds().len(), 2);
    assert_eq!(k.dsp_adds().len(), 2);
    assert_eq!(k.dsp_adds()[0].object, o as usize);
    // a reset forgets them
    k.reset();
    assert!(k.dsp_adds().is_empty());
}

// ---------------------------------------------------------------------------------------------
// attributes

#[test]
fn attributes_through_offset_clip_and_accessors() {
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1);
    let o = x as *mut c_void;
    assert_eq!(k.attr_names(o), ["gain", "level"]);
    // offset attribute with a clip filter
    k.attr_set(o, "gain", &[Atom::Float(0.25)]).unwrap();
    unsafe { assert_eq!((*x).gain, 0.25) };
    k.attr_set(o, "gain", &[Atom::Float(7.0)]).unwrap();
    assert_eq!(k.attr_get(o, "gain").unwrap(), [Atom::Float(1.0)]);
    k.attr_set(o, "gain", &[Atom::Long(-4)]).unwrap();
    assert_eq!(k.attr_get(o, "gain").unwrap(), [Atom::Float(0.0)]);
    // accessors, with the getter allocating the result
    k.attr_set(o, "level", &[Atom::Long(12)]).unwrap();
    unsafe { assert_eq!((*x).level, 12) };
    assert_eq!(k.attr_get(o, "level").unwrap(), [Atom::Long(12)]);
    assert!(matches!(
        k.attr_get(o, "nope"),
        Err(median_mock::AttrError::NoAttr(_))
    ));
    assert_eq!(k.attr_flags(o, "gain"), Some(0));

    unsafe { max_sys::object_attr_touch(x as _, sym(c"gain")) };
    assert_eq!(k.attr_touches(), vec![(x as usize, "gain".to_string())]);
}

#[test]
fn attributes_of_attributes_are_recorded_per_attribute() {
    let k = median_mock::lock();
    register();
    let o = thing(&k, 1) as *mut c_void;
    assert_eq!(k.attr_attr_names(o, "level"), ["enumvals", "label"]);
    assert_eq!(
        k.attr_attr(o, "level", "enumvals"),
        Some((
            "atom".to_string(),
            vec![
                Atom::Sym("zero".into()),
                Atom::Sym("one".into()),
                Atom::Sym("two three".into()),
                Atom::Long(4),
                Atom::Float(5.5),
                Atom::Long(-6),
            ]
        ))
    );
    assert_eq!(
        k.attr_attr(o, "level", "label"),
        Some(("symbol".to_string(), vec![Atom::Sym("The Level".into())]))
    );
    assert_eq!(
        k.attr_attr(o, "gain", "save"),
        Some(("long".to_string(), vec![Atom::Long(1)]))
    );
    assert_eq!(
        k.attr_attr(o, "gain", "order"),
        Some(("long".to_string(), vec![Atom::Long(2)]))
    );
    assert_eq!(k.attr_attr(o, "gain", "label"), None);
    assert_eq!(k.attr_attr(o, "nope", "label"), None);
    // an attribute the class doesn't have is an error
    let c = THING_CLASS.load(Ordering::SeqCst);
    let code = unsafe {
        max_sys::class_attr_addattr_parse(
            c,
            c"nope".as_ptr(),
            c"save".as_ptr(),
            sym(c"long"),
            0,
            c"1".as_ptr(),
        )
    };
    assert_ne!(code, 0);
    assert_eq!(k.take_console_log().len(), 1);
}

#[test]
fn parse_strings_split_like_atom_setparse() {
    use median_mock::attr::parse_atoms;
    assert_eq!(parse_atoms(""), []);
    assert_eq!(
        parse_atoms("  a 1 -2 3.5 .5 1e3 -x \"q \\\" r\" "),
        [
            Atom::Sym("a".into()),
            Atom::Long(1),
            Atom::Long(-2),
            Atom::Float(3.5),
            Atom::Float(0.5),
            Atom::Float(1000.0),
            Atom::Sym("-x".into()),
            Atom::Sym("q \" r".into()),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// registration, notification, buffers

#[test]
fn getters_and_setters_with_null_pointers_and_a_caller_buffer() {
    use median_mock::GetterArgs;
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1) as *mut c_void;
    k.attr_set(x, "level", &[Atom::Long(4)]).unwrap();
    for args in [GetterArgs::NullAc, GetterArgs::NullAv, GetterArgs::NullBoth] {
        let r = k.attr_get_with(x, "level", args).unwrap();
        assert_eq!(
            (r.code, r.atoms.len(), r.used_buffer),
            (0, 0, false),
            "{args:?}"
        );
    }
    let r = k.attr_get_with(x, "level", GetterArgs::Buffer(2)).unwrap();
    assert_eq!(
        (r.code, r.atoms.as_slice(), r.used_buffer),
        (0, &[Atom::Long(4), Atom::Long(0)][..], true)
    );
    let r = k.attr_get_with(x, "level", GetterArgs::Buffer(0)).unwrap();
    assert_eq!(
        (r.atoms.as_slice(), r.used_buffer),
        (&[Atom::Long(4)][..], false)
    );
    assert_eq!(k.attr_set_null_argv(x, "level", 1), Ok(0));
    assert_eq!(k.attr_get(x, "level").unwrap(), [Atom::Long(4)]);
    assert!(k.method_names(x).contains(&"bang".to_string()));
    assert!(k.free_object(x));
}

#[test]
fn register_attach_and_notify() {
    let k = median_mock::lock();
    register();
    NOTES.lock().unwrap().clear();
    let owner = thing(&k, 1) as *mut c_void;
    let client = thing(&k, 2) as *mut c_void;
    unsafe {
        let ns = sym(c"testspace");
        assert!(max_sys::object_findregistered(ns, sym(c"n")).is_null());
        // subscribe before the object exists, then register: the client is attached and told
        assert!(max_sys::object_subscribe(ns, sym(c"n"), null_mut(), client).is_null());
        assert_eq!(max_sys::object_register(ns, sym(c"n"), owner), owner);
        assert_eq!(max_sys::object_findregistered(ns, sym(c"n")), owner);
        assert_eq!(
            NOTES.lock().unwrap().as_slice(),
            [(
                "n".to_string(),
                "globalsymbol_binding".to_string(),
                owner as usize
            )]
        );
        NOTES.lock().unwrap().clear();
        // notify reaches attached clients only
        assert_eq!(
            max_sys::object_notify(owner, sym(c"changed"), 5 as *mut c_void),
            0
        );
        assert_eq!(
            NOTES.lock().unwrap().as_slice(),
            [("n".to_string(), "changed".to_string(), 5)]
        );
        NOTES.lock().unwrap().clear();
        assert_eq!(max_sys::object_detach(ns, sym(c"n"), client), 0);
        max_sys::object_notify(owner, sym(c"changed"), null_mut());
        assert!(NOTES.lock().unwrap().is_empty());
        // freeing the registered object unregisters it
        assert_eq!(max_sys::object_attach(ns, sym(c"n"), client), owner);
        assert!(k.free_object(owner));
        assert!(max_sys::object_findregistered(ns, sym(c"n")).is_null());
        assert_eq!(NOTES.lock().unwrap().len(), 1);
        assert_eq!(NOTES.lock().unwrap()[0].1, "globalsymbol_unbinding");
    }
}

#[test]
fn classnames_retains_and_the_dictionary_registry_cover_every_class() {
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1) as *mut c_void;
    unsafe {
        assert_eq!(max_sys::object_classname(x), sym(c"thing"));
        let d = max_sys::dictionary_new();
        assert_eq!(max_sys::object_classname(d.cast()), sym(c"dictionary"));
        assert!(max_sys::object_classname(null_mut()).is_null());
        // a retained object is released by `object_free`, not freed
        assert_eq!(k.retain_count(x), Some(0));
        assert_eq!(max_sys::object_retain(x.cast()), 0);
        assert_eq!(k.retain_count(x), Some(1));
        assert_eq!(max_sys::object_free(x), 0);
        assert_eq!(k.retain_count(x), Some(0));
        // `dictobj_findregistered_retain` finds any class registered in the dictionary namespace
        let ns = sym(c"dictionary");
        assert_eq!(max_sys::object_register(ns, sym(c"foreign"), x), x);
        assert_eq!(
            max_sys::dictobj_findregistered_retain(sym(c"foreign")).cast::<c_void>(),
            x
        );
        assert_eq!(k.retain_count(x), Some(1));
        assert_eq!(max_sys::object_free(x), 0);
        assert_eq!(k.retain_count(x), Some(0));
        // and a dictionary under its own name
        assert_eq!(max_sys::object_free(d.cast()), 0);
        let mut dname = null_mut();
        let d2 = max_sys::dictionary_new();
        assert_eq!(max_sys::dictobj_register(d2, &mut dname), d2);
        let found = max_sys::dictobj_findregistered_retain(dname);
        assert_eq!(found, d2);
        assert_eq!(max_sys::object_free(found.cast()), 0);
        assert_eq!(max_sys::object_free(d2.cast()), 0);
    }
    assert!(k.free_object(x));
}

#[test]
fn buffer_refs_bind_to_buffers() {
    let k = median_mock::lock();
    register();
    NOTES.lock().unwrap().clear();
    let client = thing(&k, 2) as *mut c_void;
    unsafe {
        let r = max_sys::buffer_ref_new(client as _, sym(c"buf"));
        assert_eq!(max_sys::buffer_ref_exists(r), 0);
        assert!(max_sys::buffer_ref_getobject(r).is_null());
        max_sys::object_subscribe(sym(c"globalsymbol"), sym(c"buf"), sym(c"buffer~"), client);

        let b = k.create_buffer("buf", 2, 4, 44100.0);
        assert_eq!(
            NOTES.lock().unwrap().as_slice(),
            [(
                "buf".to_string(),
                "globalsymbol_binding".to_string(),
                b as usize
            )]
        );
        assert_eq!(max_sys::buffer_ref_exists(r), 1);
        assert_eq!(max_sys::buffer_ref_getobject(r), b);
        assert_eq!(max_sys::buffer_getchannelcount(b), 2);
        assert_eq!(max_sys::buffer_getframecount(b), 4);
        assert_eq!(max_sys::buffer_getsamplerate(b), 44100.0);
        assert_eq!(max_sys::buffer_getmillisamplerate(b), 44.1);

        // buffer~ answers `getname`
        let mut name: *mut t_symbol = null_mut();
        max_sys::object_method(b as _, sym(c"getname"), &raw mut name as *mut c_void);
        assert_eq!(name, sym(c"buf"));

        // interleaved: frame major
        let p = max_sys::buffer_locksamples(b);
        assert!(!p.is_null());
        for frame in 0..4 {
            for ch in 0..2 {
                *p.add(frame * 2 + ch) = (frame * 10 + ch) as f32;
            }
        }
        assert_eq!(k.buffer_state("buf"), Some((1, false)));
        max_sys::buffer_unlocksamples(b);
        max_sys::buffer_setdirty(b);
        assert_eq!(k.buffer_state("buf"), Some((0, true)));
        assert_eq!(
            k.buffer_samples("buf").unwrap(),
            [0.0, 1.0, 10.0, 11.0, 20.0, 21.0, 30.0, 31.0]
        );

        max_sys::buffer_ref_set(r, sym(c"other"));
        assert_eq!(max_sys::buffer_ref_exists(r), 0);
        max_sys::buffer_ref_set(r, sym(c"buf"));
        max_sys::buffer_ref_notify(
            r,
            sym(c"buf"),
            sym(c"globalsymbol_binding"),
            b as _,
            null_mut(),
        );
        assert_eq!(k.buffer_ref_notifications().len(), 1);

        NOTES.lock().unwrap().clear();
        k.destroy_buffer("buf");
        assert_eq!(NOTES.lock().unwrap()[0].1, "globalsymbol_unbinding");
        assert_eq!(max_sys::buffer_ref_exists(r), 0);
        // a destroyed buffer cannot be locked
        assert!(max_sys::buffer_locksamples(b).is_null());
    }
}

fn errors(k: &median_mock::Kernel) -> Vec<String> {
    k.take_console_log()
        .into_iter()
        .filter(|e| e.level == Level::Error)
        .map(|e| e.text)
        .collect()
}

#[test]
fn buffer_edits_pair_and_exclude_claims() {
    let k = median_mock::lock();
    let b = k.create_buffer("ed", 2, 3, 48000.0);
    unsafe {
        assert_eq!(k.buffer_edits("ed"), Some((0, 0)));
        assert_eq!(max_sys::buffer_edit_begin(b), 0);
        assert_eq!(k.buffer_editing("ed"), Some(true));
        // no claims while editing
        assert!(max_sys::buffer_locksamples(b).is_null());
        assert_eq!(k.buffer_state("ed"), Some((0, false)));
        // the samples, through buffer_getinfo
        let mut info: max_sys::t_buffer_info = std::mem::zeroed();
        assert_eq!(max_sys::buffer_getinfo(b, &mut info), 0);
        assert_eq!(
            (
                info.b_frames as i64,
                info.b_nchans as i64,
                info.b_size as i64
            ),
            (3, 2, 6)
        );
        assert_eq!(info.b_sr, 48000.0);
        assert_eq!(info.b_name, sym(c"ed"));
        *info.b_samples.add(5) = 7.0;
        // a nested begin is reported, not refused: Max would deadlock or alias, which the
        // mock doesn't model; it pairs with an end of its own
        assert_eq!(max_sys::buffer_edit_begin(b), 0);
        let e = errors(&k);
        assert!(
            e.len() == 1 && e[0].contains("nested buffer_edit_begin"),
            "{e:?}"
        );
        assert_eq!(max_sys::buffer_edit_end(b, -1), 0);
        assert_eq!(k.buffer_editing("ed"), Some(true));
        assert_eq!(k.buffer_last_edit_valid("ed"), Some(Some(-1)));
        assert_eq!(max_sys::buffer_edit_end(b, 1), 0);
        assert_eq!(k.buffer_editing("ed"), Some(false));
        assert_eq!(k.buffer_last_edit_valid("ed"), Some(Some(1)));
        assert_eq!(k.buffer_edits("ed"), Some((2, 2)));
        assert_eq!(k.buffer_samples("ed").unwrap(), [0., 0., 0., 0., 0., 7.]);
        // an end without a begin is refused and reported
        assert_ne!(max_sys::buffer_edit_end(b, 1), 0);
        assert_eq!(k.buffer_edits("ed"), Some((2, 2)));
        let e = errors(&k);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("without buffer_edit_begin"), "{e:?}");
        // a begin while a claim is held, or on the audio thread, is reported (but done)
        assert!(!max_sys::buffer_locksamples(b).is_null());
        assert_eq!(max_sys::buffer_edit_begin(b), 0);
        let e = errors(&k);
        assert!(e.len() == 1 && e[0].contains("claim(s) are held"), "{e:?}");
        assert_eq!(max_sys::buffer_edit_end(b, 1), 0);
        max_sys::buffer_unlocksamples(b);
        k.with_role(Role::Audio, || assert_eq!(max_sys::buffer_edit_begin(b), 0));
        let e = errors(&k);
        assert!(e.len() == 1 && e[0].contains("audio thread"), "{e:?}");
        assert_eq!(max_sys::buffer_edit_end(b, 1), 0);
        // destroying mid-edit or with claims is reported
        assert!(!max_sys::buffer_locksamples(b).is_null());
        assert_eq!(max_sys::buffer_edit_begin(b), 0);
        assert_eq!(errors(&k).len(), 1);
    }
    k.destroy_buffer("ed");
    let e = errors(&k);
    assert_eq!(e.len(), 2, "{e:?}");
    assert!(
        e[0].contains("not unlocked") && e[1].contains("buffer_edit_begin"),
        "{e:?}"
    );
}

#[test]
fn buffer_view_and_dirty() {
    let k = median_mock::lock();
    let b = k.create_buffer("v", 1, 1, 44100.0);
    unsafe {
        max_sys::buffer_view(b);
        max_sys::buffer_view(null_mut());
        max_sys::buffer_setdirty(b);
    }
    assert_eq!(k.buffer_views(), ["v"]);
    assert_eq!(errors(&k).len(), 1);
    assert_eq!(k.buffer_state("v"), Some((0, true)));
    k.clear_buffer_dirty("v");
    assert_eq!(k.buffer_state("v"), Some((0, false)));
}

unsafe extern "C" fn leaky_perform(
    _x: *mut t_object,
    _dsp64: *mut t_object,
    _ins: *mut *mut f64,
    _numins: c_long,
    _outs: *mut *mut f64,
    _numouts: c_long,
    _frames: c_long,
    _flags: c_long,
    buffer: *mut c_void,
) {
    // claims the buffer twice and releases it once
    unsafe {
        max_sys::buffer_locksamples(buffer as _);
        max_sys::buffer_locksamples(buffer as _);
        max_sys::buffer_unlocksamples(buffer as _);
    }
}

#[test]
fn a_perform_routine_that_keeps_a_claim_is_reported() {
    let k = median_mock::lock();
    let b = k.create_buffer("leak", 1, 4, 44100.0);
    // a claim from before the call is not the perform routine's
    unsafe { max_sys::buffer_locksamples(b) };
    let add = median_mock::DspAdd {
        object: b as usize,
        perform: leaky_perform as *const () as usize,
        flags: 0,
        userparam: b as usize,
        dsp64: 0,
    };
    add.run(&mut [], &mut [vec![0.0; 4]]);
    let e = errors(&k);
    assert!(
        e.len() == 1 && e[0].contains("holding 1 buffer~ claim"),
        "{e:?}"
    );
    assert_eq!(k.buffer_state("leak"), Some((2, false)));
}

// ---------------------------------------------------------------------------------------------
// obex and threads

#[test]
fn obex_store_and_thread_roles() {
    let k = median_mock::lock();
    register();
    let x = thing(&k, 1) as *mut c_void;
    unsafe {
        let key = sym(c"#P");
        let mut out: *mut t_object = null_mut();
        assert_ne!(max_sys::object_obex_lookup(x, key, &mut out), 0);
        assert_eq!(max_sys::object_obex_storeflags(x, key, 0x40 as _, 1), 0);
        assert_eq!(max_sys::object_obex_lookup(x, key, &mut out), 0);
        assert_eq!(out as usize, 0x40);
        assert!(max_sys::scheduler_fromobject(x as _).is_null());
    }
    k.with_role(Role::Other, || unsafe {
        assert_eq!(max_sys::systhread_ismainthread(), 0);
        assert_eq!(max_sys::systhread_isaudiothread(), 0);
        assert_eq!(max_sys::systhread_istimerthread(), 0);
    });
    k.with_role(Role::Timer, || unsafe {
        assert_eq!(max_sys::systhread_istimerthread(), 1);
    });
    k.set_sample_rate(96000.0);
    assert_eq!(unsafe { max_sys::sys_getsr() }, 96000.0);
    unsafe {
        assert_eq!(max_sys::gettime(), 0);
        let mut t = 1.0;
        k.advance(2.5);
        max_sys::clock_getftime(&mut t);
        assert_eq!((t, max_sys::gettime()), (2.5, 2));
    }
}
