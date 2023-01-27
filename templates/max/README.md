# {{project-name}}

This is a simple Max external with a few methods and attributes.

## Build with cargo-make

To build this external, make sure you have [cargo-make](https://sagiegurari.github.io/cargo-make/)
installed on your system:

```sh
cargo install cargo-make
```

You can then build the package for your current platform:

```sh
cargo make package # creates a development build
cargo make --profile release package # creates a production build
```

If you'd like to install the built object into Max, run:

```sh
cargo make install # creates and installs a development build
cargo make --profile release install # creates and installs a production build
```

## Cross Compiling

Currently this is only enabled for macOS, but with a little bit of work we should be able to cross compile on Linux.
We can build fat (x86 and arm64) macOS and Windows externals.

Assuming you're on x86 macOS, install the Windows and arm64 targets/toolchains:

```sh
rustup target add x86_64-pc-windows-gnu
rustup target add aarch64-apple-darwin
rustup toolchain install stable-x86_64-pc-windows-gnu
```

Or, _untested_, on an aarch64 (arm64) macOS machine:

```sh
rustup target add x86_64-pc-windows-gnu
rustup target add x86_64-apple-darwin
rustup toolchain install stable-x86_64-pc-windows-gnu
```

Then should be able to build all the externals with:

```sh
cargo make package-all # creates a development build
cargo make --profile release package-all # creates a production build
```

Or build and install them directly:

```sh
cargo make install-all # creates and installs a development build
cargo make --profile release install-all # creates and installs a production build
```

If this succeeds, you should see a printout of where the externals were put.
