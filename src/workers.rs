//! Dedicated long-running workers. UI handles cancel/retire without joining.
//! The workspace retains bounded thread records until their owned cleanup ends.
use crate::{
    requests::{Owner, RequestLease, RequestScope},
    task::Cancellation,
};
use std::{
    io,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread::{self, JoinHandle},
};

const LIMIT: usize = 8;
struct Running {
    cancel: Cancellation,
    thread: JoinHandle<()>,
}
#[derive(Default)]
pub(crate) struct Supervisor {
    running: Vec<Running>,
}
impl Supervisor {
    pub fn spawn<T: Send + 'static>(
        &mut self,
        name: &str,
        scope: &RequestScope,
        owner: Owner,
        slot: &'static str,
        run: impl FnOnce(Cancellation) -> io::Result<T> + Send + 'static,
    ) -> io::Result<Worker<T>> {
        self.poll();
        if self.running.len() >= LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Background workers are still cleaning up; wait before starting another job",
            ));
        }
        scope.bind(owner);
        let request = scope.lease(slot);
        let live = request.clone();
        let cancel = Cancellation::default();
        let cancellation = cancel.clone();
        let (send, receive) = mpsc::sync_channel(1);
        let thread = match thread::Builder::new().name(name.into()).spawn(move || {
            if !live.current() {
                return;
            }
            let result = cancellation.check().and_then(|()| run(cancellation));
            // One bounded completion, including cancellation/publication warnings.
            // Retired replies and their values are destroyed on this worker.
            if live.current() {
                let _ = send.try_send(result);
            }
        }) {
            Ok(thread) => thread,
            Err(error) => {
                request.finish();
                return Err(error);
            }
        };
        self.running.push(Running {
            cancel: cancel.clone(),
            thread,
        });
        Ok(Worker {
            cancel,
            request,
            receive,
        })
    }
    pub fn poll(&mut self) -> bool {
        let previous = self.running.len();
        // Never join on the UI thread, even after is_finished. Dropping a Rust
        // JoinHandle releases it; the OS reclaims the finished thread. Retired
        // workers remain counted until this point, including unwinding/cleanup.
        self.running.retain(|job| !job.thread.is_finished());
        previous != self.running.len()
    }
    pub fn pending(&self) -> bool {
        !self.running.is_empty()
    }
    pub fn cancel_all(&self) {
        for job in &self.running {
            job.cancel.cancel();
        }
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.cancel_all();
    }
}
pub(crate) struct Worker<T> {
    cancel: Cancellation,
    request: RequestLease,
    receive: Receiver<io::Result<T>>,
}
impl<T> Worker<T> {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    #[cfg(test)]
    pub fn cancellation_flag(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.cancel.flag()
    }
    pub fn try_recv(&self) -> Result<io::Result<T>, TryRecvError> {
        if !self.request.current() {
            return Err(TryRecvError::Disconnected);
        }
        let result = self.receive.try_recv();
        if !matches!(result, Err(TryRecvError::Empty)) {
            self.request.finish();
        }
        result
    }
}
impl<T> Drop for Worker<T> {
    fn drop(&mut self) {
        self.cancel();
        self.request.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    };
    fn reply<T>(worker: &Worker<T>) -> Result<io::Result<T>, TryRecvError> {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            match worker.try_recv() {
                Err(TryRecvError::Empty) => {
                    assert!(Instant::now() < end);
                    thread::sleep(Duration::from_millis(1));
                }
                value => return value,
            }
        }
    }
    fn drain(supervisor: &mut Supervisor) {
        let end = Instant::now() + Duration::from_secs(5);
        while supervisor.pending() {
            supervisor.poll();
            assert!(Instant::now() < end, "owned worker cleanup did not finish");
            thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn retired_handles_do_not_wait_and_cleanup_keeps_its_admission_slot() -> io::Result<()> {
        let mut supervisor = Supervisor::default();
        let scope = RequestScope::default();
        let (started, ready) = mpsc::sync_channel(LIMIT);
        let (continued, observed) = mpsc::sync_channel(1);
        let mut releases = vec![];
        for _ in 0..LIMIT {
            let (release, gate) = mpsc::sync_channel(1);
            let started = started.clone();
            let worker = supervisor.spawn(
                "dn-worker-test",
                &scope,
                Owner::Workspace,
                "held",
                move |cancel| {
                    started.send(()).unwrap();
                    gate.recv_timeout(Duration::from_secs(5)).unwrap();
                    assert!(cancel.cancelled());
                    Ok(())
                },
            )?;
            ready.recv_timeout(Duration::from_secs(5)).unwrap();
            releases.push(release);
            // Dropping does not release the running-thread budget.
            drop(worker);
        }
        let watchdog = thread::spawn(move || {
            let result = observed.recv_timeout(Duration::from_secs(3));
            for release in releases {
                let _ = release.send(());
            }
            result
        });
        assert!(supervisor.pending());
        let rejected = supervisor.spawn(
            "dn-worker-overflow",
            &scope,
            Owner::Workspace,
            "extra",
            |_| Ok(()),
        );
        assert_eq!(rejected.err().unwrap().kind(), io::ErrorKind::WouldBlock);
        let _ = continued.send(());
        assert!(watchdog.join().unwrap().is_ok());
        drain(&mut supervisor);
        let next = supervisor.spawn("dn-worker-next", &scope, Owner::Workspace, "extra", |_| {
            Ok(7)
        })?;
        assert_eq!(reply(&next).unwrap()?, 7);
        drain(&mut supervisor);
        Ok(())
    }
    #[test]
    fn cancellation_keeps_the_final_outcome_and_stale_generations_are_rejected() -> io::Result<()> {
        let mut supervisor = Supervisor::default();
        let scope = RequestScope::default();
        let (started, ready) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let old = supervisor.spawn(
            "dn-worker-old",
            &scope,
            Owner::Workspace,
            "slot",
            move |_| {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(1)
            },
        )?;
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let new = supervisor.spawn("dn-worker-new", &scope, Owner::Workspace, "slot", |_| Ok(2))?;
        assert!(matches!(old.try_recv(), Err(TryRecvError::Disconnected)));
        release.send(()).unwrap();
        assert_eq!(reply(&new).unwrap()?, 2);
        let (started, ready) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let cancelled = supervisor.spawn(
            "dn-worker-cancel",
            &scope,
            Owner::Workspace,
            "slot",
            move |cancel| {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                cancel.check()?;
                Ok(0)
            },
        )?;
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        cancelled.cancel();
        release.send(()).unwrap();
        assert_eq!(
            reply(&cancelled).unwrap().unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        drain(&mut supervisor);
        Ok(())
    }
    #[test]
    fn dropping_supervisor_cancels_without_waiting_for_cleanup() -> io::Result<()> {
        let mut supervisor = Supervisor::default();
        let scope = RequestScope::default();
        let (started, ready) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let worker = supervisor.spawn(
            "dn-supervisor-drop",
            &scope,
            Owner::Workspace,
            "drop",
            move |cancel| {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                cancel.check()
            },
        )?;
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let flag = worker.cancellation_flag();
        drop(supervisor);
        assert!(flag.load(Ordering::Relaxed));
        release.send(()).unwrap();
        assert_eq!(
            reply(&worker).unwrap().unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        Ok(())
    }
    #[test]
    fn panic_disconnects_once_and_retired_results_are_cleaned_on_the_worker() -> io::Result<()> {
        let mut supervisor = Supervisor::default();
        let scope = RequestScope::default();
        let panicked =
            supervisor.spawn::<()>("dn-worker-panic", &scope, Owner::Workspace, "panic", |_| {
                panic!("intentional worker panic")
            })?;
        assert!(matches!(reply(&panicked), Err(TryRecvError::Disconnected)));
        assert!(matches!(
            panicked.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
        struct Cleanup(Arc<AtomicBool>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let cleaned = Arc::new(AtomicBool::new(false));
        let cleanup = Cleanup(cleaned.clone());
        let (started, ready) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let worker = supervisor.spawn(
            "dn-worker-cleanup",
            &scope,
            Owner::Workspace,
            "cleanup",
            move |_| {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(cleanup)
            },
        )?;
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(worker);
        release.send(()).unwrap();
        drain(&mut supervisor);
        assert!(cleaned.load(Ordering::Relaxed));
        Ok(())
    }
}
