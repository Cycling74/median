## Dependencies

* [rust](https://rustup.rs/)
* [cargo make](https://github.com/sagiegurari/cargo-make)
  * `cargo install cargo-make`

## Examples

checkout the examples in **median/examples**
You can build/package/install them with:

`cargo make build`
`cargo make package`
`cargo make install`

## Resources
- [writing docs in rust](https://facility9.com/2016/05/writing-documentation-in-rust/)

josuha
```
if you have more than 7 args or if you are mixing floating point and other argument types, use A_GIMME
```

# TODO

* m1
* cross compile
  * osxcross
  * mingw
  * ```
    rustup target add x86_64-pc-windows-gnu
    rustup toolchain install stable-x86_64-pc-windows-gnu
    cargo build --target x86_64-pc-windows-gnu

    rustup target add aarch64-apple-darwin

    ```
  * [cctools](https://github.com/tpoechtrager/cctools-port) lipo for linux
  * `lipo -create -output libsimp.dylib target/aarch64-apple-darwin/debug/libsimp.dylib target/debug/libsimp.dylib`
