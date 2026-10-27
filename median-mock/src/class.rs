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

/// What Max returns from `object_getmethod` for a missing method (`ext_obex.h`: "returns the
/// method if successful, or method_false() if unsuccessful"), and what `class_method` returned
/// for a missing method on 2026-10-27 in Max 9.2 ARM64: a function that does nothing and
/// returns 0.
///
/// # Safety
/// Any argument.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn method_false(_x: *mut c_void) -> max_sys::t_atom_long {
    0
}

/// Look up a method on a registered or still-being-built class. For a method the class does
/// not have, this returns [`method_false`], not null, as Max does; use the class' `methods`
/// hashtab (`class_extra_lookup`, `hashtab_lookup`) to tell whether a method exists.
///
/// # Safety
/// `c` is a live class and `name` an interned symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn class_method(c: *mut t_class, name: *mut t_symbol) -> method {
    let address = state()
        .classes
        .get(&(c as usize))
        .and_then(|class| class.methods.get(&name_of(name as usize)))
        .map_or(method_false as *const () as usize, |m| m.fun);
    Some(unsafe {
        std::mem::transmute::<usize, unsafe extern "C" fn(*mut c_void) -> *mut c_void>(address)
    })
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
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
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
            methods_table: 0,
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
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
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
            obj: 0,
            attrs: Default::default(),
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
// method attributes: what `CLASS_METHOD_ATTR_PARSE` expands to

/// Only the `methods` hashtab is known: it gives a pointer that [`hashtab_lookup`] accepts.
/// Anything else is null.
///
/// # Safety
/// `x` must be a class from `class_new` (anything else gives null), `s` a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn class_extra_lookup(x: *mut t_class, s: *mut t_symbol) -> *mut c_void {
    // SAFETY: the caller's contract
    if unsafe { sym_name(s) } != "methods" {
        return std::ptr::null_mut();
    }
    let mut st = state();
    let Some(class) = st.classes.get(&(x as usize)) else {
        return std::ptr::null_mut();
    };
    if class.methods_table != 0 {
        return class.methods_table as *mut c_void;
    }
    let table = st.alloc_sub(SubKind::MethodsTable { class: x as usize });
    st.classes
        .get_mut(&(x as usize))
        .expect("checked above")
        .methods_table = table;
    table as *mut c_void
}

/// Only for the `methods` hashtab of a class (from [`class_extra_lookup`]): `val` gets the method
/// object of the method `key`, the same pointer every time, or null and `MAX_ERR_GENERIC` if
/// the class has no such method. Any other hashtab is `MAX_ERR_INVALID_PTR`.
///
/// # Safety
/// `key` must be a symbol and `val` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hashtab_lookup(
    x: *mut max_sys::t_hashtab,
    key: *mut t_symbol,
    val: *mut *mut t_object,
) -> t_max_err {
    // SAFETY: the caller's contract
    let name = unsafe { sym_name(key) };
    let mut st = state();
    let class = match st.subs.get(&(x as usize)) {
        Some(crate::state::Sub {
            kind: SubKind::MethodsTable { class },
            ..
        }) => *class,
        _ => {
            st.post_error(
                0,
                format!("median-mock: hashtab_lookup on {x:?}, which is not a methods hashtab"),
            );
            return max_err(err::MAX_ERR_INVALID_PTR);
        }
    };
    let found = st
        .classes
        .get(&class)
        .and_then(|c| c.methods.get(&name))
        .map(|m| m.obj);
    let obj = match found {
        None => 0,
        Some(0) => {
            let obj = st.alloc_sub(SubKind::Method {
                class,
                name: name.clone(),
            });
            let rec = st
                .classes
                .get_mut(&class)
                .and_then(|c| c.methods.get_mut(&name))
                .expect("found above");
            rec.obj = obj;
            obj
        }
        Some(obj) => obj,
    };
    if !val.is_null() {
        // SAFETY: the caller's contract
        unsafe { val.write(obj as *mut t_object) };
    }
    if obj == 0 {
        max_err(err::MAX_ERR_GENERIC)
    } else {
        max_err(err::MAX_ERR_NONE)
    }
}

/// For method objects (from [`hashtab_lookup`]): records the attribute with its type and parse
/// string, which [`crate::Kernel::method_attr`] reads back; the string is not parsed. For
/// attribute objects (from `attribute_new`/`attr_offset_new`): records the attribute of the
/// attribute, with the string split as [`crate::attr::parse_atoms`] does, which
/// [`crate::Kernel::attr_attr`] reads back.
///
/// # Safety
/// `attrname` and `parsestr` must be NUL-terminated strings, `type_` a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_addattr_parse(
    x: *mut t_object,
    attrname: *const c_char,
    type_: *mut t_symbol,
    _flags: c_long,
    parsestr: *const c_char,
) -> t_max_err {
    if attrname.is_null() {
        return max_err(err::MAX_ERR_INVALID_PTR);
    }
    // SAFETY: the caller's contract
    let (attr, ty_name) = unsafe {
        (
            CStr::from_ptr(attrname).to_string_lossy().into_owned(),
            sym_name(type_),
        )
    };
    let parse = if parsestr.is_null() {
        String::new()
    } else {
        // SAFETY: the caller's contract
        unsafe { CStr::from_ptr(parsestr) }
            .to_string_lossy()
            .into_owned()
    };
    let mut st = state();
    let (class, name) = match st.subs.get(&(x as usize)) {
        Some(crate::state::Sub {
            kind: SubKind::Method { class, name },
            ..
        }) => (*class, name.clone()),
        Some(crate::state::Sub {
            kind: SubKind::Attr(_),
            ..
        }) => {
            drop(st);
            let atoms = crate::attr::parse_atoms(&parse);
            return match crate::attr::add_attr_attr(x as usize, attr, ty_name, atoms) {
                Ok(()) => max_err(err::MAX_ERR_NONE),
                Err(()) => max_err(err::MAX_ERR_INVALID_PTR),
            };
        }
        _ => {
            st.post_error(0, format!("median-mock: object_addattr_parse on {x:?}: only method and attribute objects are supported"));
            return max_err(err::MAX_ERR_INVALID_PTR);
        }
    };
    match st
        .classes
        .get_mut(&class)
        .and_then(|c| c.methods.get_mut(&name))
    {
        Some(rec) => {
            rec.attrs.insert(attr, (ty_name, parse));
            max_err(err::MAX_ERR_NONE)
        }
        None => max_err(err::MAX_ERR_INVALID_PTR),
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
            extra_refs: 0,
        },
    );
    obj as *mut c_void
}

/// The class name of an object made by `object_alloc`, or of a dictionary or atomarray; null for
/// anything else.
///
/// # Safety
/// `x` is a pointer the kernel handed out, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_classname(x: *mut c_void) -> *mut t_symbol {
    let name = {
        let st = state();
        st.objects
            .get(&(x as usize))
            .map(|o| st.classes[&o.class].name.clone())
    };
    match name
        .as_deref()
        .or_else(|| crate::containers::class_name(x as usize))
    {
        Some(name) => intern(name.as_bytes()),
        None => std::ptr::null_mut(),
    }
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
    if crate::containers::release(x as usize) {
        return 0;
    }
    let addr = x as usize;
    enum Kind {
        Object(usize),
        Sub,
        DeadSub,
        Unknown,
    }
    let kind = {
        let mut st = state();
        if let Some(o) = st.objects.get_mut(&addr) {
            if o.extra_refs > 0 {
                // a retained object: this is only a release
                o.extra_refs -= 1;
                return max_err(err::MAX_ERR_NONE);
            }
            let o = &st.objects[&addr];
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
    // Max frees the inlets and outlets with their owner, but a proxy is an object of its own
    // that the owner must free (in its free method): one still alive here is leaked.
    let leaked: Vec<usize> = obj
        .inlets
        .iter()
        .copied()
        .filter(|p| {
            matches!(st.subs.get(p), Some(s) if s.alive
                && matches!(s.kind, SubKind::Inlet { kind: crate::state::InletKind::Proxy { .. } }))
        })
        .collect();
    if obj.signal_inlets.is_some() {
        st.post_error(
            addr,
            format!("median-mock: {addr:#x} was freed without z_dsp_free after z_dsp_setup"),
        );
    }
    for p in leaked {
        st.post_error(
            addr,
            format!("median-mock: proxy {p:#x} was not freed before its owner {addr:#x}"),
        );
    }
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

// Argument kinds shared with c/variadic.c. A_CANT has no signature: never guess its arity.
const METHOD_PTR: c_int = 1;
const METHOD_INT: c_int = 2;
const METHOD_FLOAT: c_int = 3;
const METHOD_LONG: c_int = 4;
const MAX_METHOD_ARGS: usize = 5; // call_raw supports six arguments including the receiver.

fn object_method_signature(x: usize, s: usize) -> Result<(Option<usize>, Vec<c_int>), String> {
    let sel = name_of(s);
    let st = state();
    if st.buffers.values().any(|b| b.ptr == x) && sel == "getname" {
        return Ok((None, vec![METHOD_PTR]));
    }
    let obj = st
        .objects
        .get(&x)
        .ok_or_else(|| "receiver is not a live object".to_owned())?;
    let rec = st.classes[&obj.class]
        .methods
        .get(&sel)
        .ok_or_else(|| format!("no method \"{sel}\""))?;
    let kinds = if rec.types.as_slice() == [ty::A_GIMME as i32] {
        vec![METHOD_PTR, METHOD_LONG, METHOD_PTR]
    } else if rec.types.as_slice() == [ty::A_GIMMEBACK as i32] {
        vec![METHOD_PTR, METHOD_LONG, METHOD_PTR, METHOD_PTR]
    } else {
        rec.types.iter().map(|&t| match t as u32 {
            ty::A_LONG | ty::A_DEFLONG => Ok(METHOD_INT),
            ty::A_FLOAT | ty::A_DEFFLOAT => Ok(METHOD_FLOAT),
            ty::A_SYM | ty::A_DEFSYM | ty::A_OBJ => Ok(METHOD_PTR),
            _ => Err(format!("method \"{sel}\" has no supported argument signature (A_CANT requires a direct typed call)")),
        }).collect::<Result<Vec<_>, _>>()?
    };
    if kinds.len() > MAX_METHOD_ARGS {
        return Err(format!(
            "method \"{sel}\" exceeds {MAX_METHOD_ARGS} arguments"
        ));
    }
    Ok((Some(rec.fun), kinds))
}

/// Describe the arguments before the C shim reads any variadic slots. Unknown signatures
/// are reported and rejected, including A_CANT (which specifies neither types nor arity).
///
/// # Safety
/// `s` is an interned symbol and `kinds` has room for five ints.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn median_mock_object_method_signature(
    x: *mut c_void,
    s: *mut c_void,
    kinds: *mut c_int,
) -> c_int {
    match object_method_signature(x as usize, s as usize) {
        Ok((_, signature)) => {
            unsafe {
                std::ptr::copy_nonoverlapping(signature.as_ptr(), kinds, signature.len());
            }
            signature.len() as c_int
        }
        Err(e) => {
            state().post_error(x as usize, format!("median-mock: object_method: {e}"));
            -1
        }
    }
}

/// Dispatch arguments collected with the registered ABI. Supports up to five typed arguments,
/// A_GIMME's explicit `(symbol, long, atoms)` arguments, A_GIMMEBACK's `(symbol, long, atoms, rv)`
/// (the result is its `t_max_err`), and buffer~'s `getname(symbol**)`.
/// Default argument types still require actual arguments: this raw API performs no defaulting.
///
/// # Safety
/// `x` and `s` identify the same live method inspected by the signature query. Both arrays have
/// five initialized elements; each argument occupies its index in the corresponding array.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn median_mock_object_method(
    x: *mut c_void,
    s: *mut c_void,
    integers: *const usize,
    floats: *const f64,
) -> *mut c_void {
    let Ok((fun, kinds)) = object_method_signature(x as usize, s as usize) else {
        return std::ptr::null_mut();
    };
    let integers = unsafe { std::slice::from_raw_parts(integers, MAX_METHOD_ARGS) };
    let floats = unsafe { std::slice::from_raw_parts(floats, MAX_METHOD_ARGS) };
    if let Some(fun) = fun {
        let mut args = vec![Raw::I(x as usize)];
        args.extend(kinds.iter().enumerate().map(|(i, &kind)| {
            if kind == METHOD_FLOAT {
                Raw::F(floats[i])
            } else {
                Raw::I(integers[i])
            }
        }));
        unsafe { call_raw(fun, &args) }
    } else {
        let name = state()
            .buffers
            .values()
            .find(|b| b.ptr == x as usize)
            .map(|b| intern(b.name.as_bytes()));
        if let Some(name) = name.filter(|_| integers[0] != 0) {
            unsafe {
                (integers[0] as *mut *mut t_symbol).write(name);
            }
        }
        std::ptr::null_mut()
    }
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
    /// The method is `A_GIMMEBACK`: a message from a patcher cannot reach it, only a typed call
    /// (`object_method_typed`, [`crate::Kernel::call`]). This models an assumption about Max
    /// that no header states, so it is a different case from `NotSendable`, whose `A_CANT`
    /// meaning is documented.
    TypedOnly(String),
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
                return Err(format!(
                    "argument {} ({a:?}) does not match type {t}",
                    i + 1
                ));
            }
            (_, None) => return Err(format!("missing argument {} (type {t})", i + 1)),
        };
        out.push(raw);
    }
    // Max ignores arguments beyond the method's types.
    Ok(out)
}

/// Pick the method for `sel`: `int` falls back to `list` (unverified); a
/// one element `list` goes to `int` or `float` (a float element only to `float`); and any other
/// selector not understood goes to an `anything` method. `float` has no fallback at all: checked in
/// Max (2026-10-22), `simp` (methods `int`, `list` and `anything`, no `float`) posts
/// `doesn't understand "float"`. A float-only class rejects `int` in Max (2026-10-26);
/// int-to-list/anything and list-to-number fallbacks remain unverified. Returns the selector
/// to pass to an `A_GIMME` method.
fn resolve(c: &ClassData, sel: &str, atoms: &[Atom]) -> Option<(String, MethodRec)> {
    if let Some(m) = c.methods.get(sel) {
        return Some((sel.to_string(), m.clone()));
    }
    let alts: &[&str] = match sel {
        "int" => &["list"],
        "float" => return None,
        "list" => match atoms {
            [Atom::Long(_)] => &["int", "float"],
            [Atom::Float(_)] => &["float"],
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
/// typed ones their converted arguments. An `A_GIMMEBACK` method is refused
/// ([`SendError::TypedOnly`]): see [`object_method_typed`] for the way to call it.
fn invoke(
    obj: usize,
    selector: &str,
    rec: &MethodRec,
    atoms: &[Atom],
) -> Result<*mut c_void, SendError> {
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
        [t] if *t as u32 == ty::A_GIMMEBACK => {
            return Err(SendError::TypedOnly(selector.to_string()));
        }
        types => {
            let mut raws = vec![Raw::I(obj)];
            raws.extend(
                convert_args(types, atoms).map_err(|reason| SendError::BadArgs {
                    selector: selector.to_string(),
                    reason,
                })?,
            );
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
            st.post_error(
                obj,
                format!("{class_name}: doesn't understand \u{201c}{sel}\u{201d}"),
            );
        }
        found
    };
    let Some((used, rec)) = found else {
        return Err(SendError::NoMethod(sel.to_string()));
    };
    let res = invoke(obj, &used, &rec, atoms);
    if let Err(SendError::TypedOnly(_)) = &res {
        let class_name = {
            let st = state();
            st.objects
                .get(&obj)
                .map(|o| st.classes[&o.class].name.clone())
        };
        state().post_error(
            obj,
            format!(
                "{}: {used} is a typed-call (A_GIMMEBACK) method; a patcher message cannot reach it",
                class_name.unwrap_or_default()
            ),
        );
    }
    if let Err(SendError::BadArgs { reason, .. }) = &res {
        let class_name = {
            let st = state();
            st.objects
                .get(&obj)
                .map(|o| st.classes[&o.class].name.clone())
        };
        state().post_error(
            obj,
            format!(
                "{}: bad arguments for message \u{201c}{used}\u{201d}: {reason}",
                class_name.unwrap_or_default()
            ),
        );
    }
    res
}

/// Whether `obj` would answer `sel atoms` (see [`resolve`]): `NotLive` or `NoMethod` if not.
pub(crate) fn lookup(obj: usize, sel: &str, atoms: &[Atom]) -> Result<(), SendError> {
    let st = state();
    let Some(o) = st.objects.get(&obj) else {
        return Err(SendError::NotLive);
    };
    resolve(&st.classes[&o.class], sel, atoms)
        .map(|_| ())
        .ok_or_else(|| SendError::NoMethod(sel.to_string()))
}

/// `object_method_typed` (`ext_obex.h`): send `s` with the arguments `av` to `x`, with the result
/// in `rv`. "If the receiver object can respond to the message, `object_method_typed()` returns
/// the result in `rv`. Otherwise, `rv` will contain an `A_NOTHING` atom." The selector is
/// resolved as [`crate::Kernel::send`] does (including its fallbacks). Then:
///
/// * an `A_GIMMEBACK` method is called as `(x, s, ac, av, rv)` and its `t_max_err` is returned
///   (a null `rv` is `MAX_ERR_GENERIC` and the method is not called);
/// * an `A_GIMME` method is called as `(x, s, ac, av)`, and a typed method with its converted
///   arguments (as for a message), and `MAX_ERR_NONE` is returned; `rv` stays `A_NOTHING`;
/// * an unknown selector, arguments that don't fit a typed method, or an `A_CANT` method are
///   `MAX_ERR_GENERIC`; a receiver that is not a live object is `MAX_ERR_INVALID_PTR`.
///
/// Whoever reads an `A_OBJ` reply owns it (the mock doesn't free it): the assumption the SDK's
/// `simplejs` example implies and median builds on.
///
/// # Safety
/// `s` is an interned symbol, `av` is valid for `ac` atoms (or `ac` is 0), and `rv` is null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_method_typed(
    x: *mut c_void,
    s: *mut t_symbol,
    ac: c_long,
    av: *mut max_sys::t_atom,
    rv: *mut max_sys::t_atom,
) -> t_max_err {
    if !rv.is_null() {
        // SAFETY: the caller's contract
        unsafe {
            (*rv).a_type = ty::A_NOTHING as _;
            (*rv).a_w.w_long = 0;
        }
    }
    let obj = x as usize;
    // SAFETY: the caller's contract
    let atoms = unsafe { crate::atoms::atoms_from_raw(ac.max(0) as usize, av) };
    let found = {
        let st = state();
        let Some(o) = st.objects.get(&obj) else {
            return max_err(err::MAX_ERR_INVALID_PTR);
        };
        resolve(&st.classes[&o.class], &name_of(s as usize), &atoms)
    };
    let Some((used, rec)) = found else {
        return max_err(err::MAX_ERR_GENERIC);
    };
    if rec.types.as_slice() == [ty::A_GIMMEBACK as i32] {
        if rv.is_null() {
            return max_err(err::MAX_ERR_GENERIC);
        }
        let raws = [
            Raw::I(obj),
            Raw::I(intern(used.as_bytes()) as usize),
            Raw::I(ac.max(0) as usize),
            Raw::I(av as usize),
            Raw::I(rv as usize),
        ];
        // SAFETY: `rec.fun` was registered `A_GIMMEBACK`: `(x, s, ac, av, rv) -> t_max_err`
        let code = unsafe { call_raw(rec.fun, &raws) };
        return code as isize as t_max_err;
    }
    match invoke(obj, &used, &rec, &atoms) {
        Ok(_) => max_err(err::MAX_ERR_NONE),
        Err(_) => max_err(err::MAX_ERR_GENERIC),
    }
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
