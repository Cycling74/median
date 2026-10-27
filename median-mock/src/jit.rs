//! Jitter class/lifecycle and matrix fixtures. Models registration, MOP iolets, attributes,
//! ob3d attachment and direct callbacks; it does not implement a GL renderer or Jitter scheduler.
//! Exported C functions use the SDK's pointer and lifecycle contracts.
#![allow(clippy::missing_safety_doc)]

mod gl;
pub use gl::{
    buffer_descriptor, buffer_destroy_count, fail_next_chunk_copy, live_chunk_count,
    live_geometry_count, new_geometry,
};

use crate::{
    atoms::Atom,
    state::{SubKind, state},
    symbols::{intern, name_of},
};
use core::ffi::{c_char, c_long, c_short, c_void};
use max_sys::*;
use std::{collections::HashMap, ffi::CStr, sync::LazyLock};

#[derive(Clone)]
struct Mop {
    inputs: c_long,
    outputs: c_long,
    kind: usize,
    planes: c_long,
}
enum Aux {
    Geometry,
    Mop(Mop),
    List(Vec<usize>),
    Matrix(Box<MatrixData>),
}
struct MatrixData {
    info: t_jit_matrix_info,
    data: Vec<u64>,
    locked: c_long,
}
// SAFETY: the symbol field is opaque/interned; raw data borrows follow the SDK caller contract.
unsafe impl Send for MatrixData {}
#[derive(Default)]
struct Jitter {
    aux: HashMap<usize, Aux>,
    adornments: HashMap<usize, usize>,
    ob3d_offsets: HashMap<usize, usize>,
    contexts: HashMap<usize, usize>,
    max_classes: HashMap<usize, usize>,
    /// The `flags` each Max class passed to `max_jit_class_wrap_standard` (survives resets, like
    /// the classes).
    wrap_flags: HashMap<usize, c_long>,
    max_obex_offsets: HashMap<usize, usize>,
    max_objects: HashMap<usize, usize>,
    dumps: HashMap<usize, usize>,
    gl_attached: HashMap<usize, usize>,
    gl_calls: Vec<GlCall>,
    inletinfo_calls: Vec<(usize, usize, c_long)>,
    mop_owners: std::collections::HashSet<usize>,
    fail_mop_setup: bool,
    no_sdk_notify: bool,
    mop_notify_calls: Vec<(usize, String, String)>,
}
static JIT: LazyLock<parking_lot::Mutex<Jitter>> =
    LazyLock::new(|| parking_lot::Mutex::new(Jitter::default()));
fn alloc(value: Aux) -> *mut c_void {
    let ptr = unsafe { crate::sysmem::sysmem_newptrclear(std::mem::size_of::<t_object>()) };
    JIT.lock().aux.insert(ptr as usize, value);
    ptr.cast()
}
pub(crate) fn reset() {
    gl::reset();
    let mut jit = JIT.lock();
    let retained: Vec<usize> = jit.adornments.values().copied().collect();
    jit.aux.retain(|p, _| {
        if retained.contains(p) {
            true
        } else {
            unsafe {
                crate::sysmem::sysmem_freeptr(*p as _);
            }
            false
        }
    });
    for ptr in jit.contexts.values() {
        unsafe {
            crate::sysmem::sysmem_freeptr(*ptr as _);
        }
    }
    jit.gl_calls.clear();
    jit.inletinfo_calls.clear();
    jit.mop_owners.clear();
    jit.fail_mop_setup = false;
    jit.no_sdk_notify = false;
    jit.mop_notify_calls.clear();
    jit.contexts.clear();
    jit.max_objects.clear();
    jit.dumps.clear();
    jit.gl_attached.clear();
}
pub(crate) fn attribute_target(obj: usize) -> usize {
    JIT.lock().max_objects.get(&obj).copied().unwrap_or(obj)
}
/// The Jitter instance owned by a Max wrapper, for calling registered Jitter methods in tests.
pub fn object_for(max: *mut c_void) -> *mut c_void {
    attribute_target(max as usize) as _
}
/// Live GL context attachments (separate from the user value's destructor).
pub fn context_count() -> usize {
    JIT.lock().contexts.len()
}

pub fn take_inletinfo_calls() -> Vec<(usize, usize, c_long)> {
    std::mem::take(&mut JIT.lock().inletinfo_calls)
}
pub fn fail_next_mop_setup() {
    JIT.lock().fail_mop_setup = true;
}
/// Until the next kernel reset, `max_jit_class_wrap_standard` installs no `notify` method (the
/// SDK's wrapper does not for every class; median then installs its own). Set it before the
/// class is registered.
pub fn skip_sdk_notify() {
    JIT.lock().no_sdk_notify = true;
}
/// The `max_jit_mop_notify` calls so far, as (object, sender name, message), cleared by the call.
pub fn take_mop_notify_calls() -> Vec<(usize, String, String)> {
    std::mem::take(&mut JIT.lock().mop_notify_calls)
}
/// The `flags` the Max class named `name` passed to `max_jit_class_wrap_standard`, `None` if the
/// class is unknown or never called it. Kept across kernel resets, like the class.
pub fn wrap_flags(name: &str) -> Option<c_long> {
    let class = crate::class::find_class(name)?;
    JIT.lock().wrap_flags.get(&class).copied()
}
pub fn live_mop_count() -> usize {
    JIT.lock().mop_owners.len()
}

/// Own a matrix with aligned storage for direct `matrix_calc` tests. Raw pointers passed to SDK
/// calls must not outlive this fixture or race its safe read/write methods.
pub struct MatrixFixture {
    ptr: usize,
}
impl MatrixFixture {
    pub fn new(info: t_jit_matrix_info) -> Self {
        let size = usize::try_from(info.size).expect("nonnegative fixture size");
        let ptr = alloc(Aux::Matrix(Box::new(MatrixData {
            info,
            data: vec![0; size.div_ceil(8)],
            locked: 0,
        })));
        Self { ptr: ptr as usize }
    }
    pub fn as_ptr(&self) -> *mut c_void {
        self.ptr as _
    }
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        let mut jit = JIT.lock();
        let Aux::Matrix(m) = jit.aux.get_mut(&self.ptr).unwrap() else {
            unreachable!()
        };
        assert!(bytes.len() <= m.info.size as usize);
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), m.data.as_mut_ptr().cast(), bytes.len());
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        let jit = JIT.lock();
        let Aux::Matrix(m) = &jit.aux[&self.ptr] else {
            unreachable!()
        };
        unsafe { std::slice::from_raw_parts(m.data.as_ptr().cast(), m.info.size as usize) }.to_vec()
    }
    pub fn lock_state(&self) -> c_long {
        let jit = JIT.lock();
        let Aux::Matrix(m) = &jit.aux[&self.ptr] else {
            unreachable!()
        };
        m.locked
    }
}
impl Drop for MatrixFixture {
    fn drop(&mut self) {
        unsafe {
            jit_object_free(self.as_ptr());
        }
    }
}
/// An SDK-style list of borrowed matrix pointers. The caller keeps the matrices alive.
pub struct MatrixList<'a> {
    ptr: usize,
    _matrices: std::marker::PhantomData<&'a MatrixFixture>,
}
impl<'a> MatrixList<'a> {
    pub fn new(matrices: &[&'a MatrixFixture]) -> Self {
        Self {
            ptr: alloc(Aux::List(matrices.iter().map(|m| m.ptr).collect())) as usize,
            _matrices: std::marker::PhantomData,
        }
    }
    pub fn as_ptr(&self) -> *mut c_void {
        self.ptr as _
    }
}
impl Drop for MatrixList<'_> {
    fn drop(&mut self) {
        unsafe {
            jit_object_free(self.as_ptr());
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_class_register(c: *mut c_void) -> t_jit_err {
    unsafe { crate::class::class_register(intern(b"jit"), c.cast()) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_class_free(c: *mut c_void) -> t_jit_err {
    let mop = JIT.lock().adornments.remove(&(c as usize));
    if let Some(mop) = mop {
        unsafe {
            jit_object_free(mop as _);
        }
    }
    JIT.lock().ob3d_offsets.remove(&(c as usize));
    let class = {
        let mut st = state();
        st.class_names.retain(|_, v| *v != c as usize);
        st.classes.remove(&(c as usize))
    };
    if let Some(class) = class {
        for attr in class.attrs {
            unsafe {
                crate::class::object_free(attr as _);
            }
        }
        unsafe {
            drop(Box::from_raw(c.cast::<t_class>()));
            drop(Box::from_raw(
                (class.messlist as *mut t_messlist)
                    .sub(1)
                    .cast::<[t_messlist; 2]>(),
            ));
        }
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_object_alloc(c: *mut c_void) -> *mut c_void {
    unsafe { crate::class::object_alloc(c.cast()) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_object_free(p: *mut c_void) -> t_jit_err {
    let aux = JIT.lock().aux.remove(&(p as usize));
    if aux.is_some() {
        unsafe {
            crate::sysmem::sysmem_freeptr(p);
        }
        0
    } else {
        unsafe { crate::class::object_free(p) }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn median_mock_jit_new(
    name: *mut t_symbol,
    dest: *mut t_symbol,
) -> *mut c_void {
    let f = {
        let st = state();
        let name = name_of(name as usize);
        st.class_names
            .get(&("jit".into(), name))
            .and_then(|c| st.classes.get(c))
            .map(|c| c.new_fn)
    };
    match f {
        Some(f) => unsafe {
            std::mem::transmute::<usize, unsafe extern "C" fn(*mut t_symbol) -> *mut c_void>(f)(
                dest,
            )
        },
        None => std::ptr::null_mut(),
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn median_mock_jit_mop_new(inputs: c_long, outputs: c_long) -> *mut c_void {
    alloc(Aux::Mop(Mop {
        inputs,
        outputs,
        kind: intern(b"char") as usize,
        planes: 1,
    }))
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_class_addadornment(c: *mut c_void, mop: *mut t_object) -> t_jit_err {
    JIT.lock().adornments.insert(c as usize, mop as usize);
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_class_addattr(c: *mut c_void, attr: *mut t_object) -> t_jit_err {
    unsafe { crate::attr::class_addattr(c.cast(), attr) }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_class_attr_get(c: *mut c_void, name: *mut t_symbol) -> *mut c_void {
    let st = state();
    let name = name_of(name as usize);
    st.classes
        .get(&(c as usize))
        .and_then(|c| {
            c.attrs
                .iter()
                .find(|a| matches!(&st.subs[a].kind, SubKind::Attr(d) if d.name == name))
        })
        .map_or(std::ptr::null_mut(), |a| *a as _)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_getbytes(n: c_long) -> *mut c_void {
    unsafe { crate::sysmem::sysmem_newptr(n.max(0) as usize) }.cast()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_freebytes(p: *mut c_void, _: c_long) {
    unsafe {
        crate::sysmem::sysmem_freeptr(p.cast());
    }
}
macro_rules! filter {
    ($name:ident, $max:ident) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            p: *mut c_void,
            min: f64,
            max: f64,
            use_min: c_long,
            use_max: c_long,
        ) -> t_jit_err {
            unsafe { crate::attr::$max(p, min, max, use_min, use_max) }
        }
    };
}
filter!(jit_attr_addfilterget_clip, attr_addfilterget_clip);
filter!(jit_attr_addfilterset_clip, attr_addfilterset_clip);
filter!(jit_attr_addfilter_clip, attr_addfilter_clip);
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_addattr_atoms(
    obj: *mut t_object,
    name: *const c_char,
    ty: *mut t_symbol,
    _: c_long,
    n: c_long,
    atoms: *mut t_atom,
) -> t_max_err {
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    let atoms = if n == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(atoms, n as usize) }
            .iter()
            .map(Atom::from_raw)
            .collect()
    };
    crate::attr::add_attr_attr(obj as usize, name, name_of(ty as usize), atoms).map_or(-1, |_| 0)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_mop_single_type(p: *mut c_void, kind: *mut t_symbol) -> t_jit_err {
    if let Some(Aux::Mop(m)) = JIT.lock().aux.get_mut(&(p as usize)) {
        m.kind = kind as usize;
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_mop_single_planecount(p: *mut c_void, planes: c_long) -> t_jit_err {
    if let Some(Aux::Mop(m)) = JIT.lock().aux.get_mut(&(p as usize)) {
        m.planes = planes;
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn median_mock_jit_method(
    p: *mut c_void,
    selector: *mut t_symbol,
    number: c_long,
    pointer: *mut c_void,
) -> *mut c_void {
    let mut jit = JIT.lock();
    let name = name_of(selector as usize);
    match (jit.aux.get_mut(&(p as usize)), name.as_str()) {
        (Some(Aux::List(v)), "getsize") => v.len() as _,
        (Some(Aux::List(v)), "getindex") => v.get(number as usize).copied().unwrap_or(0) as _,
        (Some(Aux::Matrix(m)), "lock") => std::mem::replace(&mut m.locked, number) as _,
        (Some(Aux::Matrix(m)), "getinfo") => {
            unsafe {
                pointer.cast::<t_jit_matrix_info>().write(m.info);
            }
            std::ptr::null_mut()
        }
        (Some(Aux::Matrix(m)), "getdata") => {
            unsafe {
                pointer.cast::<*mut u8>().write(m.data.as_mut_ptr().cast());
            }
            std::ptr::null_mut()
        }
        _ => {
            drop(jit);
            state().post_error(0, format!("median-mock: unsupported Jitter method {name}"));
            std::ptr::null_mut()
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_class_obex_setup(c: *mut t_class, offset: c_long) {
    // Jitter's classex offset is independent of class_obexoffset_set (Max's table).
    JIT.lock()
        .max_obex_offsets
        .insert(c as usize, offset as usize);
}
/// `max_jit_class_wrap_standard` (`jit.max.h`). It installs `inletinfo` and (unless
/// [`skip_sdk_notify`]) `notify` on the Max class `c`, records `flags` for [`wrap_flags`], and,
/// with `MAX_JIT_CLASS_FLAGS_GIMMEBACK_WRAP` (0x1, "uses standard dumpout `A_DEFER_LOW` method"),
/// installs for each method of the Jitter class `jit` registered `A_GIMMEBACK` an `A_GIMME`
/// method of the same name on `c` that calls it and sends the reply out the dumpout outlet:
///
/// * the Jitter method is called through [`object_method_typed`](crate::class::object_method_typed)
///   on the Jitter object, with the message's arguments;
/// * an atomarray reply goes out as its atoms, then is freed; an int, float or symbol as one atom;
///   `A_NOTHING`, a non-zero `t_max_err`, or any other object (a dictionary, say), which is freed,
///   sends nothing;
/// * the outlet message carries the selector the message had without a leading `get`
///   (`getvalues` goes out as `values`, `getcount` as `count`; a selector that does not start with
///   `get`, or is exactly `get`, goes out unchanged, so `get` alone answers under `get`), and the
///   send is synchronous (the SDK's `A_DEFER_LOW` is not modelled).
///
/// The list reply out dumpout and the stripped `get` were observed in Max 9.2 on 2026-10-27
/// (`doc/validation-max.md`); the rest (a scalar as a one-atom list, an object that is not an
/// atomarray sending nothing, the synchronous send) is median's assumption, not a fact from the
/// headers. Without the flag nothing is
/// installed for those selectors: a patcher message to one is "doesn't understand" on the Max
/// class, as today.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_class_wrap_standard(
    c: *mut t_class,
    jit: *mut t_class,
    flags: c_long,
) {
    let no_notify = {
        let mut jit_state = JIT.lock();
        jit_state.max_classes.insert(c as usize, jit as usize);
        jit_state.wrap_flags.insert(c as usize, flags);
        jit_state.no_sdk_notify
    };
    if flags & GIMMEBACK_WRAP != 0 {
        let selectors: Vec<String> =
            state()
                .classes
                .get(&(jit as usize))
                .map_or_else(Vec::new, |class| {
                    class
                        .methods
                        .iter()
                        .filter(|(_, rec)| {
                            rec.types.as_slice() == [e_max_atomtypes::A_GIMMEBACK as i32]
                        })
                        .map(|(name, _)| name.clone())
                        .collect()
                });
        for name in selectors {
            let name = std::ffi::CString::new(name).expect("a selector has no NUL");
            // SAFETY: `c` is a class from `class_new`; `gimmeback_wrap` is `A_GIMME`
            unsafe {
                crate::class::median_mock_class_addmethod(
                    c,
                    Some(std::mem::transmute::<
                        unsafe extern "C" fn(*mut c_void, *mut t_symbol, c_long, *mut t_atom),
                        unsafe extern "C" fn(*mut c_void) -> *mut c_void,
                    >(gimmeback_wrap)),
                    name.as_ptr(),
                    [e_max_atomtypes::A_GIMME as i32].as_ptr(),
                    1,
                );
            }
        }
    }
    // Install distinguishable SDK-owned callbacks so wrapper tests detect accidental
    // replacement by the ordinary Max iolet/notification trampolines.
    unsafe {
        crate::class::median_mock_class_addmethod(
            c,
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut c_void, *mut c_void, c_long, *mut c_char),
                unsafe extern "C" fn(*mut c_void) -> *mut c_void,
            >(standard_inletinfo)),
            c"inletinfo".as_ptr(),
            [e_max_atomtypes::A_CANT as i32].as_ptr(),
            1,
        );
        if !no_notify {
            crate::class::median_mock_class_addmethod(
                c,
                Some(std::mem::transmute::<
                    unsafe extern "C" fn(
                        *mut c_void,
                        *mut t_symbol,
                        *mut t_symbol,
                        *mut c_void,
                        *mut c_void,
                    ) -> t_max_err,
                    unsafe extern "C" fn(*mut c_void) -> *mut c_void,
                >(standard_notify)),
                c"notify".as_ptr(),
                [e_max_atomtypes::A_CANT as i32].as_ptr(),
                1,
            );
        }
    }
}
/// `MAX_JIT_CLASS_FLAGS_GIMMEBACK_WRAP` (`jit.max.h`).
const GIMMEBACK_WRAP: c_long = 0x1;
/// The Max method [`max_jit_class_wrap_standard`] installs for a gimmeback method of the Jitter
/// class: `(x, s, ac, av)` as an `A_GIMME` method.
unsafe extern "C" fn gimmeback_wrap(x: *mut c_void, s: *mut t_symbol, ac: c_long, av: *mut t_atom) {
    let (jitob, dump) = {
        let jit = JIT.lock();
        (
            jit.max_objects.get(&(x as usize)).copied().unwrap_or(0),
            jit.dumps.get(&(x as usize)).copied().unwrap_or(0),
        )
    };
    if jitob == 0 || dump == 0 {
        state().post_error(
            x as usize,
            "median-mock: gimmeback wrap without a Jitter object or a dumpout outlet".into(),
        );
        return;
    }
    // SAFETY: an uninitialised `t_atom` is plain data; `object_method_typed` sets it
    let mut rv: t_atom = unsafe { std::mem::zeroed() };
    // SAFETY: `jitob` is the live Jitter object, `s`/`ac`/`av` are what the message delivered
    let code = unsafe { crate::class::object_method_typed(jitob as _, s, ac, av, &mut rv) };
    // `None`: nothing goes out (no value, an error, or an object that is not an atomarray).
    // `Some(atoms)`: a list, which may be empty (an empty atomarray is still a reply).
    let mut reply: Option<Vec<t_atom>> = None;
    if code == 0 {
        match rv.a_type as u32 {
            e_max_atomtypes::A_LONG | e_max_atomtypes::A_FLOAT | e_max_atomtypes::A_SYM => {
                reply = Some(vec![rv]);
            }
            e_max_atomtypes::A_OBJ => {
                // SAFETY: the reply is an `A_OBJ` atom; the caller owns an object reply
                unsafe {
                    let p = rv.a_w.w_obj;
                    if crate::containers::atomisatomarray(&mut rv) == 1 {
                        let mut n = 0;
                        let mut items = std::ptr::null_mut();
                        max_sys::atomarray_getatoms(p.cast(), &mut n, &mut items);
                        reply = Some(std::slice::from_raw_parts(items, n.max(0) as usize).to_vec());
                    }
                    max_sys::object_free(p.cast());
                }
            }
            _ => {}
        }
    }
    let Some(mut reply) = reply else {
        return;
    };
    // the reply goes out under the selector without a leading `get` (Max 9.2, 2026-10-27); `get`
    // alone is left as it is
    // SAFETY: `s` is the selector of a delivered message, a valid symbol
    let name = unsafe { CStr::from_ptr((*s).s_name) }.to_bytes();
    let out_sel = match name.strip_prefix(b"get") {
        Some(rest) if !rest.is_empty() => {
            let rest = std::ffi::CString::new(rest).expect("a selector has no NUL");
            // SAFETY: `rest` is a NUL-terminated string
            unsafe { max_sys::gensym(rest.as_ptr()) }
        }
        _ => s,
    };
    // SAFETY: `dump` is the dumpout outlet of `x`
    unsafe {
        crate::io::outlet_anything(
            dump as _,
            out_sel,
            reply.len().min(c_short::MAX as usize) as c_short,
            reply.as_mut_ptr(),
        );
    }
}
unsafe extern "C" fn standard_inletinfo(
    owner: *mut c_void,
    box_ptr: *mut c_void,
    index: c_long,
    cold: *mut c_char,
) {
    JIT.lock()
        .inletinfo_calls
        .push((owner as usize, box_ptr as usize, index));
    unsafe {
        cold.write((index > 0) as c_char);
    }
}
unsafe extern "C" fn standard_notify(
    _: *mut c_void,
    _: *mut t_symbol,
    _: *mut t_symbol,
    _: *mut c_void,
    _: *mut c_void,
) -> t_max_err {
    crate::state::state().post_error(0, "median-mock: SDK Jitter notification delivered".into());
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_class_mop_wrap(
    _: *mut t_class,
    _: *mut t_class,
    _: c_long,
) -> t_jit_err {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_object_wrap_complete(
    obj: *mut t_object,
    _jit: *mut t_object,
    _: c_long,
) {
    let (class, max_offset) = {
        let st = state();
        let class = st.objects[&(obj as usize)].class;
        (class, st.classes[&class].obex_offset as usize)
    };
    let mut jit = JIT.lock();
    if jit.max_obex_offsets.get(&class) == Some(&max_offset) {
        drop(jit);
        state().post_error(
            obj as usize,
            "median-mock: Jitter storage overlaps Max obex table".into(),
        );
        return;
    }
    // The SDK initializes the wrapper with a null jitob; attaching it is a separate call.
    jit.max_objects.insert(obj as usize, 0);
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_obex_jitob_set(obj: *mut c_void, core: *mut c_void) {
    let mut jit = JIT.lock();
    let Some(target) = jit.max_objects.get_mut(&(obj as usize)) else {
        return;
    };
    *target = core as usize;
    drop(jit);
    if !core.is_null() {
        unsafe {
            crate::notify::object_obex_storeflags(core, intern(b"maxwrapper"), obj.cast(), 1);
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_object_free(obj: *mut c_void) {
    let mut jit = JIT.lock();
    jit.max_objects.remove(&(obj as usize));
    jit.dumps.remove(&(obj as usize));
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_attr_args(obj: *mut c_void, n: c_short, atoms: *mut t_atom) {
    let atoms = if n <= 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(atoms, n as usize) }
    };
    let mut i = 0;
    while i < atoms.len() {
        if let Atom::Sym(s) = Atom::from_raw(&atoms[i]) {
            if let Some(name) = s.strip_prefix('@') {
                i += 1;
                let start = i;
                while i < atoms.len()
                    && !matches!(Atom::from_raw(&atoms[i]), Atom::Sym(s) if s.starts_with('@'))
                {
                    i += 1;
                }
                let values: Vec<_> = atoms[start..i].iter().map(Atom::from_raw).collect();
                if let Err(e) = crate::attr::set(obj as usize, name, &values) {
                    state().post_error(obj as usize, format!("Jitter attribute: {e:?}"));
                }
                continue;
            }
        }
        i += 1;
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_mop_setup_simple(
    obj: *mut c_void,
    core: *mut c_void,
    _: c_long,
    _: *mut t_atom,
) -> t_jit_err {
    let class = state().objects[&(core as usize)].class;
    let mop = {
        let jit = JIT.lock();
        let Aux::Mop(m) = &jit.aux[&jit.adornments[&class]] else {
            return -1;
        };
        m.clone()
    };
    let dump = unsafe { crate::io::outlet_new(obj, std::ptr::null()) };
    {
        let mut jit = JIT.lock();
        jit.dumps.insert(obj as usize, dump as usize);
        jit.mop_owners.insert(obj as usize);
        if std::mem::take(&mut jit.fail_mop_setup) {
            return -1;
        }
    }
    for _ in 0..mop.outputs.max(0) {
        unsafe {
            crate::io::outlet_new(obj, c"jit_matrix".as_ptr());
        }
    }
    for _ in 1..mop.inputs.max(0) {
        unsafe {
            crate::io::inlet_new(obj.cast(), c"jit_matrix".as_ptr());
        }
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_mop_free(obj: *mut c_void) {
    if !JIT.lock().mop_owners.remove(&(obj as usize)) {
        state().post_error(obj as usize, "median-mock: MOP freed without setup".into());
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_mop_notify(
    obj: *mut c_void,
    sender_name: *mut t_symbol,
    message: *mut t_symbol,
) -> t_jit_err {
    let call = (
        obj as usize,
        name_of(sender_name as usize),
        name_of(message as usize),
    );
    JIT.lock().mop_notify_calls.push(call);
    0
}
unsafe fn assist(io: c_long, index: c_long, out: *mut c_char) {
    let text = format!(
        "{} {index}: Jitter matrix\0",
        if io == 1 { "inlet" } else { "outlet" }
    );
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr().cast(), out, text.len().min(511));
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_mop_assist(
    _: *mut c_void,
    _: *mut c_void,
    io: c_long,
    index: c_long,
    text: *mut c_char,
) -> t_jit_err {
    unsafe {
        assist(io, index, text);
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_ob3d_assist(
    _: *mut c_void,
    _: *mut c_void,
    io: c_long,
    index: c_long,
    text: *mut c_char,
) -> t_jit_err {
    unsafe {
        assist(io, index, text);
    }
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_ob3d_setup(c: *mut c_void, offset: c_long, _: c_long) -> *mut c_void {
    JIT.lock().ob3d_offsets.insert(c as usize, offset as usize);
    c
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_ob3d_new(obj: *mut c_void, _: *mut t_symbol) -> *mut c_void {
    let class = state().objects[&(obj as usize)].class;
    let mut jit = JIT.lock();
    let offset = jit.ob3d_offsets[&class];
    let context = unsafe { crate::sysmem::sysmem_newptrclear(16) };
    unsafe {
        obj.cast::<u8>()
            .add(offset)
            .cast::<*mut c_void>()
            .write(context.cast());
    }
    jit.contexts.insert(obj as usize, context as usize);
    context.cast()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_ob3d_free(obj: *mut c_void) {
    if let Some(ptr) = JIT.lock().contexts.remove(&(obj as usize)) {
        unsafe {
            crate::sysmem::sysmem_freeptr(ptr as _);
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_class_ob3d_wrap(_: *mut t_class) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_obex_dumpout_set(obj: *mut c_void, outlet: *mut c_void) {
    JIT.lock().dumps.insert(obj as usize, outlet as usize);
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_ob3d_attach(
    obj: *mut c_void,
    core: *mut t_object,
    _: *mut c_void,
) {
    JIT.lock().gl_attached.insert(obj as usize, core as usize);
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn max_jit_ob3d_detach(obj: *mut c_void) {
    JIT.lock().gl_attached.remove(&(obj as usize));
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_object_register(obj: *mut c_void, _: *mut t_symbol) -> *mut c_void {
    obj
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn max_and_jitter_obex_offsets_are_independent_and_core_attachment_is_explicit() {
        let kernel = crate::lock();
        unsafe {
            let class = class_new(c"mock.jitter.storage".as_ptr(), None, None, 64, None, 0);
            crate::class::class_obexoffset_set(class, 32);
            max_jit_class_obex_setup(class, 40);
            assert_eq!(state().classes[&(class as usize)].obex_offset, 32);
            let obj = crate::class::object_alloc(class);
            let core = crate::class::object_alloc(class);
            max_jit_object_wrap_complete(obj.cast(), core.cast(), 0);
            assert!(object_for(obj).is_null());
            max_jit_obex_jitob_set(obj, core);
            assert_eq!(object_for(obj), core);
            let mut owner = std::ptr::null_mut();
            assert_eq!(
                crate::notify::object_obex_lookup(core, intern(b"maxwrapper"), &mut owner),
                0
            );
            assert_eq!(owner.cast::<c_void>(), obj);
            max_jit_object_free(obj);
            assert!(kernel.free_object(core));
            assert!(kernel.free_object(obj));
            assert!(kernel.take_console_log().is_empty());

            // Report the collision deterministically instead of corrupting a table as Max does.
            max_jit_class_obex_setup(class, 32);
            let obj = crate::class::object_alloc(class);
            max_jit_object_wrap_complete(obj.cast(), std::ptr::null_mut(), 0);
            assert!(
                kernel.take_console_log()[0]
                    .text
                    .contains("overlaps Max obex")
            );
            assert!(kernel.free_object(obj));
        }
    }

    /// Whether the class has the method: `class_method` is non-null for a missing one.
    unsafe fn has_method(class: *mut t_class, name: &[u8]) -> bool {
        unsafe { class_method(class, intern(name)) }
            .is_some_and(|f| f as usize != crate::class::method_false as *const () as usize)
    }

    #[test]
    fn wrap_standard_installs_notify_unless_told_not_to() {
        let kernel = crate::lock();
        unsafe {
            let class = class_new(c"mock.jitter.notify".as_ptr(), None, None, 64, None, 0);
            max_jit_class_wrap_standard(class, std::ptr::null_mut(), 0);
            assert!(has_method(class, b"notify"));
            assert!(has_method(class, b"inletinfo"));
            let none = class_new(c"mock.jitter.nonotify".as_ptr(), None, None, 64, None, 0);
            skip_sdk_notify();
            max_jit_class_wrap_standard(none, std::ptr::null_mut(), 0);
            assert!(!has_method(none, b"notify"));
            assert!(has_method(none, b"inletinfo"));
            assert_eq!(
                max_jit_mop_notify(std::ptr::null_mut(), intern(b"a"), intern(b"b")),
                0
            );
            assert_eq!(
                take_mop_notify_calls(),
                [(0, "a".to_string(), "b".to_string())]
            );
        }
        drop(kernel);
        let _kernel = crate::lock();
        assert!(!JIT.lock().no_sdk_notify);
    }

    #[test]
    fn borrowed_matrix_metadata_and_lock_restore() {
        let _kernel = crate::lock();
        let mut info: t_jit_matrix_info = unsafe { std::mem::zeroed() };
        info.size = 16;
        info.dimcount = 1;
        info.dim[0] = 4;
        info.dimstride[0] = 4;
        info.planecount = 1;
        info.type_ = intern(b"float32");
        let fixture = MatrixFixture::new(info);
        let list = MatrixList::new(&[&fixture]);
        unsafe {
            assert_eq!(
                jit_object_method(list.as_ptr(), intern(b"getsize")) as usize,
                1
            );
            assert_eq!(
                jit_object_method(list.as_ptr(), intern(b"getindex"), 0 as c_long),
                fixture.as_ptr()
            );
            assert!(jit_object_method(fixture.as_ptr(), intern(b"lock"), 1 as c_long).is_null());
            assert_eq!(fixture.lock_state(), 1);
            let mut copy: t_jit_matrix_info = std::mem::zeroed();
            jit_object_method(fixture.as_ptr(), intern(b"getinfo"), &mut copy);
            assert_eq!(copy.size, 16);
            jit_object_method(fixture.as_ptr(), intern(b"lock"), 0 as c_long);
        }
        assert_eq!(fixture.lock_state(), 0);
    }
}

/// Recorded immediate-mode drawing commands; these do not render.
#[derive(Debug, Clone, PartialEq)]
pub enum GlCall {
    Begin(e_jit_state::Type),
    Vertex([f64; 3]),
    End,
}
pub fn take_gl_log() -> Vec<GlCall> {
    std::mem::take(&mut JIT.lock().gl_calls)
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_gl_immediate_begin(primitive: e_jit_state::Type) {
    JIT.lock().gl_calls.push(GlCall::Begin(primitive));
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_gl_immediate_vertex3f(x: f64, y: f64, z: f64) {
    JIT.lock().gl_calls.push(GlCall::Vertex([x, y, z]));
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_gl_immediate_end() {
    JIT.lock().gl_calls.push(GlCall::End);
}
