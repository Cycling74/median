//! A port of the SDK's `misc/filein`: reads a binary file and outputs the unsigned 8, 16 or 32
//! bit value at the offsets it is sent.
//!
//! The file is opened with `std::fs`, at the path `path_toabsolutesystempath` gives for what
//! `FilePath` found, rather than with Max's `sysfile` functions: Rust's `File` closes itself and
//! reads into a `Vec`, and only one raw call is needed. The C's bang after reading the file named
//! by its argument, sent from `new` where nothing can receive it, is not sent. median-mock has no
//! `path_toabsolutesystempath`, so reading a file (and spooling) is untested.

use std::{
    ffi::{CStr, c_char},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::Mutex,
};

use median::{file::FilePath, max_sys, prelude::*, thread::SysThread};

const MAX_PATH_CHARS: usize = 2048;

enum Source {
    Closed,
    Spool(File),
    Data(Vec<u8>),
}

enum Access {
    Value(t_atom_long),
    Eof,
    Error(&'static str),
}

#[derive(MaxObj)]
#[max(name = "median.filein")]
pub struct Filein {
    obj: ObjRef,
    source: Mutex<Source>,
    // Spooled reads that arrived on the scheduler thread, as (width, offset), run by `drain`.
    pending: Mutex<Vec<(usize, t_atom_long)>>,
    drain: Queue,
    out: Outlet<Int>,
    eof: Outlet<Bang>,
    done: Outlet<Bang>,
}

impl MaxObjWrapped for Filein {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let name: SymbolRef = builder.args().at(0).unwrap_or_default()?;
        let spool: SymbolRef = builder.args().at(1).unwrap_or_default()?;
        builder.inlet(Inlet::default().assist("Read 8 bit unsigned integer from File"));
        builder.inlet(
            Inlet::int(|s: &Self, n| s.access(2, n, true))
                .assist("Read 16 bit unsigned integer from File"),
        );
        builder.inlet(
            Inlet::int(|s: &Self, n| s.access(4, n, true))
                .assist("Read 32 bit unsigned integer from File"),
        );
        let obj = builder.obj();
        let source = if name.is_empty() {
            Source::Closed
        } else {
            match load(name, spool == median::sym!("spool")) {
                Ok(source) => source.unwrap_or(Source::Closed),
                Err(e) => {
                    obj.error(e);
                    Source::Closed
                }
            }
        };
        Ok(Self {
            obj,
            source: Mutex::new(source),
            pending: Mutex::new(Vec::new()),
            drain: builder.queue(Self::drain),
            out: builder.outlet(Outlet::int().assist("File Data Output")),
            eof: builder.outlet(Outlet::bang().assist("bang On End of File")),
            done: builder.outlet(Outlet::bang().assist("bang When Read/Spool Finished")),
        })
    }
}

#[median::methods]
impl Filein {
    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.access(1, n, true);
    }

    #[sel("fclose")]
    pub fn fclose(&self) {
        *self.source.lock().unwrap() = Source::Closed;
    }

    #[sel("read", defaults = 1)]
    #[defer]
    pub fn read(&self, name: SymbolRef) -> Result<(), String> {
        self.open(name, false)
    }

    #[sel("spool", defaults = 1)]
    #[defer]
    pub fn spool(&self, name: SymbolRef) -> Result<(), String> {
        self.open(name, true)
    }

    fn open(&self, name: SymbolRef, spool: bool) -> Result<(), String> {
        self.fclose();
        if let Some(source) = load(name, spool)? {
            *self.source.lock().unwrap() = source;
            let _ = self.done.send();
        }
        Ok(())
    }

    fn drain(&self) {
        let pending = std::mem::take(&mut *self.pending.lock().unwrap());
        for (width, n) in pending {
            self.access(width, n, false);
        }
    }

    /// Output the `width`-byte value at offset `n`. A spooled read that arrives on the scheduler
    /// thread is moved to the main thread if `may_defer`.
    fn access(&self, width: usize, n: t_atom_long, may_defer: bool) {
        let result = match &mut *self.source.lock().unwrap() {
            Source::Closed => return,
            Source::Spool(_) if may_defer && SysThread::is_scheduler() => {
                self.pending.lock().unwrap().push((width, n));
                self.drain.set();
                return;
            }
            Source::Spool(file) => read_spooled(file, width, n),
            Source::Data(data) => match usize::try_from(n) {
                Err(_) => Access::Error("access out of range"),
                Ok(n) => match data.get(n..n.saturating_add(width)) {
                    Some(bytes) => Access::Value(value(bytes)),
                    None => Access::Eof,
                },
            },
        };
        match result {
            Access::Value(v) => {
                let _ = self.out.send(v);
            }
            Access::Eof => {
                let _ = self.eof.send();
            }
            Access::Error(e) => self.obj.error(e),
        }
    }
}

fn read_spooled(file: &mut File, width: usize, n: t_atom_long) -> Access {
    let Ok(pos) = u64::try_from(n) else {
        return Access::Error("seek err");
    };
    if file.seek(SeekFrom::Start(pos)).is_err() {
        return Access::Error("seek err");
    }
    let mut bytes = Vec::with_capacity(width);
    match file.take(width as u64).read_to_end(&mut bytes) {
        Ok(count) if count == width => Access::Value(value(&bytes)),
        _ => Access::Eof,
    }
}

// Native byte order, as the C reads the bytes in place.
fn value(bytes: &[u8]) -> t_atom_long {
    match *bytes {
        [b] => b as t_atom_long,
        [a, b] => u16::from_ne_bytes([a, b]) as t_atom_long,
        [a, b, c, d] => u32::from_ne_bytes([a, b, c, d]) as t_atom_long,
        _ => unreachable!("widths are 1, 2 or 4"),
    }
}

/// Find the file (with a dialog if `name` is empty) and open or read it. `Ok(None)` if the
/// dialog was cancelled.
fn load(name: SymbolRef, spool: bool) -> Result<Option<Source>, String> {
    let found = if name.is_empty() {
        match FilePath::open_dialog(None) {
            Some(f) => f,
            None => return Ok(None),
        }
    } else {
        FilePath::locate(&name, None).map_err(|_| format!("{name}: can't find file"))?
    };
    let shown = found.file_name.to_string_lossy();
    let path = system_path(&found).ok_or_else(|| format!("{shown}: error opening file"))?;
    let mut file = File::open(path).map_err(|e| format!("{shown}: error {e} opening file"))?;
    if spool {
        return Ok(Some(Source::Spool(file)));
    }
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut data = Vec::new();
    usize::try_from(len)
        .ok()
        .and_then(|len| data.try_reserve_exact(len).ok())
        .ok_or_else(|| format!("{shown} too big to read"))?;
    file.read_to_end(&mut data)
        .map_err(|e| format!("{shown}: error {e} reading file"))?;
    Ok(Some(Source::Data(data)))
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

median::ext_main! { Filein }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn closed_until_a_file_is_read() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.filein", &[]);
        assert!(!x.is_null());
        k.send_int(x, 0).unwrap();
        k.send_inlet(x, 1, "int", &[Atom::Long(0)]).unwrap();
        k.send_inlet(x, 2, "int", &[Atom::Long(0)]).unwrap();
        // median-mock's `open_dialog` is always cancelled.
        k.send(x, "read", &[]).unwrap();
        k.send(x, "fclose", &[]).unwrap();
        k.run_main_queue();
        assert!(k.take_outlet_log().is_empty());
        assert!(k.take_console_log().is_empty());
        let assists: Vec<_> = (0..3).map(|i| k.assist(x, true, i).unwrap()).collect();
        assert_eq!(
            assists,
            [
                "File Data Output",
                "bang On End of File",
                "bang When Read/Spool Finished"
            ]
        );
        assert_eq!(
            k.assist(x, false, 2).as_deref(),
            Some("Read 32 bit unsigned integer from File")
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn reports_a_missing_file() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.filein", &[Atom::Sym("nothere".into())]);
        assert!(!x.is_null());
        k.send(x, "spool", &[Atom::Sym("missing".into())]).unwrap();
        let errors: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(
            errors,
            ["nothere: can't find file", "missing: can't find file"]
        );
        assert!(k.take_outlet_log().is_empty());
        assert!(k.free_object(x));
    }

    #[test]
    fn values_are_unsigned() {
        assert_eq!(super::value(&[0xff]), 255);
        assert_eq!(super::value(&0xfffe_u16.to_ne_bytes()), 0xfffe);
        assert_eq!(super::value(&0xffff_fffe_u32.to_ne_bytes()), 0xffff_fffe);
    }
}
