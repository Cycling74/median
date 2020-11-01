use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    FnArg, Ident, ImplItemMethod, Pat, Type,
};

type Res<T> = syn::parse::Result<T>;

struct TrampArgs {
    pub wrapper: syn::Type,
}

impl Parse for TrampArgs {
    fn parse(input: ParseStream) -> Res<Self> {
        let wrapper: syn::Type = input.parse()?;
        Ok(Self { wrapper })
    }
}

pub fn wrapped_tramp(attr: TokenStream, item: TokenStream) -> TokenStream {
    let TrampArgs { wrapper } = parse_macro_input!(attr as TrampArgs);
    let meth: ImplItemMethod = parse_macro_input!(item as ImplItemMethod);
    crate::error::wrap(tramp_with_type(wrapper, meth))
}

pub fn tramp_with_type(t: Type, meth: ImplItemMethod) -> Res<TokenStream> {
    let meth_name = meth.sig.ident.clone();
    let tramp_name = Ident::new(
        &format!("{}_tramp", meth_name.to_string()),
        meth_name.span(),
    );
    //get args, skip self
    let args: Vec<&FnArg> = meth.sig.inputs.iter().skip(1).collect();
    let vars = args
        .iter()
        .map(|a| match a {
            FnArg::Receiver(r) => Err(syn::Error::new(
                r.span(),
                format!("unexpected type in signature"),
            )),
            FnArg::Typed(t) => Ok(t.clone().pat),
        })
        .collect::<Result<Vec<Box<Pat>>, _>>()?;
    let expanded = quote! {
        pub extern "C" fn #tramp_name(wrapper: &#t, #(#args)*) {
            wrapper.wrapped().#meth_name(#(#vars)*)
        }
        #meth
    };
    Ok(expanded.into())
}
