# Median Examples

There are a few examples here:

* `simp` - is a basic Max object with a few methods and attributes.
* `hello_dsp` - is a basic MSP object.
* `multi` - is a multi object external that contains both a MSP and Max object and an init file that tells Max where to find them.

## Dependencies

* [cargo make](https://github.com/sagiegurari/cargo-make)
  * `cargo install cargo-make`

## Building

You can build/package/install them going into the subdir and:

```
cargo make build
cargo make package
cargo make install
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

`cargo make package-all` or `cargo make package-all --profile release`

If this succeeds, you should see a printout of where the externals were put.

