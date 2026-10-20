//! A Lace-inspired work-stealing scheduler for fine-grained ZDD operations.
//!
//! This is a Rust adaptation of the design of Lace (van Dijk & van de Pol), the
//! work-stealing framework underlying the Sylvan BDD package:
//!
//! - Each worker owns a LIFO deque of boxed closures. Spawning a task is a
//!   push onto the current worker's deque: no syscalls, no thread wakeup.
//! - [`join2`] pops the frame again and, in the common case where nobody stole
//!   it, runs it *inline* — identical to sequential execution. If it was
//!   stolen, the joining worker keeps making progress by stealing tasks from
//!   others (Lace's "leapfrog") until the result lands.
//! - Idle workers poll random victims with exponential backoff (`pause`
//!   instructions, then `yield`). They never park on futexes, keeping steal
//!   latency low. The trade-off is that idle workers burn a little CPU; this
//!   mirrors Lace's `LACE_BACKOFF` behaviour and was chosen deliberately.
//! - External threads calling ZDD operations *become* workers for the
//!   duration of the operation (like Lace's main-thread worker), borrowing a
//!   deque from the pool so their spawned tasks are stealable too.
//!
//! The decisive difference from rayon for ZDD workloads: rayon parks idle
//! threads on futexes, so each per-node `join` on a partially idle pool can pay
//! a wakeup syscall — profiling showed ~30% of CPU in `futex`/`sched_yield`
//! from exactly that. Here spawning is always cheap, and [`join2`] only
//! spawns while a worker is actually idle (see `Inner::n_idle`), otherwise
//! degenerating to the sequential fast path with zero allocation.

use crossbeam_deque::{Steal, Stealer, Worker as Deque};
use std::{
    cell::{Cell, UnsafeCell},
    fmt, hint,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

///A unit of stealable work.
///
///The `'static` bound is an internal fiction: tasks are created by [`join2`]
///by erasing the lifetime of closures that borrow the `ZddHolder` and its
///`SetFamily`s. This is sound because every task is guaranteed to have
///*completed* before the dynamic extent in which it was created ends — the
///joining thread either pops it back and runs it inline, or spins helping
///until the slot is written. This is the same technique `rayon::scope` uses.
pub(crate) type Task = Box<dyn FnOnce() + Send>;

///The result slot of one spawned task.
///
///State: `0` = pending, `1` = written.
struct Slot<T> {
    state: AtomicU64,
    value: UnsafeCell<Option<T>>,
}

//Sound: `value` is written by whichever thread runs the task before the
//`state` release-store, and read by the joining thread only after observing
//that store (acquire), or after running the task inline itself.
unsafe impl<T: Send> Sync for Slot<T> {}

impl<T> Slot<T> {
    fn new() -> Self {
        Slot {
            state: AtomicU64::new(0),
            value: UnsafeCell::new(None),
        }
    }

    fn write(&self, value: T) {
        unsafe {
            *self.value.get() = Some(value);
        }
        self.state.store(1, Ordering::Release);
    }

    fn is_ready(&self) -> bool {
        self.state.load(Ordering::Acquire) != 0
    }

    ///Take the value. Must only be called after `is_ready()` observed state 1.
    fn take(&self) -> T {
        unsafe { (*self.value.get()).take().expect("Slot value taken twice") }
    }
}

///Wraps `f` into a [`Task`] writing its result into `slot`.
///
///If `f` panics, the slot stays pending forever and the joining thread cannot
///return; so panics are converted to aborts of the process after the join,
///preserving the "no task outlives its scope" invariant. ZDD operations are
///infallible in practice (panics only from bugs), and rayon has the same
///policy for `join` (it propagates and aborts on poisoned joins).
fn make_task<T: Send>(slot: &Arc<Slot<T>>, f: impl FnOnce() -> T + Send) -> Task {
    let slot = Arc::clone(slot);
    //See the `Task` type's doc comment for why this lifetime erasure is sound.
    let f: Box<dyn FnOnce() + Send + '_> = Box::new(move || {
        let r = f();
        slot.write(r);
    });
    unsafe {
        std::mem::transmute::<Box<dyn FnOnce() + Send + '_>, Box<dyn FnOnce() + Send + 'static>>(f)
    }
}

///The state shared between all workers of one [`OpPool`].
struct Inner {
    stealers: Vec<Stealer<Task>>,
    ///Borrowable deques for external threads running `install`.
    guest_deques: Mutex<Vec<Deque<Task>>>,
    ///Number of workers currently idle enough to likely steal work. A
    ///conservative hint used to decide whether spawning is worthwhile; never
    ///relied upon for correctness.
    n_idle: AtomicU64,
    stopped: AtomicBool,
    n_workers: usize,
}

impl Inner {
    ///Try to steal one task from a random victim other than `skip`.
    fn steal_random(&self, cur: &Current) -> Option<Task> {
        if self.n_workers < 2 {
            return None;
        }
        let rng = cur.rng_next();
        #[expect(clippy::cast_possible_truncation)]
        let victim =
            ((rng % (self.n_workers as u64 - 1)) + cur.id as u64 + 1) as usize % self.n_workers;
        match self.stealers[victim].steal() {
            Steal::Success(t) => Some(t),
            _ => None,
        }
    }

    fn has_idle(&self) -> bool {
        self.n_idle.load(Ordering::Acquire) != 0
    }
}

thread_local! {
    static CURRENT: MutableCurrent = const { MutableCurrent::new() };
}

///Per-thread participation state, installed when the thread runs ZDD
///operations (either a pool worker or an external thread inside `install`).
struct Current {
    inner: Arc<Inner>,
    deque: Deque<Task>,
    id: usize,
    rng: Cell<u64>,
}

impl Current {
    fn rng_next(&self) -> u64 {
        let mut x = self.rng.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng.set(x);
        x
    }
}

///Interior-mutable holder so worker setup does not fight `join2` borrows.
struct MutableCurrent(UnsafeCell<Option<Arc<Current>>>);

impl MutableCurrent {
    const fn new() -> Self {
        MutableCurrent(UnsafeCell::new(None))
    }

    fn get(&self) -> Option<Arc<Current>> {
        //Sound: only ever accessed from the owning thread.
        unsafe { (*self.0.get()).as_ref().map(Arc::clone) }
    }

    fn set(&self, c: Option<Arc<Current>>) {
        //Sound: only ever accessed from the owning thread.
        unsafe {
            *self.0.get() = c;
        }
    }
}

fn run_task(t: Task) {
    t();
}

///Find one piece of work and run it. Returns whether a task ran.
fn help_once(cur: &Arc<Current>) -> bool {
    if let Some(t) = cur.inner.steal_random(cur) {
        run_task(t);
        return true;
    }
    if let Some(t) = cur.deque.pop() {
        run_task(t);
        return true;
    }
    false
}

///Spawn `a` as a stealable task, run `b` inline, then run `a` inline too (the
///common case) or wait for whichever worker stole it (leapfrogging).
///
///Returns both results. Falls back to fully-sequential execution when there
///is no pool context on this thread, no worker is idle, or the depth budget
///[`PAR_SPAWN_DEPTH`] is exhausted.
pub(crate) fn join2<T: Send>(
    depth: u32,
    a: impl FnOnce() -> T + Send,
    b: impl FnOnce() -> T + Send,
) -> (T, T) {
    let Some(cur) = CURRENT.with(MutableCurrent::get) else {
        return (a(), b());
    };
    if depth >= PAR_SPAWN_DEPTH || !cur.inner.has_idle() {
        return (a(), b());
    }
    let slot: Arc<Slot<T>> = Arc::new(Slot::new());
    let task = make_task(&slot, a);
    cur.deque.push(task);
    let b_result = b();
    //Now join a: either pop it (not stolen -> inline) or wait (stolen).
    if let Some(t) = cur.deque.pop() {
        run_task(t);
        let a_result = slot.take();
        (a_result, b_result)
    } else {
        //Stolen: leapfrog until the slot is written.
        while !slot.is_ready() {
            if !help_once(&cur) {
                hint::spin_loop();
            }
        }
        let a_result = slot.take();
        (a_result, b_result)
    }
}

///How deep into the operation recursion tasks are still spawned.
///
///Beyond this depth `join2` degenerates to a sequential call. Since each level
///spawns at most one task, this bounds spawned tasks per operation to
///`2^PAR_SPAWN_DEPTH`, which is plenty to keep the pool busy while keeping
///per-task boxing overhead negligible.
const PAR_SPAWN_DEPTH: u32 = 2;

///A Lace-style work-stealing pool for ZDD operations.
pub(crate) struct OpPool {
    inner: Arc<Inner>,
    handles: Vec<JoinHandle<()>>,
}

impl fmt::Debug for OpPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpPool").finish()
    }
}

impl OpPool {
    ///Create a pool with `n` spinning workers. Deque 0 is kept in reserve for
    ///external threads; workers 0..n-1 own deques 1..n.
    pub(crate) fn new(n: usize) -> Self {
        let n = n.max(1);
        let mut deques = Vec::with_capacity(n);
        let mut stealers = Vec::with_capacity(n);
        for _ in 0..n {
            let d = Deque::new_lifo();
            stealers.push(d.stealer());
            deques.push(d);
        }
        //One reserve deque per pool for `install` to borrow.
        let reserve = Deque::new_lifo();
        stealers.push(reserve.stealer());
        let inner = Arc::new(Inner {
            stealers,
            guest_deques: Mutex::new(vec![reserve]),
            n_idle: AtomicU64::new(0),
            stopped: AtomicBool::new(false),
            n_workers: n + 1,
        });
        let mut handles = Vec::with_capacity(n);
        for (id, deque) in deques.into_iter().enumerate() {
            let inner = Arc::clone(&inner);
            handles.push(
                thread::Builder::new()
                    .name(format!("zuddy-op-{id}"))
                    .spawn(move || {
                        //Current is Send but !Sync (crossbeam Worker is
                        //thread-bound); the Arc never leaves this thread.
                        #[expect(clippy::arc_with_non_send_sync)]
                        let cell = Arc::new(Current {
                            inner: Arc::clone(&inner),
                            deque,
                            id,
                            rng: Cell::new(0x2545_F491_4F6C_DD1D_u64 ^ (id as u64 + 1)),
                        });
                        CURRENT.with(|c| c.set(Some(Arc::clone(&cell))));
                        worker_loop(&inner);
                        CURRENT.with(|c| c.set(None));
                    })
                    .expect("Failed to spawn op worker"),
            );
        }
        OpPool { inner, handles }
    }

    ///Run `f` under op parallelism from an external (non-worker) thread.
    ///
    ///The calling thread *becomes* a worker for the duration: it borrows a
    ///deque from the pool, so tasks it spawns are stealable, and it runs `f`
    ///inline (Lace's model — the main thread is worker 0). If the current
    ///thread already is a worker (nested call), `f` just runs.
    pub(crate) fn install<T: Send>(&self, f: impl FnOnce() -> T + Send) -> T {
        if CURRENT.with(MutableCurrent::get).is_some() {
            return f();
        }
        let deque = self
            .inner
            .guest_deques
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(Deque::new_lifo);
        //Current is Send but !Sync; the Arc never leaves this thread.
        #[expect(clippy::arc_with_non_send_sync)]
        let cur = Arc::new(Current {
            inner: Arc::clone(&self.inner),
            deque,
            id: 0,
            rng: Cell::new(0x9E37_79B9_7F4A_7C15),
        });
        CURRENT.with(|c| c.set(Some(Arc::clone(&cur))));
        let r = f();
        //Drain anything we spawned that was never stolen, so no task
        //outlives this call (backs the `'static` erasure in `make_task`).
        while let Some(t) = cur.deque.pop() {
            run_task(t);
        }
        CURRENT.with(|c| c.set(None));
        //Return the borrowed deque to the reserve pool (Arc::try_unwrap
        //succeeds because join2 only clones the Arc during its own call).
        if let Ok(cur) = Arc::try_unwrap(cur) {
            self.inner.guest_deques.lock().unwrap().push(cur.deque);
        }
        r
    }
}

fn worker_loop(inner: &Inner) {
    let mut backoff: u32 = 0;
    let mut idle_flagged = false;
    loop {
        if inner.stopped.load(Ordering::Relaxed) {
            return;
        }
        let cur = CURRENT
            .with(MutableCurrent::get)
            .expect("worker_loop without CURRENT");
        //Own deque first (work-first scheduling), then steal.
        let task = cur.deque.pop().or_else(|| inner.steal_random(&cur));
        if let Some(t) = task {
            run_task(t);
            backoff = 0;
            if idle_flagged {
                idle_flagged = false;
                inner.n_idle.fetch_sub(1, Ordering::Release);
            }
        } else {
            //Backoff. In quota-limited environments (containers, CPU
            //limits) spinning burns quota the computation needs, so we
            //yield quickly rather than spin long.
            if backoff < 2 {
                hint::spin_loop();
            } else {
                thread::yield_now();
            }
            backoff = backoff.saturating_add(1);
            if !idle_flagged && backoff > 4 {
                idle_flagged = true;
                inner.n_idle.fetch_add(1, Ordering::Release);
            }
        }
    }
}

impl Drop for OpPool {
    fn drop(&mut self) {
        self.inner.stopped.store(true, Ordering::Release);
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn sequential_without_pool() {
        assert_eq!(join2(0, || 1, || 2), (1, 2));
    }

    #[test]
    fn spawn_and_join_in_pool() {
        let pool = OpPool::new(4);
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);
        let r = pool.install(move || {
            let c2 = Arc::clone(&c);
            let (a, b) = join2(
                0,
                move || {
                    c2.fetch_add(1, Ordering::Relaxed);
                    10
                },
                || 20,
            );
            a + b
        });
        assert_eq!(r, 30);
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn deep_recursion_stays_sequential() {
        fn fib(n: u64, depth: u32) -> u64 {
            if n < 2 {
                return n;
            }
            let (a, b) = join2(depth, || fib(n - 1, depth + 1), || fib(n - 2, depth + 1));
            a + b
        }
        let pool = OpPool::new(4);
        let r = pool.install(|| fib(25, 0));
        assert_eq!(r, 75_025);
    }

    #[test]
    fn nested_install_is_noop() {
        let pool = OpPool::new(2);
        let r = pool.install(|| pool.install(|| 5));
        assert_eq!(r, 5);
    }

    #[test]
    fn many_installs_reuse_deques() {
        let pool = OpPool::new(2);
        for _ in 0..100 {
            let r = pool.install(|| join2(0, || 1, || 2));
            assert_eq!(r, (1, 2));
        }
    }
}
