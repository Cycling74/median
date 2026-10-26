# Max runtime checks for 0.3

The automated tests exercise the Rust trampolines against `median-mock`. They do not verify
Max's implementation of the C API. Record the Max version, OS, architecture, Overdrive and
Scheduler in Audio Interrupt settings with each result below. An unchecked item is unverified.

Build an example from its directory with `cargo make package`; `cargo make install` copies it
into the Max 9 user Packages folder (`MAX_VERSION=8` selects Max 8). Restart Max after replacing
an already loaded external.

The [validation fixtures](../median/examples/validation/README.md) provide three runnable
patches for message/thread diagnostics, mixed MSP inlets, and custom MOP/GL iolets.
Build their complete package with `cargo make package-fixtures` from
`median/examples/validation`. The same external code is exercised by `tests/validation.rs`;
host results and their limits are recorded below.

## Examples and messages

- [ ] Create `simplemax 42 1.25 hello`. Expect its registration post once per load, a creation
  post, three typed argument posts, and `I am inlet 0` when hovering the inlet. Also create
  `simplemax` without arguments and delete both objects.
- [ ] Create `simp` and inspect its message menu: `ft1`–`ft9` and `in1`–`in9` should be hidden.
  If `undocumented` does not hide them, implement API redesign §F(b).
- [ ] Open `simp`'s inspector. Check the Value label/category, clipping to 0–100, and persistence
  after saving/reopening the patch. Check the getter/setter attribute too.
- [x] `02-signals.maxpat`: signal outputs read 0.125 and 0.75; the integer callback
  receives 17 at inlet 2 and the proxy reports inlet 3 (2026-10-26).
- [ ] For patch 02, confirm assistance text, save/reopen and repeated creation/deletion;
  record which DSP/Overdrive/Scheduler in Audio Interrupt combinations were exercised.
- [x] `01-messages.maxpat` delivers loadbang on the main thread and float 17.5 correctly.
  An integer 17 is rejected despite a registered float method: no integer-to-float fallback
  in this fixture (2026-10-26, DSP off). Float-to-int rejection was checked previously.

## Scheduling and teardown

- [ ] Send `read test` to `simp` from a `metro`, with Overdrive enabled. Expect its deferred
  report on the main thread. Delete the object with a call pending; there should be no later
  callback or crash.
- [x] `01-messages.maxpat`, DSP off, Overdrive on/off: integer counter values are preserved,
  and both deferred methods report main=true. With Overdrive off, direct/front report both
  main=true and scheduler=true; low reports main=true, scheduler=false (2026-10-26).
- [ ] Repeat the thread checks with DSP on and record Scheduler in Audio Interrupt.
  Also check manual `42 hello` arguments, reopen/loadbang, and pending-call deletion.
- [ ] Exercise deferred delivery while a modal dialog runs a nested event loop. Calls queued
  during delivery should not disappear or deadlock. A main-thread assertion/instrumented
  external is needed to establish thread identity; a console post alone cannot do that.
- [ ] Confirm that setting a qelem inside its own callback does not reschedule it, and that
  `qelem_front` on an already-set qelem leaves its position unchanged.
- [ ] Establish whether `qelem_set` is supported from the audio thread before treating the
  FIFO-to-queue example as verified for real-time use.
- [ ] Investigate the two open clock shutdown races in `plan-soundness-fixes.md`. Ordinary
  deletion smoke tests cannot establish that either race is absent.

## Buffer exclusivity

- [ ] Verify `buffer_locksamples` returns null while `buffer_edit_begin` is active and that
  `buffer_getinfo().b_samples` is the editable sample pointer. This needs an instrumented
  external, with audio active and inactive; successful playback alone does not verify it.

## Containers and Jitter

- [ ] Verify dictionary registration collisions, child ownership, string reads, and
  `appendtodictionary` callback delivery in Max. Mock tests cannot establish these kernel contracts.
- [ ] Verify the `jsonwriter` `getoutput` calling convention and handle ownership before adding
  in-memory JSON serialization.
- [ ] Exercise `Dict::from_json`/`from_atoms`, file read/write, name switching and modified
  notifications, including malformed input and file errors. Confirm the SDK parser's partial
  output ownership on failure; the mock exercises cleanup but cannot establish that contract.
- [ ] Draw/copy GL chunks and grids in an ob3d callback, including destination changes. Verify
  `jit_glchunk_copy` partial-result ownership. Before safe buffer/geometry construction or
  upload is added, establish which allocations `destroy_tagged` releases and the geometry
  `setdata` ABI, synchronous copying versus retention, and render-context requirements.
- [ ] Create `jit.median.scalebias` between a 4-plane char `jit.matrix` and `jit.pwindow`.
  Check `scale`, `bias`, `@rscale`, clipping, assist text and repeated creation/deletion.
  Compare in-place processing and mismatched dimensions against the SDK scalebias example.
- [ ] Create `jit.gl.median.simple <context>` with `jit.gl.render <context>` and a window.
  Confirm the quad, automatic drawing, destination changes and context close/reopen. Test
  deletion after closing the render context. The example uses SDK immediate-mode GL calls.
- [ ] Check Jitter attribute inspector/save behavior, SDK notifications and hot/cold iolets;
  the mock checks forwarding/handler preservation but does not implement the host wrappers.
- [ ] Check a fixed-count MOP and ob3d with extra numeric/proxy inlets and message outlets.
  Verify SDK iolets precede the custom ones (including dumpout), returned inlet positions,
  assistance text, hot/cold display and proxy message dispatch. Repeat creation/deletion and
  constructor refusal. `03-jitter-iolets.maxpat` supplies runnable fixtures; use
  `jit.median.validate refuse` for intentional construction failure.
- [ ] Run Jitter parallel callback tests in Max; current tests cover validated iteration
  and direct trampoline calls, not the host scheduler.

## Recorded results

From the previous handoff: the user verified `hello_dsp~` following and playing its named
`buffer~`, and a class with `int` but no `float` rejecting float messages.

### 2026-10-26 — support-info environment snapshot

User-supplied Max support information, received after the patch 03 crash report:

| Setting | Value |
| --- | --- |
| Max | 9.2.0 (e9c80e453de), native ARM64 |
| OS | macOS 14.6.1, build 23G93, ARM64 |
| Audio driver | Core Audio |
| Sample rate | 44,100 Hz |
| I/O vector size | 512 samples |
| Signal vector size | 64 samples |
| Overdrive | On |
| Scheduler in Audio Interrupt | Off |
| Jitter GL engine | `gl3` |
| Video engine | `avf` |

The version/platform match the crash report. These are the settings at snapshot time;
they do not establish DSP on/off status or the settings at every earlier test. In particular,
patch 01 explicitly exercised both Overdrive settings. The snapshot does not identify the
loaded `median.validate` build or confirm a successful run after the crash fix.

### 2026-10-26 — 01-messages, DSP off

User-supplied console output, first with Overdrive on, then switched off. DSP was off
throughout (explicitly confirmed). Environment details were supplied later in the support-info
snapshot above; the tested commit and Scheduler in Audio Interrupt setting for this run
were not recorded with its output.

| Observation | main | scheduler | audio |
| --- | --- | --- | --- |
| loadbang; float 17.5 | true | false | false |
| direct 0–31, Overdrive on | false | true | false |
| front/low 0–31, Overdrive on | true | false | false |
| direct/front 32–38, Overdrive off | true | true | false |
| low 32–38, Overdrive off | true | false | false |

Integer 17 produced `doesn't understand "int"`: the float-only class does not receive an
integer through its float handler. The mock's int-to-float fallback was corrected accordingly;
its other unverified int/list/anything fallback paths are not established by this fixture.

Every logged counter value had direct, front, then low delivery, with matching arguments.
The Overdrive-off result is consistent with `#[defer]` running immediately on the main
thread and `#[defer_low]` waiting for the main queue. Main/scheduler flags are not exclusive.
This does not validate DSP-on behavior, modal reentrancy, pending-call teardown, symbol
arguments, or the assumed position of an already-set qelem after `qelem_front`.

### 2026-10-26 — 02-signals

The user reported the expected snapshot values: left 0.125, right 0.75. The console log
contains four pairs of `integer inlet 2 value 17` and `proxy inlet 3`, each with
main=true, scheduler=false, audio=false. This validates the two signal paths and custom
integer/proxy dispatch in the tested configuration.

The message-thread flags do not establish whether DSP was running at each click. Exact
DSP/Overdrive/Scheduler in Audio Interrupt settings for the four pairs were not supplied.
Environment details were supplied later in the support-info snapshot above; the tested commit
remains unspecified. Assistance, save/reopen and repeated creation/deletion were not reported.

### 2026-10-26 — 03-jitter-iolets: construction crash

Opening the patch crashed Max 9.2.0 (e9c80e453de), native ARM64, on macOS 14.6.1.
The supplied report at 10:37:51 shows `object_obex_lookup` called by `jnewobj_new` during
patcher loading, with an invalid-address read. No iolet or rendering checks passed.

The wrapper registered the same pointer field for Max's object obex table and Jitter's
wrapper data. Jitter initialization overwrote Max's table pointer, consistent with this
stack. The correction gives each its own slot. Inspection of the installed Max 9.2 Jitter
implementation also established that `max_jit_object_wrap_complete` initializes a null
core pointer; the wrapper now calls `max_jit_obex_jitob_set` explicitly, matching the SDK's
core attachment sequence. The old mock conflated both storage registrations and implicitly
attached the core, hiding both defects.

Regression checks cover independent offsets, collision detection, explicit attachment,
the core's `maxwrapper` backlink, and Max obex store/lookup after construction for both
validation classes. Reinstall the rebuilt package and restart Max before retrying patch 03.
The corrected binary still needs a host retest, including creation/deletion and refusal.

Local verification: all 252 workspace tests passed, including UI tests and doctests;
strict Clippy and documentation with warnings denied passed. Temporarily removing either
fix made the validation regression fail. The Apple Silicon diagnostic package was rebuilt
and its bundle signature verified.
