# Changelog

## 0.1.2

- Fix clock creation to use the owning object's scheduler and free clocks when their handles are dropped.
- Pass console messages as literal text rather than printf format strings.
- Use real atomics for `Float64` and `Int64`, preserving their existing API.
- Honor over-aligned allocations in `MaxAllocator` and correct the sysmem test stubs.
- Fix buffer notification output pointers, symbol reads and non-UTF-8 display, offset attribute defaults, attribute getter allocation ordering, and the private DSP callback's C ABI.
- Rename example Cargo configuration files to `config.toml`.
- Default package installation to `Documents/Max 9/Packages` on macOS and Windows. Set the `MAX_VERSION` environment variable to `Max 8` to install for Max 8 instead.

Public APIs are unchanged. The broader clock lifetime and API changes from the review branch are not included.
