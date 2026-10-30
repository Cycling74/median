//! Procedural macros for `median`, which re-exports them; depend on `median`, not on this
//! crate.

use crate::derive::Kind;
use proc_macro::TokenStream;
use syn::{DeriveInput, ItemImpl, parse_macro_input};

mod attr_opts;
mod cstr;
mod derive;
mod error;
mod methods;
mod reserved;

#[cfg(test)]
mod tests;

/// See `median::MaxObj`.
#[proc_macro_derive(MaxObj, attributes(max, attr))]
pub fn derive_max_obj(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    crate::error::wrap(derive::derive(input, Kind::Max).map(Into::into))
}

/// See `median::MSPObj`.
#[proc_macro_derive(MSPObj, attributes(max, attr))]
pub fn derive_msp_obj(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    crate::error::wrap(derive::derive(input, Kind::Msp).map(Into::into))
}

/// See `median::JitMatrixObj`.
#[proc_macro_derive(JitMatrixObj, attributes(max, attr))]
pub fn derive_jit_matrix_obj(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    crate::error::wrap(derive::derive(input, Kind::Matrix).map(Into::into))
}

/// See `median::JitGlObj`.
#[proc_macro_derive(JitGlObj, attributes(max, attr))]
pub fn derive_jit_gl_obj(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    crate::error::wrap(derive::derive(input, Kind::Gl).map(Into::into))
}

/// See `median::methods`.
#[proc_macro_attribute]
pub fn methods(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as ItemImpl);
    methods::methods(attr.into(), item).into()
}
