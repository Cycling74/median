//! Max symbols: interned strings.
//!
//! Max keeps one `t_symbol` per distinct string in its symbol table (`gensym`), and symbols are
//! never removed from it, so a symbol pointer is a cheap, comparable, process-lifetime handle to a
//! string. [`SymbolRef`] is that pointer: `Copy`, compared and hashed by address (which, because
//! of interning, is the same as comparing the strings), and with no interior mutability.
//!
//! Make one with [`sym!`](crate::sym) for a literal (checked at compile time), `From<&CStr>`
//! (`SymbolRef::from(c"foo")`), or `TryFrom<&str>`/`TryFrom<String>` for a runtime string, which
//! fails if the string contains a NUL byte. Each of these calls `gensym`, which looks the string up
//! in Max's table (and takes a lock to do so), so on the audio thread use symbols made in advance.

use std::{
    convert::{From, TryFrom},
    ffi::{CStr, CString, NulError},
    fmt::{self, Debug, Display, Formatter},
    ptr::NonNull,
    str::Utf8Error,
};

/// A Max symbol (`t_symbol *`), see the [module documentation](self).
///
/// Never null: [`from_raw`](SymbolRef::from_raw) maps a null pointer to the empty symbol, which is
/// also the [`Default`].
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SymbolRef(NonNull<max_sys::t_symbol>);

// SAFETY: a `SymbolRef` is only a pointer to a symbol in Max's symbol table. Symbols are made by
// `gensym`, which is thread safe, are never freed (they live for the process), and their name
// (`s_name`) is never changed after `gensym` made them (the SDK, `ext_mess.h`: "never manipulate
// the s_name field"). `SymbolRef` only ever reads `s_name`, through the raw pointer and without
// making a reference to the whole `t_symbol` (Max writes the other field, `s_thing`, when an
// object binds to the symbol). So the pointer can be sent to, and used from, any thread.
unsafe impl Send for SymbolRef {}
// SAFETY: see `Send`; `SymbolRef` has no interior mutability, `&SymbolRef` only reads `s_name`.
unsafe impl Sync for SymbolRef {}

impl SymbolRef {
    /// Wrap a raw symbol pointer, mapping null to the empty symbol ([`SymbolRef::default`]).
    ///
    /// Mapping null rather than refusing it is deliberate: the typed method trampolines (and
    /// `Atom::get_symbol`, `Notification`) pass the `t_symbol *` Max hands them straight here, and
    /// rely on a null one arriving as the empty symbol instead of being undefined behaviour.
    ///
    /// # Safety
    /// If `v` is not null it must point to a symbol from Max's symbol table, for example one
    /// returned by `gensym` or handed to you by Max (as a method argument, in an atom, ...).
    /// Such symbols are valid for the rest of the process, which the returned `SymbolRef` (being
    /// `Copy` and `'static`) relies on.
    pub unsafe fn from_raw(v: *mut max_sys::t_symbol) -> Self {
        match NonNull::new(v) {
            Some(p) => Self(p),
            None => Self::default(),
        }
    }

    /// The raw symbol pointer, for passing to the Max API. Never null.
    pub fn as_ptr(&self) -> *mut max_sys::t_symbol {
        self.0.as_ptr()
    }

    /// The symbol's name.
    ///
    /// The lifetime is tied to `self` rather than `'static`: Max keeps symbols for the life of
    /// the process (and `SymbolRef` relies on that), but the SDK headers do not state it, so the
    /// API does not promise more than it has to.
    pub fn name(&self) -> &CStr {
        // SAFETY: `self.0` points to a symbol from the symbol table (`from_raw`'s contract, or
        // `gensym`), whose `s_name` is a NUL-terminated string that is never changed or freed.
        // Only the `s_name` field is read, through the raw pointer.
        unsafe { CStr::from_ptr((*self.0.as_ptr()).s_name) }
    }

    /// The symbol's name as a `&str`, if it is valid UTF-8 (Max symbols normally are).
    pub fn to_str(&self) -> Result<&str, Utf8Error> {
        self.name().to_str()
    }

    /// Is this the empty symbol (`""`, Max's `s_nothing`)?
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl From<SymbolRef> for *mut max_sys::t_symbol {
    fn from(s: SymbolRef) -> Self {
        s.as_ptr()
    }
}

impl From<&CStr> for SymbolRef {
    fn from(v: &CStr) -> Self {
        // SAFETY: `v` is a NUL-terminated string; `gensym` returns a symbol from the table
        unsafe { Self::from_raw(max_sys::gensym(v.as_ptr())) }
    }
}

impl From<CString> for SymbolRef {
    fn from(v: CString) -> Self {
        Self::from(v.as_c_str())
    }
}

impl TryFrom<&str> for SymbolRef {
    type Error = NulError;
    /// Fails if `v` contains a NUL byte.
    fn try_from(v: &str) -> Result<Self, Self::Error> {
        CString::new(v).map(Self::from)
    }
}

impl TryFrom<String> for SymbolRef {
    type Error = NulError;
    /// Fails if `v` contains a NUL byte.
    fn try_from(v: String) -> Result<Self, Self::Error> {
        CString::new(v).map(Self::from)
    }
}

impl Default for SymbolRef {
    /// The empty symbol, Max's `s_nothing`.
    fn default() -> Self {
        let p = crate::max::common_symbols().s_nothing;
        // the common symbols table is filled by Max before any external is loaded
        Self(NonNull::new(p).expect("the common symbols table has no s_nothing"))
    }
}

impl Display for SymbolRef {
    /// The name, with invalid UTF-8 replaced (`to_string_lossy`).
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(&self.name().to_string_lossy(), f)
    }
}

impl Debug for SymbolRef {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_tuple("SymbolRef").field(&self.name()).finish()
    }
}

/// A [`SymbolRef`] for a string literal, checked at compile time: `median::sym!("foo")`.
///
/// It expands to `SymbolRef::from(c"foo")` (spelled so that it works in a crate of any edition),
/// so it calls `gensym` each time it runs. A literal that contains a NUL byte is a compile error:
///
/// ```compile_fail,E0080
/// let s = median::sym!("a\0b");
/// ```
#[macro_export]
macro_rules! sym {
    ($name:literal) => {{
        const __MEDIAN_SYM: &::core::ffi::CStr =
            match ::core::ffi::CStr::from_bytes_with_nul(::core::concat!($name, "\0").as_bytes()) {
                ::core::result::Result::Ok(c) => c,
                ::core::result::Result::Err(_) => {
                    ::core::panic!("sym!: the symbol name contains a NUL byte")
                }
            };
        $crate::symbol::SymbolRef::from(__MEDIAN_SYM)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_handles_non_utf8_names() {
        let name = CString::new(b"name\xff".to_vec()).unwrap();
        let mut symbol = max_sys::t_symbol {
            s_name: name.as_ptr() as *mut _,
            s_thing: std::ptr::null_mut(),
        };
        let symbol = SymbolRef::new(&mut symbol);
        assert_eq!(format!("{}", symbol), "name\u{fffd}");
        assert!(symbol.to_string().is_err());
    }

    #[test]
    fn size_align() {
        assert_eq!(
            std::mem::size_of::<*mut max_sys::t_symbol>(),
            std::mem::size_of::<SymbolRef>()
        );
        assert_eq!(
            std::mem::size_of::<*mut max_sys::t_symbol>(),
            std::mem::size_of::<Option<SymbolRef>>()
        );
        assert_eq!(
            std::mem::align_of::<*mut max_sys::t_symbol>(),
            std::mem::align_of::<SymbolRef>()
        );
    }

    #[test]
    fn interned() {
        let a = SymbolRef::try_from("foo").unwrap();
        let b = SymbolRef::from(c"foo");
        let c = crate::sym!("foo");
        let d = SymbolRef::try_from(String::from("foo")).unwrap();
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_eq!(a, d);
        assert_eq!(a.as_ptr(), c.as_ptr());
        assert_ne!(a, crate::sym!("bar"));
        let mut set = std::collections::HashSet::new();
        set.insert(a);
        assert!(set.contains(&crate::sym!("foo")));
        assert!(!set.contains(&crate::sym!("fo")));
    }

    #[test]
    fn names() {
        let s = crate::sym!("héllo");
        assert_eq!(s.name(), c"héllo");
        assert_eq!(s.to_str(), Ok("héllo"));
        assert_eq!(s.to_string(), "héllo");
        assert_eq!(format!("{s:?}"), r#"SymbolRef("héllo")"#);
        assert_eq!(format!("[{s:>7}]"), "[  héllo]");
        // invalid UTF-8: `to_str` fails, `Display` is lossy
        let bad = SymbolRef::from(c"a\xffb");
        assert!(bad.to_str().is_err());
        assert_eq!(bad.to_string(), "a\u{fffd}b");
    }

    #[test]
    fn nul_is_an_error() {
        assert!(SymbolRef::try_from("a\0b").is_err());
        assert!(SymbolRef::try_from(String::from("\0")).is_err());
    }

    #[test]
    fn null_and_default_are_the_empty_symbol() {
        // SAFETY: null is allowed
        let n = unsafe { SymbolRef::from_raw(std::ptr::null_mut()) };
        assert_eq!(n, SymbolRef::default());
        assert!(n.is_empty());
        assert_eq!(n, crate::sym!(""));
        assert_eq!(n.name(), c"");
        assert!(!crate::sym!("x").is_empty());
        // SAFETY: a pointer from `gensym`
        let r = unsafe { SymbolRef::from_raw(crate::sym!("x").as_ptr()) };
        assert_eq!(r, crate::sym!("x"));
    }

    #[test]
    fn send_and_sync() {
        fn check<T: Send + Sync + Copy + 'static>() {}
        check::<SymbolRef>();
        let s = crate::sym!("threads");
        let t = std::thread::spawn(move || (s, crate::sym!("threads")));
        let (a, b) = t.join().unwrap();
        assert_eq!(a, b);
        assert_eq!(a, s);
    }
}
