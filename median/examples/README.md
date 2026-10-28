# Median Examples

There are a few examples here:

* `simplemax` - ports the SDK's minimal Max object: reports creation arguments and describes its inlet. Start here for an object without state or messages.
* `simp` - is a basic Max object with a few methods and attributes, written with `#[derive(MaxObj)]` and `#[median::methods]`.
* `simp_explicit` - is `simp` written without those macros: the trait impls they generate, written out. Read it to see what the macros do, or to register things by hand.
* `hello_dsp` - is a basic MSP object.
* `multi` - is a multi object external that contains both a MSP and Max object and an init file that tells Max where to find them.

* `jit_scalebias` - a 4-plane char matrix operator, registered as `jit.median.scalebias`; demonstrates `#[derive(JitMatrixObj)]`, MOP configuration, atomic attributes, and explicit matrix access contracts.
* `jit_gl_simple` - the SDK quad, registered as `jit.gl.median.simple`; demonstrates `#[derive(JitGlObj)]` and GL callback contexts. Requires a Jitter render destination.

The Jitter examples have mock coverage and build/package checks; validation in Max is pending
([checklist](../../doc/validation-max.md)).

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
