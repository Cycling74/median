//! The symbol table: `gensym` and the common symbols table.
//!
//! Symbols are interned for the life of the process: a `t_symbol` is a leaked allocation, so a
//! pointer to one stays valid and stays unique to its string across tests and threads. The table
//! has its own lock, independent of the rest of the kernel.

use max_sys::{t_common_symbols_table, t_symbol};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::sync::{LazyLock, Mutex, OnceLock};

static SYMBOLS: LazyLock<Mutex<HashMap<Vec<u8>, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Intern `name`. Embedded NULs cut the name short, as they would with a C string.
pub fn intern(name: &[u8]) -> *mut t_symbol {
    let name = name.split(|&b| b == 0).next().unwrap_or(&[]);
    let mut table = SYMBOLS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&p) = table.get(name) {
        return p as *mut t_symbol;
    }
    let cname = CString::new(name).expect("NULs were removed above");
    let sym = Box::into_raw(Box::new(t_symbol {
        s_name: cname.into_raw(),
        s_thing: std::ptr::null_mut(),
    }));
    table.insert(name.to_vec(), sym as usize);
    sym
}

/// The string of a symbol, lossily converted; `""` for null.
///
/// # Safety
/// `s` must be null or a symbol from [`intern`] (that is, from `gensym`).
pub unsafe fn sym_name(s: *const t_symbol) -> String {
    if s.is_null() {
        return String::new();
    }
    // SAFETY: the caller guarantees `s` is one of our symbols, whose name is a leaked C string
    unsafe { CStr::from_ptr((*s).s_name) }
        .to_string_lossy()
        .into_owned()
}

/// Like [`sym_name`] but for a pointer taken from an untrusted place (a `usize` in the state).
pub(crate) fn name_of(s: usize) -> String {
    // SAFETY: every `usize` that is passed here was produced by `intern` or is 0
    unsafe { sym_name(s as *const t_symbol) }
}

/// Max's `gensym`: the one symbol for a string. Thread safe.
///
/// # Safety
/// `s` must be null (treated as `""`) or point to a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gensym(s: *const c_char) -> *mut t_symbol {
    if s.is_null() {
        return intern(b"");
    }
    // SAFETY: the caller guarantees a NUL-terminated string
    intern(unsafe { CStr::from_ptr(s) }.to_bytes())
}

/// Max's `gensym_tr`: `gensym` of the localized string. The mock has no translations, so it is
/// `gensym`.
///
/// # Safety
/// As `gensym`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gensym_tr(s: *const c_char) -> *mut t_symbol {
    // SAFETY: the caller's contract
    unsafe { gensym(s) }
}

/// Max's `str_tr`: the localized string. The mock has no translations, so it returns `s`.
///
/// # Safety
/// No requirements; `s` is returned as it is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn str_tr(s: *const c_char) -> *mut c_char {
    s as *mut c_char
}

include!(concat!(env!("OUT_DIR"), "/common_syms_gen.rs"));

static COMMON: OnceLock<usize> = OnceLock::new();

/// Max's `common_symbols_gettable`. The table is filled from the field names of
/// `_common_symbols_table` (field `s_foo` is the symbol `"foo"`, `s_nothing` is `""`), which is
/// what the SDK's `commonsyms.h` does for every symbol except a few that the SDK spells
/// differently; none of those are known to be used by median.
///
/// # Safety
/// No requirements; the returned table is read-only and lives for the process.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn common_symbols_gettable() -> *mut t_common_symbols_table {
    *COMMON.get_or_init(|| {
        const WORD: usize = size_of::<usize>();
        assert_eq!(
            size_of::<t_common_symbols_table>(),
            WORD * (1 + COMMON_SYMBOL_NAMES.len()),
            "_common_symbols_table is not `version` followed by only symbol pointers"
        );
        let layout = std::alloc::Layout::new::<t_common_symbols_table>();
        // SAFETY: the layout has non-zero size
        let table = unsafe { std::alloc::alloc_zeroed(layout) } as *mut usize;
        assert!(!table.is_null(), "out of memory");
        // SAFETY: the assertion above makes the table `1 + N` words long
        unsafe {
            table.write(500); // COMMON_SYMBOLS_VERSION
            for (i, name) in COMMON_SYMBOL_NAMES.iter().enumerate() {
                table.add(1 + i).write(intern(name.as_bytes()) as usize);
            }
        }
        table as usize
    }) as *mut t_common_symbols_table
}
