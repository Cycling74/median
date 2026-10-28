# Plan, step 1: risk-reduction fixes that keep the public API

Goal: remove every UB path that can be removed without changing the public API, so `develop` is
safe to keep shipping while the redesign happens. This step does **not** make the crate sound:
the blanket `MaxObj for T` (H3), `SymbolRef::assign` (H5), `BufferLocked`'s `&mut [f32]` (H7) and
escaping outlet/clock handles (M6, M10) need new API shapes and stay until step 2. Everything here is source-compatible for the three examples and
the template, or changes a signature only in a way the examples never used. Items that need a new
API shape (`SymbolRef`, `BufferLocked` sample access, `ObjRef`) are in
[plan-api-redesign.md](plan-api-redesign.md) and are only stop-gapped here.

Finding numbers refer to [review-2026-10.md](review-2026-10.md).

## Order and dependencies

```
0. ABI fixes (H11, H12, H13)             tiny, independent, do first
1. lints + unsafe markers (H9)          no behaviour change, makes the rest reviewable
2. atomics (H4)                          independent
3. callbacks Send+Sync (H8)              independent
4. perform64 / dsp64 on &self (H1, H2)   independent
5. buffer notify write (H6)              independent
6. ObjBox field private, drop Wrapper::drop (H10, M5)   independent
7. clock lifecycle (M6)                  independent
8. Slice fixes (M4), MaxAllocator align  independent
9. attr::set HRTB (M3), file.rs (L)      independent
10. panic policy at the FFI boundary (M7)  touches median_macros; do last
```

Each item is one commit. Run `cargo test -j6` and `cargo clippy -j6 -- -D clippy::not_unsafe_ptr_arg_deref`
after each; build `median/examples/simp`, `hello_dsp`, `multi` with `cargo make build` at the end.

## 0. ABI fixes (H11, H12, H13, M9)

- `max.rs`, `object.rs`: call `post`/`error`/`object_post`/`object_error` as
  `(c"%s".as_ptr(), msg.as_ptr())`. Never pass user text as the format.
- `attr.rs`: `AttrTrampGetMethod<T> = extern "C" fn(&T, *mut c_void, *mut c_long, *mut *mut t_atom) -> t_max_err`,
  same for set; `get_nop`/`set_nop` return `MAX_ERR_NONE`; `tramp.rs` getter/setter generators
  return the helper's result. `AttrBuilder::build`: pass `None` (not the no-ops) when a method is
  absent, so offset attributes get the SDK defaults.
- `wrapper.rs:740,829`: `maxvectorsize: c_long, flags: c_long`.

Acceptance: `simp`'s `foo`/`blah` attributes read back correctly in the inspector; a
`new_offset` attribute on a test object reads and writes the field; `post!("{}", "100%s")` prints
literally.

## 1. Lints and `unsafe` markers (H9)

- Add to `median/Cargo.toml`:
  ```toml
  [lints.rust]
  unsafe_op_in_unsafe_fn = "warn"
  [lints.clippy]
  not_unsafe_ptr_arg_deref = "deny"
  missing_safety_doc = "warn"
  ```
- Mark `unsafe fn` and add `# Safety` sections: `attr::get`, `attr::set`, `method::sel_list`,
  `Proxy::new`, `Proxy::get_inlet`, `Outlet::append_*`, `thread::defer`, `thread::defer_low`,
  `object::post`, `object::error`, `Slice::from_raw_parts_mut`, `SymbolRef::new`,
  `From<*mut t_symbol> for SymbolRef` (replace with an `unsafe fn from_raw`),
  `WrappedBuilder::new_max`/`new_msp` (make `pub(crate)`), `WrapperInternal::new`/`assist`
  (make the trait `pub(crate)` or sealed), `Notification::new`.
- The generated trampolines (`tramp.rs`) are `pub extern "C" fn`s taking raw atom pointers.
  Generate them as `pub unsafe extern "C" fn`; the `Method` aliases are already `unsafe extern`.
  Wrapping the body in `unsafe { }` without changing the fn's own contract is not a fix.
- Update the generated trampolines in `median_macros/src/tramp.rs` and the `int_float_tramps!` /
  `register_ft_in` code to call them inside `unsafe { }` blocks. The `post!`/`object_post!` macros
  route through `object::post` with a pointer that came from `max_obj()`; keep a safe `post` that
  takes `&impl MaxObj` (now `MaxObjHeader`) and make the raw-pointer one unsafe.

Acceptance: clippy reports zero `not_unsafe_ptr_arg_deref`; examples build unchanged.

## 2. Atomics (H4)

Replace `median/src/num.rs`:

```rust
#[repr(transparent)] pub struct Float64(AtomicU64);
#[repr(transparent)] pub struct Int64(AtomicI64);
impl Float64 {
    pub const fn new(v: f64) -> Self { Self(AtomicU64::new(v.to_bits())) }
    pub fn get(&self) -> f64 { f64::from_bits(self.0.load(Ordering::Relaxed)) }
    pub fn set(&self, v: f64) { self.0.store(v.to_bits(), Ordering::Relaxed) }
}
```

- Keep `Default`, `Clone`, `Display`, `From`, `Into` impls; remove `unsafe impl Send/Sync`.
- Gate on `#[cfg(target_has_atomic = "64")]` instead of `target_arch`.
- Add `Float32` (`AtomicU32`) while here; the jitter branch wants it.
- Document the ordering contract: Relaxed is for a single scalar read as "latest value"; publishing
  anything multi-word needs Release/Acquire.

Acceptance: `num::tests` pass; `cargo asm` or `--emit asm` shows plain `str`/`ldr` for `set`/`get`
(optional, for your own confidence).

## 3. `Send + Sync` on stored callbacks (H8)

- `inlet.rs`: `pub type FloatCB<T> = Box<dyn Fn(&T, f64) + Send + Sync>;` same for `IntCB`.
- `builder.rs` `with_clock(&mut self, func: Box<dyn Fn(&T) + Send + Sync>)`.
- `clock.rs` `ClockInner::target: Option<(SyncPtr<t_object>, Box<dyn Fn(*mut t_object) + Send + Sync>)>`
  where `SyncPtr` is a crate-private `#[repr(transparent)]` newtype with a justified
  `unsafe impl Send + Sync` (the pointer is only dereferenced on the scheduler thread by the
  callback, and only while the owner is alive, see item 7). `ObjWrapped: Sync` means `ClockInner`
  itself must stay `Sync`, so the raw pointer cannot simply lose its impl.

Acceptance: `simp` still compiles (its closures capture nothing).

## 4. `perform64` and `dsp64` on `&self` (H1, H2)

- `MSPWrapperInternal`: change `ins: Vec<MaybeUninit<&'static [f64]>>` / `outs` into one
  `UnsafeCell<PerformScratch>` with the documented invariant "MSP never runs two perform calls for
  one object concurrently, and never runs perform concurrently with dsp64". Or avoid the storage:
  build the slice tables on the stack with a fixed cap (`[MaybeUninit<&[f64]>; 64]`) and
  `object_error` + return if `numins`/`numouts` exceed it.
- Signatures: `extern "C" fn perform64(&self, ..)`, `extern "C" fn dsp64(&self, ..)`,
  `MSPWrapperInternal::perform64(&self, ..)`. The `transmute` of the pointer-to-fn types in
  `register` and `dsp64` must change `&mut Self` to `&Self` accordingly.
- Remove `fn dsp_in_place() -> bool` from `MSPObjWrapped`. Always set `Z_NO_INPLACE`. If someone
  needs in-place later, add an `unsafe trait InPlace` with a `perform_in_place(&self, bufs: &mut [&mut [f64]])`
  signature that cannot alias.
- `free_wrapped`/`free_msp` keep `&mut self`: Max guarantees `free` is the last call.
- Replace the two `assert!`s at the top of `perform64` with a bounds clamp (`min(numins, cap)`); an
  assert there aborts Max from the audio thread.

Acceptance: `hello_dsp` and `multi` build and pass audio in Max (manual); `cargo test` passes.

## 5. Buffer notification write (H6)

`buffer.rs:124-132`:

```rust
let mut name: *mut max_sys::t_symbol = std::ptr::null_mut();
max_sys::object_method(notification.data(), GET_NAME.inner(), &raw mut name as *mut c_void);
```

Also make `notify_if(&self)` (it took `&mut self` for no reason) and keep `notify_if_unchecked` unsafe.

Acceptance: a `buffer~ foo` bound after the external is created still triggers `exists()` to flip
to true (manual test in Max with `hello_dsp`).

## 6. `ObjBox` and `Wrapper::drop` (H10, M5)

- `object.rs`: `pub struct ObjBox<T: MaxObjHeader> { value: NonNull<T> }`, private. Remove `DerefMut`;
  keep `Deref`. `into_raw` returns the pointer and `mem::forget`s. `Drop` calls `object_free`.
- Remove `impl Drop for Wrapper`. Nothing drops a `Wrapper` by value once `ObjBox` holds `NonNull`.
- `Wrapper::wrapped_mut(&mut self)` becomes `pub(crate)`; the only caller is `ClockHandle::new`,
  which runs before the object is visible to Max.

Acceptance: `clock.rs` compiles with `clock_target.wrapped_mut()` replaced by an `unsafe` crate-internal
call; examples unchanged.

## 7. Clock lifecycle (M6)

- `ClockHandle::drop`: `clock_unset`, then `max_sys::object_free(self.clock)` (the SDK says
  `freeobject()`; `object_free` is the modern spelling). Then drop `_target`.
- Use `clock_new_withscheduler(target, fn, scheduler_fromobject(owner))` instead of the `#S` obex
  store, when the scheduler is non-null.
- Lifetime: `!Send` alone does not help (a handle can still escape into thread-local storage), and
  a null check in the callback races with teardown. Do the real fix here, it is small: `ClockInner`
  holds `Mutex<Option<(SyncPtr<t_object>, Box<dyn Fn ..>)>>`; the callback locks for the duration of
  the call; `Wrapper::free_wrapped` locks and takes the target *before* dropping `T`, so an
  in-flight callback finishes first and a later one finds `None`. A stray handle then fires into a
  no-op. Same pattern is reused for outlets in step 2.

Acceptance: create/delete a `simp` box 1000 times in a Max loop and watch memory stay flat.

## 8. `Slice<T>` and `MaxAllocator` (M4)

- `Slice::new_with_length` and `From<U>`: use `size_of::<T>().checked_mul(len).expect(..)` then
  `ptr::write` per element. In `From<U>`, count how many items the iterator actually yielded and
  set the slice length to that count (do not trust `len()`).
- `Drop`: `ptr::drop_in_place` the initialised elements before `sysmem_freeptr` (today `Slice<T: Drop>`
  leaks every element).
- `Slice::from_raw_parts_mut`: `unsafe fn`.
- Alignment: `assert!(align_of::<T>() <= 16)` at construction, documented, until a
  `sysmem_newptr`-based aligned allocator exists.
- `MaxAllocator::alloc`: if `layout.align() > 16`, over-allocate by `align` and store the original
  pointer just before the aligned block; `dealloc` reads it back. Or return null for `align > 16`
  and document it. Implement `realloc` via `sysmem_resizeptr` for the aligned-OK case.

Acceptance: `slice::test::can_create` extended with a `T: Drop` type (`String`) and with an
iterator that lies about `len()`; both pass. Fix `median/src/test/sysmem.rs` first (M11): back the
stub with `libc::malloc`/`free`, or store the size in a header, so the tests themselves are not UB.

## 9. `attr::set` HRTB and `file.rs` (M3, L)

- `pub unsafe fn set<T, F>(ac, av, setter: F) where F: Fn(T), T: for<'b> From<&'b Atom>`.
- `file.rs::locate`: reject names longer than `MAX_PATH_CHARS - 1` with `LocateError::NameTooLong`;
  copy with `copy_from_slice` on `c_char`, not `as i8`.
- `file.rs::read_text`: check `sysfile_readtextfile`'s return; check `*texthandle` non-null before
  `CStr::from_ptr`; close the file handle on every path.
- `error.rs`: replace `println!` with `max_sys::error`.
- `SymbolRef::fmt`: `to_string_lossy`.

## 10. Panic policy at the FFI boundary (M7)

- `median_macros/src/external.rs`: `ext_main` on panic posts via `max_sys::error` ("median: failed
  to register <class>") and returns; never `process::exit`.
- Every generated trampoline body (`tramp.rs`, the `int_float_tramps!` macro, `new_tramp`,
  `handle_notification_tramp`, `assist_tramp`, `perform64`, `dsp64`, `free_*`) wraps its call in
  `std::panic::catch_unwind(AssertUnwindSafe(|| ..))`. On `Err`, `object_error` the payload string
  if it is a `&str`/`String`, and return the neutral value (`null` from `new_tramp`, zeros from
  `perform64`, nothing elsewhere). A helper `median::ffi::guard(obj, || ..)` keeps this in one place.
- `new_tramp`: on panic in `T::new`, do not drop the `ObjBox` through `free_wrapped`
  (the internals are uninitialised). Mark the wrapper "constructed" only after `T::new` returns,
  and have `free_wrapped` skip the inner drop otherwise.
- `templates/max/Cargo.toml.liquid`: `panic = "unwind"` in `[profile.release]` with a comment
  explaining that `abort` would turn any panic into a Max crash.

Acceptance: an example method that deliberately panics prints to the Max console and the patch
keeps running.

## Status (2026-10-21, branch `xnor/soundness-fixes`)

Items 0-10 are implemented, one commit each, plus fixups from two review passes. `cargo test`,
clippy with `not_unsafe_ptr_arg_deref` denied, and builds of `median_macros` and the three examples
pass. Item 10 was checked in Max with temporary panics in `hello_dsp~`: a panic in a message
method, an attribute getter and setter, a clock callback and `perform` each posted to the console
and the patch kept running (`perform` went silent until DSP restarted). The other acceptance checks
that need Max have not been run.

Where the implementation differs from the text above:

- Edition 2018 has no `c"..."` literals; the `"%s"` format is a `b"%s\0"` constant in `max.rs`.
- Item 1: `Proxy::get_inlet` became `unsafe`, so `simp`, the template and the README wrap that call
  in `unsafe`. `attr::touch_with_name` is also `unsafe` now (safe `attr_touch_with_name` on
  `MaxObj`/`MSPObj`). `From<*mut t_symbol> for SymbolRef` is replaced by `unsafe SymbolRef::from_raw`.
- Item 0/M9: absent accessors are passed as `None` only for offset attributes; non-offset
  attributes keep the no-ops, because `attribute_new`'s defaults store into class-shared memory.
- Item 4: signal i/o above 64 channels skips user `perform` and silences all outlets. A panic in
  `perform` (or `dsp_setup`) silences the object until DSP is restarted.
- Item 6: `ObjBox::alloc` returns `Option`; a null `object_alloc` makes `new` return null to Max.
- Item 7: clocks hold an `Arc<ClockShared>` (mutex held during the callback, `armed` flag set only
  after construction). The free trampolines take `*mut Self`, clear the clocks, then drop the
  internals in place. `clock_new_withscheduler` replaces the `#S` obex store.
- Item 8: the alignment check is a post-monomorphisation compile error, not a runtime assert.
  `Slice::new_with_length`/`From` can still panic on size overflow or a null allocation.
- Item 10: the examples' release profiles also switch to `panic = "unwind"`. `WrappedDefer::defer`
  and `defer_low` are now `unsafe` (the object must outlive the deferred call).

Known remaining issues found in review, not fixed here:

- A clock callback that frees its own owner (or drops its own `ClockHandle`) deadlocks instead of
  using freed memory. Documented on `ClockShared`. (Since the redesign's step 7 review fixes the
  lock is reentrant, so a callback re-entered on the same thread runs nested, and only this case
  blocks, deliberately; see `doc/plan-api-redesign.md`.)
- A clock callback that Max has dispatched but that hasn't taken the `ClockShared` lock yet still
  reads the `ClockInner` object, which `ClockHandle::drop` can free in that window: `clock_unset`
  and `freeobject` aren't documented to wait for a running callback. The window is a few
  instructions. Fixing it needs to know whether Max runs clock callbacks inside the global critical
  region (then take `critical_enter(0)` around unset/clear) or a different ownership scheme.
- Possible cross-thread deadlock: the callback holds the `ClockShared` lock while calling into Max;
  if Max's box-deletion path holds a Max lock the callback needs while `free` waits on
  `ClockShared`, both threads block. Test in Max: Overdrive on, a clock that outputs and re-delays
  itself every 1 ms, delete and recreate the box in a loop with `thispatcher`, watch for a hang.
- ~~An MSP object whose `T::new` panics after `z_dsp_setup` never gets `z_dsp_free` (likely a
  leak).~~ Fixed in redesign step 5: the builder pairs `z_dsp_setup` with `z_dsp_free` until the
  object is constructed (see plan-api-redesign Status).
- ~~`object_post!`/`object_error!` still accept any pointer from safe code; fixing it needs a
  signature change, so it goes with step 2.~~ Fixed in redesign step 1: they take `&ObjRef` (see
  plan-api-redesign step 1).
- Buffer `getname` (item 5): the headers don't say whether it writes through the argument or
  returns the symbol. Test a late-bound `buffer~` with `hello_dsp`.

## Out of scope for step 1 (handled in the redesign)

After step 1 the crate is still unsound through these paths; say so in the README until step 2 ships.

- `SymbolRef` as a `Copy` newtype over `NonNull` (H5): changes `assign` callers in `BufferRef`.
- `BufferLocked` sample access through `Cell`/raw views and interleaved layout (H7, M1).
- `ObjRef` handle replacing the blanket `MaxObj for T` (H3).
- Fallible `new` (M8).
