# max-sys

Rust FFI bindings for the Max/MSP/Jitter SDK. Generated files are committed; ordinary builds
do not need libclang or bindgen. Supported targets: macOS ARM64/x64 and Windows x64.

## SDK and generated bindings

All three targets use `max-sys/thirdparty/max-sdk-base`, pinned to
`c03a2922a2a8ff149165a1e2ea134e9321d6e202`, with bindgen **0.73.2**, Rust target **1.85**,
`generate_cstr` and `wrap_unsafe_ops`. Max, MSP and Jitter declarations are generated together.
The `SdkComments` bindgen callback removes leading comment whitespace and converts Doxygen
`@code`/`@endcode` blocks into fenced C examples, escaping literal brackets in prose.
This keeps SDK prose and examples readable in rustdoc without interpreting them as Rust code
or array subscripts as Rust documentation links.
The old `max-sys/thirdparty/max-sdk` submodule is retained for the examples and design notes;
it is not used to generate or link these bindings.

Windows ARM64 is not enabled: the pinned SDK only supplies `x64` import libraries. Linux is
not enabled until a host and its ABI are specified. Windows builds link MaxAPI, MaxAudio and
jitlib from the pinned SDK; unused imports are discarded by the linker.

## Regenerating

Run on macOS with Xcode/Command Line Tools, libclang and MinGW-w64 x86_64 headers installed:

```sh
git submodule update --init --recursive
python3 script/regenerate-bindings.py --windows-include /path/to/x86_64-w64-mingw32/include
```

Run this command from the repository root. The include directory must contain `windows.h`.
`xcrun --show-sdk-path` locates the macOS SDK. Set `LIBCLANG_PATH` if libclang cannot be found.
The checked-in files were generated with the macOS 14.5 SDK and MinGW-w64 13.0.0 headers.
The Windows Clang target is `x86_64-pc-windows-gnu` with MS extensions enabled (the SDK uses
backslashes in include paths); SDK C types have the Windows x64 ABI on both GNU and MSVC
targets. Running Windows tests remains a native CI/manual check.

The script stages all three files and replaces the tracked set only after all generators
succeed. Review and commit all three in the same change. Regenerate when changing `build.rs`,
`wrapper*.h` or the SDK, and record the new SDK commit/version here. `cargo build --features
build-bindings` generates just the current target in Cargo's `OUT_DIR`; it does not update a
tracked binding. `MAX_SYS_BINDINGS_TARGET` and `MAX_SYS_BINDINGS_DIR` are the script's overrides.

## Container API dependency

The container plan depends on `object_retain`, which the SDK marks private but includes in its
export list. Keep retain operations behind one internal helper; see `doc/plan-containers.md`
for the ownership contract before implementing wrappers.
