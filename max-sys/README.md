# max-sys

Automatically generated [Rust](https://www.rust-lang.org/) bindings for the [Max SDK](https://github.com/Cycling74/max-sdk).

## Regenerating the bindings
 
You'll need:

* [clang](https://clang.llvm.org/)
  * on Windows I used `scoop` to install clang and then set the enviroment variable: `LIBCLANG_PATH` to `C:\Users\xnor\scoop\apps\llvm\current\bin`
  * on Mac I think I just had it installed by default, or maybe it came with Xcode

Since the SDK is rather large and doesn't change, we include the generated
bindings in the repository.  If you make some changes to `build.rs`, the
headers: `wrapper.h` or `wrapper-max.h` or the SDK, you can rebuild the
bindings (for the OS/platform you're on) with:

```sh
cargo build --features=build-bindings
```

Then you can commit the result back to the repo if it is appropriate.

## TODO

* expose the JitterAPI
* post on https://cycling74.com/forums/c-api-via-llvm

