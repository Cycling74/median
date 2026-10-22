//! An object freed from inside its own clock callback: the documented behaviour (see
//! `ClockShared` in `src/clock.rs`) is that the freeing thread blocks forever instead of freeing
//! the object under the running callback, after posting an error for the object.
//!
//! The blocked thread keeps the `median-mock` kernel, so this is the only test in its binary.

use median::prelude::*;
use std::sync::{
    Once,
    atomic::{AtomicUsize, Ordering},
};

#[derive(MaxObject)]
#[max(name = "free.inside")]
pub struct SelfFreeing {
    obj: ObjRef,
    clock: Clock,
}

static STARTED: AtomicUsize = AtomicUsize::new(0);
static AFTER_FREE: AtomicUsize = AtomicUsize::new(0);

impl MaxObjWrapped for SelfFreeing {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            obj: b.obj(),
            clock: b.clock(Self::tick),
        })
    }
}

#[median::methods]
impl SelfFreeing {
    #[bang]
    pub fn bang(&self) {
        self.clock.delay(1);
    }

    /// Frees its own object (what deleting its box through scripting does).
    pub fn tick(&self) {
        STARTED.fetch_add(1, Ordering::SeqCst);
        // SAFETY: the object is alive; freeing it from its own callback never returns (see
        // `ClockShared`), so `self` is not used after the free
        unsafe { max_sys::object_free(self.obj.as_ptr().cast()) };
        //never reached
        AFTER_FREE.fetch_add(1, Ordering::SeqCst);
    }
}

median::ext_main! { SelfFreeing }

fn register() {
    static ONCE: Once = Once::new();
    // SAFETY: called once, as Max does
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
}

#[test]
fn freeing_the_owner_from_its_own_clock_callback_blocks_that_thread() {
    //leaked: the blocked thread uses it until the process exits
    let k: &'static median_mock::Kernel = Box::leak(Box::new(median_mock::lock()));
    register();
    let obj = k.new_object("free.inside", &[]) as usize;
    assert_ne!(obj, 0);
    let objects = k.live_object_count();
    let parked = median::clock::parked_in_own_callback();
    k.send_bang(obj as _).unwrap();
    //the "scheduler thread": never joined, it stays blocked until the test binary exits
    let t = std::thread::spawn(move || k.advance(10.0));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while median::clock::parked_in_own_callback() == parked && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    //the callback ran, and its thread is parked in median's free-inside-callback path, not hung
    //somewhere else
    assert_eq!(STARTED.load(Ordering::SeqCst), 1);
    assert!(median::clock::parked_in_own_callback() > parked);
    assert!(!t.is_finished());
    assert_eq!(AFTER_FREE.load(Ordering::SeqCst), 0);
    //the free method has not returned, so the object's memory is still there
    assert_eq!(k.live_object_count(), objects);
    assert_eq!(k.class_name_of(obj as _).as_deref(), Some("free.inside"));
    //and the user is told, for the object
    let log = k.console_log();
    assert!(
        log.iter().any(|e| e.level == median_mock::Level::Error
            && e.object == obj
            && e.text.contains("from inside its own clock, queue or deferred callback")),
        "{log:?}"
    );
}
