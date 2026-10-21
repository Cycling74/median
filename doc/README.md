# median design notes

Written on branch `xnor/review`, 2026-10-21, from a full review of `develop` at bf91d59.

| document | what it is |
|---|---|
| [review-2026-10.md](review-2026-10.md) | The review: soundness findings with file:line, ergonomics, comparison with min, branch verdicts, Rust features worth adopting. |
| [plan-soundness-fixes.md](plan-soundness-fixes.md) | Step 1. Fixes that land on `develop` without changing the public API, in dependency order, each with its acceptance check. |
| [plan-api-redesign.md](plan-api-redesign.md) | Step 2. The breaking release: `#[derive(MaxObject)]` + `#[median::methods]`, generic trampolines, `ObjRef` handle, fallible `new`, iolet descriptors, attributes on fields, concrete outlets. |
| [plan-containers.md](plan-containers.md) | Dictionary, atomarray and string wrappers: the ownership facts from the SDK headers (verified against the kernel source), a single refcounted handle design, the `dictionary` message protocol, what to salvage from `feature/dict*`, acceptance tests. |
| [plan-branches-and-build.md](plan-branches-and-build.md) | Steps 3 to 5. What to do with each open branch, the build/packaging pipeline, bindings regeneration, testing, and dependency refresh. |

Conventions used in these notes:

- `H`/`M`/`L` severities are defined at the top of the review.
- Code sketches are illustrative; names are proposals, not decisions.
- "Max" means the Max/MSP runtime; "SDK" means `max-sys/thirdparty/max-sdk` (Max 8 SDK, pinned June 2021) unless `max-sdk-base` is named.
