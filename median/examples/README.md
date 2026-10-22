# Median Examples

There are a few examples here:

* `simp` - is a basic Max object with a few methods and attributes, written with `#[derive(MaxObject)]` and `#[median::methods]`.
* `simp_explicit` - is `simp` written without those macros: the trait impls they generate, written out. Read it to see what the macros do, or to register things by hand.
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
