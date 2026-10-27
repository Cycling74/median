# `A_GIMMEBACK` methods: design

Written 2026-10-27 on `xnor/api-redesign`. Sources: the SDK headers and examples in this repository.

## What the SDK says

`ext_mess.h` (`e_max_atomtypes`, value 14):

> `A_GIMMEBACK`: request that args be passed as an array, the routine will check the types
> itself. can return atom value in final atom ptr arg. function returns long error code 0 = no
> err. see `gimmeback_meth` typedef

```c
typedef long *(*gimmeback_meth)(void *x, t_symbol *s, long ac, t_atom *av, t_atom *rv);
```

The typedef's `long *` return disagrees with its own comment and with the one SDK example,
`basics/simplejs` (`t_max_err simplejs_doabs(x, s, ac, av, rv)`); both are a register-sized
integer, so the ABI is the same and the comment wins (an error code).

Callers that read the reply, from the public SDK:

- `object_method_typed(x, s, ac, av, rv)` (`ext_obex.h`): "If the receiver object can respond
  to the message, `object_method_typed()` returns the result in `rv`. Otherwise, `rv` will
  contain an `A_NOTHING` atom." The method is called as `(x, s, ac, av, rv)` and its return
  value is the error code. `simplejs` exists to be driven from `js`: a script calling the method
  on the object receives the reply.
- `simplejs` makes a fresh `atomarray`, stores it in `rv` with `atom_setobj`, and never frees
  it. The only reading that doesn't leak is that the caller owns an object reply.
- Jitter: `max_jit_class_wrap_standard(mclass, jclass, flags)` with
  `MAX_JIT_CLASS_FLAGS_GIMMEBACK_WRAP` ("uses standard dumpout `A_DEFER_LOW` method",
  `jit.max.h`) wraps the Jitter class's gimmeback methods so a message from the patcher calls
  the method and sends the reply out the dumpout outlet (`max_jit_obex_gimmeback_dumpout`,
  declared next to it). Without the flag the wrap is `max_jit_obex_gimmeback`.

Assumed by this design and listed under "Host checks" below, since no header states them:

- A patch-cord message does not reach an `A_GIMMEBACK` method on a Max class. The type checker
  converts a message's arguments against the registered argument types, and `A_GIMMEBACK` is
  not an argument type, so median treats the method as reachable only through
  `object_method_typed` callers (`js`, other externals, the Jitter wrap). Confirmed in Max 9.2 on
  2026-10-27 (patch 05 part B: `unknown message argument type`, `bad arguments`, `missing
  arguments`, the method not run). Since the patcher's message list still offered the selector,
  `Class::add_gimmeback` marks the method `undocumented` on a Max class (see Registration).
- The caller frees an object reply (what `simplejs` implies), and the Jitter dumpout wrap sends
  an atomarray reply as its atoms. Observed 2026-10-27 (fourth run): true of a caller that
  consumes the reply (the Jitter dumpout wrap); a caller that discards it leaks it (`js`'s
  `Maxobj.message`, about 450 MB per 6000 replies of 64 KiB). Scalar replies cannot leak.

So a gimmeback method is a gimme method with a return value for typed callers.

## Public API

### The reply type

```rust
/// What a gimmeback method hands back in `rv`.
pub enum Reply {
    /// `A_NOTHING`: the method has no value for this call.
    Nothing,
    Int(t_atom_long),
    Float(f64),
    Symbol(SymbolRef),
    /// A list: packaged in a fresh `atomarray` the caller owns (`A_OBJ`), the convention
    /// `simplejs` sets. Object atoms are refused (`AtomArray::new` validates), so a reply
    /// cannot carry a pointer median didn't make.
    Atoms(Vec<Atom>),
    /// A dictionary (`A_OBJ`): median's reference goes to the caller.
    Dict(Dict),
}
```

With `From<t_atom_long | f64 | SymbolRef | Vec<Atom> | AtomArray | ()>` for `Reply`, and a
sealed `GimmebackResult` trait implemented for `Reply`, each of those `From` sources, and
`Result<R, E> where R: Into<Reply>, E: Display`:

- `Ok(r)` / plain `r`: `*rv = r`, return `MAX_ERR_NONE`.
- `Err(e)`: `*rv = A_NOTHING`, `e` goes to `report_error` (the console), return
  `MAX_ERR_GENERIC`.
- `Reply::Atoms` whose `atomarray_new` fails: `A_NOTHING`, `MAX_ERR_OUT_OF_MEM`.

`Reply::Dict(Dict)` belongs in the same enum: `Dict::into_raw` gives up median's reference and
the caller's free releases it (a shared `Dict` just loses one reference), under the same
ownership assumption as the atomarray. A string reply
(`Reply::Str(String)`, a fresh `t_string` from `string_new`) is a TODO (`TODO.md`): median has
no public string type by design (`doc/plan-containers.md`), so it would be the one place that
makes one and hands it straight over.

`Atom` itself is deliberately not a reply type: `Atom::set_obj` is safe, so a bare `Atom` could
return any pointer and the ownership of `rv` is exactly what is unverified. `Reply` is the whole
set of things we know how to hand over.

### What it looks like

The `simplejs` example (`doAbs`: the absolute value of its one argument, for `js`) in the
macro form, with the two reply shapes and an error:

```rust
#[median::methods]
impl Simp {
    /// For a typed caller that reads the reply (`object_method_typed` from another external):
    /// `doAbs -3` gives `3`, `doAbs 1 -2` gives the list `[1, 2]`, no argument is an error in
    /// the Max window and no value. A patch cord can't reach it, and `js`'s `Maxobj.message`
    /// runs it but never hands the reply back (observed 2026-10-27).
    #[gimmeback("doAbs")]
    pub fn do_abs(&self, atoms: &[Atom]) -> Result<Reply, String> {
        match atoms {
            [] => Err("missing argument for method doAbs()".into()),
            [one] => Ok(match one.get_value() {
                Some(AtomValue::Int(i)) => Reply::Int(i.abs()),
                Some(AtomValue::Float(f)) => Reply::Float(f.abs()),
                _ => Reply::Nothing,
            }),
            many => Ok(Reply::Atoms(
                many.iter()
                    .map(|a| match a.get_value() {
                        Some(AtomValue::Int(i)) => Atom::from(i.abs()),
                        Some(AtomValue::Float(f)) => Atom::from(f.abs()),
                        _ => *a,
                    })
                    .collect(),
            )),
        }
    }

    /// A count: the plain value is enough when nothing can go wrong.
    #[gimmeback("getcount")]
    pub fn count(&self, _atoms: &[Atom]) -> t_atom_long {
        self.count.load(Ordering::Relaxed)
    }

    /// A snapshot of the object's state for `js` (`obj.getstate()` is a JS object).
    #[gimmeback("getstate")]
    pub fn state(&self, _atoms: &[Atom]) -> DictResult<Dict> {
        let d = Dict::new()?;
        d.insert(sym!("count"), self.count.load(Ordering::Relaxed))?;
        Ok(d)
    }
}
```

The same three without the macro (`simp_explicit` style), in `Methods::register`:

```rust
c.add_gimmeback(c"doAbs", Self::do_abs)?;
c.add_gimmeback(c"getcount", Self::count)?;
c.add_gimmeback(c"getstate", Self::state)?;
```

The return type is anything `GimmebackResult` accepts: `Reply`, a value that converts to one
(`t_atom_long`, `f64`, `SymbolRef`, `Vec<Atom>`, `AtomArray`, `Dict`, `()`), or `Result` of
one of those with a `Display` error. `#[defer]` on it is a compile error.

### The selector argument (decided 2026-10-27)

`#[sel("name")]` names the selector in the marker, so a method under `#[gimme]` or
`#[gimmeback]` has no use for the `&SymbolRef` Max passes; the macro forms take
`(&self, &[Atom])`, like `#[list]`. (`#[any]` keeps the selector: that is its point.) This
changes `#[gimme]`, which today demands `(&self, &SymbolRef, &[Atom])`; 0.3 is unreleased,
so `simp`, the tests and `doc/migration-0.3.md` change with it.

The explicit API keeps one name and accepts both shapes, so one function can serve several
selectors:

```rust
c.add_gimme(c"foo", Self::foo)?;   // fn foo(&self, sel: &SymbolRef, atoms: &[Atom])
c.add_gimme(c"bar", Self::foo)?;
c.add_gimme(c"read", Self::read)?; // fn read(&self, atoms: &[Atom])
```

That is a sealed trait with a marker type parameter that the callable's signature selects
(checked to compile):

```rust
pub trait GimmeFn<T, R, A>: Copy + Send + Sync + 'static {
    fn call(self, t: &T, sel: &SymbolRef, atoms: &[Atom]) -> R;
}
pub struct WithSel;
pub struct NoSel;
impl<T, R, F: Fn(&T, &SymbolRef, &[Atom]) -> R + ..> GimmeFn<T, R, WithSel> for F { .. }
impl<T, R, F: Fn(&T, &[Atom]) -> R + ..> GimmeFn<T, R, NoSel> for F { .. }

pub fn add_gimme<T, F, R, A>(&mut self, sel: &CStr, f: F) -> MaxResult<()>
where F: GimmeFn<T, R, A>, R: MethodResult, ..
```

The two impls don't overlap because `A` differs, and `A` is inferred from `F`, so callers
never write it. The trampoline becomes `sel_gimme_tramp::<W, T, F, R, A>` and calls
`F::call`, which ignores the selector in the `NoSel` impl. The same trait serves
`add_gimme_deferred` and `add_gimmeback` (whose `R: GimmebackResult`). The macro
emits the same `c.add_gimme(..)` call for either shape, so it needs no shape check beyond
"takes `&self`"; a wrong signature is a trait-bound error at the generated call, which the
`ui` tests pin.

Cost: one more type parameter on three `Class` methods and the trampolines, and a slightly
longer error for a method with the wrong arguments (the `ui` expectation shows it).


### Registration

```rust
impl<W> Class<W> {
    pub fn add_gimmeback<T, F, R, A>(&mut self, sel: &CStr, f: F) -> MaxResult<()>
    where
        W: WrapperWrapped<T> + MaxObj,
        F: GimmeFn<T, R, A>,   // `(&T, &[Atom])` or `(&T, &SymbolRef, &[Atom])`, see below
        R: GimmebackResult;
}
```

Same shape as `add_gimme`: a zero-sized callable, `is_untyped_selector` refused
(`MaxError::Duplicate`), `RESERVED_SELECTORS` refused through `add_typed`, registered with
`&[A_GIMMEBACK]`. No deferred variant: a reply can't be delivered after the call returns. A
method that wants "query, answer later out an outlet" is a `#[gimme]` with an outlet, as
today.

On a Max class (not a Jitter core class, whose Max wrapper owns the method the patcher uses)
`add_gimmeback` also calls `mark_undocumented` (`wrapper.rs`), the SDK's
`CLASS_METHOD_ATTR_PARSE(c, name, "undocumented", gensym("long"), 0, "1")` (`ext_obex_util.h`:
"an example which makes a method invisible to users"), after a successful registration, so the
selector is not in the patcher's message list: a patch cord cannot reach it.

The trampoline in `method.rs`:

```rust
pub(crate) extern "C" fn sel_gimmeback_tramp<W, T, F, R>(
    w: &W, sel: *mut t_symbol, ac: c_long, av: *const t_atom, rv: *mut t_atom,
) -> t_max_err
```

It writes `A_NOTHING` to `rv` first (if non-null), runs the user's function under
`ffi::guard_or(w, "gimmeback", MAX_ERR_GENERIC, ..)`, and converts the result as above. A
panic therefore returns `MAX_ERR_GENERIC` with `rv` already `A_NOTHING`. `object_method_typed`
always supplies `rv`; a null is tolerated anyway (the method runs, the reply is dropped) for a
direct caller that passes one.

The docs of `add_gimmeback` and `#[gimmeback]` must say that the method is for typed
callers (`js`, `object_method_typed`, the Jitter dumpout wrap) and that a patch-cord message
is not expected to reach it. A method that should answer both a patch cord and `js` registers
two selectors: a `#[gimme]` that sends out an outlet and a `#[gimmeback]` that
returns.

### Macro

`#[gimmeback("doAbs")]` in `#[median::methods]`, parsed by `parse_sel` with a third mode
(`gimme: true, back: true`), emitting `Class::add_gimmeback`. The method's return type must
implement `GimmebackResult`; a wrong one is an ordinary trait-bound error at the generated
call, which the `ui` tests cover. `#[defer]`/`#[defer_low]` on it is a spanned error, as on
`#[notify]`. The `reserved.rs` mirror needs no change: the refusals are the same as
`#[gimme]`'s.

### Jitter

Two places a gimmeback method can live on a Jitter type, matching where methods live today:

- `#[gimmeback]` (through `Methods::register`) lands on the Max glue class (`Forward`),
  like every other `#[sel]`. `js` can call it on the Max object; a patcher message is not
  expected to reach it. Nothing in the SDK wrapper is involved.
- `Class::<Core<T, K>>::add_gimmeback` in `class_setup` registers it on the Jitter class.
  To make the patcher reply go out dumpout, `Class` records `gimmeback_added` (like
  `notify_added`) and `setup_max_class` passes `MAX_JIT_CLASS_FLAGS_GIMMEBACK_WRAP` to
  `max_jit_class_wrap_standard` only when the core class has one. `class_setup` runs before
  `setup_max_class` (`jit/wrapper.rs`), so the flag is known in time. Today the flags are `0`;
  classes without a gimmeback method keep that.

The second path is the one for `get...`-style queries answered out dumpout, so it is the one
that matters; the first comes for free. Observed in Max 9.2 on 2026-10-27 (patch 05 part C): the
reply goes out dumpout as a list under the selector with its leading `get` stripped
(`getvalues 7` printed `dumpout: values 1 2.5 foo 7`, `getcount 1 2 3` printed `dumpout: count
3`). The mock does the same; a selector that is exactly `get` goes out as `get` there (a choice,
not observed).

### Calling one

Not in scope here, but the natural counterpart: `ObjRef`/`Object::call_typed(sel, atoms) ->
Result<Reply, MaxError>` over `object_method_typed`, which owns and frees an `atomarray` it
gets back. It would give the mock test below a median-level shape and lets an external query
another object. The host checks are done (see Status), so it can follow the first cut. It is also the only
caller that both reads and frees an object reply from a Max class.

For a `js` script, observed in Max 9.2 on 2026-10-27: `Maxobj.message(..)` on `median.validate`
reaches the method but hands back the `message` function object, not the reply, and `v8`'s
`message()` takes the typed-message route and refuses the selector. The SDK's route for a script
to receive a reply is the `simplejs` pattern: a `CLASS_NOBOX` class with `CLASS_FLAG_POLYGLOT`
(`ext_mess.h`) instantiated from the script with `new`. median does not support that yet (the
derive has no class-flag option). It remains the only way a script gets a value: not started.

## Mock

- `median-mock/src/class.rs`: `object_method_signature`/`invoke` learn `A_GIMMEBACK`
  (`(ptr, long, ptr, ptr)` after the receiver). A `Kernel::send` (a patch-cord message) to a
  gimmeback method is refused with an error on the console and never calls it, modelling the
  dispatch assumption above. New `object_method_typed`
  (resolves like `dispatch`; passes the caller's `rv` for a gimmeback method, writes
  `A_NOTHING` for any other) and `atomisatomarray` (the registry says whether the pointer is a
  mock atomarray). `Kernel::call(obj, sel, atoms) -> Result<Reply-like, SendError>` for tests:
  returns the atom, or the atoms of an atomarray reply after freeing it, so a leaked array
  shows up in the mock's live-object count.
- `median-mock/src/jit.rs`: `max_jit_class_wrap_standard` honours the `GIMMEBACK_WRAP` flag by
  installing, for each `A_GIMMEBACK` method of the Jitter class, a Max method that calls it
  and sends the reply out dumpout (an atomarray as its list, then freed; a scalar as a
  one-atom list; `A_NOTHING` as nothing), under the selector minus a leading `get` (observed
  2026-10-27; first written with the selector unchanged).
- Tests in `median-mock/tests/` for each new function, per `AGENTS.md`.

## Tests (`median/tests/gimmeback.rs`, `tests/ui/`)

- `#[gimmeback("doAbs")]` returning `Result<Reply, String>`: `k.call` gets the value for
  int and float arguments; a missing argument gives `Err`, `MAX_ERR_GENERIC`, the error on the
  console, and an `A_NOTHING` atom.
- A `Reply::Atoms` reply round-trips and leaves no live atomarray after `Kernel::call`.
- A patcher `k.send` to the method is refused and the method does not run.
- A panicking method returns `MAX_ERR_GENERIC`, `A_NOTHING`, and the panic text on the console.
- `add_gimmeback` on an untyped selector and on a reserved one is refused.
- `ui`: `#[gimmeback] #[defer]` is an error; a method returning `Atom` is an error.
- Jitter: a core-class gimmeback registered in `class_setup`; a patcher message produces the
  dumpout list; the wrap flag is passed only for that class.
- `simp`/`simp_explicit` gain a `doAbs`-style method so the hand-written form is shown.

## Host checks (add to `doc/validation-max.md`)

1. `js` calling the method: the reply arrives as a number / symbol / array / object; an `Err`
   shows the error in the Max window and `js` gets `null`. After many calls Max's memory does
   not grow (the caller frees an atomarray and a dictionary reply). If it grows, the ownership
   assumption is wrong and `Reply::Atoms`/`Reply::Dict` must change. (Outcome 2026-10-27: the
   reply is not readable from `js`, and memory grows, so `js` does not free it; see Status.)
2. A patcher message to the same selector: what Max reports, and that the method does not run.
3. Jitter: a message to a core-class gimmeback method sends the reply out dumpout; an
   atomarray reply as a list; which selector the reply carries (observed: without the leading
   `get`); no leak (recorded 2026-10-27, fourth run).

## Status

Step 1 is done (`median/src/gimmeback.rs`, `sel_gimmeback_tramp` in `method.rs`,
`Class::add_gimmeback`, the mock's `object_method_typed`/`Kernel::call`/`SendError::TypedOnly`,
`median/tests/gimmeback.rs`, `median-mock/tests/object_method_typed.rs`). Where it differs from
the sketch above:

- `Reply` has a seventh variant, `Array(AtomArray)`: `From<AtomArray> for Reply` can't be
  infallible without it (`Atoms(Vec<Atom>)` would have to copy the array out and can fail).
- `Atom` is neither `Copy` nor `Clone`, so the `_ => *a` in the `doAbs` sketch does not compile;
  the tests and the `add_gimmeback` docs use `Atom::from(a.get_symbol())`.
- A list that `AtomArray::new` refuses for a reason other than allocation (an object atom) is
  posted to `report_error` and answers `MAX_ERR_GENERIC`; only an allocation failure answers
  `MAX_ERR_OUT_OF_MEM`. Either way `rv` stays `A_NOTHING`.
- `Dict::from_raw` takes `Ownership::Retained` (not `Owned`) for a reference the caller already
  holds, which is what a test uses to take over a `Reply::Dict`.
- The mock's `Kernel::call` returns `Result<CallReply, SendError>`; a method that answers a
  non-zero `t_max_err` is `Ok(CallReply::Error(code))`. A patcher message to a gimmeback method is
  `SendError::TypedOnly` (a new variant: `NotSendable` documents `A_CANT`, this is an assumption).

Step 2 is done (`#[gimmeback("name")]` in `median_macros/src/methods.rs`; `ui` cases
`methods_gimmeback_{defer,signature,types}.rs`; `doAbs` in `simp` and `simp_explicit`, tested
through `tests/common/simp_script.rs`; `median/tests/gimmeback_macro.rs`). Where it differs from
the sketch above:

- `parse_sel` takes a `SelKind` (`Typed`, `Gimme`, `Gimmeback`) instead of the `gimme: bool`, and
  `Marker::Sel` carries it, rather than a third boolean mode.
- The `ui` cases are three files, not two: the macro's own refusals (a wrong number of arguments,
  an untyped or reserved selector, an option) suppress the compiler's trait-bound errors in the
  same crate, so those cannot share a file with the `GimmebackResult` and `GimmeFn` errors
  (`methods_gimmeback_signature.rs` and `_types.rs`).
- `Reply` is not in the prelude: the examples and tests import `median::gimmeback::Reply`.
- The `GimmeFn` diagnostic no longer mentions the return type (it is checked separately, by
  `MethodResult` or `GimmebackResult`).

Step 3 is done (`Class::has_gimmeback` in `median/src/class.rs`; `Entry::gimmeback`, `wrap_flags` and
the `gimmeback` parameter of `JitterObject::setup_max_class` in `median/src/jit/wrapper.rs`; the
wrap in `median-mock/src/jit.rs`; `median-mock/tests/jit_gimmeback.rs`;
`median/tests/jitter_gimmeback.rs`; the three host checks in `doc/validation-max.md`). Where it
differs from the sketch above:

- The flag is a local `const GIMMEBACK_WRAP: c_long = 0x1` in `jit/wrapper.rs`: the header's
  anonymous enum (`MAX_JIT_CLASS_FLAGS_GIMMEBACK_WRAP`) is not in the generated bindings, and
  regenerating them for one constant would mean regenerating every target.
- The flag reaches `setup_max_class` through `Entry` (the registry `Methods::register` of the glue
  already reads): `register` records `class.has_gimmeback()` after `class_setup`, and
  `setup_max_class(c, jit, gimmeback)` takes it as a third argument. `Class` records
  `gimmeback_added` only when a method is registered (not in the attributes-only pass over
  `#[median::methods]`, which registers nothing on the Jitter class).
- The mock's wrap is installed from the Jitter class's `A_GIMMEBACK` methods at the time
  `max_jit_class_wrap_standard` runs (after `class_setup`). A non-zero `t_max_err`, `A_NOTHING`
  and an object reply that is not an atomarray (freed) send nothing. The reply goes out
  synchronously through the dumpout outlet recorded by `max_jit_obex_dumpout_set` (the SDK's
  `A_DEFER_LOW` is not modelled). The mock has no `max_jit_obex_dumpout`; it sends through the
  recorded outlet directly. `median_mock::jit::wrap_flags(class_name)` exposes the flags a Max
  class passed. Without the flag the mock installs nothing for those selectors, as before.

Step 4 (the host checks): patch 05 (`median/examples/validation/patchers/05-gimmeback.maxpat`
and `gimmeback.js`, with the gimmeback methods of `median.validate` and `jit.median.validate`)
exists so the three host checks can be run in Max. Run on 2026-10-27 (Max 9.2 ARM64, results in
`doc/validation-max.md`):

- Patch cord (check 2): refused (`unknown message argument type`, `bad arguments`, `missing
  arguments`), the method does not run. The assumption holds. `Class::add_gimmeback` now marks
  the method `undocumented` on a Max class so the patcher does not list it; a Jitter core class
  is left alone (`median/tests/gimmeback_macro.rs`, `jitter_gimmeback.rs`).
- Jitter (check 3): the reply is a list out dumpout under the selector without its leading
  `get`; `median-mock` strips a leading `get` the same way. Memory (fourth run, commit 18ca359,
  `uzi 6000` into `getbig` on the print-less `jit.median.validate`, 384 MB of replies): no visible
  growth, so the SDK's dumpout wrap consumes and frees an object reply, as the design assumed.
- `js` (check 1): the method runs, `Err` posts, `getcount` is hidden from the message list (third
  run, cb437e1), but `Maxobj.message(..)` returns the `message` function object instead of the
  reply, so a reply can't be read through a box object (observed; the `simplejs` route is the
  SDK's way and is not started); `v8` is a negative control. Memory (fourth run): `burst 6000`
  (six thousand 4096-atom `getquiet` replies, 384 MB) took Max from 317 MB to 773 MB, 1.24 GB and
  1.69 GB over three clicks (peaks 1.02, 1.46 and 1.92 GB), about 450 MB per click, never
  released. `Maxobj.message` does not consume the reply, so it never frees an object reply, and
  every `Reply::Atoms`/`Array`/`Dict` reply to `js` leaks.

Consequence: an object reply is freed by a caller that consumes it and leaked by one that
discards it. On a Max (box) class prefer `Int`/`Float`/`Symbol`/`Nothing` unless the caller is
known to read the reply, which in Max today means another external calling `object_method_typed`;
object replies are for the Jitter dumpout path and such callers. The docs of `Reply`,
`GimmebackResult`, `Class::add_gimmeback` and the crate state this.

Step 4 is complete. What is left is the caller side (`call_typed`, "Calling one") and the
`simplejs` route for scripts, neither started.

## Order

0. `GimmeFn` and the selector-less `#[gimme]` (its own commit: it changes existing API).
1. `Reply`, `GimmebackResult`, trampoline, `add_gimmeback`, mock support, tests.
2. Macro marker and `ui` cases; examples.
3. Jitter wrap flag, mock dumpout wrap, Jitter test.
4. Host checks (done 2026-10-27); then `call_typed` on the caller side.
