//! `#[median::methods]`: re-emits an inherent impl without its marker attributes, and implements
//! `median::wrapper::Methods` for the type with one `Class::add_*` call per marked method.

use crate::attr_opts::AttrOpts;
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{
    Attribute, FnArg, ImplItem, ImplItemFn, ItemImpl, LitInt, LitStr, ReturnType, Signature, Type,
    spanned::Spanned,
};

/// The most typed arguments a `#[sel]` method can have (what `class_addmethod` takes).
const MAX_SEL_ARGS: usize = 7;

enum Marker {
    Bang,
    Dblclick,
    Loadbang,
    Notify,
    Dictionary,
    Savestate,
    Int,
    Float,
    Sym,
    List,
    Any,
    Sel {
        name: LitStr,
        defaults: Option<LitInt>,
        gimme: bool,
    },
    Attr(Box<AttrOpts>),
    /// `#[defer]` (`low: false`) or `#[defer_low]`: goes with one of the message markers.
    Defer {
        low: bool,
    },
}

impl Marker {
    /// The marker of `attr`, `None` if it isn't one.
    fn parse(attr: &Attribute) -> Option<syn::Result<Self>> {
        let ident = attr.path().get_ident()?.to_string();
        let no_args = |m: Marker| match &attr.meta {
            syn::Meta::Path(_) => Ok(m),
            other => Err(syn::Error::new(
                other.span(),
                format!("`#[{ident}]` takes no arguments"),
            )),
        };
        Some(match ident.as_str() {
            "bang" => no_args(Marker::Bang),
            "dblclick" => no_args(Marker::Dblclick),
            "loadbang" => no_args(Marker::Loadbang),
            "notify" => no_args(Marker::Notify),
            "dictionary" => no_args(Marker::Dictionary),
            "savestate" => no_args(Marker::Savestate),
            "int" => no_args(Marker::Int),
            "float" => no_args(Marker::Float),
            "sym" => no_args(Marker::Sym),
            "list" => no_args(Marker::List),
            "any" => no_args(Marker::Any),
            "sel" => Self::parse_sel(attr, false),
            "sel_gimme" => Self::parse_sel(attr, true),
            "attr" => AttrOpts::parse(attr).map(|o| Marker::Attr(Box::new(o))),
            "defer" => no_args(Marker::Defer { low: false }),
            "defer_low" => no_args(Marker::Defer { low: true }),
            _ => return None,
        })
    }

    /// Typed selectors accept defaults; gimme selectors accept only a name.
    fn parse_sel(attr: &Attribute, gimme: bool) -> syn::Result<Self> {
        let form = if gimme {
            "expected `#[sel_gimme(\"name\")]`"
        } else {
            "expected `#[sel(\"name\")]` or `#[sel(\"name\", defaults = n)]`"
        };
        let list = match &attr.meta {
            syn::Meta::List(l) => l,
            other => return Err(syn::Error::new(other.span(), form)),
        };
        list.parse_args_with(|input: syn::parse::ParseStream| {
            let name: LitStr = input.parse().map_err(|e| syn::Error::new(e.span(), form))?;
            crate::cstr::check(&name)?;
            if crate::reserved::RESERVED_SELECTORS.contains(&name.value().as_str()) {
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "`{}` is a selector median registers for itself (inlets, assist, dsp); \
                         pick another name",
                        name.value()
                    ),
                ));
            }
            if crate::reserved::UNTYPED_SELECTORS.contains(&name.value().as_str()) {
                let hint = match crate::reserved::untyped_marker(&name.value()) {
                    Some(marker) => format!("use `{marker}` instead"),
                    None => "median has no hook for it yet".to_string(),
                };
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "`{}` is an untyped message Max calls with its own arguments, which a \
                         typed selector would misread; {hint}",
                        name.value(),
                    ),
                ));
            }
            let mut defaults = None;
            while !input.is_empty() {
                input.parse::<syn::Token![,]>()?;
                if input.is_empty() {
                    break;
                }
                let key: syn::Ident = input.parse()?;
                if gimme {
                    return Err(syn::Error::new(
                        key.span(),
                        "`#[sel_gimme]` takes only a selector name; it receives all the atoms",
                    ));
                }
                match key.to_string().as_str() {
                    "defaults" if defaults.is_none() => {
                        input.parse::<syn::Token![=]>()?;
                        let n: LitInt = input.parse()?;
                        n.base10_parse::<usize>()?;
                        defaults = Some(n);
                    }
                    "gimme" => {
                        return Err(syn::Error::new(
                            key.span(),
                            "use `#[sel_gimme(\"name\")]` instead of `#[sel(\"name\", gimme)]`",
                        ));
                    }
                    "defaults" => {
                        return Err(syn::Error::new(key.span(), format!("`{key}` given twice")));
                    }
                    _ => {
                        return Err(syn::Error::new(
                            key.span(),
                            "unknown option, expected `defaults = n`",
                        ));
                    }
                }
            }
            Ok(Marker::Sel {
                name,
                defaults,
                gimme,
            })
        })
    }

    /// The expected signature, for errors.
    fn expected(&self) -> &'static str {
        match self {
            Marker::Bang => "`#[bang]` method must take `(&self)`",
            Marker::Dblclick => "`#[dblclick]` method must take `(&self)`",
            Marker::Loadbang => "`#[loadbang]` method must take `(&self)`",
            Marker::Notify => "`#[notify]` method must take `(&self, &Notification)`",
            Marker::Dictionary => "`#[dictionary]` method must take `(&self, Dict)`",
            Marker::Savestate => "`#[savestate]` method must take `(&self, &Dict)`",
            Marker::Int => "`#[int]` method must take `(&self, t_atom_long)`",
            Marker::Float => "`#[float]` method must take `(&self, f64)`",
            Marker::Sym => "`#[sym]` method must take `(&self, SymbolRef)`",
            Marker::List => "`#[list]` method must take `(&self, &[Atom])`",
            Marker::Any => "`#[any]` method must take `(&self, &SymbolRef, &[Atom])`",
            Marker::Sel { gimme: true, .. } => "`#[sel_gimme]` method must take `(&self, &[Atom])`",
            Marker::Sel { .. } => {
                "`#[sel]` method must take `(&self, ..)` with up to 7 arguments, each `f64`, \
                 `t_atom_long` or `SymbolRef`"
            }
            Marker::Attr(_) => {
                "`#[attr]` getter must take `(&self)` and return the value (`f64`, `f32`, \
                 `t_atom_long` or `SymbolRef`)"
            }
            Marker::Defer { .. } => "`#[defer]` goes with a message marker",
        }
    }
}

/// What an argument has to look like, syntactically. The compiler checks the actual types (the
/// `Class::add_*` bounds); this only catches what can be told from the syntax, so the error says
/// which signature is expected.
#[derive(Clone, Copy)]
enum Arg {
    /// Not a reference (`t_atom_long`, `f64`, `SymbolRef`).
    Value,
    /// `&[..]`
    Slice,
    /// `&T`, not a slice
    Ref,
}

fn arg_matches(ty: &Type, want: Arg) -> bool {
    match (ty, want) {
        (Type::Reference(_), Arg::Value) => false,
        (_, Arg::Value) => true,
        (Type::Reference(r), Arg::Slice) => matches!(*r.elem, Type::Slice(_)),
        (Type::Reference(r), Arg::Ref) => !matches!(*r.elem, Type::Slice(_)),
        _ => false,
    }
}

/// The arguments after `&self` that `check_sig` accepts.
#[derive(Clone, Copy)]
enum Want<'a> {
    /// Exactly these, each looking like its `Arg`.
    Exact(&'a [Arg]),
    /// Any number of value arguments, up to `MAX_SEL_ARGS` (a typed `#[sel]`).
    Values,
    /// One or two arguments of any type (`#[sel_gimme]`): `Class::add_sel_gimme` takes
    /// `(&self, &[Atom])` or `(&self, &SymbolRef, &[Atom])`, and the compiler's `GimmeFn` bound
    /// says what is wrong with the types.
    Gimme,
}

/// Check that `sig` is `(&self, <args>)` with arguments that look like `want`, and that it is a
/// plain method (no generics, not async/unsafe/extern). Returns the number of arguments after
/// `&self`.
fn check_sig(sig: &Signature, want: Want, expected: &str) -> syn::Result<usize> {
    let err = || syn::Error::new(sig.span(), expected);
    if let Some(a) = &sig.asyncness {
        return Err(syn::Error::new(
            a.span(),
            "a median method can't be `async`",
        ));
    }
    if let syn::Safety::Unsafe(u) = &sig.safety {
        return Err(syn::Error::new(
            u.span(),
            "a median method can't be `unsafe`",
        ));
    }
    if let Some(a) = &sig.abi {
        return Err(syn::Error::new(
            a.span(),
            "a median method can't have an ABI",
        ));
    }
    if sig.generics.type_params().next().is_some() || sig.generics.const_params().next().is_some() {
        return Err(syn::Error::new(
            sig.generics.span(),
            "a median method can't be generic",
        ));
    }
    if let Some(v) = &sig.variadic {
        return Err(syn::Error::new(v.span(), expected));
    }
    let mut inputs = sig.inputs.iter();
    match inputs.next() {
        Some(FnArg::Receiver(r))
            if matches!(r.kind, syn::ReceiverKind::Reference(_, _, None))
                && r.mutability.is_none() => {}
        _ => return Err(err()),
    }
    let args: Vec<&Type> = inputs
        .map(|a| match a {
            FnArg::Typed(t) => Ok(&*t.ty),
            FnArg::Receiver(_) => Err(err()),
        })
        .collect::<syn::Result<_>>()?;
    match want {
        Want::Exact(want) => {
            if args.len() != want.len() || args.iter().zip(want).any(|(t, w)| !arg_matches(t, *w)) {
                return Err(err());
            }
        }
        Want::Values => {
            if args.len() > MAX_SEL_ARGS || args.iter().any(|t| !arg_matches(t, Arg::Value)) {
                return Err(err());
            }
        }
        Want::Gimme => {
            if !(1..=2).contains(&args.len()) {
                return Err(err());
            }
        }
    }
    Ok(args.len())
}

/// The argument type is written `f64` (best effort: an alias isn't recognised; the const
/// assertion in `median`'s `SelFn` catches what this misses).
fn is_f64(ty: &Type) -> bool {
    match ty {
        Type::Path(p) if p.qself.is_none() => p
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "f64" && s.arguments.is_empty()),
        Type::Paren(p) => is_f64(&p.elem),
        Type::Group(g) => is_f64(&g.elem),
        _ => false,
    }
}

/// A typed `#[sel]` method's arguments must be all `f64` or none: Max passes floats and other
/// arguments of a typed method in different registers.
fn check_no_float_mix(sig: &Signature) -> syn::Result<()> {
    let args: Vec<&Type> = sig
        .inputs
        .iter()
        .filter_map(|a| match a {
            FnArg::Typed(t) => Some(&*t.ty),
            FnArg::Receiver(_) => None,
        })
        .collect();
    let floats = args.iter().filter(|t| is_f64(t)).count();
    if floats > 0 && floats < args.len() {
        return Err(syn::Error::new(
            sig.span(),
            "a typed selector cannot mix f64 with other argument types (Max passes them in \
             different registers); use #[sel_gimme(\"name\")] / add_sel_gimme",
        ));
    }
    Ok(())
}

/// A marked method, checked.
struct Marked {
    ident: syn::Ident,
    marker: Marker,
    marker_span: Span,
    /// `#[defer]` (`Some(false)`) or `#[defer_low]` (`Some(true)`).
    defer: Option<bool>,
}

pub fn methods(args: TokenStream, mut item: ItemImpl) -> TokenStream {
    let mut errors: Option<syn::Error> = None;
    let mut push_err = |e: syn::Error| match &mut errors {
        Some(errs) => errs.combine(e),
        None => errors = Some(e),
    };
    if !args.is_empty() {
        push_err(syn::Error::new(
            args.span(),
            "#[median::methods] takes no arguments",
        ));
    }
    if let Some((path, _)) = &item.trait_ {
        push_err(syn::Error::new(
            path.span(),
            "#[median::methods] goes on an inherent impl (`impl MyObject { .. }`), not a trait impl",
        ));
    }
    if !item.generics.params.is_empty() {
        push_err(syn::Error::new(
            item.generics.span(),
            "#[median::methods] does not support generic impls: a Max class is one concrete type",
        ));
    }

    //take the markers off the methods, checking each
    let mut marked: Vec<Marked> = Vec::new();
    for it in item.items.iter_mut() {
        let ImplItem::Fn(f) = it else { continue };
        let mut found: Option<(Marker, Span)> = None;
        let mut defer: Option<(bool, Span)> = None;
        let mut keep = Vec::new();
        for attr in std::mem::take(&mut f.attrs) {
            match Marker::parse(&attr) {
                None => keep.push(attr),
                Some(Err(e)) => push_err(e),
                Some(Ok(Marker::Defer { low })) => {
                    if defer.is_some() {
                        push_err(syn::Error::new(
                            attr.span(),
                            "a method can have only one of `#[defer]` and `#[defer_low]`",
                        ));
                    } else {
                        defer = Some((low, attr.span()));
                    }
                }
                Some(Ok(m)) => {
                    if found.is_some() {
                        push_err(syn::Error::new(
                            attr.span(),
                            "a method can have only one median marker",
                        ));
                    } else {
                        found = Some((m, attr.span()));
                    }
                }
            }
        }
        f.attrs = keep;
        if let Some((low, span)) = defer {
            let name = if low { "defer_low" } else { "defer" };
            match &found {
                None => {
                    push_err(syn::Error::new(
                        span,
                        format!(
                            "`#[{name}]` goes with a message marker: `#[bang]`, `#[int]`, \
                             `#[float]`, `#[sym]`, `#[list]`, `#[any]`, `#[sel(..)]` or `#[sel_gimme(..)]`"
                        ),
                    ));
                    continue;
                }
                Some((Marker::Notify, _)) => {
                    push_err(syn::Error::new(
                        span,
                        "notify callbacks cannot be deferred; a notification is valid only during the call, copy the data explicitly",
                    ));
                    continue;
                }
                Some((Marker::Dictionary | Marker::Savestate, _)) => {
                    push_err(syn::Error::new(
                        span,
                        "dictionary and savestate callbacks cannot be deferred; copy the data explicitly",
                    ));
                    continue;
                }
                Some((Marker::Attr(_), _)) => {
                    push_err(syn::Error::new(
                        span,
                        format!(
                            "`#[{name}]` doesn't go with `#[attr]`: attribute accessors aren't deferred"
                        ),
                    ));
                    continue;
                }
                Some((Marker::Dblclick | Marker::Loadbang, _)) => {
                    push_err(syn::Error::new(
                        span,
                        format!(
                            "`#[{name}]` doesn't go with `#[dblclick]` or `#[loadbang]`: Max calls \
                             them on the main thread already"
                        ),
                    ));
                    continue;
                }
                Some(_) => {}
            }
        }
        let Some((marker, marker_span)) = found else {
            continue;
        };
        use Arg::*;
        let want = match &marker {
            Marker::Bang | Marker::Dblclick | Marker::Loadbang | Marker::Attr(_) => {
                Want::Exact(&[])
            }
            Marker::Int | Marker::Float | Marker::Sym | Marker::Dictionary => Want::Exact(&[Value]),
            Marker::Savestate | Marker::Notify => Want::Exact(&[Ref]),
            Marker::List => Want::Exact(&[Slice]),
            Marker::Any => Want::Exact(&[Ref, Slice]),
            Marker::Sel { gimme: true, .. } => Want::Gimme,
            Marker::Sel { .. } => Want::Values,
            Marker::Defer { .. } => unreachable!("taken out above"),
        };
        match check_sig(&f.sig, want, marker.expected()) {
            Ok(arity) => {
                if let Marker::Attr(_) = &marker {
                    if !returns_value(&f.sig) {
                        push_err(syn::Error::new(f.sig.span(), marker.expected()));
                        continue;
                    }
                }
                if let Marker::Sel { gimme: false, .. } = &marker {
                    if let Err(e) = check_no_float_mix(&f.sig) {
                        push_err(e);
                        continue;
                    }
                }
                if let Marker::Sel {
                    defaults: Some(d), ..
                } = &marker
                {
                    if d.base10_parse::<usize>().unwrap_or(0) > arity {
                        push_err(syn::Error::new(
                            d.span(),
                            format!(
                                "`defaults = {d}` is more than the method's {arity} argument(s)"
                            ),
                        ));
                        continue;
                    }
                }
                marked.push(Marked {
                    ident: f.sig.ident.clone(),
                    marker,
                    marker_span,
                    defer: defer.map(|(low, _)| low),
                });
            }
            Err(e) => push_err(e),
        }
    }

    //the setters, by name, for `#[attr]` getters
    let fns: Vec<&ImplItemFn> = item
        .items
        .iter()
        .filter_map(|i| match i {
            ImplItem::Fn(f) => Some(f),
            _ => None,
        })
        .collect();

    let mut selectors: Vec<String> = Vec::new();
    let mut calls = Vec::new();
    for m in &marked {
        let span = m.ident.span();
        let path = {
            let ident = &m.ident;
            quote_spanned!(span=> Self::#ident)
        };
        let selector = match &m.marker {
            Marker::Bang => Some("bang".to_string()),
            Marker::Dblclick => Some("dblclick".to_string()),
            Marker::Loadbang => Some("loadbang".to_string()),
            Marker::Notify => Some("notify".to_string()),
            Marker::Dictionary => Some("dictionary".to_string()),
            Marker::Savestate => Some("appendtodictionary".to_string()),
            Marker::Int => Some("int".to_string()),
            Marker::Float => Some("float".to_string()),
            Marker::Sym => Some("symbol".to_string()),
            Marker::List => Some("list".to_string()),
            Marker::Any => Some("anything".to_string()),
            Marker::Sel { name, .. } => Some(name.value()),
            Marker::Attr(o) => Some(
                o.name
                    .as_ref()
                    .map_or_else(|| m.ident.unraw_string(), LitStr::value),
            ),
            Marker::Defer { .. } => None,
        };
        if let Some(sel) = selector {
            if selectors.contains(&sel) {
                push_err(syn::Error::new(
                    m.marker_span,
                    format!("`{sel}` is registered twice in this impl"),
                ));
                continue;
            }
            selectors.push(sel);
        }
        let mode = m.defer.map(|low| {
            if low {
                quote_spanned!(span=> ::median::defer::Defer::Low)
            } else {
                quote_spanned!(span=> ::median::defer::Defer::Front)
            }
        });
        let call = match (&m.marker, &mode) {
            (Marker::Bang, None) => quote_spanned!(span=> c.add_bang(#path)?;),
            (Marker::Dblclick, _) => quote_spanned!(span=> c.add_dblclick(#path)?;),
            (Marker::Loadbang, _) => quote_spanned!(span=> c.add_loadbang(#path)?;),
            (Marker::Notify, _) => quote_spanned!(span=> c.add_notify(#path)?;),
            (Marker::Dictionary, _) => quote_spanned!(span=> c.add_dictionary(#path)?;),
            (Marker::Savestate, _) => quote_spanned!(span=> c.add_savestate(#path)?;),
            (Marker::Int, None) => quote_spanned!(span=> c.add_int(#path)?;),
            (Marker::Float, None) => quote_spanned!(span=> c.add_float(#path)?;),
            (Marker::Sym, None) => quote_spanned!(span=> c.add_sym(#path)?;),
            (Marker::List, None) => quote_spanned!(span=> c.add_list(#path)?;),
            (Marker::Any, None) => quote_spanned!(span=> c.add_any(#path)?;),
            (Marker::Bang, Some(mode)) => {
                quote_spanned!(span=> c.add_bang_deferred(#path, #mode)?;)
            }
            (Marker::Int, Some(mode)) => quote_spanned!(span=> c.add_int_deferred(#path, #mode)?;),
            (Marker::Float, Some(mode)) => {
                quote_spanned!(span=> c.add_float_deferred(#path, #mode)?;)
            }
            (Marker::Sym, Some(mode)) => quote_spanned!(span=> c.add_sym_deferred(#path, #mode)?;),
            (Marker::List, Some(mode)) => {
                quote_spanned!(span=> c.add_list_deferred(#path, #mode)?;)
            }
            (Marker::Any, Some(mode)) => quote_spanned!(span=> c.add_any_deferred(#path, #mode)?;),
            (
                Marker::Sel {
                    name,
                    defaults,
                    gimme,
                },
                mode,
            ) => {
                let sel = crate::cstr::expr(&name.value(), name.span());
                let d = defaults
                    .as_ref()
                    .map_or(0, |d| d.base10_parse::<usize>().unwrap_or(0));
                match (gimme, mode) {
                    (true, None) => quote_spanned!(span=> c.add_sel_gimme(#sel, #path)?;),
                    (false, None) => quote_spanned!(span=> c.add_sel(#sel, #path, #d)?;),
                    (true, Some(mode)) => {
                        quote_spanned!(span=> c.add_sel_gimme_deferred(#sel, #path, #mode)?;)
                    }
                    (false, Some(mode)) => {
                        quote_spanned!(span=> c.add_sel_deferred(#sel, #path, #d, #mode)?;)
                    }
                }
            }
            (Marker::Defer { .. }, _) => unreachable!("not a marker of a method"),
            (Marker::Attr(opts), _) => {
                let name = opts
                    .name
                    .as_ref()
                    .map_or_else(|| m.ident.unraw_string(), LitStr::value);
                let name_expr =
                    crate::cstr::expr(&name, opts.name.as_ref().map_or(span, LitStr::span));
                let options = opts.options();
                let setter_name = format!("set_{}", m.ident.unraw_string());
                let setter = fns.iter().find(|f| f.sig.ident == setter_name);
                match setter {
                    Some(_) if opts.readonly => {
                        quote_spanned!(span=> c.add_attr_readonly(#name_expr, #path, #options)?;)
                    }
                    Some(s) => {
                        let expected = format!(
                            "`{setter_name}`, the setter of the `#[attr]` `{}`, must take \
                             `(&self, value)`",
                            m.ident
                        );
                        if let Err(e) = check_sig(&s.sig, Want::Exact(&[Arg::Value]), &expected) {
                            push_err(e);
                            continue;
                        }
                        let set_ident = &s.sig.ident;
                        let set_path = quote_spanned!(set_ident.span()=> Self::#set_ident);
                        quote_spanned!(span=> c.add_attr(#name_expr, #path, #set_path, #options)?;)
                    }
                    None => {
                        quote_spanned!(span=> c.add_attr_readonly(#name_expr, #path, #options)?;)
                    }
                }
            }
        };
        calls.push(call);
    }

    let self_ty = &item.self_ty;
    let methods_impl = match &errors {
        //an empty impl, so the errors aren't followed by "`Methods` is not implemented"
        Some(_) => quote!(impl ::median::wrapper::Methods for #self_ty {}),
        None => quote! {
            impl ::median::wrapper::Methods for #self_ty {
                #[allow(unused_variables)]
                fn register<W>(c: &mut ::median::class::Class<W>) -> ::median::error::MaxResult<()>
                where
                    W: ::median::wrapper::WrapperWrapped<Self> + ::median::object::MaxObj,
                {
                    #(#calls)*
                    ::core::result::Result::Ok(())
                }
            }
        },
    };
    let errors = errors.map(|e| e.to_compile_error());
    quote! {
        #item
        #methods_impl
        #errors
    }
}

/// The method returns something other than `()`.
fn returns_value(sig: &Signature) -> bool {
    match &sig.output {
        ReturnType::Default => false,
        ReturnType::Type(_, t) => !matches!(&**t, Type::Tuple(t) if t.elems.is_empty()),
    }
}

trait UnrawString {
    fn unraw_string(&self) -> String;
}

impl UnrawString for syn::Ident {
    fn unraw_string(&self) -> String {
        syn::ext::IdentExt::unraw(self).to_string()
    }
}
