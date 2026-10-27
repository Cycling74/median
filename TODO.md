# TODO

- Checkpoint (2026-10-23): see `doc/handoff-2026-10-23.md` for completed work and next steps.
  The 0.2.x clock backport below remains deferred at the user's request.

- [ ] Backport f587265 ("clock: pass a symbol, not a C string, as the `object_obex_lookup` key")
  to `develop` (a 0.2.x patch release). 0.2.0 has the bug: `ClockHandle::new_unarmed`
  (`median/src/clock.rs`) passes the `#P`/`#B` C strings straight to `object_obex_lookup`, whose
  key is a `t_symbol *` (`ext_obex.h`), for every object that creates a clock. Only the
  `median/src/clock.rs` hunk applies: wrap the key in `max_sys::gensym(..)` in place of the
  `transmute`. The commit's `AGENTS.md` and `median/tests/e2e.rs` hunks touch files that only
  exist on `xnor/api-redesign`.
- [ ] Run the "To check in Max" list (summary under "Where things stand" in
  `doc/plan-api-redesign.md`) and record the results in `doc/validation-max.md`.
- [x] Port `simplemax` (SDK `basics/simplemax/simplemax.c`) as `median/examples/simplemax`
  with the new API, plus a mock test. Construction, arguments, errors, assist and cleanup
  pass against the mock; `cargo make package` builds a signed macOS ARM64 bundle. Not run in Max.
- [ ] Push `xnor/api-redesign` and open the 0.3 PR (decide the target: GitHub has no `main`).
- [ ] Validate the new unflagged Jitter wrappers/examples in Max, then complete extra iolets
  and context-safe GL resource helpers (`doc/plan-branches-and-build.md` 3b).
- [ ] Verify if setting qelem is realtime-safe and update median::queue docs.
- [ ] `Reply::Str(String)` for gimmeback methods (`doc/plan-gimmeback.md`): a fresh `t_string`
  (`string_new`) handed to the caller, which frees it. median has no public string type, so this
  is the one place that makes one; needs `string_new` in the mock.
