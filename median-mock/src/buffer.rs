//! `buffer~` basics: `buffer_ref_*`, `buffer_locksamples` and the accessors.
//!
//! A buffer~ is made by a test with [`crate::Kernel::create_buffer`]; it is registered in the
//! `globalsymbol` namespace under its name, so `object_subscribe`/`object_attach` clients (such as
//! a `buffer_ref`) are bound to it and notified like in Max. Sample data is interleaved
//! (frame major, channel minor) as in Max, which is the layout review M1 is about. Locking is
//! counted but never refuses.

use crate::state::{BufferData, SubKind, state};
use crate::symbols::name_of;
use max_sys::{
    e_max_errorcodes as err, t_atom_float, t_atom_long, t_buffer_obj, t_buffer_ref, t_max_err,
    t_object, t_symbol,
};
use std::ffi::c_void;

const NAMESPACE: &str = "globalsymbol";

/// Create the buffer, bind subscribers. Returns its pointer.
pub(crate) fn create(name: &str, channels: usize, frames: usize, sample_rate: f64) -> usize {
    let ptr = Box::into_raw(Box::new([0u64; 8])) as usize;
    state().buffers.insert(
        name.to_string(),
        BufferData {
            ptr,
            name: name.to_string(),
            channels,
            frames,
            sample_rate,
            data: vec![0.0; channels * frames],
            locks: 0,
            dirty: false,
        },
    );
    crate::notify::register_with_class(NAMESPACE, name, ptr, Some("buffer~"));
    ptr
}

/// Unregister the buffer (its clients are sent `globalsymbol_unbinding`) and forget it.
pub(crate) fn destroy(name: &str) {
    let ptr = state().buffers.get(name).map(|b| b.ptr);
    if let Some(ptr) = ptr {
        let _ = crate::notify::unregister_addr(ptr);
        state().buffers.remove(name);
    }
}

fn with_buffer<R>(obj: *mut t_buffer_obj, f: impl FnOnce(&mut BufferData) -> R) -> Option<R> {
    state()
        .buffers
        .values_mut()
        .find(|b| b.ptr == obj as usize)
        .map(f)
}

/// # Safety
/// `self_` must be a live object; `name` a symbol (may be null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_ref_new(
    self_: *mut t_object,
    name: *mut t_symbol,
) -> *mut t_buffer_ref {
    let name = name_of(name as usize);
    let _ = self_;
    state().alloc_sub(SubKind::BufferRef { name }) as *mut t_buffer_ref
}

/// # Safety
/// `x` must be a buffer reference from `buffer_ref_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_ref_set(x: *mut t_buffer_ref, name: *mut t_symbol) {
    let new = name_of(name as usize);
    if let Some(s) = state().subs.get_mut(&(x as usize)) {
        if let SubKind::BufferRef { name, .. } = &mut s.kind {
            *name = new;
        }
    }
}

fn ref_name(x: *mut t_buffer_ref) -> Option<String> {
    match &state().subs.get(&(x as usize))?.kind {
        SubKind::BufferRef { name, .. } => Some(name.clone()),
        _ => None,
    }
}

/// # Safety
/// As `buffer_ref_set`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_ref_exists(x: *mut t_buffer_ref) -> t_atom_long {
    let Some(name) = ref_name(x) else { return 0 };
    state().buffers.contains_key(&name) as t_atom_long
}

/// # Safety
/// As `buffer_ref_set`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_ref_getobject(x: *mut t_buffer_ref) -> *mut t_buffer_obj {
    let Some(name) = ref_name(x) else {
        return std::ptr::null_mut();
    };
    state()
        .buffers
        .get(&name)
        .map_or(std::ptr::null_mut(), |b| b.ptr as *mut t_buffer_obj)
}

/// Records the call (see [`crate::Kernel::buffer_ref_notifications`]); the mock buffer reference
/// itself has nothing to update.
///
/// # Safety
/// As `buffer_ref_set`; the symbols from `gensym`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_ref_notify(
    x: *mut t_buffer_ref,
    s: *mut t_symbol,
    msg: *mut t_symbol,
    _sender: *mut c_void,
    _data: *mut c_void,
) -> t_max_err {
    state()
        .buffer_ref_notifications
        .push((x as usize, name_of(s as usize), name_of(msg as usize)));
    err::MAX_ERR_NONE as t_max_err
}

/// Null if `buffer_object` is not a buffer~.
///
/// # Safety
/// `buffer_object` must be a buffer from [`crate::Kernel::create_buffer`] or null. The returned
/// pointer is valid until the next `reset` and while the buffer is not destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_locksamples(buffer_object: *mut t_buffer_obj) -> *mut f32 {
    with_buffer(buffer_object, |b| {
        b.locks += 1;
        b.data.as_mut_ptr()
    })
    .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// As `buffer_locksamples`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_unlocksamples(buffer_object: *mut t_buffer_obj) {
    let unbalanced = with_buffer(buffer_object, |b| {
        let unbalanced = b.locks == 0;
        b.locks = b.locks.saturating_sub(1);
        unbalanced
    });
    if unbalanced == Some(true) {
        state().post_error(
            buffer_object as usize,
            "median-mock: buffer_unlocksamples without a matching buffer_locksamples".into(),
        );
    }
}

/// # Safety
/// As `buffer_locksamples`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_getchannelcount(buffer_object: *mut t_buffer_obj) -> t_atom_long {
    with_buffer(buffer_object, |b| b.channels as t_atom_long).unwrap_or(0)
}

/// # Safety
/// As `buffer_locksamples`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_getframecount(buffer_object: *mut t_buffer_obj) -> t_atom_long {
    with_buffer(buffer_object, |b| b.frames as t_atom_long).unwrap_or(0)
}

/// # Safety
/// As `buffer_locksamples`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_getsamplerate(buffer_object: *mut t_buffer_obj) -> t_atom_float {
    with_buffer(buffer_object, |b| b.sample_rate).unwrap_or(0.0)
}

/// # Safety
/// As `buffer_locksamples`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_getmillisamplerate(
    buffer_object: *mut t_buffer_obj,
) -> t_atom_float {
    with_buffer(buffer_object, |b| b.sample_rate / 1000.0).unwrap_or(0.0)
}

/// # Safety
/// As `buffer_locksamples`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_setdirty(buffer_object: *mut t_buffer_obj) -> t_max_err {
    match with_buffer(buffer_object, |b| b.dirty = true) {
        Some(()) => err::MAX_ERR_NONE as t_max_err,
        None => err::MAX_ERR_INVALID_PTR as t_max_err,
    }
}

/// The global sample rate (`sys_getsr`), set with [`crate::Kernel::set_sample_rate`].
///
/// # Safety
/// No requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_getsr() -> f32 {
    state().sample_rate as f32
}
