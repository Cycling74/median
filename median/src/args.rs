//! Creation arguments: the atoms after the class name in the box, `[simp 1 2.5 foo @value 7]`.
//!
//! [`MaxBuilder::args`](crate::builder::MaxBuilder::args) (also on
//! [`MSPBuilder`](crate::builder::MSPBuilder) and [`JitBuilder`](crate::jit::JitBuilder)) gives
//! an [`Args`], which splits the list the way the SDK does: the *positional* arguments come
//! first, then the *attribute arguments*, each an `@name` and the values up to the next `@name`
//! (the SDK's `attr_args_offset`, `ext_obex.h`, "the atom list `foo bar 3.0 @mode 6` would cause
//! `attr_args_offset` to return 3"). Max writes the attributes an object saves with the patcher
//! into the box text as `@name value`, so a saved inspector value arrives here too.
//!
//! ```no_run
//! use median::prelude::*;
//!
//! #[derive(MaxObject)]
//! #[max(name = "chans", no_methods)]
//! pub struct Chans {
//!     outs: Vec<Outlet<Int>>,
//! }
//!
//! impl MaxObjWrapped for Chans {
//!     fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
//!         // the first positional argument, 2 if it is left out; `?` posts a bad one
//!         // (`argument 1: expected a number, got `foo``) and refuses the object
//!         let args = b.args();
//!         let first: t_atom_long = args.at(0).unwrap_or(2)?;
//!         // the attribute `@channels`, which only `new` can act on: it decides how many
//!         // outlets to make
//!         let n: t_atom_long = args.attr(median::sym!("channels")).unwrap_or(first)?;
//!         let n = n.clamp(1, 16);
//!         let outs = (0..n).map(|_| b.outlet(Outlet::int())).collect();
//!         Ok(Self { outs })
//!     }
//! }
//! # median::ext_main! { Chans }
//! ```
//!
//! [`Args::at`] and [`Args::attr`] each give an [`Arg`], one creation argument that may or may
//! not be there: [`unwrap_or`](Arg::unwrap_or) reads it with a default for a left-out one,
//! [`require`](Arg::require) makes it an error to leave it out, [`get`](Arg::get) gives an
//! `Option`.
//!
//! Raw access stays: [`Args::raw`] (and `creation_args()` on the builders) is the whole list,
//! `@name` atoms included.
//!
//! The attributes are still applied to the object after `new` returns, with the same atoms, by
//! the SDK's `attr_args_process`; [`Args::attr`] is for the decisions only `new` can make
//! (how many outlets, which buffer), not for setting a field the attribute already sets.
//!
//! [`Args::text`] and its relatives are the arguments as Max shows them (`atom_gettext`), for
//! an object that wants the raw textual value, as `[message]`-like objects do.

use crate::{
    atom::{Atom, AtomType, text_of},
    error::NewError,
    symbol::SymbolRef,
};
use core::ffi::{CStr, c_short};
use std::fmt;

/// The creation arguments of an object, borrowed, with the split between positional and
/// attribute arguments computed. See the [module documentation](self).
///
/// It is `Copy`; make one from any atom list with [`new`](Self::new) (a `gimme` method's
/// arguments, for example) or from a builder with `args()`.
#[derive(Clone, Copy)]
pub struct Args<'a> {
    raw: &'a [Atom],
    /// The index of the first attribute argument (`raw.len()` if there is none).
    offset: usize,
}

impl<'a> Args<'a> {
    /// Split `raw`: the positional arguments end where the SDK's `attr_args_offset` says the
    /// first attribute argument begins. (The SDK takes the count as a `short`: of a list longer
    /// than 32767 atoms only the first 32767 are searched.)
    pub fn new(raw: &'a [Atom]) -> Self {
        let n = raw.len().min(c_short::MAX as usize);
        // SAFETY: `Atom` is a transparent `t_atom`: `raw` is `n` valid atoms, which
        // `attr_args_offset` only reads
        let offset =
            unsafe { max_sys::attr_args_offset(n as c_short, raw.as_ptr().cast_mut().cast()) };
        Self {
            raw,
            offset: usize::try_from(offset).unwrap_or(0).min(raw.len()),
        }
    }

    /// The whole list, untouched: the same atoms as the builders' `creation_args()`, positional
    /// and attribute arguments (`@name` atoms included).
    pub fn raw(&self) -> &'a [Atom] {
        self.raw
    }

    /// The atoms before the first attribute argument.
    pub fn positional(&self) -> &'a [Atom] {
        &self.raw[..self.offset]
    }

    /// Each attribute argument, in order: the name (the symbol without its `@`) and the values,
    /// the atoms up to the next `@name` (none if the next atom is one). An `@name` that is
    /// repeated is yielded each time.
    pub fn attrs(&self) -> impl Iterator<Item = (SymbolRef, &'a [Atom])> + use<'a> {
        AttrArgs {
            rest: &self.raw[self.offset..],
        }
    }

    /// Positional argument `i`: absent if there are not that many positional arguments. An
    /// `@name` is not a positional argument.
    pub fn at(&self, i: usize) -> Arg<'a> {
        Arg {
            origin: Origin::Positional(i),
            values: self.positional().get(i).map(std::slice::from_ref),
        }
    }

    /// The attribute argument `@name` (pass the name without the `@`): absent if there is no
    /// such `@name`; present with no values for an `@name` followed directly by another or by
    /// the end. If the attribute is given more than once the last occurrence wins, the one that
    /// the SDK applies last.
    ///
    /// The attribute's setter still runs after `new` with the same atoms, so this is for the
    /// decisions only `new` can make, such as how many outlets to create; for a value the
    /// attribute stores itself, read the attribute, not the argument.
    pub fn attr(&self, name: SymbolRef) -> Arg<'a> {
        Arg {
            origin: Origin::Attr(name),
            values: self
                .attrs()
                .filter(|(n, _)| *n == name)
                .map(|(_, values)| values)
                .last(),
        }
    }

    /// All the arguments as text, the way Max shows them (`atom_gettext`, default flags):
    /// `1 2.5 foo @value 7`.
    pub fn text(&self) -> String {
        text_of(self.raw)
    }

    /// The positional arguments as text, see [`text`](Self::text).
    pub fn positional_text(&self) -> String {
        text_of(self.positional())
    }
}

#[derive(Clone, Copy)]
enum Origin {
    Positional(usize),
    Attr(SymbolRef),
}

/// One creation argument, present or not: positional `i` ([`Args::at`]) or the values of
/// `@name` ([`Args::attr`]). It is `Copy`; the typed accessors consume it.
///
/// A positional argument is one atom. An attribute argument is the atoms after its `@name`,
/// and the typed accessors read the first one: an `@name` without values counts as absent for
/// them ([`get`](Self::get) gives `None`, [`require`](Self::require) an error), though
/// [`is_present`](Self::is_present) and [`values`](Self::values) tell it from no `@name` at all.
///
/// ```no_run
/// # use median::prelude::*;
/// # fn f(b: &mut MaxBuilder<impl MaxObjWrapped>) -> Result<(), median::args::ArgError> {
/// let args = b.args();
/// let value: t_atom_long = args.at(0).unwrap_or(0)?;
/// let name = args.at(1).require::<SymbolRef>()?;
/// let chans: t_atom_long = args.attr(median::sym!("channels")).unwrap_or(2)?;
/// # Ok(()) }
/// ```
#[derive(Clone, Copy)]
pub struct Arg<'a> {
    origin: Origin,
    values: Option<&'a [Atom]>,
}

impl<'a> Arg<'a> {
    /// The atom the typed accessors read: the positional atom, the first value of an attribute.
    fn first(self) -> Option<&'a Atom> {
        self.values.and_then(<[Atom]>::first)
    }

    fn type_error(self, e: ArgTypeError) -> ArgError {
        match self.origin {
            Origin::Positional(index) => e.at_arg(index),
            Origin::Attr(name) => e.at_attr(name),
        }
    }

    fn missing(self) -> ArgError {
        match self.origin {
            Origin::Positional(index) => ArgError::Missing { index },
            Origin::Attr(name) => ArgError::MissingAttr { name },
        }
    }

    /// The argument as a `T`, or `default` if it is absent.
    ///
    /// It is named after [`Option::unwrap_or`] because it reads the same way, but it returns a
    /// `Result`: the default only covers absence. An argument of the wrong type is an
    /// [`ArgError`], not the default, and the `?` after it reports it.
    pub fn unwrap_or<T: FromAtom>(self, default: T) -> Result<T, ArgError> {
        Ok(self.get()?.unwrap_or(default))
    }

    /// The argument as a `T`, or `T::default()` if it is absent; a wrong type is an error, as
    /// for [`unwrap_or`](Self::unwrap_or).
    pub fn unwrap_or_default<T: FromAtom + Default>(self) -> Result<T, ArgError> {
        Ok(self.get()?.unwrap_or_default())
    }

    /// The argument as a `T`; leaving it out is an [`ArgError::Missing`] (positional) or
    /// [`ArgError::MissingAttr`] (attribute, also an `@name` with no values).
    pub fn require<T: FromAtom>(self) -> Result<T, ArgError> {
        match self.get()? {
            Some(v) => Ok(v),
            None => Err(self.missing()),
        }
    }

    /// The argument as a `T`: `Ok(None)` if it is absent (for an attribute also if it has no
    /// values), an [`ArgError::Type`] or [`ArgError::Attr`] if it is not of the type `T` takes.
    pub fn get<T: FromAtom>(self) -> Result<Option<T>, ArgError> {
        match self.first() {
            None => Ok(None),
            Some(a) => T::from_atom(a).map(Some).map_err(|e| self.type_error(e)),
        }
    }

    /// The atom: the positional one, the first value of an attribute (`None` if it has none).
    pub fn atom(self) -> Option<&'a Atom> {
        self.first()
    }

    /// All the atoms: one for a positional argument, the values after the `@name` for an
    /// attribute (which may be none: `Some` of an empty slice). `None` if the argument is
    /// absent.
    pub fn values(self) -> Option<&'a [Atom]> {
        self.values
    }

    /// The atoms as text, the way Max shows them (`atom_gettext`, default flags), without the
    /// `@name`, with all the values of an attribute. `None` if the argument is absent.
    pub fn text(self) -> Option<String> {
        self.values.map(text_of)
    }

    /// Whether the argument is there: the positional index exists, or the `@name` was given
    /// (with or without values).
    pub fn is_present(self) -> bool {
        self.values.is_some()
    }
}

/// The name of an `@name` atom, without the `@`; `None` for any other atom.
fn attr_name(a: &Atom) -> Option<SymbolRef> {
    if !matches!(a.get_type(), Some(AtomType::Symbol)) {
        return None;
    }
    let name = a.get_symbol();
    let rest = name.name().to_bytes_with_nul().strip_prefix(b"@")?;
    // the rest of a C string is a C string
    CStr::from_bytes_with_nul(rest).ok().map(SymbolRef::from)
}

struct AttrArgs<'a> {
    rest: &'a [Atom],
}

impl<'a> Iterator for AttrArgs<'a> {
    type Item = (SymbolRef, &'a [Atom]);

    fn next(&mut self) -> Option<Self::Item> {
        // `Args` starts at an `@name`; skipping to one is only for a list that `attr_args_offset`
        // and this code read differently
        let at = self.rest.iter().position(|a| attr_name(a).is_some())?;
        let name = attr_name(&self.rest[at])?;
        let values = &self.rest[at + 1..];
        let end = values
            .iter()
            .position(|a| attr_name(a).is_some())
            .unwrap_or(values.len());
        self.rest = &values[end..];
        Some((name, &values[..end]))
    }
}

/// A type that an argument atom converts to, for the typed accessors of [`Arg`]. Implement it
/// for your own types (an enum from a symbol, say):
///
/// ```
/// use median::{args::{ArgTypeError, FromAtom}, atom::Atom};
///
/// enum Mode { Fast, Slow }
///
/// impl FromAtom for Mode {
///     fn from_atom(a: &Atom) -> Result<Self, ArgTypeError> {
///         match a.text().as_str() {
///             "fast" => Ok(Mode::Fast),
///             "slow" => Ok(Mode::Slow),
///             _ => Err(ArgTypeError::new("fast or slow", a)),
///         }
///     }
/// }
/// ```
///
/// Implemented for [`t_atom_long`](max_sys::t_atom_long) (an int only), `f64` (an int or a float,
/// as Max's own number coercion), [`SymbolRef`] (a symbol only), `String` (a symbol; an int or a
/// float as [`Atom::text`] shows it) and `bool` (the int 0 or 1). An object atom is never
/// accepted by the implementations here.
pub trait FromAtom: Sized {
    /// Convert the atom, or say what was expected.
    fn from_atom(a: &Atom) -> Result<Self, ArgTypeError>;
}

/// An integer atom, or a float atom truncated toward zero: the coercion Max applies to a typed
/// `A_LONG` argument, which a `#[sel]` argument of this type gets too.
impl FromAtom for max_sys::t_atom_long {
    fn from_atom(a: &Atom) -> Result<Self, ArgTypeError> {
        match a.get_type() {
            Some(AtomType::Int) => Ok(a.get_int()),
            Some(AtomType::Float) => Ok(a.get_float() as Self),
            _ => Err(ArgTypeError::new("a number", a)),
        }
    }
}

impl FromAtom for f64 {
    fn from_atom(a: &Atom) -> Result<Self, ArgTypeError> {
        match a.get_type() {
            Some(AtomType::Int | AtomType::Float) => Ok(a.get_float()),
            _ => Err(ArgTypeError::new("a number", a)),
        }
    }
}

impl FromAtom for SymbolRef {
    fn from_atom(a: &Atom) -> Result<Self, ArgTypeError> {
        match a.get_type() {
            Some(AtomType::Symbol) => Ok(a.get_symbol()),
            _ => Err(ArgTypeError::new("a symbol", a)),
        }
    }
}

impl FromAtom for String {
    fn from_atom(a: &Atom) -> Result<Self, ArgTypeError> {
        match a.get_type() {
            Some(AtomType::Symbol) => Ok(a.get_symbol().to_string()),
            Some(AtomType::Int | AtomType::Float) => Ok(a.text()),
            _ => Err(ArgTypeError::new("a symbol or a number", a)),
        }
    }
}

impl FromAtom for bool {
    fn from_atom(a: &Atom) -> Result<Self, ArgTypeError> {
        match (a.get_type(), a.get_int()) {
            (Some(AtomType::Int), 0) => Ok(false),
            (Some(AtomType::Int), 1) => Ok(true),
            _ => Err(ArgTypeError::new("0 or 1", a)),
        }
    }
}

/// Why a [`FromAtom`] refused an atom: what it expected and what it got, as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgTypeError {
    /// What was expected, as it reads after "expected": `an integer`, `a number`.
    pub expected: &'static str,
    /// The atom that was found, as text (`foo`, `2.5`; `an object` for an object atom).
    pub found: String,
}

impl ArgTypeError {
    /// An error for `found`, which was not `expected` (`"a symbol"`).
    pub fn new(expected: &'static str, found: &Atom) -> Self {
        let found = match found.get_type() {
            Some(AtomType::Object) => "an object".to_owned(),
            _ => found.text(),
        };
        Self { expected, found }
    }

    fn at_arg(self, index: usize) -> ArgError {
        ArgError::Type {
            index,
            expected: self.expected,
            found: self.found,
        }
    }

    fn at_attr(self, name: SymbolRef) -> ArgError {
        ArgError::Attr {
            name,
            expected: self.expected,
            found: self.found,
        }
    }
}

impl fmt::Display for ArgTypeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "expected {}, got `{}`", self.expected, self.found)
    }
}

impl std::error::Error for ArgTypeError {}

/// Why an argument could not be read, see [`Args`]. `?` in `new` turns it into a
/// [`NewError`]: the message is posted to the Max console and the object is refused.
///
/// `index` is zero-based, as in [`Args::at`]; the message counts from 1, as a patcher user
/// does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgError {
    /// A required positional argument is not there ([`Arg::require`]).
    Missing { index: usize },
    /// A required attribute argument is not there, or has no values ([`Arg::require`]).
    MissingAttr { name: SymbolRef },
    /// A positional argument is not of the type asked for.
    Type {
        index: usize,
        expected: &'static str,
        found: String,
    },
    /// The first value of an attribute argument is not of the type asked for.
    Attr {
        name: SymbolRef,
        expected: &'static str,
        found: String,
    },
}

impl fmt::Display for ArgError {
    /// `argument 1: expected a number, got `foo``, `argument 2: missing`,
    /// `@channels: expected a number, got `x``, `@channels: missing`.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ArgError::Missing { index } => write!(f, "argument {}: missing", index + 1),
            ArgError::MissingAttr { name } => write!(f, "@{name}: missing"),
            ArgError::Type {
                index,
                expected,
                found,
            } => write!(
                f,
                "argument {}: expected {expected}, got `{found}`",
                index + 1
            ),
            ArgError::Attr {
                name,
                expected,
                found,
            } => write!(f, "@{name}: expected {expected}, got `{found}`"),
        }
    }
}

impl std::error::Error for ArgError {}

impl From<ArgError> for NewError {
    fn from(e: ArgError) -> Self {
        NewError::new(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        let foo = Atom::from(crate::sym!("foo"));
        assert_eq!(
            ArgTypeError::new("a number", &foo).at_arg(1).to_string(),
            "argument 2: expected a number, got `foo`"
        );
        assert_eq!(
            ArgError::Missing { index: 0 }.to_string(),
            "argument 1: missing"
        );
        assert_eq!(
            ArgError::MissingAttr {
                name: crate::sym!("channels")
            }
            .to_string(),
            "@channels: missing"
        );
    }
}
