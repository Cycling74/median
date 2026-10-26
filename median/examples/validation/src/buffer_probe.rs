//! Read-only SDK contract probe. No sample references or sample writes are made here.

use median::{builder::ManagedBufferRef, max_sys as sys, prelude::*, thread::SysThread};
use std::{
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
    time::Duration,
};

const IDLE: usize = 0;
const BEFORE: usize = 1;
const DURING: usize = 2;
const AFTER: usize = 3;

#[derive(MaxObject)]
#[max(name = "median.validate.buffer~", kind = msp)]
pub struct BufferProbe {
    obj: ObjRef,
    name: SymbolRef,
    buffer: ManagedBufferRef,
    running: AtomicBool,
    phase: AtomicUsize,
    // Per-window successful / refused claims; perform updates these without locks.
    claims: [[AtomicUsize; 2]; 4],
}

impl MSPObjWrapped for BufferProbe {
    fn new(b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let name = b
            .creation_args()
            .first()
            .map(|a| a.get_symbol())
            .filter(|s| *s != median::sym!(""))
            .ok_or("provide the name of a dedicated buffer~")?;
        b.inlet(Inlet::default().assist("probe: inspect buffer edit exclusion; see console"));
        b.outlet(Outlet::signal().assist("silence; connect to snapshot~ to keep DSP active"));
        Ok(Self {
            obj: b.obj(),
            name,
            buffer: b.with_buffer(Some(name)),
            running: AtomicBool::new(false),
            phase: AtomicUsize::new(IDLE),
            claims: std::array::from_fn(|_| std::array::from_fn(|_| AtomicUsize::new(0))),
        })
    }

    fn perform(&self, _: &[&[f64]], outs: &mut [&mut [f64]], _: usize) {
        outs[0].fill(0.);
        let phase = self.phase.load(SeqCst);
        if phase == IDLE {
            return;
        }
        // try_lock calls buffer_locksamples; no slice is requested or sample accessed.
        let locked = self.buffer.try_lock();
        let refused = usize::from(locked.is_err());
        drop(locked);
        // Discard calls spanning a phase boundary rather than attribute them to the edit.
        if self.phase.load(SeqCst) == phase {
            self.claims[phase][refused].fetch_add(1, SeqCst);
        }
    }
}

struct Run<'a>(&'a BufferProbe);
impl Drop for Run<'_> {
    fn drop(&mut self) {
        self.0.phase.store(IDLE, SeqCst);
        self.0.running.store(false, SeqCst);
    }
}

// A short-lived raw reference on the main thread. No event loop is pumped while it lives;
// the builder-managed reference separately handles binding notifications for perform.
struct Reference(NonNull<sys::t_buffer_ref>);
impl Drop for Reference {
    fn drop(&mut self) {
        unsafe {
            sys::object_free(self.0.as_ptr().cast());
        }
    }
}

struct Edit(NonNull<sys::t_buffer_obj>, bool);
impl Edit {
    fn finish(&mut self) -> sys::t_max_err {
        self.1 = false;
        // No data was changed: restore the buffer's original valid flag.
        unsafe { sys::buffer_edit_end(self.0.as_ptr(), -1) }
    }
}
impl Drop for Edit {
    fn drop(&mut self) {
        if self.1 {
            self.finish();
        }
    }
}

struct Claim(NonNull<sys::t_buffer_obj>, *mut f32);
impl Claim {
    /// # Safety
    /// `buffer` must remain a live buffer object until the returned claim is dropped.
    unsafe fn new(buffer: NonNull<sys::t_buffer_obj>) -> Self {
        Self(buffer, unsafe { sys::buffer_locksamples(buffer.as_ptr()) })
    }
}
impl Drop for Claim {
    fn drop(&mut self) {
        if !self.1.is_null() {
            unsafe {
                sys::buffer_unlocksamples(self.0.as_ptr());
            }
        }
    }
}

#[median::methods]
impl BufferProbe {
    #[sel("probe")]
    #[defer_low]
    fn probe(&self) -> Result<(), String> {
        self.probe_with(|| std::thread::sleep(Duration::from_millis(150)))
    }

    // The observation hook lets tests drive deterministic audio callbacks in each window.
    pub(crate) fn probe_with(&self, mut observe: impl FnMut()) -> Result<(), String> {
        if !SysThread::is_main() || SysThread::is_scheduler() || SysThread::is_audio() {
            return Err("buffer probe requires the low-priority main thread".into());
        }
        if self.running.swap(true, SeqCst) {
            return Err("buffer probe already running".into());
        }
        let _run = Run(self);
        for counts in &self.claims {
            for n in counts {
                n.store(0, SeqCst);
            }
        }
        // SAFETY: owner lives throughout this synchronous main-thread call. No binding
        // changes or notifications are dispatched through a nested event loop here.
        let reference = Reference(
            NonNull::new(unsafe { sys::buffer_ref_new(self.obj.as_ptr(), self.name.as_ptr()) })
                .ok_or("unable to allocate probe buffer reference")?,
        );
        let buffer = NonNull::new(unsafe { sys::buffer_ref_getobject(reference.0.as_ptr()) })
            .ok_or("probe buffer~ does not exist")?;
        let before = unsafe { Claim::new(buffer) };
        if before.1.is_null() {
            return Err("INCONCLUSIVE: buffer has no claimable samples before edit".into());
        }
        let before_ptr = before.1 as usize;
        drop(before); // Must release before edit_begin, which waits for existing claims.
        self.window(BEFORE, &mut observe);

        let begin = unsafe { sys::buffer_edit_begin(buffer.as_ptr()) };
        if begin != 0 {
            return Err(format!("buffer_edit_begin failed: {begin}"));
        }
        let mut edit = Edit(buffer, true);
        let mut info: sys::t_buffer_info = unsafe { std::mem::zeroed() };
        let info_error = unsafe { sys::buffer_getinfo(buffer.as_ptr(), &mut info) };
        let during = unsafe { Claim::new(buffer) };
        let during_ptr = during.1 as usize;
        drop(during); // Also balance an unexpectedly successful claim.
        self.window(DURING, &mut observe);
        let end = edit.finish();
        let after = unsafe { Claim::new(buffer) };
        let after_ptr = after.1 as usize;
        drop(after);
        self.window(AFTER, &mut observe);

        let info_ptr = info.b_samples as usize;
        let pointer_result =
            if info_error != 0 || info.b_frames <= 0 || info.b_nchans <= 0 || end != 0 {
                "INCONCLUSIVE"
            } else if before_ptr == info_ptr && info_ptr == after_ptr {
                "PASS"
            } else {
                "FAIL"
            };
        self.obj.post(format!(
            "buffer probe {}: main={} scheduler={} audio={}; edit_begin={begin} getinfo={info_error} edit_end={end}",
            self.name, SysThread::is_main(), SysThread::is_scheduler(), SysThread::is_audio(),
        ));
        self.obj.post(format!(
            "buffer probe: before=0x{before_ptr:x} edit_info=0x{info_ptr:x} during_lock=0x{during_ptr:x} after=0x{after_ptr:x}; frames={} channels={}",
            info.b_frames, info.b_nchans,
        ));
        self.obj.post(format!(
            "buffer probe: main_exclusion={} pointer_identity={}",
            if during_ptr == 0 { "PASS" } else { "FAIL" },
            pointer_result,
        ));
        let counts: Vec<_> = [BEFORE, DURING, AFTER]
            .into_iter()
            .map(|phase| {
                [
                    self.claims[phase][0].load(SeqCst),
                    self.claims[phase][1].load(SeqCst),
                ]
            })
            .collect();
        let audio = if counts[1][0] > 0 {
            "FAIL"
        } else if counts[0][0] > 0 && counts[1][1] > 0 && counts[2][0] > 0 {
            "PASS"
        } else {
            "INCONCLUSIVE (DSP off or insufficient callbacks)"
        };
        self.obj.post(format!(
            "buffer probe: audio claims/refused before={}/{} during={}/{} after={}/{}; audio_exclusion={audio}",
            counts[0][0], counts[0][1], counts[1][0], counts[1][1], counts[2][0], counts[2][1],
        ));
        Ok(())
    }

    fn window(&self, phase: usize, observe: &mut impl FnMut()) {
        self.phase.store(phase, SeqCst);
        observe();
        self.phase.store(IDLE, SeqCst);
    }
}
