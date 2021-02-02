#![allow(dead_code)]
/*

    println!("cargo:rustc-link-lib=dylib=c++");
    println!("cargo:rustc-link-lib=static=Juce");
    println!("cargo:rustc-link-lib=static=MaxInternals");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=AudioUnit");
    println!("cargo:rustc-link-lib=framework=Accelerate");
    println!("cargo:rustc-link-lib=framework=WebKit");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=AVKit");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=CoreAudioKit");
    println!("cargo:rustc-link-lib=framework=AudioToolBox");
    println!("cargo:rustc-link-lib=framework=Cocoa");
    println!("cargo:rustc-link-lib=framework=Carbon");
    println!("cargo:rustc-link-lib=framework=CoreData");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rustc-link-lib=framework=CoreServices");
    println!("cargo:rustc-link-lib=framework=QuartzCore");
*/

/*
#[link(name = "c++", kind = "dylib")]
#[link(name = "iconv", kind = "dylib")]
#[link(name = "MaxCoreStubs", kind = "static")]
#[link(name = "MaxInternals", kind = "static")]
#[link(name = "C74ApiPrivate", kind = "static")]
#[link(name = "Juce", kind = "static")]
#[link(name = "CoreData", kind = "framework")]
#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CoreServices", kind = "framework")]
#[link(name = "QuartzCore", kind = "framework")]
#[link(name = "Carbon", kind = "framework")]
#[link(name = "Cocoa", kind = "framework")]
#[link(name = "AVFoundation", kind = "framework")]
#[link(name = "AppKit", kind = "framework")]
#[link(name = "AVKit", kind = "framework")]
#[link(name = "IOKit", kind = "framework")]
#[link(name = "WebKit", kind = "framework")]
#[link(name = "AudioToolBox", kind = "framework")]
#[link(name = "AudioUnit", kind = "framework")]
*/

#[link(name = "MaxCore", kind = "static")]
#[link(name = "MaxCoreStubs", kind = "static")]
#[link(name = "C74TestSupport", kind = "static")]
#[link(name = "c74_rand", kind = "static")]
#[link(name = "Juce", kind = "static")]
#[link(name = "iconv", kind = "static")]
#[link(name = "boost_filesystem", kind = "static")]
#[link(name = "CoreServices", kind = "framework")]
#[link(name = "Accelerate", kind = "framework")]
#[link(name = "AudioUnit", kind = "framework")]
#[link(name = "AudioToolbox", kind = "framework")]
#[link(name = "AVFoundation", kind = "framework")]
#[link(name = "AVKit", kind = "framework")]
#[link(name = "Carbon", kind = "framework")]
#[link(name = "Cocoa", kind = "framework")]
#[link(name = "CoreAudio", kind = "framework")]
#[link(name = "CoreAudioKit", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CoreMedia", kind = "framework")]
#[link(name = "CoreMIDI", kind = "framework")]
#[link(name = "QuartzCore", kind = "framework")]
#[link(name = "IOKit", kind = "framework")]
#[link(name = "OpenGL", kind = "framework")]
#[link(name = "WebKit", kind = "framework")]
#[link(name = "Security", kind = "framework")]
#[link(name = "AppKit", kind = "framework")]
#[link(name = "CFNetwork", kind = "framework")]
#[link(name = "Foundation", kind = "framework")]
#[link(name = "System", kind = "dylib")]
#[link(name = "resolv", kind = "dylib")]
#[link(name = "c++", kind = "dylib")]
#[link(name = "objc", kind = "dylib")]
extern "C" {
    fn max_core_init();
    fn max_core_deinit();
}

#[no_mangle]
unsafe extern "C" fn byteorder_swap_pointer_64(p: *mut ::std::os::raw::c_char) {
    let mut c: ::std::os::raw::c_char;
    let p = std::slice::from_raw_parts_mut(p, 8);
    c = p[7];
    p[7] = p[0];
    p[0] = c;
    c = p[6];
    p[6] = p[1];
    p[1] = c;
    c = p[5];
    p[5] = p[2];
    p[2] = c;
    c = p[4];
    p[4] = p[3];
    p[3] = c;
}
#[no_mangle]
unsafe extern "C" fn byteorder_swap_pointer_32(p: *mut ::std::os::raw::c_char) {
    let mut c: ::std::os::raw::c_char;
    let p = std::slice::from_raw_parts_mut(p, 4);
    c = p[3];
    p[3] = p[0];
    p[0] = c;
    c = p[2];
    p[2] = p[1];
    p[1] = c;
}

pub fn with_setup<F: FnOnce() + std::panic::UnwindSafe>(func: F) {
    unsafe {
        max_core_init();
    }
    let result = std::panic::catch_unwind(move || {
        func();
    });
    unsafe {
        max_core_deinit();
    }
    assert!(result.is_ok());
}

pub fn setup() {
    unsafe {
        max_core_init();
    }
}
