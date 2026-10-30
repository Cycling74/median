# Max SDK examples in median

Ports of the C examples in the Max SDK (`max-sys/thirdparty/max-sdk/source`) to median, one crate
per example, laid out like the SDK: `basics/plussz` ports `source/basics/plussz/plussz.c`. Read
the two side by side to see how an SDK idiom is written with median.

`basics/simplemax` is ported separately as `median/examples/simplemax`.

## Names

Every class is registered with a `median.` prefix (`median.plussz`, `median.times~`), so the
ports can be loaded next to the C builds. Crate names replace `.` with `_` and `~` with `_tilde`;
the Max class name is the crate's `[package.metadata.max] name`. A crate without that table is a
shared library the externals link against (`advanced/mydll`); packaging puts it in the package's
`support/`. A crate's `init/` files (object mappings, as for `median.mc.combine~`) go to the
package's `init/`.

Jitter matrix classes are `jit.median.<name>` (`jit.median.clip` in crate `jit_clip`; `simplejit`
registers `jit.median.simple`), and their Jitter core classes `median_jit.median.<name>`.
`matrix/jit.scalebias` is ported separately as `median/examples/jit_scalebias`.
Jitter GL classes are `jit.gl.median.<name>` (`jit.gl.median.cube` in crate `jit_gl_cube`);
`gl/jit.gl.simple` is ported separately as `median/examples/jit_gl_simple`.

## Coverage

The SDK has 108 examples (`source/<category>/<name>`). 72 are ported here, in 74 crates
(`advanced/dllexample` is `mydll`, `dllcounter` and `dllreport`). 3 are ported elsewhere in
`median/examples`: `simplemax`, `jit_scalebias` and `jit_gl_simple`. The other 33 are not ported;
[GAPS.md](GAPS.md) lists them under "Not ported" with what blocks each.

## Gaps

Where median has no safe API for something an example needs, the port calls `max_sys` directly,
in an `unsafe` block marked `// GAP:`. [GAPS.md](GAPS.md) lists every gap and the examples that
hit it.

## Building and testing

This directory is its own cargo workspace. Each crate tests its port against `median-mock`
(no Max needed):

```
cargo test
cargo make --profile release package   # one package with every external, macOS
cargo make --profile release install   # copies it to ~/Documents/Max 9/Packages
```

The mock does not implement the functions behind most gaps, so the gap paths are untested;
nothing here has been run in Max.
