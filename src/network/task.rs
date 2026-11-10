//! Bounded network workers, independent from the short local-I/O pool.
use crate::{
    requests::{RequestLease, RequestScope},
    task::Cancellation,
};
use std::{
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
};
static WORKERS: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}
pub struct Task<T> {
    receive: Receiver<T>,
    cancel: Cancellation,
    request: RequestLease,
}
impl<T: Send + 'static> Task<T> {
    pub fn spawn(
        scope: &RequestScope,
        slot: &'static str,
        run: impl FnOnce(Cancellation) -> T + Send + 'static,
    ) -> io::Result<Self> {
        WORKERS
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 8).then_some(n + 1)
            })
            .map_err(|_| {
                io::Error::other("Network workers are busy; cancel a pending request or try again")
            })?;
        let permit = Permit;
        let request = scope.lease(slot);
        let lease = request.clone();
        let cancel = Cancellation::default();
        let token = cancel.clone();
        let (send, receive) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name(slot.into())
            .spawn(move || {
                let _permit = permit;
                if lease.current() && !token.cancelled() {
                    let result = run(token.clone());
                    if lease.current() && !token.cancelled() {
                        let _ = send.send(result);
                    }
                }
            })?;
        Ok(Self {
            receive,
            cancel,
            request,
        })
    }
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        if !self.request.current() {
            return Err(TryRecvError::Disconnected);
        }
        let r = self.receive.try_recv()?;
        self.request.finish();
        Ok(r)
    }
}
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
