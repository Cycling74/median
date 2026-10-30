# Compiler-independent rejection tests

Status: implemented and locally verified, 2026-10-30.

Summary: replaced rustc output snapshots with direct macro-diagnostic tests and paired
compilation tests. CI now runs the rejection coverage on minimum Rust and stable. All local
workspace tests, Clippy, and formatting checks pass; cross-platform CI remains to be run.

## Goal and approach

Keep coverage of accepted/rejected programs and our macro diagnostics without comparing
rustc's rendered output. The original suite had 26 fixtures and 87 diagnostic headings:
14 fixtures validate macro rules and 12 exercise compiler/API restrictions.

- Test macro validation directly inside `median_macros`, retaining every expected macro
  error (including multiple errors in one input). Compare our messages, not rustc output.
- Replace the remaining snapshots with isolated compilation cases. Each case has a valid
  control and one invalid variation; both run through code generation. Parse Cargo JSON
  diagnostics to reject infrastructure/link failures, without matching compiler wording.
- Retain end-to-end macro coverage and the ordinary Max `notify` passing example.
- Run the rejection suite on both Rust 1.85.0 and stable; remove the snapshot-driven CI pin,
  `rust-src` requirement, and MSRV skip. Update contributor guidance.

## Progress

- [x] Inventory current fixtures and inspect macro entry points, CI, and codegen requirements.
- [x] Move 14 macro-validation fixtures into direct tests with explicit expected errors.
- [x] Split compiler/API cases into independently checked valid/invalid pairs.
- [x] Replace trybuild with a Cargo/JSON runner; preserve codegen-only rejection checks.
- [x] Update CI, dependencies, formatting guidance, and relevant design notes.
- [x] Verify targeted tests on stable and 1.85.0, workspace tests, formatting, and Clippy.
- [x] Record final results and any remaining work here.

## Important checks

The original `jitter_context` combined private construction and non-Send checks; signal iolets
combined inlet and outlet rejection; several method fixtures combined unrelated signature
errors. These are now split so one rejection cannot hide another regression. Jitter `notify` requires
code generation, not just `cargo check`. All valid controls must link against `median-mock`.
Compiler error codes, full diagnostic counts, rendered spans, and suggestions are not test
contracts. Actual language/API behavior changes may still require investigation.

## Verification and resume notes

Implemented: all 14 macro fixtures retain their 65 expected messages and source lines in
`.errors` files, checked directly without rustc. The 12 compiler/API fixtures became 20
isolated pairs; two macro smoke-test pairs and the existing Max notify control make 23
valid controls and 22 rejection cases. Every invalid variant replaces an explicit `// ACCEPT`
region with its `// REJECT:` line. Regions tolerate rustfmt expanding a statement or method.

The runner handles codegen errors whose fixture location appears in a separate instantiation
note. Harness regression tests reject missing diagnostics, dependency errors, linker-style
errors without source spans, and compiler crashes. Failed runs retain generated sources under
`target/compile-tests/cases-*` for inspection; successful runs remove their generated sources
and keep the shared build cache.

Verification on Apple Silicon:

- `cargo test --workspace --offline`: passed on stable Rust 1.99.1 (355 passed, 3 ignored).
- `cargo +1.85.0 test --workspace --offline`: passed (355 passed, 3 ignored), including the
  rejection suite that the previous CI configuration skipped.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed after removing
  one redundant closure in the new macro test harness.
- `cargo fmt --all -- --check`, `rustfmt --check median/tests/compile/*.rs`, and
  `git diff --check`: passed.
- Targeted compile and macro suites passed again after the final harness/test cleanup.

No implementation work remains for this migration. CI is configured to run the same suites
on stable macOS ARM64, macOS Intel and Windows, plus minimum Rust on macOS ARM64; the remote
workflow and other platforms have not been run locally. No library behavior changed.

When resuming: review the migration commit and run CI when ready. Original snapshots remain
in git history for auditing coverage. The moved macro fixtures retain their original file
names; combined compiler fixtures were split into descriptive names under `median/tests/compile`.
Actual changes in language/API acceptance can still fail these tests; compiler prose, error
codes, formatting, suggestions, and standard-library source excerpts are no longer compared.
