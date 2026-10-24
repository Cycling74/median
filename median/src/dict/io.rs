//! SDK dictionary parsers and file operations. In-memory serialization remains separate:
//! `jsonwriter.getoutput` is not a documented C entry point.
use super::*;
use crate::file::FilePath;

impl Dict {
    // All three SDK constructors write an owned dictionary into an initially null slot.
    fn construct(
        f: impl FnOnce(*mut *mut sys::t_dictionary) -> sys::t_max_err,
    ) -> DictResult<Self> {
        non_audio();
        let mut ptr = std::ptr::null_mut();
        let code = f(&mut ptr);
        if code != 0 {
            if !ptr.is_null() {
                unsafe {
                    sys::object_free(ptr.cast());
                }
            }
            return Err(DictError::Kernel(code));
        }
        let dict = unsafe { Self::from_raw(ptr, Ownership::Retained)? };
        mark_fresh(ptr as usize);
        Ok(dict)
    }

    /// Parse a JSON object with Max's dictionary parser. Embedded NULs are rejected before
    /// calling Max. Kernel parse failures retain their error code.
    pub fn from_json(json: &str) -> DictResult<Self> {
        let text = CString::new(json).map_err(|_| DictError::InvalidString)?;
        // The SDK's dict.route example supplies 256 bytes for this parser's diagnostic.
        let mut diagnostic = [0 as core::ffi::c_char; 256];
        Self::construct(|out| unsafe {
            sys::dictobj_dictionaryfromstring(out, text.as_ptr(), 1, diagnostic.as_mut_ptr())
        })
    }

    /// Parse Max dictionary-syntax atoms. The SDK copies them; object atoms are refused.
    pub fn from_atoms(atoms: &[Atom]) -> DictResult<Self> {
        let count = validate_atoms(atoms)?;
        Self::construct(|out| unsafe {
            sys::dictobj_dictionaryfromatoms(out, count, atoms.as_ptr().cast())
        })
    }

    /// Read JSON from a Max file/path pair, without a dialog or implicit search-path lookup.
    pub fn read(path: &FilePath) -> DictResult<Self> {
        Self::construct(|out| unsafe {
            sys::dictionary_read(path.file_name.as_ptr(), path.vol, out)
        })
    }

    /// Write JSON to a Max file/path pair. A private deep copy keeps serialization separate
    /// from later edits to this handle. Weak foreign entries retain SDK clone semantics.
    pub fn write(&self, path: &FilePath) -> DictResult<()> {
        let snapshot = self.deep_clone()?;
        check(unsafe {
            sys::dictionary_write(snapshot.as_ptr(), path.file_name.as_ptr(), path.vol)
        })
    }

    /// Dump a private snapshot using the SDK's console/debug output routine.
    pub fn dump(&self, recurse: bool, console: bool) -> DictResult<()> {
        let snapshot = self.deep_clone()?;
        check(unsafe {
            sys::dictionary_dump(snapshot.as_ptr(), recurse as c_long, console as c_long)
        })
    }
}
