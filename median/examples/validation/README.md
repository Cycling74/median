# Max validation fixtures

Four diagnostic classes in one external, with three patches that exercise the same code
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
its objectfile mappings let Max discover all four classes in `median.validate`.
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
These fixtures do not yet exercise nested modal event loops, buffer edit exclusivity,
audio-thread qelem support, dictionary kernel ownership, JSON output, or GL upload ownership.
The remaining host checks are in the repository's `doc/validation-max.md`.

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
