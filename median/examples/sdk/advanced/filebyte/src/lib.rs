//! A port of the SDK's `advanced/filebyte`: keeps a file open and outputs the byte at each offset
//! it is sent.
//!
//! The file is opened with `std::fs`, at the path `path_toabsolutesystempath` gives for what
//! `FilePath` found, rather than with Max's `sysfile` functions. median-mock has no
//! `path_toabsolutesystempath`, so opening a file (and reading from one) is untested.

use std::{
    ffi::{CStr, c_char},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::Mutex,
};

use median::{file::FilePath, max_sys, prelude::*};

const MAX_PATH_CHARS: usize = 2048;

#[derive(MaxObj)]
#[max(name = "median.filebyte")]
pub struct Filebyte {
    obj: ObjRef,
    file: Mutex<Option<File>>,
    out: Outlet<Int>,
}

impl MaxObjWrapped for Filebyte {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let name: SymbolRef = builder.args().at(0).unwrap_or_default()?;
        builder.inlet(Inlet::default().assist("(int) byte in file"));
        let x = Self {
            obj: builder.obj(),
            file: Mutex::new(None),
            out: builder.outlet(Outlet::int().assist("(int) byte in file output")),
        };
        if !name.is_empty() {
            x.do_open(name);
        }
        Ok(x)
    }
}

#[median::methods]
impl Filebyte {
    #[int]
    #[defer_low]
    pub fn int(&self, n: t_atom_long) {
        let mut file = self.file.lock().unwrap();
        let Some(file) = file.as_mut() else {
            self.obj.error("filebyte: no open file");
            return;
        };
        if !u64::try_from(n).is_ok_and(|p| file.seek(SeekFrom::Start(p)).is_ok()) {
            self.obj.error("seek err");
            return;
        }
        let mut byte = [0u8];
        match file.read_exact(&mut byte) {
            Ok(()) => {
                let _ = self.out.send(byte[0] as t_atom_long);
            }
            Err(e) => self.obj.error(format!("filebyte: read err {e}")),
        }
    }

    #[sel("fclose")]
    pub fn fclose(&self) {
        *self.file.lock().unwrap() = None;
    }

    #[sel("open", defaults = 1)]
    #[defer_low]
    pub fn open(&self, name: SymbolRef) {
        self.do_open(name);
    }

    fn do_open(&self, name: SymbolRef) {
        self.fclose();
        let found = if name.is_empty() {
            match FilePath::open_dialog(None) {
                Some(f) => f,
                None => return,
            }
        } else {
            match FilePath::locate(&name, None) {
                Ok(f) => f,
                Err(_) => {
                    self.obj.error(format!("{name}: can't find file"));
                    return;
                }
            }
        };
        let shown = found.file_name.to_string_lossy();
        match system_path(&found).map(File::open) {
            Some(Ok(file)) => *self.file.lock().unwrap() = Some(file),
            Some(Err(e)) => self.obj.error(format!("{shown}: error {e} opening file")),
            None => self.obj.error(format!("{shown}: error opening file")),
        }
    }
}

fn system_path(file: &FilePath) -> Option<PathBuf> {
    let mut buf = [0 as c_char; MAX_PATH_CHARS];
    // SAFETY: `file_name` is NUL-terminated and `buf` is the `MAX_PATH_CHARS` buffer the
    // function writes a NUL-terminated path into.
    // GAP: path_toabsolutesystempath
    let err = unsafe {
        max_sys::path_toabsolutesystempath(file.vol, file.file_name.as_ptr(), buf.as_mut_ptr())
    };
    if err != 0 {
        return None;
    }
    // SAFETY: as above, `buf` holds a NUL-terminated string.
    let path = unsafe { CStr::from_ptr(buf.as_ptr()) };
    path.to_str().ok().map(PathBuf::from)
}

median::ext_main! { Filebyte }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn setup() -> median_mock::Kernel {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        k
    }

    #[test]
    fn reads_nothing_without_a_file() {
        let k = setup();
        let x = k.new_object("median.filebyte", &[]);
        assert!(!x.is_null());
        k.send_int(x, 3).unwrap();
        // median-mock's `open_dialog` is always cancelled.
        k.send(x, "open", &[]).unwrap();
        assert!(
            k.take_console_log().is_empty(),
            "deferred until the queue runs"
        );
        k.run_main_queue();
        k.send(x, "fclose", &[]).unwrap();
        let errors: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(errors, ["filebyte: no open file"]);
        assert!(k.take_outlet_log().is_empty());
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("(int) byte in file"));
        assert_eq!(
            k.assist(x, true, 0).as_deref(),
            Some("(int) byte in file output")
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn reports_a_missing_file() {
        let k = setup();
        let x = k.new_object("median.filebyte", &[Atom::Sym("nothere".into())]);
        assert!(!x.is_null());
        k.send(x, "open", &[Atom::Sym("missing".into())]).unwrap();
        k.run_main_queue();
        let errors: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(
            errors,
            ["nothere: can't find file", "missing: can't find file"]
        );
        assert!(k.free_object(x));
    }
}
