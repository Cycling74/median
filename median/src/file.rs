use crate::symbol::SymbolRef;
use std::{
    ffi::{CStr, CString},
    os::raw::c_char,
};

//XXX can we get this from the SDK?
const MAX_PATH_CHARS: usize = 2048;

pub struct FileInfo {
    //The name of the file.
    pub file_name: CString,
    //The volume the file exists in.
    pub vol: std::os::raw::c_short,
    //The fourcc type of the file.
    pub typ: max_sys::t_fourcc,
}

pub enum TextLineBreak {
    ///Use the linebreak format native to the current platform.
    Native,
    ///Use Macintosh line breaks.
    Mac,
    ///Use Windows line breaks.
    Windows,
    ///Use Unix line breaks.
    Unix,
}

impl Default for TextLineBreak {
    fn default() -> Self {
        Self::Native
    }
}

/// Error reading from a file.
pub enum FileReadError {
    ErrorOpening,
}

/// Error locating a file.
pub enum LocateError {
    NameEmpty,
    NotFound,
}

impl FileInfo {
    /// Find the file by name, if it isn't empty, otherwise, present user with dialog.
    ///
    /// # Arguments
    /// * `name` - The name of the file to look for, if empty, opens dialog.
    /// * `types` - An optional list of file types to filter the search or open dialog, pass `None` to
    /// disable filtering.
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
    /// filtering.
    ///
    /// # Remarks
    /// Will return `LocateError::NameEmpty` if the name is empty.
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

            unsafe {
                //copy the name into the space we have
                let name = name.to_cstring().into_bytes_with_nul();
                for (f, n) in file_name.iter_mut().zip(name) {
                    *f = n as i8;
                }
                if max_sys::locatefile_extended(
                    file_name.as_mut_ptr(),
                    &mut vol,
                    &mut typ,
                    types_ptr,
                    len as _,
                ) == 0
                {
                    Ok(FileInfo {
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
    /// filtering.
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
                std::mem::transmute::<_, _>(types_ptr), //max sdk should have made this const
                len as _,
            ) == 0
            {
                Some(FileInfo {
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
                max_sys::sysfile_readtextfile(
                    fh,
                    texthandle,
                    maxlen.unwrap_or(0) as _,
                    lb | max_sys::t_sysfile_text_flags::TEXT_NULL_TERMINATE,
                );
                let out = CStr::from_ptr(*texthandle).to_owned();
                max_sys::sysmem_freehandle(texthandle);
                max_sys::sysfile_close(fh);
                Ok(out)
            } else {
                Err(FileReadError::ErrorOpening)
            }
        }
    }
}
