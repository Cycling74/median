# TODO

- [ ] Backport f587265 ("clock: pass a symbol, not a C string, as the `object_obex_lookup` key")
  to `develop` (a 0.2.x patch release). 0.2.0 has the bug: `ClockHandle::new_unarmed`
  (`median/src/clock.rs`) passes the `#P`/`#B` C strings straight to `object_obex_lookup`, whose
  key is a `t_symbol *` (`ext_obex.h`), for every object that creates a clock. Only the
  `median/src/clock.rs` hunk applies: wrap the key in `max_sys::gensym(..)` in place of the
  `transmute`. The commit's `AGENTS.md` and `median/tests/e2e.rs` hunks touch files that only
  exist on `xnor/api-redesign`.
