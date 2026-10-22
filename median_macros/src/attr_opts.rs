//! The options of `#[attr(..)]`, shared by field attributes (`#[derive(MaxObject)]`) and getter
//! attributes (`#[median::methods]`).

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{
    Attribute, Expr, LitStr, Meta, Token, parenthesized, parse::Parse, punctuated::Punctuated,
    spanned::Spanned,
};

/// The options, for the error message of an unknown one.
const EXPECTED: &str = "expected `name = \"..\"`, `clip(min, max)`, `readonly`, `label = \"..\"`, \
                        `style = \"..\"`, `category = \"..\"`, `order = n`, `default = value`, \
                        `save`, `enum = \"..\"`, `range(min, max)` or `hidden`";

pub struct AttrOpts {
    pub name: Option<LitStr>,
    clip: Option<(Expr, Expr)>,
    pub readonly: bool,
    label: Option<LitStr>,
    style: Option<LitStr>,
    category: Option<LitStr>,
    order: Option<Expr>,
    default: Option<Expr>,
    save: bool,
    enum_vals: Option<LitStr>,
    range: Option<(Expr, Expr)>,
    hidden: bool,
    /// The span of the whole `#[attr(..)]`.
    pub span: Span,
}

/// A string option: a literal that can be a C string.
fn string(
    meta: &syn::meta::ParseNestedMeta,
    slot: &mut Option<LitStr>,
    key: &str,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error(format!("`{key}` given twice")));
    }
    let v: LitStr = meta.value()?.parse()?;
    if v.value().contains('\0') {
        return Err(syn::Error::new(v.span(), "must not contain a NUL"));
    }
    *slot = Some(v);
    Ok(())
}

/// `key(a, b)`.
fn pair(
    meta: &syn::meta::ParseNestedMeta,
    slot: &mut Option<(Expr, Expr)>,
    key: &str,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error(format!("`{key}` given twice")));
    }
    let content;
    parenthesized!(content in meta.input);
    let bounds: Punctuated<Expr, Token![,]> = content.parse_terminated(Expr::parse, Token![,])?;
    if bounds.len() != 2 {
        return Err(meta.error(format!("expected `{key}(min, max)`")));
    }
    let mut b = bounds.into_iter();
    *slot = Some((b.next().unwrap(), b.next().unwrap()));
    Ok(())
}

/// A flag option.
fn flag(meta: &syn::meta::ParseNestedMeta, slot: &mut bool, key: &str) -> syn::Result<()> {
    if *slot {
        return Err(meta.error(format!("`{key}` given twice")));
    }
    *slot = true;
    Ok(())
}

/// `key = expr`.
fn value(meta: &syn::meta::ParseNestedMeta, slot: &mut Option<Expr>, key: &str) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error(format!("`{key}` given twice")));
    }
    *slot = Some(meta.value()?.parse()?);
    Ok(())
}

impl AttrOpts {
    /// Parse `#[attr]` or `#[attr(name = "..", clip(min, max), readonly)]`.
    pub fn parse(attr: &Attribute) -> syn::Result<Self> {
        let mut opts = AttrOpts {
            name: None,
            clip: None,
            readonly: false,
            label: None,
            style: None,
            category: None,
            order: None,
            default: None,
            save: false,
            enum_vals: None,
            range: None,
            hidden: false,
            span: attr.span(),
        };
        match &attr.meta {
            Meta::Path(_) => return Ok(opts),
            Meta::NameValue(nv) => {
                return Err(syn::Error::new(
                    nv.span(),
                    "expected `#[attr]` or `#[attr(option, ..)]`, such as \
                     `#[attr(name = \"..\", clip(min, max), label = \"..\", save)]`",
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
                "clip" => pair(&meta, &mut opts.clip, "clip")?,
                "readonly" => flag(&meta, &mut opts.readonly, "readonly")?,
                "label" => string(&meta, &mut opts.label, "label")?,
                "style" => string(&meta, &mut opts.style, "style")?,
                "category" => string(&meta, &mut opts.category, "category")?,
                "order" => value(&meta, &mut opts.order, "order")?,
                "default" => value(&meta, &mut opts.default, "default")?,
                "save" => flag(&meta, &mut opts.save, "save")?,
                "enum" => string(&meta, &mut opts.enum_vals, "enum")?,
                "range" => pair(&meta, &mut opts.range, "range")?,
                "hidden" => flag(&meta, &mut opts.hidden, "hidden")?,
                _ => return Err(meta.error(format!("unknown attribute option, {EXPECTED}"))),
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
        let cstr = |l: &LitStr| crate::cstr::expr(&l.value(), l.span());
        if let Some(l) = &self.label {
            let l = cstr(l);
            o = quote!(#o.label(#l));
        }
        if let Some(st) = &self.style {
            let st = cstr(st);
            o = quote!(#o.style(#st));
        }
        if let Some(c) = &self.category {
            let c = cstr(c);
            o = quote!(#o.category(#c));
        }
        if let Some(n) = &self.order {
            o = quote!(#o.order((#n) as ::median::max_sys::t_atom_long));
        }
        if let Some(d) = &self.default {
            //a string literal is a parse string, anything else a number
            let d = match d {
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(st),
                    ..
                }) => cstr(st),
                d => quote!(#d),
            };
            o = quote!(#o.default(#d));
        }
        if self.save {
            o = quote!(#o.save());
        }
        if let Some(e) = &self.enum_vals {
            let e = cstr(e);
            o = quote!(#o.enum_vals(#e));
        }
        if let Some((min, max)) = &self.range {
            o = quote!(#o.range((#min) as f64, (#max) as f64));
        }
        if self.hidden {
            o = quote!(#o.hidden());
        }
        o
    }
}
