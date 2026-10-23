//! Things that refer to files.

use crate::symbol::SymbolRef;
use std::{
    ffi::{CStr, CString},
    os::raw::c_char,
};

//XXX can we get this from the SDK?
const MAX_PATH_CHARS: usize = 2048;

//TODO macro that makes this nicer
pub fn fourcc(s: [char; 4]) -> max_sys::t_fourcc {
    ((s[0] as u8) as max_sys::t_fourcc) << 24
        | ((s[1] as u8) as max_sys::t_fourcc) << 16
        | ((s[2] as u8) as max_sys::t_fourcc) << 8
        | ((s[3] as u8) as max_sys::t_fourcc)
}

/// Information about a file and its location.
pub struct FilePath {
    //The name of the file.
    pub file_name: CString,
    //The volume the file exists in.
    pub vol: std::os::raw::c_short,
    //The fourcc type of the file.
    pub typ: max_sys::t_fourcc,
}

/// Line break conversions.
#[derive(Default)]
pub enum TextLineBreak {
    ///Use the linebreak format native to the current platform.
    #[default]
    Native,
    ///Use Macintosh line breaks.
    Mac,
    ///Use Windows line breaks.
    Windows,
    ///Use Unix line breaks.
    Unix,
}

/// Error reading from a file.
pub enum FileReadError {
    ErrorOpening,
    /// The file was opened but its text could not be read.
    ErrorReading,
}

/// Error locating a file.
pub enum LocateError {
    NameEmpty,
    /// The name doesn't fit in a path buffer, including its terminating NUL.
    NameTooLong,
    NotFound,
}

impl FilePath {
    /// Find the file by name, if it isn't empty, otherwise, present user with dialog.
    ///
    /// # Arguments
    /// * `name` - The name of the file to look for, if empty, opens dialog.
    /// * `types` - An optional list of file types to filter the search or open dialog, pass `None` to
    ///   disable filtering.
    pub fn find_with_dialog(
        name: &SymbolRef,
        types: Option<&Vec<max_sys::t_fourcc>>,
    ) -> Option<Self> {
        if name.is_empty() {
            Self::open_dialog(types)
        } else {
            Self::locate(name, types).ok()
        }
    }
    /// Find a file by name. If a complete path is not specified, search for the name in the search path.
    ///
    /// # Arguments
    /// * `name` - The name of the file to look for.
    /// * `types` - An optional list of file types to filter the search, pass `None` to disable
    ///   filtering.
    ///
    /// # Remarks
    /// Will return `LocateError::NameEmpty` if the name is empty and `LocateError::NameTooLong` if
    /// it is `MAX_PATH_CHARS` bytes or longer.
    pub fn locate(
        name: &SymbolRef,
        types: Option<&Vec<max_sys::t_fourcc>>,
    ) -> Result<Self, LocateError> {
        if name.is_empty() {
            Err(LocateError::NameEmpty)
        } else {
            let mut vol: std::os::raw::c_short = 0;
            let mut typ: max_sys::t_fourcc = 0;
            let mut file_name = [0 as c_char; MAX_PATH_CHARS];

            let (types_ptr, len) = match types {
                Some(t) => (t.as_ptr(), t.len()),
                None => (std::ptr::null(), 0),
            };

            //copy the name into the space we have, leaving room for the terminating NUL
            let name = name.name().to_bytes();
            if name.len() >= MAX_PATH_CHARS {
                return Err(LocateError::NameTooLong);
            }
            for (f, n) in file_name.iter_mut().zip(name) {
                *f = *n as c_char;
            }

            unsafe {
                if max_sys::locatefile_extended(
                    file_name.as_mut_ptr(),
                    &mut vol,
                    &mut typ,
                    types_ptr,
                    len as _,
                ) == 0
                {
                    Ok(FilePath {
                        file_name: CStr::from_ptr(file_name.as_ptr()).to_owned(),
                        vol,
                        typ,
                    })
                } else {
                    Err(LocateError::NotFound)
                }
            }
        }
    }

    /// Present the user with the standard open file dialog.
    ///
    /// # Arguments
    /// * `types` - An optional list of file types to filter the display, pass `None` to disable
    ///   filtering.
    pub fn open_dialog(types: Option<&Vec<max_sys::t_fourcc>>) -> Option<Self> {
        let (types_ptr, len) = match types {
            Some(t) => (t.as_ptr(), t.len()),
            None => (std::ptr::null(), 0),
        };
        let mut file_name = [0 as c_char; MAX_PATH_CHARS];
        let mut vol: std::os::raw::c_short = 0;
        let mut typ: max_sys::t_fourcc = 0;
        unsafe {
            if max_sys::open_dialog(
                file_name.as_mut_ptr(),
                &mut vol,
                &mut typ,
                types_ptr.cast_mut(), //max sdk should have made this const
                len as _,
            ) == 0
            {
                Some(FilePath {
                    file_name: CStr::from_ptr(file_name.as_ptr()).to_owned(),
                    vol,
                    typ,
                })
            } else {
                None
            }
        }
    }

    /// Try to read text from the file.
    ///
    /// # Arguments
    /// * `line_breaks` - The linebreak translation.
    /// * `maxlen` - An optional maximum length to read, reads all if `None` or `Some(0)`.
    pub fn read_text(
        &self,
        line_breaks: TextLineBreak,
        maxlen: Option<usize>,
    ) -> Result<CString, FileReadError> {
        unsafe {
            let mut fh = std::mem::MaybeUninit::<max_sys::t_filehandle>::uninit();
            if max_sys::path_opensysfile(
                self.file_name.as_c_str().as_ptr(),
                self.vol,
                fh.as_mut_ptr(),
                max_sys::e_max_openfile_permissions::PATH_READ_PERM as _,
            ) == 0
            {
                let lb = match line_breaks {
                    TextLineBreak::Native => max_sys::t_sysfile_text_flags::TEXT_LB_NATIVE,
                    TextLineBreak::Mac => max_sys::t_sysfile_text_flags::TEXT_LB_MAC,
                    TextLineBreak::Windows => max_sys::t_sysfile_text_flags::TEXT_LB_PC,
                    TextLineBreak::Unix => max_sys::t_sysfile_text_flags::TEXT_LB_UNIX,
                } as max_sys::t_sysfile_text_flags::Type;
                let fh = fh.assume_init();
                let texthandle = max_sys::sysmem_newhandle(0);
                if texthandle.is_null() {
                    max_sys::sysfile_close(fh);
                    return Err(FileReadError::ErrorReading);
                }
                let res = if max_sys::sysfile_readtextfile(
                    fh,
                    texthandle,
                    maxlen.unwrap_or(0) as _,
                    lb | max_sys::t_sysfile_text_flags::TEXT_NULL_TERMINATE,
                ) == 0
                {
                    // a handle that holds no data is an empty text
                    if (*texthandle).is_null() {
                        Ok(CString::default())
                    } else {
                        Ok(CStr::from_ptr(*texthandle).to_owned())
                    }
                } else {
                    Err(FileReadError::ErrorReading)
                };
                // the handle is ours to free, and the file must be closed on every path
                max_sys::sysmem_freehandle(texthandle);
                max_sys::sysfile_close(fh);
                res
            } else {
                Err(FileReadError::ErrorOpening)
            }
        }
    }
}
