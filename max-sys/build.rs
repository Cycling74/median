use std::env;
use std::path::PathBuf;

fn main() {
    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=wrapper-max.h");
    println!("cargo:rerun-if-changed=wrapper-jitter.h");

    let support_dir = "./thirdparty/max-sdk/source/c74support";
    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}/max-includes/", support_dir))
        .clang_arg(format!("-I{}/msp-includes/", support_dir))
        .clang_arg(format!("-I{}/jit-includes/", support_dir))
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
        .clang_arg(
            "-DWIN_VERSION",
        );
    }

    //include functions, types, etc.. disabled for now
    /*
    builder = [
        "atom_.*",
        "atomarray_.*",
        "attr_.*",
        "class_.*",
        "critical_.*",
        "db_.*",
        "defer.*",
        "dictionary_.*",
        "dictobj_.*",
        "disposehandle",
        "dsp_.*",
        "freeobject",
        "fileusage.*",
        "filewatcher_new",
        "freebytes.*",
        "gensym",
        "getbytes.*",
        "growhandle",
        "hashtab_.*",
        "inlet_.*",
        "indexmap_.*",
        "jit_.*",
        "jmonitor_.*",
        "linklist_.*",
        "locatefile.*",
        "newhandle",
        "object_.*",
        "open_dialog",
        "open_promptset",
        "outlet_.*",
        "path_.*",
        "preset_.*",
        "saveas_.*",
        "string_.*",
        "symobject_.*",
        "sys_.*",
        "sysfile_.*",
        "sysmem_.*",
        "sysmem_.*",
        "systhread_.*",
        "table_.*",
        "quickmap_.*",
        //scheduleing
        "clock_.*",
        "setclock_.*",
        "gettime.*",
        "qelem_.*",
        "sched_.*",
        "schedule.*",
        "systime.*",
        "sysdate.*",
        "itm_.*",
        "time_.*",
        //typed io
        "bangout",
        "floatin",
        "floatout",
        "intin",
        "intout",
        "listout",
        "proxy_.*",
        //printing
        "cpost",
        "post",
        "error",
        "ouchstring",
        "postatom",
        //loading max files
        "readtohandle",
        "fileload",
        "intload",
        "stringload",
        //patcher
        "jbox.*",
        "jpatchline.*",
        "jpatcher.*",
        "jpatcherview.*",
        //attributes
        "attribute_new.*",
        "object_addattr",
        "object_attr.*",
        "object_chuckattr",
        "object_deleteattr",
        "object_new_parse",
        //buffers
        "buffer_.*",
    ]
    .iter()
    .fold(builder, |b, i| b.whitelist_function(i));

    builder = [
        "t_symbol",
        "t_itm",
        "t_clock",
        "t_parameter_notify_data",
        "t_param_class_defcolor_data",
    ]
    .iter()
    .fold(builder, |b, i| b.whitelist_type(i));
    */

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

    builder = enums
        .iter()
        .fold(builder, |b, i| b.constified_enum_module(i));

    /*
    builder = enums.iter().fold(builder, |b, i| {
        b.whitelist_type(i).constified_enum_module(i)
    });
    */

    //remove types and functions that we don't want in the exposed lib
    builder = [
        "clock_sleep_trap",
        "clock_getres",
        "clock_gettime.*",
        "clock_settime",
        //unsure about the below
        "path_.*fsref",
        "jit_mac_gestalt",
        //windows
        "AddVectoredContinueHandler",
        "AddVectoredExceptionHandler",
        "CopyContext",
        "GetThreadContext",
        "GetXStateFeaturesMask",
        "InitializeContext",
        "InitializeContext2",
        "InitializeSListHead",
        "InterlockedFlushSList",
        "InterlockedPopEntrySList",
        "InterlockedPushEntrySList",
        "InterlockedPushListSListEx",
        "LocateXStateFeature",
        "QueryDepthSList",
        "RaiseFailFastException",
        "RtlCaptureContext",
        "RtlCaptureContext2",
        "RtlFirstEntrySList",
        "RtlInitializeSListHead",
        "RtlInterlockedFlushSList",
        "RtlInterlockedPopEntrySList",
        "RtlInterlockedPushEntrySList",
        "RtlInterlockedPushListSListEx",
        "RtlQueryDepthSList",
        "RtlRestoreContext",
        "RtlUnwindEx",
        "RtlVirtualUnwind",
        "SetThreadContext",
        "SetUnhandledExceptionFilter",
        "SetXStateFeaturesMask",
        "UnhandledExceptionFilter",
        "__C_specific_handler",
    ]
    .iter()
    .fold(builder, |b, i| b.blacklist_function(i));

    builder = [
        "clock_t",
        "pthread.*",
        "FSRef",
        "mach_.*",
        "kern_return.*",
        "clockid_t",
        "clock_res_t",
        "timespec",
        "sleep_type_t",
        "natural_t",

        //windows
        "LPMONITORINFOEXA?W?",
        "LPTOP_LEVEL_EXCEPTION_FILTER",
        "MONITORINFOEXA?W?",
        "PEXCEPTION_FILTER",
        "PEXCEPTION_ROUTINE",
        "PSLIST_HEADER",
        "PTOP_LEVEL_EXCEPTION_FILTER",
        "PVECTORED_EXCEPTION_HANDLER",
        "_?L?P?CONTEXT",
        "_?L?P?EXCEPTION_POINTERS",
        "_?P?DISPATCHER_CONTEXT",
        "_?P?EXCEPTION_REGISTRATION_RECORD",
        "_?P?IMAGE_TLS_DIRECTORY.*",
        "_?P?NT_TIB",
        "tagMONITORINFOEXA",
        "tagMONITORINFOEXW",
    ]
    .iter()
    .fold(builder, |b, i| b.blacklist_type(i));

    let bindings = builder.generate().expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
