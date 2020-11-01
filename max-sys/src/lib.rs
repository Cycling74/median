#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

//! # max-sys
//!
//! `max-sys` is a Rust FFI binding to the [Cycling 74](https://cycling74.com/) [Max SDK](https://github.com/Cycling74/max-sdk).
//! It is automatically generated from the SDK source with [bindgen](https://github.com/rust-lang/rust-bindgen).
//!
//! The [Max API Docs](https://cycling74.com/sdk/max-sdk-8.0.3/html/index.html) will be useful for
//! understanding this library.

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

//pointer to a t_pxobject can be savely turned into a t_object
impl std::convert::From<&mut crate::t_pxobject> for &mut crate::object {
    fn from(o: &mut crate::t_pxobject) -> Self {
        unsafe { std::mem::transmute::<_, _>(o) }
    }
}
