//! Bounded short-lived I/O tasks. Dropping a task cancels queued work and replies.
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc, Mutex, OnceLock,
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
struct Pool {
    send: SyncSender<Work>,
}
impl Pool {
    fn new() -> io::Result<Self> {
        let (send, receive) = mpsc::sync_channel::<Work>(16);
        let receive = Arc::new(Mutex::new(receive));
        for i in 0..4 {
            let receive = receive.clone();
            thread::Builder::new()
                .name(format!("dn-task-{i}"))
                .spawn(move || loop {
                    let work = match receive.lock() {
                        Ok(r) => r.recv(),
                        Err(_) => break,
                    };
                    let Ok(work) = work else {
                        break;
                    };
                    // A failed task disconnects its reply; other tasks remain serviceable.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
                })?;
        }
        Ok(Self { send })
    }
}
pub struct Task<T> {
    reply: Receiver<T>,
    cancel: Cancellation,
}
impl<T: Send + 'static> Task<T> {
    pub fn spawn(
        _name: &str,
        run: impl FnOnce(Cancellation) -> T + Send + 'static,
    ) -> io::Result<Self> {
        static POOL: OnceLock<Result<Pool, String>> = OnceLock::new();
        let pool = POOL
            .get_or_init(|| Pool::new().map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| io::Error::other(e.clone()))?;
        let (send, reply) = mpsc::sync_channel(1);
        let cancel = Cancellation::default();
        let token = cancel.clone();
        pool.send
            .try_send(Box::new(move || {
                if !token.cancelled() {
                    let value = run(token.clone());
                    if !token.cancelled() {
                        let _ = send.send(value);
                    }
                }
            }))
            .map_err(|_| {
                io::Error::new(io::ErrorKind::WouldBlock, "Background task queue is full")
            })?;
        Ok(Self { reply, cancel })
    }
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.reply.try_recv()
    }
}
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
