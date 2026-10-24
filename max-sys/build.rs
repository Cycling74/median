use std::env;

/// Bindgen strips C comment delimiters but leaves the SDK's Doxygen code markers and the
/// whitespace after an opening `/**`. Rustdoc otherwise treats C arrays as links and a
/// leading tab in prose as an indented Rust example.
#[cfg(feature = "build-bindings")]
#[derive(Debug)]
struct SdkComments;

#[cfg(feature = "build-bindings")]
impl bindgen::callbacks::ParseCallbacks for SdkComments {
    fn process_comment(&self, comment: &str) -> Option<String> {
        let mut code = false;
        Some(
            comment
                .trim_start()
                .lines()
                .map(|line| match line.trim() {
                    "@code" => {
                        code = true;
                        "\n```c".to_owned()
                    }
                    "@endcode" => {
                        code = false;
                        "```\n".to_owned()
                    }
                    // SDK prose also mentions C expressions such as argv[0]. They are
                    // literal subscripts/ranges, not Rust intra-doc links.
                    _ if !code => line.replace('[', "\\[").replace(']', "\\]"),
                    _ => line.to_owned(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

#[cfg(feature = "build-bindings")]
fn build_bindings(support_dir: &str) {
    let target =
        env::var("MAX_SYS_BINDINGS_TARGET").unwrap_or_else(|_| env::var("TARGET").unwrap());
    assert!(
        matches!(
            target.as_str(),
            "aarch64-apple-darwin"
                | "x86_64-apple-darwin"
                | "x86_64-pc-windows-gnu"
                | "x86_64-pc-windows-msvc"
        ),
        "unsupported bindings target: {target}"
    );
    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(SdkComments))
        .clang_arg(format!("--target={target}"))
        .clang_arg(format!("-I{support_dir}/max-includes"))
        .clang_arg(format!("-I{support_dir}/msp-includes"))
        .clang_arg(format!("-I{support_dir}/jit-includes"))
        .rust_target("1.85".parse().expect("valid Rust target"))
        .generate_cstr(true)
        .wrap_unsafe_ops(true);

    if target.ends_with("apple-darwin") {
        let sdk = std::process::Command::new("xcrun")
            .arg("--show-sdk-path")
            .output()
            .expect("xcrun is required to find the macOS SDK");
        assert!(sdk.status.success(), "xcrun --show-sdk-path failed");
        let sdk = String::from_utf8(sdk.stdout).expect("UTF-8 SDK path");
        builder = builder
            .clang_arg("-DMAC_VERSION")
            .clang_arg("-isysroot")
            .clang_arg(sdk.trim());
    } else if target.contains("windows") {
        builder = builder
            .clang_arg("-DWIN_VERSION")
            .clang_arg("-DWIN32_LEAN_AND_MEAN")
            .clang_arg("-fms-extensions");
        // When cross-generating on macOS, point at a MinGW-w64 include directory.
        // On Windows, libclang discovers the installed Windows SDK itself.
        if let Ok(include) = env::var("MAX_SYS_WINDOWS_INCLUDE") {
            builder = builder.clang_arg(format!("-isystem{include}"));
        }
    } else {
        panic!("unsupported bindings target: {}", target);
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
            .filter(|s| !s.is_empty())
            .collect();

    builder = max.iter().fold(builder, |b, i| b.allowlist_function(i));

    let msp_jitter = [
        //msp
        "z_dsp.*",
        "dsp_.*",
        "dspmess",
        "dspchain.*",
        "atom_.*",
        "canvas_.*",
        "aiff_parse",
        "t_resizebytes",
        "t_getbytes",
        "t_freebytes",
        "buffer_.*",
        "sys_.*",
        "class_.*",
        "z_jbox.*",
        "z_isconnected",
        "z_sysinit",
        "canvas_.*",
        "set_zero",
        "set_zero64",
        "copy_64from32",
        "copy_32from64",
        "plus_perform",
        "plus_perform64",
        "sig_perform",
        "sig_perform64",
        "copy_perform",
        "copy_perform64",
        "plus2_perform",
        "plus2_perform64",
        //jitter
        "jit_.*",
        "max_jit.*",
        "max_addmethod.*",
        "swapf32",
        "swapf64",
    ];
    builder = msp_jitter
        .iter()
        .fold(builder, |b, i| b.allowlist_function(i));

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
        "t_jit_attr_flags",
        "t_jit_matrix_info_flags",
        "t_matrix_conv_info_flags",
        "t_jit_error_code",
        "t_jit_gl_.*",
        "t_jit_ob3d_flags",
        "t_jit_anim_ui_priority",
        "t_jit_window_.*",
        "t_max_jit_mop_flag",
        "e_buffer_data_flag",
    ];

    builder = enums.iter().fold(builder, |b, i| {
        b.allowlist_type(i).constified_enum_module(i)
    });

    builder = builder.allowlist_var("_jit_sym_.*");

    let bindings = builder.generate().expect("Unable to generate bindings");

    let platform = if target.contains("apple-darwin") {
        "macos"
    } else {
        "windows"
    };
    let arch = target.split('-').next().expect("target architecture");
    // The regeneration script stages every target before replacing the tracked set.
    let out_dir = env::var("MAX_SYS_BINDINGS_DIR").unwrap_or_else(|_| env::var("OUT_DIR").unwrap());
    let out_path = std::path::PathBuf::from(out_dir).join(format!("ffi-{platform}-{arch}.rs"));
    bindings
        .write_to_file(out_path)
        .expect("Couldn't write bindings!");
}

fn main() {
    let support_dir = "thirdparty/max-sdk-base/c74support";
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
        println!(
            "cargo:rustc-link-search={}/{}/jit-includes/x64/",
            manifest_dir, support_dir
        );
        println!("cargo:rustc-link-lib=jitlib");
    } else {
        panic!("{:?} is not a supported target os", target_os);
    }

    #[cfg(feature = "build-bindings")]
    {
        // Tell cargo to invalidate the built crate whenever the wrapper changes
        println!("cargo:rerun-if-changed=wrapper.h");
        println!("cargo:rerun-if-changed=wrapper-max.h");
        println!("cargo:rerun-if-changed=wrapper-jitter.h");
        println!("cargo:rerun-if-changed=thirdparty/max-sdk-base/c74support");
        for key in [
            "MAX_SYS_BINDINGS_TARGET",
            "MAX_SYS_BINDINGS_DIR",
            "MAX_SYS_WINDOWS_INCLUDE",
            "LIBCLANG_PATH",
        ] {
            println!("cargo:rerun-if-env-changed={key}");
        }
        build_bindings(support_dir);
    }
}
