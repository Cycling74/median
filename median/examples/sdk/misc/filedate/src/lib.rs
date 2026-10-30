//! A port of the SDK's `misc/filedate`: outputs the modification date of a file or folder.
//!
//! The date lookups have no median API and median-mock does not implement them, so the tests
//! cover only the empty name and the assist texts.

use std::{
    ffi::{c_char, c_short},
    mem::MaybeUninit,
};

use median::{file::FilePath, max_sys, prelude::*};

const MAX_PATH_CHARS: usize = 2048;

#[derive(MaxObj)]
#[max(name = "median.filedate")]
pub struct Filedate {
    out: Outlet<List>,
}

impl MaxObjWrapped for Filedate {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("File/Folder to Check Modification Date"));
        Ok(Self {
            out: builder.outlet(Outlet::list().assist("Mod Date (Month/Day/Year/Hour/Min/Sec)")),
        })
    }
}

#[median::methods]
impl Filedate {
    #[any]
    #[defer]
    pub fn anything(&self, sel: &SymbolRef, _atoms: &[Atom]) -> Result<(), String> {
        if sel.is_empty() {
            return Ok(());
        }
        let date = match FilePath::locate(sel, None) {
            Ok(file) => {
                let mut date = 0;
                // SAFETY: `file_name` is NUL-terminated and `date` is writable.
                // GAP: path_getfilemoddate
                let err = unsafe {
                    max_sys::path_getfilemoddate(file.file_name.as_ptr(), file.vol, &mut date)
                };
                if err != 0 {
                    return Err(format!("{sel}: error {err} getting date"));
                }
                date
            }
            // Not a file: try it as a folder.
            Err(_) => match folder_date(sel) {
                Some(Ok(date)) => date,
                Some(Err(err)) => return Err(format!("{sel}: error {err} getting date")),
                None => return Err(format!("{sel}: can't find")),
            },
        };
        let _ = self.out.send(&date_atoms(date));
        Ok(())
    }
}

/// `None` if `name` is not found; otherwise the folder's date or the error getting it.
fn folder_date(name: &SymbolRef) -> Option<Result<max_sys::t_ptr_uint, c_short>> {
    let bytes = name.name().to_bytes();
    if bytes.len() >= MAX_PATH_CHARS {
        return None;
    }
    let mut buf = [0 as c_char; MAX_PATH_CHARS];
    for (b, n) in buf.iter_mut().zip(bytes) {
        *b = *n as c_char;
    }
    let mut vol: c_short = 0;
    // SAFETY: `buf` is a NUL-terminated, writable `MAX_PATH_CHARS` buffer, as
    // `path_resolvefile` needs, and `vol` is writable.
    // GAP: path_resolvefile
    if unsafe { max_sys::path_resolvefile(buf.as_mut_ptr(), 0, &mut vol) } != 0 {
        return None;
    }
    let mut date = 0;
    // SAFETY: `date` is writable.
    // GAP: path_getmoddate
    let err = unsafe { max_sys::path_getmoddate(vol, &mut date) };
    Some(if err == 0 { Ok(date) } else { Err(err) })
}

fn date_atoms(date: max_sys::t_ptr_uint) -> [Atom; 6] {
    let mut dt = MaybeUninit::<max_sys::t_datetime>::uninit();
    // SAFETY: `systime_secondstodate` fills in every field of `dt`.
    // GAP: systime_secondstodate
    let dt = unsafe {
        max_sys::systime_secondstodate(date, dt.as_mut_ptr());
        dt.assume_init()
    };
    [dt.month, dt.day, dt.year, dt.hour, dt.minute, dt.second].map(|v| Atom::from(v as i64))
}

median::ext_main! { Filedate }

#[cfg(test)]
mod tests {
    #[test]
    fn ignores_the_empty_name() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.filedate", &[]);
        assert!(!x.is_null());
        k.send(x, "", &[]).unwrap();
        k.run_main_queue();
        assert!(k.take_outlet_log().is_empty());
        assert!(k.take_console_log().is_empty());
        assert_eq!(
            k.assist(x, false, 0).as_deref(),
            Some("File/Folder to Check Modification Date")
        );
        assert_eq!(
            k.assist(x, true, 0).as_deref(),
            Some("Mod Date (Month/Day/Year/Hour/Min/Sec)")
        );
        assert!(k.free_object(x));
    }
}
