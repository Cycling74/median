# Max runtime checks for 0.3

The automated tests exercise the Rust trampolines against `median-mock`. They do not verify
Max's implementation of the C API. Record the Max version, OS, architecture, Overdrive and
Scheduler in Audio Interrupt settings with each result below. An unchecked item is unverified.

Build an example from its directory with `cargo make package`; `cargo make install` copies it
into the Max 9 user Packages folder (`MAX_VERSION=8` selects Max 8). Restart Max after replacing
an already loaded external.

## Examples and messages

- [ ] Create `simplemax 42 1.25 hello`. Expect its registration post once per load, a creation
  post, three typed argument posts, and `I am inlet 0` when hovering the inlet. Also create
  `simplemax` without arguments and delete both objects.
- [ ] Create `simp` and inspect its message menu: `ft1`–`ft9` and `in1`–`in9` should be hidden.
  If `undocumented` does not hide them, implement API redesign §F(b).
- [ ] Open `simp`'s inspector. Check the Value label/category, clipping to 0–100, and persistence
  after saving/reopening the patch. Check the getter/setter attribute too.
- [ ] Check MSP inlet ordering with a proxy after signal inlets (a dedicated test external is
  still needed; `hello_dsp~` alone does not establish this).
- [ ] Check `loadbang`'s `A_CANT` callback signature and integer-to-float message fallback with
  a dedicated test external. Float-to-int fallback was already checked and does not occur.

## Scheduling and teardown

- [ ] Send `read test` to `simp` from a `metro`, with Overdrive enabled. Expect its deferred
  report on the main thread. Delete the object with a call pending; there should be no later
  callback or crash.
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
- [ ] Create `jit.median.scalebias` between a 4-plane char `jit.matrix` and `jit.pwindow`.
  Check `scale`, `bias`, `@rscale`, clipping, assist text and repeated creation/deletion.
  Compare in-place processing and mismatched dimensions against the SDK scalebias example.
- [ ] Create `jit.gl.median.simple <context>` with `jit.gl.render <context>` and a window.
  Confirm the quad, automatic drawing, destination changes and context close/reopen. Test
  deletion after closing the render context. The example uses SDK immediate-mode GL calls.
- [ ] Check Jitter attribute inspector/save behavior, SDK notifications and hot/cold iolets;
  the mock checks forwarding/handler preservation but does not implement the host wrappers.
- [ ] Run Jitter parallel callback tests in Max; current tests cover validated iteration
  and direct trampoline calls, not the host scheduler.

## Recorded results

From the previous handoff: the user verified `hello_dsp~` following and playing its named
`buffer~`, and a class with `int` but no `float` rejecting float messages. No further runtime
results have been recorded in this continuation.
