//! Replies of `A_GIMMEBACK` methods.
//!
//! A gimmeback method is a gimme method (`A_GIMME`: the arguments as a list of atoms) that also
//! hands a value back to its caller in a `t_atom` (`rv`). `ext_mess.h` describes `A_GIMMEBACK` as
//! "can return atom value in final atom ptr arg. function returns long error code 0 = no err",
//! and `object_method_typed` (`ext_obex.h`) is the caller that reads the value. Register one with
//! [`Class::add_gimmeback`](crate::class::Class::add_gimmeback).
//!
//! What a method may return is [`GimmebackResult`]: a [`Reply`], anything that converts to one
//! (`t_atom_long`, `f64`, [`SymbolRef`], `Vec<Atom>`, [`AtomArray`], [`Dict`], `()`), or a
//! `Result` of one of those with an `E: Display` error. An `Err` goes to the type's `report_error`
//! hook, as for any other method, and the caller gets no value and `MAX_ERR_GENERIC`.
//!
//! The design is in `doc/plan-gimmeback.md`. Which side owns an object reply (an atomarray or a
//! dictionary) is not stated in the headers: median hands it to the caller, as the `simplejs`
//! SDK example implies by never freeing the atomarray it returns. The consequence, observed in
//! Max 9.2 on 2026-10-27 (`doc/validation-max.md`): a caller that consumes the reply frees it
//! (the Jitter dumpout wrap did, six thousand 64 KiB replies left no growth), and a caller that
//! discards the reply leaks it (`js`'s `Maxobj.message` does: about 64 KiB per 4096-atom reply,
//! never released). `js` cannot read the reply either: `Maxobj.message` returns its own function
//! object. So on a Max (box) class prefer [`Reply::Int`], [`Reply::Float`], [`Reply::Symbol`] or
//! [`Reply::Nothing`] unless the caller is known to read the reply, which in Max today means
//! another external calling `object_method_typed`; object replies are for the Jitter dumpout
//! path and such callers. Scalars cannot leak.

use crate::{
    atom::Atom,
    dict::{AtomArray, Dict, DictError},
    symbol::SymbolRef,
};
use max_sys::{e_max_atomtypes, e_max_errorcodes, t_atom, t_atom_long, t_max_err};
use std::fmt::Display;

/// What a gimmeback method hands back in `rv`.
///
/// There is deliberately no variant for a bare [`Atom`]: [`Atom::set_obj`] is safe, so an atom
/// could carry any pointer, and who owns what is in `rv` is exactly what is assumed rather than
/// known. `Reply` is the whole set of things median knows how to hand over.
///
/// The three object variants ([`Atoms`](Reply::Atoms), [`Array`](Reply::Array),
/// [`Dict`](Reply::Dict)) hand an owned `A_OBJ` to the caller. A caller that consumes the reply
/// frees it (observed 2026-10-27 in Max 9.2 for the Jitter dumpout wrap); a caller that discards
/// it leaks it (observed for `js`'s `Maxobj.message`: about 64 KiB per 4096-atom reply, never
/// released). On a Max (box) class prefer `Int`, `Float`, `Symbol` or `Nothing` unless the caller
/// is known to read the reply, which in Max today means another external calling
/// `object_method_typed`; object replies are for the Jitter dumpout path and such callers.
/// Scalar replies cannot leak.
pub enum Reply {
    /// `A_NOTHING`: the method has no value for this call.
    Nothing,
    /// `A_LONG`.
    Int(t_atom_long),
    /// `A_FLOAT`.
    Float(f64),
    /// `A_SYM`.
    Symbol(SymbolRef),
    /// A list: packaged in a fresh atomarray that the caller owns (`A_OBJ`), the convention the
    /// SDK's `simplejs` example sets. Object atoms are refused (see [`AtomArray::new`]), so a
    /// reply cannot carry a pointer median did not make; the caller gets no value then. Freed by
    /// a caller that consumes it, leaked by one that discards it (see [`Reply`]): `js` leaks it
    /// (observed 2026-10-27).
    Atoms(Vec<Atom>),
    /// An atomarray the method built: it goes to the caller as it is (`A_OBJ`), without the copy
    /// `Atoms` makes. It holds no object atoms either ([`AtomArray`] refuses them). The same
    /// ownership rule as `Atoms`: a caller that discards it leaks it.
    Array(AtomArray),
    /// A dictionary (`A_OBJ`): median's reference goes to the caller ([`Dict::into_raw`]); a
    /// shared dictionary just loses one reference. The same ownership rule as `Atoms`: a caller
    /// that discards it leaks the reference.
    Dict(Dict),
}

impl From<t_atom_long> for Reply {
    fn from(v: t_atom_long) -> Self {
        Reply::Int(v)
    }
}

impl From<f64> for Reply {
    fn from(v: f64) -> Self {
        Reply::Float(v)
    }
}

impl From<SymbolRef> for Reply {
    fn from(v: SymbolRef) -> Self {
        Reply::Symbol(v)
    }
}

impl From<Vec<Atom>> for Reply {
    fn from(v: Vec<Atom>) -> Self {
        Reply::Atoms(v)
    }
}

impl From<AtomArray> for Reply {
    fn from(v: AtomArray) -> Self {
        Reply::Array(v)
    }
}

impl From<Dict> for Reply {
    fn from(v: Dict) -> Self {
        Reply::Dict(v)
    }
}

impl From<()> for Reply {
    fn from((): ()) -> Self {
        Reply::Nothing
    }
}

/// Set `*rv` to `A_NOTHING`.
///
/// # Safety
/// `rv` must point to a writable `t_atom`.
pub(crate) unsafe fn set_nothing(rv: *mut t_atom) {
    // SAFETY: guaranteed by the caller; a `t_atom` is plain data
    unsafe {
        (*rv).a_type = e_max_atomtypes::A_NOTHING as _;
        (*rv).a_w.w_long = 0;
    }
}

fn err(code: e_max_errorcodes::Type) -> t_max_err {
    code as t_max_err
}

impl Reply {
    /// Write the reply into `rv` and return the error code for the caller. A null `rv` drops the
    /// reply (nothing is made for it, and a dictionary is released).
    ///
    /// # Safety
    /// `rv` must be null or point to a writable `t_atom` that holds `A_NOTHING`. An
    /// `A_OBJ` reply becomes the caller's to free.
    pub(crate) unsafe fn write(
        self,
        rv: *mut t_atom,
        report: &mut dyn FnMut(&dyn Display),
    ) -> t_max_err {
        let none = err(e_max_errorcodes::MAX_ERR_NONE);
        if rv.is_null() {
            return none;
        }
        match self {
            Reply::Nothing => {}
            // SAFETY: `rv` is valid (the caller's contract), and these only store into it
            Reply::Int(v) => unsafe {
                max_sys::atom_setlong(rv, v);
            },
            Reply::Float(v) => unsafe {
                max_sys::atom_setfloat(rv, v);
            },
            Reply::Symbol(v) => unsafe {
                max_sys::atom_setsym(rv, v.into());
            },
            Reply::Atoms(atoms) => match AtomArray::new(&atoms) {
                Ok(a) => unsafe {
                    max_sys::atom_setobj(rv, a.into_raw().cast());
                },
                Err(DictError::Allocation) => return err(e_max_errorcodes::MAX_ERR_OUT_OF_MEM),
                Err(e) => {
                    report(&e);
                    return err(e_max_errorcodes::MAX_ERR_GENERIC);
                }
            },
            Reply::Array(a) => unsafe {
                max_sys::atom_setobj(rv, a.into_raw().cast());
            },
            Reply::Dict(d) => unsafe {
                max_sys::atom_setobj(rv, d.into_raw().cast());
            },
        }
        none
    }
}

mod sealed {
    pub trait Sealed {}
}

/// What a method registered with
/// [`Class::add_gimmeback`](crate::class::Class::add_gimmeback) may return: a [`Reply`],
/// a value that converts to one (`t_atom_long`, `f64`, [`SymbolRef`], `Vec<Atom>`, [`AtomArray`],
/// [`Dict`], `()` for no value), or `Result<R, E>` of one of those with `E: Display`.
///
/// `Ok(r)` (or a plain `r`) is written to the caller's `rv` and answers `MAX_ERR_NONE`. `Err(e)`
/// goes to the type's `report_error` hook
/// ([`MaxObjWrapped::report_error`](crate::wrapper::MaxObjWrapped::report_error)), leaves `rv`
/// at `A_NOTHING` and answers `MAX_ERR_GENERIC`. A list reply whose atomarray can't be allocated
/// leaves `A_NOTHING` and answers `MAX_ERR_OUT_OF_MEM`; one that is refused (an object atom)
/// is reported and answers `MAX_ERR_GENERIC`.
///
/// An object reply (`Reply::Atoms`, `Array`, `Dict`, and the plain `Vec<Atom>`, [`AtomArray`] and
/// [`Dict`] that convert to them) is freed by a caller that consumes it (observed 2026-10-27 in
/// Max 9.2 for the Jitter dumpout wrap) and leaked by a caller that discards it (observed for
/// `js`'s `Maxobj.message`: about 64 KiB per 4096-atom reply, never released). On a Max (box)
/// class return `t_atom_long`, `f64`, [`SymbolRef`] or `()` unless the caller is known to read the
/// reply (in Max today, another external calling `object_method_typed`); object replies are for
/// the Jitter dumpout path and such callers. Scalars cannot leak.
///
/// This trait is sealed.
#[diagnostic::on_unimplemented(
    message = "a gimmeback method can't return `{Self}`",
    label = "returns `{Self}`",
    note = "it must return `Reply`, `t_atom_long`, `f64`, `SymbolRef`, `Vec<Atom>`, `AtomArray`, `Dict` or `()`, or `Result<_, E>` of one of those with `E: Display` (`Atom` is not accepted)"
)]
pub trait GimmebackResult: sealed::Sealed {
    /// Write the reply into `rv` and return the `t_max_err` for the caller; an error goes to
    /// `report`.
    ///
    /// # Safety
    /// `rv` must be null or point to a writable `t_atom` that holds `A_NOTHING`.
    #[doc(hidden)]
    unsafe fn write_reply(self, rv: *mut t_atom, report: &mut dyn FnMut(&dyn Display))
    -> t_max_err;
}

impl sealed::Sealed for Reply {}

impl GimmebackResult for Reply {
    unsafe fn write_reply(
        self,
        rv: *mut t_atom,
        report: &mut dyn FnMut(&dyn Display),
    ) -> t_max_err {
        // SAFETY: guaranteed by the caller
        unsafe { self.write(rv, report) }
    }
}

/// The plain values that convert to a `Reply`.
macro_rules! plain_result {
    ($($t:ty),+) => {$(
        impl sealed::Sealed for $t {}

        impl GimmebackResult for $t {
            unsafe fn write_reply(
                self,
                rv: *mut t_atom,
                report: &mut dyn FnMut(&dyn Display),
            ) -> t_max_err {
                // SAFETY: guaranteed by the caller
                unsafe { Reply::from(self).write(rv, report) }
            }
        }
    )+};
}

plain_result!(t_atom_long, f64, SymbolRef, Vec<Atom>, AtomArray, Dict, ());

impl<R: Into<Reply>, E: Display> sealed::Sealed for Result<R, E> {}

impl<R: Into<Reply>, E: Display> GimmebackResult for Result<R, E> {
    unsafe fn write_reply(
        self,
        rv: *mut t_atom,
        report: &mut dyn FnMut(&dyn Display),
    ) -> t_max_err {
        match self {
            // SAFETY: guaranteed by the caller
            Ok(r) => unsafe { r.into().write(rv, report) },
            Err(e) => {
                report(&e);
                err(e_max_errorcodes::MAX_ERR_GENERIC)
            }
        }
    }
}
