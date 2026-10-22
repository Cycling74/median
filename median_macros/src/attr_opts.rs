//! The options of `#[attr(..)]`, shared by field attributes (`#[derive(MaxObject)]`) and getter
//! attributes (`#[median::methods]`).

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{
    Attribute, Expr, LitStr, Meta, Token, parenthesized, parse::Parse, punctuated::Punctuated,
    spanned::Spanned,
};

/// Options that `AttrBuilder` doesn't support yet. They are rejected with an error that says so,
/// rather than as unknown.
const LATER: &[&str] = &[
    "label", "style", "category", "order", "default", "save", "enum", "hidden", "range",
];

pub struct AttrOpts {
    pub name: Option<LitStr>,
    clip: Option<(Expr, Expr)>,
    pub readonly: bool,
    /// The span of the whole `#[attr(..)]`.
    pub span: Span,
}

impl AttrOpts {
    /// Parse `#[attr]` or `#[attr(name = "..", clip(min, max), readonly)]`.
    pub fn parse(attr: &Attribute) -> syn::Result<Self> {
        let mut opts = AttrOpts {
            name: None,
            clip: None,
            readonly: false,
            span: attr.span(),
        };
        match &attr.meta {
            Meta::Path(_) => return Ok(opts),
            Meta::NameValue(nv) => {
                return Err(syn::Error::new(
                    nv.span(),
                    "expected `#[attr]` or `#[attr(name = \"..\", clip(min, max), readonly)]`",
                ));
            }
            Meta::List(_) => {}
        }
        attr.parse_nested_meta(|meta| {
            let key = meta
                .path
                .get_ident()
                .map(|i| i.to_string())
                .unwrap_or_default();
            match key.as_str() {
                "name" => {
                    if opts.name.is_some() {
                        return Err(meta.error("`name` given twice"));
                    }
                    let name: LitStr = meta.value()?.parse()?;
                    crate::cstr::check(&name)?;
                    opts.name = Some(name);
                }
                "clip" => {
                    if opts.clip.is_some() {
                        return Err(meta.error("`clip` given twice"));
                    }
                    let content;
                    parenthesized!(content in meta.input);
                    let bounds: Punctuated<Expr, Token![,]> =
                        content.parse_terminated(Expr::parse, Token![,])?;
                    if bounds.len() != 2 {
                        return Err(meta.error("expected `clip(min, max)`"));
                    }
                    let mut b = bounds.into_iter();
                    opts.clip = Some((b.next().unwrap(), b.next().unwrap()));
                }
                "readonly" => {
                    if opts.readonly {
                        return Err(meta.error("`readonly` given twice"));
                    }
                    opts.readonly = true;
                }
                k if LATER.contains(&k) => {
                    return Err(meta.error(format!(
                        "the attribute option `{k}` is not supported yet; only `name`, `clip` and \
                         `readonly` are (the others come with the attribute options of the API \
                         redesign, step 7)"
                    )));
                }
                _ => {
                    return Err(meta.error(
                        "unknown attribute option, expected `name = \"..\"`, `clip(min, max)` or \
                         `readonly`",
                    ));
                }
            }
            Ok(())
        })?;
        Ok(opts)
    }

    /// The `median::attr::AttrOptions` expression.
    pub fn options(&self) -> TokenStream {
        let mut o = quote_spanned!(self.span=> ::median::attr::AttrOptions::new());
        if let Some((min, max)) = &self.clip {
            o = quote!(#o.clip((#min) as f64, (#max) as f64));
        }
        if self.readonly {
            o = quote!(#o.readonly());
        }
        o
    }
}

