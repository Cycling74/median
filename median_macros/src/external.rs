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

pub fn parse_and_build(input: proc_macro::TokenStream, with_main: bool) -> proc_macro::TokenStream {
    let Parsed { items } = parse_macro_input!(input as Parsed);
    crate::error::wrap(match process(items) {
        Ok((ts, class_name)) => {
            if with_main {
                match ext_main_classes(&[class_name]) {
                    Ok(m) => Ok(quote! {
                        #ts
                        #m
                    }
                    .into()),
                    Err(e) => Err(e),
                }
            } else {
                Ok(ts.into())
            }
        }
        Err(e) => Err(e),
    })
}

pub fn ext_main(tokens: proc_macro2::TokenStream) -> syn::Result<proc_macro2::TokenStream> {
    Ok(quote! {
        #[no_mangle]
        pub unsafe extern "C" fn ext_main(_r: *mut ::std::ffi::c_void) {
            if std::panic::catch_unwind(|| {
                #tokens
            }).is_err() {
                std::process::exit(1);
            }
        }
    }
    .into())
}

pub fn ext_main_classes(class_names: &[Ident]) -> syn::Result<proc_macro2::TokenStream> {
    let register: Vec<_> = class_names
        .iter()
        .map(|n| quote! { #n::register() })
        .collect();
    Ok(quote! {
        #[no_mangle]
        pub unsafe extern "C" fn ext_main(_r: *mut ::std::ffi::c_void) {
            if std::panic::catch_unwind(|| {
                #(#register)*
            }).is_err() {
                std::process::exit(1);
            }
        }
    }
    .into())
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
    if let Some(pos) = s.attrs.iter().position(|a| {
        a.path
            .segments
            .last()
            .expect("the attribute path to have at least 1 segment")
            .ident
            == "name"
    }) {
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
    class_name: &Ident,
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
        }
        processed_impls.push(i);
    }

    let wrapper_type = wrapper_type.ok_or(syn::Error::new(
        the_struct.span(),
        "Failed to find MaxObjWrapper or MSPObjWrapper",
    ))?;

    //find class_setup, if it exists
    let mut the_impl = the_impl.unwrap();

    let mut class_setup = None;
    if let Some(pos) = the_impl.items.iter().position(|item| {
        if let syn::ImplItem::Method(m) = item {
            if m.sig.ident == "class_setup" {
                class_setup = Some(m.clone());
                true
            } else {
                false
            }
        } else {
            false
        }
    }) {
        let _ = the_impl.items.remove(pos);
    };

    let mut class_setup: syn::ImplItemMethod = class_setup.unwrap_or_else(|| {
        syn::parse(
            quote! {
                fn class_setup(c: &mut ::median::class::Class<::median::wrapper::#wrapper_type<Self>>) {
                }
            }
            .into(),
        )
        .expect("to parse as method")
    });

    //get the var "c" from class setup
    let class_setup_class_var = match class_setup.sig.inputs.first().unwrap() {
        syn::FnArg::Receiver(_) => panic!("failed"),
        syn::FnArg::Typed(t) => {
            if let syn::Pat::Ident(i) = t.pat.as_ref() {
                i.clone()
            } else {
                panic!("failed to get ident");
            }
        }
    };

    //get the setup method
    the_impl.items = the_impl
        .items
        .iter()
        .map(|item| match item {
            syn::ImplItem::Method(m) => syn::ImplItem::Method(m.clone()),
            _ => item.clone(),
        })
        .collect();

    //process methods for attributes, add Type if needed
    {
        let attr_add_type = |a: &mut syn::Attribute| {
            //add the wrapper type to the attribute
            a.tokens = quote! {
                (::median::wrapper::#wrapper_type::<#class_name>)
            };
        };
        for imp in &mut processed_impls {
            imp.items = imp
                .items
                .iter()
                .map(|item| match item {
                    syn::ImplItem::Method(m) => {
                        let mut m = m.clone();

                        //find any attributes that end with "tramp" and don't have any tokens
                        //(tokens is the part after the attribute name, including parens)
                        if let Some(pos) = m.attrs.iter().position(|a| {
                            a.tokens.is_empty()
                                && a.path
                                    .segments
                                    .last()
                                    .expect("attribute path to have at least 1 segment")
                                    .ident
                                    .to_string()
                                    .ends_with("tramp")
                        }) {
                            let mut a = m.attrs.remove(pos).clone();
                            attr_add_type(&mut a);
                            m.attrs.push(a);
                        };

                        //create automatic method mappings
                        for (attr_name, var_name, attr_new_name) in [
                            ("bang", "Bang", "tramp"),
                            ("int", "Int", "tramp"),
                            ("float", "Float", "tramp"),
                            ("sym", "Symbol", "tramp"),
                            ("list", "List", "list_tramp"),
                            ("any", "Anything", "sel_list_tramp"),
                        ] {
                            if let Some(pos) = m.attrs.iter().position(|a| {
                                a.path
                                    .segments
                                    .last()
                                    .expect("attribute path to have at least 1 segment")
                                    .ident
                                    == attr_name
                            }) {
                                //create a tramp and register the method
                                let mut a = m.attrs.remove(pos).clone();
                                a.path = syn::parse_str(&format!("::median::wrapper::{}", attr_new_name)).expect("to make tramp");
                                attr_add_type(&mut a);

                                let tramp_name = std::format!("{}_tramp", m.sig.ident);
                                let tramp_name = Ident::new(tramp_name.as_str(), m.span());
                                let var_name = Ident::new(var_name, a.span());

                                class_setup.block.stmts.push(
                                    syn::parse(
                                        quote! { #class_setup_class_var.add_method(median::method::Method::#var_name(Self::#tramp_name)).unwrap(); }.into()
                                    ).expect("to create a statement"));
                                m.attrs.push(a);
                            };
                        }

                        syn::ImplItem::Method(m)
                    }
                    _ => item.clone(),
                })
                .collect();
        }
    }

    the_impl.items.push(syn::ImplItem::Method(class_setup));

    processed_impls.push(the_impl);

    Ok(ImplDetails {
        wrapper_type,
        processed_impls,
    })
}

fn process(items: Vec<Item>) -> syn::Result<(proc_macro2::TokenStream, Ident)> {
    let mut impls = Vec::new();
    let mut the_struct = None;
    let mut remain = Vec::new();
    let mut has_obj_wrapped = false;

    for item in items.iter() {
        match item {
            Item::Struct(i) => {
                if the_struct.is_some() {
                    return Err(syn::Error::new(i.span(), "only one wrapped struct allowed"));
                }
                the_struct = Some(i);
            }
            Item::Impl(i) => {
                //see if ObjWrapped is already implemented so we don't double impl
                match &i.trait_ {
                    Some((_, path, _)) => {
                        if let Some(l) = path.segments.last() {
                            if l.ident == "ObjWrapped" {
                                has_obj_wrapped = true;
                            }
                        }
                    }
                    _ => (),
                }
                impls.push(i.clone());
            }
            _ => remain.push(item),
        }
    }

    //process the struct, getting the names
    let StructDetails {
        the_struct,
        class_name,
        class_alias,
    } = process_struct(the_struct.unwrap().clone())?;

    //process the impls, getting the wrapper type
    let ImplDetails {
        wrapper_type,
        processed_impls: impls,
    } = process_impls(&the_struct, &class_name, impls)?;

    let mut out = quote! {
        #the_struct

        impl #class_name {
            /// Register your wrapped class with Max
            pub(crate) unsafe fn register() {
                ::median::wrapper::#wrapper_type::<#class_name>::register(false)
            }
        }

        #(#impls)*

        #(#remain)*
    };

    if !has_obj_wrapped {
        let max_class_name = LitStr::new(&class_alias, the_struct.span());
        out = quote! {
            #out
            impl ::median::wrapper::ObjWrapped<#class_name> for #class_name {
                fn class_name() -> &'static str {
                    &#max_class_name
                }
            }
        };
    }

    Ok((out.into(), class_name))
}
