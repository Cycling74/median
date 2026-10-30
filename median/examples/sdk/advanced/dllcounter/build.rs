use std::{env, path::PathBuf};

// Links libmydll from the profile's `deps` folder (`OUT_DIR` is `<profile>/build/<crate>/out`),
// where cargo puts it whether it is built as this crate's dependency or as a workspace member.
// A search path inside the target folder is also on the library path of `cargo test`.
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let profile = out.ancestors().nth(3).unwrap();
    println!(
        "cargo:rustc-link-search=native={}",
        profile.join("deps").display()
    );
    println!("cargo:rustc-link-lib=dylib=mydll");
}
