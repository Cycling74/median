//! Atoms: the safe [`Atom`] value type and the `atom_*` functions.

use crate::symbols::{gensym, intern, name_of};
use max_sys::{e_max_atomtypes as ty, e_max_errorcodes as err};
use max_sys::{t_atom, t_atom_float, t_atom_long, t_max_err, t_symbol, word};
use std::ffi::{c_char, c_long, c_void};

/// An owned copy of a `t_atom`, comparable in tests.
#[derive(Clone, Debug, PartialEq)]
pub enum Atom {
    Long(i64),
    Float(f64),
    Sym(String),
    /// An `A_OBJ` pointer.
    Obj(usize),
    /// Any other atom type (comma, semicolon, ...), by its `a_type`; the payload is not kept.
    Other(i16),
}

impl From<i64> for Atom {
    fn from(v: i64) -> Self {
        Atom::Long(v)
    }
}
impl From<i32> for Atom {
    fn from(v: i32) -> Self {
        Atom::Long(v as i64)
    }
}
impl From<f64> for Atom {
    fn from(v: f64) -> Self {
        Atom::Float(v)
    }
}
impl From<&str> for Atom {
    fn from(v: &str) -> Self {
        Atom::Sym(v.to_string())
    }
}

impl Atom {
    /// Copy a `t_atom`.
    pub fn from_raw(a: &t_atom) -> Atom {
        // SAFETY: which union field is read is decided by `a_type`
        unsafe {
            match a.a_type as u32 {
                ty::A_LONG => Atom::Long(a.a_w.w_long as i64),
                ty::A_FLOAT => Atom::Float(a.a_w.w_float),
                ty::A_SYM => Atom::Sym(name_of(a.a_w.w_sym as usize)),
                ty::A_OBJ => Atom::Obj(a.a_w.w_obj as usize),
                _ => Atom::Other(a.a_type),
            }
        }
    }

    /// Make a `t_atom`; symbols are interned.
    pub fn to_raw(&self) -> t_atom {
        let (a_type, a_w) = match self {
            Atom::Long(v) => (ty::A_LONG, word { w_long: *v as _ }),
            Atom::Float(v) => (ty::A_FLOAT, word { w_float: *v }),
            Atom::Sym(s) => (
                ty::A_SYM,
                word {
                    w_sym: intern(s.as_bytes()),
                },
            ),
            Atom::Obj(p) => (
                ty::A_OBJ,
                word {
                    w_obj: *p as *mut max_sys::t_object,
                },
            ),
            Atom::Other(t) => (*t as u32, word { w_long: 0 }),
        };
        t_atom {
            a_type: a_type as _,
            a_w,
        }
    }
}

/// Copy `argc` atoms from `argv`.
///
/// # Safety
/// `argv` must be null or valid for reading `argc` atoms.
pub unsafe fn atoms_from_raw(argc: usize, argv: *const t_atom) -> Vec<Atom> {
    if argv.is_null() {
        return Vec::new();
    }
    // SAFETY: the caller's contract
    unsafe { std::slice::from_raw_parts(argv, argc) }
        .iter()
        .map(Atom::from_raw)
        .collect()
}

/// # Safety
/// `a` must be null or point to a valid `t_atom`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atom_setlong(a: *mut t_atom, b: t_atom_long) -> t_max_err {
    if a.is_null() {
        return err::MAX_ERR_INVALID_PTR as t_max_err;
    }
    // SAFETY: the caller's contract
    unsafe {
        (*a).a_type = ty::A_LONG as _;
        (*a).a_w.w_long = b;
    }
    err::MAX_ERR_NONE as t_max_err
}

/// # Safety
/// As `atom_setlong`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atom_setfloat(a: *mut t_atom, b: f64) -> t_max_err {
    if a.is_null() {
        return err::MAX_ERR_INVALID_PTR as t_max_err;
    }
    // SAFETY: the caller's contract
    unsafe {
        (*a).a_type = ty::A_FLOAT as _;
        (*a).a_w.w_float = b;
    }
    err::MAX_ERR_NONE as t_max_err
}

/// # Safety
/// As `atom_setlong`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atom_setsym(a: *mut t_atom, b: *mut t_symbol) -> t_max_err {
    if a.is_null() {
        return err::MAX_ERR_INVALID_PTR as t_max_err;
    }
    // SAFETY: the caller's contract
    unsafe {
        (*a).a_type = ty::A_SYM as _;
        (*a).a_w.w_sym = b;
    }
    err::MAX_ERR_NONE as t_max_err
}

/// # Safety
/// As `atom_setlong`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atom_setobj(a: *mut t_atom, b: *mut c_void) -> t_max_err {
    if a.is_null() {
        return err::MAX_ERR_INVALID_PTR as t_max_err;
    }
    // SAFETY: the caller's contract
    unsafe {
        (*a).a_type = ty::A_OBJ as _;
        (*a).a_w.w_obj = b as *mut max_sys::t_object;
    }
    err::MAX_ERR_NONE as t_max_err
}

/// Floats are truncated; anything else is 0.
///
/// # Safety
/// `a` must be null or point to a valid `t_atom`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atom_getlong(a: *const t_atom) -> t_atom_long {
    if a.is_null() {
        return 0;
    }
    // SAFETY: the caller's contract, and the union field read is the one `a_type` names
    unsafe {
        match (*a).a_type as u32 {
            ty::A_LONG => (*a).a_w.w_long,
            ty::A_FLOAT => (*a).a_w.w_float as t_atom_long,
            _ => 0,
        }
    }
}

/// Longs are converted; anything else is 0.
///
/// # Safety
/// As `atom_getlong`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atom_getfloat(a: *const t_atom) -> t_atom_float {
    if a.is_null() {
        return 0.0;
    }
    // SAFETY: as `atom_getlong`
    unsafe {
        match (*a).a_type as u32 {
            ty::A_LONG => (*a).a_w.w_long as t_atom_float,
            ty::A_FLOAT => (*a).a_w.w_float,
            _ => 0.0,
        }
    }
}

/// The empty symbol (`_sym_nothing`) if the atom is not a symbol.
///
/// # Safety
/// As `atom_getlong`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atom_getsym(a: *const t_atom) -> *mut t_symbol {
    // SAFETY: as `atom_getlong`
    unsafe {
        if !a.is_null() && (*a).a_type as u32 == ty::A_SYM {
            (*a).a_w.w_sym
        } else {
            gensym(c"".as_ptr())
        }
    }
}

/// Null if the atom is not an object.
///
/// # Safety
/// As `atom_getlong`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atom_getobj(a: *const t_atom) -> *mut c_void {
    // SAFETY: as `atom_getlong`
    unsafe {
        if !a.is_null() && (*a).a_type as u32 == ty::A_OBJ {
            (*a).a_w.w_obj as *mut c_void
        } else {
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// As `atom_getlong`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atom_gettype(a: *const t_atom) -> c_long {
    if a.is_null() {
        return ty::A_NOTHING as c_long;
    }
    // SAFETY: as `atom_getlong`
    unsafe { (*a).a_type as c_long }
}

/// Render atoms as text the way Max shows them: separated by single spaces, an int as digits, a
/// float with at most six decimals and the trailing zeros removed (`3.`, `2.5`), a symbol as its
/// name, in quotes if it contains whitespace. An object pointer is `<object>`; any other atom
/// type renders as nothing. This is an approximation for tests: the SDK documents only that
/// `atom_gettext` eliminates redundant zeros and quotes symbols with spaces, with the default
/// flags.
pub fn atoms_text(atoms: &[Atom]) -> String {
    let mut out = String::new();
    for (i, a) in atoms.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        match a {
            Atom::Long(v) => out.push_str(&v.to_string()),
            Atom::Float(v) => {
                let s = format!("{v:.6}");
                let s = if s.contains('.') {
                    s.trim_end_matches('0')
                } else {
                    s.as_str()
                };
                out.push_str(s);
            }
            Atom::Sym(s) if s.contains(char::is_whitespace) => {
                out.push('"');
                out.push_str(s);
                out.push('"');
            }
            Atom::Sym(s) => out.push_str(s),
            Atom::Obj(_) => out.push_str("<object>"),
            Atom::Other(_) => {}
        }
    }
    out
}

/// Format `ac` atoms with [`atoms_text`] into a new block from `sysmem_newptr` (free it with
/// `sysmem_freeptr`) and store it in `*text`; `*textsize` is the length of the text without the
/// terminator. `flags` are ignored (the default rules are always used). Both out pointers must
/// be non-null.
///
/// # Safety
/// `av` must be null or valid for reading `ac` atoms; `textsize` and `text` must be null or valid
/// for writing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn atom_gettext(
    ac: c_long,
    av: *mut t_atom,
    textsize: *mut c_long,
    text: *mut *mut c_char,
    _flags: c_long,
) -> t_max_err {
    if text.is_null() || textsize.is_null() {
        return err::MAX_ERR_INVALID_PTR as t_max_err;
    }
    // SAFETY: the caller's contract
    let atoms = unsafe { atoms_from_raw(ac.max(0) as usize, av) };
    let s = atoms_text(&atoms);
    // SAFETY: plain allocation
    let buf = unsafe { crate::sysmem::sysmem_newptr((s.len() + 1) as _) } as *mut c_char;
    if buf.is_null() {
        return err::MAX_ERR_OUT_OF_MEM as t_max_err;
    }
    // SAFETY: `buf` is `s.len() + 1` bytes, `text` and `textsize` are writable
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr().cast::<c_char>(), buf, s.len());
        buf.add(s.len()).write(0);
        *textsize = s.len() as c_long;
        *text = buf;
    }
    err::MAX_ERR_NONE as t_max_err
}
