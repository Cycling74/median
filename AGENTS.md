# Working in this repository

Guidance for coding agents (and people) working on `median`, the Rust bindings for the
Max/MSP/Jitter SDK. Everything referenced here is inside this repository; do not assume access
to any other checkout.

## Layout

| path | what it is |
|---|---|
| `max-sys/` | Low-level FFI crate. Pre-generated bindgen output per target in `max-sys/src/ffi-*.rs`, selected by `cfg` in `max-sys/src/lib.rs`. The Max SDK headers are the git submodule at `max-sys/thirdparty/max-sdk` (`source/c74support/{max,msp,jit}-includes`). Regeneration instructions are in `max-sys/README.md`; the generated files are committed, so regenerate only when `build.rs`, the `wrapper*.h` files or the SDK change, and regenerate every target together. |
| `median/` | The safe(r) wrapper crate: `src/wrapper.rs` (object wrappers and trampolines), `builder.rs` (inlets/outlets/clocks at construction), `class.rs`/`method.rs` (class registration: `Class::add_*` take a zero-sized callable and build the trampoline generically; no build script), `attr.rs` (also the generic attribute accessors, `AttrField`, `AttrOptions`), `lib.rs` (crate docs, `ext_main!`, `prelude`), `atom.rs`, `symbol.rs`, `outlet.rs`, `inlet.rs`, `clock.rs`, `buffer.rs`, `notify.rs`, `thread.rs`, `file.rs`, `num.rs`, `slice.rs`, `alloc.rs`. |
| `median_macros/` | Proc macros re-exported by `median`: `#[derive(MaxObject)]` (`derive.rs`: `ObjClass`, `register()`, `FieldAttrs`), `#[median::methods]` (`methods.rs`: re-emits the impl without markers, implements `Methods` with `Class::add_*` calls), shared `#[attr(..)]` options (`attr_opts.rs`), and `wrapped_defer_tramp` (re-exported as `median::wrapper::defer_tramp`). The generated code calls only public `median` API; `ext_main!` is a `macro_rules!` in `median`. |
| `median/examples/` | `simp` (Max object, derive + `#[median::methods]`), `simp_explicit` (the same without the macros: the reference for the hand-written style), `hello_dsp` (MSP), `multi` (two classes in one external). Each is a `cdylib` built with `cargo make` using `median/utils/Makefile.toml`; `median/tests/` also compiles each into a test (`#[path]` modules) and runs it against `median-mock`. |
| `templates/` | `cargo generate` template for a new external. Note `templates/max/Makefile.toml` downloads `median/utils/Makefile.toml` from the `develop` branch at build time, so changes to that file affect template users immediately. |
| `doc/` | Design notes: the October 2026 review of the crate and the plans that follow from it. Start with `doc/README.md`. |

There is no Cargo workspace. Run cargo commands inside `median/`, `max-sys/` or `median_macros/`.

## Building and testing

```sh
cd median && cargo build && cargo test        # unit, end-to-end (median-mock), doc and UI tests, no Max needed
cd median && cargo clippy                      # see "Known state" below
cd median/examples/simp && cargo make build    # needs cargo-make; package/install also exist
```

`median/tests/ui.rs` runs the `tests/ui/*.rs` compile-fail cases of the macros with `trybuild`
(it caps trybuild's cargo at half the cores and builds without a rustc wrapper); after changing a
macro error, regenerate the expected output with `TRYBUILD=overwrite cargo test --test ui` and
review the `.stderr` diff.

Externals are `cdylib`s linked with `-undefined dynamic_lookup` (see the `.cargo/config*` files
next to each example and the template); Max symbols resolve at load time. Nothing in this repo
can run an external outside Max, so behaviour in Max has to be checked by hand or stated as
untested. The unit tests, doc tests and `median/tests/` link `median-mock/`, a Rust implementation
of the subset of the Max C API that median calls (dev-dependency; `use median_mock as _;` makes the
lib tests link it). It creates objects through their `new` method, dispatches messages, records
outlet and console output and drives clocks and `dsp64` deterministically. Every test that touches
it takes `median_mock::lock()` for its whole body (tests run concurrently otherwise). There are no
`#[no_mangle]` stubs of SDK functions in median: add a missing function to `median-mock`, with a
test there. `median/tests/e2e.rs` shows an external tested end to end.

## Known state (read before changing wrapper code)

`doc/review-2026-10.md` lists soundness defects in `median/src` with `file:line` references,
graded H/M/L. Step 1 of the fixes (`doc/plan-soundness-fixes.md`, see its "Status" section) is
done: `perform64`/`dsp64` take `&self`, `num` types are real atomics, raw-pointer fns are `unsafe`
(clippy's `not_unsafe_ptr_arg_deref` is `deny` in `median/Cargo.toml`), console output uses
`"%s"`, attribute accessors return `t_max_err`, clocks are shut out before their owner is dropped,
and every trampoline catches panics. Redesign step 1 (`doc/plan-api-redesign.md`) fixed H3 and
the `object_post!`/`object_error!` hole: the offset-based blanket `MaxObj`/`MSPObj for T` impls and
the `field-offset` dependency are gone; an object keeps an `ObjRef` (from `builder.obj()`) and
posts/touches attributes/defers through it, and the macros take `&ObjRef`. `MaxObj`/`MSPObj` are
only implemented for the wrapper types. Do not add impls of the old shape. Documented limit:
`ObjRef` is `!Clone`, `!Send`, but safe code can still deliberately leak it (`Box::leak`,
`thread_local!`) and use it after the object is deleted. Redesign step 4 replaced the boxed
outlet aliases with `Outlet<K>`, which has the same shape and the same documented limit (M10 is
fixed modulo deliberate leaking). The iolet table is filled once at construction; `z_dsp_setup`
runs before the other inlets are made (unverified in Max for objects with 2+ signal inlets and
other inlets, see the plan's step 4 status). Step 5 made `new` return `Result<Self, NewError>`;
a refused or panicking `new` releases what the builder made, and an MSP object's `z_dsp_setup`
is always paired with `z_dsp_free` (the builder owes it until construction finishes). Still
open, and reachable from safe code:

- `SymbolRef::assign` (H5) and `BufferLocked`'s `&mut [f32]` over a shared lock (H7).
- buffer~ data is interleaved, not planar (M1).
- A clock callback must not free its own owner or drop its own `ClockHandle` (it deadlocks); see
  `ClockShared` in `median/src/clock.rs`.

`doc/plan-soundness-fixes.md` is the ordered list of fixes that keep the public API;
`doc/plan-api-redesign.md` is the breaking redesign (derive + `#[methods]` instead of
`external!`, an explicit object handle, iolet descriptors, fallible `new`);
`doc/plan-containers.md` covers dictionaries; `doc/plan-branches-and-build.md` says what to do
with each open branch. If a change touches an area covered by a plan, follow the plan or update
it in the same change.

## Conventions

- `median`, `median_macros`, the examples and the template are edition 2024 (`rust-version`
  1.85). Use `c"..."` literals for fixed C strings, not `CString::new(..).unwrap()`, and
  `core::ffi::c_*` types in new code. Code the macros generate must not depend on the user's
  edition (no `c"..."` in it: names go through `median::__private::cstr`).
- An external is `#[derive(MaxObject)]` on the struct, `impl MaxObjWrapped` (or `MSPObjWrapped`),
  `#[median::methods]` on an inherent impl, and `median::ext_main! { Type, .. }`. The macros only
  emit calls to public API (`ObjClass`, `FieldAttrs`, `Methods`, `Class::add_*`,
  `Class::add_field_attr`/`add_attr`); anything they can do must stay writable by hand, as
  `simp_explicit` shows. Registration order is field attributes, `Methods::register`, then
  `class_setup`.
- Every `extern "C"` function Max can call is a trampoline. A panic escaping one aborts Max, so
  each trampoline (hand-written in `wrapper.rs`, generic over a zero-sized callable in
  `method.rs`/`attr.rs`, or generated by `defer_tramp`) runs the user's code
  inside a `median::ffi::guard*` helper, which catches the panic, posts it to the Max console and
  returns a neutral value (null from `new`, `MAX_ERR_GENERIC` from attribute accessors, silence
  from `perform`). That relies on `panic = "unwind"` (the template and examples set it). New
  trampolines must use the guards, and prefer returning an error code or posting with
  `object_error` over `assert!`/`unwrap`/`expect` on paths Max calls.
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
