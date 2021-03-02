use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    Ident, Item, ItemImpl, ItemStruct, LitStr, Token,
};

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

struct ImplDetails {
    wrapper_type: Ident,
    processed_impls: Vec<ItemImpl>,
}

fn process_impls(
    the_struct: &ItemStruct,
    _class_name: &Ident,
    impls: Vec<ItemImpl>,
) -> syn::Result<ImplDetails> {
    let mut processed_impls = Vec::new();
    let mut the_impl = None;
    let mut wrapper_type = None;

    for i in impls {
        if let Some((_, path, _)) = &i.trait_ {
            let t = path.segments.last().unwrap().ident.clone();
            if t == "MaxObjWrapped" {
                wrapper_type = Some(Ident::new(&"MaxObjWrapper", the_struct.span()));
                the_impl = Some(i);
                continue;
            } else if t == "MSPObjWrapped" {
                wrapper_type = Some(Ident::new(&"MSPObjWrapper", the_struct.span()));
                the_impl = Some(i);
                continue;
            }
        } else {
            //XXX extract methods etc.
        }
        processed_impls.push(i);
    }

    let wrapper_type = wrapper_type.ok_or(syn::Error::new(
        the_struct.span(),
        "Failed to find MaxObjWrapper or MSPObjWrapper",
    ))?;

    let the_impl = the_impl.unwrap();

    //TODO actually process

    processed_impls.push(the_impl);

    Ok(ImplDetails {
        wrapper_type,
        processed_impls,
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
            Item::Impl(i) => impls.push(i.clone()),
            _ => remain.push(item),
        }
    }

    //process the struct, getting the names
    let StructDetails {
        the_struct,
        class_name,
        class_alias,
    } = process_struct(the_struct.unwrap().clone())?;
    let max_class_name = LitStr::new(&class_alias, the_struct.span());

    //process the impls, getting the wrapper type
    let ImplDetails {
        wrapper_type,
        processed_impls: impls,
    } = process_impls(&the_struct, &class_name, impls)?;

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
            if std::panic::catch_unwind(|| {
                ::median::wrapper::#wrapper_type::<#class_name>::register(false)
            }).is_err() {
                std::process::exit(1);
            }
        }
    };
    Ok(expanded.into())
}
