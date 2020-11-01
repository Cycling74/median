pub fn wrap(input: syn::Result<proc_macro::TokenStream>) -> proc_macro::TokenStream {
    match input {
        Ok(ts) => ts,
        Err(err) => err.to_compile_error().into(),
    }
}
