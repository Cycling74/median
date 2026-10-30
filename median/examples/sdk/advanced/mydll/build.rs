// The externals record this as where to load the library from: the package's `support` folder,
// from `<package>/externals/<name>.mxo/Contents/MacOS/<name>`.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!(
            "cargo:rustc-cdylib-link-arg=-Wl,-install_name,@loader_path/../../../../support/libmydll.dylib"
        );
    }
}
