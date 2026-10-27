# median design notes

Written on branch `xnor/review`, 2026-10-21, from a full review of `develop` at bf91d59.

| document | what it is |
|---|---|
| [review-2026-10.md](review-2026-10.md) | The review: soundness findings with file:line, ergonomics, comparison with min, branch verdicts, Rust features worth adopting. |
| [plan-soundness-fixes.md](plan-soundness-fixes.md) | Step 1, **complete** (`median` 0.2.0). Fixes that land on `develop` without changing the public API, in dependency order, each with its acceptance check. Its "Status" section records deviations and the issues left open. |
| [plan-api-redesign.md](plan-api-redesign.md) | Step 2, implemented on `xnor/api-redesign` (all items ticked in its "Status" section); not yet checked in Max. The breaking release (`median` 0.3): `#[derive(MaxObject)]` + `#[median::methods]`, generic trampolines, `ObjRef` handle, fallible `new`, iolet descriptors, attributes on fields, concrete outlets. |
| [handoff-2026-10-23.md](handoff-2026-10-23.md) | Current continuation checkpoint, verification and remaining work. |
| [migration-0.3.md](migration-0.3.md) | Porting an external from `median` 0.2 to 0.3: old form, new form, a before/after of `simp`, and the things that trip a port. |
| [plan-containers.md](plan-containers.md) | Dictionary, atomarray and string wrappers: the ownership facts from the SDK headers (verified against the kernel source), a single refcounted handle design, the `dictionary` message protocol, what to salvage from `feature/dict*`, acceptance tests. |
| [plan-gimmeback.md](plan-gimmeback.md) | Design and status (2026-10-27, steps 0-3 implemented on `xnor/api-redesign`; host checks open): `A_GIMMEBACK` methods (`#[sel_gimmeback]`, `Class::add_sel_gimmeback`, a `Reply` type), what the SDK says about the reply and its ownership, the Jitter dumpout wrap, mock support, tests and the host checks that gate it. |
| [plan-branches-and-build.md](plan-branches-and-build.md) | Steps 3 to 5. What to do with each open branch, the build/packaging pipeline, bindings regeneration, testing, and dependency refresh. |

Conventions used in these notes:

- `H`/`M`/`L` severities are defined at the top of the review.
- Code sketches are illustrative; names are proposals, not decisions.
- "Max" means the Max/MSP runtime; "SDK" means `max-sys/thirdparty/max-sdk` (Max 8 SDK, pinned June 2021) unless `max-sdk-base` is named.
