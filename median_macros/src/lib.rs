use proc_macro::TokenStream;
mod error;
mod tramp;

#[proc_macro_attribute]
pub fn wrapped_defer_tramp(attr: TokenStream, item: TokenStream) -> TokenStream {
    tramp::defer_tramp(attr, item)
}
