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
- Write `doc/migration-0.3.md` from the diff of the examples: old form, new form, one line each.
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
- [ ] 3. derive + `#[methods]`, `simp` ported
- [ ] 4. Iolet descriptors, `hello_dsp`/`multi` ported
- [ ] 5. Fallible `new`
- [ ] 6. `SymbolRef`, `BufferLocked`
- [ ] 7. Clock/queue/fifo, attribute options
