use std::env;
use std::path::PathBuf;

fn main() {
    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=wrapper-max.h");
    //println!("cargo:rerun-if-changed=wrapper-jitter.h");

    let support_dir = "thirdparty/max-sdk/source/c74support";
    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I./{}/max-includes/", support_dir))
        .clang_arg(format!("-I./{}/msp-includes/", support_dir))
        .clang_arg(format!("-I./{}/jit-includes/", support_dir))
        .rustfmt_bindings(true);

    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=CoreServices");
        builder = builder
        .clang_args(&[
            "-isysroot",
            "/Library/Developer/CommandLineTools/SDKs/MacOSX11.0.sdk/",
        ])
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
            .split(" ")
            .map(|l| {
                //lines are in the form: '-Wl,-U,_addbang'
                let mut e = l.split(',').last().unwrap().to_string();
                e.remove(0); //remove _
                e.pop(); //remove '
                e
            })
            .collect();

    builder = max.iter().fold(builder, |b, i| b.whitelist_function(i));

    //msp
    let msp = [
        "z_dsp.*",
        "dsp_.*",
        "buffer_.*",
        "sys_.*",
        "class_.*",
        "z_jbox.*",
        "z_isconnected",
        "canvas_.*",
    ];
    builder = msp.iter().fold(builder, |b, i| b.whitelist_function(i));

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
    ];

    builder = enums.iter().fold(builder, |b, i| {
        b.whitelist_type(i).constified_enum_module(i)
    });

    let bindings = builder.generate().expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
