//! String references.

use std::cell::UnsafeCell;
use std::convert::{From, TryFrom};
use std::ffi::CString;
use std::fmt::{Display, Formatter};

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

    /// Try to convert to a rust String.
    pub fn to_string(&self) -> Result<String, std::str::Utf8Error> {
        unsafe {
            match CString::from_raw(self.inner_ref().s_name).to_str() {
                Ok(s) => Ok(s.to_string()),
                Err(e) => Err(e),
            }
        }
    }
}

unsafe impl Send for SymbolRef {}
unsafe impl Sync for SymbolRef {}

impl From<*mut max_sys::t_symbol> for SymbolRef {
    fn from(v: *mut max_sys::t_symbol) -> Self {
        Self::new(v)
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
        match CString::new(v.as_str()) {
            Ok(s) => Ok(Self::from(s)),
            Err(_) => Err(&"couldn't create CString"),
        }
    }
}

impl Display for SymbolRef {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        unsafe {
            write!(
                f,
                "{}",
                CString::from_raw(self.inner_ref().s_name)
                    .to_str()
                    .expect("failed to convert to str")
            )
        }
    }
}

impl Clone for SymbolRef {
    fn clone(&self) -> Self {
        unsafe { Self::new(self.inner()) }
    }
}
