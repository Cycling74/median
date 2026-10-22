# TODO

- [ ] Backport f587265 ("clock: pass a symbol, not a C string, as the `object_obex_lookup` key")
  to `develop` (a 0.2.x patch release). 0.2.0 has the bug: `ClockHandle::new_unarmed`
  (`median/src/clock.rs`) passes the `#P`/`#B` C strings straight to `object_obex_lookup`, whose
  key is a `t_symbol *` (`ext_obex.h`), for every object that creates a clock. Only the
  `median/src/clock.rs` hunk applies: wrap the key in `max_sys::gensym(..)` in place of the
  `transmute`. The commit's `AGENTS.md` and `median/tests/e2e.rs` hunks touch files that only
  exist on `xnor/api-redesign`.
- [ ] Run the "To check in Max" list (summary under "Where things stand" in
  `doc/plan-api-redesign.md`) and record the results there.
- [ ] Port `simplemax` (SDK `basics/simplemax/simplemax.c`) as `median/examples/simplemax`
  with the new API, plus a mock test.
- [ ] Push `xnor/api-redesign` and open the 0.3 PR (decide the target: GitHub has no `main`).
- [ ] After the redesign: fix Jitter's known bugs and drop its feature flag
  (`doc/plan-branches-and-build.md` 3b).
