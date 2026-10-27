# Working in this repository

Guidance for coding agents (and people) working on `median`, the Rust bindings for the
Max/MSP/Jitter SDK. Everything referenced here is inside this repository; do not assume access
to any other checkout.

## Layout

| path | what it is |
|---|---|
| `max-sys/` | Low-level FFI crate. Pre-generated bindgen output per target in `max-sys/src/ffi-*.rs`, selected by `cfg` in `max-sys/src/lib.rs`. The Max SDK headers are the git submodule at `max-sys/thirdparty/max-sdk-base` (`c74support/{max,msp,jit}-includes`); the older `max-sdk` submodule supplies examples. Regeneration instructions are in `max-sys/README.md`; the generated files are committed, so regenerate only when `build.rs`, the `wrapper*.h` files or the SDK change, and regenerate every target together. |
| `median/` | The safe(r) wrapper crate: `src/wrapper.rs` (object wrappers and trampolines), `builder.rs` (inlets/outlets/clocks/queues at construction), `class.rs`/`method.rs` (class registration: `Class::add_*` take a zero-sized callable and build the trampoline generically; no build script), `attr.rs` (also the generic attribute accessors, `AttrField`, `AttrOptions`), `lib.rs` (crate docs, `ext_main!`, `prelude`), `atom.rs`, `symbol.rs`, `outlet.rs`, `inlet.rs`, `clock.rs` (`Clock`, also the main-thread `clock_deferred` variant, and `ClockShared`, the callback target clocks, queues and deferred calls share), `queue.rs` (`Queue`, a qelem), `defer.rs` (`#[defer]`/`#[defer_low]`: the per-object deferred-call queue), `fifo.rs` (re-exported `rtrb` ring buffers, `Shared`), `gimmeback.rs` (`Reply`/`GimmebackResult`, what an `A_GIMMEBACK` method returns; registered with `Class::add_sel_gimmeback`, design in `doc/plan-gimmeback.md`), `buffer.rs`, `notify.rs`, `thread.rs`, `file.rs`, `num.rs`, `slice.rs`, `alloc.rs`. |
| `median_macros/` | Proc macros re-exported by `median`: `#[derive(MaxObject)]` (`derive.rs`: `ObjClass`, `register()`, `FieldAttrs`), `#[median::methods]` (`methods.rs`: re-emits the impl without markers, implements `Methods` with `Class::add_*` calls; markers `#[bang]` .. `#[sel]`, `#[sel_gimme]`, `#[sel_gimmeback]`, `#[attr]`, `#[dblclick]`, `#[loadbang]`, `#[notify]`, `#[dictionary]`, `#[savestate]`, each message marker except `#[sel_gimmeback]` and the last three optionally with `#[defer]`/`#[defer_low]`), shared `#[attr(..)]` options (`attr_opts.rs`), and the reserved-selector mirror (`reserved.rs`). The generated code calls only public `median` API; `ext_main!` is a `macro_rules!` in `median`. |
| `median/examples/` | `simplemax` (SDK constructor/console example), `simp` (Max object, derive + `#[median::methods]`), `simp_explicit` (the same without the macros: the reference for the hand-written style), `hello_dsp` (MSP), `multi` (two classes in one external), `validation` (five diagnostic classes and four Max patches; `cargo make package-fixtures`). Each is a `cdylib` built with `cargo make` using `median/utils/Makefile.toml`; `median/tests/` also compiles each into a test (`#[path]` modules) and runs it against `median-mock`. |
| `templates/` | `cargo generate` template for a new external. The packaging tasks are vendored in `templates/max/Makefile.package.toml`; keep that copy and `PkgInfo` synchronized with `median/utils/` when changing the pipeline. |
| `median/src/dict.rs` | Retained dictionaries and owned atomarrays; `dictionary`/`savestate` callbacks are in `method.rs`. See `doc/plan-containers.md` for ownership contracts and unfinished helpers. |
| `median/src/jit/` | Shared matrix/GL class wrappers (`wrapper.rs`), MOP configuration and borrowed matrix views/iterators/parallel adapters (`matrix/`). Examples: `jit_scalebias`, `jit_gl_simple`. |
| `doc/` | Design notes: the October 2026 review of the crate and the plans that follow from it. Start with `doc/README.md`. |

The root Cargo workspace contains `median`, `max-sys`, `median_macros`, and `median-mock`. Examples and the template are standalone workspaces for their own linker flags and profiles.

## Building and testing

```sh
cargo build --workspace && cargo test --workspace        # unit, end-to-end (median-mock), doc and UI tests, no Max needed
cargo clippy --workspace --all-targets -- -D warnings
cd median/examples/simp && cargo make build    # needs cargo-make; package/install also exist
```

`median/tests/ui.rs` runs the `tests/ui/*.rs` compile-fail cases of the macros with `trybuild`
(it caps trybuild's cargo at half the cores and builds without a rustc wrapper; `tests/ui_pass/`
holds a pass case so the suite builds rather than checks, which is what makes the
post-monomorphisation `const` asserts fire; expectations are from current stable); after changing a
macro error, regenerate the expected output with `TRYBUILD=overwrite cargo test --test ui` and
review the `.stderr` diff.

Formatting: run `script/install-hooks.sh` once per clone. The pre-commit hook
(`script/pre-commit.sh`) runs `rustfmt` on staged `.rs` files, re-staging fully staged ones and
aborting if a partly staged file needs formatting. `rustfmt.toml` pins edition 2024, so single-file `rustfmt` agrees with `cargo fmt`. The generated bindings
(`max-sys/src/ffi-*.rs`) and the trybuild cases (`median/tests/ui/*.rs`, whose `.stderr` files
record line numbers) are left alone. To format everything, run `cargo fmt --all` and
`rustfmt templates/max/src/lib.rs`. A commit that only reformats goes in `.git-blame-ignore-revs`
(the install script sets `blame.ignoreRevsFile`).

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
posts/touches attributes through it, and the macros take `&ObjRef`. `MaxObj`/`MSPObj` are
only implemented for the wrapper types. Do not add impls of the old shape. Documented limit:
`ObjRef` is `!Clone`, `!Send`, but safe code can still deliberately leak it (`Box::leak`,
`thread_local!`) and use it after the object is deleted. Redesign step 4 replaced the boxed
outlet aliases with `Outlet<K>`, which has the same shape and the same documented limit (M10 is
fixed modulo deliberate leaking). The iolet table is filled once at construction; the non-signal inlets
are made first (right to left) and `z_dsp_setup` last, which puts the signal inlets leftmost
whether or not Max keeps `z_dsp_setup`'s inlets next to the leftmost (the mock does not: it
inserts every inlet just right of the leftmost, as `intin`'s documentation says). Step 5 made `new` return `Result<Self, NewError>`;
a refused or panicking `new` releases what the builder made, and an MSP object's `z_dsp_setup`
is always paired with `z_dsp_free` (the builder owes it until construction finishes). Step 6
fixed H5, H7 and M1: `SymbolRef` is a `Copy` handle over `NonNull<t_symbol>` with no interior
mutability (`median::sym!("..")` for literals); `BufferLocked` (the shared
`buffer_locksamples` claim) hands out `&[AtomicSample]`, indexed interleaved (frame major), and
`BufferEdit` (`buffer_edit_begin/end`, refused on the audio and scheduler threads) is the only
`&mut [f32]`. On 2026-10-26 patch 04 observed `buffer_locksamples` returning null during an
edit on the main and audio threads in Max 9.2 ARM64, and `buffer_getinfo`'s `b_samples` matching
shared-claim pointers before/after, with Overdrive on and off. This read-only probe supports
the exclusion and identity assumptions; actual sample editing, Scheduler in Audio Interrupt
enabled and other host versions remain
untested (see `doc/validation-max.md`). median allows one `BufferEdit` per `buffer~`
at a time (`EditError::Busy`), so its own edits never nest. Still open, and reachable from safe code:

- A clock or queue callback, or a deferred method (it runs in its object's queue callback), must
  not free its own owner or drop its own `Clock`/`Queue`: `ClockShared::clear` then posts an
  error for the owner and parks that thread forever (deliberately, instead of a use-after-free);
  see `ClockShared` in `median/src/clock.rs`. A callback that re-enters on the same thread (a nested event loop, such
  as a modal dialog's, running the same clock) runs nested: the exclusion is a
  `ReentrantMutex` and nothing else is held across user code. Two clock issues found in step 1 remain open (a callback Max has
  dispatched but that has not taken the `ClockShared` lock yet can read the freed clock target;
  a possible deadlock between that lock and a Max lock): see the Status section of
  `doc/plan-soundness-fixes.md`. Step 7 and its review fixes did not change them.
- Deferred methods (`#[defer]`, `#[defer_low]`, `Class::add_*_deferred`) are not Max's
  `defer`/`defer_low`: each object of a class with deferred methods has its own queue of pending
  calls (arguments copied) and two qelems, owned by the wrapper, disabled and freed with the
  object, so a pending call can't outlive it. Two because a qelem is unset only after its
  function returns (`ext_proto.h`, `qelem_set`), so setting the one that is draining does
  nothing; every push sets both. The same rule makes `Queue::set` from inside its own callback a
  no-op (documented, not worked around), and `median-mock` models it. Queuing allocates and
  locks: a deferred message must not be sent from `perform`. Whether `qelem_set` (`Queue::set`)
  is real-time safe is not in the SDK headers; calling it from `perform` (the `median::fifo`
  pattern) is unverified. So is what
  `qelem_front` (`Queue::front`, `#[defer]` from another thread) does to a qelem that is already
  set: median and the mock assume it stays where it is.

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
  `method.rs`/`attr.rs`, the deferring ones in `method.rs`, clock and queue callbacks) runs the user's code
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
- `AttrBuilder` now passes absent offset accessors as null so the SDK installs them (M9 fixed).
  Built `Attr` values own their allocation until added to a class or converted to a raw pointer;
  the wrapper's `MaxObj::IS_JITTER` selects the Jitter constructor and getter allocator.

## Branches

`develop` is the integration branch; `main` is for PRs/releases. Long-lived branches
(`feature/jitter`, `feature/dict`, `feature/dict-test`, `feature/maxcore-test`, `xnor/linux`,
`xnor/inletinfo`) are assessed in `doc/plan-branches-and-build.md`. Do not merge them as-is;
each has a stated path. `xnor/api-redesign` holds the 0.3 redesign (local, not yet pushed); its
state and what is left are under "Where things stand" in `doc/plan-api-redesign.md`. GitHub has
only `develop` (no `main`), so check the PR target before opening one.

## Do not

- Point documentation or code at paths outside this repository.
- Commit regenerated bindings for one target only.
- Change `median/utils/Makefile.toml` without considering template users (see Layout).
- Add new offset-based pointer recovery, new non-atomic `Sync` cells, or new safe fns over raw
  pointers; these are the defect classes the review is removing.

## Continuation checkpoint (2026-10-23)

Read `doc/handoff-2026-10-23.md` before resuming. The 0.2.x clock backport remains deferred.
Jitter derives support `kind = matrix_op|gl`; callbacks use `&self`, per-call borrowed matrices
and `&DrawContext` for GL. Preserve SDK-owned notification/inlet handlers in Max glue. General
methods/deferred calls live on the Max forwarding class; direct Jitter `class_setup` methods
cannot defer. `JitBuilder<Self>` adds custom numeric/proxy inlets and message outlets after
SDK-owned positions for fixed-count MOPs and GL objects; variable-count layouts reject extras.
Preserve SDK inlet-info callback arguments when delegating for SDK positions. GL chunks are
callback-scoped; buffer/geometry adoption and upload remain unsafe pending host ownership
verification. The Max runs recorded in `doc/validation-max.md` (patches 01-04, 2026-10-26)
are the only host checks so far; mock success does not verify host locking, iolet ordering or
GL ownership contracts. The 2026-10-27 review of this work and its fixes are recorded under
"Where things stand" in `doc/plan-api-redesign.md`.
