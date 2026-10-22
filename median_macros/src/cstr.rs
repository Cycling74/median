//! Names that become C strings: checked here, emitted as a call to `median::__private::cstr` with a
//! NUL-terminated byte string, which works in user crates of any edition (a `c".."` literal
//! needs 2021).

use proc_macro2::{Span, TokenStream};
use quote::quote_spanned;
use syn::{LitByteStr, LitStr};

/// An error unless `name` can be a C string naming something in Max: not empty, no NUL.
pub fn check(name: &LitStr) -> syn::Result<()> {
    let v = name.value();
    if v.is_empty() {
        Err(syn::Error::new(name.span(), "the name must not be empty"))
    } else if v.contains('\0') {
        Err(syn::Error::new(
            name.span(),
            "the name must not contain a NUL",
        ))
    } else {
        Ok(())
    }
}

/// An expression for the `&'static CStr` of `value` (checked with `check`).
pub fn expr(value: &str, span: Span) -> TokenStream {
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(0);
    let lit = LitByteStr::new(&bytes, span);
    quote_spanned!(span=> ::median::__private::cstr(#lit))
}
