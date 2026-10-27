//! Reference-counted Max dictionaries and owned atomarray builders.
//!
//! These operations allocate and lock; do not call them from the audio thread.
//! Reads copy or retain under the dictionary's transaction lock. `Clone` shares a dictionary;
//! `deep_clone` copies it. Inserting `&Dict` copies, while `insert_shared` explicitly shares.
//! Objects placed in a dictionary by other externals must obey Max's ownership and locking rules.

mod io;

use crate::{
    atom::{Atom, AtomType, AtomValue},
    symbol::SymbolRef,
    thread::SysThread,
};
use core::ffi::{c_long, c_void};
use max_sys as sys;
use std::{
    collections::HashSet,
    ffi::{CStr, CString},
    ptr::NonNull,
};

/// Errors preserve allocation/kernel failures rather than treating them as missing entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictError {
    Kernel(sys::t_max_err),
    Allocation,
    ObjectAtom,
    InvalidAtom,
    InvalidString,
    InvalidPath,
    InvalidData,
    OpaqueObject,
    Cycle,
}
impl std::fmt::Display for DictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "dictionary error: {self:?}")
    }
}
impl std::error::Error for DictError {}
pub type DictResult<T> = Result<T, DictError>;

fn check(code: sys::t_max_err) -> DictResult<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(DictError::Kernel(code))
    }
}
fn non_audio() {
    debug_assert!(
        !SysThread::is_audio(),
        "dictionary operations are not audio-thread safe"
    );
}

/// Ownership of a pointer supplied to [`Dict::from_raw`].
#[derive(Clone, Copy, Debug)]
pub enum Ownership {
    Retained,
    Borrowed,
}

/// Ownership policy for foreign entries inserted with `insert_object`.
#[derive(Clone, Copy, Debug)]
pub enum ObjectFlags {
    /// Transfer one reference, released with `object_free`.
    Owned,
    /// Borrow a live Max object; the dictionary does not release it.
    Reference,
    /// Opaque borrowed data, never released by the dictionary.
    Data,
    /// Transfer an allocation made by `sysmem_newptr`, freed by the dictionary.
    Memory,
}

/// A non-owning object address. Safe code cannot dereference it or insert it into a dictionary.
#[derive(Clone, Copy, Debug)]
pub struct ObjectPtr(*mut c_void);
impl ObjectPtr {
    pub fn as_ptr(self) -> *mut c_void {
        self.0
    }
}

/// A copied value, or a retained dictionary. Object atoms in `Atoms` remain non-owning.
pub enum DictValue {
    Int(sys::t_atom_long),
    Float(f64),
    Symbol(SymbolRef),
    Str(CString),
    Atoms(Vec<Atom>),
    Dict(Dict),
    Object(ObjectPtr),
}

/// Exactly one strong reference to a Max dictionary.
#[derive(Debug)]
pub struct Dict(NonNull<sys::t_dictionary>);
// SAFETY: every pointer-producing read is copied/retained under the recursive SDK lock;
// handles own a reference and no borrowed storage escapes. Mutation uses the same lock.
unsafe impl Send for Dict {}
unsafe impl Sync for Dict {}

// Serialize graph-changing inserts and their cycle checks. Without this lock two threads could
// both check A->B and B->A before either inserts. It does not serialize arbitrary external C code.
static GRAPH: parking_lot::ReentrantMutex<()> = parking_lot::ReentrantMutex::new(());

thread_local! {
    // Transaction scope and dictionaries created privately within it, for lock-order checks.
    static TRANSACTIONS: std::cell::RefCell<Vec<(usize, HashSet<usize>)>> = const { std::cell::RefCell::new(Vec::new()) };
}
fn check_lock_order(ptr: usize) {
    debug_assert!(
        TRANSACTIONS.with(|t| t
            .borrow()
            .iter()
            .all(|(owner, fresh)| *owner == ptr || fresh.contains(&ptr))),
        "a dictionary transaction cannot lock another existing dictionary"
    );
}
fn mark_fresh(ptr: usize) {
    TRANSACTIONS.with(|t| {
        for (_, fresh) in t.borrow_mut().iter_mut() {
            fresh.insert(ptr);
        }
    });
}

struct Locked<'a>(&'a Dict);
impl Drop for Locked<'_> {
    fn drop(&mut self) {
        unsafe {
            sys::dictionary_transaction_unlock(self.0.as_ptr());
        }
    }
}

// Keep the SDK-private retain dependency in one place.
unsafe fn retain(ptr: *mut sys::t_dictionary) -> DictResult<()> {
    check(unsafe { sys::object_retain(ptr.cast()) })
}

impl Dict {
    pub fn new() -> DictResult<Self> {
        non_audio();
        let d = unsafe { Self::from_raw(sys::dictionary_new(), Ownership::Retained)? };
        mark_fresh(d.as_ptr() as usize);
        Ok(d)
    }

    /// Wrap a dictionary pointer; `Borrowed` acquires a reference before returning.
    ///
    /// # Safety
    /// A non-null pointer must be a live Max dictionary with transaction locking enabled.
    /// For `Retained`, transfer exactly one owned reference. For `Borrowed`, the object must
    /// stay alive until this call has retained it. Other users must obey Max's locking and
    /// ownership rules, including for children and arrays held in entries.
    pub unsafe fn from_raw(ptr: *mut sys::t_dictionary, ownership: Ownership) -> DictResult<Self> {
        non_audio();
        let p = NonNull::new(ptr).ok_or(DictError::Allocation)?;
        if matches!(ownership, Ownership::Borrowed) {
            unsafe {
                retain(ptr)?;
            }
        }
        Ok(Self(p))
    }
    pub fn as_ptr(&self) -> *mut sys::t_dictionary {
        self.0.as_ptr()
    }
    pub fn into_raw(self) -> *mut sys::t_dictionary {
        let p = self.as_ptr();
        std::mem::forget(self);
        p
    }
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
    fn lock(&self) -> Locked<'_> {
        non_audio();
        check_lock_order(self.as_ptr() as usize);
        unsafe {
            sys::dictionary_transaction_lock(self.as_ptr());
        }
        Locked(self)
    }
    pub fn try_clone(&self) -> DictResult<Self> {
        unsafe { Self::from_raw(self.as_ptr(), Ownership::Borrowed) }
    }
    /// Deep copy using the SDK. External weak entries remain weak/shared; objects that the SDK
    /// cannot clone become null entries. Dictionaries built with this API have neither.
    pub fn deep_clone(&self) -> DictResult<Self> {
        let _lock = self.lock();
        let d =
            unsafe { Self::from_raw(sys::dictionary_clone(self.as_ptr()), Ownership::Retained)? };
        mark_fresh(d.as_ptr() as usize);
        Ok(d)
    }

    /// Run several operations atomically on this dictionary. The guard unlocks on panic.
    /// Do not access another existing dictionary inside the closure (debug builds check this).
    /// Newly created private dictionaries are allowed. Never wait for a thread that needs this
    /// dictionary or another dictionary transaction. The graph lock precedes the SDK lock.
    ///
    /// The transaction holds the process-wide graph lock for its whole duration, so every other
    /// thread's `transaction`, `insert_child`, `copy_from` and `insert_object` waits for it:
    /// keep `f` short and do not block in it.
    pub fn transaction<R>(&self, f: impl FnOnce(&Dict) -> R) -> R {
        non_audio();
        let _graph = GRAPH.lock();
        let _lock = self.lock();
        TRANSACTIONS.with(|t| {
            t.borrow_mut()
                .push((self.as_ptr() as usize, HashSet::new()))
        });
        struct Scope;
        impl Drop for Scope {
            fn drop(&mut self) {
                TRANSACTIONS.with(|t| {
                    t.borrow_mut().pop();
                });
            }
        }
        let _scope = Scope;
        f(self)
    }

    /// A non-atomic copy across dictionaries: snapshot the source, then insert into the target.
    /// Children are deep-copied. Opaque objects/object atoms are refused before target mutation.
    ///
    /// An entry holding an atomarray of dictionaries (what Max's JSON parser produces for an
    /// array of objects) reads as [`DictValue::Atoms`] holding object atoms, which are refused
    /// with [`DictError::ObjectAtom`] before the target is changed: this fails on such a source
    /// today (see the open items in `doc/plan-containers.md`).
    pub fn merge_from(&self, source: &Dict) -> DictResult<()> {
        self.copy_from(source, false)
    }
    /// [`merge_from`](Self::merge_from) that skips keys already in this dictionary; it refuses
    /// an atomarray of dictionaries in the source in the same way.
    pub fn copy_unique_from(&self, source: &Dict) -> DictResult<()> {
        self.copy_from(source, true)
    }
    /// [`merge_from`](Self::merge_from) with the roles swapped; it refuses an atomarray of
    /// dictionaries in `self` in the same way.
    pub fn copy_to(&self, target: &Dict) -> DictResult<()> {
        target.merge_from(self)
    }
    fn copy_from(&self, source: &Dict, unique: bool) -> DictResult<()> {
        if self.ptr_eq(source) {
            return Ok(());
        }
        let mut snapshot = Vec::new();
        for key in source.keys()? {
            if let Some(value) = source.get(key)? {
                let value = match value {
                    DictValue::Object(_) => return Err(DictError::OpaqueObject),
                    DictValue::Atoms(ref a) => {
                        validate_atoms(a)?;
                        value
                    }
                    DictValue::Dict(d) => DictValue::Dict(d.deep_clone()?),
                    other => other,
                };
                snapshot.push((key, value));
            }
        }
        for (key, value) in snapshot {
            // The source dictionary is never locked at the same time as the destination.
            // The unique-key test is observational: concurrent scalar writes may race it.
            let _graph = GRAPH.lock();
            // Child cycle checking must precede the destination lock.
            if unique && self.contains_key(key) {
                continue;
            }
            self.insert(key, value)?;
        }
        Ok(())
    }
    pub fn insert(&self, key: SymbolRef, value: impl DictInsert) -> DictResult<()> {
        non_audio();
        value.insert_into(self, key)
    }

    /// Insert a foreign object/data pointer with an explicit SDK ownership flag.
    ///
    /// # Safety
    /// `value` must satisfy `flags`' ownership policy. Borrowed objects/data must remain valid
    /// while any dictionary/clone can access the entry. Owned references/allocations transfer
    /// even on an ambiguous SDK failure; do not free them afterwards. Object entries must
    /// obey Max's threading and cloning rules. Do not create dictionary cycles or store an
    /// unlocked shared atomarray. Do not use `Memory` for a Max object.
    ///
    /// Safe calls made later on this dictionary (or on a dictionary that holds it) hand an
    /// `Owned`, `Reference`, `Data` or `Memory` entry's pointer to kernel code that treats it as
    /// a Max object: `get` and `remove` (through `dictionary_entryis*`), the cycle walk of
    /// `insert_child` (`insert`, `insert_shared`) and everything that goes through
    /// `dictionary_clone` (`deep_clone`, `write`, `dump`, `find_registered_clone`, `insert` of a
    /// `&Dict`, `merge_from`). For a `Data` or `Memory` pointer that is not a Max object, the
    /// caller must be sure each of these is sound for it, or never make them. The kernel table in
    /// `doc/plan-containers.md` is the only evidence this crate has for what the kernel does with
    /// such entries; none of it is verified in Max.
    pub unsafe fn insert_object(
        &self,
        key: SymbolRef,
        value: *mut sys::t_object,
        flags: ObjectFlags,
    ) -> DictResult<()> {
        let _graph = GRAPH.lock();
        let _lock = self.lock();
        let flags = match flags {
            ObjectFlags::Owned => 0,
            ObjectFlags::Reference => sys::e_max_datastore_flags::OBJ_FLAG_REF,
            ObjectFlags::Data => sys::e_max_datastore_flags::OBJ_FLAG_DATA,
            ObjectFlags::Memory => sys::e_max_datastore_flags::OBJ_FLAG_MEMORY,
        };
        check(unsafe {
            sys::dictionary_appendobject_flags(self.as_ptr(), key.as_ptr(), value, flags as c_long)
        })
    }
    /// Share a child, rejecting direct and indirect cycles made through this API.
    /// External C writers are outside the graph lock; do not create cycles through them.
    pub fn insert_shared(&self, key: SymbolRef, value: &Dict) -> DictResult<()> {
        let _graph = GRAPH.lock();
        self.insert_child(key, value.try_clone()?)
    }
    fn insert_child(&self, key: SymbolRef, value: Dict) -> DictResult<()> {
        let _graph = GRAPH.lock();
        let mut todo = vec![value.try_clone()?];
        let mut seen = HashSet::new();
        while let Some(d) = todo.pop() {
            if self.ptr_eq(&d) {
                return Err(DictError::Cycle);
            }
            if !seen.insert(d.as_ptr() as usize) {
                continue;
            }
            for k in d.keys()? {
                if let Some(DictValue::Dict(child)) = d.get(k)? {
                    todo.push(child);
                }
            }
        }
        let _lock = self.lock();
        // An allocation failure can happen after the kernel stores the pointer. Never release
        // the transferred reference on error; a possible leak is preferable to a double free.
        check(unsafe {
            sys::dictionary_appenddictionary(self.as_ptr(), key.as_ptr(), value.into_raw().cast())
        })
    }
    /// Insert an arbitrary object with SDK object flags.
    ///
    /// # Safety
    /// The pointer and flags must satisfy `dictionary_appendobject_flags`, including transfer
    /// of an owned reference unless flags specify a weak or memory entry. The object must remain
    /// valid for every operation the flags permit. Do not bypass the cycle/locking rules.
    pub unsafe fn insert_object_raw(
        &self,
        key: SymbolRef,
        ptr: *mut sys::t_object,
        flags: c_long,
    ) -> DictResult<()> {
        let _graph = GRAPH.lock();
        let _lock = self.lock();
        check(unsafe {
            sys::dictionary_appendobject_flags(self.as_ptr(), key.as_ptr(), ptr, flags)
        })
    }
    /// The value of `key`, copied or retained under the lock.
    ///
    /// An entry holding an atomarray of dictionaries (what Max's JSON parser produces for an
    /// array of objects) comes back as [`DictValue::Atoms`] whose object atoms are non-owning
    /// pointers to those dictionaries, valid only while the entry stays in this dictionary;
    /// they are not wrapped as [`Dict`]s. Operations that refuse object atoms
    /// ([`merge_from`](Self::merge_from) and the like) fail with [`DictError::ObjectAtom`].
    pub fn get(&self, key: SymbolRef) -> DictResult<Option<DictValue>> {
        let _lock = self.lock();
        let d = self.as_ptr();
        let k = key.as_ptr();
        unsafe {
            if sys::dictionary_hasentry(d, k) == 0 {
                return Ok(None);
            }
            let value = if sys::dictionary_entryisstring(d, k) != 0 {
                let mut p = std::ptr::null();
                check(sys::dictionary_getstring(d, k, &mut p))?;
                if p.is_null() {
                    return Err(DictError::InvalidData);
                }
                DictValue::Str(CStr::from_ptr(p).to_owned())
            } else if sys::dictionary_entryisdictionary(d, k) != 0 {
                let mut p = std::ptr::null_mut();
                check(sys::dictionary_getdictionary(d, k, &mut p))?;
                DictValue::Dict(Self::from_raw(p.cast(), Ownership::Borrowed)?)
            } else if sys::dictionary_entryisatomarray(d, k) != 0 {
                let mut p = std::ptr::null_mut();
                check(sys::dictionary_getatomarray(d, k, &mut p))?;
                if p.is_null() {
                    return Err(DictError::InvalidData);
                }
                DictValue::Atoms(copy_array(p.cast())?)
            } else {
                let mut atom = Atom::from(0i64);
                check(sys::dictionary_getatom(d, k, &mut atom.value))?;
                match atom.get_value() {
                    Some(AtomValue::Int(v)) => DictValue::Int(v),
                    Some(AtomValue::Float(v)) => DictValue::Float(v),
                    Some(AtomValue::Symbol(v)) => DictValue::Symbol(v),
                    Some(AtomValue::Object(v)) => DictValue::Object(ObjectPtr(v)),
                    None => return Err(DictError::InvalidAtom),
                }
            };
            Ok(Some(value))
        }
    }
    pub fn contains_key(&self, key: SymbolRef) -> bool {
        let _lock = self.lock();
        unsafe { sys::dictionary_hasentry(self.as_ptr(), key.as_ptr()) != 0 }
    }
    pub fn len(&self) -> usize {
        let _lock = self.lock();
        unsafe { sys::dictionary_getentrycount(self.as_ptr()).max(0) as usize }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn keys(&self) -> DictResult<Vec<SymbolRef>> {
        let _lock = self.lock();
        let mut n = 0;
        let mut p = std::ptr::null_mut();
        unsafe {
            let result = check(sys::dictionary_getkeys_ordered(
                self.as_ptr(),
                &mut n,
                &mut p,
            ));
            // Free even when a kernel failure left an allocated key buffer.
            struct Keys(*mut sys::t_dictionary, c_long, *mut *mut sys::t_symbol);
            impl Drop for Keys {
                fn drop(&mut self) {
                    if !self.2.is_null() {
                        unsafe {
                            sys::dictionary_freekeys(self.0, self.1, self.2);
                        }
                    }
                }
            }
            let _keys = Keys(self.as_ptr(), n, p);
            result?;
            if n == 0 {
                return Ok(Vec::new());
            }
            if n < 0 || p.is_null() {
                return Err(DictError::InvalidData);
            }
            Ok(std::slice::from_raw_parts(p, n as usize)
                .iter()
                .map(|&s| SymbolRef::from_raw(s))
                .collect())
        }
    }
    /// Copy/retain the value, then delete it while still locked. Opaque objects are left in place.
    ///
    /// For an entry holding an atomarray of dictionaries (see [`get`](Self::get)) the deletion
    /// frees those dictionaries, so the object atoms in the returned [`DictValue::Atoms`] are
    /// dangling: do not dereference them.
    pub fn remove(&self, key: SymbolRef) -> DictResult<Option<DictValue>> {
        let _lock = self.lock();
        let value = self.get(key)?;
        if matches!(value, Some(DictValue::Object(_))) {
            return Err(DictError::OpaqueObject);
        }
        if value.is_some() {
            check(unsafe { sys::dictionary_deleteentry(self.as_ptr(), key.as_ptr()) })?;
        }
        Ok(value)
    }
    pub fn delete(&self, key: SymbolRef) -> DictResult<bool> {
        let _lock = self.lock();
        if !self.contains_key(key) {
            return Ok(false);
        }
        check(unsafe { sys::dictionary_deleteentry(self.as_ptr(), key.as_ptr()) })?;
        Ok(true)
    }
    pub fn clear(&self) -> DictResult<()> {
        let _lock = self.lock();
        check(unsafe { sys::dictionary_clear(self.as_ptr()) })
    }
    pub fn get_path(&self, path: &str) -> DictResult<Option<DictValue>> {
        let parts: Vec<_> = path.split("::").collect();
        if parts.iter().any(|s| s.is_empty()) {
            return Err(DictError::InvalidPath);
        }
        let mut current = self.try_clone()?;
        for (i, part) in parts.iter().enumerate() {
            let key = SymbolRef::try_from(*part).map_err(|_| DictError::InvalidString)?;
            let value = current.get(key)?;
            if i + 1 == parts.len() {
                return Ok(value);
            }
            match value {
                Some(DictValue::Dict(d)) => current = d,
                _ => return Ok(None),
            }
        }
        Ok(None)
    }
    pub fn name(&self) -> Option<SymbolRef> {
        non_audio();
        let p = unsafe { sys::dictobj_namefromptr(self.as_ptr()) };
        if p.is_null() {
            None
        } else {
            Some(unsafe { SymbolRef::from_raw(p) })
        }
    }
    /// The dictionary registered under `name`, retained. Any class can hold a name in the
    /// registry (it is the `"dictionary"` namespace of `object_register`); a name held by an
    /// object that is not a dictionary yields `None`, and that object's reference is released.
    pub fn find_registered(name: SymbolRef) -> Option<Self> {
        non_audio();
        let p = unsafe { sys::dictobj_findregistered_retain(name.as_ptr()) };
        if p.is_null() {
            return None;
        }
        // SAFETY: `p` is a live object, retained for us (`ext_obex.h`, `object_classname`).
        // That Max's dictionary class is named `dictionary` (`_sym_dictionary`) is unverified
        // in a host; see `doc/validation-max.md`.
        if unsafe { sys::object_classname(p.cast()) } != crate::sym!("dictionary").as_ptr() {
            unsafe { sys::object_free(p.cast()) };
            return None;
        }
        unsafe { Self::from_raw(p, Ownership::Retained).ok() }
    }
    pub fn find_registered_clone(name: SymbolRef) -> DictResult<Option<Self>> {
        Self::find_registered(name)
            .map(|d| d.deep_clone())
            .transpose()
    }
    /// Consume an unregistered dictionary. A name collision returns the existing dictionary,
    /// retained by the kernel, which has released the input. Keep the returned handle alive.
    pub fn register(self, name: Option<SymbolRef>) -> Result<(Self, SymbolRef), RegisterError> {
        let _graph = GRAPH.lock();
        if self.name().is_some() {
            return Err(RegisterError::AlreadyRegistered(self));
        }
        let mut name = name.map_or(std::ptr::null_mut(), |s| s.as_ptr());
        let p = unsafe { sys::dictobj_register(self.into_raw(), &mut name) };
        let Some(ptr) = NonNull::new(p) else {
            return Err(RegisterError::NameTakenByOtherClass);
        };
        let d = Self(ptr);
        match d.name() {
            Some(n) => Ok((d, n)),
            None => Err(RegisterError::InvalidName(d)),
        }
    }
    pub fn unregister(&self) -> DictResult<()> {
        let _graph = GRAPH.lock();
        non_audio();
        check(unsafe { sys::dictobj_unregister(self.as_ptr()) })
    }

    /// Select the dictionary registered under `name`, creating an empty one if necessary.
    /// This follows Max's `dict` name-switching behavior: it does not move/copy entries or
    /// rename the shared dictionary behind other handles. Replace your handle with the result.
    /// Selecting the current name simply retains, without registering it a second time.
    pub fn rename(&self, name: SymbolRef) -> Result<Self, RegisterError> {
        if self.name() == Some(name) {
            return self.try_clone().map_err(RegisterError::Creation);
        }
        Self::new()
            .map_err(RegisterError::Creation)?
            .register(Some(name))
            .map(|(d, _)| d)
    }

    /// Notify subscribers after modifying this dictionary. Delivery may synchronously call
    /// other objects; call this outside a transaction so no dictionary lock spans their code.
    pub fn notify_modified(&self) -> DictResult<()> {
        non_audio();
        debug_assert!(
            TRANSACTIONS.with(|t| t.borrow().is_empty()),
            "notify outside dictionary transactions"
        );
        check(unsafe {
            sys::object_notify(
                self.as_ptr().cast(),
                crate::sym!("modified").as_ptr(),
                std::ptr::null_mut(),
            )
        })
    }

    /// Borrow an atomarray entry while its dictionary remains locked. Other entry kinds
    /// return `None`; no string-to-symbol conversion is performed.
    ///
    /// # Safety
    /// During `f`, do not mutate this dictionary or the array through any handle or callback.
    /// `f` runs with this dictionary's lock held but without the graph lock that `transaction`,
    /// `insert_child`, `copy_from` and `insert_object` take first, so it must not insert a child
    /// into, or run a transaction on, any dictionary: another thread doing so would take the two
    /// locks in the opposite order.
    /// Object atoms are non-owning and must not be dereferenced without their own lifetime
    /// guarantees. The array's contents must follow the SDK's valid-atom contract.
    pub unsafe fn with_atoms_raw<R>(
        &self,
        key: SymbolRef,
        f: impl FnOnce(&[Atom]) -> R,
    ) -> DictResult<Option<R>> {
        let _lock = self.lock();
        if unsafe { sys::dictionary_entryisatomarray(self.as_ptr(), key.as_ptr()) } == 0 {
            return Ok(None);
        }
        let mut array = std::ptr::null_mut();
        check(unsafe { sys::dictionary_getatomarray(self.as_ptr(), key.as_ptr(), &mut array) })?;
        if array.is_null() {
            return Err(DictError::InvalidData);
        }
        let mut count = 0;
        let mut atoms = std::ptr::null_mut();
        check(unsafe { sys::atomarray_getatoms(array.cast(), &mut count, &mut atoms) })?;
        let values = if count == 0 {
            &[]
        } else {
            if count < 0
                || atoms.is_null()
                || count as usize > isize::MAX as usize / std::mem::size_of::<Atom>()
            {
                return Err(DictError::InvalidData);
            }
            unsafe { std::slice::from_raw_parts(atoms.cast::<Atom>(), count as usize) }
        };
        Ok(Some(f(values)))
    }
}
impl Clone for Dict {
    fn clone(&self) -> Self {
        // A live owned reference cannot have a zero retain count. On kernel contract violation,
        // fail without constructing a second owner (FFI callers' guards contain this panic).
        self.try_clone()
            .expect("Max refused to retain a live dictionary")
    }
}
impl Drop for Dict {
    fn drop(&mut self) {
        unsafe {
            sys::object_free(self.as_ptr().cast());
        }
    }
}

#[derive(Debug)]
pub enum RegisterError {
    Creation(DictError),
    AlreadyRegistered(Dict),
    InvalidName(Dict),
    NameTakenByOtherClass,
}

/// Values accepted by [`Dict::insert`].
pub trait DictInsert {
    fn insert_into(self, dict: &Dict, key: SymbolRef) -> DictResult<()>;
}

impl DictInsert for DictValue {
    fn insert_into(self, d: &Dict, k: SymbolRef) -> DictResult<()> {
        match self {
            Self::Int(v) => (v as i64).insert_into(d, k),
            Self::Float(v) => v.insert_into(d, k),
            Self::Symbol(v) => v.insert_into(d, k),
            Self::Str(v) => v.as_c_str().insert_into(d, k),
            Self::Atoms(v) => v.as_slice().insert_into(d, k),
            Self::Dict(v) => v.insert_into(d, k),
            Self::Object(_) => Err(DictError::OpaqueObject),
        }
    }
}

macro_rules! typed_getters {
    ($($name:ident, $variant:ident, $ty:ty;)*) => { impl Dict { $(
        pub fn $name(&self, key: SymbolRef) -> DictResult<Option<$ty>> {
            Ok(match self.get(key)? { Some(DictValue::$variant(v)) => Some(v), _ => None })
        }
    )* } };
}
typed_getters! {
    get_int, Int, sys::t_atom_long;
    get_float, Float, f64;
    get_sym, Symbol, SymbolRef;
    get_string, Str, CString;
    get_atoms, Atoms, Vec<Atom>;
    get_dict, Dict, Dict;
}
macro_rules! scalar {
    ($ty:ty, $func:ident) => {
        impl DictInsert for $ty {
            fn insert_into(self, d: &Dict, k: SymbolRef) -> DictResult<()> {
                let _lock = d.lock();
                check(unsafe { sys::$func(d.as_ptr(), k.as_ptr(), self as _) })
            }
        }
    };
}
scalar!(i64, dictionary_appendlong);
scalar!(i32, dictionary_appendlong);
scalar!(f64, dictionary_appendfloat);
impl DictInsert for SymbolRef {
    fn insert_into(self, d: &Dict, k: SymbolRef) -> DictResult<()> {
        let _lock = d.lock();
        check(unsafe { sys::dictionary_appendsym(d.as_ptr(), k.as_ptr(), self.as_ptr()) })
    }
}
impl DictInsert for &CStr {
    fn insert_into(self, d: &Dict, k: SymbolRef) -> DictResult<()> {
        let _lock = d.lock();
        check(unsafe { sys::dictionary_appendstring(d.as_ptr(), k.as_ptr(), self.as_ptr()) })
    }
}
impl DictInsert for &str {
    fn insert_into(self, d: &Dict, k: SymbolRef) -> DictResult<()> {
        CString::new(self)
            .map_err(|_| DictError::InvalidString)?
            .as_c_str()
            .insert_into(d, k)
    }
}
fn validate_atoms(atoms: &[Atom]) -> DictResult<c_long> {
    for a in atoms {
        match a.get_type() {
            Some(AtomType::Object) => return Err(DictError::ObjectAtom),
            None => return Err(DictError::InvalidAtom),
            _ => (),
        }
    }
    atoms.len().try_into().map_err(|_| DictError::InvalidData)
}
impl DictInsert for &[Atom] {
    fn insert_into(self, d: &Dict, k: SymbolRef) -> DictResult<()> {
        let n = validate_atoms(self)?;
        let _lock = d.lock();
        check(unsafe {
            sys::dictionary_appendatoms(d.as_ptr(), k.as_ptr(), n, self.as_ptr().cast_mut().cast())
        })
    }
}
impl DictInsert for Dict {
    fn insert_into(self, d: &Dict, k: SymbolRef) -> DictResult<()> {
        d.insert_child(k, self)
    }
}
impl DictInsert for &Dict {
    fn insert_into(self, d: &Dict, k: SymbolRef) -> DictResult<()> {
        d.insert_child(k, self.deep_clone()?)
    }
}

pub(crate) unsafe fn copy_array(p: *mut sys::t_atomarray) -> DictResult<Vec<Atom>> {
    let mut n = 0;
    let mut atoms = std::ptr::null_mut();
    check(unsafe { sys::atomarray_getatoms(p, &mut n, &mut atoms) })?;
    if n == 0 {
        return Ok(Vec::new());
    }
    if n < 0 || atoms.is_null() {
        return Err(DictError::InvalidData);
    }
    Ok(unsafe { std::slice::from_raw_parts(atoms, n as usize) }
        .iter()
        .map(|a| Atom { value: *a })
        .collect())
}

/// An unshared atomarray builder. Mutations require `&mut self`; no object atoms or child-owning
/// flags are accepted. Consuming it in `Dict::insert` transfers its sole reference.
pub struct AtomArray(NonNull<sys::t_atomarray>);
// SAFETY: sole owner, no sharing or borrowed slices; ownership may move between threads.
unsafe impl Send for AtomArray {}
impl AtomArray {
    pub fn new(atoms: &[Atom]) -> DictResult<Self> {
        non_audio();
        let n = validate_atoms(atoms)?;
        NonNull::new(unsafe { sys::atomarray_new(n, atoms.as_ptr().cast_mut().cast()) })
            .map(Self)
            .ok_or(DictError::Allocation)
    }
    pub fn atoms(&self) -> DictResult<Vec<Atom>> {
        non_audio();
        unsafe { copy_array(self.0.as_ptr()) }
    }
    pub fn len(&self) -> usize {
        non_audio();
        unsafe { sys::atomarray_getsize(self.0.as_ptr()).max(0) as usize }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn extend(&mut self, atoms: &[Atom]) -> DictResult<()> {
        // setatoms reports allocation failure, unlike the void-returning appendatoms.
        validate_atoms(atoms)?;
        let mut combined = self.atoms()?;
        combined.extend(atoms.iter().map(|a| Atom { value: a.value }));
        let n = validate_atoms(&combined)?;
        check(unsafe { sys::atomarray_setatoms(self.0.as_ptr(), n, combined.as_mut_ptr().cast()) })
    }
    pub fn push(&mut self, atom: Atom) -> DictResult<()> {
        self.extend(&[atom])
    }
    pub fn clear(&mut self) {
        non_audio();
        unsafe {
            sys::atomarray_clear(self.0.as_ptr());
        }
    }
    pub fn try_clone(&self) -> DictResult<Self> {
        Self::new(&self.atoms()?)
    }
    /// Give up the atomarray without freeing it: the caller owns the one reference now (as with
    /// [`Dict::into_raw`]).
    pub fn into_raw(self) -> *mut sys::t_atomarray {
        let p = self.0.as_ptr();
        std::mem::forget(self);
        p
    }
}
impl Drop for AtomArray {
    fn drop(&mut self) {
        unsafe {
            sys::object_free(self.0.as_ptr().cast());
        }
    }
}
impl DictInsert for AtomArray {
    fn insert_into(self, d: &Dict, k: SymbolRef) -> DictResult<()> {
        let _lock = d.lock();
        let ptr = self.0.as_ptr();
        std::mem::forget(self);
        check(unsafe { sys::dictionary_appendatomarray(d.as_ptr(), k.as_ptr(), ptr.cast()) })
    }
}
