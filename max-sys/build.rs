use std::env;

#[cfg(feature = "build-bindings")]
fn build_bindings(support_dir: &str) {
    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I./{}/max-includes/", support_dir))
        .clang_arg(format!("-I./{}/msp-includes/", support_dir))
        .clang_arg(format!("-I./{}/jit-includes/", support_dir))
        .rustfmt_bindings(true);

    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=CoreServices");
        println!("cargo:rustc-link-lib=framework=Carbon");
        builder = builder
        .clang_args(&[
            "-isysroot",
            "/Library/Developer/CommandLineTools/SDKs/MacOSX11.0.sdk/",
        ])
        .clang_arg("-DMAC_VERSION")
        .clang_arg(
            "-F/Library/Developer/CommandLineTools/SDKs/MacOSX11.0.sdk/System/Library/Frameworks/",
        );
    } else if cfg!(target_os = "windows") {
        builder = builder
            .clang_arg("-DWIN_VERSION")
            .clang_arg("-DWIN32_LEAN_AND_MEAN");

        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        println!(
            "cargo:rustc-link-search={}/{}/max-includes/x64/",
            manifest_dir, support_dir,
        );
        println!(
            "cargo:rustc-link-search={}/{}/msp-includes/x64/",
            manifest_dir, support_dir,
        );
        println!("cargo:rustc-link-lib=static=MaxAPI");
        println!("cargo:rustc-link-lib=static=MaxAudio");
    }

    //windows is really spammy so, we just parse the link flags to figure out what we want to include
    //and we also add some msp and enums below
    let max: Vec<String> =
        std::fs::read_to_string(format!("{}/max-includes/c74_linker_flags.txt", support_dir))
            .expect("Something went wrong reading the file")
            .split(&"-Wl,-U,")
            .map(|l| {
                if let Some(e) = l.trim().strip_prefix('_') {
                    e.strip_suffix('\'').unwrap_or(e).to_string()
                } else {
                    "".to_string()
                }
            })
            .filter(|s| s.len() > 0)
            .collect();

    builder = max.iter().fold(builder, |b, i| b.whitelist_function(i));

    //msp, jitter
    let msp_jitter = [
        "z_dsp.*",
        "dsp_.*",
        "buffer_.*",
        "sys_.*",
        "class_.*",
        "z_jbox.*",
        "z_isconnected",
        "canvas_.*",
        "jit_.*",
    ];
    builder = msp_jitter
        .iter()
        .fold(builder, |b, i| b.whitelist_function(i));

    let enums = [
        "e_max_attrflags",
        "e_max_atomtypes",
        "e_max_datastore_flags",
        "e_max_errorcodes",
        "e_max_class_flags",
        "e_max_dateflags",
        "e_max_expr_types",
        "e_max_fileinfo_flags",
        "e_max_openfile_permissions",
        "e_max_searchpath_flags",
        "e_max_systhread_.*",
        "e_max_typelists",
        "e_max_wind_advise_result",
        "e_max_atom_gettext_flags",
        "e_max_path_.*",
        "t_sysfile_.*",
        "PARAM_.*",
        "PARAMETER_ENABLE_SAVESTATE",
        "e_jit_state",
        "e_view_tag",
        "_modifiers",
        "_jdesktopui_flags",
        "_jgraphics_.*",
        "_jmouse_cursortype",
    ];

    builder = enums.iter().fold(builder, |b, i| {
        b.whitelist_type(i).constified_enum_module(i)
    });

    let bindings = builder.generate().expect("Unable to generate bindings");

    //let out_path = std::path::PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs");
    let out_path = std::path::PathBuf::from("src").join(format!(
        "ffi-{}-{}.rs",
        env::var("CARGO_CFG_TARGET_OS").expect("to get target os"),
        env::var("CARGO_CFG_TARGET_ARCH").expect("to get target architecture"),
    ));
    bindings
        .write_to_file(out_path)
        .expect("Couldn't write bindings!");
}

fn main() {
    let support_dir = "thirdparty/max-sdk/source/c74support";
    let target_os = env::var_os("CARGO_CFG_TARGET_OS").expect("failed to get target os");

    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=CoreServices");
        println!("cargo:rustc-link-lib=framework=Carbon");
    } else if target_os == "windows" {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        println!(
            "cargo:rustc-link-search={}/{}/max-includes/x64/",
            manifest_dir, support_dir,
        );
        println!(
            "cargo:rustc-link-search={}/{}/msp-includes/x64/",
            manifest_dir, support_dir,
        );
        println!("cargo:rustc-link-lib=MaxAPI");
        println!("cargo:rustc-link-lib=MaxAudio");
    } else {
        panic!("{:?} is not a supported target os", target_os);
    }

    #[cfg(feature = "build-bindings")]
    {
        // Tell cargo to invalidate the built crate whenever the wrapper changes
        println!("cargo:rerun-if-changed=wrapper.h");
        println!("cargo:rerun-if-changed=wrapper-max.h");
        println!("cargo:rerun-if-changed=wrapper-jitter.h");
        build_bindings(&support_dir);
    }
}
