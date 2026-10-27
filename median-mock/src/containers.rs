//! Reference counts, recursive transaction locks, ordered dictionary entries, and atomarrays.
//! The C entry points require live pointers, writable output parameters and valid atom slices,
//! exactly as the SDK does. Arrays have no independent locking contract; a containing dictionary
//! must be transaction-locked while a borrowed array is copied. JSON/file APIs implement a
//! subset for ownership tests; they do not reproduce every Max parser coercion or path lookup.
#![allow(clippy::missing_safety_doc)] // Shared SDK pointer contract above, for these mock exports.

mod io;
pub use io::fail_next_constructor;

use crate::{
    atoms::Atom,
    symbols::{intern, name_of},
};
use max_sys::*;
use parking_lot::{Mutex, ReentrantMutex};
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::{CStr, CString, c_char, c_long, c_void},
    sync::{Arc, LazyLock},
};

#[derive(Clone)]
enum Entry {
    Scalar(Atom),
    String(CString),
    Dict(usize),
    Array(usize),
    Object(usize, c_long),
}
struct Atoms(Vec<t_atom>);
// SAFETY: atoms are copied data; their pointer payloads are opaque/interned. All storage mutation
// is under Body's lock. Returned raw pointers have the C caller's transaction-lock contract.
unsafe impl Send for Atoms {}
enum Body {
    Dict(Vec<(usize, Entry)>),
    Array(Atoms),
}
struct Container {
    body: ReentrantMutex<RefCell<Body>>,
}
struct Record {
    object: Arc<Container>,
    refs: usize,
}
#[derive(Default)]
struct Store {
    objects: HashMap<usize, Record>,
    names: HashMap<usize, usize>,
    next_name: usize,
}
static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| Mutex::new(Store::default()));
const ERR: t_max_err = -1;

/// One-shot SDK failure scenarios for ownership tests. These model selected failure points,
/// not every allocation sequence or rollback guarantee of Max's container implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    DictionaryNew,
    DictionaryClone,
    AtomArrayNew,
    /// Fail before changing the array.
    AtomArraySetAtoms,
    /// The dictionary owns the new child even though the append reports an error.
    AppendDictionaryAfterStore,
    /// The dictionary owns the new array even though the append reports an error.
    AppendAtomArrayAfterStore,
}
static FAILURE: Mutex<Option<Failure>> = Mutex::new(None);

/// Fail the next matching container operation. Hold the kernel test lock while using this.
pub fn fail_next(failure: Failure) {
    *FAILURE.lock() = Some(failure);
}

fn take_failure(expected: Failure) -> bool {
    let mut failure = FAILURE.lock();
    if *failure == Some(expected) {
        *failure = None;
        true
    } else {
        false
    }
}

fn lookup(p: usize) -> Arc<Container> {
    STORE
        .lock()
        .objects
        .get(&p)
        .expect("live container")
        .object
        .clone()
}
fn alloc(body: Body) -> usize {
    // Allocate a full opaque header so clients can use its address as a Max object.
    let p = unsafe { crate::sysmem::sysmem_newptrclear(std::mem::size_of::<t_dictionary>() as _) }
        as usize;
    STORE.lock().objects.insert(
        p,
        Record {
            object: Arc::new(Container {
                body: ReentrantMutex::new(RefCell::new(body)),
            }),
            refs: 1,
        },
    );
    p
}
fn release_entry(e: Entry) {
    match e {
        Entry::Object(_, flags)
            if flags
                & (e_max_datastore_flags::OBJ_FLAG_REF | e_max_datastore_flags::OBJ_FLAG_DATA)
                    as c_long
                != 0 => {}
        Entry::Dict(p) | Entry::Array(p) => unsafe {
            crate::class::object_free(p as *mut c_void);
        },
        Entry::Object(p, 0) => unsafe {
            crate::class::object_free(p as *mut c_void);
        },
        Entry::Object(p, flags)
            if flags & e_max_datastore_flags::OBJ_FLAG_MEMORY as c_long != 0 =>
        unsafe {
            crate::sysmem::sysmem_freeptr(p as *mut c_void);
        },
        _ => (),
    }
}
pub(crate) fn release(p: usize) -> bool {
    let object = {
        let mut s = STORE.lock();
        let Some(r) = s.objects.get_mut(&p) else {
            return false;
        };
        r.refs -= 1;
        if r.refs != 0 {
            return true;
        }
        s.names.retain(|_, v| *v != p);
        s.objects.remove(&p).unwrap().object
    };
    crate::notify::unregister_addr(p);
    let lock = object.body.lock();
    if let Body::Dict(entries) = &mut *lock.borrow_mut() {
        for (_, e) in std::mem::take(entries) {
            release_entry(e);
        }
    }
    unsafe {
        crate::sysmem::sysmem_freeptr(p as *mut c_void);
    }
    true
}
pub(crate) fn reset() {
    io::reset();
    *FAILURE.lock() = None;
    let objects = {
        let mut s = STORE.lock();
        s.names.clear();
        std::mem::take(&mut s.objects)
    };
    for p in objects.keys() {
        unsafe {
            crate::sysmem::sysmem_freeptr(*p as *mut c_void);
        }
    }
}
pub(crate) fn class_name(p: usize) -> Option<&'static str> {
    let object = STORE.lock().objects.get(&p)?.object.clone();
    let lock = object.body.lock();
    let name = match &*lock.borrow() {
        Body::Dict(_) => "dictionary",
        Body::Array(_) => "atomarray",
    };
    Some(name)
}

/// Number of live container objects (for ownership/leak tests).
pub fn live_count() -> usize {
    STORE.lock().objects.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_retain(p: *mut t_object) -> t_max_err {
    let mut s = STORE.lock();
    if let Some(r) = s.objects.get_mut(&(p as usize)) {
        r.refs += 1;
        return 0;
    }
    drop(s);
    // any other live object keeps a count of its own, see `Kernel::retain_count`
    match crate::state::state().objects.get_mut(&(p as usize)) {
        Some(o) => {
            o.extra_refs += 1;
            0
        }
        None => ERR,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_new() -> *mut t_dictionary {
    if take_failure(Failure::DictionaryNew) {
        return std::ptr::null_mut();
    }
    alloc(Body::Dict(Vec::new())) as _
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_transaction_lock(d: *mut t_dictionary) -> c_long {
    let object = lookup(d as usize);
    std::mem::forget(object.body.lock());
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_transaction_unlock(d: *mut t_dictionary) -> c_long {
    unsafe {
        lookup(d as usize).body.force_unlock();
    }
    0
}
fn append(d: usize, k: usize, value: Entry) -> t_max_err {
    let object = lookup(d);
    let lock = object.body.lock();
    let old = {
        let mut body = lock.borrow_mut();
        let Body::Dict(entries) = &mut *body else {
            return ERR;
        };
        if let Some((_, e)) = entries.iter_mut().find(|(key, _)| *key == k) {
            Some(std::mem::replace(e, value))
        } else {
            entries.push((k, value));
            None
        }
    };
    if let Some(e) = old {
        release_entry(e);
    }
    0
}
fn entry(d: usize, k: usize) -> Option<Entry> {
    let object = lookup(d);
    let lock = object.body.lock();
    let body = lock.borrow();
    match &*body {
        Body::Dict(entries) => entries
            .iter()
            .find(|(key, _)| *key == k)
            .map(|(_, e)| e.clone()),
        _ => None,
    }
}
macro_rules! append_scalar {
    ($name:ident, $ty:ty, $variant:ident) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            d: *mut t_dictionary,
            k: *mut t_symbol,
            v: $ty,
        ) -> t_max_err {
            append(
                d as usize,
                k as usize,
                Entry::Scalar(Atom::$variant(v as _)),
            )
        }
    };
}
append_scalar!(dictionary_appendlong, t_atom_long, Long);
append_scalar!(dictionary_appendfloat, f64, Float);
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_appendsym(
    d: *mut t_dictionary,
    k: *mut t_symbol,
    v: *mut t_symbol,
) -> t_max_err {
    append(
        d as usize,
        k as usize,
        Entry::Scalar(Atom::Sym(name_of(v as usize))),
    )
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_appendstring(
    d: *mut t_dictionary,
    k: *mut t_symbol,
    v: *const c_char,
) -> t_max_err {
    append(
        d as usize,
        k as usize,
        Entry::String(unsafe { CStr::from_ptr(v) }.to_owned()),
    )
}
macro_rules! append_object {
    ($name:ident, $variant:ident, $failure:ident) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            d: *mut t_dictionary,
            k: *mut t_symbol,
            v: *mut t_object,
        ) -> t_max_err {
            let result = append(d as usize, k as usize, Entry::$variant(v as usize));
            if take_failure(Failure::$failure) {
                ERR
            } else {
                result
            }
        }
    };
}
append_object!(
    dictionary_appenddictionary,
    Dict,
    AppendDictionaryAfterStore
);
append_object!(dictionary_appendatomarray, Array, AppendAtomArrayAfterStore);
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_appendobject_flags(
    d: *mut t_dictionary,
    k: *mut t_symbol,
    v: *mut t_object,
    flags: c_long,
) -> t_max_err {
    append(d as usize, k as usize, Entry::Object(v as usize, flags))
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_appendatoms(
    d: *mut t_dictionary,
    k: *mut t_symbol,
    n: c_long,
    atoms: *mut t_atom,
) -> t_max_err {
    let a = unsafe { atomarray_new(n, atoms) };
    if a.is_null() {
        return ERR;
    }
    append(d as usize, k as usize, Entry::Array(a as usize))
}
macro_rules! predicate {
    ($name:ident, $pattern:pat) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(d: *const t_dictionary, k: *mut t_symbol) -> c_long {
            matches!(entry(d as usize, k as usize), $pattern) as c_long
        }
    };
}
predicate!(dictionary_entryisstring, Some(Entry::String(_)));
predicate!(dictionary_entryisdictionary, Some(Entry::Dict(_)));
predicate!(dictionary_entryisatomarray, Some(Entry::Array(_)));
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_hasentry(d: *const t_dictionary, k: *mut t_symbol) -> c_long {
    entry(d as usize, k as usize).is_some() as c_long
}
macro_rules! get_object {
    ($name:ident, $variant:ident) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            d: *const t_dictionary,
            k: *mut t_symbol,
            out: *mut *mut t_object,
        ) -> t_max_err {
            if let Some(Entry::$variant(p)) = entry(d as usize, k as usize) {
                unsafe {
                    *out = p as _;
                }
                0
            } else {
                ERR
            }
        }
    };
}
get_object!(dictionary_getdictionary, Dict);
get_object!(dictionary_getatomarray, Array);
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_getstring(
    d: *const t_dictionary,
    k: *mut t_symbol,
    out: *mut *const c_char,
) -> t_max_err {
    let object = lookup(d as usize);
    let lock = object.body.lock();
    let body = lock.borrow();
    if let Body::Dict(entries) = &*body {
        if let Some((_, Entry::String(s))) = entries.iter().find(|(key, _)| *key == k as usize) {
            unsafe {
                *out = s.as_ptr();
            }
            return 0;
        }
    }
    ERR
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_getatom(
    d: *const t_dictionary,
    k: *mut t_symbol,
    out: *mut t_atom,
) -> t_max_err {
    let object = lookup(d as usize);
    let lock = object.body.lock();
    let mut body = lock.borrow_mut();
    let Body::Dict(entries) = &mut *body else {
        return ERR;
    };
    let Some((_, e)) = entries.iter_mut().find(|(key, _)| *key == k as usize) else {
        return ERR;
    };
    // Model the SDK's dangerous conversion on read so tests catch accidental getatom(string).
    if let Entry::String(s) = e {
        *e = Entry::Scalar(Atom::Sym(s.to_string_lossy().into_owned()));
    }
    let a = match e {
        Entry::Scalar(a) => a.to_raw(),
        Entry::Dict(p) | Entry::Array(p) | Entry::Object(p, _) => Atom::Obj(*p).to_raw(),
        Entry::String(_) => unreachable!(),
    };
    unsafe {
        *out = a;
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_getentrycount(d: *const t_dictionary) -> t_atom_long {
    let object = lookup(d as usize);
    let lock = object.body.lock();
    let body = lock.borrow();
    match &*body {
        Body::Dict(e) => e.len() as _,
        _ => 0,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_getkeys_ordered(
    d: *const t_dictionary,
    count: *mut c_long,
    out: *mut *mut *mut t_symbol,
) -> t_max_err {
    let object = lookup(d as usize);
    let lock = object.body.lock();
    let body = lock.borrow();
    let Body::Dict(entries) = &*body else {
        return ERR;
    };
    let p = if entries.is_empty() {
        std::ptr::null_mut()
    } else {
        unsafe {
            crate::sysmem::sysmem_newptr(
                (entries.len() * std::mem::size_of::<*mut t_symbol>()) as _,
            )
        }
        .cast::<*mut t_symbol>()
    };
    for (i, (k, _)) in entries.iter().enumerate() {
        unsafe {
            p.add(i).write(*k as _);
        }
    }
    unsafe {
        *count = entries.len() as _;
        *out = p;
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_freekeys(
    _: *mut t_dictionary,
    _: c_long,
    keys: *mut *mut t_symbol,
) {
    unsafe {
        crate::sysmem::sysmem_freeptr(keys.cast());
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_deleteentry(
    d: *mut t_dictionary,
    k: *mut t_symbol,
) -> t_max_err {
    let object = lookup(d as usize);
    let lock = object.body.lock();
    let value = {
        let mut body = lock.borrow_mut();
        let Body::Dict(entries) = &mut *body else {
            return ERR;
        };
        let Some(i) = entries.iter().position(|(key, _)| *key == k as usize) else {
            return ERR;
        };
        entries.remove(i).1
    };
    release_entry(value);
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_clear(d: *mut t_dictionary) -> t_max_err {
    let object = lookup(d as usize);
    let lock = object.body.lock();
    let values = {
        let mut body = lock.borrow_mut();
        let Body::Dict(e) = &mut *body else {
            return ERR;
        };
        std::mem::take(e)
    };
    for (_, v) in values {
        release_entry(v);
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_clone(d: *mut t_dictionary) -> *mut t_dictionary {
    if take_failure(Failure::DictionaryClone) {
        return std::ptr::null_mut();
    }
    let object = lookup(d as usize);
    let lock = object.body.lock();
    let body = lock.borrow();
    let Body::Dict(entries) = &*body else {
        return std::ptr::null_mut();
    };
    let mut copied = Vec::new();
    for (k, e) in entries {
        let e = match e {
            Entry::Dict(p) => Entry::Dict(unsafe { dictionary_clone(*p as _) } as usize),
            Entry::Array(p) => {
                let object = lookup(*p);
                let lock = object.body.lock();
                let body = lock.borrow();
                let Body::Array(a) = &*body else {
                    unreachable!()
                };
                Entry::Array(alloc(Body::Array(Atoms(a.0.clone()))))
            }
            // an object the kernel cannot clone (a `Memory` entry, or an object with no
            // `object_clone`) becomes a null object entry; the original keeps ownership of its own
            Entry::Object(_, flags)
                if flags
                    & (e_max_datastore_flags::OBJ_FLAG_REF | e_max_datastore_flags::OBJ_FLAG_DATA)
                        as c_long
                    == 0 =>
            {
                Entry::Object(0, 0)
            }
            other => other.clone(),
        };
        copied.push((*k, e));
    }
    alloc(Body::Dict(copied)) as _
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atomarray_new(n: c_long, atoms: *mut t_atom) -> *mut t_atomarray {
    if take_failure(Failure::AtomArrayNew) {
        return std::ptr::null_mut();
    }
    let copied = if n == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(atoms, n as usize) }.to_vec()
    };
    alloc(Body::Array(Atoms(copied))) as _
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atomarray_getatoms(
    a: *mut t_atomarray,
    n: *mut c_long,
    out: *mut *mut t_atom,
) -> t_max_err {
    let object = lookup(a as usize);
    let lock = object.body.lock();
    let body = lock.borrow();
    let Body::Array(atoms) = &*body else {
        return ERR;
    };
    unsafe {
        *n = atoms.0.len() as _;
        *out = atoms.0.as_ptr().cast_mut();
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atomarray_getsize(a: *mut t_atomarray) -> t_atom_long {
    let object = lookup(a as usize);
    let lock = object.body.lock();
    let body = lock.borrow();
    match &*body {
        Body::Array(a) => a.0.len() as _,
        _ => 0,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atomarray_setatoms(
    a: *mut t_atomarray,
    n: c_long,
    atoms: *mut t_atom,
) -> t_max_err {
    if take_failure(Failure::AtomArraySetAtoms) {
        return ERR;
    }
    let copied = if n == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(atoms, n as usize) }.to_vec()
    };
    let object = lookup(a as usize);
    let lock = object.body.lock();
    *lock.borrow_mut() = Body::Array(Atoms(copied));
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atomarray_clear(a: *mut t_atomarray) {
    unsafe {
        atomarray_setatoms(a, 0, std::ptr::null_mut());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictobj_namefromptr(d: *mut t_dictionary) -> *mut t_symbol {
    STORE
        .lock()
        .names
        .iter()
        .find(|(_, p)| **p == d as usize)
        .map_or(std::ptr::null_mut(), |(k, _)| *k as _)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictobj_unregister(d: *mut t_dictionary) -> t_max_err {
    STORE.lock().names.retain(|_, p| *p != d as usize);
    crate::notify::unregister_addr(d as usize);
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictobj_findregistered_retain(name: *mut t_symbol) -> *mut t_dictionary {
    // The registry is the "dictionary" namespace of `object_register`: it holds whatever class
    // registered a name there, and the caller gets the object retained either way.
    let key = ("dictionary".to_string(), name_of(name as usize));
    let Some(&p) = crate::state::state().registry.get(&key) else {
        return std::ptr::null_mut();
    };
    if let Some(r) = STORE.lock().objects.get_mut(&p) {
        r.refs += 1;
    } else if let Some(o) = crate::state::state().objects.get_mut(&p) {
        o.extra_refs += 1;
    } else {
        return std::ptr::null_mut();
    }
    p as _
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictobj_register(
    d: *mut t_dictionary,
    name: *mut *mut t_symbol,
) -> *mut t_dictionary {
    let registry = crate::state::state().registry.clone();
    let mut s = STORE.lock();
    let mut n = unsafe { *name };
    if n.is_null() || name_of(n as usize).is_empty() {
        loop {
            s.next_name += 1;
            n = intern(format!("u{}", s.next_name).as_bytes());
            if !s.names.contains_key(&(n as usize))
                && !registry.contains_key(&("dictionary".into(), name_of(n as usize)))
            {
                break;
            }
        }
        unsafe {
            *name = n;
        }
    }
    if name_of(n as usize).contains(char::is_whitespace) {
        return d;
    }
    if let Some(&existing) = s.names.get(&(n as usize)) {
        s.objects.get_mut(&existing).unwrap().refs += 1;
        drop(s);
        if existing != d as usize {
            release(d as usize);
        }
        return existing as _;
    }
    if registry.contains_key(&("dictionary".into(), name_of(n as usize))) {
        drop(s);
        release(d as usize);
        return std::ptr::null_mut();
    }
    s.names.insert(n as usize, d as usize);
    drop(s);
    crate::notify::register_with_class(
        "dictionary",
        &name_of(n as usize),
        d as usize,
        Some("dictionary"),
    );
    d
}

fn atom_container(atom: &t_atom) -> Option<Arc<Container>> {
    let Atom::Obj(p) = Atom::from_raw(atom) else {
        return None;
    };
    STORE.lock().objects.get(&p).map(|r| r.object.clone())
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atomisdictionary(atom: *mut t_atom) -> c_long {
    atom_container(unsafe { &*atom })
        .is_some_and(|object| matches!(*object.body.lock().borrow(), Body::Dict(_))) as c_long
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atomisatomarray(atom: *mut t_atom) -> c_long {
    atom_container(unsafe { &*atom })
        .is_some_and(|object| matches!(*object.body.lock().borrow(), Body::Array(_))) as c_long
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn injected_failures_are_one_shot_and_post_store_errors_transfer_ownership() {
        let _kernel = crate::lock();
        unsafe {
            fail_next(Failure::DictionaryNew);
            assert!(dictionary_new().is_null());
            let d = dictionary_new();
            assert!(!d.is_null());
            fail_next(Failure::DictionaryClone);
            assert!(dictionary_clone(d).is_null());
            let copy = dictionary_clone(d);
            assert!(!copy.is_null());
            release(copy as usize);

            fail_next(Failure::AtomArrayNew);
            assert!(atomarray_new(0, std::ptr::null_mut()).is_null());
            let array = atomarray_new(0, std::ptr::null_mut());
            assert!(!array.is_null());
            let mut value = Atom::Long(42).to_raw();
            fail_next(Failure::AtomArraySetAtoms);
            assert_eq!(atomarray_setatoms(array, 1, &mut value), ERR);
            assert_eq!(atomarray_getsize(array), 0);
            assert_eq!(atomarray_setatoms(array, 1, &mut value), 0);
            assert_eq!(atomarray_getsize(array), 1);

            fail_next(Failure::AtomArrayNew);
            let key = intern(b"failed-array");
            assert_eq!(dictionary_appendatoms(d, key, 1, &mut value), ERR);
            assert_eq!(dictionary_hasentry(d, key), 0);
            assert_eq!(live_count(), 2);

            let child = dictionary_new();
            fail_next(Failure::AppendDictionaryAfterStore);
            assert_eq!(
                dictionary_appenddictionary(d, intern(b"child"), child.cast()),
                ERR
            );
            assert!(
                matches!(entry(d as usize, intern(b"child") as usize), Some(Entry::Dict(p)) if p == child as usize)
            );
            fail_next(Failure::AppendAtomArrayAfterStore);
            assert_eq!(
                dictionary_appendatomarray(d, intern(b"array"), array.cast()),
                ERR
            );
            assert!(
                matches!(entry(d as usize, intern(b"array") as usize), Some(Entry::Array(p)) if p == array as usize)
            );
            assert_eq!(live_count(), 3);
            release(d as usize);
            assert_eq!(live_count(), 0);
        }
    }

    #[test]
    fn borrowed_entry_flags_suppress_release_even_with_memory_flag() {
        let _k = crate::lock();
        for flags in [
            e_max_datastore_flags::OBJ_FLAG_REF,
            e_max_datastore_flags::OBJ_FLAG_DATA,
        ] {
            let ptr = alloc(Body::Dict(Vec::new()));
            release_entry(Entry::Object(
                ptr,
                (flags | e_max_datastore_flags::OBJ_FLAG_MEMORY) as c_long,
            ));
            assert_eq!(live_count(), 1);
            release(ptr);
        }
        let ptr = alloc(Body::Dict(Vec::new()));
        release_entry(Entry::Object(ptr, 0));
        assert_eq!(live_count(), 0);
        let ptr = unsafe { crate::sysmem::sysmem_newptr(16) };
        release_entry(Entry::Object(
            ptr as usize,
            e_max_datastore_flags::OBJ_FLAG_MEMORY as c_long,
        ));
    }
    #[test]
    fn cloning_a_dictionary_turns_memory_entries_into_null_objects() {
        let _k = crate::lock();
        unsafe {
            let d = dictionary_new();
            let ptr = crate::sysmem::sysmem_newptr(16);
            let flags = e_max_datastore_flags::OBJ_FLAG_MEMORY as c_long;
            assert_eq!(
                dictionary_appendobject_flags(d, intern(b"mem"), ptr.cast(), flags),
                0
            );
            let c = dictionary_clone(d);
            let object = lookup(c as usize);
            {
                let lock = object.body.lock();
                let body = lock.borrow();
                let Body::Dict(entries) = &*body else {
                    unreachable!()
                };
                assert!(matches!(entries[..], [(_, Entry::Object(0, 0))]));
            }
            drop(object);
            // each dictionary frees only what it owns: one `sysmem_freeptr`, not two
            assert!(release(c as usize));
            assert!(release(d as usize));
        }
    }
    #[test]
    fn ownership_registry_and_string_conversion_match_sdk() {
        let _kernel = crate::lock();
        unsafe {
            let d = dictionary_new();
            let child = dictionary_new();
            let key = intern(b"child");
            assert_eq!(object_retain(child.cast()), 0);
            dictionary_appenddictionary(d, key, child.cast());
            assert!(release(d as usize));
            assert_eq!(live_count(), 1);
            let mut name = std::ptr::null_mut();
            assert_eq!(dictobj_register(child, &mut name), child);
            let second = dictionary_new();
            assert_eq!(dictobj_register(second, &mut name), child);
            assert_eq!(live_count(), 1);
            dictionary_appendstring(child, key, c"text".as_ptr());
            assert_eq!(dictionary_entryisstring(child, key), 1);
            let mut atom = Atom::Long(0).to_raw();
            dictionary_getatom(child, key, &mut atom);
            assert_eq!(dictionary_entryisstring(child, key), 0);
            assert_eq!(Atom::from_raw(&atom), Atom::Sym("text".into()));
            release(child as usize);
            release(child as usize);
            assert!(dictobj_findregistered_retain(name).is_null());
            assert_eq!(live_count(), 0);
        }
    }
}
