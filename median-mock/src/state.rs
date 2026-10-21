//! The global kernel state.
//!
//! One `Mutex<State>` holds everything except the symbol table (which has its own lock so that
//! `gensym` never contends with, or deadlocks against, the rest). The lock is only ever held
//! inside a single API call and is **never held while user code (a method, a clock callback, a
//! free function ...) runs**, because that code calls straight back into the kernel.
//!
//! Pointers are stored as `usize` so the state is `Send` without any `unsafe impl`.

use crate::atoms::Atom;
use crate::thread::Role;
use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex, MutexGuard};

/// A method registered with `class_addmethod`.
#[derive(Clone, Debug)]
pub(crate) struct MethodRec {
    /// The function pointer, never 0.
    pub fun: usize,
    /// The `A_*` argument types, without the terminating 0.
    pub types: Vec<i32>,
}

pub(crate) struct ClassData {
    pub name: String,
    /// The namespace given to `class_register`, if it has been registered.
    pub namespace: Option<String>,
    pub new_fn: usize,
    pub free_fn: usize,
    pub size: usize,
    pub new_types: Vec<i32>,
    pub methods: HashMap<String, MethodRec>,
    /// Attribute objects, in the order they were added.
    pub attrs: Vec<usize>,
    pub dsp: bool,
    pub obex_offset: i64,
    /// Pointer to the second element of the class's two element `messlist` array; objects'
    /// `o_messlist` points here and element `[-1]` holds the class pointer, as in Max.
    pub messlist: usize,
}

pub(crate) struct ObjData {
    pub class: usize,
    /// Outlets in creation order (which is right to left).
    pub outlets: Vec<usize>,
    /// Inlets in creation order (which is right to left).
    pub inlets: Vec<usize>,
    /// `Some(n)` once `z_dsp_setup(x, n)` has been called.
    pub signal_inlets: Option<usize>,
    /// The obex store: key -> (value, flags).
    pub obex: HashMap<String, (usize, i64)>,
    /// `(namespace, name)` if registered with `object_register`.
    pub registered: Option<(String, String)>,
}

#[derive(Clone, Debug)]
pub(crate) enum InletKind {
    /// From `proxy_new`.
    Proxy { id: i64, stuffloc: usize },
    /// From `intin`: messages go to the method `in<n>`.
    Int(i16),
    /// From `floatin`: messages go to the method `ft<n>`.
    Float(i16),
    /// From `inlet_new`.
    Named(Option<String>),
    /// Created by `z_dsp_setup`.
    Signal,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Clip {
    pub min: f64,
    pub max: f64,
    pub use_min: bool,
    pub use_max: bool,
}

pub(crate) struct AttrData {
    pub name: String,
    pub type_name: String,
    pub flags: i64,
    pub getter: usize,
    pub setter: usize,
    /// `Some` for attributes made with `attr_offset_new`.
    pub offset: Option<i64>,
    pub set_clip: Option<Clip>,
    pub get_clip: Option<Clip>,
}

pub(crate) enum SubKind {
    Outlet {
        owner: usize,
        ty: Option<String>,
    },
    Inlet {
        kind: InletKind,
    },
    Clock {
        obj: usize,
        fun: usize,
        fire: Option<f64>,
        seq: u64,
    },
    Qelem {
        obj: usize,
        fun: usize,
        set: bool,
    },
    Attr(AttrData),
    BufferRef {
        name: String,
    },
}

/// Something with its own pointer that is not a class instance. These records are never removed,
/// only marked dead, and the pointer they are keyed by is a leaked allocation that is never
/// reused, so a stale pointer is detected rather than aliasing a newer object.
pub(crate) struct Sub {
    pub alive: bool,
    pub kind: SubKind,
}

pub(crate) struct BufferData {
    pub ptr: usize,
    pub name: String,
    pub channels: usize,
    pub frames: usize,
    pub sample_rate: f64,
    /// Interleaved samples.
    pub data: Vec<f32>,
    pub locks: usize,
    pub dirty: bool,
}

pub(crate) struct Subscription {
    pub namespace: String,
    pub name: String,
    pub classname: Option<String>,
    pub client: usize,
}

pub(crate) enum Deferred {
    Call {
        obj: usize,
        fun: usize,
        sym: usize,
        atoms: Vec<Atom>,
    },
    Qelem(usize),
}

/// A call to `dsp_add64`.
#[derive(Clone, Copy, Debug)]
pub struct DspAdd {
    pub object: usize,
    pub perform: usize,
    pub flags: i64,
    pub userparam: usize,
    /// The `t_object` passed as the dsp64 chain.
    pub dsp64: usize,
}

/// An entry of the outlet log, see [`crate::Kernel::outlet_log`].
#[derive(Clone, Debug, PartialEq)]
pub struct OutletEvent {
    /// The outlet pointer.
    pub outlet: usize,
    /// The object that owns the outlet.
    pub owner: usize,
    /// The outlet's index counted from the left (the order `outlet_new` is called is right to
    /// left). Computed when the event is logged.
    pub index: usize,
    /// `"bang"`, `"int"`, `"float"`, `"list"`, or the selector given to `outlet_anything`.
    pub selector: String,
    pub atoms: Vec<Atom>,
    /// The thread role the outlet was called from.
    pub role: Role,
    /// The logical time (ms) the outlet was called at.
    pub time: f64,
}

/// Console message severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Post,
    Warn,
    Error,
}

/// An entry of the console log, see [`crate::Kernel::console_log`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleEntry {
    pub level: Level,
    /// The object passed to `object_post`/`object_warn`/`object_error`, 0 for `post`/`error`.
    pub object: usize,
    pub text: String,
}

pub(crate) struct State {
    pub classes: HashMap<usize, ClassData>,
    /// `(namespace, class name)` -> class
    pub class_names: HashMap<(String, String), usize>,
    pub objects: HashMap<usize, ObjData>,
    pub subs: HashMap<usize, Sub>,
    /// `(namespace, name)` -> registered object
    pub registry: HashMap<(String, String), usize>,
    /// registered object -> attached clients
    pub attached: HashMap<usize, Vec<usize>>,
    pub subscriptions: Vec<Subscription>,
    pub buffers: HashMap<String, BufferData>,
    /// Calls to `buffer_ref_notify`: (ref, sender name, message).
    pub buffer_ref_notifications: Vec<(usize, String, String)>,
    pub outlet_log: Vec<OutletEvent>,
    pub console: Vec<ConsoleEntry>,
    /// `(object, attribute name)` for every `object_attr_touch`.
    pub touches: Vec<(usize, String)>,
    pub now: f64,
    pub seq: u64,
    pub queue: VecDeque<Deferred>,
    pub dsp_adds: Vec<DspAdd>,
    pub sample_rate: f64,
}

impl State {
    fn new() -> Self {
        Self {
            classes: HashMap::new(),
            class_names: HashMap::new(),
            objects: HashMap::new(),
            subs: HashMap::new(),
            registry: HashMap::new(),
            attached: HashMap::new(),
            subscriptions: Vec::new(),
            buffers: HashMap::new(),
            buffer_ref_notifications: Vec::new(),
            outlet_log: Vec::new(),
            console: Vec::new(),
            touches: Vec::new(),
            now: 0.0,
            seq: 0,
            queue: VecDeque::new(),
            dsp_adds: Vec::new(),
            sample_rate: 44100.0,
        }
    }

    /// Add a record for a new sub-object and return its (never reused) pointer.
    pub(crate) fn alloc_sub(&mut self, kind: SubKind) -> usize {
        // 64 zeroed bytes: big enough that nothing that treats it as a `t_object` (32 bytes)
        // reads out of bounds, and leaked so that the address is unique for the process.
        let ptr = Box::into_raw(Box::new([0u64; 8])) as usize;
        self.subs.insert(ptr, Sub { alive: true, kind });
        ptr
    }

    pub(crate) fn post_error(&mut self, object: usize, text: String) {
        self.console.push(ConsoleEntry {
            level: Level::Error,
            object,
            text,
        });
    }

    /// Reset everything that is per-test. Classes, symbols and sub-object records survive, see
    /// [`crate::Kernel::reset`].
    pub(crate) fn reset(&mut self) {
        self.objects.clear();
        self.registry.clear();
        self.attached.clear();
        self.subscriptions.clear();
        self.buffers.clear();
        self.buffer_ref_notifications.clear();
        self.outlet_log.clear();
        self.console.clear();
        self.touches.clear();
        self.now = 0.0;
        self.queue.clear();
        self.dsp_adds.clear();
        self.sample_rate = 44100.0;
        // Everything created by a previous test is dead, except the attributes, which belong to
        // the classes that survive.
        for sub in self.subs.values_mut() {
            if !matches!(sub.kind, SubKind::Attr(_)) {
                sub.alive = false;
            }
        }
    }
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::new()));

/// Lock the state. A panic in a test must not poison the kernel for the next one.
pub(crate) fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}
