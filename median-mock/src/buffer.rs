//! `buffer~` basics: `buffer_ref_*`, `buffer_locksamples`, `buffer_edit_*`, `buffer_getinfo`,
//! `buffer_view` and the accessors.
//!
//! A buffer~ is made by a test with [`crate::Kernel::create_buffer`]; it is registered in the
//! `globalsymbol` namespace under its name, so `object_subscribe`/`object_attach` clients (such as
//! a `buffer_ref`) are bound to it and notified like in Max. Sample data is interleaved
//! (frame major, channel minor) as in Max, which is the layout review M1 is about.
//!
//! Claims and edits are tracked, and misuse is reported on the console (as an error for the
//! buffer object):
//!
//! * `buffer_locksamples` claims are counted ([`crate::Kernel::buffer_state`]). An unbalanced
//!   `buffer_unlocksamples`, a perform routine that returns still holding a claim it took
//!   ([`crate::DspAdd::run`]), and destroying a buffer with claims outstanding are reported.
//! * `buffer_edit_begin`/`buffer_edit_end` must pair: a nested begin, an end without a begin and
//!   destroying a buffer mid-edit are reported (the stray end is refused with `MAX_ERR_GENERIC`;
//!   the nested begin is done, since what Max does, deadlock or two editors on the same samples,
//!   isn't modelled: code under test must never get there), as is a begin on the audio thread or
//!   while claims are held (Max's `buffer_edit_begin` waits for the claims to be released, which
//!   never happens if the claim is held by the caller). The `valid` given to `buffer_edit_end`
//!   is recorded.
//! * **Assumption**: while an edit is in progress `buffer_locksamples` returns null (the
//!   header says `buffer_edit_begin` clears the buffer's valid flag and waits for the in-use
//!   count, which is what `buffer_locksamples` raises). To check in Max.

use crate::state::{BufferData, SubKind, state};
use crate::symbols::{intern, name_of};
use max_sys::{
    e_max_errorcodes as err, t_atom_float, t_atom_long, t_buffer_info, t_buffer_obj, t_buffer_ref,
    t_max_err, t_object, t_symbol,
};
use std::ffi::{c_long, c_void};

const NAMESPACE: &str = "globalsymbol";

/// Create the buffer, bind subscribers. Returns its pointer.
pub(crate) fn create(name: &str, channels: usize, frames: usize, sample_rate: f64) -> usize {
    let ptr = Box::into_raw(Box::new([0u64; 8])) as usize;
    state().buffers.insert(
        name.to_string(),
        BufferData {
            edit_exclusion: true,
            ptr,
            name: name.to_string(),
            channels,
            frames,
            sample_rate,
            data: vec![0.0; channels * frames],
            locks: 0,
            dirty: false,
            editing: 0,
            last_edit_valid: None,
            edits_begun: 0,
            edits_ended: 0,
        },
    );
    crate::notify::register_with_class(NAMESPACE, name, ptr, Some("buffer~"));
    ptr
}

/// Unregister the buffer (its clients are sent `globalsymbol_unbinding`) and forget it. Claims
/// or an edit still outstanding are reported.
pub(crate) fn destroy(name: &str) {
    let found = state()
        .buffers
        .get(name)
        .map(|b| (b.ptr, b.locks, b.editing));
    if let Some((ptr, locks, editing)) = found {
        if locks > 0 {
            state().post_error(ptr, format!("median-mock: buffer~ \"{name}\" destroyed with {locks} buffer_locksamples claim(s) not unlocked"));
        }
        if editing > 0 {
            state().post_error(ptr, format!("median-mock: buffer~ \"{name}\" destroyed between buffer_edit_begin and buffer_edit_end"));
        }
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

/// The claims (`buffer_locksamples` without `buffer_unlocksamples`) outstanding on all buffers.
pub(crate) fn total_locks() -> usize {
    state().buffers.values().map(|b| b.locks).sum()
}

/// Null if `buffer_object` is not a buffer~, or while it is being edited (see the module docs).
///
/// # Safety
/// `buffer_object` must be a buffer from [`crate::Kernel::create_buffer`] or null. The returned
/// pointer is valid until the next `reset` and while the buffer is not destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_locksamples(buffer_object: *mut t_buffer_obj) -> *mut f32 {
    with_buffer(buffer_object, |b| {
        if b.editing > 0 && b.edit_exclusion {
            return std::ptr::null_mut();
        }
        b.locks += 1;
        b.data.as_mut_ptr()
    })
    .unwrap_or(std::ptr::null_mut())
}

/// Override the mock's assumption that an edit excludes shared claims, for a named fixture.
/// Returns false if the buffer does not exist. Newly created buffers default to exclusion.
/// This is for read-only contract probes: do not use mutable sample views with exclusion off.
pub fn set_edit_exclusion(name: &str, enabled: bool) -> bool {
    let mut st = state();
    let Some(buffer) = st.buffers.get_mut(name) else {
        return false;
    };
    buffer.edit_exclusion = enabled;
    true
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

/// Start an exclusive edit. Reported, but done, on the audio thread, while claims are held, and
/// when an edit is already in progress (a nested begin: in Max that self-deadlocks on the
/// buffer's mutex, or, if the mutex is recursive, gives two editors the same samples; the mock
/// doesn't model either, it only reports that the code under test got here).
///
/// # Safety
/// As `buffer_locksamples`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_edit_begin(buffer_object: *mut t_buffer_obj) -> t_max_err {
    let audio = crate::thread::current_role() == crate::thread::Role::Audio;
    let r = with_buffer(buffer_object, |b| {
        let mut problems = Vec::new();
        if b.editing > 0 {
            problems.push(format!("median-mock: nested buffer_edit_begin on buffer~ \"{}\", which is already being edited (Max deadlocks or aliases)", b.name));
        }
        if audio {
            problems.push(format!(
                "median-mock: buffer_edit_begin on buffer~ \"{}\" on the audio thread",
                b.name
            ));
        }
        if b.locks > 0 {
            problems.push(format!(
                "median-mock: buffer_edit_begin on buffer~ \"{}\" while {} buffer_locksamples claim(s) are held (Max waits for them)",
                b.name, b.locks
            ));
        }
        b.editing += 1;
        b.edits_begun += 1;
        Ok::<_, String>(problems)
    });
    let addr = buffer_object as usize;
    match r {
        None => err::MAX_ERR_INVALID_PTR as t_max_err,
        Some(Err(e)) => {
            state().post_error(addr, e);
            err::MAX_ERR_GENERIC as t_max_err
        }
        Some(Ok(problems)) => {
            for p in problems {
                state().post_error(addr, p);
            }
            err::MAX_ERR_NONE as t_max_err
        }
    }
}

/// End an edit. Refused (`MAX_ERR_GENERIC`, reported) without a matching `buffer_edit_begin`.
/// `valid` is recorded ([`crate::Kernel::buffer_last_edit_valid`]), not modelled.
///
/// # Safety
/// As `buffer_locksamples`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_edit_end(
    buffer_object: *mut t_buffer_obj,
    valid: c_long,
) -> t_max_err {
    let r = with_buffer(buffer_object, |b| {
        if b.editing == 0 {
            return Err(format!(
                "median-mock: buffer_edit_end on buffer~ \"{}\" without buffer_edit_begin",
                b.name
            ));
        }
        b.editing -= 1;
        b.edits_ended += 1;
        b.last_edit_valid = Some(valid as i64);
        Ok(())
    });
    match r {
        None => err::MAX_ERR_INVALID_PTR as t_max_err,
        Some(Err(e)) => {
            state().post_error(buffer_object as usize, e);
            err::MAX_ERR_GENERIC as t_max_err
        }
        Some(Ok(())) => err::MAX_ERR_NONE as t_max_err,
    }
}

/// Fill `info` (`b_modtime` is 0, `b_rfu` zeroed).
///
/// # Safety
/// As `buffer_locksamples`; `info` must be null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_getinfo(
    buffer_object: *mut t_buffer_obj,
    info: *mut t_buffer_info,
) -> t_max_err {
    if info.is_null() {
        return err::MAX_ERR_INVALID_PTR as t_max_err;
    }
    let r = with_buffer(buffer_object, |b| {
        // SAFETY: `t_buffer_info` is plain data, all zeros is a valid value
        let mut i: t_buffer_info = unsafe { std::mem::zeroed() };
        i.b_name = intern(b.name.as_bytes());
        i.b_samples = b.data.as_mut_ptr();
        i.b_frames = b.frames as c_long;
        i.b_nchans = b.channels as c_long;
        i.b_size = (b.frames * b.channels) as c_long;
        i.b_sr = b.sample_rate as f32;
        i
    });
    match r {
        Some(i) => {
            // SAFETY: the caller passes a writable `t_buffer_info`
            unsafe { info.write(i) };
            err::MAX_ERR_NONE as t_max_err
        }
        None => err::MAX_ERR_INVALID_PTR as t_max_err,
    }
}

/// Records the buffer's name (see [`crate::Kernel::buffer_views`]); a null or unknown buffer is
/// reported.
///
/// # Safety
/// As `buffer_locksamples`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buffer_view(buffer_object: *mut t_buffer_obj) {
    let name = with_buffer(buffer_object, |b| b.name.clone());
    let mut st = state();
    match name {
        Some(n) => st.buffer_views.push(n),
        None => st.post_error(
            buffer_object as usize,
            format!("median-mock: buffer_view on {buffer_object:?}, which is not a buffer~"),
        ),
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
