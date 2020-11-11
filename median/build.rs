// build.rs
use quote::quote;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Arg {
    Float,
    Int,
    Symbol,
}

impl Arg {
    pub fn to_string(&self) -> String {
        match self {
            Arg::Float => "F",
            Arg::Int => "I",
            Arg::Symbol => "S",
        }
        .to_string()
    }

    pub fn to_sig(&self) -> proc_macro2::TokenStream {
        match self {
            Arg::Float => quote! { f64 },
            Arg::Int => quote! { i64 },
            Arg::Symbol => quote! { *mut max_sys::t_symbol },
        }
    }

    pub fn to_arg(&self) -> proc_macro2::TokenStream {
        match self {
            Arg::Float => quote! { max_sys::e_max_atomtypes::A_FLOAT },
            Arg::Int => quote! { max_sys::e_max_atomtypes::A_LONG },
            Arg::Symbol => quote! { max_sys::e_max_atomtypes::A_SYM },
        }
    }
}

fn append_perms(types: &[Arg], perms: &mut Vec<Vec<Arg>>, recurse: usize) {
    if recurse == 0 {
        return;
    }
    let mut append = Vec::new();
    for t in types {
        for p in perms.iter() {
            let mut n = p.clone();
            n.push(*t);
            append.push(n);
        }
    }
    append_perms(types, &mut append, recurse - 1);
    for a in append.into_iter() {
        perms.push(a);
    }
}

fn type_alias_name(perm: &Vec<Arg>) -> String {
    perm.iter()
        .map(Arg::to_string)
        .collect::<Vec<String>>()
        .join("")
}

fn sel_variant_name(type_alias: &String) -> syn::Ident {
    syn::Ident::new(
        &format!("Sel{}", type_alias),
        proc_macro2::Span::call_site(),
    )
}

fn gen_method(perms: &Vec<Vec<Arg>>) -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = env::var("OUT_DIR")?;
    let dest_path = Path::new(&out_dir).join("method-gen.rs");
    let mut f = File::create(&dest_path)?;

    //build types
    let mut variants = Vec::new();
    for p in perms.iter() {
        //build type alias
        let alias = type_alias_name(&p);
        let t = syn::Ident::new(&alias, proc_macro2::Span::call_site());
        let v = sel_variant_name(&alias);

        //build method signature
        let args = p.iter().map(Arg::to_sig);
        f.write_all(
            quote! {
                pub type #t<T> = unsafe extern "C" fn(&T, #(#args),*);
            }
            .to_string()
            .as_bytes(),
        )?;
        f.write_all(b"\n")?;

        //build up variant
        //TODO when we allow pointers, don't provide defaults if a pointer is at the end?
        variants.push(quote! {
            #v (&'a str, #t<T>, usize)
        });
    }

    //build enumeration
    f.write_all(
        quote! {
        pub enum Method<'a, T> {
            Bang(B<T>),
            Int(I<T>),
            Float(F<T>),
            Symbol(S<T>),
            List(SelList<T>),
            Anything(SelList<T>),
            Sel(&'a str, B<T>),
            SelVarArg(&'a str, SelList<T>),
            #(#variants),*
        }
        }
        .to_string()
        .as_bytes(),
    )?;

    Ok(())
}

fn gen_class(perms: &Vec<Vec<Arg>>) -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = env::var("OUT_DIR")?;
    let dest_path = Path::new(&out_dir).join("class-gen.rs");
    let mut f = File::create(&dest_path)?;

    //implementation that adds methods to class
    let mut matches = vec![
        quote! {
            Method::Bang(f) => {
                max_sys::class_addmethod(
                    self.class,
                    Some(std::mem::transmute::<crate::method::B<T>, MaxMethod>(f)),
                    ::std::ffi::CString::new("bang").unwrap().as_ptr(),
                    0,
                    );
            }
        },
        quote! {
            Method::Float(f) => {
                max_sys::class_addmethod(
                    self.class,
                    Some(std::mem::transmute::<crate::method::F<T>, MaxMethod>(f)),
                    ::std::ffi::CString::new("float").unwrap().as_ptr(),
                    max_sys::e_max_atomtypes::A_FLOAT,
                    0,
                );
            }
        },
        quote! {
            Method::Int(f) => {
                max_sys::class_addmethod(
                    self.class,
                    Some(std::mem::transmute::<crate::method::I<T>, MaxMethod>(f)),
                    ::std::ffi::CString::new("int").unwrap().as_ptr(),
                    max_sys::e_max_atomtypes::A_LONG,
                    0,
                );
            }
        },
        quote! {
            Method::Symbol(f) => {
                max_sys::class_addmethod(
                    self.class,
                    Some(std::mem::transmute::<crate::method::S<T>, MaxMethod>(f)),
                    ::std::ffi::CString::new("symbol").unwrap().as_ptr(),
                    max_sys::e_max_atomtypes::A_SYM,
                    0,
                );
            }
        },
        quote! {
            Method::List(f) => {
                max_sys::class_addmethod(
                    self.class,
                    Some(std::mem::transmute::<crate::method::SelList<T>, MaxMethod>(f)),
                    ::std::ffi::CString::new("list").unwrap().as_ptr(),
                    max_sys::e_max_atomtypes::A_GIMME,
                    0,
                );
            }
        },
        quote! {
            Method::Anything(f) => {
                max_sys::class_addmethod(
                    self.class,
                    Some(std::mem::transmute::<crate::method::SelList<T>, MaxMethod>(f)),
                    ::std::ffi::CString::new("anything").unwrap().as_ptr(),
                    max_sys::e_max_atomtypes::A_GIMME,
                    0,
                );
            }
        },
        quote! {
            Method::Sel(sel, f) => {
                self.add_sel_method(
                    sel,
                    Some(std::mem::transmute::<crate::method::B<T>, MaxMethod>(f)),
                    &mut [],
                    0,
                );
            }
        },
        quote! {
            Method::SelVarArg(sel, f) => {
                self.add_sel_method(
                    sel,
                    Some(std::mem::transmute::<crate::method::SelList<T>, MaxMethod>(f)),
                    &mut [max_sys::e_max_atomtypes::A_GIMME],
                    0,
                );
            }
        },
    ];
    for p in perms.iter() {
        let alias = type_alias_name(&p);
        let t = syn::Ident::new(&alias, proc_macro2::Span::call_site());
        let v = sel_variant_name(&alias);
        let args = p.iter().map(Arg::to_arg);
        matches.push(quote! {
            Method::#v(sel, f, defaults) => {
                self.add_sel_method(
                    sel,
                    Some(std::mem::transmute::<crate::method::#t<T>, MaxMethod>(f)),
                    &mut [#(#args),*],
                    defaults,
                );
            }
        });
    }

    f.write_all(
        quote! {
        impl<T> Class<T> {
            pub fn add_method(&mut self, m: Method<T>) {
                unsafe {
                    match m {
                        #(#matches)*
                    }
                }
            }
        }
        }
        .to_string()
        .as_bytes(),
    )?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    //don't mix floats and other types
    let mut perms = vec![vec![Arg::Int], vec![Arg::Symbol]];
    let mut fperms = vec![vec![Arg::Float]];
    append_perms(&[Arg::Int, Arg::Symbol], &mut perms, 6);
    append_perms(&[Arg::Float], &mut fperms, 6);
    perms.append(&mut fperms);
    perms.sort();

    gen_method(&perms)?;
    gen_class(&perms)?;

    Ok(())
}
