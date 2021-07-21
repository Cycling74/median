# Median

A Rust wrapper around the `max-sys` automatically generated bindings to the [Max SDK](https://github.com/Cycling74/max-sdk).

## Dependencies

* [rust](https://rustup.rs/)

## Examples

Checkout the examples in [examples/README.md](examples/README.md)

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

`cargo make package-all` or `cargo make package-all --profile release`

If this succeeds, you should see a printout of where the externals were put.


## Resources

* [writing docs in rust](https://facility9.com/2016/05/writing-documentation-in-rust/)

josuha
```
if you have more than 7 args or if you are mixing floating point and other argument types, use A_GIMME
```

## TODO

* cross compile on linux
  * [cctools](https://github.com/tpoechtrager/cctools-port) lipo for linux
