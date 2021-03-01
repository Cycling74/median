use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    FnArg, Ident, ImplItemMethod, Pat, Type,
};

type Res<T> = syn::parse::Result<T>;

pub fn wrapped_tramp(attr: TokenStream, item: TokenStream) -> TokenStream {
    let TrampArgs { wrapper } = parse_macro_input!(attr as TrampArgs);
    let meth: ImplItemMethod = parse_macro_input!(item as ImplItemMethod);
    crate::error::wrap(wrapped_tramp_with_type(wrapper, meth))
}

pub fn defer_tramp(attr: TokenStream, item: TokenStream) -> TokenStream {
    let TrampArgs { wrapper } = parse_macro_input!(attr as TrampArgs);
    let meth: ImplItemMethod = parse_macro_input!(item as ImplItemMethod);
    crate::error::wrap(wrapped_defer_tramp_with_type(wrapper, meth))
}

pub fn wrapped_attr_get_tramp(attr: TokenStream, item: TokenStream) -> TokenStream {
    let TrampArgs { wrapper } = parse_macro_input!(attr as TrampArgs);
    let meth: ImplItemMethod = parse_macro_input!(item as ImplItemMethod);
    crate::error::wrap(wrapped_attr_get_tramp_with_type(wrapper, meth))
}

pub fn wrapped_attr_set_tramp(attr: TokenStream, item: TokenStream) -> TokenStream {
    let TrampArgs { wrapper } = parse_macro_input!(attr as TrampArgs);
    let meth: ImplItemMethod = parse_macro_input!(item as ImplItemMethod);
    crate::error::wrap(wrapped_attr_set_tramp_with_type(wrapper, meth))
}

struct Names {
    meth_name: Ident,
    tramp_name: Ident,
}

fn get_names(meth: &ImplItemMethod) -> Names {
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

pub fn wrapped_tramp_with_type(t: Type, meth: ImplItemMethod) -> Res<TokenStream> {
    let Names {
        meth_name,
        tramp_name,
    } = get_names(&meth);
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

pub fn wrapped_defer_tramp_with_type(t: Type, meth: ImplItemMethod) -> Res<TokenStream> {
    let Names {
        meth_name,
        tramp_name,
    } = get_names(&meth);

    //TODO check signature
    //TODO allow for no sym and or no atoms
    let expanded = quote! {
        pub extern "C" fn #tramp_name(
            wrapper: &#t,
            sym: *mut max_sys::t_symbol,
            ac: ::std::os::raw::c_long,
            av: *const ::max_sys::t_atom,
        ) {
            let sym = ::median::symbol::SymbolRef::from(sym);
            let atoms = unsafe {
                std::slice::from_raw_parts(std::mem::transmute::<*const ::max_sys::t_atom, *const ::median::atom::Atom>(av), ac as _)
            };
            wrapper.wrapped().#meth_name(&sym, &atoms);
        }
        #meth
    };
    Ok(expanded.into())
}

pub fn wrapped_attr_get_tramp_with_type(t: Type, meth: ImplItemMethod) -> Res<TokenStream> {
    let Names {
        meth_name,
        tramp_name,
    } = get_names(&meth);
    //TODO check signature to make sure it has no inputs and returns a type we support
    let expanded = quote! {
        pub extern "C" fn #tramp_name(
            wrapper: &#t,
            _attr: ::std::ffi::c_void,
            ac: *mut ::std::os::raw::c_long,
            av: *mut *mut ::max_sys::t_atom,
        ) {
            ::median::attr::get(ac, av, || wrapper.wrapped().#meth_name());
        }
        #meth
    };
    Ok(expanded.into())
}

pub fn wrapped_attr_set_tramp_with_type(t: Type, meth: ImplItemMethod) -> Res<TokenStream> {
    let Names {
        meth_name,
        tramp_name,
    } = get_names(&meth);

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
    //TODO check signature to make sure it has 1 input and no returns with the type we support
    let expanded = quote! {
        pub extern "C" fn #tramp_name(
            wrapper: &#t,
            _attr: ::std::ffi::c_void,
            ac: ::std::os::raw::c_long,
            av: *mut ::max_sys::t_atom,
        ) {
            ::median::attr::set(ac, av, |#(#args)*| wrapper.wrapped().#meth_name(#(#vars)*));
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
