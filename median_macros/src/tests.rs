//! Validate our diagnostics before rustc renders them. Each `.errors` line records the
//! source line and message of one error, so missing errors and misplaced spans fail too.

use crate::derive::Kind;
use proc_macro2::TokenStream;
use quote::quote;
use std::{fs, path::Path};

fn emitted_errors(tokens: TokenStream) -> Vec<String> {
    syn::parse2::<syn::File>(tokens)
        .expect("expansion must be syntactically valid Rust")
        .items
        .into_iter()
        .filter_map(|item| match item {
            syn::Item::Macro(item)
                if item
                    .mac
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "compile_error") =>
            {
                let message = syn::parse2::<syn::LitStr>(item.mac.tokens).unwrap();
                Some(format!(
                    "{}: {}",
                    message.span().start().line,
                    message.value()
                ))
            }
            _ => None,
        })
        .collect()
}

fn is_methods(attr: &syn::Attribute) -> bool {
    attr.path()
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .eq(["median", "methods"])
}

#[test]
fn validation_errors() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/validation");
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no validation fixtures found");
    for path in files {
        let input = syn::parse_file(&fs::read_to_string(&path).unwrap()).unwrap();
        let mut actual = Vec::new();
        for item in input.items {
            match item {
                syn::Item::Struct(item) => {
                    // These fixtures use MaxObj; structs without a derive are just scaffolding.
                    if item.attrs.iter().any(|a| a.path().is_ident("derive")) {
                        let input = syn::parse2(quote!(#item)).unwrap();
                        if let Err(errors) = crate::derive::derive(input, Kind::Max) {
                            actual.extend(
                                errors.into_iter().map(|error| {
                                    format!("{}: {}", error.span().start().line, error)
                                }),
                            );
                        }
                    }
                }
                syn::Item::Impl(mut item) => {
                    let marked = item.attrs.iter().any(is_methods);
                    if marked {
                        item.attrs.retain(|a| !is_methods(a));
                        actual.extend(emitted_errors(crate::methods::methods(
                            TokenStream::new(),
                            item,
                        )));
                    }
                }
                _ => {}
            }
        }
        let expected = fs::read_to_string(path.with_extension("errors")).unwrap();
        assert_eq!(
            actual,
            expected.lines().collect::<Vec<_>>(),
            "{}",
            path.display()
        );
    }
}

#[test]
fn valid_macro_inputs() {
    for kind in [Kind::Max, Kind::Msp, Kind::Matrix, Kind::Gl] {
        let input = syn::parse_quote! {
            #[max(name = "valid", description = "Example", tags = "a, b")]
            struct Valid {
                #[attr(save, range(0, 1), label = "Value")]
                value: Int64,
            }
        };
        let output = crate::derive::derive(input, kind).unwrap();
        assert!(emitted_errors(output).is_empty());
    }
    let input = syn::parse_quote! {
        impl Valid {
            #[bang]
            fn bang(&self) {}
            #[sel("floats")]
            fn floats(&self, a: f64, b: f64) {}
            #[sel("others")]
            fn others(&self, a: t_atom_long, b: SymbolRef) {}
            #[notify]
            fn notified(&self, n: &Notification) {}
        }
    };
    assert!(emitted_errors(crate::methods::methods(TokenStream::new(), input)).is_empty());
}
