//! Which Max thread the code is running on.
//! These queries are not mutually exclusive: with Overdrive off, a scheduler callback can
//! report both main and scheduler (observed in Max with DSP off).
//!
//! To run a method on the main thread, mark it `#[defer]` or `#[defer_low]` (see
//! [`crate::defer`]), or use a [`Queue`](crate::queue::Queue).

/// A convenience wrapper around Max's systhread.
pub struct SysThread;

impl SysThread {
    /// Is the current thread the audio thread.
    pub fn is_audio() -> bool {
        unsafe { max_sys::systhread_isaudiothread() != 0 }
    }

    /// Is the current thread the main thread.
    pub fn is_main() -> bool {
        unsafe { max_sys::systhread_ismainthread() != 0 }
    }

    /// Is the current thread a scheduler (timer) thread.
    pub fn is_scheduler() -> bool {
        unsafe { max_sys::systhread_istimerthread() != 0 }
    }
}
