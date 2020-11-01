use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{
    parenthesized, parse_macro_input, Attribute, FnArg, Ident, ImplItem, ImplItemMethod, Item,
    ItemImpl, ItemStruct, Lit, LitInt, LitStr, Pat, Token, Type, TypePath,
};

#[derive(Copy, Clone, Debug)]
enum ExternalType {
    Max,
    MSP,
}

struct Parsed {
    items: Vec<Item>,
}

impl Parse for Parsed {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        let mut items = Vec::new();
        while !input.is_empty() {
            items.push(input.parse()?);
        }

        Ok(Self { items })
    }
}

pub fn parse_and_build(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let Parsed { items } = parse_macro_input!(input as Parsed);
    crate::error::wrap(process(items))
}

struct StructDetails {
    the_struct: ItemStruct,
    class_name: Ident,
    class_alias: String,
}

//an attribute to specify the name of the class
struct ClassNameArgs {
    pub name: LitStr,
}

impl Parse for ClassNameArgs {
    fn parse(input: ParseStream) -> syn::parse::Result<Self> {
        let _: Token![=] = input.parse()?;
        let name: LitStr = input.parse()?;
        Ok(Self { name })
    }
}

fn process_struct(mut s: ItemStruct) -> syn::Result<StructDetails> {
    let class_name = s.ident.clone();
    let mut class_alias = class_name.to_string().to_lowercase();

    //find name attribute and remove it
    if let Some(pos) = s
        .attrs
        .iter()
        .position(|a| a.path.segments.last().unwrap().ident == "name")
    {
        let a = s.attrs.remove(pos);
        let n: ClassNameArgs = syn::parse2(a.tokens.clone())?;
        class_alias = n.name.value();
    }

    Ok(StructDetails {
        the_struct: s,
        class_name,
        class_alias,
    })
}

fn process(items: Vec<Item>) -> syn::Result<proc_macro::TokenStream> {
    let mut impls = Vec::new();
    let mut the_struct = None;
    let mut remain = Vec::new();

    for item in items.iter() {
        match item {
            Item::Struct(i) => {
                if the_struct.is_some() {
                    return Err(syn::Error::new(i.span(), "only one wrapped struct allowed"));
                }
                the_struct = Some(i);
            }
            Item::Impl(i) => impls.push(i),
            _ => remain.push(item),
        }
    }

    let StructDetails {
        the_struct,
        class_name,
        class_alias,
    } = process_struct(the_struct.unwrap().clone())?;

    let max_class_name = LitStr::new(&class_alias, the_struct.span());

    let expanded = quote! {
        #the_struct

        impl ::median::wrapper::ObjWrapped<#class_name> for #class_name {
            fn class_name() -> &'static str {
                &#max_class_name
            }
        }

        #(#impls)*

        #(#remain)*

        #[no_mangle]
        pub unsafe extern "C" fn ext_main(_r: *mut ::std::ffi::c_void) {
            MaxObjWrapper::<#class_name>::register(false)
        }
    };
    Ok(expanded.into())
}
