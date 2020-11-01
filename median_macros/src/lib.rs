use proc_macro::TokenStream;
mod error;
mod external;
mod tramp;

#[proc_macro]
pub fn external(input: TokenStream) -> TokenStream {
    external::parse_and_build(input)
}

#[proc_macro_attribute]
pub fn wrapped_tramp(attr: TokenStream, item: TokenStream) -> TokenStream {
    tramp::wrapped_tramp(attr, item)
}
