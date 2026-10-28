# median/max-sys

Rust bindings for [Max/MSP/Jitter](https://cycling74.com/products/max).

Look into the subdirs for more information. Most likely, you want [median/README.md](median).

# Projects

* [max-sys](max-sys/README.md) - Low level automatically generated bindings for the [Max SDK](https://github.com/Cycling74/max-sdk).
* [median](median/README.md) - Higher level Rust wrappers around `max-sys` and utility methods/macros.
* **median_macros** - Procedural macros (`#[derive(MaxObj)]`, `#[derive(MSPObj)]`, `#[derive(JitMatrixObj)]`, `#[derive(JitGlObj)]`, `#[median::methods]`). These are re-exported by `median` so there is no need to depend on this directly.
* [median project templates](templates/) - `cargo generate` compatible template(s) for starting your own max external project.


## Development

The root workspace builds `median`, `max-sys`, `median_macros`, and the `median-mock` test
kernel. Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.
Examples are standalone projects with their own linker and packaging settings.

The 0.3 redesign is in progress. Start with [doc/README.md](doc/README.md) and the
[current handoff](doc/handoff-2026-10-23.md) for completed work and remaining validation.
