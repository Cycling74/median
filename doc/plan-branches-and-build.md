# Plan, steps 3 to 5: branches, build pipeline, bindings, tests, dependencies

Branch states as of 2026-10-21 (merge-base and conflict status from `git merge-tree --write-tree`).
Details and file:line pointers are in [review-2026-10.md](review-2026-10.md) §4.

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
  `IS_LINUX_X86`; decide whether the Max 8 to Max 9 default install path is wanted now (every
  template user picks it up on merge, see 4a).
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
- `origin/igk/examples`: rename `.cargo/config` to `config.toml`, add to `examples/README.md`, port
  to the current API.
- Delete `template-updates` and `origin/xnor/assist` (merged). Keep `feature/maxcore-test` local.
- `xnor/inletinfo`: do not merge; its idea lands via the iolet descriptors (step 2 F).

## Step 4: build, packaging, testing

### 4a. Build pipeline
- `templates/max/Makefile.toml` downloads `median/utils/Makefile.toml` and `PkgInfo` from
  `raw.githubusercontent.com/.../develop` at build time. Pin it: download by tag matching the
  `median` version in `Cargo.toml`, or vendor the file into the template and bump on release.
- Consider replacing cargo-make with an `xtask` crate (`cargo xtask package|install|universal`):
  one Rust file, no external tool, readable error messages, trivially testable. Keep the task
  names. If cargo-make stays, add `MAX_VERSION` as `env_not_set`-conditional, add
  `Info.plist` to the `.mxo`, and make the `build` task dispatch on all platforms.
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
