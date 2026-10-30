//! A port of the SDK's `misc/folder`: lists the files of a folder that have one of a list of
//! types, as `clear` and `append <name>` messages for a menu, then their count.
//!
//! The default type list (`typelist_make`) is made when a folder is first listed rather than in
//! `new`, so that the object can be made in median-mock. The folder functions have no median API
//! and median-mock does not implement them, so listing a folder is untested.

use std::{
    ffi::{CStr, c_char, c_long, c_short},
    mem::MaybeUninit,
    sync::Mutex,
};

use median::{
    atom::AtomValue,
    file::fourcc,
    max_sys::{self, t_fourcc},
    prelude::*,
};

const MAXTYPES: usize = 128;
const MAX_PATH_CHARS: usize = 2048;

struct State {
    input: Option<SymbolRef>,
    // The folder's path ID, 0 until `input` is looked up.
    path: c_short,
    // `None` until set or resolved to Max's default list; empty matches every file.
    types: Option<Vec<t_fourcc>>,
}

#[derive(MaxObj)]
#[max(name = "median.folder")]
pub struct Folder {
    obj: ObjRef,
    state: Mutex<State>,
    out: Outlet<Any>,
    count: Outlet<Int>,
}

impl MaxObjWrapped for Folder {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("Path Name of Folder to List"));
        let args = builder.args().raw();
        let input = match args.first().and_then(Atom::get_value) {
            Some(AtomValue::Symbol(s)) => Some(s),
            _ => None,
        };
        let types = args.get(1..).map(parse_types).filter(|t| !t.is_empty());
        let obj = builder.obj();
        let path = input.map_or(0, |input| lookup(&obj, input));
        Ok(Self {
            obj,
            state: Mutex::new(State { input, path, types }),
            out: builder.outlet(Outlet::any().assist("File Name List, Connect to Menu")),
            count: builder.outlet(Outlet::int().assist("Count of Items Listed")),
        })
    }
}

#[median::methods]
impl Folder {
    #[bang]
    #[defer]
    pub fn bang(&self) {
        let (path, types) = {
            let mut state = self.state.lock().unwrap();
            if let (0, Some(input)) = (state.path, state.input) {
                state.path = lookup(&self.obj, input);
            }
            if state.path != 0 && state.types.is_none() {
                state.types = Some(default_types());
            }
            (state.path, state.types.clone().unwrap_or_default())
        };
        let count = if path == 0 {
            0
        } else {
            self.enumerate(path, &types)
        };
        let _ = self.count.send(count);
    }

    #[int]
    #[defer]
    pub fn int(&self, _n: t_atom_long) {
        self.bang();
    }

    #[any]
    #[defer]
    pub fn anything(&self, sel: &SymbolRef, _atoms: &[Atom]) {
        self.symbol(*sel);
    }

    #[sym]
    #[defer]
    pub fn symbol(&self, s: SymbolRef) {
        {
            let mut state = self.state.lock().unwrap();
            state.input = Some(s);
            state.path = 0;
        }
        self.bang();
    }

    #[gimme("types")]
    pub fn types(&self, atoms: &[Atom]) {
        self.state.lock().unwrap().types = Some(parse_types(atoms));
    }

    fn enumerate(&self, path: c_short, types: &[t_fourcc]) -> t_atom_long {
        // SAFETY: opening a folder by path ID has no preconditions.
        // GAP: path_openfolder
        let Some(folder) = std::ptr::NonNull::new(unsafe { max_sys::path_openfolder(path) }) else {
            return 0;
        };
        let folder = OpenFolder(folder);
        let mut name = [0 as c_char; MAX_PATH_CHARS];
        let mut count = 0;
        loop {
            let mut typ: t_fourcc = 0;
            // SAFETY: `folder` is open, `typ` is writable and `name` is a buffer of at least the
            // `MAX_FILENAME_CHARS` the function writes.
            // GAP: path_foldernextfile
            let more = unsafe {
                max_sys::path_foldernextfile(folder.0.as_ptr(), &mut typ, name.as_mut_ptr(), 0)
            };
            if more == 0 {
                break;
            }
            if !types.is_empty() {
                if typ == 0 {
                    typ = sniff_type(&name, path, types).unwrap_or(0);
                }
                if !types.contains(&typ) {
                    continue;
                }
            }
            if count == 0 {
                let _ = self.out.send(&median::sym!("clear"), &[]);
            }
            // SAFETY: `path_foldernextfile` wrote a NUL-terminated name.
            let file = SymbolRef::from(unsafe { CStr::from_ptr(name.as_ptr()) });
            let _ = self.out.send(&median::sym!("append"), &[file.into()]);
            count += 1;
        }
        count
    }
}

struct OpenFolder(std::ptr::NonNull<std::ffi::c_void>);

impl Drop for OpenFolder {
    fn drop(&mut self) {
        // SAFETY: the folder is open and closed only here.
        // GAP: path_closefolder
        unsafe { max_sys::path_closefolder(self.0.as_ptr()) };
    }
}

fn sniff_type(name: &[c_char], path: c_short, types: &[t_fourcc]) -> Option<t_fourcc> {
    let mut info = MaybeUninit::<max_sys::t_fileinfo>::uninit();
    // SAFETY: `name` is NUL-terminated, `info` is writable, `types` has `types.len()` entries
    // (at most `MAXTYPES`, which fits a `c_short`).
    // GAP: path_extendedfileinfo
    let err = unsafe {
        max_sys::path_extendedfileinfo(
            name.as_ptr(),
            path,
            info.as_mut_ptr(),
            types.as_ptr(),
            types.len() as c_short,
            1,
        )
    };
    // SAFETY: on success the function filled in `info`.
    (err == 0).then(|| unsafe { info.assume_init() }.type_)
}

/// The path ID of the folder `input`, or 0 (with an error) if it is not one.
fn lookup(obj: &ObjRef, input: SymbolRef) -> c_short {
    let mut path: c_short = 0;
    let mut file = [0 as c_char; MAX_PATH_CHARS];
    // SAFETY: the name is NUL-terminated, `path` is writable and `file` is large enough for the
    // file name part.
    // GAP: path_frompathname
    let err =
        unsafe { max_sys::path_frompathname(input.name().as_ptr(), &mut path, file.as_mut_ptr()) };
    if err != 0 || file[0] != 0 {
        median::object_error!(obj, "{input}: not a folder");
        0
    } else {
        path
    }
}

fn default_types() -> Vec<t_fourcc> {
    let mut types = [0; MAXTYPES];
    let mut n: c_short = 0;
    // SAFETY: `types` has room for the list Max makes (the SDK's `folder` uses the same size) and
    // `n` is writable.
    // GAP: typelist_make
    unsafe {
        max_sys::typelist_make(
            types.as_mut_ptr(),
            (max_sys::e_max_typelists::TYPELIST_MAXFILES
                | max_sys::e_max_typelists::TYPELIST_MAXFORLIVE) as c_long,
            &mut n,
        )
    };
    types[..(n.max(0) as usize).min(MAXTYPES)].to_vec()
}

/// The four-char codes of the symbols in `atoms`: their first four bytes, padded with spaces.
/// The C writes past its 128 entries when given more; here the rest are ignored.
fn parse_types(atoms: &[Atom]) -> Vec<t_fourcc> {
    atoms
        .iter()
        .filter_map(|a| match a.get_value() {
            Some(AtomValue::Symbol(s)) => {
                let mut code = [b' '; 4];
                for (c, b) in code.iter_mut().zip(s.name().to_bytes()) {
                    *c = *b;
                }
                Some(fourcc(code.map(char::from)))
            }
            _ => None,
        })
        .take(MAXTYPES)
        .collect()
}

median::ext_main! { Folder }

#[cfg(test)]
mod tests {
    use median::prelude::*;

    #[test]
    fn counts_nothing_without_a_folder() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.folder", &[]);
        assert!(!x.is_null());
        k.send(x, "types", &[median_mock::Atom::Sym("TEXT".into())])
            .unwrap();
        k.send_bang(x).unwrap();
        k.send_int(x, 3).unwrap();
        k.run_main_queue();
        let out: Vec<_> = k.take_outlet_log().into_iter().map(|e| e.atoms).collect();
        assert_eq!(
            out,
            [
                vec![median_mock::Atom::Long(0)],
                vec![median_mock::Atom::Long(0)]
            ]
        );
        assert_eq!(
            k.assist(x, true, 1).as_deref(),
            Some("Count of Items Listed")
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn types_are_padded_four_char_codes() {
        let _k = median_mock::lock();
        let atoms = [
            Atom::from(sym!("TEXT")),
            Atom::from(1i64),
            Atom::from(sym!("js")),
            Atom::from(sym!("maxpat")),
        ];
        assert_eq!(
            super::parse_types(&atoms),
            [
                u32::from_be_bytes(*b"TEXT"),
                u32::from_be_bytes(*b"js  "),
                u32::from_be_bytes(*b"maxp")
            ]
        );
    }
}
