# Max validation fixtures

Five diagnostic classes in one external, with five patches that exercise the same code
as `median/tests/validation.rs`. Mock tests check the probes; only a run in Max establishes
the host behavior. On 2026-10-26 the user verified patch 01 with DSP off and Overdrive on/off,
plus patch 02's signal outputs and custom integer/proxy inlet positions;
see `doc/validation-max.md` in the source checkout for the observations and remaining checks.
Patch 03 exposed a construction crash in Max 9.2 ARM64. After fixing the wrapper's Max/Jitter
storage collision and core attachment, the user confirmed opening the patch, MOP assistance
and float/proxy dispatch, GL hotness/outlets/proxy output, and no crashes during GL
deletion/recreation. Intentional constructor refusal reports both expected errors in Max;
several refused/normal construction cycles also work as expected. Context-first teardown
still needs confirmation.

## Build and load

From this directory:

```sh
cargo make package-fixtures
```

The complete package is `target/<native-target>/debug/median-validation/`. On Apple Silicon,
`<native-target>` is `aarch64-apple-darwin`; on Intel Mac, `x86_64-apple-darwin`; on native
Windows, `x86_64-pc-windows-msvc`. Copy the **whole** package into
`Documents/Max 9/Packages/` (or Max 8's Packages folder), then restart Max. Keep `init/`:
its objectfile mappings let Max discover all five classes in `median.validate`.
Open the patches in the installed package's `patchers/` directory.

Alternatively, on macOS `cargo make install` installs the external and init mappings;
open this source directory's patches after restarting Max. Restart again whenever replacing
an already loaded binary. Do not keep two copies of this package on Max's search path.

## Run in order

Open the Max console first. Inlets and outlets are numbered **from zero, left to right**.
The patches include controls and expected results; start with all toggles off.

| Patch | What to check |
| --- | --- |
| `01-messages.maxpat` | On opening/reopening, `messages: loadbang; main=true scheduler=false audio=false`. Clicking `17.5` reports `float 17.5`; `17` is rejected (no int-to-float fallback observed). With Overdrive on, direct reports main=false, scheduler=true; low/front report main=true, scheduler=false. With Overdrive off, direct/front report both main=true and scheduler=true; low reports main=true, scheduler=false. These results were observed with DSP off. Check argument preservation too. |
| `02-signals.maxpat` | Enable DSP using ezdac~, then enable the sampling toggle. The two displays should read `0.125` and `0.75`. No signal is routed to speakers. The integer inlet reports position 2; `probe` at the proxy reports position 3. Hover to check assistance, then repeat with DSP and scheduler settings changed. |
| `03-jitter-iolets.maxpat` | MOP custom inlets occupy 2–4; SDK matrix outlets and dumpout occupy 0–2, then integer echo 3 and bang 4. GL custom proxy is inlet 1 and its index appears at outlet 2. Check assistance/hotness and repeated creation/deletion. The GL fixture draws nothing. The MOP fixture does not process matrices. |
| `04-buffer-exclusivity.maxpat` | Click `probe` with DSP off, then with DSP on using ezdac~. The dedicated buffer's pointers and lock results appear in four console lines. The signal output is silence. Each probe pauses the UI for about 450 ms; do not change the buffer during it. |
| `05-gimmeback.maxpat` | Three `A_GIMMEBACK` host checks (`doc/plan-gimmeback.md`); results are in `doc/validation-max.md`. **A.** `js gimmeback.js` (a `v8 gimmeback.js` box runs the same script as a negative control: on 2026-10-27 `v8`'s `message()` refused the selectors with "bad arguments"/"missing arguments" and the methods did not run) calls `getcount`, `getlist`, `getstate` and `getfail` on `median.validate gimme` with `obj.message(..)`. Observed in `js` on 2026-10-27: the method runs with the right arguments and `getfail`'s error posts, but `Maxobj.message(..)` returns the `message` function object itself, never the method's reply, so a script cannot read a gimmeback reply through a box object (the SDK's `simplejs` route is not supported by median yet). Observed in the third run: `getcount` no longer appears in `median.validate`'s message list (the `undocumented` marking works). The memory check is the `burst 6000` message box (the `v8` box refuses it, which is fine): it calls `getquiet` (a list of 4096 ints, 64 KiB, no console line) 6000 times in a loop and posts `burst done: 6000 calls`. Read Max's memory in Activity Monitor, click, read again, repeat: 6000 replies are 384 MB if they leak, so a leak is a step of about that size per click that never comes back; a few tens of MB that stay flat across repeated clicks is allocator slack. The `metro 10` toggles (`getquiet` 100 times a second) are the weaker test: their first run was inconclusive. **B.** The message boxes `getcount 1 2` and `getcount` wired straight to a `median.validate`. Observed 2026-10-27: `unknown message argument type`, `median.validate: bad arguments for message "getcount"`, `median.validate: missing arguments for message "getcount"`; the method did not run. median marks a gimmeback method `undocumented`, so the patcher's message list does not offer it. **C.** `getvalues 7` and `getcount 1 2 3` into `jit.median.validate`, `print dumpout` on its dumpout (outlet 2). Observed 2026-10-27: `dumpout: values 1 2.5 foo 7` and `dumpout: count 3`, the reply as a list under the selector without its leading `get`. The `getvalues` metro toggle prints every reply, so it measures the console as much as the reply (inconclusive). **C2.** The decisive memory test: a second `jit.median.validate` with nothing attached; a button runs `uzi 6000` into a `getbig` message (a list of 4096 ints, 64 KiB, no console line). Expected: Max's memory a few tens of MB higher at most after a click and flat across repeated clicks; a step of about 384 MB per click that never comes back means the reply leaks. Use the recording block under "Record results". The script is `patchers/gimmeback.js`. |

Stop metros/rendering/DSP when finished. For constructor cleanup, manually create
`jit.median.validate refuse`: it must report an intentional constructor error. Delete it and
create `jit.median.validate` normally; repeat a few times. For GL teardown, also delete the
context before deleting the GL object, then recreate both.
Normal creation/deletion is quiet: no error or lifecycle post is expected. After recreating
`jit.gl.median.validate median_validation`, reconnect `probe` to inlet 1 and check that it
again posts `gl: proxy inlet 1` and sends 1 from the rightmost outlet. The GL fixture draws
nothing; an empty window is expected.

The message patch supports a deletion smoke test while the metro is running. That does not
prove every pending-call interleaving or either documented clock shutdown race is safe.
These fixtures do not yet exercise nested modal event loops,
audio-thread qelem support, dictionary kernel ownership, JSON output, or GL upload ownership.
The remaining host checks are in the repository's `doc/validation-max.md`.

## Buffer probe (patch 04)

`median.validate.buffer~ <buffer-name>` defers `probe` to the low-priority main thread.
It observes three 150 ms windows: before the edit, during `buffer_edit_begin/end`, and after.
The audio callback attempts shared claims and immediately releases successful ones; it only
updates atomic counters and produces silence. All console output happens after the edit ends.
The probe never reads/writes sample data or constructs sample slices, including if exclusion
fails. It restores the buffer's old valid flag with `buffer_edit_end(buffer, -1)`.

Expected results:

- `edit_begin=0 getinfo=0 edit_end=0` (successful SDK calls).
- `main_exclusion=PASS`: the main-thread claim during the edit returned null.
- `pointer_identity=PASS`: `b_samples` during the edit matches the successful shared-claim
  pointers before and after, with positive frame/channel counts.
- DSP off: `audio_exclusion=INCONCLUSIVE (DSP off or insufficient callbacks)` is expected.
- DSP on: `audio_exclusion=PASS` requires successful claims before/after, at least one refused
  claim during the edit, and zero successful claims during it. Calls spanning phase boundaries
  are discarded. `INCONCLUSIVE` is not a pass; check DSP and the connected snapshot~ path.

First run with Scheduler in Audio Interrupt off. Repeat with Overdrive on/off and record the
settings. Send all four console lines for each run. Stop and report a `FAIL` before continuing
to sample-edit tests. These observations test the exclusion and pointer-identity assumptions;
pointer equality alone does not establish permission for mutable slices, nor do finite runs
prove every interleaving safe. The probe has mock coverage, including deliberately disabled
exclusion. On 2026-10-26 the user's first Max run passed main exclusion and pointer identity
with no audio callbacks (expected at the DSP-off step). The second passed all three checks:
104 successful audio claims before/after and 104 refused claims during the edit, with no
successful claims during it. The user confirmed Overdrive was on for that pass and repeated
the test with Overdrive off, obtaining the same results and counts. See `doc/validation-max.md`
in the repository for the environment and scope; Scheduler in Audio Interrupt enabled and
actual sample editing remain untested.

## Record results

Copy this block per test session and include the exact console output for unexpected results:

```text
Commit:
Max version:
OS / architecture:
Overdrive:
Scheduler in Audio Interrupt:
DSP on/off:
Patch / action:
Expected:
Observed:
```

Record pass/fail in `doc/validation-max.md` in the source checkout. A blank checkbox means
unverified, not failed. Windows packaging is configured but has not been run on Windows here.
