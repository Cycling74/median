//! Access to MSP `buffer~` data.
//!
//! A [`BufferRef`] (usually a [`ManagedBufferRef`](crate::builder::ManagedBufferRef) from
//! `builder.with_buffer(..)`) follows a `buffer~` by name. It gives two kinds of access to the
//! samples, matching the two the SDK has (`ext_buffer.h`):
//!
//! * [`BufferReference::try_lock`] → [`BufferLocked`]: the perform-time **shared** claim
//!   (`buffer_locksamples`/`buffer_unlocksamples`). It may be taken on any thread, including the
//!   audio thread: it does not allocate or lock. Other objects (another `index~`, `record~`, ...)
//!   may read *and write* the samples at the same time, so they are handed out as
//!   [`AtomicSample`]s, never as `&[f32]`/`&mut [f32]`.
//! * [`BufferReference::edit`] → [`BufferEdit`]: the **exclusive** edit
//!   (`buffer_edit_begin`/`buffer_edit_end`), which waits for the shared claims to be released and
//!   keeps new ones out, and so can hand out a real `&mut [f32]`. Never on the audio thread (or
//!   the scheduler thread, which can run inside it): it takes the buffer's mutex and waits.
//!
//! The SDK has no call to resize a buffer (`ext_buffer.h` declares none), so neither guard
//! offers one.
//!
//! # Layout
//!
//! Samples are **interleaved**: frame major, channel minor (the SDK: "stored with interleaved
//! channels if multi-channel"). Sample `ch` of frame `i` is at index `i * channels + ch`. A
//! frame is contiguous ([`BufferLocked::frame`]); a channel is strided
//! ([`BufferLocked::channel_iter`]).
//!
//! # Messages and notifications
//!
//! A `buffer_ref` must see the notifications its owner gets (binding, unbinding, ...). The
//! wrapper forwards them to the references made with the builder; a `BufferRef` made by hand needs
//! [`BufferRef::notify_if`] called from the owner's notification handler.
//! [`Class::add_buffer_messages`](crate::class::Class::add_buffer_messages) registers the usual
//! `set <name>` message and the double-click that opens the buffer's window.
use crate::{notify::Notification, symbol::SymbolRef};
use core::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};

/// Why [`BufferReference::try_lock`] failed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TryLockError {
    /// There is no `buffer~` with the referenced name, or it refused the claim (for example
    /// because it is being edited).
    BufferDoesNotExist,
}

/// Why [`BufferReference::edit`] failed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EditError {
    /// There is no `buffer~` with the referenced name.
    BufferDoesNotExist,
    /// Called on the audio thread or the scheduler thread (`systhread_isaudiothread`,
    /// `systhread_istimerthread`); see [`BufferReference::edit`].
    WrongThread,
    /// `buffer_edit_begin` or `buffer_getinfo` returned this error code.
    Failed(max_sys::t_max_err),
    /// The `buffer~` is being edited through median already (a [`BufferEdit`] of it exists, made
    /// through this reference or another one, on any thread). A second edit would deadlock on
    /// the buffer's mutex, or, if Max's mutex is recursive, give two `&mut [f32]` to the same
    /// samples.
    Busy,
}

/// The `buffer~` objects (by address) that have a [`BufferEdit`] now. Edits are refused on the
/// audio and scheduler threads, so this lock is never taken there.
static EDITING: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

/// Claim `buffer` for an edit in [`EDITING`]: `false` if it is claimed already.
fn claim_edit(buffer: NonNull<max_sys::t_buffer_obj>) -> bool {
    let mut e = EDITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let addr = buffer.as_ptr() as usize;
    if e.contains(&addr) {
        false
    } else {
        e.push(addr);
        true
    }
}

/// Release a claim made by [`claim_edit`].
fn release_edit(buffer: NonNull<max_sys::t_buffer_obj>) {
    let mut e = EDITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let addr = buffer.as_ptr() as usize;
    e.retain(|&a| a != addr);
}

/// One `f32` sample that may be read and written concurrently, by this object and by others.
///
/// A `#[repr(transparent)]` wrapper over `AtomicU32` holding the bits of the `f32`. `get` and
/// `set` are `Relaxed` loads and stores, which compile to plain loads and stores: no fences and no
/// read-modify-write. They make concurrent access from Rust well defined; they do not order
/// anything else, and they do not make a read-modify-write (`s.set(s.get() * 0.5)`) atomic.
///
/// The other writers (C externals, `record~`) use non-atomic stores, so at the C level a
/// concurrent read and write of one sample is still a race; on every platform Max runs on, an
/// aligned 32-bit access is not torn, so the reader sees the old or the new value. That is what
/// Max objects have always relied on.
#[repr(transparent)]
pub struct AtomicSample(AtomicU32);

// `BufferLocked` reinterprets Max's `float *` as `*const AtomicSample`.
const _: () = {
    assert!(std::mem::size_of::<AtomicSample>() == std::mem::size_of::<f32>());
    assert!(std::mem::align_of::<AtomicSample>() == std::mem::align_of::<f32>());
};

impl AtomicSample {
    /// A sample with the value `v`.
    pub const fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }

    /// The value (a `Relaxed` load).
    #[inline]
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    /// Store `v` (a `Relaxed` store).
    #[inline]
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
}

impl std::fmt::Debug for AtomicSample {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.get(), f)
    }
}

/// A reference to a `buffer~` by name (`t_buffer_ref`).
pub struct BufferRef {
    /// From `buffer_ref_new`, freed in `Drop`; null if Max returned null (then there is never a
    /// buffer). Never changed, so reading it needs no lock.
    value: *mut max_sys::t_buffer_ref,
    /// The name the reference follows. `set` and the notification forwarding hold the lock while
    /// they call into the `t_buffer_ref`, so the name and the reference change together.
    name: parking_lot::Mutex<SymbolRef>,
}

// SAFETY: `value` is only passed to the `buffer_ref_*` functions, which the SDK's own objects
// call from the main thread (`set`, `notify`) and the audio thread (`buffer_ref_getobject` in
// perform, `index~.c`) concurrently; `name` is behind a mutex. Nothing else is stored.
unsafe impl Send for BufferRef {}
// SAFETY: as `Send`; every method takes `&self` and either only calls those functions or
// locks `name`.
unsafe impl Sync for BufferRef {}

/// What a [`BufferRef`] can do; [`ManagedBufferRef`](crate::builder::ManagedBufferRef) is an
/// `Arc<dyn BufferReference>`.
pub trait BufferReference: Send + Sync {
    /// Follow the `buffer~` named `name` instead (`buffer_ref_set`). Main thread: it takes a
    /// lock and Max rebinds the reference.
    fn set(&self, name: SymbolRef);

    /// The name of the `buffer~` this follows. Takes the lock `set` takes.
    fn name(&self) -> SymbolRef;

    /// Whether a `buffer~` with the name exists.
    fn exists(&self) -> bool;

    /// The number of channels of the `buffer~`, if there is one.
    fn channels(&self) -> Option<usize>;

    /// The number of frames of the `buffer~`, if there is one.
    fn frames(&self) -> Option<usize>;

    /// The sample rate of the `buffer~`'s data in samples per second, if there is one.
    fn sample_rate(&self) -> Option<f64>;

    /// The sample rate of the `buffer~`'s data in samples per millisecond, if there is one.
    fn millisample_rate(&self) -> Option<f64>;

    /// Claim the samples for shared access (`buffer_locksamples`), released when the
    /// [`BufferLocked`] is dropped (also when a panic unwinds through it). For `perform`: it
    /// neither allocates nor locks. It fails if there is no buffer or the buffer refuses the
    /// claim, e.g. while it is being edited.
    fn try_lock(&self) -> Result<BufferLocked<'_>, TryLockError>;

    /// Start an exclusive edit (`buffer_edit_begin`), ended when the [`BufferEdit`] is dropped
    /// (also when a panic unwinds through it).
    ///
    /// **Threads.** `buffer_edit_begin` takes the buffer's mutex and waits until every shared
    /// claim (every [`BufferLocked`], and every C object's `buffer_locksamples`) is released, so
    /// it must not run where a perform routine would wait for it: it is refused with
    /// [`EditError::WrongThread`] on the audio thread and on the scheduler thread (which, with
    /// "Scheduler in Audio Interrupt", runs inside the audio thread). Use it on the main thread
    /// (defer to it from a clock) or a thread of your own. Do not hold a [`BufferLocked`] of the
    /// same buffer on the same thread while calling it: the edit would wait for that claim
    /// forever.
    ///
    /// **One at a time.** While a [`BufferEdit`] of a `buffer~` exists, another `edit` of it,
    /// through this reference or any other, on any thread, is [`EditError::Busy`] (median keeps
    /// a list of the buffers it is editing): a second `buffer_edit_begin` would deadlock, or alias
    /// the samples.
    fn edit(&self) -> Result<BufferEdit<'_>, EditError>;

    /// Open the `buffer~`'s window (`buffer_view`), as `index~` does on a double-click. Returns
    /// whether there was a buffer to show. Main thread.
    fn view(&self) -> bool;
}

impl BufferRef {
    /// Make a reference to the `buffer~` named `name` (the empty symbol if `None`) for `owner`.
    ///
    /// # Safety
    /// `owner` must be a valid Max object that outlives the returned reference, and its `notify`
    /// method must pass its notifications to [`BufferRef::notify_if`] (the wrapper does that for
    /// references made with the builder).
    pub unsafe fn new(owner: *mut max_sys::t_object, name: Option<SymbolRef>) -> Self {
        let name = name.unwrap_or_default();
        // SAFETY: the caller guarantees `owner` is valid, and `name` is a symbol
        let value = unsafe { max_sys::buffer_ref_new(owner, name.as_ptr()) };
        Self {
            value,
            name: parking_lot::Mutex::new(name),
        }
    }

    /// The `buffer~` object, if there is one. No lock is taken.
    fn object(&self) -> Option<NonNull<max_sys::t_buffer_obj>> {
        if self.value.is_null() {
            return None;
        }
        // SAFETY: `value` is our valid buffer reference
        NonNull::new(unsafe { max_sys::buffer_ref_getobject(self.value) })
    }

    /// Whether `notification` is one a buffer reference has to see (a binding or unbinding in the
    /// `globalsymbol` namespace).
    pub fn is_applicable(notification: &Notification) -> bool {
        if notification.sender().is_null() {
            false
        } else {
            let message = *notification.message();
            message == crate::sym!("globalsymbol_binding")
                || message == crate::sym!("globalsymbol_unbinding")
        }
    }

    /// Pass the notification to the buffer reference (`buffer_ref_notify`) if it is about the
    /// buffer this follows.
    ///
    /// It is fine to pass notifications meant for other objects, including other buffer
    /// references.
    ///
    /// # Safety
    /// [`is_applicable`](Self::is_applicable) must have returned `true` for `notification`:
    /// its data is then the `buffer~` (or other object) that answers the `getname` message.
    pub unsafe fn notify_if_unchecked(&self, notification: &Notification) {
        if self.value.is_null() {
            return;
        }
        //try to get the name of the buffer
        // Max writes the symbol through this pointer, so it must be derived from a mutable place.
        let mut name: *mut max_sys::t_symbol = std::ptr::null_mut();
        // SAFETY: the caller guarantees `notification` is a binding notification Max handed us,
        // so its data is a valid object for the `getname` message, which writes a symbol through
        // the out pointer
        unsafe {
            max_sys::object_method(
                notification.data(),
                crate::sym!("getname").as_ptr(),
                &raw mut name as *mut c_void,
            );
        }
        let ours = self.name.lock();
        //if the name matches our buffer's name, send notification
        // SAFETY: `name` was just written by Max and is checked for null
        if !name.is_null() && unsafe { SymbolRef::from_raw(name) } == *ours {
            // SAFETY: `value` is our valid buffer reference and the notification's symbols and
            // pointers are what Max passed (`Notification::new`'s contract)
            unsafe {
                max_sys::buffer_ref_notify(
                    self.value,
                    notification.sender_name().as_ptr(),
                    notification.message().as_ptr(),
                    notification.sender(),
                    notification.data(),
                );
            }
        }
    }

    /// Pass the notification to the buffer reference if it is applicable (see
    /// [`notify_if_unchecked`](Self::notify_if_unchecked)).
    pub fn notify_if(&self, notification: &Notification) {
        if Self::is_applicable(notification) {
            // SAFETY: applicable; a `Notification` holds what Max passed (`Notification::new`)
            unsafe { self.notify_if_unchecked(notification) }
        }
    }
}

impl BufferReference for BufferRef {
    fn set(&self, name: SymbolRef) {
        let mut ours = self.name.lock();
        *ours = name;
        if !self.value.is_null() {
            // SAFETY: `value` is our valid buffer reference, `name` a symbol
            unsafe { max_sys::buffer_ref_set(self.value, name.as_ptr()) };
        }
    }

    fn name(&self) -> SymbolRef {
        *self.name.lock()
    }

    fn exists(&self) -> bool {
        // SAFETY: `value` is our valid buffer reference
        !self.value.is_null() && unsafe { max_sys::buffer_ref_exists(self.value) != 0 }
    }

    fn channels(&self) -> Option<usize> {
        // SAFETY: `object` returns a buffer object
        self.object()
            .map(|b| unsafe { max_sys::buffer_getchannelcount(b.as_ptr()) }.max(0) as usize)
    }

    fn frames(&self) -> Option<usize> {
        // SAFETY: as `channels`
        self.object()
            .map(|b| unsafe { max_sys::buffer_getframecount(b.as_ptr()) }.max(0) as usize)
    }

    fn sample_rate(&self) -> Option<f64> {
        // SAFETY: as `channels`
        self.object()
            .map(|b| unsafe { max_sys::buffer_getsamplerate(b.as_ptr()) })
    }

    fn millisample_rate(&self) -> Option<f64> {
        // SAFETY: as `channels`
        self.object()
            .map(|b| unsafe { max_sys::buffer_getmillisamplerate(b.as_ptr()) })
    }

    fn try_lock(&self) -> Result<BufferLocked<'_>, TryLockError> {
        let buffer = self.object().ok_or(TryLockError::BufferDoesNotExist)?;
        // SAFETY: a buffer object; paired with `buffer_unlocksamples` in `BufferLocked::drop`
        let samples = unsafe { max_sys::buffer_locksamples(buffer.as_ptr()) };
        let samples = NonNull::new(samples).ok_or(TryLockError::BufferDoesNotExist)?;
        //the claim is held from here: make the guard first so that every path releases it
        let mut locked = BufferLocked {
            buffer,
            samples: samples.cast(),
            frames: 0,
            channels: 0,
            dirty: false,
            _ref: PhantomData,
        };
        //read after the claim, as `index~.c` does: the size can't change while it is held
        // SAFETY: a buffer object
        let frames = unsafe { max_sys::buffer_getframecount(buffer.as_ptr()) }.max(0) as usize;
        // SAFETY: as above
        let channels = unsafe { max_sys::buffer_getchannelcount(buffer.as_ptr()) }.max(0) as usize;
        if frames.checked_mul(channels).is_none() {
            return Err(TryLockError::BufferDoesNotExist);
        }
        locked.frames = frames;
        locked.channels = channels;
        Ok(locked)
    }

    fn edit(&self) -> Result<BufferEdit<'_>, EditError> {
        // SAFETY: no requirements
        let wrong = unsafe {
            max_sys::systhread_isaudiothread() != 0 || max_sys::systhread_istimerthread() != 0
        };
        if wrong {
            return Err(EditError::WrongThread);
        }
        let buffer = self.object().ok_or(EditError::BufferDoesNotExist)?;
        //one median edit per buffer~, whichever reference it is made through: checked before
        //`buffer_edit_begin`, which would deadlock (or alias) on a second edit
        if !claim_edit(buffer) {
            return Err(EditError::Busy);
        }
        // SAFETY: a buffer object; paired with `buffer_edit_end` in `BufferEdit::drop`
        let r = unsafe { max_sys::buffer_edit_begin(buffer.as_ptr()) };
        if r != max_sys::e_max_errorcodes::MAX_ERR_NONE as max_sys::t_max_err {
            release_edit(buffer);
            return Err(EditError::Failed(r));
        }
        //the edit is in progress from here: make the guard first so that every path ends it (and
        //releases the claim)
        let mut edit = BufferEdit {
            buffer,
            samples: NonNull::dangling(),
            frames: 0,
            channels: 0,
            dirty: false,
            restore_valid: true,
            _ref: PhantomData,
        };
        // SAFETY: `t_buffer_info` is plain data, all zeros is a valid value
        let mut info: max_sys::t_buffer_info = unsafe { std::mem::zeroed() };
        // SAFETY: a buffer object and a writable `t_buffer_info`
        let r = unsafe { max_sys::buffer_getinfo(buffer.as_ptr(), &mut info) };
        if r != max_sys::e_max_errorcodes::MAX_ERR_NONE as max_sys::t_max_err {
            return Err(EditError::Failed(r));
        }
        if let Some(samples) = NonNull::new(info.b_samples) {
            let frames = info.b_frames.max(0) as usize;
            let channels = info.b_nchans.max(0) as usize;
            if frames.checked_mul(channels).is_none() {
                return Err(EditError::Failed(
                    max_sys::e_max_errorcodes::MAX_ERR_GENERIC as max_sys::t_max_err,
                ));
            }
            edit.samples = samples;
            edit.frames = frames;
            edit.channels = channels;
            //it had sample memory: valid after the edit
            edit.restore_valid = frames == 0;
        }
        Ok(edit)
    }

    fn view(&self) -> bool {
        match self.object() {
            Some(b) => {
                // SAFETY: a buffer object
                unsafe { max_sys::buffer_view(b.as_ptr()) };
                true
            }
            None => false,
        }
    }
}

impl Drop for BufferRef {
    fn drop(&mut self) {
        if !self.value.is_null() {
            // SAFETY: `value` came from `buffer_ref_new` and is freed once, here
            unsafe { max_sys::object_free(self.value as _) };
        }
    }
}

/// A shared claim on a `buffer~`'s samples, from [`BufferReference::try_lock`]; released on drop.
///
/// The samples are [`AtomicSample`]s (other objects may write them while the claim is held, see
/// the [module documentation](self)), interleaved: sample `ch` of frame `i` is at
/// `i * channels() + ch`. The size is read once, when the claim is taken; a buffer can't be
/// resized while it is claimed.
///
/// Not `Send`: release a claim on the thread that took it.
///
/// **Don't leak it** (`mem::forget`, a reference cycle): the claim (the buffer's in-use count)
/// is then never released, and every later `buffer_edit_begin` on that `buffer~`, by any object in
/// Max, waits for it forever: a hang of whatever thread edits the buffer (usually the main
/// thread), not a local leak.
pub struct BufferLocked<'a> {
    buffer: NonNull<max_sys::t_buffer_obj>,
    samples: NonNull<AtomicSample>,
    frames: usize,
    channels: usize,
    dirty: bool,
    _ref: PhantomData<&'a ()>,
}

impl BufferLocked<'_> {
    /// The number of channels.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The number of frames.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The sample rate of the data, samples per second.
    pub fn sample_rate(&self) -> f64 {
        // SAFETY: a claimed buffer object
        unsafe { max_sys::buffer_getsamplerate(self.buffer.as_ptr()) }
    }

    /// The sample rate of the data, samples per millisecond.
    pub fn millisample_rate(&self) -> f64 {
        // SAFETY: a claimed buffer object
        unsafe { max_sys::buffer_getmillisamplerate(self.buffer.as_ptr()) }
    }

    /// All the samples, interleaved (`frames() * channels()` of them).
    pub fn as_slice(&self) -> &[AtomicSample] {
        // SAFETY: Max's `float *` from `buffer_locksamples` points to `frames * channels` floats
        // (checked not to overflow) that stay allocated while the claim is held, which is at
        // least as long as `&self`; `AtomicSample` has the size and alignment of `f32` (checked
        // above), and all access through it is atomic
        unsafe { std::slice::from_raw_parts(self.samples.as_ptr(), self.frames * self.channels) }
    }

    /// The samples of frame `i`, one per channel.
    ///
    /// # Panics
    /// If `i >= frames()`.
    pub fn frame(&self, i: usize) -> &[AtomicSample] {
        assert!(i < self.frames, "frame {i} out of range ({} frames)", self.frames);
        &self.as_slice()[i * self.channels..(i + 1) * self.channels]
    }

    /// Sample `channel` of frame `frame`.
    ///
    /// # Panics
    /// If `frame >= frames()` or `channel >= channels()`.
    pub fn sample(&self, frame: usize, channel: usize) -> &AtomicSample {
        assert!(
            channel < self.channels,
            "channel {channel} out of range ({} channels)",
            self.channels
        );
        &self.frame(frame)[channel]
    }

    /// The samples of one channel, one per frame (a strided view of the interleaved data).
    ///
    /// # Panics
    /// If `channel >= channels()`.
    pub fn channel_iter(
        &self,
        channel: usize,
    ) -> impl ExactSizeIterator<Item = &AtomicSample> + DoubleEndedIterator + Clone {
        assert!(
            channel < self.channels,
            "channel {channel} out of range ({} channels)",
            self.channels
        );
        //`get`: an empty buffer (0 frames) has no samples at all, whatever the channel count
        self.as_slice().get(channel..).unwrap_or_default().iter().step_by(self.channels)
    }

    /// Mark the buffer as changed (`buffer_setdirty`, when the claim is released), so that its
    /// window and other clients update. Call it after writing.
    pub fn set_dirty(&mut self) {
        self.dirty = true;
    }

    /// The first sample, as Max's raw pointer. `frames() * channels()` interleaved floats are
    /// valid while `self` is.
    ///
    /// # Safety
    /// Calling this is harmless, but any access through the pointer races with other objects
    /// using the buffer: use only atomic (or volatile) accesses that are aligned and in bounds,
    /// never make a `&`/`&mut` slice of `f32` from it, and don't use it after `self` is dropped.
    pub unsafe fn samples_ptr(&self) -> *mut f32 {
        self.samples.as_ptr().cast()
    }
}

impl Drop for BufferLocked<'_> {
    fn drop(&mut self) {
        // SAFETY: the buffer object we claimed with `buffer_locksamples`, released once, here
        unsafe {
            if self.dirty {
                max_sys::buffer_setdirty(self.buffer.as_ptr());
            }
            max_sys::buffer_unlocksamples(self.buffer.as_ptr());
        }
    }
}

/// An exclusive edit of a `buffer~`, from [`BufferReference::edit`]; ended on drop
/// (`buffer_edit_end`, then `buffer_setdirty` if a mutable view was taken).
///
/// While it exists no perform routine can claim the buffer (their `buffer_locksamples` fails),
/// and other editors wait, so the samples are a plain `&mut [f32]`, interleaved as for
/// [`BufferLocked`]. The sample pointer and size come from `buffer_getinfo`.
///
/// Not `Send`: `buffer_edit_end` must run on the thread that began the edit (it unlocks the
/// buffer's mutex).
///
/// Only one `BufferEdit` of a `buffer~` exists at a time (a second [`BufferReference::edit`] is
/// [`EditError::Busy`]). **Don't leak it** (`mem::forget`, a reference cycle): the edit never
/// ends, so the buffer's mutex stays held and its valid flag cleared. Every perform routine in Max
/// that plays the buffer then fails to claim it (silence), and the next `buffer_edit_begin` on
/// it, by any object, waits forever: a Max-wide hang on that `buffer~`, not a local leak.
pub struct BufferEdit<'a> {
    buffer: NonNull<max_sys::t_buffer_obj>,
    samples: NonNull<f32>,
    frames: usize,
    channels: usize,
    dirty: bool,
    /// The buffer had no sample memory when the edit began: end the edit restoring its old
    /// valid flag instead of marking it valid.
    restore_valid: bool,
    _ref: PhantomData<&'a ()>,
}

impl BufferEdit<'_> {
    /// The number of channels.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The number of frames (0 if the buffer has no sample memory).
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The sample rate of the data, samples per second.
    pub fn sample_rate(&self) -> f64 {
        // SAFETY: the buffer object being edited
        unsafe { max_sys::buffer_getsamplerate(self.buffer.as_ptr()) }
    }

    /// All the samples, interleaved.
    pub fn as_slice(&self) -> &[f32] {
        // SAFETY: `buffer_getinfo` gave `b_samples` with `frames * channels` floats (or nothing,
        // with a dangling pointer and length 0), allocated and not accessed by anyone else for as
        // long as the edit is in progress, which is at least as long as `&self`
        unsafe { std::slice::from_raw_parts(self.samples.as_ptr(), self.frames * self.channels) }
    }

    /// All the samples, interleaved, for writing. Marks the buffer dirty.
    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        self.dirty = true;
        // SAFETY: as `as_slice`, and `&mut self` makes this the only view
        unsafe {
            std::slice::from_raw_parts_mut(self.samples.as_ptr(), self.frames * self.channels)
        }
    }

    /// The samples of frame `i`, one per channel.
    ///
    /// # Panics
    /// If `i >= frames()`.
    pub fn frame(&self, i: usize) -> &[f32] {
        assert!(i < self.frames, "frame {i} out of range ({} frames)", self.frames);
        &self.as_slice()[i * self.channels..(i + 1) * self.channels]
    }

    /// The samples of frame `i`, for writing. Marks the buffer dirty.
    ///
    /// # Panics
    /// If `i >= frames()`.
    pub fn frame_mut(&mut self, i: usize) -> &mut [f32] {
        assert!(i < self.frames, "frame {i} out of range ({} frames)", self.frames);
        let c = self.channels;
        &mut self.as_mut_slice()[i * c..(i + 1) * c]
    }

    /// The samples of one channel, one per frame.
    ///
    /// # Panics
    /// If `channel >= channels()`.
    pub fn channel_iter(
        &self,
        channel: usize,
    ) -> impl ExactSizeIterator<Item = &f32> + DoubleEndedIterator + Clone {
        assert!(
            channel < self.channels,
            "channel {channel} out of range ({} channels)",
            self.channels
        );
        //`get`: an empty buffer (0 frames) has no samples at all, whatever the channel count
        self.as_slice().get(channel..).unwrap_or_default().iter().step_by(self.channels)
    }

    /// The samples of one channel, for writing. Marks the buffer dirty.
    ///
    /// # Panics
    /// If `channel >= channels()`.
    pub fn channel_iter_mut(
        &mut self,
        channel: usize,
    ) -> impl ExactSizeIterator<Item = &mut f32> + DoubleEndedIterator {
        assert!(
            channel < self.channels,
            "channel {channel} out of range ({} channels)",
            self.channels
        );
        let c = self.channels;
        //as in `channel_iter`
        self.as_mut_slice().get_mut(channel..).unwrap_or_default().iter_mut().step_by(c)
    }

    /// Mark the buffer as changed even if no mutable view was taken.
    pub fn set_dirty(&mut self) {
        self.dirty = true;
    }
}

impl Drop for BufferEdit<'_> {
    fn drop(&mut self) {
        //`ext_buffer.h`: `buffer_edit_end(t_buffer_obj *, long valid); // valid 0=FALSE,
        //positive=TRUE, negative=RESTORE_OLD_VALID (not common)`. A buffer that had samples is
        //valid after the edit; one without sample memory gets its old valid flag back rather than
        //being declared valid
        let valid = if self.restore_valid { -1 } else { 1 };
        // SAFETY: the buffer object we began editing, ended once, here
        unsafe {
            max_sys::buffer_edit_end(self.buffer.as_ptr(), valid);
            if self.dirty {
                max_sys::buffer_setdirty(self.buffer.as_ptr());
            }
        }
        release_edit(self.buffer);
    }
}
