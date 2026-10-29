//! Bounded short-lived I/O tasks. Dropping a task cancels queued work and replies.
use std::{
    collections::VecDeque,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc, Condvar, Mutex, OnceLock, Weak,
    },
    thread,
};
#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    pub fn check(&self) -> io::Result<()> {
        if self.cancelled() {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Operation cancelled",
            ))
        } else {
            Ok(())
        }
    }
}
type Work = Box<dyn FnOnce() + Send>;
struct WorkItem {
    run: Work,
    request: crate::requests::RequestLease,
    cancel: Cancellation,
}
impl WorkItem {
    fn live(&self) -> bool {
        !self.cancel.cancelled() && self.request.current()
    }
}
#[derive(Default)]
struct Queue {
    work: VecDeque<WorkItem>,
    stopped: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}
struct Pool {
    shared: Arc<Shared>,
    capacity: usize,
}
enum Admission {
    Full(WorkItem),
    Stopped,
}
impl Pool {
    fn new() -> io::Result<Arc<Self>> {
        Self::with_limits(4, 16)
    }
    fn with_limits(workers: usize, capacity: usize) -> io::Result<Arc<Self>> {
        let pool = Arc::new(Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue::default()),
                ready: Condvar::new(),
            }),
            capacity,
        });
        for i in 0..workers {
            let shared = pool.shared.clone();
            thread::Builder::new()
                .name(format!("dn-task-{i}"))
                .spawn(move || loop {
                    let work = {
                        let Ok(mut queue) = shared.queue.lock() else {
                            break;
                        };
                        while queue.work.is_empty() && !queue.stopped {
                            let Ok(next) = shared.ready.wait(queue) else {
                                return;
                            };
                            queue = next;
                        }
                        if queue.stopped {
                            break;
                        }
                        queue.work.pop_front().expect("nonempty task queue")
                    };
                    // Neither application work nor its destructor runs under the queue lock.
                    if work.live() {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work.run));
                    }
                })?;
        }
        Ok(pool)
    }
    fn submit(&self, work: WorkItem) -> Result<(), Admission> {
        let Ok(mut queue) = self.shared.queue.lock() else {
            return Err(Admission::Stopped);
        };
        if queue.stopped {
            return Err(Admission::Stopped);
        }
        // Generations coalesce replaceable work before it consumes scarce slots.
        // Drop removed closures outside the lock: captured values may own tasks.
        let mut retired = Vec::new();
        let mut kept = VecDeque::new();
        while let Some(old) = queue.work.pop_front() {
            if old.live() {
                kept.push_back(old);
            } else {
                retired.push(old);
            }
        }
        queue.work = kept;
        let result = if queue.work.len() >= self.capacity {
            Err(Admission::Full(work))
        } else {
            queue.work.push_back(work);
            self.shared.ready.notify_one();
            Ok(())
        };
        drop(queue);
        drop(retired);
        result
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.stopped = true;
            self.shared.ready.notify_all();
        }
    }
}
pub struct Task<T> {
    reply: Receiver<T>,
    cancel: Cancellation,
    request: crate::requests::RequestLease,
    scope: Option<crate::requests::RequestScope>,
    pool: Option<Weak<Pool>>,
    deferred: Mutex<Option<WorkItem>>,
}
impl<T: Send + 'static> Task<T> {
    pub fn spawn(
        _name: &'static str,
        run: impl FnOnce(Cancellation) -> T + Send + 'static,
    ) -> io::Result<Self> {
        let scope = crate::requests::RequestScope::new(crate::requests::Owner::Workspace);
        let mut task = Self::spawn_owned(&scope, _name, run)?;
        task.scope = Some(scope);
        Ok(task)
    }
    pub(crate) fn spawn_owned(
        scope: &crate::requests::RequestScope,
        name: &'static str,
        run: impl FnOnce(Cancellation) -> T + Send + 'static,
    ) -> io::Result<Self> {
        static POOL: OnceLock<Result<Arc<Pool>, String>> = OnceLock::new();
        let pool = POOL
            .get_or_init(|| Pool::new().map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| io::Error::other(e.clone()))?
            .clone();
        Self::on_pool(scope, name, run, pool)
    }
    fn on_pool(
        scope: &crate::requests::RequestScope,
        name: &'static str,
        run: impl FnOnce(Cancellation) -> T + Send + 'static,
        pool: Arc<Pool>,
    ) -> io::Result<Self> {
        let request = scope.lease(name);
        let (send, reply) = mpsc::sync_channel(1);
        let cancel = Cancellation::default();
        let token = cancel.clone();
        let work = WorkItem {
            request: request.clone(),
            cancel: cancel.clone(),
            run: Box::new(move || {
                let value = run(token.clone());
                if !token.cancelled() {
                    let _ = send.send(value);
                }
            }),
        };
        let deferred = match pool.submit(work) {
            Ok(()) => None,
            Err(Admission::Full(work)) => Some(work),
            Err(Admission::Stopped) => {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Background task pool stopped",
                ))
            }
        };
        // A saturated pool never grows a global overflow queue. One deferred
        // request remains owned by this task, is cancelled on Drop, and retries
        // through the owner's existing bounded polling cycle.
        Ok(Self {
            reply,
            cancel,
            request,
            scope: None,
            pool: Some(Arc::downgrade(&pool)),
            deferred: Mutex::new(deferred),
        })
    }
    pub fn token(&self) -> Option<crate::requests::RequestToken> {
        self.request.token()
    }
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        if !self.request.current() {
            return Err(TryRecvError::Disconnected);
        }
        {
            let mut deferred = self
                .deferred
                .lock()
                .map_err(|_| TryRecvError::Disconnected)?;
            if let Some(work) = deferred.take() {
                match self
                    .pool
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .ok_or(TryRecvError::Disconnected)?
                    .submit(work)
                {
                    Ok(()) => {}
                    Err(Admission::Full(work)) => {
                        *deferred = Some(work);
                        return Err(TryRecvError::Empty);
                    }
                    Err(Admission::Stopped) => return Err(TryRecvError::Disconnected),
                }
            }
        }
        let value = self.reply.try_recv()?;
        self.request.finish();
        Ok(value)
    }
}
/// Poll a one-shot task without consuming a pending slot. Completed/disconnected
/// slots retire once; stale-owner/generation checks remain in Task::try_recv.
/// None means no reply yet (or no task); Some(Err) is always Disconnected.
pub fn take_reply<T: Send + 'static>(
    pending: &mut Option<Task<T>>,
) -> Option<Result<T, TryRecvError>> {
    match pending.as_ref()?.try_recv() {
        Err(TryRecvError::Empty) => None,
        reply => {
            *pending = None;
            Some(reply)
        }
    }
}
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::SyncSender;
    fn channel() -> (SyncSender<u32>, Option<Task<u32>>) {
        let (send, reply) = mpsc::sync_channel(1);
        let scope = crate::requests::RequestScope::new(crate::requests::Owner::Workspace);
        let request = scope.lease("test");
        (
            send,
            Some(Task {
                reply,
                cancel: Cancellation::default(),
                request,
                scope: Some(scope),
                pool: None,
                deferred: Mutex::new(None),
            }),
        )
    }
    #[test]
    fn one_shot_keeps_pending_and_consumes_completion_once() {
        let (send, mut task) = channel();
        assert_eq!(take_reply(&mut task), None);
        assert!(task.is_some());
        send.send(42).unwrap();
        assert_eq!(take_reply(&mut task), Some(Ok(42)));
        assert!(task.is_none());
        assert_eq!(take_reply(&mut task), None);
    }
    #[test]
    fn one_shot_disconnect_and_replaced_generation_retire_without_applying_reply() {
        let (send, mut task) = channel();
        drop(send);
        assert_eq!(take_reply(&mut task), Some(Err(TryRecvError::Disconnected)));
        assert!(task.is_none());
        let (send, mut task) = channel();
        send.send(42).unwrap();
        task.as_ref().unwrap().scope.as_ref().unwrap().start("test");
        assert_eq!(take_reply(&mut task), Some(Err(TryRecvError::Disconnected)));
        assert!(task.is_none());
    }
}

#[cfg(test)]
mod pressure_tests {
    use super::*;
    use crate::requests::{Owner, RequestScope};
    use std::time::{Duration, Instant};
    fn wait<T: Send + 'static>(task: &Task<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match task.try_recv() {
                Ok(v) => return v,
                Err(TryRecvError::Empty) => {
                    assert!(Instant::now() < deadline, "deferred task made no progress");
                    thread::sleep(Duration::from_millis(1));
                }
                Err(e) => panic!("task disconnected: {e}"),
            }
        }
    }
    fn blocked() -> (Arc<Pool>, RequestScope, Task<()>, mpsc::Sender<()>) {
        let pool = Pool::with_limits(1, 1).unwrap();
        let scope = RequestScope::new(Owner::Workspace);
        let (ready, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let task = Task::on_pool(
            &scope,
            "gate",
            move |_| {
                ready.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            },
            pool.clone(),
        )
        .unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        (pool, scope, task, release)
    }
    #[test]
    fn saturated_work_stays_with_its_owner_and_side_effects_run_once() {
        use std::io::Write;
        let (pool, scope, gate, release) = blocked();
        let first = Task::on_pool(&scope, "read", |_| 7, pool.clone()).unwrap();
        let root = std::env::temp_dir().join(format!("dn-task-pressure-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let cleanup = Cleanup(root.clone());
        let destination = root.join("save");
        let path = destination.clone();
        let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let writes = runs.clone();
        let save = Task::on_pool(
            &scope,
            "save",
            move |_| {
                writes.fetch_add(1, Ordering::SeqCst);
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)?;
                f.write_all(b"kept")
            },
            pool.clone(),
        )
        .unwrap();
        for _ in 0..10 {
            assert!(matches!(save.try_recv(), Err(TryRecvError::Empty)));
        }
        assert_eq!(pool.shared.queue.lock().unwrap().work.len(), 1);
        assert!(!destination.exists());
        assert!(save.token().is_some());
        release.send(()).unwrap();
        wait(&gate);
        assert_eq!(wait(&first), 7);
        wait(&save).unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(destination).unwrap(), b"kept");
        drop(cleanup);
    }
    #[test]
    fn superseded_and_cancelled_queued_reads_release_bounded_slots() {
        let (pool, scope, gate, release) = blocked();
        let ran = Arc::new(AtomicBool::new(false));
        let stale = ran.clone();
        let mut previous = Task::on_pool(
            &scope,
            "preview",
            move |_| {
                stale.store(true, Ordering::SeqCst);
                0
            },
            pool.clone(),
        )
        .unwrap();
        for n in 1..100 {
            let next = Task::on_pool(&scope, "preview", move |_| n, pool.clone()).unwrap();
            assert!(matches!(
                previous.try_recv(),
                Err(TryRecvError::Disconnected)
            ));
            assert!(next.deferred.lock().unwrap().is_none());
            assert_eq!(pool.shared.queue.lock().unwrap().work.len(), 1);
            previous = next;
        }
        drop(previous);
        let latest = Task::on_pool(&scope, "other", |_| 100, pool.clone()).unwrap();
        assert!(latest.deferred.lock().unwrap().is_none());
        release.send(()).unwrap();
        wait(&gate);
        assert_eq!(wait(&latest), 100);
        assert!(!ran.load(Ordering::SeqCst));
    }
    #[test]
    fn retired_and_dropped_deferred_owners_never_run() {
        let (pool, scope, gate, release) = blocked();
        let first = Task::on_pool(&scope, "read", |_| (), pool.clone()).unwrap();
        let owner = RequestScope::new(Owner::Panel(0));
        let ran = Arc::new(AtomicBool::new(false));
        let effect = ran.clone();
        let retired = Task::on_pool(
            &owner,
            "preview",
            move |_| effect.store(true, Ordering::SeqCst),
            pool.clone(),
        )
        .unwrap();
        assert!(retired.deferred.lock().unwrap().is_some());
        drop(owner);
        assert!(matches!(
            retired.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
        let effect = ran.clone();
        let cancelled = Task::on_pool(
            &scope,
            "cancel",
            move |_| effect.store(true, Ordering::SeqCst),
            pool.clone(),
        )
        .unwrap();
        drop(cancelled);
        release.send(()).unwrap();
        wait(&gate);
        wait(&first);
        assert!(!ran.load(Ordering::SeqCst));
    }
    #[test]
    fn panicking_work_disconnects_only_its_reply_and_worker_survives() {
        let pool = Pool::with_limits(1, 1).unwrap();
        let scope = RequestScope::new(Owner::Workspace);
        let failed = Task::on_pool(
            &scope,
            "failure",
            |_| panic!("injected worker failure"),
            pool.clone(),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while matches!(failed.try_recv(), Err(TryRecvError::Empty)) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        let next = Task::on_pool(&scope, "next", |_| 42, pool.clone()).unwrap();
        assert_eq!(wait(&next), 42);
    }
    #[test]
    fn unavailable_pool_reports_disconnection_instead_of_retrying_forever() {
        let pool = Pool::with_limits(0, 0).unwrap();
        let scope = RequestScope::new(Owner::Workspace);
        let task = Task::on_pool(&scope, "deferred", |_| 42, pool.clone()).unwrap();
        drop(pool);
        assert!(matches!(task.try_recv(), Err(TryRecvError::Disconnected)));
    }
}
