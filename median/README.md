# Median

Ergonomic bindings for the [Max/MSP](https://cycling74.com/) [SDK](https://github.com/Cycling74/max-sdk).

See the [changelog](CHANGELOG.md) for release notes.

Porting an external from 0.2? See the [migration guide](../doc/migration-0.3.md).

## Disclaimer

This is a work in progress.

## Dependencies

* [rust](https://rustup.rs/)
* [cargo make](https://github.com/sagiegurari/cargo-make) for building examples and template projects

## Example

A very basic external that has `bang`, `int`, `list`, and `any` methods, a `set` message, an
attribute, a proxy inlet and a list outlet.

See the [examples folder](examples/README.md) for more detailed examples, and the crate
documentation for the full list of markers and options.

```rust,no_run
use median::prelude::*;

// the struct is the object; the derive makes it a Max class named "example"
#[derive(MaxObject)]
#[max(name = "example")]
pub struct Example {
    obj: ObjRef,
    // an attribute "value" backed by the field, clipped to 0..=100
    #[attr(clip(0, 100))]
    value: Int64,
    // a typed outlet: `send` takes what a list outlet sends
    out: Outlet<List>,
}

impl MaxObjWrapped for Example {
    // `Err` (or `?`) refuses to make the object: the error is posted to the Max console
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        // inlets and outlets, left to right, each with an optional assist text
        builder.inlet(Inlet::default().assist("bang, int, list, set"));
        builder.inlet(Inlet::proxy().assist("bang, int"));
        Ok(Self {
            obj: builder.obj(),
            value: Int64::new(0),
            out: builder.outlet(Outlet::list().assist("the list, reversed")),
        })
    }
}

// the marked methods are registered with Max; the impl is otherwise left as written
#[median::methods]
impl Example {
    #[bang]
    pub fn bang(&self) {
        let i = self.obj.proxy_inlet();
        median::object_post!(&self.obj, "bang from inlet {}", i);
    }

    #[int]
    pub fn int(&self, v: t_atom_long) {
        self.value.set(v);
        post!("int {} from inlet {}", v, self.obj.proxy_inlet());
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        let rev: Vec<Atom> = atoms.iter().rev().filter_map(Atom::get_value).map(Atom::from).collect();
        let _ = self.out.send(&rev);
    }

    #[any]
    pub fn baz(&self, sel: &SymbolRef, atoms: &[Atom]) {
        post!("got any with sel {} and length {}", sel, atoms.len());
    }

    // a method may return `Result<(), E>`: the error is posted to the console for the object
    #[sel("set", defaults = 1)]
    pub fn set(&self, name: SymbolRef) -> Result<(), String> {
        if name == SymbolRef::default() {
            return Err("set what?".into());
        }
        post!("set {}", name);
        Ok(())
    }
}

// the entry point Max calls to register the class
median::ext_main! { Example }
```

Everything the macros generate is ordinary, public API: the `simp_explicit` example is the
`simp` example written without them (`ObjClass`, `FieldAttrs` and `Methods` impls calling
`Class::add_bang(Self::bang)` and friends). That style is supported too, and is the way to
register methods conditionally or under computed names.

## Building Externals

If you use the [utils/Makefile.toml](utils/Makefile.toml) setup, like the
[examples](examples/README.md) or projects based off the [median templates](../templates),
you should be able to build, package and install with [cargo-make](https://sagiegurari.github.io/cargo-make/).

First, make sure you have [cargo-make](https://sagiegurari.github.io/cargo-make/) installed on your system:

```sh
cargo install cargo-make
```

You can then build the package for your current platform:

```sh
cargo make package # creates a development build
cargo make --profile release package # creates a production build
```

If you'd like to install the built object into Max, run:

```sh
cargo make install # creates and installs a development build
cargo make --profile release install # creates and installs a production build
```

## Cross Compiling

Currently this is only enabled for Mac OS, but with a little bit of work we should be able to cross compile on Linux.
We can build fat (x86 and arm64) Mac OS and Windows externals.

Assuming you're on x86 Mac OS, install the windows and arm64 targets/toolchains:

```
rustup target add x86_64-pc-windows-gnu
rustup target add aarch64-apple-darwin
rustup toolchain install stable-x86_64-pc-windows-gnu
```

Or, *untested*, on an aarch64 (arm64) Mac OS machine:

```
rustup target add x86_64-pc-windows-gnu
rustup target add x86_64-apple-darwin
rustup toolchain install stable-x86_64-pc-windows-gnu
```

Then should be able to build all the externals with:

`cargo make package-all` or `cargo make --profile release package-all`

If this succeeds, you should see a printout of where the externals were put.


## Resources

* [writing docs in rust](https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html)

josuha
```
if you have more than 7 args or if you are mixing floating point and other argument types, use A_GIMME
```
(`#[sel]` and `Class::add_sel` enforce this: a typed selector whose arguments mix `f64` with `t_atom_long`/`SymbolRef` is a compile error, so use `#[sel("name", gimme)]` / `Class::add_sel_gimme` for it. `#[median::methods]` reports it at the method when it can see the types; otherwise, e.g. behind a type alias, a const assertion in `add_sel` fails when the crate is built (`cargo build`, not `cargo check`).)

## TODO

* dictionaries
* Jitter wrapper(s)
* cross compile on linux
  * [cctools](https://github.com/tpoechtrager/cctools-port) lipo for linux
* [github actions](https://github.com/features/actions)
* explain non mut methods and threading model and `Sync` in docs
