# Plan: containers (dictionary, atomarray, string) and their ownership

Status: design, replaces the "merge feature/dict" step. Revised after a second review pass that
re-verified every kernel citation; the changes from the first draft are listed in §9.

Sources: the Max 8 SDK headers in `max-sys/thirdparty/max-sdk/source/c74support/max-includes`
(`ext_dictionary.h`, `ext_dictobj.h`, `ext_atomarray.h`, `ext_obstring.h`, `ext_obex.h`), the doxygen
page `doxygen/content/datastore.h`, and the SDK examples in `source/dictionary/`. Behaviour the
headers do not document was verified against the kernel source; those rows are marked *kernel*
in the table below and should be re-verified against a newer kernel before relying on them.

## 1. What the kernel actually does

| fact | documented in |
|---|---|
| Objects made with `class_new` (not "tiny" classes) are created with `retain = 1`. Dictionary, dictionary entry, atomarray and string are not tiny; clock, inlet, outlet, binbuf are tiny and skip refcounting entirely. | *kernel* |
| `object_free` and `object_release` are the same call (`freeobject`): decrement, free at zero. No force-free. Double release goes negative; the assert is commented out. | *kernel* |
| `object_retain` increments and fails on an object already at zero. The SDK header calls it "private, internal use only" but it is in the SDK's `c74_linker_flags.txt` and in the kernel's export lists, as are `object_release`, `dictobj_findregistered_retain`, `dictobj_release`. | *kernel*; `ext_obex.h:2267`, SDK `c74_linker_flags.txt` |
| `dictionary_free` frees each `A_OBJ` value with `object_free` (a decrement) unless the entry has `OBJ_FLAG_REF`/`OBJ_FLAG_DATA` (weak, never freed) or `OBJ_FLAG_MEMORY` (`sysmem_freeptr`). | *kernel*; `datastore.h` (when_to_free_a_dictionary) |
| `append{dictionary,atomarray,object}` store the pointer: "transfer one reference", no retain. `appenddictionary` asserts `d != value` (direct self-insert only; `C74_ASSERT` may compile out). The same child in two parents without an extra retain is a double free. | *kernel*; `ext_dictionary.h:183-212` |
| Replacing a key frees the old value (decrement) first, preserving order. | *kernel*; `ext_dictionary.h:76,89,...` ("existing value ... will be freed") |
| `appendatom_flags` can store the entry in the hashtab and then return an error if the linklist append fails; nothing is rolled back. | *kernel* |
| `appendatoms` copies the atom structs into a new atomarray and sets `ATOMARRAY_FLAG_FREECHILDREN`; `A_OBJ` pointers inside are copied **without retain**, so the array will later free objects the caller still owns. | *kernel*; `ext_dictionary.h:147-159` |
| `getatom`, `getlong`, `getsym`, `getobject` default to `stringtosymbol = 1`: reading a string entry **converts it in place** to a symbol and frees the `t_string`. `getatoms_ext(stringstosymbols = true)` does the same to array elements. `getstring` and the `entryis*` predicates do not convert. | *kernel* |
| Every getter that returns a pointer locks, reads the entry's own pointer, **unlocks, then returns**. `dictionary_copyatoms` copies *after* unlocking. So a concurrent replace or delete between unlock and the caller's copy/retain is a use-after-free. The only protection is holding `dictionary_transaction_lock` yourself (a per-dictionary recursive `systhread` mutex, on by default). | *kernel*; `ext_dictionary.h:828-852` (transaction lock) |
| `chuckentry` zeroes the entry's value and frees only the entry struct; it reads no flags and returns no pointer. The caller must have read the value *before* chucking, and inherits whatever the entry held: an owned reference (normal object entries), nothing (`REF`/`DATA`), or a `sysmem` pointer (`MEMORY`). | *kernel*; `ext_dictionary.h:509-522` |
| `deleteentry`/`clear` free (decrement) the values. | *kernel*; `ext_dictionary.h:509-539` |
| Atomarrays: `atomarray_new` copies atoms, no retain, `flags = 0`. They lock only with `ATOMARRAY_FLAG_USELOCKS`, which exists only in the Max 9 private header (`= 2`); the public SDK has only `FREECHILDREN`. `atomarray_clear` is shallow even with `FREECHILDREN` (children are dropped, not freed). Only `atomarray_free` honours `FREECHILDREN`. | *kernel*; `ext_atomarray.h:15-24, 275-283` |
| `atomisdictionary`/`atomisatomarray` dereference the atom's object pointer with no null or validity check. | *kernel* |
| Cross-dictionary ops (`copy_to_existing`, `merge_to_existing`, `clone_to_existing`, `dictionary_clone`) run `dictionary_funall` on the source while appending to the destination, so the source lock is held while the destination lock is taken. Two opposite-direction merges on two threads can deadlock. The kernel's own comment on `funall` says so. | *kernel* |
| `dictionary_clone`'s per-entry callback copies `REF`/`DATA` entries as shared weak pointers and appends a **null object** for anything `object_clone` cannot clone, while still returning success. | *kernel* |
| `dictobj_key_parse` splits on `::`, traverses children without retaining, and evaluates `s_name[strlen(s_name) - 1]` on each component, which reads one byte before the string for an empty component (`"::x"`, `"a::"`). | *kernel* |
| Registry: `dictobj_register(d, &name)` is `object_register(_sym_dictionary, name, d)`; empty name gets `symbol_unique()`. Cases: new name, same object: stored, returns `d`, no retain. Same object already registered under the same name: `object_retain(d)`, returns `d` (caller now holds two). Different object, name taken, same class: **input freed**, existing returned retained. Name taken by another class: input freed, returns NULL. Single-symbol-syntax rejection: input returned unregistered, not freed. | *kernel* |
| `dictobj_findregistered_retain` = `object_findregistered_retain` (+1 under the global registry lock; NULL if the object is dying). `dictobj_release` = `freeobject` (-1). `findregistered_clone` returns an unregistered clone at retain 1. | *kernel*; `ext_dictobj.h:48-72` |
| Freeing is decided by the retain count, not registration; at zero `freeobject` unregisters then runs the class free. | *kernel* |
| No public SDK call serializes a `t_dictionary` to a JSON string in memory. `dictobj_dictionarytoatoms` yields Max dictionary-syntax atoms; `dictobj_jsonfromstring` takes a dictionary-syntax *string*. The kernel's own path is the `jsonwriter` class: `object_new_typed(CLASS_NOBOX, "jsonwriter")`, `object_method(jw, "writedictionary", d)`, `object_method(jw, "getoutput", &handle)`, `object_attr_setlong(jw, "format", 0)` for compact. `jsonwriter_*` symbols themselves are not exported to externals. | *kernel*; `ext_dictobj.h:166-238` |
| `dictobj_validate` is schema validation (schema keys must exist, `"*"` is a wildcard, extra candidate keys allowed). Not equality. `dictionary_compare` exists but is internal. | `ext_dictobj.h:143-163` |
| Dictionary operations allocate and take a mutex; nothing documents any of them as audio-thread safe. | *kernel* |
| The `dictionary` message carries a registered *name* (`A_SYM`). Receivers `findregistered_retain` / use / `release`, or `findregistered_clone` to keep a copy ("we store dictionaries by copy, not by reference", `dict.pack`). Senders register (auto name) and `outlet_anything(_sym_dictionary, 1, &name_atom)`. | *kernel*; SDK `dict.route.c:104-132` |
| `appendtodictionary` (patcher save): deep-clone own dict, `appenddictionary` the clone into the patcher dict (transfer). | *kernel* |

Header-versus-example contradictions: `dictionary_getkeys` says "you must use `dictionary_freekeys`", the SDK's `dict.group` example uses `sysmem_freeptr`; follow the header. `getdefatoms` documents a `t_atomarray*` default but takes `t_atom*`; do not wrap it.

## 2. Design principles that follow

1. **One strong reference per handle, like `Arc`.** The kernel is refcounted for these classes and a
   retained child survives its parent, so `Dict` owns exactly one reference; `Clone` retains;
   `Drop` releases. This is the model `dict-test` already used for `&Dict` inserts.
2. **Every read is a copy or a retain, performed under the transaction lock.** Not after the
   getter returns: the kernel's own `copyatoms` races. `Dict` methods take
   `dictionary_transaction_lock`, classify the entry with the non-mutating predicates, copy or
   retain, then unlock. Borrowed pointers never cross the API boundary in safe code.
3. **Never let the kernel convert on read.** Classify with `entryisstring` /
   `entryisdictionary` / `entryisatomarray` first; read strings with `getstring`; call `getatom`
   only on entries already known not to be strings. Never use `getatoms_ext(stringstosymbols)`.
4. **Object atoms are opaque in safe code.** An `A_OBJ` atom owns nothing and the kernel's own
   predicates dereference it blindly, so the safe API never turns an atom into a `Dict` and never
   inserts an atom slice containing object atoms (`appendatoms` would later free objects the caller
   still owns). Both are `unsafe` escape hatches.
5. **Shared children are explicit.** `insert(key, dict)` by value transfers; `insert(key, &dict)`
   deep-clones (what `dict.pack` does); sharing one child between parents is a separately named
   call because it is the only way to make a cycle and the kernel has no cycle detection.
6. **No nested locks across dictionaries.** Cross-dictionary operations are implemented in Rust as
   snapshot-then-insert so only one dictionary is locked at a time; the kernel's `merge_to_existing`
   family is not wrapped.
7. **Atomarrays are build-only.** A plain atomarray has no lock in Max 8, so a shared handle to one
   cannot be made `Sync`. `AtomArray` is an owned, unshared builder you fill and hand to a
   dictionary; arrays *read* from a dictionary come back as `Vec<Atom>` copied under the
   dictionary's lock, never as a handle.

## 3. Types

### `Dict`

```rust
pub struct Dict(NonNull<t_dictionary>);     // Send + Sync: see §4
impl Clone for Dict { /* object_retain; shared, not a copy */ }
impl Drop  for Dict { /* object_free == release one reference */ }
```

Construction
- `new()`, `from_json(&str) -> Result<Dict, DictError>` (`dictobj_dictionaryfromstring`),
  `from_atoms(&[Atom]) -> Result<Dict, DictError>` (copies; fails on object atoms, see 4 above),
  `read(path) -> Result<Dict, DictError>`.
- `deep_clone(&self) -> Dict` (`dictionary_clone`). Caveats, documented on the method: weak
  (`REF`/`DATA`) entries created by other objects are shared, not copied; an entry the kernel cannot
  clone becomes a null object entry. Dictionaries built only through this API have neither.
- `unsafe fn from_raw(ptr, Ownership::{Retained, Borrowed})`; `Borrowed` retains so the handle
  always owns one reference. `into_raw(self) -> *mut t_dictionary` forgets the handle.

Insert (`&self`; the kernel locks per call)
- `insert(key, value) -> Result<(), DictError>` through a `DictValue` conversion trait (kept from
  `dict-test`'s `DictInsert` idea):
  - `i64`, `f64`, `SymbolRef`, `&str`/`&CStr` (copied into a `t_string`).
  - `&[Atom]`: copied via `appendatoms`, **rejected with `DictError::ObjectAtom`** if any atom is
    `A_OBJ`.
  - `Dict` by value: transfers the handle's reference (`into_raw` + `appenddictionary`).
  - `&Dict`: `deep_clone` then transfer. Never shares.
  - `AtomArray` by value: transfers (see below).
- `insert_shared(key, &Dict) -> Result<(), DictError>`: retain, then append. Documented as the
  only way to create a shared child or a cycle; rejects `ptr == self` (`DictError::SelfInsert`) and
  walks `other`'s descendants under `other`'s lock to reject an indirect cycle.
- `unsafe fn insert_object_raw(key, *mut t_object, ObjectFlags)` for arbitrary objects, weak
  (`REF`/`DATA`) entries, and `CLONE`.
- Failure semantics: if `append*` returns an error after a by-value transfer, the kernel may or
  may not have stored the value (hashtab stored, linklist failed). The wrapper **does not free** in
  that case (a leak is preferable to a double free) and returns `DictError::Kernel(code)`. This
  only happens on allocation failure.
- Replacing a key releases the old value; a `Dict` handle to it stays valid.

Read (copy or retain, all inside one transaction lock)
- `get(key) -> Option<DictValue>` where
  `enum DictValue { Int(i64), Float(f64), Symbol(SymbolRef), Str(CString), Atoms(Vec<Atom>), Dict(Dict), Object(ObjectPtr) }`.
  Order of classification under the lock: `hasentry`, `entryisstring` (copy via `getstring`),
  `entryisdictionary` (`getdictionary` + `object_retain`), `entryisatomarray` (`getatomarray` +
  `atomarray_getatoms` + copy; any `A_OBJ` elements are left as raw `Object` atoms in the `Vec`,
  which safe code cannot dereference), else `getatom` (now safe: not a string) → scalar, symbol, or
  `Object(ObjectPtr)` for other objects. `ObjectPtr` is a raw non-owning pointer newtype with only
  `as_ptr()`.
- Typed shortcuts: `get_int`, `get_float`, `get_sym`, `get_string -> Option<CString>`,
  `get_atoms -> Option<Vec<Atom>>`, `get_dict -> Option<Dict>`.
- `get_path("a::b::c")`: implemented in Rust by splitting on `::` and chaining `get_dict`
  (retained at each step). Empty components are an error. `dictobj_key_parse` is not used.
- `contains_key`, `len`, `is_empty`, `keys() -> Vec<SymbolRef>` (`getkeys_ordered`, copy,
  `freekeys`, all under the lock).
- `unsafe fn with_atoms_raw<R>(&self, key, f: impl FnOnce(&[Atom]) -> R) -> Option<R>`: holds the
  transaction lock and passes the borrowed slice. Unsound if `f` mutates this dictionary through
  any handle (the mutex is recursive). Hot paths only.

Remove (inside one transaction lock)
- `remove(key) -> Result<Option<DictValue>, DictError>`: classify as in `get`; for a dictionary
  read the pointer; for a string read the `t_string` pointer; for an atomarray copy the atoms and
  read the pointer; for scalars `getatom`. Then `chuckentry`. Then: dictionary → wrap as `Dict`
  (`Ownership::Retained`, we now own the one reference); string → copy out (already done) and
  `object_free` the `t_string`; atomarray → `object_free` it (children: see `AtomArray` below);
  scalar → done. For an entry classified as an unknown `Object`, **refuse** with
  `DictError::OpaqueObject` and leave it in place: its entry flags (owned / weak / sysmem) are not
  visible through the public API, so the wrapper cannot know what it would inherit. Use `delete`.
- `delete(key) -> bool` (`deleteentry`, kernel frees per entry flags), `clear()`.

Whole-dictionary operations (one lock at a time)
- `merge_from(&self, other: &Dict)`, `copy_unique_from`, `copy_to(&self, dst: &Dict)`: implemented
  as `other.keys()` then per key `other.get(k)` (locks `other` only) and `self.insert(k, v)` (locks
  `self` only). Values that come back as `DictValue::Dict` are shared handles, so for these
  operations they are inserted by value after `deep_clone`, matching `copy_to_existing`'s
  semantics for owned entries. Not atomic across the two dictionaries; documented.
- `to_json(&self) -> Result<String, DictError>`: via a `jsonwriter` instance
  (`object_new_typed(CLASS_NOBOX, "jsonwriter")`, `writedictionary`, `getoutput` into a
  `t_handle`, `format` attribute for compact). All through exported symbols; the exact `getoutput`
  signature must be confirmed in Max before implementation (§7, step 3).
- `write(path)`, `dump(recurse, console)`.
- `transaction(&self, f: impl FnOnce(&Dict) -> R) -> R`: a panic-safe guard around
  `dictionary_transaction_lock/unlock` for atomic multi-step updates on *one* dictionary. Rule,
  documented and asserted in debug builds: inside a transaction on `A`, do not call any method on
  another `Dict` that is not freshly created inside the closure. This is the lock-order rule that
  prevents the kernel's deadlock.

Registry (the `dictionary` message protocol)
- `Dict::find_registered(name) -> Option<Dict>` (`findregistered_retain`; Drop releases).
- `Dict::find_registered_clone(name) -> Option<Dict>`.
- `register(self, name: Option<SymbolRef>) -> Result<(Dict, SymbolRef), RegisterError>`:
  precondition `self.name().is_none()` (already-registered dictionaries are renamed with `rename`,
  below); otherwise `RegisterError::AlreadyRegistered(self)`. Calls `dictobj_register` on the
  consumed handle's pointer and interprets the result:
  - returned pointer == input: registered, the handle keeps its one reference;
  - returned pointer != input, non-null: the kernel freed our input and returned the existing
    dictionary retained; wrap the returned pointer (exactly one reference for the new handle);
  - null: name held by another class; our input was freed; `Err(RegisterError::NameTakenByOtherClass)`;
  - the single-symbol-syntax rejection returns the input unregistered and not freed:
    `Err(RegisterError::InvalidName(Dict))` with the handle recovered.
- `rename(&self, name) -> Result<Dict, RegisterError>`: the `dictobj_setname` pattern: create or
  `find_registered` the target, `register` it, release the old; returns the handle to use from now
  on. Re-registering the same object under its existing name (which would retain without release)
  is never issued.
- `name(&self) -> Option<SymbolRef>` (`dictobj_namefromptr`), `unregister(&self)`.
- Outlets: `Outlet<Any>::send_dict(&self, d: &Dict) -> Result<SymbolRef, SendError>`: if
  `d.name()` is `None`, register with an auto name first (consumes and returns a handle, so this
  takes `&mut Dict` or returns the new handle); then `outlet_anything(dictionary, [name])`. The
  sender must keep the dictionary alive while downstream may look it up; the kernel's `dict`
  objects keep theirs for their lifetime, `dict.group` sends then clears, so receivers must consume
  synchronously or clone.
- Receiving: `#[dictionary]` marker, `fn dictionary(&self, d: Dict)`. The trampoline registers
  `A_SYM`, does `find_registered`, reports "unable to reference dictionary named X" on `None`,
  calls the method; the `Dict` drop is the release. Mutating `d` in place is legal and visible to
  the sender (`dict.strip` does it, then notifies); offer `d.notify_modified()`.
- `#[savestate]` marker, `fn save(&self, patcher_dict: &Dict)`: the trampoline wraps the incoming
  pointer as a temporary retained handle for the call. Users do
  `patcher_dict.insert("data", self.state.deep_clone())`, transferring the clone, as `dictobj` does.

Atoms containing objects
- Safe code gets `AtomValue::Object(ObjectPtr)` and can do nothing with it.
- `unsafe fn Atom::as_dict_unchecked(&self) -> Option<Dict>`: caller asserts the object is alive;
  checks `atomisdictionary`, retains. Same for `as_atomarray_unchecked -> Option<Vec<Atom>>`.
- `unsafe fn Atom::from_object_raw(*mut t_object)`: stores a non-owning pointer. There is no safe
  `Atom::from(&Dict)`; an atom cannot keep its object alive, so the conversion was never safe.
- Never put an `A_OBJ` atom on an outlet (`dictobj_atom_safety` exists because receivers cannot
  handle it). `send_dict` is the supported path.

### `AtomArray`: an owned, unshared builder

```rust
pub struct AtomArray(NonNull<t_atomarray>);    // Send, !Sync, no Clone-as-share
```
- `new(&[Atom]) -> Result<AtomArray, DictError>` (copies; rejects object atoms), `push`, `extend`,
  `len`, `atoms() -> Vec<Atom>`, `clear`. `Clone` is a deep copy (`atomarray_duplicate`).
- Consumed by `Dict::insert` by value (`appendatomarray`, transfer). Not retained by reference:
  there is no shared `AtomArray` handle because a plain atomarray has no lock in Max 8, and
  `atomarray_clear` on an array with `FREECHILDREN` leaks children anyway.
- Never sets `FREECHILDREN` and never contains object atoms, so freeing it is always correct.
- Arrays read out of a dictionary surface as `DictValue::Atoms(Vec<Atom>)`.

### Strings

No public type. `insert(&str)` creates the `t_string` inside the dictionary; `get_string` copies
out under the lock. `t_string` is an implementation detail of dictionaries for our purposes.

### Not wrapped

`t_linklist`, `t_hashtab`, `t_indexmap`, `t_symobject`, `t_quickmap`, `t_database`: Rust
collections do the job and no Max API median calls needs them. `t_atombuf`, `t_binbuf`: not
referenced by anything we expose. `t_jit_matrix` belongs to the jitter plan. buffer~ has its own
plan (review H7, redesign G). `dictionary_funall`, `dictobj_key_parse`, `merge_to_existing` and
friends, `getdefatoms`/`copydefatoms`, `getatoms_ext`: deliberately not wrapped (§1 says why).

## 4. Threading contract

- `Dict` is `Send + Sync`: every method takes the dictionary's own recursive mutex for the whole
  lookup-classify-copy sequence, and every result is a copy or a retained handle.
- `AtomArray` is `Send`, not `Sync`, and has no shared clone, because the underlying object has no
  lock.
- Not audio-thread safe: every operation may allocate and takes a mutex. `debug_assert!`
  `!SysThread::is_audio()` at the top of each call in debug builds; document it.
- Lock order: a `transaction` on one dictionary must not call into another existing dictionary.
  Cross-dictionary helpers are written to lock one at a time. `deep_clone` locks the source and a
  freshly created private destination, which is safe.
- Other Max objects may hold the same dictionary (via the registry) and mutate it through the C
  API, including the kernel's own unlocked `copyatoms`. The transaction mutex is the protection
  boundary Max offers; median uses it correctly on its side and cannot do more.

## 5. `object_retain` is marked private

The SDK header says "private, internal use only (and perhaps not exported?)". It is in the SDK's
own `c74_linker_flags.txt`, and its use by Max's own externals was verified against the kernel source.
Relying on it is reasonable for a Cycling '74 project; record the dependency in `max-sys/README.md`
and keep every retain in one internal function (`Dict::clone`, `from_raw(Borrowed)`,
`insert_shared`, `get_dict`, `remove` of a dictionary) so a fallback is a local change. The public
pair `dictobj_findregistered_retain`/`dictobj_release` has the same semantics and is what the
`dictionary` message path uses anyway.

## 6. What to keep from the branches

- From `feature/dict-test`: the `DictInsert` conversion-trait shape (becomes `DictValue`), its
  retain-before-insert for `&Dict` (which was correct; it becomes `insert_shared`, while plain
  `insert(&Dict)` clones), `remove`/`len`/`keys`, `AtomArray` with `Drop`, `MaxError::map`, and
  `tests/dict.rs`.
- From `feature/dict`: the typed `append_*` methods as `insert` impls; `append_obj` becomes the
  `unsafe` raw insert.
- The branches' actual defects, for the record: `get` retained *after* the kernel getter unlocked
  (the race in §1), classification via `getatom` which converts strings in place,
  `From<AtomValue::Dict> for Atom` leaking +1 into a non-owning atom, `DictKeysIter`'s null slice,
  `locked()`'s asserts and non-panic-safety, no `AtomArray` `Drop` in `feature/dict`, `PartialEq`
  by pointer presented as equality.
- Drop: `DictKeysIter`, `from_obj` with its asserts, owning `AtomValue` variants, `PartialEq`
  (identity equality can be `ptr_eq`; structural equality is not provided until `to_json` exists).
- Write the module fresh against this plan and port the tests; do not merge either `dict.rs`.

## 7. Testing

Dictionaries live in MaxCore, so a Rust mock would have to re-implement them. This is the
strongest argument for route 1 in [plan-branches-and-build.md](plan-branches-and-build.md) 4b
(link Cycling '74's `mock` kernel, or MaxCore itself as `feature/maxcore-test` tried) for this
module. Acceptance tests:

1. `insert`/`get` round trip for every `DictValue` variant; strings and atoms compare equal after
   the copy; `insert(&[Atom])` with an object atom returns `ObjectAtom`.
2. Child survives parent: `parent.insert_shared("s", &sub); drop(parent); sub.insert("x", 1)` ok.
3. Transfer: `parent.insert("s", sub_by_value)`; `parent.get_dict("s")` yields a handle to the same
   object; dropping parent then the handle frees exactly once (observe via a registered name and
   `find_registered` returning `None` afterwards).
4. Clone on `insert(&Dict)`: mutating the original afterwards does not change the child.
5. Replace releases the old child but a held handle stays valid.
6. `remove` returns the value and leaves `len - 1`; a removed `Dict` is usable; `remove` on a
   string entry returns `Str` and does not convert the entry first (insert a string, `get_string`,
   `remove`, both return the text).
7. Registry: `register(None)` yields a unique name; `find_registered` twice gives two handles;
   dropping all frees and `find_registered` returns `None`; registering a second dictionary under
   an existing name returns the first with the input freed and no double free on drop.
8. `insert_shared` self and indirect cycle rejection.
9. Two threads inserting into one `Dict`, plus a third reading, for 10k iterations.
10. (step 3) `from_json`/`to_json` round trip including nested dictionaries and arrays.

## 8. Order

1. `Dict` core (construct, insert, get, remove, keys, registry) and `AtomArray`, tests 1-9.
2. `#[dictionary]`, `#[savestate]`, `send_dict` in the redesign's `#[methods]` (depends on
   [plan-api-redesign.md](plan-api-redesign.md) B).
3. `transaction`, `with_atoms_raw`, `to_json` via `jsonwriter` (verify `getoutput` in Max first),
   file I/O, test 10.

## 9. Changes from the first draft (after the second review)

- Reads and removals now happen *inside* the transaction lock; the kernel's own getters unlock
  before returning and `copyatoms` copies after unlocking.
- String entries are classified with `entryisstring` before any `getatom` call, because `getatom`
  converts the stored string to a symbol and frees it.
- `insert(key, &Dict)` deep-clones; sharing is `insert_shared` with cycle checks.
- `insert(&[Atom])` and `AtomArray::new` reject object atoms (`appendatoms` would free them).
- `AtomArray` is an unshared builder, not a shared handle; arrays read out are `Vec<Atom>`.
- `Atom` to `Dict` conversion is `unsafe`; there is no safe `Atom::from(&Dict)`.
- `remove` refuses opaque object entries; `chuckentry` is used only after the value is read.
- `get_path` is implemented in Rust; `dictobj_key_parse` has an out-of-bounds read on empty
  components.
- `register` is fallible and specifies all five kernel outcomes; `rename` is separate.
- Cross-dictionary operations are snapshot-then-insert, one lock at a time.
- `to_json` goes through `jsonwriter`; no public function does it directly.
- `PartialEq` dropped; `dictobj_validate` is not equality.
- Insert failure after a transfer never frees.
- §1 scoped refcounting to non-tiny classes; `object_retain` is in the SDK linker list.
