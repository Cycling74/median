use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    Ident, ImplItemFn, Type,
};

type Res<T> = syn::parse::Result<T>;

pub fn defer_tramp(attr: TokenStream, item: TokenStream) -> TokenStream {
    let TrampArgs { wrapper } = parse_macro_input!(attr as TrampArgs);
    let meth: ImplItemFn = parse_macro_input!(item as ImplItemFn);
    crate::error::wrap(wrapped_defer_tramp_with_type(wrapper, meth))
}

struct Names {
    meth_name: Ident,
    tramp_name: Ident,
}

fn get_names(meth: &ImplItemFn) -> Names {
    let meth_name = meth.sig.ident.clone();
    let tramp_name = Ident::new(
        &format!("{}_tramp", meth_name.to_string()),
        meth_name.span(),
    );
    Names {
        meth_name,
        tramp_name,
    }
}

pub fn wrapped_defer_tramp_with_type(t: Type, meth: ImplItemFn) -> Res<TokenStream> {
    let Names {
        meth_name,
        tramp_name,
    } = get_names(&meth);

    //TODO check signature
    //TODO allow for no sym and or no atoms
    let expanded = quote! {
        /// Trampoline called by Max.
        ///
        /// # Safety
        /// Must only be called by Max (or through a pointer registered with Max): the object and
        /// every pointer argument must be valid as the Max SDK guarantees for this kind of method.
        pub unsafe extern "C" fn #tramp_name(
            wrapper: &#t,
            sel: *mut ::median::max_sys::t_symbol,
            ac: ::std::os::raw::c_short,
            av: *const ::median::max_sys::t_atom,
        ) {
            ::median::ffi::guard(wrapper, stringify!(#meth_name), || unsafe {
                median::method::sel_list(sel, ac as ::std::os::raw::c_long, av, |sym, atoms| {
                    ::median::wrapper::WrapperWrapped::wrapped(wrapper).#meth_name(&sym, atoms);
                });
            })
        }
        #meth
    };
    Ok(expanded.into())
}

struct TrampArgs {
    pub wrapper: syn::Type,
}

impl Parse for TrampArgs {
    fn parse(input: ParseStream) -> Res<Self> {
        let wrapper: syn::Type = input.parse()?;
        Ok(Self { wrapper })
    }
}
