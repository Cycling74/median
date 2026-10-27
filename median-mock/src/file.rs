//! `locatefile_extended`, `open_dialog`, `path_opensysfile`, `sysfile_readtextfile` and
//! `sysfile_close`: a minimal, deterministic model of the file API `median::file` calls.
//!
//! There is no file system. A test registers in-memory files with [`add_file`]; only those can be
//! found, and only for reading. `locatefile_extended` searches them by name (as a search path
//! would, whatever the volume) and by type; `open_dialog` is always cancelled; `path_opensysfile`
//! opens a registered file by name and volume; `sysfile_readtextfile` reads the rest of it into
//! a handle, translating line breaks as the flags say; `sysfile_close` releases the file handle.
//! Everything is dropped by [`Kernel::reset`](crate::Kernel::reset); [`open_file_count`] shows
//! the files left open. Encodings are not modelled: the bytes are copied as they are.

use max_sys::{t_filehandle, t_fourcc, t_handle, t_max_err, t_ptr_size, t_sysfile_text_flags};
use std::collections::BTreeMap;
use std::ffi::{CStr, c_char, c_short};
use std::sync::Mutex;

/// What the real functions return for "not found", "cancelled" and "bad argument": non-zero.
const FAIL: c_short = -1;
const ERR: t_max_err = -1;

struct File {
    name: String,
    vol: c_short,
    typ: t_fourcc,
    contents: Vec<u8>,
}

struct Open {
    file: usize,
    pos: usize,
}

#[derive(Default)]
struct Files {
    files: Vec<File>,
    /// Open files by handle address; the handles are leaked boxes, freed by `sysfile_close`.
    open: BTreeMap<usize, Open>,
}
static FILES: Mutex<Files> = Mutex::new(Files {
    files: Vec::new(),
    open: BTreeMap::new(),
});

fn files() -> std::sync::MutexGuard<'static, Files> {
    FILES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Register an in-memory file that `locatefile_extended` can find by `name` and
/// `path_opensysfile` open under the volume `vol`. Hold the kernel test lock while using this.
pub fn add_file(name: &str, vol: i16, typ: t_fourcc, contents: &[u8]) {
    files().files.push(File {
        name: name.to_owned(),
        vol: vol as c_short,
        typ,
        contents: contents.to_vec(),
    });
}

/// The number of files opened with `path_opensysfile` and not yet closed.
pub fn open_file_count() -> usize {
    files().open.len()
}

pub(crate) fn reset() {
    let mut f = files();
    f.files.clear();
    for (handle, _) in std::mem::take(&mut f.open) {
        // SAFETY: every key is the address of a `Box<u8>` leaked by `path_opensysfile`
        drop(unsafe { Box::from_raw(handle as *mut u8) });
    }
}

/// Finds a registered file by name (and by type if `types` lists any); writes its volume and type.
///
/// # Safety
/// `name` must be a NUL-terminated string, `outvol` and `outtype` null or writable, `filetypelist`
/// null or valid for `numtypes` types.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn locatefile_extended(
    name: *mut c_char,
    outvol: *mut c_short,
    outtype: *mut t_fourcc,
    filetypelist: *const t_fourcc,
    numtypes: c_short,
) -> c_short {
    if name.is_null() {
        return FAIL;
    }
    // SAFETY: the caller's contract
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let types: &[t_fourcc] = if filetypelist.is_null() || numtypes <= 0 {
        &[]
    } else {
        // SAFETY: the caller's contract
        unsafe { std::slice::from_raw_parts(filetypelist, numtypes as usize) }
    };
    let f = files();
    let Some(file) = f
        .files
        .iter()
        .find(|file| file.name == name && (types.is_empty() || types.contains(&file.typ)))
    else {
        return FAIL;
    };
    // SAFETY: the caller's contract
    unsafe {
        if !outvol.is_null() {
            *outvol = file.vol;
        }
        if !outtype.is_null() {
            *outtype = file.typ;
        }
    }
    0
}

/// The user always cancels: non-zero, and the outputs are left alone.
///
/// # Safety
/// None needed; the pointers are not used.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn open_dialog(
    _name: *mut c_char,
    _volptr: *mut c_short,
    _typeptr: *mut t_fourcc,
    _types: *mut t_fourcc,
    _ntypes: c_short,
) -> c_short {
    FAIL
}

/// Opens a registered file for reading. Any other permission is refused.
///
/// # Safety
/// `name` must be a NUL-terminated string and `ref_` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn path_opensysfile(
    name: *const c_char,
    path: c_short,
    ref_: *mut t_filehandle,
    perm: c_short,
) -> c_short {
    if name.is_null()
        || ref_.is_null()
        || perm as u32 != max_sys::e_max_openfile_permissions::PATH_READ_PERM
    {
        return FAIL;
    }
    // SAFETY: the caller's contract
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let mut f = files();
    let Some(file) = f
        .files
        .iter()
        .position(|file| file.name == name && file.vol == path)
    else {
        return FAIL;
    };
    let handle = Box::into_raw(Box::new(0u8)) as usize;
    f.open.insert(handle, Open { file, pos: 0 });
    // SAFETY: `ref_` is writable
    unsafe { *ref_ = handle as t_filehandle };
    0
}

/// Releases a file opened with `path_opensysfile`; anything else is an error.
///
/// # Safety
/// `f` must be a handle from `path_opensysfile` or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysfile_close(f: t_filehandle) -> t_max_err {
    if files().open.remove(&(f as usize)).is_none() {
        return ERR;
    }
    // SAFETY: the handle was a leaked `Box<u8>`, and it was open until now
    drop(unsafe { Box::from_raw(f as *mut u8) });
    0
}

/// Reads up to `maxlen` bytes (all if 0) from the file position into `htext`, with the line
/// breaks translated to the kind in `flags` and a NUL added if asked for.
///
/// # Safety
/// `f` must be a handle from `path_opensysfile` and `htext` a live handle from `sysmem_newhandle`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sysfile_readtextfile(
    f: t_filehandle,
    htext: t_handle,
    maxlen: t_ptr_size,
    flags: t_sysfile_text_flags::Type,
) -> t_max_err {
    if htext.is_null() {
        return ERR;
    }
    let text = {
        let mut files = files();
        let Files { files, open } = &mut *files;
        let Some(open) = open.get_mut(&(f as usize)) else {
            return ERR;
        };
        let rest = &files[open.file].contents[open.pos..];
        let take = if maxlen == 0 {
            rest.len()
        } else {
            rest.len().min(maxlen)
        };
        open.pos += take;
        // `ext_sysfile.h`: line breaks are translated only when a `TEXT_LB_*` flag asks for it
        let target: Option<&[u8]> = match flags & t_sysfile_text_flags::TEXT_LB_MASK {
            0 => None,
            t_sysfile_text_flags::TEXT_LB_MAC => Some(b"\r"),
            t_sysfile_text_flags::TEXT_LB_PC => Some(b"\r\n"),
            t_sysfile_text_flags::TEXT_LB_UNIX => Some(b"\n"),
            _ if cfg!(windows) => Some(b"\r\n"),
            _ => Some(b"\n"),
        };
        let mut text = Vec::with_capacity(take + 1);
        if let Some(target) = target {
            let mut bytes = rest[..take].iter().copied().peekable();
            while let Some(b) = bytes.next() {
                match b {
                    b'\r' => {
                        if bytes.peek() == Some(&b'\n') {
                            bytes.next();
                        }
                        text.extend_from_slice(target);
                    }
                    b'\n' => text.extend_from_slice(target),
                    b => text.push(b),
                }
            }
        } else {
            text.extend_from_slice(&rest[..take]);
        }
        if flags & t_sysfile_text_flags::TEXT_NULL_TERMINATE != 0 {
            text.push(0);
        }
        text
    };
    // SAFETY: the caller's contract; the master pointer holds a block from the `sysmem` module
    unsafe {
        let data = crate::sysmem::sysmem_resizeptr((*htext).cast(), text.len());
        if data.is_null() {
            return ERR;
        }
        *htext = data.cast();
        std::ptr::copy_nonoverlapping(text.as_ptr(), data.cast::<u8>(), text.len());
    }
    0
}
