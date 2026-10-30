//! A port of the SDK's `advanced/threadpool`: `task <args>` runs a computation on a pool of
//! four threads shared by every object, and outputs `taskoutput <args>` when it is done.
//!
//! The SDK's `threadpooltask.c` (a sleeping-and-polling pool over `systhread` and `t_linklist`)
//! becomes `Pool`: a `VecDeque` and a `Condvar`. Each task carries its object's id, so `cancel`
//! and freeing the object drop that object's waiting tasks and wait for its running ones. The
//! completion posts on the worker and hands the arguments back to the main thread through a
//! `Queue`. The C stops the pool's threads at quit with `quittask_install`, which median does
//! not wrap; here they are left idle, waiting on the condvar, when Max quits. The completion
//! post names the worker by its Rust `ThreadId` rather than the `systhread` pointer. A test build
//! does a thousand iterations of the work instead of a hundred million.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Instant,
};

use median::{atom::AtomValue, prelude::*};

const THREAD_COUNT: usize = 4;
const ITERATIONS: u64 = if cfg!(test) { 1_000 } else { 100_000_000 };

struct Task {
    owner: u64,
    run: Box<dyn FnOnce() + Send>,
}

#[derive(Default)]
struct Pool {
    state: Mutex<PoolState>,
    work: Condvar,
    done: Condvar,
}

#[derive(Default)]
struct PoolState {
    waiting: VecDeque<Task>,
    // The owner of each task a worker is running.
    running: Vec<u64>,
}

impl Pool {
    fn start(threads: usize) -> &'static Pool {
        let pool: &'static Pool = Box::leak(Box::default());
        for _ in 0..threads {
            thread::spawn(|| pool.serve());
        }
        pool
    }

    fn serve(&self) {
        let mut state = self.state.lock().unwrap();
        loop {
            let Some(task) = state.waiting.pop_front() else {
                state = self.work.wait(state).unwrap();
                continue;
            };
            state.running.push(task.owner);
            drop(state);
            // A panicking task must still leave `running`, or purging its owner would wait forever.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task.run));
            state = self.state.lock().unwrap();
            let i = state.running.iter().position(|&o| o == task.owner);
            state
                .running
                .swap_remove(i.expect("a running task is listed"));
            self.done.notify_all();
        }
    }

    fn execute(&self, owner: u64, run: impl FnOnce() + Send + 'static) {
        let run = Box::new(run);
        self.state
            .lock()
            .unwrap()
            .waiting
            .push_back(Task { owner, run });
        self.work.notify_one();
    }

    /// Drop the waiting tasks of `owner` and wait for its running ones to finish.
    fn purge(&self, owner: u64) {
        let mut state = self.state.lock().unwrap();
        state.waiting.retain(|t| t.owner != owner);
        let _state = self
            .done
            .wait_while(state, |s| s.running.contains(&owner))
            .unwrap();
    }
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<&'static Pool> = OnceLock::new();
    POOL.get_or_init(|| Pool::start(THREAD_COUNT))
}

// `Atom` is not `Send`, so task arguments travel as these.
#[derive(Clone, Copy)]
enum Value {
    Int(t_atom_long),
    Float(f64),
    Sym(SymbolRef),
}

impl From<Value> for Atom {
    fn from(v: Value) -> Self {
        match v {
            Value::Int(i) => i.into(),
            Value::Float(f) => f.into(),
            Value::Sym(s) => s.into(),
        }
    }
}

struct Shared {
    completed: Mutex<Vec<Vec<Value>>>,
    output: Queue,
}

#[derive(MaxObj)]
#[max(name = "median.threadpool")]
pub struct Threadpool {
    id: u64,
    shared: Arc<Shared>,
    out: Outlet<Any>,
}

impl MaxObjWrapped for Threadpool {
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        pool();
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        builder.inlet(Inlet::default().assist("make a new task"));
        Ok(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            shared: Arc::new(Shared {
                completed: Mutex::new(Vec::new()),
                output: builder.queue(Self::output),
            }),
            out: builder.outlet(Outlet::any().assist("report when done/cancelled")),
        })
    }
}

#[median::methods]
impl Threadpool {
    #[gimme("task")]
    pub fn task(&self, atoms: &[Atom]) {
        let text = atoms.iter().map(Atom::text).collect::<Vec<_>>().join(" ");
        let args: Vec<Value> = atoms
            .iter()
            .filter_map(|a| match a.get_value()? {
                AtomValue::Int(i) => Some(Value::Int(i)),
                AtomValue::Float(f) => Some(Value::Float(f)),
                AtomValue::Symbol(s) => Some(Value::Sym(s)),
                AtomValue::Object(_) => None,
            })
            .collect();
        let shared = self.shared.clone();
        pool().execute(self.id, move || {
            work(&text);
            post!(
                "threadpool background task ({text}) completed in thread {:?}",
                thread::current().id()
            );
            post!("deferring output to the main thread");
            shared.completed.lock().unwrap().push(args);
            shared.output.set();
        });
    }

    #[sel("cancel")]
    pub fn cancel(&self) {
        pool().purge(self.id);
        let _ = self.out.send(&sym!("cancelled"), &[]);
    }

    fn output(&self) {
        let completed = std::mem::take(&mut *self.shared.completed.lock().unwrap());
        for args in completed {
            let atoms: Vec<Atom> = args.into_iter().map(Atom::from).collect();
            let _ = self.out.send(&sym!("taskoutput"), &atoms);
        }
    }
}

impl Drop for Threadpool {
    fn drop(&mut self) {
        pool().purge(self.id);
    }
}

fn work(text: &str) {
    post!("threadpool starting background task ({text})");
    let start = Instant::now();
    let mut f = 0.74_f64;
    for _ in 0..ITERATIONS {
        f = (f.sin() * f.sin() + f.cos() * f.cos()).sqrt();
    }
    let ms = start.elapsed().as_secs_f64() * 1000.;
    post!("threadpool finished background task ({text}) result={f:.6} time={ms:.6}");
}

median::ext_main! { Threadpool }

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc,
        },
        time::{Duration, Instant},
    };

    use median_mock::Atom;

    use super::Pool;

    #[test]
    fn outputs_each_task_on_the_main_thread() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.threadpool", &[]);
        assert!(!x.is_null());
        k.send(x, "task", &[Atom::Long(1), Atom::Sym("a".into())])
            .unwrap();
        k.send(x, "task", &[Atom::Float(2.5)]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut out = Vec::new();
        while out.len() < 2 {
            assert!(Instant::now() < deadline, "the tasks did not complete");
            k.run_main_queue();
            out.extend(k.take_outlet_log());
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(out.iter().all(|e| e.selector == "taskoutput"));
        let mut args: Vec<_> = out.into_iter().map(|e| e.atoms).collect();
        args.sort_by_key(|a| a.len());
        assert_eq!(
            args,
            [
                vec![Atom::Float(2.5)],
                vec![Atom::Long(1), Atom::Sym("a".into())]
            ]
        );
        let console: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(console.len(), 8);
        assert!(console.contains(&"threadpool starting background task (1 a)".to_string()));
        assert!(console.iter().any(|t| {
            t.starts_with("threadpool finished background task (2.5) result=1.000000 time=")
        }));
        assert!(k.free_object(x));
    }

    #[test]
    fn cancel_and_free_wait_for_running_tasks() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.threadpool", &[]);
        for i in 0..20 {
            k.send(x, "task", &[Atom::Long(i)]).unwrap();
        }
        k.send(x, "cancel", &[]).unwrap();
        let log = k.take_outlet_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].selector, "cancelled");
        // Nothing of the object's is waiting or running, so the posts are all in.
        let console = k.take_console_log();
        std::thread::sleep(Duration::from_millis(20));
        assert!(k.take_console_log().is_empty());
        assert_eq!(console.len() % 4, 0);
        // Tasks that completed before the cancel are still output.
        assert!(k.run_main_queue() <= 1);
        let outputs = k.take_outlet_log();
        assert!(outputs.iter().all(|e| e.selector == "taskoutput"));
        assert_eq!(outputs.len(), console.len() / 4);

        k.send(x, "task", &[Atom::Long(0)]).unwrap();
        assert!(k.free_object(x));
        k.take_console_log();
        std::thread::sleep(Duration::from_millis(20));
        assert!(k.take_console_log().is_empty());
        k.run_main_queue();
        assert!(k.take_outlet_log().is_empty());
    }

    #[test]
    fn assists() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.threadpool", &[]);
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("make a new task"));
        assert_eq!(
            k.assist(x, true, 0).as_deref(),
            Some("report when done/cancelled")
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn purge_drops_waiting_tasks_of_its_owner_only() {
        let pool = Pool::default();
        let ran = Arc::new(AtomicUsize::new(0));
        for owner in [1, 2, 1] {
            let ran = ran.clone();
            pool.execute(owner, move || {
                ran.fetch_add(1, Ordering::Relaxed);
            });
        }
        pool.purge(1);
        let waiting: Vec<_> = pool.state.lock().unwrap().waiting.drain(..).collect();
        assert_eq!(waiting.iter().map(|t| t.owner).collect::<Vec<_>>(), [2]);
        assert_eq!(ran.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn purge_waits_for_a_running_task() {
        let pool = Pool::start(1);
        let (started, wait_started) = mpsc::channel();
        let finished = Arc::new(AtomicBool::new(false));
        let f = finished.clone();
        pool.execute(7, move || {
            started.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(20));
            f.store(true, Ordering::Relaxed);
        });
        wait_started.recv().unwrap();
        pool.purge(7);
        assert!(finished.load(Ordering::Relaxed));
    }
}
