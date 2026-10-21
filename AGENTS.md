# Working in this repository

Guidance for coding agents (and people) working on `median`, the Rust bindings for the
Max/MSP/Jitter SDK. Everything referenced here is inside this repository; do not assume access
to any other checkout.

## Layout

| path | what it is |
|---|---|
| `max-sys/` | Low-level FFI crate. Pre-generated bindgen output per target in `max-sys/src/ffi-*.rs`, selected by `cfg` in `max-sys/src/lib.rs`. The Max SDK headers are the git submodule at `max-sys/thirdparty/max-sdk` (`source/c74support/{max,msp,jit}-includes`). Regeneration instructions are in `max-sys/README.md`; the generated files are committed, so regenerate only when `build.rs`, the `wrapper*.h` files or the SDK change, and regenerate every target together. |
| `median/` | The safe(r) wrapper crate: `src/wrapper.rs` (object wrappers and trampolines), `builder.rs` (inlets/outlets/clocks at construction), `class.rs`/`method.rs` (class registration; `build.rs` generates the typed selector permutations into `OUT_DIR`), `attr.rs`, `atom.rs`, `symbol.rs`, `outlet.rs`, `inlet.rs`, `clock.rs`, `buffer.rs`, `notify.rs`, `thread.rs`, `file.rs`, `num.rs`, `slice.rs`, `alloc.rs`. |
| `median_macros/` | Proc macros re-exported by `median`: `external!`, `external_no_main!`, `ext_main!`, and the `*_tramp` attribute macros. |
| `median/examples/` | `simp` (Max object), `hello_dsp` (MSP), `multi` (two classes in one external). Each is a `cdylib` built with `cargo make` using `median/utils/Makefile.toml`. |
| `templates/` | `cargo generate` template for a new external. Note `templates/max/Makefile.toml` downloads `median/utils/Makefile.toml` from the `develop` branch at build time, so changes to that file affect template users immediately. |
| `doc/` | Design notes: the October 2026 review of the crate and the plans that follow from it. Start with `doc/README.md`. |

There is no Cargo workspace. Run cargo commands inside `median/`, `max-sys/` or `median_macros/`.

## Building and testing

```sh
cd median && cargo build && cargo test        # unit + doc tests, no Max needed
cd median && cargo clippy                      # see "Known state" below
cd median/examples/simp && cargo make build    # needs cargo-make; package/install also exist
```

Externals are `cdylib`s linked with `-undefined dynamic_lookup` (see the `.cargo/config*` files
next to each example and the template); Max symbols resolve at load time. Nothing in this repo
can run an external outside Max, so behaviour in Max has to be checked by hand or stated as
untested. `median/src/test/` contains `#[no_mangle]` stubs for a few SDK functions so unit tests
can link; they are not a Max mock.

## Known state (read before changing wrapper code)

`doc/review-2026-10.md` lists soundness defects in `median/src` with `file:line` references,
graded H/M/L. Many are reachable from safe code. The important ones for anyone editing the core:

- `perform64`/`dsp64` take `&mut self` while other threads hold `&self` (H1).
- The blanket `MaxObj for T` recovers the object pointer by offset arithmetic from `&self`; it is
  only valid inside a wrapper (H3). Do not add more impls of this shape.
- `num::Float64`/`Int64` are non-atomic cells declared `Sync` (H4); use real atomics in new code.
- Many `pub fn`s dereference raw pointers but are not `unsafe` (H9; this is what clippy's
  `not_unsafe_ptr_arg_deref` errors are).
- buffer~ data is interleaved, not planar (M1).
- Console output passes the message as a printf format string (H11); use `"%s"`.
- Attribute accessor trampolines return `()` where the SDK expects `t_max_err` (H12).

`doc/plan-soundness-fixes.md` is the ordered list of fixes that keep the public API;
`doc/plan-api-redesign.md` is the breaking redesign (derive + `#[methods]` instead of
`external!`, an explicit object handle, iolet descriptors, fallible `new`);
`doc/plan-containers.md` covers dictionaries; `doc/plan-branches-and-build.md` says what to do
with each open branch. If a change touches an area covered by a plan, follow the plan or update
it in the same change.

## Conventions

- Edition 2018 today; moving to 2024 is planned. Prefer `c"..."` literals over
  `CString::new(..).unwrap()` and `core::ffi::c_*` types in new code.
- Every `extern "C"` function Max can call is a trampoline. Since Rust 1.81 a panic escaping one
  aborts Max, and the template's release profile uses `panic = "abort"`, so a panic anywhere in an
  external kills the host. Do not add `assert!`/`unwrap`/`expect` on paths Max calls (`new`,
  message methods, `perform`, `notify`, `assist`, attribute accessors). Return an error code or
  post with `object_error`.
- Anything that dereferences a caller-supplied raw pointer is `unsafe fn` with a `# Safety`
  section. Wrapping the body in `unsafe { }` does not discharge the contract.
- Max calls message methods on the main and scheduler threads concurrently with `perform` on the
  audio thread. Wrapped objects are `Sync` and all methods take `&self`; state needs atomics or
  locks, and the audio path must not lock or allocate.
- Follow the SDK headers in `max-sys/thirdparty/max-sdk` for signatures and ownership rules; where
  the SDK's examples and its headers disagree, the headers win (there are known cases, listed in
  `doc/plan-containers.md`). Use `c_long`, not `i64`, for SDK `long` parameters (32-bit on Windows).
- `AttrBuilder` with an offset and no accessors is currently broken (M9); pass accessors or fix
  the builder first.

## Branches

`develop` is the integration branch; `main` is for PRs/releases. Long-lived branches
(`feature/jitter`, `feature/dict`, `feature/dict-test`, `feature/maxcore-test`, `xnor/linux`,
`xnor/inletinfo`) are assessed in `doc/plan-branches-and-build.md`. Do not merge them as-is;
each has a stated path.

## Do not

- Point documentation or code at paths outside this repository.
- Commit regenerated bindings for one target only.
- Change `median/utils/Makefile.toml` without considering template users (see Layout).
- Add new offset-based pointer recovery, new non-atomic `Sync` cells, or new safe fns over raw
  pointers; these are the defect classes the review is removing.
