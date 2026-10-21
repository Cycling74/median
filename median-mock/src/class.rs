//! Classes, objects and message dispatch: `class_*`, `object_alloc`, `object_free`,
//! `object_method`, and the dispatcher the harness uses to send messages.

use crate::atoms::Atom;
use crate::state::{ClassData, MethodRec, ObjData, SubKind, state};
use crate::symbols::{intern, name_of, sym_name};
use max_sys::{e_max_atomtypes as ty, e_max_errorcodes as err};
use max_sys::{method, t_class, t_max_err, t_messlist, t_object, t_symbol};
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_int, c_long, c_void};

/// An argument in the register class it is passed in, see `build.rs`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Raw {
    I(usize),
    F(f64),
}

include!(concat!(env!("OUT_DIR"), "/dispatch_gen.rs"));

fn method_addr(m: method) -> usize {
    m.map_or(0, |f| f as usize)
}

fn max_err(e: err::Type) -> t_max_err {
    e as t_max_err
}

// ---------------------------------------------------------------------------------------------
// classes

/// Called by `class_new` in `c/variadic.c` with the type list collected.
///
/// # Safety
/// `name` must be a NUL-terminated string and `types` valid for `ntypes` ints.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn median_mock_class_new(
    name: *const c_char,
    mnew: method,
    mfree: method,
    size: c_long,
    _mmenu: method,
    types: *const c_int,
    ntypes: c_int,
) -> *mut t_class {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the caller's contract
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();
    // SAFETY: the caller's contract
    let new_types = unsafe { std::slice::from_raw_parts(types, ntypes.max(0) as usize) }.to_vec();

    // SAFETY: `t_class` is plain data, valid when zeroed
    let mut class: Box<t_class> = Box::new(unsafe { std::mem::zeroed() });
    class.c_sym = intern(name.as_bytes());
    class.c_freefun = mfree;
    class.c_size = size as _;
    let class = Box::into_raw(class);

    // Objects point `o_messlist` at element 1 of this; element 0 holds the class (the "-1 entry").
    // SAFETY: `t_messlist` is plain data, valid when zeroed
    let mut ml: Box<[t_messlist; 2]> = Box::new(unsafe { std::mem::zeroed() });
    ml[0].m_sym = class as *mut t_symbol;
    let ml = Box::into_raw(ml) as *mut t_messlist;

    state().classes.insert(
        class as usize,
        ClassData {
            name,
            namespace: None,
            new_fn: method_addr(mnew),
            free_fn: method_addr(mfree),
            size: size.max(0) as usize,
            new_types,
            methods: Default::default(),
            attrs: Vec::new(),
            dsp: false,
            obex_offset: 0,
            // SAFETY: the array has two elements
            messlist: unsafe { ml.add(1) } as usize,
        },
    );
    class
}

/// Called by `class_addmethod` in `c/variadic.c`.
///
/// # Safety
/// `c` must be a class from `class_new`, `name` a NUL-terminated string and `types` valid for
/// `ntypes` ints.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn median_mock_class_addmethod(
    c: *mut t_class,
    m: method,
    name: *const c_char,
    types: *const c_int,
    ntypes: c_int,
) -> t_max_err {
    let (Some(_), false) = (m, name.is_null()) else {
        return max_err(err::MAX_ERR_INVALID_PTR);
    };
    // SAFETY: the caller's contract
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();
    // SAFETY: the caller's contract
    let types = unsafe { std::slice::from_raw_parts(types, ntypes.max(0) as usize) }.to_vec();
    let mut st = state();
    let Some(class) = st.classes.get_mut(&(c as usize)) else {
        return max_err(err::MAX_ERR_INVALID_PTR);
    };
    class.methods.insert(
        name,
        MethodRec {
            fun: method_addr(m),
            types,
        },
    );
    max_err(err::MAX_ERR_NONE)
}

/// # Safety
/// `c` must be a class from `class_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn class_register(name_space: *mut t_symbol, c: *mut t_class) -> t_max_err {
    // SAFETY: symbols from `gensym` or null
    let ns = unsafe { sym_name(name_space) };
    let mut st = state();
    let Some(class) = st.classes.get_mut(&(c as usize)) else {
        return max_err(err::MAX_ERR_INVALID_PTR);
    };
    class.namespace = Some(ns.clone());
    let name = class.name.clone();
    // SAFETY: `c` is one of our classes, allocated as a `t_class`
    unsafe { (*c).c_flags |= max_sys::e_max_class_flags::CLASS_FLAG_REGISTERED as c_long };
    st.class_names.insert((ns, name), c as usize);
    max_err(err::MAX_ERR_NONE)
}

/// # Safety
/// Symbols from `gensym` or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn class_findbyname(
    name_space: *mut t_symbol,
    classname: *mut t_symbol,
) -> *mut t_class {
    // SAFETY: the caller's contract
    let (ns, name) = unsafe { (sym_name(name_space), sym_name(classname)) };
    state()
        .class_names
        .get(&(ns, name))
        .map_or(std::ptr::null_mut(), |&c| c as *mut t_class)
}

/// # Safety
/// `c` must be a class from `class_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn class_dspinit(c: *mut t_class) {
    if let Some(class) = state().classes.get_mut(&(c as usize)) {
        class.dsp = true;
    }
}

/// # Safety
/// `c` must be a class from `class_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn class_obexoffset_set(c: *mut t_class, offset: c_long) {
    if let Some(class) = state().classes.get_mut(&(c as usize)) {
        class.obex_offset = offset as i64;
    }
}

// ---------------------------------------------------------------------------------------------
// objects

/// Zeroed, with the header filled in as Max does: `o_messlist` points into the class and the
/// entry before it is the class.
///
/// # Safety
/// `c` must be a class from `class_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_alloc(c: *mut t_class) -> *mut c_void {
    let mut st = state();
    let Some(class) = st.classes.get(&(c as usize)) else {
        return std::ptr::null_mut();
    };
    let size = class.size.max(size_of::<t_object>());
    let messlist = class.messlist;
    // SAFETY: plain allocation
    let obj = unsafe { crate::sysmem::sysmem_newptrclear(size) } as *mut t_object;
    if obj.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: `obj` is at least `size_of::<t_object>()` zeroed bytes
    unsafe { (*obj).o_messlist = messlist as *mut t_messlist };
    st.objects.insert(
        obj as usize,
        ObjData {
            class: c as usize,
            outlets: Vec::new(),
            inlets: Vec::new(),
            signal_inlets: None,
            obex: Default::default(),
            registered: None,
        },
    );
    obj as *mut c_void
}

/// Frees an object from `object_alloc` (running the class's free method first and then releasing
/// its inlets, outlets and registrations), or a clock, qelem, inlet, proxy or outlet. Freeing
/// something that is already freed, or that the kernel does not know, is reported on the console
/// log and returns an error; it is never dereferenced.
///
/// # Safety
/// `x` must not be used after this returns success.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_free(x: *mut c_void) -> t_max_err {
    let addr = x as usize;
    enum Kind {
        Object(usize),
        Sub,
        DeadSub,
        Unknown,
    }
    let kind = {
        let st = state();
        if let Some(o) = st.objects.get(&addr) {
            Kind::Object(st.classes.get(&o.class).map_or(0, |c| c.free_fn))
        } else if let Some(s) = st.subs.get(&addr) {
            if s.alive { Kind::Sub } else { Kind::DeadSub }
        } else {
            Kind::Unknown
        }
    };
    match kind {
        Kind::Object(free_fn) => {
            if free_fn != 0 {
                // The free method runs with the object still alive and no lock held.
                // SAFETY: `free_fn` was registered as a `method`
                unsafe {
                    std::mem::transmute::<usize, unsafe extern "C" fn(*mut c_void) -> *mut c_void>(
                        free_fn,
                    )(x);
                }
            }
            // clients attached to it are told it is going away while it is still intact
            let _ = crate::notify::unregister_addr(addr);
            release_object(addr);
            // SAFETY: the object was allocated with `sysmem_newptrclear` by `object_alloc`
            unsafe { crate::sysmem::sysmem_freeptr(x) };
            max_err(err::MAX_ERR_NONE)
        }
        Kind::Sub => {
            kill_sub(addr);
            max_err(err::MAX_ERR_NONE)
        }
        Kind::DeadSub => {
            state().post_error(
                0,
                format!("median-mock: object_free of {addr:#x}, which is already freed (an inlet, outlet or clock is released with its owner)"),
            );
            max_err(err::MAX_ERR_INVALID_PTR)
        }
        Kind::Unknown => {
            state().post_error(
                0,
                format!("median-mock: object_free of {addr:#x}, which is not a live object"),
            );
            max_err(err::MAX_ERR_INVALID_PTR)
        }
    }
}

/// Mark a sub-object dead (and cancel it if it is scheduled).
pub(crate) fn kill_sub(addr: usize) {
    let mut st = state();
    if let Some(s) = st.subs.get_mut(&addr) {
        s.alive = false;
        match &mut s.kind {
            SubKind::Clock { fire, .. } => *fire = None,
            SubKind::Qelem { set, .. } => *set = false,
            _ => {}
        }
    }
}

/// Everything that goes away with an object: its sub-objects, registrations, attachments.
fn release_object(addr: usize) {
    let mut st = state();
    let Some(obj) = st.objects.remove(&addr) else {
        return;
    };
    for sub in obj.outlets.iter().chain(obj.inlets.iter()) {
        if let Some(s) = st.subs.get_mut(sub) {
            s.alive = false;
        }
    }
    if let Some((ns, name)) = obj.registered {
        st.registry.remove(&(ns, name));
    }
    st.attached.remove(&addr);
    for clients in st.attached.values_mut() {
        clients.retain(|&c| c != addr);
    }
    st.subscriptions.retain(|s| s.client != addr);
    // clocks and qelems owned by the object stay alive: in Max they must be freed by the owner,
    // and a clock that fires into a freed object is the owner's bug to find.
}

/// Forwards a call of `object_method` from `c/variadic.c`. Max's `object_method` calls the method
/// `s` of `x` with the variadic arguments. Here up to four pointer sized arguments are forwarded:
/// to an `A_CANT` method as they are (a callee that takes fewer ignores the rest), and to a method
/// with a typed argument list converted per that list as long as it has no float argument.
///
/// `getname` on a buffer~ made by [`crate::Kernel::create_buffer`] is understood.
///
/// # Safety
/// `x` must be a live object (or buffer) and `s` a symbol; the arguments must be what the method
/// expects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn median_mock_object_method(
    x: *mut c_void,
    s: *mut c_void,
    a0: *mut c_void,
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
) -> *mut c_void {
    let sel = name_of(s as usize);
    let args = [a0 as usize, a1 as usize, a2 as usize, a3 as usize];
    let found = {
        let mut st = state();
        if let Some(b) = st.buffers.values().find(|b| b.ptr == x as usize) {
            if sel == "getname" && !a0.is_null() {
                let name = intern(b.name.as_bytes());
                // SAFETY: the caller passes a `t_symbol **` for `getname`
                unsafe { (a0 as *mut *mut t_symbol).write(name) };
            }
            return std::ptr::null_mut();
        }
        match st.objects.get(&(x as usize)) {
            Some(o) => st.classes[&o.class].methods.get(&sel).cloned(),
            None => {
                st.post_error(x as usize, format!("median-mock: object_method \"{sel}\" on {x:?}, which is not a live object"));
                return std::ptr::null_mut();
            }
        }
    };
    let Some(rec) = found else {
        state().post_error(x as usize, format!("median-mock: object_method: no method \"{sel}\""));
        return std::ptr::null_mut();
    };
    let mut raws = vec![Raw::I(x as usize)];
    if rec.types.as_slice() == [ty::A_CANT as i32] {
        raws.extend(args.iter().map(|&a| Raw::I(a)));
    } else {
        for (i, &t) in rec.types.iter().enumerate().take(args.len()) {
            match t as u32 {
                ty::A_LONG | ty::A_DEFLONG | ty::A_SYM | ty::A_DEFSYM | ty::A_OBJ => {
                    raws.push(Raw::I(args[i]))
                }
                _ => {
                    state().post_error(x as usize, format!("median-mock: object_method \"{sel}\": float and gimme arguments are not supported"));
                    return std::ptr::null_mut();
                }
            }
        }
    }
    // SAFETY: `rec.fun` is a registered method whose arguments are of the kinds in `raws`
    unsafe { call_raw(rec.fun, &raws) }
}

// ---------------------------------------------------------------------------------------------
// dispatch

thread_local! {
    /// The `id` of the proxy that is delivering the message being handled (0 for the left inlet).
    static CURRENT_INLET: Cell<i64> = const { Cell::new(0) };
}

pub(crate) fn current_inlet() -> i64 {
    CURRENT_INLET.with(|c| c.get())
}

/// Run `f` with `proxy_getinlet` returning `id`.
pub(crate) fn with_inlet<R>(id: i64, f: impl FnOnce() -> R) -> R {
    struct Restore(i64);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT_INLET.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(CURRENT_INLET.with(|c| c.replace(id)));
    f()
}

/// Why a message could not be delivered.
#[derive(Clone, Debug, PartialEq)]
pub enum SendError {
    /// The object pointer is not a live object made by `object_alloc`.
    NotLive,
    /// No method for the selector (nor a fallback, see [`crate::Kernel::send`]).
    NoMethod(String),
    /// The method is `A_CANT`: it cannot be reached by a message, only by a direct call.
    NotSendable(String),
    /// The atoms do not satisfy the method's argument types.
    BadArgs { selector: String, reason: String },
    /// There is no inlet at that position.
    NoInlet(usize),
    /// There is no registered class of that name (or it has no `new` method).
    NoClass(String),
}

fn convert_args(types: &[i32], atoms: &[Atom]) -> Result<Vec<Raw>, String> {
    let mut out = Vec::with_capacity(types.len());
    for (i, &t) in types.iter().enumerate() {
        let a = atoms.get(i);
        let raw = match (t as u32, a) {
            (ty::A_LONG | ty::A_DEFLONG, Some(Atom::Long(v))) => Raw::I(*v as usize),
            (ty::A_LONG | ty::A_DEFLONG, Some(Atom::Float(f))) => Raw::I(*f as i64 as usize),
            (ty::A_DEFLONG, None) => Raw::I(0),
            (ty::A_FLOAT | ty::A_DEFFLOAT, Some(Atom::Float(f))) => Raw::F(*f),
            (ty::A_FLOAT | ty::A_DEFFLOAT, Some(Atom::Long(v))) => Raw::F(*v as f64),
            (ty::A_DEFFLOAT, None) => Raw::F(0.0),
            (ty::A_SYM | ty::A_DEFSYM, Some(Atom::Sym(s))) => Raw::I(intern(s.as_bytes()) as usize),
            (ty::A_DEFSYM, None) => Raw::I(intern(b"") as usize),
            (ty::A_OBJ, Some(Atom::Obj(p))) => Raw::I(*p),
            (ty::A_GIMME | ty::A_CANT, _) => {
                return Err("A_GIMME and A_CANT cannot be mixed with other argument types".into());
            }
            (_, Some(a)) => {
                return Err(format!("argument {} ({a:?}) does not match type {t}", i + 1));
            }
            (_, None) => return Err(format!("missing argument {} (type {t})", i + 1)),
        };
        out.push(raw);
    }
    // Max ignores arguments beyond the method's types.
    Ok(out)
}

/// Pick the method for `sel`, with Max's fallbacks: `int` and `float` fall back to each other,
/// then to `list`; a one element `list` goes to `int` or `float`; and anything not understood goes
/// to an `anything` method. Returns the selector to pass to an `A_GIMME` method.
fn resolve(c: &ClassData, sel: &str, atoms: &[Atom]) -> Option<(String, MethodRec)> {
    if let Some(m) = c.methods.get(sel) {
        return Some((sel.to_string(), m.clone()));
    }
    let alts: &[&str] = match sel {
        "int" => &["float", "list"],
        "float" => &["int", "list"],
        "list" => match atoms {
            [Atom::Long(_)] => &["int", "float"],
            [Atom::Float(_)] => &["float", "int"],
            _ => &[],
        },
        _ => &[],
    };
    for alt in alts {
        if let Some(m) = c.methods.get(*alt) {
            return Some((alt.to_string(), m.clone()));
        }
    }
    c.methods
        .get("anything")
        .map(|m| (sel.to_string(), m.clone()))
}

/// Call `rec` (a method of `obj`) with `atoms`: `A_GIMME` methods get `(x, sym, argc, argv)`,
/// typed ones their converted arguments.
fn invoke(obj: usize, selector: &str, rec: &MethodRec, atoms: &[Atom]) -> Result<*mut c_void, SendError> {
    let mut raw_atoms;
    let raws = match rec.types.as_slice() {
        [t] if *t as u32 == ty::A_GIMME => {
            raw_atoms = atoms.iter().map(Atom::to_raw).collect::<Vec<_>>();
            vec![
                Raw::I(obj),
                Raw::I(intern(selector.as_bytes()) as usize),
                Raw::I(raw_atoms.len()),
                Raw::I(raw_atoms.as_mut_ptr() as usize),
            ]
        }
        [t] if *t as u32 == ty::A_CANT => return Err(SendError::NotSendable(selector.to_string())),
        types => {
            let mut raws = vec![Raw::I(obj)];
            raws.extend(convert_args(types, atoms).map_err(|reason| SendError::BadArgs {
                selector: selector.to_string(),
                reason,
            })?);
            raws
        }
    };
    // SAFETY: `rec.fun` was registered as a method with exactly these argument types
    Ok(unsafe { call_raw(rec.fun, &raws) })
}

/// Deliver `sel atoms` to `obj`'s class method. See [`resolve`] for the fallbacks.
pub(crate) fn dispatch(obj: usize, sel: &str, atoms: &[Atom]) -> Result<*mut c_void, SendError> {
    let found = {
        let mut st = state();
        let Some(o) = st.objects.get(&obj) else {
            return Err(SendError::NotLive);
        };
        let class = &st.classes[&o.class];
        let class_name = class.name.clone();
        let found = resolve(class, sel, atoms);
        if found.is_none() {
            st.post_error(obj, format!("{class_name}: doesn't understand \u{201c}{sel}\u{201d}"));
        }
        found
    };
    let Some((used, rec)) = found else {
        return Err(SendError::NoMethod(sel.to_string()));
    };
    let res = invoke(obj, &used, &rec, atoms);
    if let Err(SendError::BadArgs { reason, .. }) = &res {
        let class_name = {
            let st = state();
            st.objects.get(&obj).map(|o| st.classes[&o.class].name.clone())
        };
        state().post_error(obj, format!("{}: bad arguments for message \u{201c}{used}\u{201d}: {reason}", class_name.unwrap_or_default()));
    }
    res
}

/// Look up a class by name in any namespace (`box` first).
pub(crate) fn find_class(name: &str) -> Option<usize> {
    find_class_in(&state(), name)
}

/// Create an object by calling the class's `new` method as Max does: `(sym, argc, argv)` for an
/// `A_GIMME` new method, converted arguments for a typed one.
pub(crate) fn create(class_name: &str, atoms: &[Atom]) -> Result<*mut c_void, SendError> {
    let (class_ptr, new_fn, types) = {
        let st = state();
        let found = find_class_in(&st, class_name);
        match found {
            Some(c) if st.classes[&c].new_fn != 0 => {
                (c, st.classes[&c].new_fn, st.classes[&c].new_types.clone())
            }
            _ => return Err(SendError::NoClass(class_name.to_string())),
        }
    };
    let _ = class_ptr;
    let mut raw_atoms;
    let raws = match types.as_slice() {
        [t] if *t as u32 == ty::A_GIMME => {
            raw_atoms = atoms.iter().map(Atom::to_raw).collect::<Vec<_>>();
            vec![
                Raw::I(intern(class_name.as_bytes()) as usize),
                Raw::I(raw_atoms.len()),
                Raw::I(raw_atoms.as_mut_ptr() as usize),
            ]
        }
        types => convert_args(types, atoms).map_err(|reason| SendError::BadArgs {
            selector: "new".into(),
            reason,
        })?,
    };
    // SAFETY: `new_fn` was registered by `class_new` with exactly the argument types in `types`
    Ok(unsafe { call_raw(new_fn, &raws) })
}

fn find_class_in(st: &crate::state::State, name: &str) -> Option<usize> {
    ["box", "nobox"]
        .iter()
        .find_map(|ns| st.class_names.get(&(ns.to_string(), name.to_string())))
        .or_else(|| {
            st.class_names
                .iter()
                .find(|((_, n), _)| n == name)
                .map(|(_, c)| c)
        })
        .copied()
}
