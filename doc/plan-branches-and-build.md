# Plan, steps 3 to 5: branches, build pipeline, bindings, tests, dependencies

Branch states as of 2026-10-21 (merge-base and conflict status from `git merge-tree --write-tree`).
Details and file:line pointers are in [review-2026-10.md](review-2026-10.md) §4.

## Continuation status (2026-10-23)

Work in progress on `xnor/api-redesign`; the 0.2.x clock backport is deferred by request.

- SDK/bindings: all three supported targets regenerated together with bindgen 0.73.2
  against max-sdk-base `c03a2922a2a8ff149165a1e2ea134e9321d6e202`.
  `script/regenerate-bindings.py` stages the complete set before replacing tracked files.
  Windows ARM64 remains deferred: this SDK supplies only x64 Windows import libraries.
  Linux remains excluded until its intended host is specified.
- Documentation: bindgen normalizes SDK comment indentation, fences Doxygen examples as C,
  escapes literal brackets in prose, formats `#symbol` references as code, and removes
  Doxygen group metadata and trailing-field comment markers. All three binding targets were
  regenerated with documentation-only changes. CI builds workspace documentation with rustdoc
  warnings denied.
- Jitter: unflagged matrix metadata, guarded matrix views, validated shared/mutable iterators,
  and calc2/calc3 parallel adapters are implemented. Jitter `long` is i32; mutable views require
  an explicit unsafe exclusivity contract because a Jitter lock alone does not prove it.
  Unit tests cover strides, disjoint mutation, metadata rejection, callback errors/panics;
  compile-fail tests protect mutable iterator borrowing and the GL callback context.
  `MatrixOp`/`Ob3d`, `kind = matrix_op|gl`, shared attribute accessors, MOP configuration,
  Max forwarding and owner-bound deferred calls are implemented. The SDK's notification and
  inlet handlers are preserved. Both Jitter examples pass mock tests and package on macOS ARM64.
  Callback-scoped chunks and unsafe buffer/geometry adoption now have ownership tests.
  Fixed-count MOPs and ob3d now support custom inlets/outlets with SDK position accounting,
  guarded callbacks and delegated assistance/inlet-info (2026-10-24). Variable-count MOPs
  reject extra iolets pending a dynamic-layout design. Still open: safe buffer/geometry
  construction and uploads after ownership verification, and validation in Max.
  See API redesign §J for implementation decisions.
- Examples: the SDK simplemax port and constructor/console mock coverage are implemented.
  `validation` adds message/thread, mixed MSP inlet, and custom MOP/GL iolet probes with
  three Max patches and a complete diagnostic package. Its exact source passes mock tests;
  Apple Silicon packaging succeeds. CI builds/packages it on the existing platform matrix.
  Host checks are tracked in [validation-max.md](validation-max.md).
- Build: root workspace, standalone external projects, CI matrix (macos-15,
  macos-15-intel, windows-2025), and vendored template packaging tasks are implemented.
  Native Windows packaging and the workflow still need execution on those runners.
- Dependencies: edition 2024 throughout, LazyLock and explicit trampoline identifiers replace
  lazy_static/paste; parking_lot 0.12.5, syn 3.0.9. Workspace tests and strict Clippy passed
  locally. max-sys disables doctests because its generated SDK docs contain C examples;
  median's Rust doctests stay enabled. Intel macOS and Windows GNU workspace/all-targets
  cross-checks pass; those are compilation checks, not native execution. The declared Rust 1.85
  minimum still needs a run with that exact toolchain.
- Containers: core ownership/locking, registry, atomarray builders and dictionary/save message
  support are implemented with mock tests; see the detailed status in `plan-containers.md`.
  JSON/atom parsing, file operations, name switching, notifications and raw helpers are now
  implemented. In-memory JSON output awaits host ABI verification. No old feature branches
  were merged or deleted. The sections below remain the acceptance plan, not a claim of completion.

## Step 3: land the open branches

In this order; each one builds on `develop` after the previous. 3a is now a design task, see
[plan-containers.md](plan-containers.md); 3b can start first.

### 3a. `feature/dict` / `feature/dict-test`: do not merge; superseded by a design
- Both branches read entries after the kernel getter has already unlocked (a race the kernel's
  own `copyatoms` shares), classify with `getatom` (which converts string entries in place), and
  leak a +1 into non-owning atoms; the fixes are not local. The kernel's actual semantics for the
  container classes (dictionary, atomarray, string are refcounted; `object_free` is a release; a
  dictionary releases its children) support a single `Arc`-like handle type with all reads done
  under the transaction lock. That design, the ownership facts it rests on, what to salvage from
  each branch, and the acceptance tests are in [plan-containers.md](plan-containers.md).
- Keep both branches until the new `dict` module lands and their tests have been ported, then
  delete them.
- The `#[dictionary]`/`#[savestate]` markers in the redesign depend on it; nothing in the jitter or
  linux work does.

### 3b. `feature/jitter`: split into leaf modules now, wrapper glue with the redesign
The branch's changes to the core wrapper (`obex` field, `ObjWrapped::destroy`, required
`wrapped_byte_offset`/`obex_byte_offset` on `MaxObj`/`MSPObj`, blanket `JitObj for T`) are exactly
the shapes [plan-api-redesign.md](plan-api-redesign.md) §J replaces, so merging them first means
breaking users twice. Split it:

Why a feature flag (2026-10-21): not for linking. On macOS externals link with
`-undefined dynamic_lookup` and unused Jitter code is dead-stripped, so a non-Jitter external never
references a Jitter symbol; on Windows the linker drops unused imports from `jitlib`, so the flag
only saves a link-line entry. It does not shrink the bindings either (Jitter items are in the
committed `ffi-*.rs` regardless). What it buys is an opt-in marker: the leaf modules carry known
bugs (listed below) and an API that §J will reshape, so a flag lets them change without that
counting as breaking for everyone else.

Decision: after the API redesign (step 2) lands, fix the known bugs listed below and then remove the
flag; Jitter support ships unflagged. Keep the flag only for the window in which the leaf modules
are on `develop` ahead of that work.

Now (after 3d bindings), behind `features = ["jitter"]`:
- `jit::matrix::{Matrix, MatrixGuard, MatrixInfo, MatrixData, MOP, Count, IOCount, MatrixDataType}`,
  `jit::matrix::iter`, `jit::matrix::parallel`, `jit::ob3d::{chunk, buffer, geometry}`, the jitter
  enum/function whitelists in `max-sys/build.rs`, `Float32`, `From<f32> for Atom`.
- With these fixes: `parallel.rs:134` `m1i = m0.info()` to `m1.info()`; implement or delete `calc3`;
  read `dimcount` entries, not 32, in `parallel.rs:~30`; `iter.rs:19,29` map `_jit_sym_long` to
  `i32` (Jitter long is 32-bit); `iter.rs:211` `entry_iter(&self)` yielding `&mut [T]` becomes
  `&mut self` with a separate shared iterator; remove `unwrap`/`expect`/`panic!` from
  `iter.rs:33,212,294` and `parallel.rs:163,173` (worker threads).
- Resolve the submodule rename together with 3c (both branches switch to `max-sdk-base`).

With the redesign (step 2, §J):
- `jit::matrix::Wrapper<T>`, `jit::ob3d::Wrapper<T>`, `WrappedMatrixOp`/`WrappedDraw`, `JitObj`,
  the `jit::CLASSES` map, `jit/attr.rs` and the two jit tramp proc-macros, and the core-wrapper diff
  are rewritten as the `kind = matrix_op` / `kind = gl` derive modes with `ObjRef`. The branch's
  known bugs (`assert!(c > 0)` in calc, `expect` on class lookup in `new`, `&mut` in `calc`) are
  fixed by that rewrite rather than patched.
- Port `jit_scalebias` and `jit_gl_simple` to the derive; their hand-written glue (~60 lines each)
  and hand-registered `assist` disappear. Keep the README "Bugs" notes until then.
- Keep the branch until the examples run in Max under the new shape, then delete it.

### 3c. `xnor/linux`
- Fast-forward of `develop`, clean. Fix before merge: root README link (`max-sys-base/` to
  `max-sys/`); delete `rustfmt.toml`'s `merge_imports` (removed unstable option; use
  `imports_granularity` on nightly or nothing); make the generic `build` task dispatch on
  `IS_LINUX_X86`. The Max 9 install path is decided: on `xnor/api-redesign` (2026-10-22)
  `median/utils/Makefile.toml` installs into `Documents/Max ${MAX_VERSION}/Packages`, with
  `MAX_VERSION` defaulting to 9 and overridable (`MAX_VERSION=8 cargo make install`); template
  users pick it up when that reaches `develop` (see 4a).
- The submodule switch to `max-sdk-base` is the right move; regenerate all bindings against it
  (3d) in the same change so mac/windows are not left on the 2021 headers.
- State the Linux target's purpose in the README (there is no public Linux Max; if this is for an
  internal host or RNBO, say so), or keep the Linux bindings off `develop`.

### 3d. Regenerate bindings
- Bump `bindgen` to 0.73 (`--generate-cstr`, `--wrap-unsafe-ops`, `unsafe extern` output,
  `--rust-target 1.85`). Replace the hard-coded `-isysroot .../MacOSX11.0.sdk` with
  `xcrun --show-sdk-path`.
- Regenerate `ffi-macos-aarch64.rs`, `ffi-macos-x86_64.rs`, `ffi-windows-x86_64.rs`, and
  `ffi-linux-x86_64.rs` if kept, all with the jitter headers, from one `max-sdk-base` commit;
  record the commit and bindgen version in `max-sys/README.md`.
- Add `windows-aarch64` (Max 9 ships ARM64 Windows builds; verify against `max-sdk-base`).

### 3e. Small branches
- `origin/clippy`: rebase onto `develop`, resolve `slice.rs` in favour of `develop`, keep the
  `is_empty` addition; most of its `wrapper.rs` edits are superseded by step 1.
- `origin/igk/examples`: deleted on GitHub (pruned 2026-10-22). Its one commit was an unfinished
  port of the SDK's `basics/simplemax` (a post in `new`, an empty hand-registered `assist`); a
  port to the new API is written from `simplemax.c` instead.
- Delete `template-updates`. `origin/xnor/assist` (merged) is already deleted on GitHub. Keep `feature/maxcore-test` local.
- `xnor/inletinfo`: do not merge; its idea lands via the iolet descriptors (step 2 F).

## Step 4: build, packaging, testing

### 4a. Build pipeline
- `templates/max/Makefile.toml` downloads `median/utils/Makefile.toml` and `PkgInfo` from
  `raw.githubusercontent.com/.../develop` at build time. Pin it: download by tag matching the
  `median` version in `Cargo.toml`, or vendor the file into the template and bump on release.
- Consider replacing cargo-make with an `xtask` crate (`cargo xtask package|install|universal`):
  one Rust file, no external tool, readable error messages, trivially testable. Keep the task
  names. If cargo-make stays, add `Info.plist` to the `.mxo` (`MAX_VERSION` is done, see 3c), and make the `build` task dispatch on all platforms.
- Code signing stays ad hoc (`codesign -s -`) unless identities are configured; document
  notarization as out of scope.
- Add `package-info.json` generation (name, version, platforms) like min's `package-info.json.in`.

### 4b. Testing: `median-mock`
- The single biggest gap versus min. Two routes:
  1. Link Cycling74's C `mock` kernel (what min uses) from a `median-mock` crate with a `build.rs`
     that compiles it via `cc`; or
  2. Re-implement the subset of the Max C API that `median` calls (`object_alloc`, `class_*`,
     `gensym`, `atom_*`, `outlet_*` with recording, `clock_*` with a fake scheduler,
     `sysmem_*`, `object_post`, `attr_*`, `buffer_*` basics) in Rust with `#[no_mangle]` exports,
     the way `median/src/test/{atom,sysmem}.rs` already starts to.
  Route 2 keeps everything in cargo and lets tests drive the scheduler and audio thread
  deterministically; route 1 gets more API faster. Either way, replace the current
  `median/src/test/sysmem.rs` stub, which frees with the wrong layout (review M11). Either way the test crate instantiates an
  external through the real `new_tramp`, sends messages through the registered trampolines, and
  asserts on recorded outlet traffic. Run under Miri for the pure-Rust parts.
- CI: GitHub Actions matrix for macos-14 (aarch64), macos-13 (x86_64), windows-latest; `cargo test`,
  `cargo clippy -D warnings`, build the three examples, and `cargo make package` for `simp`.

## Step 5: dependency and toolchain refresh

- Edition 2024 for all three crates (`cargo fix --edition` twice); `#[unsafe(no_mangle)]`,
  `unsafe extern`.
- `lazy_static` to `std::sync::LazyLock`; `field-offset` removed (step 2 D); `paste` to `pastey` or
  a `macro_rules!` that does not need identifier concatenation; `parking_lot` 0.11 to 0.12 or
  `std::sync::Mutex`; `syn` 1 to 3 in `median_macros`.
- `CString::new("..").unwrap()` to `c"..."` literals throughout.
- `#[diagnostic::on_unimplemented]` on `MaxObjWrapped`/`MSPObjWrapped`.
- A root `Cargo.toml` `[workspace]` so `cargo test` / `cargo clippy` run once for all crates;
  `[workspace.package]` for shared `version`, `edition`, `license`, `authors`.
- Delete the untracked `max-sys/fugitive%3a/` and `.ccls-cache/` directories; add `.ccls-cache/` to
  the repo `.gitignore`.
