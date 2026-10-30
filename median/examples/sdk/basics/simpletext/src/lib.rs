//! A port of the SDK's `basics/simpletext`: `read` loads a text file (a dialog without a name),
//! double-click shows it in a text editor window, and closing the editor keeps the edited text.
//!
//! Untested: the editor (`median-mock` has no `jed` class or `object_new`) and the open dialog
//! (always cancelled in the mock).

use std::{
    ffi::{CStr, CString, c_char, c_int, c_long, c_void},
    sync::{
        Mutex,
        atomic::{AtomicPtr, Ordering},
    },
};

use median::{
    file::{FilePath, TextLineBreak, fourcc},
    max_sys,
    prelude::*,
};

#[derive(MaxObj)]
#[max(name = "median.simpletext")]
pub struct SimpleText {
    obj: ObjRef,
    text: Mutex<CString>,
    // The open `jed` editor, or null; it frees itself when closed, after calling `edclose`.
    editor: AtomicPtr<max_sys::t_object>,
}

impl MaxObjWrapped for SimpleText {
    fn class_setup(class: &mut Class<MaxObjWrapper<Self>>) {
        // GAP: A_CANT method. `edclose` takes arguments, which no median method kind passes.
        // SAFETY: `edclose` has the signature Max calls `edclose` with; the class is live.
        unsafe {
            let m = std::mem::transmute::<
                unsafe extern "C" fn(*mut MaxObjWrapper<SimpleText>, *mut *mut c_char, c_long),
                unsafe extern "C" fn(*mut c_void) -> *mut c_void,
            >(edclose);
            max_sys::class_addmethod(
                class.inner(),
                Some(m),
                c"edclose".as_ptr(),
                max_sys::e_max_atomtypes::A_CANT as c_int,
                0,
            );
        }
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        // The SDK writes this assist text but never registers its assist method.
        builder.inlet(Inlet::default().assist("Message In"));
        Ok(Self {
            obj: builder.obj(),
            text: Mutex::new(CString::default()),
            editor: AtomicPtr::new(std::ptr::null_mut()),
        })
    }
}

impl Drop for SimpleText {
    fn drop(&mut self) {
        let editor = *self.editor.get_mut();
        if !editor.is_null() {
            // GAP: jed text editor.
            // SAFETY: the editor is live until `edclose` clears the pointer.
            unsafe { max_sys::object_free(editor.cast()) };
        }
    }
}

/// # Safety
/// `x` must be a live `median.simpletext` object and `text` point to a pointer to at least
/// `size` bytes.
unsafe extern "C" fn edclose(
    x: *mut MaxObjWrapper<SimpleText>,
    text: *mut *mut c_char,
    size: c_long,
) {
    // SAFETY: the caller's contract
    let w = unsafe { &*x };
    median::ffi::guard(w, "edclose", || {
        let this = w.wrapped();
        // SAFETY: the caller's contract
        let bytes = unsafe { std::slice::from_raw_parts((*text).cast::<u8>(), size as usize) };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        *this.text.lock().unwrap() = CString::new(&bytes[..end]).unwrap_or_default();
        this.editor.store(std::ptr::null_mut(), Ordering::Relaxed);
    });
}

#[median::methods]
impl SimpleText {
    #[sel("read", defaults = 1)]
    #[defer]
    pub fn read(&self, name: SymbolRef) {
        let types = vec![fourcc(['T', 'E', 'X', 'T'])];
        let path = if name.is_empty() {
            let Some(path) = FilePath::open_dialog(Some(&types)) else {
                return;
            };
            path
        } else {
            let Ok(path) = FilePath::locate(&name, Some(&types)) else {
                median::object_error!(&self.obj, "can't find file {}", name);
                return;
            };
            path
        };
        if let Ok(text) = path.read_text(TextLineBreak::Unix, None) {
            *self.text.lock().unwrap() = text;
        }
    }

    #[dblclick]
    pub fn dblclick(&self) {
        let sym = |s: &CStr| SymbolRef::from(s).as_ptr();
        let editor = self.editor.load(Ordering::Relaxed);
        // GAP: jed text editor.
        // SAFETY: `editor` is live until `edclose` clears it; `jed` takes its owner and a long,
        // `settext` a C string and an encoding symbol.
        unsafe {
            if !editor.is_null() {
                max_sys::object_attr_setchar(editor.cast(), sym(c"visible"), 1);
                return;
            }
            let editor: *mut max_sys::t_object =
                max_sys::object_new(sym(c"nobox"), sym(c"jed"), self.obj.as_ptr(), 0 as c_long)
                    .cast();
            if editor.is_null() {
                return;
            }
            self.editor.store(editor, Ordering::Relaxed);
            let text = self.text.lock().unwrap();
            max_sys::object_method(editor.cast(), sym(c"settext"), text.as_ptr(), sym(c"utf-8"));
            max_sys::object_attr_setchar(editor.cast(), sym(c"scratch"), 1);
            max_sys::object_attr_setsym(editor.cast(), sym(c"title"), sym(c"simpletext"));
        }
    }
}

median::ext_main! { SimpleText }

#[cfg(test)]
mod tests {
    use super::SimpleText;
    use median::prelude::*;
    use median_mock::Atom;
    use std::ffi::{c_char, c_long, c_void};

    fn text(x: *mut c_void) -> String {
        // SAFETY: `x` is a live `median.simpletext` object.
        let w = unsafe { &*(x as *const MaxObjWrapper<SimpleText>) };
        w.wrapped()
            .text
            .lock()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn reads_a_file() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        median_mock::file::add_file(
            "notes.txt",
            1,
            median::file::fourcc(['T', 'E', 'X', 'T']),
            b"one\r\ntwo",
        );
        let x = k.new_object("median.simpletext", &[]);
        assert!(!x.is_null());
        k.send(x, "read", &[Atom::Sym("notes.txt".into())]).unwrap();
        k.send(x, "read", &[Atom::Sym("missing.txt".into())])
            .unwrap();
        k.send(x, "read", &[]).unwrap();
        k.run_main_queue();
        assert_eq!(text(x), "one\ntwo");
        let console: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(console, ["can't find file missing.txt"]);
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("Message In"));
        assert!(k.free_object(x));
    }

    #[test]
    fn edclose_keeps_the_text() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.simpletext", &[]);
        let (f, _) = k.method(x, "edclose").unwrap();
        let mut edited = *b"edited";
        let mut p = edited.as_mut_ptr().cast::<c_char>();
        // SAFETY: `edclose` is called as Max calls it, with a handle to `edited.len()` bytes.
        unsafe {
            let f: unsafe extern "C" fn(*mut c_void, *mut *mut c_char, c_long) =
                std::mem::transmute(f);
            f(x, &mut p, edited.len() as c_long);
        }
        assert_eq!(text(x), "edited");
        assert!(k.free_object(x));
    }
}
