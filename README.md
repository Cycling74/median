# median/max-sys

Rust bindings for [Max/MSP/Jitter](https://cycling74.com/products/max).

Look into the subdirs for more information. Most likely, you want [median/README.md](median).

# Projects

* [max-sys](max-sys/README.md) - Low level automatically generated bindings for the [Max SDK](https://github.com/Cycling74/max-sdk).
* [median](median/README.md) - Higher level Rust wrappers around `max-sys` and utility methods/macros.
* **median_macros** - Procedural macros for creating externals, trampolines, etc. These are re-exported by `median` so there is no need to depend on this directly.

