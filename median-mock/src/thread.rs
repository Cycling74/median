//! Thread roles: `systhread_ismainthread` and friends.
//!
//! With Overdrive off, Max's scheduler callbacks can report both main and timer roles.
//! In the mock each OS thread has a [`Role`]. A thread is [`Role::Main`] unless told otherwise; the
//! kernel switches the role of the calling thread while it runs a clock callback
//! ([`Role::Timer`]), a perform routine ([`Role::Audio`]) or the main queue ([`Role::Main`]).

use std::cell::Cell;
use std::ffi::c_short;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Main,
    Timer,
    /// A scheduler callback on the main thread (Overdrive off, observed with DSP off).
    MainTimer,
    Audio,
    /// Any other thread, for tests that want to check behaviour off the three Max threads.
    Other,
}

thread_local! {
    static ROLE: Cell<Role> = const { Cell::new(Role::Main) };
}

pub fn current_role() -> Role {
    ROLE.with(|r| r.get())
}

/// Run `f` with the calling thread's role set to `role`, restoring the previous role afterwards
/// (also if `f` panics).
pub fn with_role<R>(role: Role, f: impl FnOnce() -> R) -> R {
    struct Restore(Role);
    impl Drop for Restore {
        fn drop(&mut self) {
            ROLE.with(|r| r.set(self.0));
        }
    }
    let _restore = Restore(ROLE.with(|r| r.replace(role)));
    f()
}

/// # Safety
/// No requirements; `unsafe` only because it is an exported Max function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn systhread_ismainthread() -> c_short {
    matches!(current_role(), Role::Main | Role::MainTimer) as c_short
}

/// # Safety
/// No requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn systhread_istimerthread() -> c_short {
    matches!(current_role(), Role::Timer | Role::MainTimer) as c_short
}

/// # Safety
/// No requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn systhread_isaudiothread() -> c_short {
    (current_role() == Role::Audio) as c_short
}
