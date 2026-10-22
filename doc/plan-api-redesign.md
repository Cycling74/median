# Plan, step 2: the API redesign (breaking release)

Goal: an API that is sound by construction, roughly one line per Max concept (like min), and made
of ordinary Rust items that rust-analyzer and rustfmt handle natively. This is one breaking release;
`median 0.3` (0.2.0 is the step 1 soundness release). The three examples and the template are the acceptance tests and get ported as part of
the work.

Finding numbers refer to [review-2026-10.md](review-2026-10.md).

## Target shape

```rust
use median::prelude::*;

#[derive(MaxObject)]
#[max(name = "simp")]                        // kind defaults to Max; `msp` for t_pxobject
pub struct Simp {
    #[attr(clip(0, 100))]                    // attribute backed by the field, getter/setter generated
    value: Int64,
    fvalue: Float64,
    clock: Clock,
    list_out: Outlet<List>,
    obj: ObjRef,                             // explicit handle; the derive finds the field by type
}

impl MaxObjWrapped for Simp {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::default().assist("first inlet"));
        b.inlet(Inlet::float(|s: &Self, v| post!("float {v}")).assist("my float inlet"));
        b.inlet(Inlet::int(Self::int));
        b.inlet(Inlet::proxy().assist("my proxy inlet").hot());
        Ok(Self {
            value: Int64::new(0),
            fvalue: Float64::new(0.),
            clock: b.clock(Self::clocked),
            list_out: b.outlet(Outlet::list().assist("my list out")),
            obj: b.obj(),
        })
    }
}

#[median::methods]
impl Simp {
    #[bang] pub fn bang(&self) { self.obj.post(format!("bang {}", self.value)); self.clock.delay(10); }
    #[int]  pub fn int(&self, v: t_atom_long) { self.value.set(v); self.obj.attr_touch("value"); }
    #[list] pub fn list(&self, atoms: &[Atom]) { .. }
    #[any]  pub fn baz(&self, sel: &SymbolRef, atoms: &[Atom]) { .. }
    #[sel("set", defaults = 0)] pub fn set(&self, name: SymbolRef) { .. }

    #[attr]                                  // accessor attribute; paired with `set_foo` by name
    pub fn foo(&self) -> f64 { self.fvalue.get() }
    pub fn set_foo(&self, v: f64) { self.fvalue.set(v) }

    pub fn clocked(&self) { let _ = self.list_out.send(&[1i64.into(), 12f64.into()]); }
}

median::ext_main! { Simp }                   // macro_rules; expands to Simp::register()
```

## Pieces

### A. `#[derive(MaxObject)]` (replaces the struct half of `external!`)

Generates:

- `impl ObjWrapped for Simp { type Wrapper = MaxObjWrapper<Simp>; fn class_name() -> &CStr { c"simp" } fn class_type() .. }`.
  `MaxObjWrapped: ObjWrapped<Wrapper = MaxObjWrapper<Self>>` and the MSP twin, so a `kind`
  mismatch between the derive and the trait impl is a normal type error.
- `impl Simp { pub fn register() { <Self as ObjWrapped>::Wrapper::register() } }`.
- Field attributes `#[attr(..)]` register offset-free attributes with generated accessors
  (`extern "C" fn __attr_get_value<W>(..)` reading the field through `wrapped()`), with options
  `name`, `clip`, `label`, `style`, `category`, `order`, `default`, `readonly`, `hidden`, `enum`.
  Because accessors are generated there is no raw offset into the struct and no `field-offset`.
- Finds the `ObjRef` field (by type) so the wrapper can fill it in after allocation (see C).
- Collects `#[max(description = "..", author = "..", tags = "..")]` for maxref generation (later).

The derive never sees method bodies. The redundant `T` parameter on `ObjWrapped<T>`,
`MaxObjWrapped<T>`, `MSPObjWrapped<T>` goes away.

### B. `#[median::methods]` on the inherent impl (replaces the impl half)

- Re-emits the impl verbatim with the marker attributes stripped (original spans, so IDE features
  inside method bodies keep working; this is the `#[pymethods]` / `#[wasm_bindgen]` pattern).
- Emits a sibling `impl median::Methods for Simp { fn register<W: WrapperWrapped<Self>>(c: &mut Class<W>) { .. } }`
  with one `c.add_*` per marker. It has to be a trait impl, not an inherent fn: the wrapper is
  generic over `T` and can only call methods it knows through a bound (`MaxObjWrapped: Methods`).
  The wrapper's `register()` calls `T::register(&mut c)` then the optional `T::class_setup(&mut c)`.
  A type with no `#[methods]` block writes `impl Methods for Foo {}` (default empty body), or the
  derive emits it when given `#[max(no_methods)]`.
- Return types: a marked method may return `()` or `Result<(), E>` where `E: Display`. For `Result`,
  the generated trampoline calls `ObjWrapped::report_error(&self, &dyn Display)` on `Err`, whose
  default is `object_error` on the object. Overriding it lets an external route errors to
  `tracing`, a status outlet, or both, in one place (issue #17's pattern). `?` then works
  everywhere in user code.
- Markers: `#[bang]`, `#[int]`, `#[float]`, `#[sym]`, `#[list]`, `#[any]`, `#[sel("name", defaults = n)]`,
  `#[attr(..)]` on a getter (paired with `set_<name>` if present), `#[defer]`/`#[defer_low]` variants
  that register a deferring trampoline, `#[dblclick]`, `#[loadbang]`, `#[notify]`, `#[savestate]`
  (the min special messages, added as the wrapper grows support).
- Signature checks are done here with real `syn::Error` spans ("`#[int]` method must take `(&self, t_atom_long)`").
- One `#[methods]` block per type to start. If more are wanted later: pyo3's autoref-specialization
  trick (also makes the block optional) or `inventory`.
- `W` is never written by the user; it is inferred from `Class<W>` at registration. The trampolines
  are generic `extern "C"` fns, one monomorphisation per wrapper type.

### C. Trampolines without macros: ZST callables

`Class<W>` gets typed registration methods that take any zero-sized, `Copy` `Fn` (a fn item or a
non-capturing closure) and build the `extern "C"` trampoline generically. The `Copy` bound
matters: zero size alone does not rule out a closure that captured a zero-sized value with a
`Drop` impl, which the trampoline would then fabricate and drop on every call. Closures are
`Copy` only if every capture is `Copy`, and fn items are always `Copy`, so `F: Fn(&T) + Copy +
Send + Sync + 'static` is exactly "stateless callable":

```rust
unsafe fn zst<F>() -> F {
    const { assert!(size_of::<F>() == 0, "callback must be a non-capturing closure or fn item") };
    MaybeUninit::<F>::uninit().assume_init()
}
extern "C" fn bang_tramp<W, T, F>(w: &W) where W: WrapperWrapped<T>, F: Fn(&T) + Copy {
    ffi::guard(w, || (unsafe { zst::<F>() })(w.wrapped()))
}
impl<W> Class<W> {
    pub fn add_bang<T, F, R>(&mut self, _: F) where W: WrapperWrapped<T>, F: Fn(&T) -> R + Copy + Send + Sync + 'static, R: MethodResult { .. }
    // MethodResult is implemented for () and Result<(), E: Display>; Err is routed to T::report_error.
    pub fn add_int<T, F: Fn(&T, t_atom_long)>(&mut self, _: F) where .. { .. }
    pub fn add_sel<T, F, A: SelArgs>(&mut self, sel: &CStr, _: F, defaults: usize) where F: Fn(&T, A) { .. }
}
```

`#[methods]` emits calls to these, and users can call them by hand in `class_setup` for
conditional or dynamically named methods. The hand-written form is a supported, documented
style, not a fallback: `cargo expand` must never be the way to learn the crate (see "Lessons from
issue #17" below). Ship `examples/simp_explicit` written entirely without `derive`/`#[methods]`. Verified to compile on 1.94 with a compile-time error for
capturing closures. The `build.rs` that generates `Method::SelIS..` permutations becomes a
`macro_rules!` over tuples implementing `SelArgs`.

### D. `ObjRef` handle (fixes H3, removes offset arithmetic)

```rust
pub struct ObjRef(NonNull<max_sys::t_object>);   // deliberately not Clone or Copy
impl ObjRef { pub fn post(&self, msg: impl Display); pub fn error(&self, ..); pub fn warn(&self, ..);   // warn = object_warn, missing today
              pub fn attr_touch(&self, name: &CStr) -> MaxResult<()>; pub fn notify(..); pub fn as_ptr(..) }
```

- The builder hands it out once (`b.obj()`); Max allocated the object before `T::new` ran, so it is
  valid for the object's whole life. Lifetime is enforced by the borrow checker, not by convention:
  `ObjRef` is not `Clone`/`Copy`, every method takes `&self`, and the only way to reach it is
  through `&T` inside a trampoline, whose lifetime ends with the call. Nothing in the API hands out
  `&mut T` or lets `T` be moved out of the wrapper, so the handle cannot be returned or stashed.
  (A `Copy` handle would defeat this; that was a mistake in the first draft.)
  Limit (2026-10-21): between `b.obj()` and returning `Self` from `new`, the handle is an owned
  value, and safe code can still leak it (`Box::leak` gives a `&'static ObjRef`, which is `Send`
  because `ObjRef` must be `Sync`) or park it in a `thread_local!`, then use it after the box is
  deleted. Making `ObjRef` `!Send` closes the `static Mutex<Option<ObjRef>>` route but not those
  two. The same holds for `Outlet<K>` (M10). Closing it fully needs a lifetime-branded handle,
  which `T` cannot store; we accept the deliberate-escape case and document it on `ObjRef`.
- Remove `unsafe impl<T: MaxObjWrapped<T>> MaxObj for T` and the `MSPObj` twin. `MaxObj` stays as the
  `unsafe trait` for the wrapper types themselves.
- `post!`/`object_post!` take `&ObjRef` (or anything `AsRef<ObjRef>`).
- Side effect: `field-offset` is no longer needed anywhere.

### E. Fallible construction (M8)

`fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError>` where `NewError: From<String>` and
`Display`. On `Err`, the wrapper posts `object_error` with the message and returns null from
`new_tramp`; Max shows the box with a red outline, as it does for C externals. `?` on
`SymbolRef`/`Atom` conversions in constructors becomes natural.

### F. Iolet descriptors (fixes the assist bugs and enables inletinfo)

```rust
pub struct Inlet<T> { kind: InletKind<T>, assist: Option<CString>, hot: bool }
impl<T> Inlet<T> { pub fn default() -> Self; pub fn proxy() -> Self; pub fn int(f: impl Fn(&T, t_atom_long) + Send + Sync + 'static) -> Self;
                   pub fn float(..) -> Self; pub fn signal() -> Self /* MSP only */; pub fn assist(self, &str) -> Self; pub fn hot(self) -> Self }
pub struct Outlet<K: OutletKind> { .. }  // Outlet<Bang>, Outlet<Int>, Outlet<Float>, Outlet<List>, Outlet<Any>, Outlet<Signal>
```

- `b.inlet(desc) -> InletId`, `b.outlet(desc) -> Outlet<K>` (concrete type, **not** `Clone`/`Copy` for
  the same reason as `ObjRef`; `send(&self, ..)` returns `Result<(), StackOverflow>`; replaces the
  `Box<dyn SendValue + Sync>` aliases, fixes M10).
- `finalize` computes positions: for MSP, the first `n` entries must be `signal()` (error otherwise)
  and they occupy positions `0..n`; `z_dsp_setup` runs first, then the remaining inlets are created
  right to left (the order `z_dsp.h` prescribes). `in<N>`/`ft<N>` ids are allocated 1..9 independent
  of position. Assist and hot/cold are stored by position.
- `assist` and `inletinfo` trampolines read the table. `inletinfo` is registered only when some
  inlet is `.hot()`; the default matches `stdinletinfo`. Dynamic override stays on `ObjWrapped`:
  `fn assist(&self, iolet: IoLet) -> Option<Cow<'_, str>>`; the wrapper does the C conversion and
  copies with the SDK's 512 limit.
- The `_with_assist` builder methods and `with_default_inlet_assist` go away.
- Why both a table and a hook (from a survey of ~930 `assist` registrations in the kernel source,
  2026-10-21): about 80-85% are fixed strings per index; 100-120 depend on configuration fixed at
  construction (argument counts, `@chans`/mc flags, `route`/`pack` argument lists, gen/poly
  patcher names), which the builder covers because `new` sees the args; 40-60 depend on runtime
  state (`gate` "Currently Open", `receive` "currently <name>", `mc.mixdown~` active channels,
  `pvar` live target lookup, `jit.gl.*` outlet 0 following the `matrixoutput` attribute, `js`
  `setinletassist` callbacks). Those need the hook. No object was found whose assist depends on
  patch connections.
- Constraints on the hook, from how Max calls it (verified against the kernel source): always
  main thread; called on every hover, every box paint (once per
  inlet) and connect-menu enumeration; never cached; the smallest destination buffer is 512 bytes
  (box paint path) and kernel objects write at most `ASSIST_MAX_STRING_LEN` (500). So the hook
  must be cheap and lock-free with respect to the audio and scheduler threads (atomics, not a
  `Mutex` an audio callback might hold), may allocate, and the wrapper truncates to 500.
  `inletinfo` is read the same way, live, so hot/cold gets the same `Option` override
  (`gate`/`switch` flip it at runtime).
- Default when an object has no `assist` method: the kernel synthesises "<description> inlet N
  (types)" from the box description attribute and inlet/outlet type symbols. median's current
  default "inlet N" is weaker than registering nothing; the table should emit at least the type.
- Message-list noise (2026-10-21): every class currently gets `ft1`..`ft9` and `in1`..`in9`
  registered up front (`register_ft_in`), because the inlets are declared in `new`, after the class
  exists, and all 18 show in the object's message menu. Two parts: (a) mark every wrapper-internal
  method `undocumented` (`CLASS_METHOD_ATTR_PARSE(c, "ft1", "undocumented", gensym("long"), 0, "1")`,
  i.e. `class_extra_lookup(c, "methods")` + `hashtab_lookup` + `object_addattr_parse`; the SDK's
  `ext_obex_util.h` documents it as "makes a method invisible to users", `dummy.c` uses it). Has to
  be checked in Max: the header's other comment only promises the ref-page generator ignores it.
  (b) Typed `int`/`float` inlets become proxies dispatched through the class's own `int`/`float`
  methods by `proxy_getinlet`, so no `inN`/`ftN` exist at all. Cost: since inlets are declared
  after the class exists, `int` and `float` are always registered (2 menu entries instead of 18),
  and an `int`/`float` arriving on an inlet with no handler posts the usual "doesn't understand"
  error from the wrapper. Proxies are also somewhat heavier than `intin`/`floatin`.
  Do (a) in step 4 regardless; (b) if testing shows (a) does not hide them from the menu.
- Dynamic inlet/outlet counts: ~20 kernel objects (`gain~`, `gen~`, `poly~`, `mc.wrapper~`, `js`)
  add or remove iolets after construction through the `dynlet_begin`/`dynlet_end` box protocol.
  Not in scope now, but keep the iolet table behind an `RwLock` (or interior-mutable `Vec`) so a
  later `dynlet` API can update it rather than requiring a second table.
- Later: the same table feeds maxref generation with inlet/outlet descriptions (min cannot do this).

### G. `SymbolRef` (H5) and `BufferLocked` (H7, M1)

- `#[derive(Copy, Clone, PartialEq, Eq, Hash)] pub struct SymbolRef(NonNull<t_symbol>)`; `unsafe fn from_raw`,
  `From<&CStr>`, `TryFrom<&str>`, `Default` = `s_nothing`. No interior mutability; `BufferRef` keeps its
  name in the mutex it already has. A `sym!("foo")` macro validates the literal at compile time
  (or document `SymbolRef::from(c"foo")`).
- `BufferLocked` (perform-time claim): `buffer_locksamples` is a shared claim, so another object or
  `record~` may write concurrently. `Cell<f32>` would allow aliasing but still performs non-atomic
  accesses, so it is not the answer. Expose samples as `&[AtomicSample]`, a `#[repr(transparent)]`
  `AtomicU32` newtype with Relaxed `get() -> f32` / `set(f32)` (plain loads/stores in codegen),
  via `frame(i)` (contiguous, interleaved), `channel_iter(ch)` (strided), `sample(frame, ch)`.
  Plus `unsafe fn samples_ptr() -> *mut f32` for people who want raw speed and accept the C-level
  race. `BufferEdit` wrapping `buffer_edit_begin/end` is the exclusive mode and gives real
  `&mut [f32]` plus `resize`. Add `buffer_modified` notification forwarding and the
  `set`/`dblclick` convenience messages min has.

### H. Clock, defer, queue, fifo

- `Clock` (renamed `ClockHandle`): callback is `fn(&T)` or a ZST closure; the clock target holds the
  owner pointer under a `Mutex`, the callback holds the lock for the call, and the wrapper's `free`
  takes and clears it before dropping `T`. That is the synchronised teardown step 1 item 7 adds; a
  stray handle fires into a no-op and an in-flight callback finishes before the object goes away.
  `delay`, `fdelay`, `cancel`, `defer_delivery()` option (min's `defer_delivery`).
- `Queue` (qelem) with the same shape. `Fifo<T>` as a thin wrapper over a lock-free SPSC
  (`rtrb` or `ringbuf`), for audio-thread to main-thread handoff.
- `#[defer]`/`#[defer_low]` markers on methods generate the deferring trampoline; no more hand-written
  `DeferMethodWrapped` transmutes.

### I. Attributes

`AttrBuilder<W>` gains `label`, `style`, `category`, `order`, `default`, `readonly`, `save`, `enum`,
`range` (min's option set) on top of `clip` and visibility; the derive and `#[attr]` emit calls to
it. Keep `touch_with_name` on `ObjRef`.

### J. Jitter: how the redesign absorbs `feature/jitter`

What the branch builds (read from `jit/matrix/mod.rs`, `jit/ob3d/mod.rs`, the `wrapper.rs` diff and
both examples):

- Two objects per external, as in the C SDK: a Jitter object (`jit::matrix::Wrapper<T>` or
  `jit::ob3d::Wrapper<T>`, registered with `jit_class_new`, its own `CLASSES` map, a blanket
  `JitObj for T` that recovers the object pointer by offset arithmetic) and a Max object
  (`MaxObjWrapped`) that glues the two with `max_jit_object_wrap_complete`, `max_jit_mop_setup_simple`
  / `max_jit_ob3d_attach`, `max_jit_class_obex_setup`, `max_jit_class_mop_wrap`,
  `max_jit_class_wrap_standard`, and frees in reverse in a new `ObjWrapped::destroy(&mut self)`
  hook. All of that glue is hand-written `unsafe` in the examples (~60 lines each) and is identical
  between them apart from MOP vs ob3d.
- Core changes to support it: an `obex: MaybeUninit<*mut c_void>` field on `Wrapper`, `destroy`,
  and `wrapped_byte_offset`/`obex_byte_offset` added as *required* methods on `MaxObj`/`MSPObj`.
- A near-copy of `attr.rs` as `jit/attr.rs` (Jitter attribute accessors have the same C signature;
  only the atom allocation differs, `jit_getbytes` vs `sysmem_newptr`), with its own two tramp
  proc-macros.
- `WrapperInner::calc(&mut self)` reached through `unsafe fn inner(x) -> &'a mut` (the H1 pattern
  again, with the branch's own `//XXX can this be called from multiple threads?`), plus the iter /
  parallel / 32-bit-long issues listed in the review.

How each redesign piece lands on it:

1. **`ObjRef` replaces both blanket offset impls.** The jit builder hands `T::new(&mut JitBuilder)`
   a `JitObjRef`; `jit_obj()` comes from the field. `JitObj for T`, `wrapped_byte_offset` and
   `obex_byte_offset` disappear. The obex offset is `offset_of!(Wrapper, obex)` computed inside the
   wrapper and passed to `class_obexoffset_set`; nothing user-facing.
2. **The `obex` field becomes core, not Jitter-only.** Every Max object gets `obex` and
   `class_obexoffset_set` at registration. That is what enables `object_obex_dumpout` (the standard
   `dumpout` outlet) and `object_obex_lookup("#P"/"#B")` for patcher/box access, both on the min
   gap list. Cost: one pointer per object. Do it in step D of this plan regardless of Jitter.
3. **`destroy` is subsumed by `Drop` on the user type.** Once `T` holds `ObjRef`, `impl Drop for T`
   can call `max_jit_mop_free` / `jit_object_free` / `max_jit_object_free` itself: the wrapper drops
   `T` *before* Max frees the object, so the pointer is valid throughout. For the generated glue
   (next item) the derive emits that `Drop`. The `destroy` hook and the `&mut self` it needed go.
4. **A third object kind generates the Max glue.** `#[derive(MaxObject)] #[max(name = "jit.median.scalebias", kind = matrix_op)]`
   (or `kind = gl`) on the *Jitter-side* struct generates both classes: the jit class from a
   `MatrixOp`/`Ob3d` trait impl (`calc` / `draw`), and a Max wrapper class whose `new`,
   `class_setup` and `Drop` are the glue the examples currently write by hand. The user writes one
   struct, as in min's `matrix_operator<>`. `ext_main! { JitScaleBias }` registers jit then Max.
   Attributes declared with `#[attr]` on fields go to `jit_class_addattr` (Jitter convention;
   `max_jit_class_wrap_standard` exposes them on the Max side); `#[methods]` markers on the
   Jitter-side impl register with `jit_class_addmethod`, Max-side message markers (`#[bang]`,
   custom selectors that are not matrix traffic) go to the Max class.
5. **One `AttrBuilder<W, Api>`.** The `Api` parameter (Max or Jitter) selects the allocator and
   `class_addattr` vs `jit_class_addattr`; the duplicated `jit/attr.rs` and its two proc-macros
   collapse into the generic accessor trampolines of item C.
6. **Assist and iolets.** `max_jit_mop_setup_simple` creates the matrix inlets/outlets and the
   dumpout itself, so for `kind = matrix_op` the builder must not also declare them. The generated
   Max class's `assist` delegates to `max_jit_mop_assist` first (it reads the live MOP io list) and
   falls back to the iolet table for any extra iolets the user adds; `kind = gl` does the same with
   `max_jit_ob3d_assist`. Registering `assist` by hand, as both examples do, is removed. The iolet
   table needs an "externally created" count so positions line up.
7. **`calc` on `&self`.** Same fix as `perform64` (step 1 item 4): input/output `Vec<Matrix>` behind
   an `UnsafeCell` with the invariant that Jitter does not re-enter `matrix_calc` for one object,
   or stack arrays sized `JIT_MATRIX_MAX_PLANECOUNT`. `Matrix` lock guards stay; `entry_iter` splits
   into `&self` / `&mut self` variants; the `long` type maps to `i32`.

Merge consequence: the branch's *core* changes (`obex`, `destroy`, the offset trait methods) are API
the redesign removes or reshapes, so merging them first would break downstream users twice. The
branch's *leaf* modules do not depend on the wrapper shape and can land early. See
[plan-branches-and-build.md](plan-branches-and-build.md) 3b for the split.

## Lessons from issue #17 (alisomay, rytm-external, Dec 2023)

The one substantial field report on median. The author abandoned `external!`, ran `cargo expand`
on `simp`, and built a large external from the expanded code. Each of the three points maps to a
design decision above; `rytm-external`'s source (`externals/rytm/rytm-external/src/`) was read to
check that the proposal actually covers what they had to write by hand.

1. "Writing in proc macro wrappers is not as useful as it looks" (no intellisense, no formatting,
   hard to learn the types). Addressed by A and B: the struct and every impl stay ordinary items;
   `#[methods]` re-emits its input. Addressed further by C: the macro-free form needs no
   trampolines at all (`c.add_bang(Self::bang)`), so the expanded code is short and is the
   documented reference style. Their `trampoline.rs` (hand-written `extern "C" fn`s calling
   `WrapperWrapped::wrapped(wrapper).int(v)`) disappears entirely.
2. Error handling: they return `Result<(), RytmExternalError>` from every method and post at the
   trampoline boundary; they also wrote a proc macro (`error-logger-macro`) to log at each `?`.
   Addressed by `Result`-returning markers plus the `report_error` hook (B, C). Their `Post` trait
   over `&str`/`String`/`SymbolRef` and the `unsafe { object_warn(..) }` they had to call directly
   are covered by `ObjRef::{post, error, warn}(impl Display)` (D).
3. Modularization (`class.rs`, `trampoline.rs`, `rytm.rs`, `lib.rs`): with one macro wrapping
   everything this was impossible. With A and B the derive, the `MaxObjWrapped` impl and the
   `#[methods]` impl may live in different modules; nothing requires them to be adjacent.

Things in their code that confirm review findings: `ext_main` copied from the macro, including
`std::process::exit(1)` on panic (M7 propagates to users); `#![allow(clippy::not_unsafe_ptr_arg_deref)]`
forced by median's raw-pointer API (H9); `class_attr_addattr_parse` called by hand to set the
`save` flag because `AttrBuilder` has no `save` (I); `AtomicIsize` used for attribute storage
instead of `Int64` (H4, good instinct); `SymbolRef::to_string().expect(..)` everywhere (L).

## Migration

- Port `simp`, `hello_dsp`, `multi`, `templates/max`, then `igk/examples/simplemax`.
- Write `doc/migration-0.3.md` from the diff of the examples: old form, new form, one line each. Done: [migration-0.3.md](migration-0.3.md).
- Keep `external!` for one release as a deprecated shim that expands to the new forms, if the
  cost is low; otherwise drop it and point at the migration doc.

## Order of implementation

0. Toolchain for the new code (pulled forward from step 5 of
   [plan-branches-and-build.md](plan-branches-and-build.md)): edition 2024 and `rust-version` for
   `median` and `median_macros`, `syn` 2 in `median_macros` and `median/build.rs`, version
   `0.3.0-dev`. The sketches here use `c"..."`, `const { }` and `unsafe extern`, and the new proc
   macros should not be written against `syn` 1. `max-sys` and the rest of step 5 stay where they are.
1. `ObjRef` + remove blanket `MaxObj` (D). Smallest change that unblocks the rest.
2. `Class<W>::add_*` ZST registration (C); keep the old `Method` enum underneath.
3. `#[derive(MaxObject)]` (A) and `#[methods]` (B); port `simp`.
4. Iolet descriptors (F); port `hello_dsp`, `multi`.
5. Fallible `new` (E).
6. `SymbolRef`, `BufferLocked` (G).
7. Clock/queue/fifo (H), attribute options (I).

Each step compiles the examples; the `median-mock` test crate from
[plan-branches-and-build.md](plan-branches-and-build.md) is what makes steps 4 onward testable
without Max.

## Status (branch `xnor/api-redesign`)

Started 2026-10-21 from `xnor/review` after step 1. One commit (or a short series) per item of
"Order of implementation"; each records here what it did and where it differs from the text above.

- [x] 0. Toolchain (2eb44da). `ext_main` now emits `#[unsafe(no_mangle)]`, accepted by 2018 user
  crates too. The examples and template stay on edition 2018 until they are ported.
- [x] 1. `ObjRef`, blanket `MaxObj`/`MSPObj` removed. Differences from section D: `ObjRef` also
  carries `PhantomData<*const ()>` (explicit `!Send`, `unsafe impl Sync`); it has `from_raw`
  (unsafe), `proxy_inlet` (wraps `proxy_getinlet`, safe but only meaningful inside a message
  method), `defer`/`defer_low` (unsafe, replacing `WrappedDefer`, which is removed), plus
  `try_register`/`try_attach`/`subscribe`/`attr_*touch_with_name` moved over from `MaxObj`;
  `attr_touch` is spelled `attr_touch_with_name` as before. `MaxObj`/`MSPObj` no longer have
  `post`/`post_error`/etc. `ObjBuilder::obj(&mut self)` hands the handle out (the unsafe
  `max_obj`/`msp_obj` builder accessors stay for now). `object_post!`/`object_error!` take
  `&ObjRef` only (not `AsRef<ObjRef>`); `object_warn!` is new. `field-offset` is removed. The
  "object pointer mismatch" sanity checks are removed. `multi` was left as is (its objects
  don't use a handle).
- [x] 2. `Class<W>::add_*`, typed ZST registration (macro-free). Public: `add_bang`, `add_int`,
  `add_float`, `add_sym`, `add_list`, `add_any`, `add_sel(sel: &CStr, f, defaults)`,
  `add_sel_gimme(sel: &CStr, f)`, all `-> MaxResult<()>`, plus the sealed `MethodResult` and the
  `SelArg`/`SelFn` traits (`macro_rules!` over arities 0..=7; `build.rs` generation and the old
  `Method` enum / `add_method` / `external!` are untouched). Differences from section C:
  `ObjWrapped::report_error` is `(&self, obj: &ObjRef, err: &dyn Display)` (not
  `(&self, &dyn Display)`): the default needs the object and `T` need not store an `ObjRef`; the
  trampoline builds the `&ObjRef` from the wrapper's own pointer for the call only. Registration
  needs `T: ObjWrapped<T>` as well as `W: WrapperWrapped<T> + MaxObj` (for `report_error`), until
  the redundant `T` parameter goes in step 3. `add_sel` returns an error (not a panic) when
  `defaults` exceeds the arity. Trampolines cannot know the selector, so a panic in a typed
  selector method is posted as "panic in method". A closure whose return type is `!` (it always
  panics) needs `-> ()` written out. The doctest on `Class` that checks the capturing-closure
  error creates its class in `median-mock` (hidden lines), so that a link error can't pass for the
  compile error. `hello_dsp` uses `c.add_sel(c"set", Self::set, 0)` and moved to edition 2021 for `c"..."`.
  - Half-built objects (6dd3616, touches D and E): `Wrapper` has a `constructed` flag, set
    after `T::new` returns and the internals are written, cleared in `free`. Safe code can make
    Max call into the object while `T::new` runs (through the `ObjRef` from `builder.obj()`), so
    every trampoline (typed methods, `in`/`ft`, `notify`, `assist`, attribute accessors, `dsp64`)
    checks it first, refuses, and posts an error for the object; `dsp64` then does not add
    `perform64`. `free` skips dropping the internals of an object whose `new` panicked.
  - Review fixes: `SelArg` and `SelFn` are sealed (`SelArg::C`/`ATOM_TYPE` define the trampoline
    ABI, so a downstream impl could make safe code misread an argument), and `add_sel` checks the
    arity before slicing.
- [x] 3. derive + `#[methods]`. Scope grew: every user is ported in this step (`simp`,
  `hello_dsp`, `multi`, the template, the crate docs and READMEs, `tests/e2e.rs`; the examples
  and template are edition 2024), and the old API is removed with no deprecated shim (this
  supersedes the `external!` shim in "Migration"): `external!`, `external_no_main!`, the
  proc-macro `ext_main!`, the `wrapped_tramp`/`wrapped_list_tramp`/`wrapped_sel_list_tramp`/
  `wrapped_attr_get_tramp`/`wrapped_attr_set_tramp` macros and their `wrapper::{tramp, list_tramp,
  sel_list_tramp, attr_get_tramp, attr_set_tramp}` re-exports, the `Method` enum with its
  generated aliases (`B`, `SelList`, `I`, `F`, `S`, `IS`, ...), `Class::add_method`,
  `add_sel_method` and `median/build.rs`. Kept: `wrapped_defer_tramp` (as `wrapper::defer_tramp`)
  and the unsafe `ObjRef::defer`/`defer_low`; deferring moves to a per-object queue in step 7,
  because a safe `#[defer]` marker over Max's `defer` would let the deferred call outlive the
  object. Differences from A and B:
  - Traits: `ObjClass` (not `ObjWrapped`): `type Wrapper; fn class_name() -> &'static CStr;
    fn class_type() -> ClassType`. The hooks (`report_error(&self, &ObjRef, &dyn Display)`,
    `handle_notification`, `assist`) are default methods of `MaxObjWrapped` and of
    `MSPObjWrapped` (each has its own copy), next to `new`/`class_setup` (and `perform`/
    `dsp_setup`). `MaxObjWrapped: ObjClass<Wrapper = MaxObjWrapper<Self>> + Methods + FieldAttrs
    + Sync + 'static`, the MSP twin likewise, so a `kind` mismatch is a type error. Generic
    trampolines reach `report_error` through `WrapperWrapped::report_error(&self, &dyn Display)`,
    which the wrappers implement; `Class::add_*` need only `W: WrapperWrapped<T> + MaxObj`.
  - `Methods::register<W>(&mut Class<W>) -> MaxResult<()>` and `FieldAttrs::register_attrs<W>(..)
    -> MaxResult<()>` (both default to `Ok(())`) return a result so hand-written impls use `?`; an
    `Err` makes the wrapper's registration panic with the error, which the guard in `register()`/
    `ext_main` posts, and the class is not registered. Order: field attributes, methods,
    `class_setup`. `MaxObjWrapper::<T>::register()` is safe and has no `lookup_class` argument
    (median's internal clock class uses a crate-private variant). `Class::new`, `find_in_max` and
    `exists_in_max` take `&CStr`.
  - Derive: `#[max(name, kind = max|msp, class_type = box|nobox, no_methods)]`; it generates
    `ObjClass`, an inherent `pub fn register()` (calls the wrapper's inside
    `ffi::guard_ext_main`), and `FieldAttrs` (empty when no field has `#[attr]`), plus an empty
    `Methods` with `no_methods`. Generic structs are rejected. It does not look for an `ObjRef`
    field (`builder.obj()` stays until step 4/5); `description`/`author`/`tags` are rejected as
    not supported yet. Field `#[attr(name = "..", clip(min, max), readonly)]` needs a field type
    implementing the new `attr::AttrField` (`Int64`, `Float64`, `Float32`); its value type is a
    sealed `attr::AttrValue` (`f64`, `f32`, `t_atom_long`, `SymbolRef`) that gives the Max
    attribute type. `clip` is `attr_addfilter_clip` (get and set, as `CLASS_ATTR_FILTER_CLIP`).
    `label`, `style`, `category`, `order`, `default`, `save`, `enum`, `hidden`, `range` are
    rejected with an error saying they are not supported yet (step 7).
  - Accessor attributes without macros: `Class::add_attr(name, get, set, AttrOptions)`,
    `add_attr_readonly(name, get, AttrOptions)` and `add_field_attr(name, |s: &Self| &s.field,
    AttrOptions)`, over generic ZST accessor trampolines (`attr::get_tramp`/`set_tramp`, unsafe
    `extern "C"`, inside the guards, returning `t_max_err`). A setter may return `Result`: `Err`
    goes to `report_error` and Max gets `MAX_ERR_GENERIC`. A read-only attribute has
    `ATTR_SET_OPAQUE_USER` and a no-op setter (as `AttrBuilder::new_get`). Options type is
    `attr::AttrOptions` (`new().clip(min, max).readonly()`, `clip_with(AttrClip)`); step 7 adds
    to it.
  - `#[median::methods]`: markers `#[bang]`, `#[int]`, `#[float]`, `#[sym]`, `#[list]`, `#[any]`,
    `#[sel("name")]`, `#[sel("name", defaults = n)]`, `#[sel("name", gimme)]`, `#[attr(..)]` on a
    getter (paired with `set_<name>` from the same impl; read-only without one or with
    `readonly`, in which case the setter is not registered). Not the min special messages and no
    `#[defer]`: `defer`, `defer_low`, `dblclick`, `loadbang`, `notify`, `savestate` are rejected
    as not supported yet; other attributes pass through untouched. Errors: one marker per method,
    a selector (or attribute name) registered twice in the impl, receiver not `&self`, wrong
    arity or reference shape (`&[..]` / `&T` / value), `defaults` above the arity, more than 7
    `#[sel]` arguments, generic/async/unsafe methods, trait or generic impls. Scalar argument
    types and return types are checked by the compiler through the `Class::add_*` bounds, with
    the call spanned at the method; `MethodResult` has a `#[diagnostic::on_unimplemented]`
    message. On errors the impl is still emitted (with an empty `Methods`), so the only errors
    are the macro's.
  - Names (class, selectors, attributes) are emitted as `median::__private::cstr(b"..\0")`, not
    `c".."`, so the macros work in user crates of any edition.
  - `median::ext_main! { A, B }` is a `macro_rules!` that calls `<A>::register()` and so on, each
    in its own `guard_ext_main`. `median::prelude` re-exports the derive, `methods`, `ext_main`,
    the traits, `ObjRef`, `Atom`, `SymbolRef`, the `num` types, `Class`/`ClassType`,
    `AttrOptions`, the builders, inlets/outlets, `ClockHandle`, `MaxResult`, `MaxObj`,
    `WrapperWrapped`, `post` and `object_post!`/`error!`/`warn!`, `t_atom_long`.
  - `examples/simp_explicit` is `simp` with every generated impl written by hand.
  - Tests (`median-mock`): `tests/derive.rs` (every marker dispatches with its arguments, `#[sel]`
    defaults and their registered types, `Err` to the default and an overridden `report_error`,
    field attributes get/set/clip/readonly/`Float32`, getter/setter and read-only getter
    attributes, a failing setter, an MSP derive with `perform`, five classes in one `ext_main!`
    including one whose registration panics), `tests/simp.rs` and `tests/simp_explicit.rs` (the
    same script, the same transcript), `tests/multi.rs` (two classes in one `ext_main!`),
    `tests/hello_dsp.rs`, `tests/e2e.rs` (its macro external now uses the derive), unit tests for
    the accessor trampolines refusing a half-built object, and `tests/ui.rs` (trybuild
    compile-fail cases for the macro errors).
  - Typed selectors may not mix `f64` with other argument types (from step 2, found again
    here; now enforced). The SDK says to use `A_GIMME` for such a method, because Max passes
    floats and other arguments of a typed method in different registers on x64, and the old
    `build.rs` never generated a mix. `SelFn::trampoline` (so `Class::add_sel` and `#[sel]`) has a
    post-monomorphisation `const { assert!(..) }` that rejects a mix ("use
    `#[sel("name", gimme)]` / `add_sel_gimme`"); like the zero-size check it fires on `cargo
    build`, not `cargo check`. `#[median::methods]` also reports it with a spanned error when the
    argument types are written `f64` (best effort: an alias hides it). The tests that mixed were
    changed to same-typed selectors.
  - Review fixes (after step 5): attribute getters (`attr::get`, `get_tramp`) do nothing and
    return `MAX_ERR_NONE` when `ac` or `av` is null, as the SDK's accessors do
    (`attrtester.c`; the getter isn't called), and setters already ignored a null `av`.
    `class::RESERVED_SELECTORS` (`notify`, `assist`, `inletinfo`, `dsp64`, `ft1..9`, `in1..9`,
    checked against what the wrapper registers) is refused by `Class::add_sel*`
    (`Err(MaxError::Duplicate)`, any `add_*` in fact) and by `#[sel]` (spanned error; the macro
    keeps a mirror in `median_macros/src/reserved.rs`, compared by a test). `Class` tracks the
    attribute names added through it and refuses a duplicate (`Err(Duplicate)` from `add_attr*`
    and `add_attribute`, which frees the extra attribute), so a field `#[attr]` and an `#[attr]`
    getter of the same name fail the registration. The deadlock of registering a class or making
    a `ClockHandle` inside `register_attrs`/`Methods::register`/`class_setup` is documented on
    those and on `ClockHandle`. Tests: `tests/registration.rs`, UI `methods_reserved`; the mock
    gained `attr_get_with` (null pointers, a caller array), `attr_set_null_argv`,
    `method_names`.
- [x] 4. Iolet descriptors (section F). Every user is ported (`simp`, `simp_explicit`,
  `hello_dsp`, `multi`, the template, crate docs, READMEs, tests). Public API:
  - `builder::Builder<T, K>` with `K` = `builder::Max` or `builder::Msp` (sealed `BuilderKind`),
    aliased `MaxBuilder<T>`/`MSPBuilder<T>`; `fn new(b: &mut MaxBuilder<Self>) -> Self` (the
    `ObjBuilder`/`MaxWrappedBuilder`/`MSPWrappedBuilder` traits are gone, the methods are
    inherent). Kept as they were: `obj()`, `with_clockfn`, `with_clock`, `with_buffer`,
    `creation_args`, `creation_symbol`, `try_register`, `attach`, `subscribe`, unsafe
    `max_obj`/`msp_obj`.
  - `b.inlet(desc) -> inlet::InletId` (`.index()`), `desc` an `inlet::Inlet<T>`
    (`Inlet::default()` through `Default`, `proxy()`, `int(f)`, `float(f)`, `.assist(&str)`,
    `.hot()`) or, MSP only, the `inlet::SignalInlet<T>` from `Inlet::signal()`: the sealed
    `InletFor<T, K>` bound makes a signal inlet on a Max builder a compile error (with a
    `diagnostic::on_unimplemented` note; UI test).
  - `b.outlet(desc) -> outlet::Outlet<K>`, `desc` an `OutletDesc<K>` from `Outlet::bang()`,
    `int()`, `float()`, `list()`, `any()`, `signal()` (`.assist(&str)`); kinds `Bang`, `Int`,
    `Float`, `List`, `Any`, `Signal` (uninhabited marker enums; sealed `OutletKind`,
    `OutletFor<K>` makes a signal outlet on a Max builder a compile error). Signal outlets
    return an `Outlet<Signal>` with no methods. `Outlet<K>` is `!Clone`, `!Send`, `Sync`
    (`PhantomData<*const ()>`, as `ObjRef`), with the same "do not leak" note. `send(&self, ..)
    -> Result<(), outlet::StackOverflow>`: `Outlet<Bang>::send()`, `Int` `send(t_atom_long)`,
    `Float` `send(f64)`, `List` `send(&[Atom])`, `Any` `send(&SymbolRef, &[Atom])` plus
    `send_bang/send_int/send_float/send_list`. Outlets are made immediately (`outlet_append`,
    left to right). The SDK's rule (outlets from the main and scheduler threads, not from
    `perform`) is documented on the module, not enforced. `OutList`/`OutBang`/..., `SendValue`,
    `SendAnything`, `SendError` and the `Outlet::append_*` fns are removed.
  - Hooks on `MaxObjWrapped` and `MSPObjWrapped`: `fn assist(&self, IoLet) -> Option<Cow<'_,
    str>>` and `fn inlet_hot(&self, usize) -> Option<bool>`, both default `None`; `IoLet` (in
    `wrapper`, replacing `AssistIOlet`) is `Inlet(usize)`/`Outlet(usize)`.
    `wrapper::ASSIST_MAX_STRING_LEN = 500`.
  - Positions: inlet 0 is described only by a first `Inlet::default()` or (MSP) the first
    signal inlet; otherwise the first description is inlet 1. Signal inlets occupy `0..n`.
    `in<N>`/`ft<N>` count each kind from 1 independently of position (so `simp`'s int inlet is
    `in1` now, not `in2`); proxy ids are their positions (what `ObjRef::proxy_inlet` returns).
    `finalize` runs `z_dsp_setup(n)` first (MSP, always, `n` may be 0), then makes the other
    inlets right to left. Mistakes (`Inlet::default()` not first, a signal inlet after another
    kind, more than 9 `int` or 9 `float` inlets, an outlet Max didn't make) are recorded by the
    builder and fail construction when `new` returns (in this step as a panic in `new_tramp`,
    which posts it; see step 5).
  - Table: assist and hot by position, filled once (`builder::Iolets`, no `RwLock`; a comment
    marks where a `dynlet` API would make it updatable). `assist` asks the hook, then the table,
    then the default: the kind (`"int"`, `"float"`, `"signal"`, `"bang"`, `"list"`,
    `"anything"`), `"inlet N"` for inlet 0 and plain proxies (N from 1), `"inlet N"`/
    `"outlet N"` past the table. The text is cut to 500 bytes on a char boundary and copied with
    a NUL (an interior NUL ends it). `inletinfo` (`A_CANT`) is always registered (difference
    from F: not only when some inlet is `.hot()`): hook, else table (inlet 0 and `.hot()` hot);
    writes `*t = 0` for hot, 1 for cold (as `thresh.c`/`minimum.c` do). Both run inside the
    guard and refuse a half-built object.
  - Message-list noise (a): `ft1..9`/`in1..9` are marked `undocumented` with
    `class_extra_lookup(c, "methods")` + `hashtab_lookup` + `object_addattr_parse` (max-sys
    has the bindings), right after they are added, which now happens before `class_register`
    (as in `dummy.c`) instead of after. `notify`/`assist`/`inletinfo`/`dsp64` are `A_CANT`, so
    not in the menu anyway. Whether this hides `ft`/`in` from Max's message menu is unchecked
    (part (b) not done).
  - `median-mock` (f4dd29e): `Kernel::inlets` / `inlets_created` (public `InletKind`),
    `assist`, `inlet_is_hot`, `method_attr`, `dsp_frees`, `live_clock_count`, and the three
    method-attribute functions. **Assumption**: the mock keeps `z_dsp_setup`'s signal inlets
    next to the leftmost when inlets are made after it (the reading of `z_dsp.h` this step
    relies on). In Max, `intin`/`proxy_new` insert each new inlet just right of the leftmost
    (`filein.c`), so if `z_dsp_setup`'s proxies are ordinary inlets, inlets made after it would
    land *left* of the signal inlets for `n >= 2`. To check in Max (see AGENTS/the report).
  - Tests: `tests/iolets.rs` (positions and creation order for Max and MSP, dispatch by id,
    proxy index, assist from the table/hook/defaults, truncation on multibyte boundaries,
    `inletinfo` default/`.hot()`/hook, `undocumented` on `ft`/`in`, typed outlet calls, MSP
    without signal inlets, description mistakes fail with nothing leaked), UI cases
    `outlet_not_clone` and `signal_iolets_on_max`, and the refusal of `inletinfo` on a
    half-built object.
  Added 2026-10-22: `Inlet::cold()` and `Inlet::hot_if(bool)` (and the `SignalInlet` twins), so
  inlet 0 can be declared cold (a control inlet, like `gate`'s left one) and hot/cold can be
  computed in `new`; the last of `.hot()`/`.cold()`/`.hot_if()` wins, and the `inlet_hot` hook
  still overrides them.
- [x] 5. Fallible `new` (section E). `fn new(b: &mut MaxBuilder<Self>) -> Result<Self,
  error::NewError>` (MSP likewise); every user is ported, `simp`/`simp_explicit` take an
  optional initial value and refuse a symbol with `?` (tested through the shared script).
  - `NewError` (in the prelude): `new(impl Display)`, `message()`, `Display`, `Debug`;
    `From<String>`, `From<&str>`, `From<MaxError>`, `From<StackOverflow>`, boxed errors and
    common std errors (`ParseIntError`, `ParseFloatError`, `TryFromIntError`, `Utf8Error`,
    `FromUtf8Error`, `NulError`, `fmt::Error`, `io::Error`). Deviation: no blanket
    `From<E: Error>`, which is incoherent next to `From<String>` (E0119: std may implement
    `Error` for `String`); the docs say to use `.map_err(NewError::new)?` for other errors.
    `NewError` deliberately does not implement `Error` (like `anyhow::Error`).
  - The wrappers share one `construct`: allocate, `constructed = false`, a `Builder` (declared
    after the `ObjBox`, so dropped first), `I::build` (`T::new`, then `finalize`), then write the
    internals, `constructed = true` and `builder.commit()`. On `Err` (from `T::new` or a mistake
    in the description, which is now an `Err` posted as `"median: ..."` instead of a panic) the
    message is posted with `object_error` for the object, the builder is dropped (clocks, buffer
    refs, proxies), the `ObjBox` frees the object (outlets and `intin`/`floatin` inlets go with
    it, `free` sees it isn't constructed) and `new_tramp` returns null. The builder owes the
    `z_dsp_free` from the moment `finalize` calls `z_dsp_setup` until `commit`, so a panic in
    between (only median code runs there; tested with a `cfg(test)` injection point) pairs it;
    afterwards `free_msp` does. That fixes the leak in plan-soundness-fixes' Status. An `Err`
    from an MSP `new` happens before `z_dsp_setup`, so nothing needs pairing. `new_or_panic`
    (the Rust-side `Wrapper::new`) panics with the message.
  - Tests: `tests/new_error.rs` (Max and MSP `Err` give null and one error for the object, with
    no live objects, clocks or buffer references left and no `z_dsp_free` needed; a panic in an
    MSP `new` likewise), the wrapper unit test for a panic after `z_dsp_setup`, `NewError`
    conversions, `simp`/`simp_explicit` arguments. `median-mock` gained
    `live_buffer_ref_count`, and reports an object freed after `z_dsp_setup` without
    `z_dsp_free` and an unfreed proxy, so every test checks those.
  - Review fixes (steps 4 and 5, after step 6):
    - Inlet order: `finalize` makes the non-signal inlets first (right to left) and calls
      `z_dsp_setup(n)` **last**, as the pre-step-4 builder did; the `dsp_pending`/`z_dsp_free`
      pairing is unchanged. This supersedes "`z_dsp_setup(n)` first" in step 4 and its
      **Assumption**: `intin`'s documentation (`ext_proto.h`) says each new inlet goes just right
      of the leftmost, so inlets made after `z_dsp_setup` would land left of signal inlets 1..n-1
      unless Max pins those; made before, they end up right of them under either reading. The
      mock no longer pins `z_dsp_setup`'s inlets (ordinary inserts), and `tests/iolets.rs`
      asserts the creation order and the resulting layout.
    - `class::MSP_RESERVED_SELECTORS` (`signal`, `userconnect`, `nsiginlets`, `enable`: what
      `class_dspinit` adds, which `z_dsp.h` says not to bind; it runs after user registration,
      so one would silently replace the other) are refused by `Class::add_*` and `#[sel]` for
      every class: neither `Class` nor the macro knows the class's kind. Mirrored in
      `median_macros/src/reserved.rs` (a test compares both lists); UI case in
      `methods_reserved`.
    - `Outlet<Int>` is made with the type `"int"` (as the SDK's `intout`), not `"long"`.
    - median-mock: the outlet calls return non-null (1, as Max) on success, so `Outlet::send`
      is `Ok` under the mock (it was always `Err(StackOverflow)`); `Kernel::set_outlet_overflow`
      makes them fail (null, nothing sent). Tested in both crates.
- [x] 6. `SymbolRef`, `BufferLocked` (section G; abeaf31, 43d09b8, aa595ef).
  - `SymbolRef`: `#[derive(Copy, Clone, PartialEq, Eq, Hash)]` over `NonNull<t_symbol>`,
    `repr(transparent)`; `assign`, `new`, `inner`, `to_cstring` and the `Result`-returning
    inherent `to_string` are removed. `unsafe fn from_raw` maps null to `Default` (`s_nothing`),
    documented, since the typed trampolines rely on it. `as_ptr`, `name() -> &CStr`, `to_str()
    -> Result<&str, Utf8Error>`, `is_empty`, `Display` (lossy), `Debug`, `From<&CStr>`,
    `From<CString>`, `From<SymbolRef> for *mut t_symbol`, `TryFrom<&str>`/`TryFrom<String>`
    (`NulError` on an interior NUL; `NewError: From<NulError>` so `?` works in `new`).
    Difference from the brief: `name()` is tied to `&self`, not `&'static`, because the SDK
    headers never say symbols are never freed (it is only implied by practice: cached
    `gensym` results in statics, the common symbols table); `SymbolRef` being `Copy + 'static`
    already relies on it, so this only avoids promising it in a signature. `Send`/`Sync` have a
    SAFETY comment. Equality and hashing are by address (equivalent to by string, through
    interning; the old `Hash` hashed the string). `median::sym!("foo")` (also in the prelude)
    is a `macro_rules!` that builds the `CStr` in a `const` (`concat!` + `from_bytes_with_nul`)
    and expands to `SymbolRef::from(<that CStr>)`; an interior NUL is a compile error (E0080),
    tested by a doctest and `ui/sym_nul`. It calls `gensym` on every use.
  - `BufferRef`: the mutex holds only the name (`set`, the new `name()`, notification
    forwarding); the `t_buffer_ref` pointer is outside it and immutable. Difference: `try_lock`,
    `exists`, `channels`, `frames`, `sample_rate` no longer take the mutex, so the perform path
    neither locks nor allocates (`buffer_ref_getobject` from perform concurrently with
    `buffer_ref_set` on the main thread is what `index~.c` does). A null `buffer_ref_new` result
    is treated as "no buffer" instead of being used. `notify_if` stays safe (a `Notification`
    can only be made with `unsafe`), `notify_if_unchecked` and `new` have `# Safety` sections.
  - `BufferLocked<'_>` (borrowing the reference; `!Send`): `as_slice`, `frame(i)`,
    `sample(frame, ch)`, `channel_iter(ch)` (strided, `ExactSizeIterator +
    DoubleEndedIterator + Clone`) over `AtomicSample` (`repr(transparent)` `AtomicU32`, Relaxed
    `get`/`set`, `new`, `Debug`; size and alignment asserted equal to `f32`), `frames`,
    `channels` (read once after the claim, as `index~.c`), `sample_rate`,
    `millisample_rate`, `set_dirty(&mut self)`, `unsafe fn samples_ptr`. `frame`/`sample`/
    `channel_iter` panic out of range (documented); `as_slice().get(..)` is the checked form.
    `as_slice` is an addition to the brief's list. Released on drop (also while unwinding).
  - `BufferEdit<'_>` (`!Send`, since `buffer_edit_end` unlocks the buffer's mutex): from
    `BufferReference::edit()`, refused with `EditError::WrongThread` on the audio and scheduler
    threads (`systhread_isaudiothread`/`istimerthread`, the SDK's own checks; the scheduler can
    run inside the audio thread). Sample pointer and size from `buffer_getinfo`. `as_slice`,
    `as_mut_slice`, `frame(_mut)`, `channel_iter(_mut)`, `set_dirty`; a mutable view marks it
    dirty, and drop calls `buffer_edit_end(b, 1)` then `buffer_setdirty`. **No `resize`**:
    `ext_buffer.h` declares no resize call (the `buffer~` messages that do it are not in the SDK
    headers). **Unverified in Max**: that `buffer_locksamples` returns null while an edit is in
    progress (the header says `buffer_edit_begin` clears the valid flag and waits for the in-use
    count; `BufferEdit`'s `&mut` relies on it), and that `b_samples` is right during an edit.
  - Messages: `BufferReference::view()` (`buffer_view`, as `index~`/`simpwave~` do on
    double-click), `Class::add_dblclick` (`A_CANT`), and `Class::add_buffer_messages(|s:
    &Self| &*s.buf)`, which registers `set <name>` and `dblclick` for one reference (call it in
    `class_setup`). `#[dblclick]` in `#[median::methods]` is still rejected (not done here).
    `buffer_modified` and binding notifications are forwarded as before.
  - `hello_dsp` uses `add_buffer_messages` (its `#[sel("set")]` is gone) and plays its first
    buffer's channel 0 on the right outlet.
  - `median-mock` (43d09b8): `buffer_edit_begin/end` (pairing checked, nested begin/stray end
    refused, begin on the audio thread or with claims held reported), `buffer_getinfo`,
    `buffer_view`, null from `buffer_locksamples` while editing; `DspAdd::run` reports a
    perform that returns holding a claim, `destroy_buffer` reports outstanding claims/edits.
    `Kernel::{buffer_editing, buffer_edits, clear_buffer_dirty, buffer_views, dblclick}`.
  - Tests: `symbol::tests` (interning equality, names, NUL errors, null/default, threads),
    `ui/sym_nul`, `tests/buffer.rs`, `tests/hello_dsp.rs` with a buffer, mock tests in
    `median-mock/tests/kernel.rs`.
  - Review fixes (after step 7):
    - One `BufferEdit` per `buffer~`: median keeps a process-wide list of the buffer objects it
      is editing (a `Mutex<Vec<usize>>`; edits are refused on the audio and scheduler threads,
      so the lock never is taken there), checked before `buffer_edit_begin`; a second edit,
      through the same reference, another reference to the same buffer, a cloned `Arc` or
      another thread, is the new `EditError::Busy`. Released in `BufferEdit::drop` after
      `buffer_edit_end` (also on unwind). It does not coordinate with another external's copy of
      median. The mock no longer refuses a nested `buffer_edit_begin` (Max deadlocks or aliases,
      which isn't modelled): it reports it and carries on, and the tests check it is never
      reached.
    - `channel_iter`/`channel_iter_mut` of `BufferLocked` and `BufferEdit` yield nothing for a
      0-frame multichannel buffer (they panicked slicing `[][channel..]`).
    - `BufferEdit::drop` passes `valid = -1` (`ext_buffer.h`: "negative=RESTORE_OLD_VALID") to
      `buffer_edit_end` when the buffer had no sample memory (null `b_samples` or 0 frames) when
      the edit began, `1` otherwise. The mock records the value
      (`Kernel::buffer_last_edit_valid`).
    - Leaking (`mem::forget`) a `BufferLocked` or `BufferEdit` is documented as a Max-wide hang
      on that buffer (the in-use count stays raised, or the mutex held and the valid flag
      cleared), not a local leak.
    - `MSP_RESERVED_SELECTORS` are refused only for MSP classes: `MaxObj` has a hidden
      `IS_MSP` (true for `MSPObjWrapper`), `Class::add_*` checks it and
      `class::is_reserved_selector(sel, msp)` takes the kind. `#[median::methods]` no longer
      mirrors or rejects them (it can't know the kind); an MSP class's `#[sel("enable")]` fails
      at registration. This supersedes "refused for every class" in the step 4/5 review fixes.
- [x] 7. Clock/queue/fifo (H), attribute options (I). One commit per piece (mock, 7a-7f, then the
  examples). Every user is ported; there are no aliases for the old names.
  - `Clock` (was `ClockHandle`): `b.clock(Self::tick)` takes a zero-sized `Copy` callable (a
    post-monomorphisation `const` assert, like `Class::add_*`, whose message points to
    `clock_with`), `b.clock_with(closure)` any `Fn(&T) + Send + Sync + 'static`. Addition: the
    callback may return `Result<(), E>`, the `Err` goes to `report_error`. `delay(c_long)` (was
    `i32`), `fdelay(f64)`, `cancel()`, plus `trigger`, `ftime`, `time` and the unsafe
    `Clock::new` as before. `with_clockfn`/`with_clock` are removed. The `ClockShared` protocol
    is unchanged (armed after construction, cleared before `T` drops); the two known clock
    issues of plan-soundness-fixes' Status are not addressed. Not done: min's
    `defer_delivery()` option.
  - `Queue` (`median::queue`): `b.queue(Self::m)` / `b.queue_with(closure)`, `set()`
    (`qelem_set`), `front()` (`qelem_front`), `cancel()` (`qelem_unset`); the callback runs on the
    main thread and may return `Result`. Its target is a `ClockShared` (the clocks' type, reused),
    so the wrapper arms and clears it with the clocks. The qelem's `obj` is a strong count of that
    `Arc` (not a nobox object, which qelems don't need), released after `qelem_free`.
    **Difference**: the wrapper *disables* the queue (clears its target, so a pending run does
    nothing) before `T` is dropped; the qelem itself is freed (`qelem_free`, which also cancels a
    pending run) when the `Queue` is dropped with `T`. Freeing it from the wrapper first would let
    `perform` (which runs until `z_dsp_free`, after the targets are cleared) `set` a freed qelem.
    No public unsafe `Queue::new`. The SDK headers give no thread rules for `qelem_set`; the
    `simplethread` example calls it from its own thread. Real-time safety on the audio thread is
    **unverified** and documented as such on `queue` and `fifo`.
  - `#[defer]`/`#[defer_low]` (`median::defer`): each object of a class with a deferred method
    has a `DeferQueue` in the wrapper internals: a `parking_lot` mutex over a `VecDeque` of boxed
    calls with their arguments copied (scalars, `SymbolRef`, atoms in `OwnedAtoms`), and one
    `Queue` whose callback takes the whole batch and runs each call in its own panic guard
    (since the step 7 review fixes below: two qelems, and the drain pops one call at a time).
    `Class` records that a deferred method was added, `CLASSES` keeps the flag per class, and
    `construct` makes the queue only then. Modes `Defer::Low` (always queued at the back,
    `qelem_set`) and `Defer::Front` (at once on the main thread, otherwise queued at the front of
    the object's calls and `qelem_front`; a qelem already queued at the back stays there; several
    `Front` calls run latest first, as Max's `defer`s do). Hand-written:
    `Class::add_{bang,int,float,sym,list,any}_deferred(f, mode)`,
    `add_sel_deferred(sel, f, defaults, mode)`, `add_sel_gimme_deferred(sel, f, mode)` (typed
    selectors get a hidden `SelFn::deferred_trampoline`; `SelArg: Copy + Send + 'static`). The
    trampolines reach the queue through a hidden `WrapperWrapped::defer_call` (default panics).
    Markers pair with a message marker only; with `#[attr]`, `#[dblclick]`, `#[loadbang]` or alone
    they are spanned errors, as are both on one method. A pending call is dropped without running
    (and its atoms freed) when the object is freed. Rules documented: queuing allocates and locks,
    so never from `perform`; a deferred method must not free its own object (it runs inside the
    queue callback; deadlock, as for clocks). Removed: `wrapper::defer_tramp`
    (`median_macros::wrapped_defer_tramp`, `tramp.rs`), `wrapper::DeferMethodWrapped`,
    `ObjRef::defer`/`defer_low`, `thread::{DeferMethod, defer, defer_low}`; `thread::SysThread`
    stays. `method::sel_list` is now unused by median but left (public).
  - `median::fifo`: `rtrb` 0.4's `RingBuffer`, `Producer`, `Consumer`, `PushError`, `PopError`
    re-exported, module docs with the perform-push / `Queue::set` / main-thread-drain pattern.
    **Difference**: a small wrapper, `fifo::Shared<E>`, is needed after all: `push`/`pop` take
    `&mut self` and median's methods and `perform` get `&self`. `Shared::try_with` is
    `parking_lot`'s `try_lock` (one CAS, never blocks or allocates; `None` only if two threads use
    the same end). Not a `Fifo<T>` type as section H sketched.
  - `#[dblclick]` and `#[loadbang]` (`A_CANT`, `fn(&self)`), `Class::add_loadbang`. The SDK
    headers declare neither signature; `(x)` is what the SDK's `dblclick` examples use and what is
    assumed for `loadbang` (unverified). `#[notify]`, `#[savestate]` stay "not supported yet".
  - Attribute options: `AttrOptions` gains `label`, `style`, `category` (`&'static CStr`),
    `order(t_atom_long)`, `default(impl Into<AttrDefault>)` (`Int`, `Float` or a `Parse` string;
    from `i32`/`i64`/`isize`/`f32`/`f64`/`&'static CStr`), `save`, `enum_vals`, `range(min, max)`,
    `hidden`; `AttrBuilder` gains the same setters plus `options(AttrOptions)` (clip, read-only as
    `ATTR_SET_OPAQUE_USER`, the rest); `Attr` carries its type and options, and
    `Class::add_attribute` adds them after `class_addattr`, as the `CLASS_ATTR_*` macros expand:
    `class_attr_addattr_parse` with the macro's type (`long` for `order`/`save`/`invisible`,
    `symbol` for `style`/`category`, `atom` for `enumvals`, the attribute's own type for
    `default`/`min`/`max`, which the macros get with `object_method(attr, gettype)`), the label
    with `class_attr_addattr_atoms` and one symbol from `gensym_tr` (what
    `CLASS_ATTR_ATTR_FORMAT(.., "s", gensym_tr(label))` makes), the category through `str_tr`
    then parsed (as the macro). `enum_vals` is `CLASS_ATTR_ENUM` (style `enum`) for a symbol
    attribute and `CLASS_ATTR_ENUMINDEX` (`enumindex`) otherwise; an explicit `style` is applied
    after it. `range` is `CLASS_ATTR_MIN`+`_MAX` (a hint; `clip` limits). `hidden` is
    `CLASS_ATTR_INVISIBLE`. Numbers are written as the attribute's type (whole for `long`).
    `save` needs nothing else from the class: the SDK's `attrtester` (a plain box class without
    a dictionary constructor or obex) uses `CLASS_ATTR_SAVE` alone and calls `attr_args_process`
    in `new`, as median does. `default` is only set automatically for dictionary constructors
    (`ext_obex_util.h`), so it is documented as informational. `#[attr(..)]` on fields and
    getters takes `label = ".."`, `style`, `category`, `order = n`, `default = value` (a string
    literal becomes a parse string), `save`, `enum = ".."`, `range(min, max)`, `hidden`. Not
    added: `basic`, `paint`, `selfsave`, `defaultname`. Issue #17's hand-written
    `class_attr_addattr_parse(.., "save", ..)` is now `save`.
  - Examples: `simp`/`simp_explicit` have a `Queue` (`changed`, set by `int`), a deferred
    `read` (`#[sel("read", gimme)] #[defer_low]` / `add_sel_gimme_deferred`) and `value` with
    `label`, `category`, `save`; `hello_dsp` and the template use attribute options. The shared
    simp script checks all three.
  - `median-mock` (dbec6cd): `qelem_front`, `qelem_free` of a dead qelem reported,
    `live_qelem_count`, attributes of attributes (`class_attr_addattr_parse`/`_atoms`,
    `object_addattr_parse` on an attribute, read back with `attr_attr`/`attr_attr_names`; parse
    strings split by `attr::parse_atoms`, an approximation of `atom_setparse`), `gensym_tr` and
    `str_tr` without translation, `Kernel::loadbang`.
  - Tests: `tests/clock.rs`, `tests/queue.rs`, `tests/defer.rs`, `tests/fifo.rs`,
    `tests/special.rs` (dblclick/loadbang), `tests/attr_options.rs`, the `defer` unit test (a
    pending call's atoms are freed at free), `fifo::Shared`, the half-built refusal of the
    deferring trampolines, compile-fail doctests for a capturing closure in `b.clock`, UI cases
    for the marker and option errors, the simp script.
  - To check in Max: a deferred `read` from a `metro` (Overdrive on) runs on the main thread
    and a pending one is dropped when the box is deleted; `Queue::set` from `perform` (the fifo
    pattern) under load, for glitches; `loadbang` is called with only the object (`(x)`);
    `label`, `category`, `order`, `style`, `enum`/`enumindex`, `range` and `hidden` show in the
    inspector; a `save` attribute is restored when the patcher is reopened; `default` shows as
    the default in the inspector.
  - Step 7 review fixes:
    - Re-entrant callbacks (was a hang). `ClockShared` held a non-reentrant mutex for the whole
      callback, so a callback that made Max service its queues on the same thread (a
      `#[defer_low] read` opening a modal dialog while a `#[defer]` call is queued from the
      scheduler) locked it again and hung the main thread. Now a callback holds `running`, a
      `parking_lot::ReentrantMutex<()>`, and the target sits behind a separate mutex that is only
      held to read it (the callback is an `Arc`, cloned out before the call; it is never the last
      reference, so the clone's drop never frees on the scheduler thread). A nested callback on
      the same thread runs inside the outer one, as Max's own `defer` did. `clear` from another
      thread still waits on `running` and empties the target, so later callbacks are no-ops.
      `clear` from a thread that is running a callback of that target (the owner freed, or its
      `Clock`/`Queue` dropped, from inside its own callback, outer or nested) can neither wait nor
      return (the owner's memory is freed when it returns, under the callback), so it writes a
      line to stderr and parks the thread forever: the old deadlock, now only in that case.
      Chosen over refusing (Max frees the object's memory after the free method whatever median
      does) and over deferring the drop (only the closure could be deferred, not the object). The
      deferred-call drain (`run_deferred`) pops one call at a time (at most as many as were
      pending when it started) instead of taking the batch, so a nested drain runs the calls
      after the current one in order; the `calls` lock is never held across a call.
      `median-mock`: `Kernel::run_main_queue_nested` and `Kernel::advance_nested` (associated
      functions, for code running under the kernel; they panic on a thread that doesn't hold
      it) run the main queue or the clocks from inside a callback, as a nested event loop does.
      Tests: `clock::tests` (nested call, clear from inside blocks, cross-thread clear waits),
      `tests/defer.rs` (a deferred method's nested drain runs a scheduler-queued `#[defer]` call
      and a pending `Low` one, in order), `tests/queue.rs`, `tests/clock.rs` (nested re-entry),
      `tests/free_inside_callback.rs` (an object freed from its own clock callback blocks that
      thread, the object stays alive), mock tests for the nested helpers. To check in Max: a
      `#[defer_low] read` that opens a dialog while a `metro` sends `#[defer]` messages (Overdrive
      on) neither hangs nor runs them out of order.
    - `AttrDefault::Int` is written as a whole number for every attribute type except
      `float32`/`float64` (it went through `number_text(v as f64, ..)`, so an `atom`, `symbol` or
      pointer attribute got `"5.0"`). Test: an `atom` attribute in `tests/attr_options.rs`.
    - `qelem_front` on a qelem that is already set: that it stays where it is (not set again,
      not moved to the front) is an assumption; `ext_proto.h` only says it is "identical to
      `qelem_set()`" apart from the position. Marked unverified in `Queue::front`, the `defer`
      module docs and the mock's `qelem_front`, and the assertions of
      `tests/queue.rs::front_goes_ahead_and_cancel_unsets` and the mock's
      `qelem_front_queues_at_the_front_and_frees_are_checked` are labelled as testing the mock's
      model. To check in Max: `front()` on a queue already set at the back.
    - Prelude: `AttrField` and `ManagedBufferRef` added (the gap the migration guide listed;
      its note is removed). `hello_dsp` imports only the prelude.
    - A qelem is unset *after* its function returns (`ext_proto.h:1012-1013`, `qelem_set`: "A
      Qelem object is unset after its queue function has been called"), so a `qelem_set` or
      `qelem_front` made while it runs is a no-op. The crate and the mock assumed the opposite
      (unset before the call). Consequence: a deferred call queued during the object's drain
      (by a deferred method, or from the scheduler meanwhile) beyond `run_deferred`'s bound was
      stranded until an unrelated later push, and dropped at free if none came. Fixed:
      - `DeferQueue` has two qelems, both running `run_deferred` through their own
        `ClockShared` target (both in `targets`, so armed after construction and cleared before
        `T` drops; both freed with the `DeferQueue`). Every push sets both (`qelem_set` for
        `Low`, `qelem_front` for `Front`); whichever is not running picks the call up, under
        either unset model. Cost: the other qelem usually runs empty (a deferred message now
        costs two qelem runs). The bound stays, so a self-requeueing method runs once per qelem
        run, alternating. A call queued while both run (one nested inside the other, in a modal
        loop) sets neither, so `run_deferred` ends with `set_if_pending` (both, `qelem_set`):
        the inner drain's is lost too, the outer one's, after the inner qelem returned, is not;
        that call runs after the loop closes, at the back even if it was `Front`.
      - `median-mock`: `run_main_queue` (and `run_main_queue_nested`) unset a qelem after its
        function returns, so a set from inside it is a no-op; a running qelem is not run again
        by a nested loop. Mock test `a_qelem_set_inside_its_own_function_stays_unset`;
        `a_nested_main_queue_runs_inside_the_callback` now expects the self-set to do nothing.
      - `Queue`: a `set`/`front` during its own callback is a no-op, documented on `Queue::set`,
        `Queue::front` and the module docs (not worked around for user queues).
        `tests/queue.rs::a_queue_callback_can_run_again_nested_inside_itself` is replaced by
        `a_queue_set_inside_its_own_callback_does_nothing`.
      - Tests (`tests/defer.rs`): a self-requeueing deferred method runs once per qelem run with
        no unrelated later push; a `#[defer]` call queued from the scheduler role during a drain
        runs; a call queued while both qelems run nested runs after the outer drain. Counts in
        the existing tests and `simp_script` follow the second qelem. Under an unset-before mock
        (checked by hand, not committed) the same calls all run once; only run counts and, in the
        nested case, where `later` runs differ.
      - Docs: `ClockShared` re-entry (Max should not run the same qelem nested; not relied on),
        the `SyncPtr` safety note (the guarantee is `running`, not the target's lock), the
        `defer` module and `run_deferred`.
      - Freeing an object, or dropping its `Clock`/`Queue`, inside its own callback now also posts
        an `object_error` for the owner before parking (the owner is alive: its free can't
        return). `tests/free_inside_callback.rs` checks the callback started, that the thread
        parked in that path (hidden `median::clock::parked_in_own_callback()`, a counter), and
        the console error, so it fails if the thread hung elsewhere or never ran.
      - To check in Max: that a `qelem_set` from inside the qelem's own function is a no-op (the
        header's reading); a self-requeueing `#[defer_low]` method keeps running; a `metro`
        sending `#[defer]` messages to an object whose deferred method is running loses none.
