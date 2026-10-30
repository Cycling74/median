# Max SDK examples in median

Ports of the C examples in the Max SDK (`max-sys/thirdparty/max-sdk/source`) to median, one crate
per example, laid out like the SDK: `basics/plussz` ports `source/basics/plussz/plussz.c`. Read
the two side by side to see how an SDK idiom is written with median.

`basics/simplemax` is ported separately as `median/examples/simplemax`.

## Names

Every class is registered with a `median.` prefix (`median.plussz`, `median.times~`), so the
ports can be loaded next to the C builds. Crate names replace `.` with `_` and `~` with `_tilde`;
the Max class name is the crate's `[package.metadata.max] name`.

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
