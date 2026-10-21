//! Attributes: `attribute_new`, `attr_offset_new`, `class_addattr`, the clip filters, and
//! `attr_args_process`, plus the harness side that gets and sets an attribute the way Max does.
//!
//! `attr_args_process` does nothing: `@attr value` arguments are not parsed, so a test sets
//! attributes with [`crate::Kernel::attr_set`].
//!
//! Only the attribute types `char`, `long`, `float32`, `float64` and `symbol` are supported for
//! attributes without accessors (read and written at the byte offset given to `attr_offset_new`).

use crate::atoms::{Atom, atoms_from_raw};
use crate::class::{Raw, call_raw};
use crate::state::{AttrData, Clip, SubKind, state};
use crate::symbols::{intern, name_of};
use max_sys::{e_max_errorcodes as err, method, t_atom, t_class, t_max_err, t_object, t_symbol};
use std::ffi::{CStr, c_char, c_long, c_short, c_void};

fn e(code: err::Type) -> t_max_err {
    code as t_max_err
}

fn new_attr(
    name: *const c_char,
    type_: *const t_symbol,
    flags: c_long,
    mget: method,
    mset: method,
    offset: Option<i64>,
) -> *mut t_object {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the caller passes a NUL-terminated string
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();
    state().alloc_sub(SubKind::Attr(AttrData {
        name,
        type_name: name_of(type_ as usize),
        flags: flags as i64,
        getter: mget.map_or(0, |f| f as usize),
        setter: mset.map_or(0, |f| f as usize),
        offset,
        set_clip: None,
        get_clip: None,
    })) as *mut t_object
}

/// # Safety
/// `name` must be a NUL-terminated string, `type_` a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn attribute_new(
    name: *const c_char,
    type_: *mut t_symbol,
    flags: c_long,
    mget: method,
    mset: method,
) -> *mut t_object {
    new_attr(name, type_, flags, mget, mset, None)
}

/// # Safety
/// As `attribute_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn attr_offset_new(
    name: *const c_char,
    type_: *const t_symbol,
    flags: c_long,
    mget: method,
    mset: method,
    offset: c_long,
) -> *mut t_object {
    new_attr(name, type_, flags, mget, mset, Some(offset as i64))
}

/// # Safety
/// `c` must be a class from `class_new`, `attr` an attribute from `attribute_new`/`attr_offset_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn class_addattr(c: *mut t_class, attr: *mut t_object) -> t_max_err {
    let mut st = state();
    if !matches!(
        st.subs.get(&(attr as usize)),
        Some(s) if matches!(s.kind, SubKind::Attr(_))
    ) {
        return e(err::MAX_ERR_INVALID_PTR);
    }
    match st.classes.get_mut(&(c as usize)) {
        Some(class) => {
            class.attrs.push(attr as usize);
            e(err::MAX_ERR_NONE)
        }
        None => e(err::MAX_ERR_INVALID_PTR),
    }
}

/// Does nothing, see the module docs.
///
/// # Safety
/// No requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn attr_args_process(_x: *mut c_void, _ac: c_short, _av: *mut t_atom) {}

fn add_clip(x: *mut c_void, clip: Clip, get: bool, set: bool) -> t_max_err {
    match state().subs.get_mut(&(x as usize)).map(|s| &mut s.kind) {
        Some(SubKind::Attr(a)) => {
            if get {
                a.get_clip = Some(clip);
            }
            if set {
                a.set_clip = Some(clip);
            }
            e(err::MAX_ERR_NONE)
        }
        _ => e(err::MAX_ERR_INVALID_PTR),
    }
}

fn clip(min: f64, max: f64, use_min: c_long, use_max: c_long) -> Clip {
    Clip {
        min,
        max,
        use_min: use_min != 0,
        use_max: use_max != 0,
    }
}

/// # Safety
/// `x` must be an attribute.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn attr_addfilterset_clip(
    x: *mut c_void,
    min: f64,
    max: f64,
    usemin: c_long,
    usemax: c_long,
) -> t_max_err {
    add_clip(x, clip(min, max, usemin, usemax), false, true)
}

/// # Safety
/// As `attr_addfilterset_clip`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn attr_addfilterget_clip(
    x: *mut c_void,
    min: f64,
    max: f64,
    usemin: c_long,
    usemax: c_long,
) -> t_max_err {
    add_clip(x, clip(min, max, usemin, usemax), true, false)
}

/// # Safety
/// As `attr_addfilterset_clip`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn attr_addfilter_clip(
    x: *mut c_void,
    min: f64,
    max: f64,
    usemin: c_long,
    usemax: c_long,
) -> t_max_err {
    add_clip(x, clip(min, max, usemin, usemax), true, true)
}

/// Why [`crate::Kernel::attr_get`] or [`crate::Kernel::attr_set`] failed.
#[derive(Clone, Debug, PartialEq)]
pub enum AttrError {
    /// Not a live object.
    NotLive,
    /// The object's class has no attribute of that name.
    NoAttr(String),
    /// The attribute's accessor returned this error code.
    Accessor(i64),
    /// The attribute has no accessor and its type or the atoms are not supported.
    Unsupported(String),
}

struct Found {
    ptr: usize,
    getter: usize,
    setter: usize,
    offset: Option<i64>,
    type_name: String,
    get_clip: Option<Clip>,
    set_clip: Option<Clip>,
}

fn find_attr(obj: usize, name: &str) -> Result<Found, AttrError> {
    let st = state();
    let o = st.objects.get(&obj).ok_or(AttrError::NotLive)?;
    st.classes[&o.class]
        .attrs
        .iter()
        .find_map(|&ptr| match &st.subs.get(&ptr)?.kind {
            SubKind::Attr(a) if a.name == name => Some(Found {
                ptr,
                getter: a.getter,
                setter: a.setter,
                offset: a.offset,
                type_name: a.type_name.clone(),
                get_clip: a.get_clip,
                set_clip: a.set_clip,
            }),
            _ => None,
        })
        .ok_or_else(|| AttrError::NoAttr(name.to_string()))
}

fn apply_clip(atoms: Vec<Atom>, clip: Option<Clip>) -> Vec<Atom> {
    let Some(c) = clip else { return atoms };
    let f = |v: f64| {
        let v = if c.use_min { v.max(c.min) } else { v };
        if c.use_max { v.min(c.max) } else { v }
    };
    atoms
        .into_iter()
        .map(|a| match a {
            Atom::Long(v) => Atom::Long(f(v as f64) as i64),
            Atom::Float(v) => Atom::Float(f(v)),
            other => other,
        })
        .collect()
}

pub(crate) fn get(obj: usize, name: &str) -> Result<Vec<Atom>, AttrError> {
    let a = find_attr(obj, name)?;
    let atoms = if a.getter != 0 {
        let mut argc: c_long = 0;
        let mut argv: *mut t_atom = std::ptr::null_mut();
        // SAFETY: attribute getters are `(x, attr, long *argc, t_atom **argv)`
        let code = unsafe {
            call_raw(
                a.getter,
                &[
                    Raw::I(obj),
                    Raw::I(a.ptr),
                    Raw::I(&raw mut argc as usize),
                    Raw::I(&raw mut argv as usize),
                ],
            )
        } as isize;
        if code != 0 {
            return Err(AttrError::Accessor(code as i64));
        }
        // SAFETY: the getter filled in `argc` atoms at `argv`
        let atoms = unsafe { atoms_from_raw(argc.max(0) as usize, argv) };
        // The getter allocates with sysmem_newptr when it is given none; the caller frees.
        // SAFETY: `argv` is null or a block from `sysmem_newptr`
        unsafe { crate::sysmem::sysmem_freeptr(argv as *mut c_void) };
        atoms
    } else {
        let offset = a
            .offset
            .ok_or_else(|| AttrError::Unsupported("no getter and no offset".into()))?;
        // SAFETY: `offset` is a byte offset into the live object given to `attr_offset_new`
        let p = unsafe { (obj as *const u8).offset(offset as isize) };
        // SAFETY: as above; the type decides the width
        unsafe {
            match a.type_name.as_str() {
                "char" => vec![Atom::Long((p as *const i8).read_unaligned() as i64)],
                "long" => vec![Atom::Long((p as *const c_long).read_unaligned() as i64)],
                "float32" => vec![Atom::Float((p as *const f32).read_unaligned() as f64)],
                "float64" => vec![Atom::Float((p as *const f64).read_unaligned())],
                "symbol" => {
                    let s = (p as *const *const t_symbol).read_unaligned();
                    vec![Atom::Sym(name_of(s as usize))]
                }
                other => return Err(AttrError::Unsupported(format!("attribute type {other}"))),
            }
        }
    };
    Ok(apply_clip(atoms, a.get_clip))
}

pub(crate) fn set(obj: usize, name: &str, atoms: &[Atom]) -> Result<(), AttrError> {
    let a = find_attr(obj, name)?;
    let atoms = apply_clip(atoms.to_vec(), a.set_clip);
    if a.setter != 0 {
        let mut raw: Vec<t_atom> = atoms.iter().map(Atom::to_raw).collect();
        // SAFETY: attribute setters are `(x, attr, long argc, t_atom *argv)`
        let code = unsafe {
            call_raw(
                a.setter,
                &[
                    Raw::I(obj),
                    Raw::I(a.ptr),
                    Raw::I(raw.len()),
                    Raw::I(raw.as_mut_ptr() as usize),
                ],
            )
        } as isize;
        return if code == 0 {
            Ok(())
        } else {
            Err(AttrError::Accessor(code as i64))
        };
    }
    let offset = a
        .offset
        .ok_or_else(|| AttrError::Unsupported("no setter and no offset".into()))?;
    let first = atoms
        .first()
        .ok_or_else(|| AttrError::Unsupported("no atoms".into()))?;
    let num = match first {
        Atom::Long(v) => Some(*v as f64),
        Atom::Float(v) => Some(*v),
        _ => None,
    };
    // SAFETY: `offset` is a byte offset into the live object given to `attr_offset_new`
    let p = unsafe { (obj as *mut u8).offset(offset as isize) };
    // SAFETY: as above; the type decides the width
    unsafe {
        match (a.type_name.as_str(), first, num) {
            ("char", _, Some(n)) => (p as *mut i8).write_unaligned(n as i8),
            ("long", Atom::Long(v), _) => (p as *mut c_long).write_unaligned(*v as c_long),
            ("long", _, Some(n)) => (p as *mut c_long).write_unaligned(n as c_long),
            ("float32", _, Some(n)) => (p as *mut f32).write_unaligned(n as f32),
            ("float64", _, Some(n)) => (p as *mut f64).write_unaligned(n),
            ("symbol", Atom::Sym(s), _) => {
                (p as *mut *mut t_symbol).write_unaligned(intern(s.as_bytes()))
            }
            (other, atom, _) => {
                return Err(AttrError::Unsupported(format!(
                    "attribute type {other} from {atom:?}"
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn flags(obj: usize, name: &str) -> Option<i64> {
    let st = state();
    let o = st.objects.get(&obj)?;
    st.classes[&o.class]
        .attrs
        .iter()
        .find_map(|p| match &st.subs.get(p)?.kind {
            SubKind::Attr(a) if a.name == name => Some(a.flags),
            _ => None,
        })
}

/// The names of the attributes of the object's class, in the order they were added.
pub(crate) fn names(obj: usize) -> Vec<String> {
    let st = state();
    let Some(o) = st.objects.get(&obj) else {
        return Vec::new();
    };
    st.classes[&o.class]
        .attrs
        .iter()
        .filter_map(|p| match &st.subs.get(p)?.kind {
            SubKind::Attr(a) => Some(a.name.clone()),
            _ => None,
        })
        .collect()
}
