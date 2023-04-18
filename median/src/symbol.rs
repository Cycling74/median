//! String references.

use std::{
    cell::UnsafeCell,
    convert::{From, Into, TryFrom, TryInto},
    ffi::{CStr, CString},
    fmt::{Display, Formatter},
    hash::{Hash, Hasher},
};

#[repr(transparent)]
pub struct SymbolRef {
    pub(crate) value: UnsafeCell<*mut max_sys::t_symbol>,
}

/// A reference to a max symbol
impl SymbolRef {
    pub fn new(v: *mut max_sys::t_symbol) -> Self {
        Self {
            value: UnsafeCell::new(v),
        }
    }

    /// Update the symbol that this points to.
    pub fn assign(&self, v: &Self) {
        unsafe {
            *self.value.get() = v.inner();
        }
    }

    /// Get the raw symbol pointer.
    pub unsafe fn inner(&self) -> *mut max_sys::t_symbol {
        *self.value.get()
    }

    unsafe fn inner_ref(&self) -> &mut max_sys::t_symbol {
        &mut (*self.inner())
    }

    /// Convert to CString.
    pub fn to_cstring(&self) -> CString {
        unsafe { CStr::from_ptr(self.inner_ref().s_name).into() }
    }

    /// Try to convert to a rust String.
    pub fn to_string(&self) -> Result<String, std::str::Utf8Error> {
        self.to_cstring().to_str().map(|s| s.to_string())
    }

    /// Is the symbol ref empty
    pub fn is_empty(&self) -> bool {
        unsafe { *self.value.get() == crate::max::common_symbols().s_nothing }
    }
}

impl Hash for SymbolRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        unsafe {
            let s = CStr::from_ptr(self.inner_ref().s_name);
            s.hash(state);
        }
    }
}

unsafe impl Send for SymbolRef {}
unsafe impl Sync for SymbolRef {}

impl From<SymbolRef> for *mut max_sys::t_symbol {
    fn from(val: SymbolRef) -> Self {
        unsafe { val.inner() }
    }
}

impl TryInto<String> for SymbolRef {
    type Error = std::ffi::IntoStringError;
    fn try_into(self) -> Result<String, Self::Error> {
        let c: CString = unsafe { CStr::from_ptr(self.inner_ref().s_name).into() };
        match c.into_string() {
            Ok(s) => Ok(s),
            Err(e) => Err(e),
        }
    }
}

impl From<*mut max_sys::t_symbol> for SymbolRef {
    fn from(v: *mut max_sys::t_symbol) -> Self {
        if v.is_null() {
            Self::new(crate::max::common_symbols().s_nothing)
        } else {
            Self::new(v)
        }
    }
}

impl From<&CStr> for SymbolRef {
    fn from(v: &CStr) -> Self {
        unsafe { SymbolRef::new(max_sys::gensym(v.as_ptr())) }
    }
}

impl From<CString> for SymbolRef {
    fn from(v: CString) -> Self {
        unsafe { SymbolRef::new(max_sys::gensym(v.as_ptr())) }
    }
}

impl TryFrom<String> for SymbolRef {
    type Error = &'static str;
    fn try_from(v: String) -> Result<Self, Self::Error> {
        return SymbolRef::try_from(v.as_str());
    }
}

impl TryFrom<&str> for SymbolRef {
    type Error = &'static str;
    fn try_from(v: &str) -> Result<Self, Self::Error> {
        match CString::new(v) {
            Ok(s) => Ok(Self::from(s)),
            Err(_) => Err("couldn't create CString"),
        }
    }
}

impl Display for SymbolRef {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "{}", self.to_string().expect("failed to convert to str"))
    }
}

impl Clone for SymbolRef {
    fn clone(&self) -> Self {
        unsafe { Self::new(self.inner()) }
    }
}

impl PartialEq for SymbolRef {
    fn eq(&self, other: &Self) -> bool {
        unsafe { self.inner() == other.inner() }
    }
}

impl Eq for SymbolRef {}

impl Default for SymbolRef {
    fn default() -> Self {
        Self::new(crate::max::common_symbols().s_nothing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_align() {
        assert_eq!(
            std::mem::size_of::<*mut max_sys::t_symbol>(),
            std::mem::size_of::<SymbolRef>()
        );
        assert_eq!(
            std::mem::align_of::<*mut max_sys::t_symbol>(),
            std::mem::align_of::<SymbolRef>()
        );
    }
}
