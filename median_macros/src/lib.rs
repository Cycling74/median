//! Procedural macros for `median`, which re-exports them; depend on `median`, not on this
//! crate.

use proc_macro::TokenStream;
use syn::{DeriveInput, ItemImpl, parse_macro_input};

mod attr_opts;
mod cstr;
mod derive;
mod error;
mod methods;
mod tramp;

/// See `median::MaxObject`.
#[proc_macro_derive(MaxObject, attributes(max, attr))]
pub fn derive_max_object(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    crate::error::wrap(derive::derive(input).map(Into::into))
}

/// See `median::methods`.
#[proc_macro_attribute]
pub fn methods(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as ItemImpl);
    methods::methods(attr.into(), item).into()
}

#[proc_macro_attribute]
pub fn wrapped_defer_tramp(attr: TokenStream, item: TokenStream) -> TokenStream {
    tramp::defer_tramp(attr, item)
}
