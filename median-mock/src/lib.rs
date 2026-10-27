//! # median-mock
//!
//! A test kernel for the Max C API, written in Rust. It defines, as exported C symbols with the
//! signatures of the Max SDK, the subset of the API that `median` calls, so that a crate that
//! links it (as a dev-dependency, with `use median_mock as _;` or any item from it to make sure
//! it is linked) can create objects, send them messages and drive clocks under `cargo test`, with
//! no Max installed. The variadic functions (`class_new`, `class_addmethod`, `object_method`,
//! `post`, `error`, `object_post`, `object_warn`, `object_error`) are defined in C, because Rust
//! cannot define variadic functions on stable, and forward to Rust.
//!
//! ## Using it
//!
//! ```ignore
//! let k = median_mock::lock();                  // exclusive, and a fresh kernel
//! my_external::ext_main(std::ptr::null_mut());  // registers the class (`class_new` ...)
//! let obj = k.new_object("my.object", &[1.into(), "foo".into()]);
//! k.send_int(obj, 5).unwrap();
//! assert_eq!(k.outlet_log()[0].atoms, [Atom::Long(5)]);
//! k.advance(10.0);                              // fire clocks due within 10 ms
//! k.free_object(obj);
//! ```
//!
//! ## Concurrency
//!
//! The kernel is process-wide state behind a mutex, because the C functions median calls have
//! nowhere else to keep it. **Tests that use it must not run concurrently**: take [`lock()`] at the
//! top of each and hold the returned [`Kernel`] to the end. The state mutex itself is only held
//! inside single API calls, never while a method, a clock callback or a free function runs, so
//! user code can call back into the kernel (and create threads that do) without deadlocking.
//! `gensym` has its own lock and is safe to call from any thread at any time.
//!
//! ## What is and is not modelled
//!
//! * Classes and objects: `class_new`, `class_addmethod`, `class_register`, `class_findbyname`,
//!   `class_addattr`, `class_dspinit`, `class_obexoffset_set`, `object_alloc` (zeroed,
//!   `o_messlist` pointing into the class as in Max), `object_free` (runs the class's free method
//!   first, then releases the object's inlets, outlets and registrations, then the memory),
//!   `freeobject`, `object_method` (see its docs for the restrictions).
//! * Inlets and outlets: `outlet_new`, `outlet_append`, `outlet_bang/int/float/list/anything`,
//!   `inlet_new`, `intin`, `floatin`, `proxy_new`, `proxy_getinlet`. `outlet_new` makes each new
//!   outlet the leftmost, as in Max, and `outlet_append` (which median uses) adds to the right.
//!   Outlets only record; they are not connectable to other objects. A proxy that its owner did
//!   not free before it is freed is reported on the console. [`Kernel::assist`] and
//!   [`Kernel::inlet_is_hot`] call the `assist` and `inletinfo` methods as Max does.
//! * Clocks: `clock_new`, `clock_new_withscheduler` (one scheduler only), `clock_delay`,
//!   `clock_fdelay`, `clock_unset`, `clock_getftime`, `gettime`, `scheduler_fromobject` (always
//!   null). Time moves only in [`Kernel::advance`].
//! * Main queue: `defer`, `defer_low`, `qelem_new/set/front/unset/free`, run by
//!   [`Kernel::run_main_queue`]. `defer` queues only when called from the timer role.
//!   [`Kernel::run_main_queue_nested`] and [`Kernel::advance_nested`] run the queue or the
//!   clocks from inside a callback, as a nested event loop (a modal dialog) does in Max.
//! * MSP: `z_dsp_setup`, `z_dsp_free`, `dsp_add64` (see [`Kernel::dsp64`], [`DspAdd::run`]). An
//!   object freed after `z_dsp_setup` without `z_dsp_free` is reported on the console.
//! * Method attributes (`CLASS_METHOD_ATTR_PARSE`): `class_extra_lookup` for `"methods"`,
//!   `hashtab_lookup` on that table, `object_addattr_parse` on the method object (see
//!   [`Kernel::method_attr`]).
//! * Attributes: `attribute_new`, `attr_offset_new`, `attr_addfilter*_clip`, `object_attr_touch`;
//!   `attr_args_process` does nothing. [`Kernel::attr_get`]/[`Kernel::attr_set`] call the
//!   accessors or read/write the offset.
//! * Attributes of attributes: `class_attr_addattr_parse`, `class_attr_addattr_atoms`, and
//!   `object_addattr_parse` on an attribute object, recorded per attribute (see
//!   [`Kernel::attr_attr`]); `gensym_tr` and `str_tr` without translation.
//! * Registration: `object_register`, `object_findregistered`, `object_unregister`,
//!   `object_subscribe`, `object_unsubscribe`, `object_attach`, `object_detach`, `object_notify`,
//!   `object_obex_lookup`, `object_obex_storeflags`.
//! * Buffers: `buffer_ref_*`, `buffer_locksamples` and friends, `buffer_edit_begin`/`end`,
//!   `buffer_getinfo`, `buffer_view`, over buffers made by [`Kernel::create_buffer`]. Claims and
//!   edits are tracked and misuse (a claim not released, unpaired edits) is reported; see
//!   [`buffer`].
//! * Symbols and memory: `gensym`, `common_symbols_gettable`, `atom_*`, `sysmem_*`,
//!   `strncpy_zero`, `systhread_is*thread`, `sys_getsr`.
//!
//! * Containers: retained dictionaries, registry, ordered entries, recursive transaction locks,
//!   strings and atomarray builders; see [`containers`]. JSON/file APIs are not modelled.
//! * Jitter: class/MOP/ob3d lifecycle, attribute forwarding, matrix fixtures and recorded immediate
//!   GL calls; see [`jit`]. Callbacks are driven directly; there is no renderer or worker scheduler.
//!
//! * Files: `locatefile_extended`, `open_dialog` (always cancelled), `path_opensysfile`,
//!   `sysfile_readtextfile` and `sysfile_close`, over in-memory files registered with
//!   [`file::add_file`]; see [`mod@file`].
//!
//! Not modelled: the other file and path functions (`sysfile_*`, `path_*`), `object_new`, patcher/box access, ordinary Max `@attr` argument
//! parsing, signal-rate connections between objects, inlet/outlet connections, and everything
//! else not listed above. A call to a function that is not defined fails at link time, or at load
//! time if the binary links lazily.

// `c_long`, `c_short` and friends are written out in the signatures that mirror the SDK, so the
// code is the same on Windows, where `c_long` is 32 bit; on this target some casts are the
// identity.
#![allow(clippy::unnecessary_cast)]

pub mod atoms;
pub mod attr;
pub mod buffer;
pub mod class;
pub mod clock;
pub mod console;
pub mod containers;
pub mod dsp;
pub mod file;
pub mod io;
pub mod jit;
mod kernel;
pub mod notify;
pub mod queue;
mod state;
pub mod symbols;
pub mod sysmem;
pub mod thread;

pub use atoms::Atom;
pub use attr::{AttrError, GetterArgs, GetterResult};
pub use class::SendError;
pub use kernel::{Kernel, lock};
pub use state::{ConsoleEntry, DspAdd, InletKind, Level, OutletEvent};
pub use thread::Role;
