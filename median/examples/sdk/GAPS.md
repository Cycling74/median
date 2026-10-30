# Gaps

What the SDK examples need that median has no safe API for. Each entry names the Max API, the
examples that use it, and what those ports do instead. In the code, the raw calls are marked
`// GAP:`.

Three sections:

- API gaps: median cannot express it; the port calls `max_sys` or changes behaviour.
- Rough edges: median can express it, but awkwardly.
- Mock limitations: the port takes a path in Max that `median-mock` cannot run, so its test
  skips it.

## API gaps

### Classes and methods

#### `A_CANT` methods with a custom selector or arguments

median registers only its own fixed `A_CANT` methods (`dblclick`, `loadbang`, `notify`, ...).
Another one needs `class_addmethod` on `Class::inner()` and a hand-written trampoline that
recovers the wrapper from the object pointer and calls `median::ffi::guard`, which is
`#[doc(hidden)]`.

- dummy: `blooop` (no arguments) is registered raw.
- simpletext: `edclose(x, char **text, long size)`, the `jed` editor's close callback, is
  registered raw.
- mc.pack~: `helpname(x, char *)` is registered raw and writes the class name.

#### `CLASS_METHOD_ATTR_PARSE`

No API sets an attribute on a method (`undocumented`, `documentable`). median marks its own
gimmeback methods undocumented with a crate-private helper.

- dummy: marks `identify` undocumented and `blooop` documentable through
  `class_extra_lookup`/`hashtab_lookup`/`object_addattr_parse`.

#### `object_addmethod`

No instance methods.

- windowwatcher: leaves out the SDK's `boxscreenrectchanged` output, which adds a method to its
  box and recovers the object from the box's first inlet.

#### Class flags (`CLASS_FLAG_POLYGLOT`) and `c_noinlet`

`#[max(class_type = nobox)]` covers `CLASS_NOBOX` only; there is no way to set other class flags
or make a class whose boxes have no inlet.

- simplejs: ORs `CLASS_FLAG_POLYGLOT` into `(*class.inner()).c_flags` in `class_setup`.
- servant: sets `c_noinlet` on `Class::inner()` in `class_setup`.

#### `class_setname`

No safe way to tie an object name to a file name.

- times~: registers no `median.*~` alias (one belongs in the package's object mappings).

### Attributes

#### `CLASS_ATTR_CHAR`

Field attributes are `Int64`, `Float64` and `Float32` only; there is no `char` field type,
though `AttrType::Char` exists.

- attrtester: `usemin` and `usemax` are `Int64` with `style = "onoff"`.
- vexpr: `scalarmode` is an `Int64` attribute.
- limi~: `bypass` and `dcblock` are `Int64` with `style = "onoff"`, `mode` an `Int64` with
  `enum = "Linear Exponential"`.

#### `CLASS_ATTR_BASIC`

No attribute option marks an attribute basic (shown in the inspector's basic view).

- dict.group: `name` is not basic.
- dict.strip: `keys` is not basic.
- lores~: `cutoff` and `resonance` are not basic.
- vexpr: `scalarmode` is not basic.
- mc.pack~: `chans` is not basic.

#### `CLASS_ATTR_SYM_VARSIZE` (array attributes)

`#[attr]` and `Class::add_attr` hold one value.

- dict.strip: registers `keys` with `AttrBuilder::new_accessors` and hand-written `extern "C"`
  accessors (see the rough edge on custom accessors).

#### ITM time attributes and objects

Nothing for `class_time_addattr`, `time_new`, `time_setvalue`, `time_schedule`,
`time_getticks`, `time_getitm`, `itm_tickstoms`, `time_stop`; the `TIME_FLAGS_*` constants
(`ext_itm.h`) are missing from the bindings. A time object's task gets the owner pointer, so it
needs a raw trampoline as for `A_CANT` methods.

- delay2: everything ITM is raw, with the flag values copied from `ext_itm.h`; the proxy inlet,
  the clock, outlets and inlet info are median.

### MSP

#### `dsp64` `count[]` (which signal inlets are connected)

median's `dsp64` trampoline drops `count`, so an object cannot tell a connected signal inlet from
an unconnected one, and the SDK's choice of perform routine (or of float vs signal for a control
inlet) cannot be ported. Every port below uses one rule instead: a value set by a float, an
argument, an attribute or `set` replaces that inlet's signal from then on; until then the signal
is used. Unconnected signal inlets read zeros, the same as the defaults, so the ports differ from
the C only when an inlet that has had a value is also connected.

- plussz~: a float replaces the signal of the inlet it arrives at; the argument the right one's.
- times~: as plussz~.
- split~: the low and high limits from arguments or floats replace inlets 1 and 2.
- lores~: cutoff and resonance from arguments, floats or the attributes replace inlets 1 and 2.
- simpwave~: start and end from arguments, floats or `set` replace inlets 1 and 2; a `set` that
  leaves one out returns it to the signal.

The same trampoline hides the dsp64 chain object, so `getnuminputchannels` (per-inlet MC channel
counts) cannot be called either.

- mc.pack~: an inlet's channel count is the last one `inputchanged` reported (1 until then, what
  Max gives an unconnected inlet).

#### More than 64 signal channels (`MAX_SIGNAL_IO`)

`perform` gets its slices from stack tables of 64; an object with more signal inlets or outlets,
or an MC signal wider than 64 channels, gets an error at `dsp64` and silent outputs. MC signals go
to 1024 channels (`MC_MAX_CHANS`).

- limi~: clamps its channel argument to 1..=64 instead of 1..=1024, silently, as the SDK clamps.
- mc.rotate~: the SDK takes up to 512 channels; the port is silent above 64.
- mc.pack~: the SDK takes up to 1024 inlets or output channels; the port is silent above 64 of
  either.

#### Multichannel signal outlets (`"multichannelsignal"`)

No outlet kind for it, and `t_outlet` is opaque in max-sys, so an `Outlet::signal()` cannot be
retyped. A raw `outlet_append(obj, null, gensym("multichannelsignal"))` made in `new` after the
builder's outlets keeps their numbering (a raw outlet made before them would shift every later
builder outlet's assist text; `outlet_new` would make it leftmost and shift all of them). median's
table has no entry for it, so its assist text must come from the `assist` hook, and median's
signal-outlet count leaves it out (used only for the 64 check; `perform` gets Max's real
`numouts`).

- mc.rotate~, mc.pack~: raw outlet appended last, assist from the hook.

#### MC channel protocol (`multichanneloutputs`, `inputchanged`)

Both are on median's untyped-selector list, which refuses them as typed methods, and there is no
hook for them.

- mc.rotate~, mc.pack~: registered raw with `class_addmethod(.., A_CANT)` and hand-written
  trampolines (`long f(x, long index)`, `long f(x, long index, long count)`) that call
  `median::ffi::guard_or`; mc.pack~'s are generic over its two classes.

#### `dsp64` `maxvectorsize`

Also dropped by the trampoline.

- dspstress~: measures the vector `perform` is given instead.
- lores~: the SDK picks an unrolled loop when it is at least 4; the port has one loop.
- fftinfo~: outputs the FFT size (half of it without full spectrum) as the spectral frame size,
  which is what `pfft~` runs its subpatch at.

#### `z_misc` flags (`Z_PUT_FIRST`, `Z_MC_INLETS`)

No builder option for them, and max-sys has none of the `Z_*` constants. There is also no hook
after the builder's `z_dsp_setup`, so a port ORs its flag in during `new`, before `z_dsp_setup`
runs; the SDK examples (and median for `Z_NO_INPLACE`) set it after. Whether `z_dsp_setup` keeps
bits set before it is untested; if it clears them, the MC ports see only the first channel of
each input.

- fftinfo~: ORs a local `Z_PUT_FIRST` into `(*builder.msp_obj()).z_misc`.
- mc.rotate~, mc.pack~: OR a local `Z_MC_INLETS` (32) the same way.

#### `pfft~` introspection (`t_pfftpub`, `__pfft~__`)

max-sys does not bind `r_pfft.h`, and median has nothing for finding the owning `pfft~`.

- fftinfo~: reads `gensym("__pfft~__")->s_thing` raw through a local `#[repr(C)]` copy of
  `t_pfftpub`.

### Objects, registration and notification

#### `object_notify` with data, and notification data

`ObjRef::notify` always passes null data, and `Notification::data` is a raw pointer with no typed
accessor, even for the common case of an atomarray.

- master: builds an `AtomArray`, takes it with `into_raw`, and calls `object_notify` and
  `object_free` itself.
- servant: reads the data in `unsafe` through `Atom::from_object_raw(..).as_atomarray_unchecked()`,
  relying on its subscription's class filter for what the pointer is.

#### `object_findregistered` and shared NOBOX objects

No safe lookup of a registered object that hands back a usable handle (`Registration::try_register`
uses it only as a collision test), and no retain/release of a shared NOBOX object.

- master: keeps its conduits, with how many masters use each, in a main-thread `thread_local!`
  list of `ObjBox`es instead of finding them in the registry and sending `retain`/`release`.

#### `object_new` of other classes, and messages and attributes of other objects

No way to own a `t_object` made with `object_new`, call another object's method
(`object_method`), or read or set its attributes (`object_attr_getobj`, `object_attr_get_rect`,
`object_attr_setchar`/`setsym`).

- simpletext: opens Max's text editor (`jed`) raw, keeps it in an `AtomicPtr` and frees it on
  drop.
- windowwatcher: reads the patcher's `firstview` and the view's `rect`/`visiblecanvasrect` raw,
  and calls `getname` on the attribute an `attr_modified` notification carries.

#### Patcher access and attaching by pointer

No way to find the object's patcher or box (`object_obex_lookup` `#P`/`#B`);
`notify::Attachment` attaches by namespace and name only, not to an object by pointer
(`object_attach_byptr_register`, `object_detach_byptr`).

- windowwatcher: looks up `#P` raw and keeps the patcher pointer (the patcher outlives its
  boxes); attaches to the view raw in a queue callback, detaches on the view's `free`
  notification and on drop, and keeps the view in an `AtomicPtr` that is null whenever it is not
  attached.

#### `quittask_install`

No way to run code when Max quits.

- threadpool: its shared pool's four threads, started in `class_setup`, are never stopped; at quit
  they sit idle on a condvar instead of being ended by a quit task.

#### `symbol_unique`

No safe way to get a unique symbol; `Dict::register(None)` gets one only for a dictionary.

- dummy: calls `max_sys::symbol_unique` when there is no name argument.

#### `gettime_forobject`

`Clock::ftime` reads the current scheduler's time (`clock_getftime`); there is nothing for the
time of the scheduler an object belongs to.

- thresh: uses `Clock::ftime`.

### Files and paths

#### Binary file reading and `path_toabsolutesystempath`

`FilePath` gives a Max path ID and file name, but no absolute system path, so `std::fs` cannot be
used on what it finds; and median has no binary reading (`sysfile_read`, `sysfile_setpos`,
`sysfile_geteof`), only `FilePath::read_text`.

- filein: gets the system path raw, then reads and seeks with `std::fs::File`.
- filebyte: the same, keeping the `File` open in a `Mutex<Option<File>>` between reads.

#### Modification dates (`path_getfilemoddate`, `path_getmoddate`, `path_resolvefile`, `systime_secondstodate`)

No way to get a file's or folder's modification date, to resolve a name `FilePath::locate` does
not find (a folder) to a path ID, or to convert Max's seconds-since-1904 dates to calendar fields.

- filedate: calls them raw, after `FilePath::locate` for files.

#### Folders (`path_frompathname`, `path_openfolder`, `path_foldernextfile`, `path_closefolder`, `path_extendedfileinfo`, `typelist_make`)

No folder lookup or enumeration, no file type sniffing, and no standard list of file types
(`median::file::fourcc` makes single codes).

- folder: calls them raw; the open folder is closed by a `Drop` guard, and `typelist_make` runs
  when a folder is first listed rather than in `new`.

#### `jed` text editor

See "`object_new` of other classes" above.

### Databases and text

#### Databases (`ext_database.h`: `db_open`, `db_close`, `db_query*`, `db_result_*`)

No binding. `db_query` is variadic and printf-style, so a caller formatting values into SQL must
escape them itself.

- dbcuelist: everything is raw, through `db_query_direct` with SQL the port formats, names and
  values written as single-quoted literals with `'` doubled (the C's double quotes make SQLite
  read a value as a column name when one matches, so `cue.destroy name` deleted every cue);
  results are freed by a `Drop` guard; the handle is in a `Mutex` with an `unsafe impl Send`.

#### `atom_setparse`

No way to parse text into atoms (the reverse of `Atom::text`).

- dbcuelist: calls it raw, reads the result as `&[Atom]` through the `#[repr(transparent)]`
  cast, and frees it with `sysmem_freeptr`.

### Dictionaries and expressions

#### `dictobj_validate`

No schema matching for dictionaries.

- dict.route: calls it raw on the two retained dictionaries.

#### `dictionary_clone_to_existing`

No in-place replacement of a dictionary's contents.

- dict.route: `Dict::clear` then `Dict::copy_to`, which refuses a source holding an array of
  dictionaries (posted as an error).

#### `dictobj_key_parse`

No path lookup that returns the containing dictionary and key; `Dict::get_path` only reads, and
only `a::b` paths (no `[n]` indexes).

- dict.strip: calls it raw for keys with `::` or `[`, and wraps the result with
  `Dict::from_raw(.., Ownership::Borrowed)`; plain keys use `Dict` directly.

#### `expr_new`, `expr_eval`

No binding of `ext_expr.h`.

- vexpr: owns the `t_expr` in a `Send` wrapper behind a `Mutex`, freed with `freeobject` on drop;
  takes the variable types from `expr_new`'s type array instead of reading `exp_var`.

## Rough edges

### `Atom` is not `Send` or `Clone`, and has no `A_NOTHING`

An object struct must be `Sync`, so `Mutex<Vec<Atom>>` cannot hold atoms an object keeps between
messages; copying atoms goes through `get_value`.

- buddy, iter, match, past, thresh, vexpr: each defines a `Copy` enum of int, float and symbol,
  converted from atoms on input and back on output.
- dict.group: copies atoms with a `copy_atom` helper.
- linky, threadpool: the same; threadpool's task arguments cross to the worker and back as it.
- vexpr: makes the `A_NOTHING` atoms `expr` needs for unused arguments with `std::mem::zeroed`.

### Raw `max_sys` calls take `*mut t_atom` from `&[Atom]`

`Atom`'s `t_atom` is `pub(crate)`; passing atoms to an SDK function needs
`atoms.as_ptr().cast_mut().cast()`, relying on `#[repr(transparent)]`.

- delay2: `time_setvalue` with message and creation atoms.

### Outlets refuse object atoms

`Outlet::send` rejects a message holding an object atom, which the SDK sends as is.

- dict.strip: an object value (or an array holding one) is not sent.

### An outlet kind chosen at run time

`Outlet<K>` fixes the kind in the type, so an outlet that is int or float depending on an
argument needs an enum.

- minimum: an `enum` of `Outlet<Int>` and `Outlet<Float>` for its left outlet.

### Proxy inlet IDs are positions

`ObjRef::proxy_inlet` returns the inlet's position; the SDK lets a proxy's ID skip numbers.

- vexpr: maps each inlet position to its expression variable itself.

### Errors without `Display`

`dict::RegisterError` (from `Dict::register`, `Dict::rename`) and `notify::RegistrationError`
(from `try_register`) cannot be posted with `{}` or turned into a `NewError` with `?`.

- dict.group, dict.route: format `RegisterError` with `{:?}`.
- master: replaces `RegistrationError` with its own message.

### `Registration` and `Subscription` are not `Sync`

They are `Send` only, so an object that keeps one must wrap it in a `Mutex` it never locks.

- master: the conduit keeps `Mutex<Registration>`.
- servant: keeps `Mutex<Subscription>`.

### `MaxObjWrapper::new` panics when `new` refuses

Creating a NOBOX object from Rust has no fallible form; a refusing `new` panics in the caller.

- master: creates its conduit with it (the conduit refuses only an empty name, which the master
  never passes, or a registration collision).

### Worker threads return results through a `Queue`

A `#[defer]`/`#[defer_low]` method is reached only by a message, and neither the object nor its
`ObjRef` is `Send`, so a thread of the object's own cannot call `defer_low` on it as the C does.

- simplethread: sets a `Queue` whose callback outputs the count.
- threadpool: the completion pushes its arguments into a `Mutex<Vec<_>>` and sets a `Queue`.

### Deferring from code

`#[defer]` defers a whole method, unconditionally; there is no way to defer a call only in some
cases (the SDK's `if (systhread_istimerthread()) defer(..)`), and `Inlet::int` callbacks cannot
be deferred.

- filein: queues spooled reads that arrive on the scheduler thread in a `Mutex<Vec<_>>` and
  drains it from a `Queue`.

### Custom attribute accessors

A hand-written accessor for `AttrBuilder::new_accessors` needs `median::ffi::guard_or`, which is
`#[doc(hidden)]`, and a getter returning several atoms must allocate them with raw
`sysmem_newptr`.

- dict.strip: does both for `keys`.

### No cell for state only `perform` writes

Filter memory and coefficients that only the audio thread touches still have to be atomics
(`Float64`, `Relaxed`) to keep the object `Sync`.

- lores~: keeps its filter state in `Float64`s; `clear` and `dsp_setup` set flags that `perform`
  acts on rather than writing the state themselves.
- limi~: the state includes lookahead buffers, which atomics do not cover; it is in a `Mutex`
  only `perform` locks, with `try_lock` (silence if it ever failed), and setters store into
  atomics and set a flag that `perform` applies.

### Attribute setters are found by the getter's method name

The setter of a `#[attr]` getter must be `set_<getter fn name>`; with
`#[attr(name = "bypass")] fn get_bypass` the macro looks for `set_get_bypass` and, finding none,
makes the attribute read-only without a diagnostic. Setting it still reports success.

- limi~: names the getters after the attributes.

### Two classes from one struct

Two classes need two Rust types, and the derive does not take generics.

- mc.pack~: shared state in a plain struct; a local `macro_rules!` emits the two derived types and
  a trait makes the raw trampolines generic over both.

### Externals that link a shared library

Cargo has no stable artifact dependency, so an external that links a `cdylib` of the workspace
orders the build with an unused path dependency and links it from `build.rs`.

- mydll: a `cdylib` without `[package.metadata.max]`; `build.rs` sets the install name
  `@loader_path/../../../../support/libmydll.dylib` (the package's `support/`).
- dllcounter, dllreport: declare mydll's functions in an `unsafe extern "C"` block and link it
  with `rustc-link-lib=dylib=mydll` from `<profile>/deps`. Windows linking is not handled.

## Mock limitations

### Functions the mock does not implement

A port that reaches one of these in a test aborts the test process ("missing symbol called"), so
those paths are untested.

- ITM (`class_time_addattr`, `time_*`, `itm_*`): delay2 calls `class_time_addattr` at
  registration, so it has no tests at all.
- `symbol_unique`: dummy's unnamed path.
- `object_new` and the `jed` class: simpletext's double-click; `edclose` is tested by calling the
  method directly.
- Dates, folders and system paths (`path_getfilemoddate`, `path_getmoddate`, `path_resolvefile`,
  `systime_secondstodate`, `path_toabsolutesystempath`, `path_frompathname`,
  `path_openfolder`/`foldernextfile`/`closefolder`, `path_extendedfileinfo`, `typelist_make`):
  filedate tests only the empty name; filein only the missing-file errors and a closed object;
  folder only the count with no folder and the `types` parsing.
- `expr_new`/`expr_eval`: a vexpr cannot be made; its tests cover registration and the
  evaluation loop with a stand-in evaluator.
- Patchers (`#P` in `object_obex_lookup`, `object_attr_getobj`, `object_attach_byptr_register`,
  `object_attr_get_rect`): windowwatcher tests only an object outside a patcher.
- `dictobj_validate`, `dictobj_key_parse`: dict.route's routing and dict.strip's key paths
  (`a::b`, `a[0]`).
- `path_toabsolutesystempath`: filebyte tests only the missing-file errors, a cancelled dialog
  and reading without a file.
- `db_*`: dbcuelist tests only an object without a `name`; the queries, `cue.do` output and
  `atom_setparse` are untested.

### `dictobj_dictionaryfromatoms` parses only `key: value` pairs

Max's syntax also has a standalone `:` and several values per key (`a : 1 2 3`).

- dict.group: the implicit-colon and multi-value forms are untested.
- dict.route: the schema arguments are tested only as `a: 1`.

### `open_dialog` is always cancelled

- simpletext: `read` with no name is tested only as doing nothing.

### `c_noinlet` is ignored

The mock creates the same iolets whatever `c_noinlet` says.

- servant: that it has no inlet is untested.

### MC

The mock's `dsp64` sizes `count[]` from `"signal"` outlets only, and nothing models channel
counts, `Z_MC_INLETS`, or Max calling `inputchanged`/`multichanneloutputs`.

- mc.rotate~, mc.pack~: the tests call those methods (and `helpname`) directly and run `perform`
  with several channel counts; that Max calls them, and in what order, is untested.

### A shared library's Max calls in a test

The test binary keeps only the mock functions something in it references, so a separately
linked library resolving `post` by `dynamic_lookup` finds nothing ("missing symbol called"). The
library is found under `cargo test` only through the library path cargo sets; its install name
does not resolve there.

- dllcounter, dllreport: each test references `median::max_sys::post` through
  `std::hint::black_box` to keep the mock's `post`, then checks mydll's post and the shared count.

### No `pfft~`

- fftinfo~: the test binds a zeroed `t_pfftpub` to `__pfft~__` itself; what a real `pfft~`
  binds, and when, is untested.

### Two externals in one test

The crates are `cdylib` only, so one crate's test cannot register another crate's classes.

- master: tested with a test-only subscriber class standing in for `median.servant`.
- servant: tested with a test-only `median.conduit` that sends `sendmessage` with raw
  `object_notify`.
