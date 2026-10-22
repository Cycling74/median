//! Lock-free single-producer single-consumer FIFOs, to hand data from the audio thread to the
//! main thread (or between any two threads).
//!
//! `perform` runs on the audio thread, where nothing may block or allocate, and it must not send
//! through outlets (the SDK's rule, see [`outlet`](crate::outlet)). To get values out of it, push
//! them into a ring buffer, [`set`](crate::queue::Queue::set) a [`Queue`](crate::queue::Queue),
//! and drain the ring buffer in the queue's callback on the main thread:
//!
//! ```no_run
//! use median::fifo::{Consumer, Producer, RingBuffer, Shared};
//! use median::prelude::*;
//!
//! #[derive(MaxObject)]
//! #[max(name = "peak~", kind = msp)]
//! pub struct Peak {
//!     tx: Shared<Producer<f64>>,
//!     rx: Shared<Consumer<f64>>,
//!     drain: Queue,
//!     out: Outlet<Float>,
//! }
//!
//! impl MSPObjWrapped for Peak {
//!     fn new(b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
//!         b.inlet(Inlet::signal());
//!         //allocate here, never on the audio thread
//!         let (tx, rx) = RingBuffer::new(64);
//!         Ok(Self {
//!             tx: Shared::new(tx),
//!             rx: Shared::new(rx),
//!             drain: b.queue(Self::drain),
//!             out: b.outlet(Outlet::float()),
//!         })
//!     }
//!
//!     //the audio thread: push (a full buffer drops the value) and wake the main thread
//!     fn perform(&self, ins: &[&[f64]], _outs: &mut [&mut [f64]], _nframes: usize) {
//!         let peak = ins[0].iter().fold(0f64, |m, v| m.max(v.abs()));
//!         let _ = self.tx.try_with(|tx| tx.push(peak));
//!         self.drain.set();
//!     }
//! }
//!
//! #[median::methods]
//! impl Peak {
//!     //the main thread: drain
//!     pub fn drain(&self) {
//!         self.rx.try_with(|rx| {
//!             while let Ok(peak) = rx.pop() {
//!                 let _ = self.out.send(peak);
//!             }
//!         });
//!     }
//! }
//! ```
//!
//! The ring buffer is [`rtrb`](https://docs.rs/rtrb)'s, re-exported: [`RingBuffer::new`] makes
//! a [`Producer`] and a [`Consumer`] (allocate it in `new`). Their `push` and `pop` take
//! `&mut self`, but your methods and `perform` only get `&self`, so keep each end in a
//! [`Shared`], whose [`try_with`](Shared::try_with) lends it out without ever blocking.
//!
//! **Caveat**: whether `qelem_set` (what [`Queue::set`](crate::queue::Queue::set) calls) is
//! real-time safe is not documented in the SDK headers; the SDK's `simplethread` example calls it
//! from a thread of its own, which shows it may be called off the main thread, not that it never
//! blocks. This is unverified for the audio thread. The SDK's own pattern for perform is a clock
//! (`pictmeter~` calls `clock_delay` from perform): a [`Clock`](crate::clock::Clock) that polls
//! the ring buffer on the scheduler thread, then a `Queue` from there, avoids the question.

pub use rtrb::{Consumer, PopError, Producer, PushError, RingBuffer};

/// One end of a ring buffer (a [`Producer`] or a [`Consumer`]), usable through `&self`.
///
/// An SPSC ring buffer end is used by one thread at a time; `Shared` makes that a check instead
/// of a promise. [`try_with`](Self::try_with) never blocks, doesn't allocate and makes no system
/// call (a single atomic compare-and-swap, `parking_lot`'s `try_lock`): it is safe on the audio
/// thread. It returns `None` only when another thread is using the same end at that moment, which
/// doesn't happen when, as intended, one thread pushes and another pops.
pub struct Shared<E> {
    end: parking_lot::Mutex<E>,
}

impl<E> Shared<E> {
    /// Wrap a ring buffer end.
    pub fn new(end: E) -> Self {
        Self {
            end: parking_lot::Mutex::new(end),
        }
    }

    /// Call `f` with the end, unless another thread is using it (then `None`, without waiting).
    pub fn try_with<R>(&self, f: impl FnOnce(&mut E) -> R) -> Option<R> {
        self.end.try_lock().map(|mut end| f(&mut end))
    }

    /// The end, back.
    pub fn into_inner(self) -> E {
        self.end.into_inner()
    }
}

impl<E> std::fmt::Debug for Shared<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_with_lends_the_end_and_refuses_a_second_user() {
        let (tx, rx) = RingBuffer::<i32>::new(2);
        let (tx, rx) = (Shared::new(tx), Shared::new(rx));
        assert_eq!(tx.try_with(|t| t.push(1)), Some(Ok(())));
        assert_eq!(tx.try_with(|t| t.push(2)), Some(Ok(())));
        assert!(matches!(tx.try_with(|t| t.push(3)), Some(Err(PushError::Full(3)))));
        //in use: the nested call is refused, not blocked
        assert_eq!(rx.try_with(|r| (r.pop(), rx.try_with(|_| ()))), Some((Ok(1), None)));
        assert_eq!(rx.into_inner().pop(), Ok(2));
    }
}
