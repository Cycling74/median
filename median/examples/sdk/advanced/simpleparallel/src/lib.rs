//! A port of the SDK's `advanced/simpleparallel`: on bang, one worker per core counts to
//! `iterations` in parallel, the counts are added to a total and the total is output.
//!
//! `std::thread::scope` with `available_parallelism` workers replaces `sysparallel`. The C
//! defines a setter that keeps `iterations` at least 1 but never registers it; the port
//! registers it.

use std::{
    sync::atomic::{AtomicBool, AtomicIsize, Ordering},
    thread,
    time::Instant,
};

use median::prelude::*;

#[derive(MaxObj)]
#[max(name = "median.simpleparallel")]
pub struct Simpleparallel {
    obj: ObjRef,
    foo: AtomicIsize,
    iterations: Int64,
    task_made: AtomicBool,
    out: Outlet<Int>,
}

impl MaxObjWrapped for Simpleparallel {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("bang spawns a parallel process"));
        Ok(Self {
            obj: builder.obj(),
            foo: AtomicIsize::new(0),
            iterations: Int64::new(5_000_000),
            task_made: AtomicBool::new(false),
            out: builder.outlet(Outlet::int().assist("report when done")),
        })
    }
}

#[median::methods]
impl Simpleparallel {
    #[bang]
    #[defer]
    pub fn bang(&self) {
        if !self.task_made.swap(true, Ordering::Relaxed) {
            post!("creating a new parallel task");
        }
        let workers = thread::available_parallelism().map_or(1, |n| n.get());
        thread::scope(|s| {
            for id in 0..workers {
                s.spawn(move || self.work(id, workers));
            }
        });
        let _ = self.out.send(self.foo.load(Ordering::Relaxed));
    }

    #[sel("foo", defaults = 1)]
    pub fn foo(&self, value: t_atom_long) {
        self.foo.store(value, Ordering::Relaxed);
    }

    #[attr]
    pub fn iterations(&self) -> t_atom_long {
        self.iterations.get()
    }

    pub fn set_iterations(&self, iterations: t_atom_long) {
        self.iterations.set(iterations.max(1));
    }

    fn work(&self, id: usize, workers: usize) {
        let start = Instant::now();
        self.obj.post(format!("worker {id} of {workers} started"));
        let mut count: t_atom_long = 0;
        for _ in 0..self.iterations.get() {
            count = std::hint::black_box(count) + 1;
        }
        self.foo.fetch_add(count, Ordering::Relaxed);
        let ms = start.elapsed().as_secs_f64() * 1000.;
        self.obj
            .post(format!("worker {id} of {workers} ended ({ms:.6}ms)"));
    }
}

median::ext_main! { Simpleparallel }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn adds_every_workers_count() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object(
            "median.simpleparallel",
            &[Atom::Sym("@iterations".into()), Atom::Long(10)],
        );
        assert!(!x.is_null());
        assert_eq!(k.attr_get(x, "iterations").unwrap(), [Atom::Long(10)]);
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get()) as i64;
        k.send_bang(x).unwrap();
        k.send(x, "foo", &[Atom::Long(5)]).unwrap();
        k.send_bang(x).unwrap();
        k.run_main_queue();
        let out: Vec<_> = k.take_outlet_log().into_iter().map(|e| e.atoms).collect();
        assert_eq!(
            out,
            [
                vec![Atom::Long(workers * 10)],
                vec![Atom::Long(5 + workers * 10)]
            ]
        );
        let console: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(console[0], "creating a new parallel task");
        assert_eq!(console.len() as i64, 1 + 4 * workers);
        let started = console.iter().filter(|t| t.ends_with("started")).count();
        assert_eq!(started as i64, 2 * workers);
        assert!(
            console
                .iter()
                .any(|t| *t == format!("worker 0 of {workers} started"))
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn iterations_are_at_least_one() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.simpleparallel", &[]);
        assert_eq!(
            k.attr_get(x, "iterations").unwrap(),
            [Atom::Long(5_000_000)]
        );
        k.attr_set(x, "iterations", &[Atom::Long(0)]).unwrap();
        assert_eq!(k.attr_get(x, "iterations").unwrap(), [Atom::Long(1)]);
        assert_eq!(
            k.assist(x, false, 0).as_deref(),
            Some("bang spawns a parallel process")
        );
        assert_eq!(k.assist(x, true, 0).as_deref(), Some("report when done"));
        assert!(k.free_object(x));
    }
}
