//! Compiles the C file holding the variadic parts of the Max API and generates
//! * `dispatch_gen.rs`: `call_raw`, which calls a method pointer with every combination of
//!   integer-class and floating point arguments (Rust cannot call a function pointer with a
//!   run-time determined signature), and
//! * `common_syms_gen.rs`: the names of the fields of `_common_symbols_table`, in order, taken
//!   from the max-sys bindings for the target.
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// Calls with up to this many arguments (including the object pointer, if any).
const MAX_RAW_ARGS: usize = 6;

fn gen_dispatch(out: &Path) {
    let mut s = String::new();
    s.push_str("/// Calls `f` with the given arguments, each passed in the register class of its variant.\n");
    s.push_str("///\n/// # Safety\n/// `f` must be a function pointer whose signature has exactly the argument classes in `args`.\n");
    s.push_str("pub(crate) unsafe fn call_raw(f: usize, args: &[Raw]) -> *mut c_void {\n    match args {\n");
    for n in 0..=MAX_RAW_ARGS {
        for mask in 0u32..(1 << n) {
            let mut pat = Vec::new();
            let mut tys = Vec::new();
            let mut call = Vec::new();
            for i in 0..n {
                if (mask >> i) & 1 == 1 {
                    pat.push(format!("Raw::F(a{i})"));
                    tys.push("f64");
                } else {
                    pat.push(format!("Raw::I(a{i})"));
                    tys.push("usize");
                }
                call.push(format!("*a{i}"));
            }
            writeln!(
                s,
                "        [{}] => unsafe {{ std::mem::transmute::<usize, unsafe extern \"C\" fn({}) -> *mut c_void>(f)({}) }},",
                pat.join(", "),
                tys.join(", "),
                call.join(", ")
            )
            .unwrap();
        }
    }
    s.push_str(
        "        _ => panic!(\"median-mock: too many arguments for a method call\"),\n    }\n}\n",
    );
    fs::write(out.join("dispatch_gen.rs"), s).unwrap();
}

fn gen_common_syms(out: &Path) {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let ffi = manifest
        .join("../max-sys/src")
        .join(format!("ffi-{os}-{arch}.rs"));
    println!("cargo:rerun-if-changed={}", ffi.display());
    let src = fs::read_to_string(&ffi).unwrap_or_else(|e| panic!("reading {}: {e}", ffi.display()));
    let mut names = Vec::new();
    let mut inside = false;
    for line in src.lines() {
        let line = line.trim();
        if line.starts_with("pub struct _common_symbols_table") {
            inside = true;
            continue;
        }
        if inside {
            if line == "}" {
                break;
            }
            if let Some(rest) = line.strip_prefix("pub s_") {
                let field = rest.split(':').next().unwrap().trim();
                // `_sym_nothing` is the empty symbol
                names.push(if field == "nothing" {
                    String::new()
                } else {
                    field.to_string()
                });
            }
        }
    }
    assert!(
        !names.is_empty(),
        "no common symbols found in {}",
        ffi.display()
    );
    let mut s = String::from(
        "/// Names of the common symbols, in field order of `_common_symbols_table`.\npub(crate) const COMMON_SYMBOL_NAMES: &[&str] = &[\n",
    );
    for n in names {
        writeln!(s, "    {n:?},").unwrap();
    }
    s.push_str("];\n");
    fs::write(out.join("common_syms_gen.rs"), s).unwrap();
}

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    gen_dispatch(&out);
    gen_common_syms(&out);
    println!("cargo:rerun-if-changed=c/variadic.c");
    println!("cargo:rerun-if-changed=build.rs");
    cc::Build::new()
        .file("c/variadic.c")
        .warnings(true)
        .flag_if_supported("-Wall")
        .flag_if_supported("-Wextra")
        .flag_if_supported("-Werror")
        .compile("median_mock_variadic");
}
