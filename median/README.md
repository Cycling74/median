# Median

Ergonomic bindings for the [Max/MSP](https://cycling74.com/) [SDK](https://github.com/Cycling74/max-sdk).

## Disclaimer

This is a work in progress.

## Dependencies

* [rust](https://rustup.rs/)
* [cargo make](https://github.com/sagiegurari/cargo-make) for building examples

## Example

A very basic external that has `bang`, `int`, `list`, and `any` methods.

See the [examples folder](examples/README.md) for more detailed examples.

```rust,no_run
use median::{
    atom::Atom, builder::MaxWrappedBuilder, max_sys::t_atom_long, object::MaxObj, post,
    symbol::SymbolRef, wrapper::*,
};

median::external! {
    pub struct Example;

    impl MaxObjWrapped<Example> for Example {
        fn new(builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
            let _ = builder.add_inlet(median::inlet::MaxInlet::Proxy);
            Self
        }
    }

    impl Example {
        #[bang]
        pub fn bang(&self) {
            let i = median::inlet::Proxy::get_inlet(self.max_obj());
            median::object_post!(self.max_obj(), "bang from inlet {}", i);
        }

        #[int]
        pub fn int(&self, v: t_atom_long) {
            let i = median::inlet::Proxy::get_inlet(self.max_obj());
            post!("int {} from inlet {}", v, i);
        }

        #[list]
        pub fn list(&self, atoms: &[Atom]) {
            post!("got list with length {}", atoms.len());
        }

        #[any]
        pub fn baz(&self, sel: &SymbolRef, atoms: &[Atom]) {
            post!("got any with sel {} and length {}", sel, atoms.len());
        }
    }
}
```

## Building Externals

If you use the [utils/Makefile.toml](utils/Makefile.toml) setup, like the
[examples](examples/README.md), you should be able to build, package and
install by running the following commands from your external project folder:

```
cargo make build
cargo make package
cargo make install
```

**NOTE**: Each subsequent task initiates the previous so you can simply do `cargo make install` and it will build and package for you.

For example:

```shell
cd examples/simp/ && cargo make install
```

### Release Builds

Add `--profile release` to create optimized release builds:

```
cargo make --profile release build
cargo make --profile release package
cargo make --profile release install
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

## TODO

* dictionaries
* Jitter wrapper(s)
* cross compile on linux
  * [cctools](https://github.com/tpoechtrager/cctools-port) lipo for linux
* [github actions](https://github.com/features/actions)
* explain non mut methods and threading model and `Sync` in docs
