//! `#[derive(MaxObj)]`, `#[derive(MSPObj)]`, `#[derive(JitMatrixObj)]` and `#[derive(JitGlObj)]`:
//! the class identity (`ObjClass`), `register()`, and the field
//! attributes (`FieldAttrs`) of a struct.

use crate::attr_opts::AttrOpts;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{Data, DeriveInput, Fields, Ident, LitStr, Member, ext::IdentExt, spanned::Spanned};

/// Which derive this is: it picks the wrapper type and the body of `register()`.
#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Max,
    Msp,
    Matrix,
    Gl,
}

impl Kind {
    /// The name of the derive, for error messages.
    fn derive_name(self) -> &'static str {
        match self {
            Kind::Max => "MaxObj",
            Kind::Msp => "MSPObj",
            Kind::Matrix => "JitMatrixObj",
            Kind::Gl => "JitGlObj",
        }
    }
}

/// The value of `key = ".."` in `#[max(..)]`: a string literal, or an error that says so.
fn string_value(meta: &syn::meta::ParseNestedMeta, key: &str) -> syn::Result<LitStr> {
    meta.value()?
        .parse::<LitStr>()
        .map_err(|e| syn::Error::new(e.span(), format!("`{key}` must be a string literal")))
}

pub fn derive(input: DeriveInput, kind: Kind) -> syn::Result<TokenStream> {
    let derive_name = kind.derive_name();
    let ident = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            format!(
                "#[derive({derive_name})] does not support generic types: a Max class is one concrete type"
            ),
        ));
    }
    let fields = match &input.data {
        Data::Struct(s) => &s.fields,
        Data::Enum(e) => {
            return Err(syn::Error::new(
                e.enum_token.span(),
                format!("#[derive({derive_name})] only works on a struct"),
            ));
        }
        Data::Union(u) => {
            return Err(syn::Error::new(
                u.union_token.span(),
                format!("#[derive({derive_name})] only works on a struct"),
            ));
        }
    };

    let mut name: Option<LitStr> = None;
    let mut class_type: Option<Ident> = None;
    let mut no_methods = false;
    let mut description: Option<LitStr> = None;
    let mut author: Option<LitStr> = None;
    let mut tags: Option<(LitStr, Vec<String>)> = None;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("max")) {
        attr.parse_nested_meta(|meta| {
            let key = meta
                .path
                .get_ident()
                .map(|i| i.to_string())
                .unwrap_or_default();
            match key.as_str() {
                "name" => {
                    if name.is_some() {
                        return Err(meta.error("`name` given twice"));
                    }
                    let n: LitStr = meta.value()?.parse()?;
                    crate::cstr::check(&n)?;
                    name = Some(n);
                }
                "class_type" => {
                    if class_type.is_some() {
                        return Err(meta.error("`class_type` given twice"));
                    }
                    let t = meta.value()?.call(Ident::parse_any)?;
                    class_type = Some(match t.to_string().as_str() {
                        "box" => Ident::new("Box", t.span()),
                        "nobox" => Ident::new("NoBox", t.span()),
                        _ => {
                            return Err(syn::Error::new(
                                t.span(),
                                "expected `class_type = box` (the default) or `class_type = nobox`",
                            ));
                        }
                    });
                }
                "no_methods" => {
                    if no_methods {
                        return Err(meta.error("`no_methods` given twice"));
                    }
                    no_methods = true;
                }
                "description" => {
                    if description.is_some() {
                        return Err(meta.error("`description` given twice"));
                    }
                    description = Some(string_value(&meta, "description")?);
                }
                "author" => {
                    if author.is_some() {
                        return Err(meta.error("`author` given twice"));
                    }
                    author = Some(string_value(&meta, "author")?);
                }
                "tags" => {
                    if tags.is_some() {
                        return Err(meta.error("`tags` given twice"));
                    }
                    let lit = string_value(&meta, "tags")?;
                    let mut list = Vec::new();
                    for tag in lit.value().split(',') {
                        let tag = tag.trim();
                        if tag.is_empty() {
                            return Err(syn::Error::new(
                                lit.span(),
                                "`tags` is a comma separated list: an entry must not be empty",
                            ));
                        }
                        list.push(tag.to_owned());
                    }
                    tags = Some((lit, list));
                }
                _ => {
                    return Err(meta.error(
                        "unknown option, expected `name = \"..\"`, \
                         `class_type = box|nobox`, `no_methods`, \
                         `description = \"..\"`, `author = \"..\"` or `tags = \"a, b\"`",
                    ));
                }
            }
            Ok(())
        })?;
    }

    let class_name = name
        .as_ref()
        .map(LitStr::value)
        .unwrap_or_else(|| ident.to_string().to_lowercase());
    let name_span = name.as_ref().map_or(ident.span(), LitStr::span);
    let class_name_expr = crate::cstr::expr(&class_name, name_span);
    let (wrapper, register) = match kind {
        Kind::Max => (
            quote!(::median::wrapper::MaxObjWrapper<Self>),
            quote!(::median::wrapper::MaxObjWrapper::<Self>::register()),
        ),
        Kind::Msp => (
            quote!(::median::wrapper::MSPObjWrapper<Self>),
            quote!(::median::wrapper::MSPObjWrapper::<Self>::register()),
        ),
        Kind::Matrix => (
            quote!(
                ::median::wrapper::MaxObjWrapper<
                    ::median::jit::wrapper::MaxGlue<Self, ::median::jit::wrapper::MatrixKind>,
                >
            ),
            quote!(::median::jit::wrapper::register_matrix::<Self>()),
        ),
        Kind::Gl => (
            quote!(
                ::median::wrapper::MaxObjWrapper<
                    ::median::jit::wrapper::MaxGlue<Self, ::median::jit::wrapper::GlKind>,
                >
            ),
            quote!(::median::jit::wrapper::register_gl::<Self>()),
        ),
    };
    let class_type = class_type.unwrap_or_else(|| format_ident!("Box"));
    //maxref metadata: only emitted when given, so `ObjClass`'s defaults stay in one place
    let description_fn = description.as_ref().map(|d| {
        quote_spanned! {d.span()=>
            fn description() -> ::core::option::Option<&'static str> {
                ::core::option::Option::Some(#d)
            }
        }
    });
    let author_fn = author.as_ref().map(|a| {
        quote_spanned! {a.span()=>
            fn author() -> ::core::option::Option<&'static str> {
                ::core::option::Option::Some(#a)
            }
        }
    });
    let tags_fn = tags.as_ref().map(|(lit, list)| {
        quote_spanned! {lit.span()=>
            fn tags() -> &'static [&'static str] {
                &[#(#list),*]
            }
        }
    });

    //field attributes
    let mut errors: Option<syn::Error> = None;
    let mut push_err = |e: syn::Error| match &mut errors {
        Some(errs) => errs.combine(e),
        None => errors = Some(e),
    };
    let mut attr_names: Vec<String> = Vec::new();
    let mut attr_calls = Vec::new();
    let members: Vec<(Member, &syn::Field)> = match fields {
        Fields::Named(f) => f
            .named
            .iter()
            .map(|f| (Member::Named(f.ident.clone().unwrap()), f))
            .collect(),
        Fields::Unnamed(f) => f
            .unnamed
            .iter()
            .enumerate()
            .map(|(i, f)| (Member::Unnamed(i.into()), f))
            .collect(),
        Fields::Unit => Vec::new(),
    };
    for (member, field) in &members {
        let mut seen = false;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("attr")) {
            if seen {
                push_err(syn::Error::new(attr.span(), "only one `#[attr]` per field"));
                continue;
            }
            seen = true;
            let opts = match AttrOpts::parse(attr) {
                Ok(o) => o,
                Err(e) => {
                    push_err(e);
                    continue;
                }
            };
            let attr_name = match (&opts.name, member) {
                (Some(n), _) => n.value(),
                (None, Member::Named(i)) => i.unraw().to_string(),
                (None, Member::Unnamed(_)) => {
                    push_err(syn::Error::new(
                        attr.span(),
                        "an attribute on a tuple field needs a name: `#[attr(name = \"..\")]`",
                    ));
                    continue;
                }
            };
            if attr_names.contains(&attr_name) {
                push_err(syn::Error::new(
                    opts.name.as_ref().map_or(attr.span(), LitStr::span),
                    format!("the attribute `{attr_name}` is defined twice"),
                ));
                continue;
            }
            let name_expr = crate::cstr::expr(
                &attr_name,
                opts.name.as_ref().map_or(member.span(), LitStr::span),
            );
            attr_names.push(attr_name);
            let options = opts.options();
            let ty = &field.ty;
            //spanned at the field's type, so that a type that isn't an `AttrField` is reported
            //there
            attr_calls.push(quote_spanned! {ty.span()=>
                c.add_field_attr(#name_expr, |s: &Self| -> &#ty { &s.#member }, #options)?;
            });
        }
    }
    if let Some(e) = errors {
        return Err(e);
    }

    let methods_impl = if no_methods {
        quote!(impl ::median::wrapper::Methods for #ident {})
    } else {
        quote!()
    };
    let register_doc = format!(
        "Register the class `{class_name}` with Max: call it from `ext_main` \
         (`median::ext_main! {{ {ident} }}` does). A failure is posted to the Max console."
    );

    Ok(quote! {
        impl ::median::wrapper::ObjClass for #ident {
            type Wrapper = #wrapper;

            fn class_name() -> &'static ::core::ffi::CStr {
                #class_name_expr
            }

            fn class_type() -> ::median::class::ClassType {
                ::median::class::ClassType::#class_type
            }

            #description_fn
            #author_fn
            #tags_fn
        }

        impl #ident {
            #[doc = #register_doc]
            pub fn register() {
                ::median::ffi::guard_ext_main(#class_name, || {
                    #register
                })
            }
        }

        impl ::median::wrapper::FieldAttrs for #ident {
            #[allow(unused_variables)]
            fn register_attrs<W>(c: &mut ::median::class::Class<W>) -> ::median::error::MaxResult<()>
            where
                W: ::median::wrapper::WrapperWrapped<Self> + ::median::object::MaxObjHeader,
            {
                #(#attr_calls)*
                ::core::result::Result::Ok(())
            }
        }

        #methods_impl
    })
}
