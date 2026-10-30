//! A port of the SDK's `advanced/simplethread`: a worker thread counts up every `sleeptime`
//! milliseconds and the count is output on the main thread.
//!
//! `std::thread` and a `Condvar` replace `systhread`: the worker waits on the condvar rather
//! than sleeping, so `cancel` (and freeing the object) stops it at once instead of after up to
//! `sleeptime` milliseconds.

use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicIsize, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use median::prelude::*;

const MIN_SLEEPTIME: t_atom_long = 10;

struct Shared {
    foo: AtomicIsize,
    sleeptime: AtomicIsize,
    cancel: Mutex<bool>,
    wake: Condvar,
    output: Queue,
}

#[derive(MaxObj)]
#[max(name = "median.simplethread")]
pub struct Simplethread {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
    out: Outlet<Any>,
}

impl MaxObjWrapped for Simplethread {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("bang starts a new thread"));
        Ok(Self {
            shared: Arc::new(Shared {
                foo: AtomicIsize::new(0),
                sleeptime: AtomicIsize::new(1000),
                cancel: Mutex::new(false),
                wake: Condvar::new(),
                output: builder.queue(Self::output),
            }),
            thread: Mutex::new(None),
            out: builder.outlet(Outlet::any().assist("report when done/cancelled")),
        })
    }
}

#[median::methods]
impl Simplethread {
    #[bang]
    pub fn bang(&self) {
        let mut thread = self.thread.lock().unwrap();
        stop(&self.shared, &mut thread);
        post!("starting a new thread");
        *self.shared.cancel.lock().unwrap() = false;
        let shared = self.shared.clone();
        *thread = Some(std::thread::spawn(move || run(&shared)));
    }

    #[sel("foo", defaults = 1)]
    pub fn foo(&self, value: t_atom_long) {
        self.shared.foo.store(value, Ordering::Relaxed);
    }

    #[sel("sleeptime", defaults = 1)]
    pub fn sleeptime(&self, ms: t_atom_long) {
        self.shared
            .sleeptime
            .store(ms.max(MIN_SLEEPTIME), Ordering::Relaxed);
    }

    #[sel("cancel")]
    pub fn cancel(&self) {
        stop(&self.shared, &mut self.thread.lock().unwrap());
        let _ = self.out.send(&sym!("cancelled"), &[]);
    }

    fn output(&self) {
        let _ = self.out.send_int(self.shared.foo.load(Ordering::Relaxed));
    }
}

impl Drop for Simplethread {
    fn drop(&mut self) {
        stop(&self.shared, self.thread.get_mut().unwrap());
    }
}

fn stop(shared: &Shared, thread: &mut Option<JoinHandle<()>>) {
    if let Some(handle) = thread.take() {
        post!("stopping our thread");
        *shared.cancel.lock().unwrap() = true;
        shared.wake.notify_all();
        let _ = handle.join();
    }
}

fn run(shared: &Shared) {
    let mut cancel = shared.cancel.lock().unwrap();
    while !*cancel {
        shared.foo.fetch_add(1, Ordering::Relaxed);
        shared.output.set();
        let ms = shared.sleeptime.load(Ordering::Relaxed) as u64;
        cancel = shared
            .wake
            .wait_timeout_while(cancel, Duration::from_millis(ms), |c| !*c)
            .unwrap()
            .0;
    }
}

median::ext_main! { Simplethread }

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use median_mock::{Atom, Kernel};

    // Drain the main queue until the object has output `n` more counts.
    fn counts(k: &Kernel, n: usize) -> Vec<i64> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut out = Vec::new();
        while out.len() < n {
            assert!(Instant::now() < deadline, "the worker stopped counting");
            k.run_main_queue();
            out.extend(k.take_outlet_log().into_iter().map(|e| match e.atoms[..] {
                [Atom::Long(v)] => v,
                _ => panic!("unexpected output {e:?}"),
            }));
            std::thread::sleep(Duration::from_millis(1));
        }
        out
    }

    #[test]
    fn counts_until_cancelled() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.simplethread", &[]);
        assert!(!x.is_null());
        k.send(x, "sleeptime", &[Atom::Long(1)]).unwrap();
        k.send(x, "foo", &[Atom::Long(100)]).unwrap();
        k.send_bang(x).unwrap();
        let first = counts(&k, 2);
        assert!(first[0] > 100 && first[1] > first[0], "{first:?}");

        k.send_bang(x).unwrap();
        counts(&k, 1);
        k.send(x, "foo", &[]).unwrap();
        let start = Instant::now();
        k.send(x, "cancel", &[]).unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        k.run_main_queue();
        let cancelled: Vec<_> = k
            .take_outlet_log()
            .into_iter()
            .filter(|e| e.selector == "cancelled")
            .collect();
        assert_eq!(cancelled.len(), 1);
        assert!(cancelled[0].atoms.is_empty());
        let console: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(
            console,
            [
                "starting a new thread",
                "stopping our thread",
                "starting a new thread",
                "stopping our thread",
            ]
        );

        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(k.run_main_queue(), 0);
        k.send(x, "cancel", &[]).unwrap();
        assert!(k.take_console_log().is_empty());
        assert!(k.free_object(x));
    }

    #[test]
    fn free_stops_the_thread_at_once() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.simplethread", &[]);
        k.send_bang(x).unwrap();
        counts(&k, 1);
        // The default sleeptime is a second; the worker is waiting on it.
        let start = Instant::now();
        assert!(k.free_object(x));
        assert!(start.elapsed() < Duration::from_millis(500));
        assert_eq!(k.live_qelem_count(), 0);
    }

    #[test]
    fn assists() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.simplethread", &[]);
        assert_eq!(
            k.assist(x, false, 0).as_deref(),
            Some("bang starts a new thread")
        );
        assert_eq!(
            k.assist(x, true, 0).as_deref(),
            Some("report when done/cancelled")
        );
        assert!(k.free_object(x));
    }
}
