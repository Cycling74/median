# Max runtime checks for 0.3

The automated tests exercise the Rust trampolines against `median-mock`. They do not verify
Max's implementation of the C API. Record the Max version, OS, architecture, Overdrive and
Scheduler in Audio Interrupt settings with each result below. An unchecked item is unverified.

Build an example from its directory with `cargo make package`; `cargo make install` copies it
into the Max 9 user Packages folder (`MAX_VERSION=8` selects Max 8). Restart Max after replacing
an already loaded external.

The [validation fixtures](../median/examples/validation/README.md) provide four runnable
patches for message/thread diagnostics, mixed MSP inlets, custom MOP/GL iolets, and buffer
edit exclusion/pointer identity.
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

- [x] Observe `buffer_locksamples` returning null while `buffer_edit_begin` is active,
  and `buffer_getinfo().b_samples` matching the shared-claim pointers before/after the edit.
  Patch 04 passed the main-thread and pointer checks with Overdrive on and off; each setting
  also passed audio exclusion (104 refused claims during the edit). See the host record below.
- [ ] Test with Scheduler in Audio Interrupt enabled and validate actual sample editing separately.
  The read-only probe creates no sample slices and touches no sample data: pointer equality
  alone does not prove writable-slice safety or every possible interleaving.

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
- [x] Patch 03 opens after the storage fix. MOP assistance/hotness, float inlet 3 and proxy
  inlet 4, GL inlet hotness/outlet layout and proxy inlet 1 output are confirmed (2026-10-26).
  Manual MOP creation twice and GL deletion/recreation produced no reported crashes.
- [x] `jit.median.validate refuse` reports the intentional constructor failure and the
  wrapper's `unable to create Jitter object` error (2026-10-26).
- [x] Several refusal → delete → normal creation cycles work as expected (2026-10-26).
- [ ] Complete patch 03 checks: MOP integer echo 19 at outlet 3 and bang at outlet 4,
  and GL teardown with the context
  deleted first. Repeat the probe after recreating the GL object.
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
validation classes. The subsequent host retest below confirms opening and basic iolet
behavior; the later refusal check also reports the expected errors. Context-first teardown
remains unverified.

Local verification: all 252 workspace tests passed, including UI tests and doctests;
strict Clippy and documentation with warnings denied passed. Temporarily removing either
fix made the validation regression fail. The Apple Silicon diagnostic package was rebuilt
and its bundle signature verified.

### 2026-10-26 — 03-jitter-iolets: retest after the storage fix

The user opened patch 03 and reported that assistance and hot/cold indicators look correct
on `jit.median.validate`. Console output confirms `matrix: float inlet 3 value 2.5` and
`matrix: proxy inlet 4`, both with main=true, scheduler=false, audio=false. Two manual
creations of `jit.median.validate` succeeded without errors, as expected without `refuse`.

For `jit.gl.median.validate`, both inlets are hot, outlets look correct, and clicking `probe`
sends 1 from the rightmost outlet. The console reports `gl: proxy inlet 1` with main=true,
scheduler=false, audio=false. The user reported no crashes when deleting/recreating the GL
object. Successful creation/deletion has no diagnostic post, and this fixture draws nothing.
The console also reported OpenGL 4.1 Metal - 88.1 and GLSL 4.10; that startup message alone
does not validate drawing or resource uploads.

This confirms that the original patch-load crash did not recur in the retest. MOP integer
echo/bang output, probe delivery after GL recreation,
and deleting the render context before the GL object were not explicitly reported. Exact
loaded build and DSP/scheduler settings for this retest were not supplied separately from
the support-info snapshot above.

The subsequent `jit.median.validate refuse` check produced the expected console output:

```text
median_jit.median.validate: validation: intentional constructor refusal after making custom iolets
jit.median.validate: unable to create Jitter object
```

This confirms delivery of the intentional refusal and the outer wrapper's failure report.
The user subsequently confirmed that several refusal → delete → normal creation cycles
work as expected. This passes the recovery smoke test; it does not measure leaks in Max.
Mock tests separately cover resource cleanup on this path.

### 2026-10-26 — 04-buffer-exclusivity

User-supplied output after the DSP-off / DSP-on instructions for the probe added in
`a467be2`. The environment snapshot above identifies Max 9.2.0 on native ARM64 macOS 14.6.1;
the loaded build was not independently identified. The user subsequently confirmed Overdrive
was on for the original DSP-on pass, then supplied a second pair of runs in response to the
Overdrive-off instructions. Scheduler in Audio Interrupt was requested off; its setting was
not separately confirmed with the logs.

All four runs report main=true, scheduler=false, audio=false for the deferred probe, and
`edit_begin=0 getinfo=0 edit_end=0`. The buffer has 44,100 frames and two channels.
In all four runs, before/edit-info/after pointers equal `0x130088050`, while the main-thread
claim during the edit returns `0x0`: `main_exclusion=PASS pointer_identity=PASS`.

| Run | Before claims/refused | During claims/refused | After claims/refused | Audio result |
| --- | --- | --- | --- | --- |
| Original pair, first (DSP-off step) | 0/0 | 0/0 | 0/0 | INCONCLUSIVE, expected without callbacks |
| Original pair, second (DSP on, Overdrive on) | 104/0 | 0/104 | 104/0 | PASS |
| Overdrive-off pair, first (no callbacks) | 0/0 | 0/0 | 0/0 | INCONCLUSIVE, expected without callbacks |
| Overdrive-off pair, second (DSP on) | 104/0 | 0/104 | 104/0 | PASS |

This establishes observed shared-claim exclusion on the main and audio threads, recovery
after the edit, and sample-pointer identity with Overdrive on and off. Zero counters alone
cannot distinguish DSP off from insufficient callbacks. The probe never accesses sample
data; actual mutable-slice use, Scheduler in Audio Interrupt enabled, and other host versions
remain outside this result.
