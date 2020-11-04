## Dependencies

* [rust](https://rustup.rs/)
* [cargo make](https://github.com/sagiegurari/cargo-make)
  * `cargo install cargo-make`
* [clang](https://clang.llvm.org/)
  * on Windows I used `scoop` to install clang and then set the enviroment variable: `LIBCLANG_PATH` to `C:\Users\xnor\scoop\apps\llvm\current\bin`

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

* osxcross
* mingw

