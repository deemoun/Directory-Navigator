//! Workspace-owned Bluetooth sessions; native links stay on their worker thread.
use super::*;
use crate::transfers::TransferId;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[derive(Clone)]
pub struct Transfer {
    pub(super) direction: Direction,
    pub(super) device: Device,
    pub(super) files: Vec<Source>,
    pub(super) name: String,
    pub(super) destination: PathBuf,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Send,
    Receive,
}
impl Transfer {
    pub(crate) fn direction(&self) -> Direction {
        self.direction
    }
    pub(crate) fn count(&self) -> usize {
        if self.direction == Direction::Send {
            self.files.len()
        } else {
            1
        }
    }
    pub(crate) fn summary(&self) -> String {
        let name = if self.direction == Direction::Send {
            self.files.first().map_or("", |f| f.name.as_str())
        } else {
            self.name.as_str()
        };
        let name = crate::filesystem::display_name(name);
        if self.files.len() > 1 {
            format!("{name} (+{})", self.files.len() - 1)
        } else {
            name
        }
    }
    pub(crate) fn destination_summary(&self) -> Option<String> {
        (self.direction == Direction::Send).then(|| {
            format!(
                "Bluetooth: {} ({})",
                crate::filesystem::display_name(&self.device.name),
                self.device.address
            )
        })
    }
    pub(crate) fn sources(&self) -> Vec<PathBuf> {
        self.files.iter().map(|f| f.path.clone()).collect()
    }
    pub(crate) fn destination(&self) -> PathBuf {
        self.destination.clone()
    }
    pub(crate) fn total(&self) -> Option<u64> {
        (self.direction == Direction::Send)
            .then(|| self.files.iter().map(|f| f.metadata.len()).sum())
    }
}
#[derive(Clone)]
pub(super) struct Flow {
    open: Arc<AtomicBool>,
    pub(super) approved: Arc<AtomicU64>,
    ready: Arc<AtomicBool>,
}
impl Default for Flow {
    fn default() -> Self {
        Self {
            open: Arc::new(AtomicBool::new(true)),
            approved: Arc::new(AtomicU64::new(0)),
            ready: Arc::new(AtomicBool::new(true)),
        }
    }
}
impl Flow {
    pub(super) fn receiver() -> Self {
        let f = Self::default();
        f.ready.store(false, Ordering::Relaxed);
        f
    }
    pub(super) fn allows(&self, id: u64) -> bool {
        self.open.load(Ordering::Acquire) || self.approved.load(Ordering::Acquire) == id
    }
    pub(super) fn open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }
    pub(super) fn ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}
pub(crate) struct Job {
    id: TransferId,
    pub(super) inner: super::Job,
    total: Option<u64>,
    all: std::cell::Cell<Option<crate::operations::conflict::Decision>>,
}
impl Job {
    pub(super) fn adopt(id: TransferId, inner: super::Job, total: Option<u64>) -> Self {
        Self {
            id,
            inner,
            total,
            all: std::cell::Cell::new(None),
        }
    }
    pub(crate) fn id(&self) -> TransferId {
        self.id
    }
    pub(crate) fn activate(&self) {
        self.inner.flow.ready.store(true, Ordering::Release);
    }
    pub(crate) fn cancel(&self) {
        self.inner.worker.cancel();
    }
    pub(crate) fn conflicts(&self) -> &crate::operations::conflict::Mailbox {
        &self.inner.conflicts
    }
    pub(crate) fn stop_offers_if_closed(&self, app: &App) {
        if self.inner.receiving
            && (!super::alive(app, self.inner.owner)
                || app.transfers.exit != crate::transfers::ExitMode::None)
        {
            self.inner.flow.open.store(false, Ordering::Release);
        }
    }
    pub(crate) fn approve(&self, id: u64, path: PathBuf) -> io::Result<()> {
        self.answer(id, true, path)
    }
    pub(crate) fn answer(&self, id: u64, accept: bool, path: PathBuf) -> io::Result<()> {
        self.inner.answer_offer(id, accept, path)
    }
    pub(crate) fn belongs_to(&self, owner: Binding) -> bool {
        self.inner.owner == owner
    }
    pub(crate) fn answer_conflict(&self, decision: crate::operations::conflict::Decision) {
        use crate::operations::conflict::Decision;
        if matches!(decision, Decision::ReplaceAll | Decision::SkipAll) {
            self.all.set(Some(decision));
        }
        if decision == Decision::Cancel {
            self.cancel();
        }
        self.inner.conflicts.answer(decision);
    }
    pub(crate) fn apply_conflict_policy(&self) {
        if let Some(decision) = self.all.get() {
            self.inner.conflicts.answer(decision);
        }
    }
    pub(crate) fn snapshot(&self) -> super::Progress {
        self.inner
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub(crate) fn progress(&self, p: &super::Progress) -> crate::operations::Progress {
        let mut progress = crate::operations::Progress {
            transfer: Some(self.id),
            files: p.completed as u64,
            skipped: p.skipped as u64,
            bytes: p.bytes,
            written: p.bytes,
            total_bytes: self.total,
            member: p.member.clone(),
            phase: p.operation,
            ..Default::default()
        };
        if !p.warning.is_empty() {
            progress.detail(PathBuf::new(), p.warning.clone());
        }
        progress
    }
    pub(crate) fn poll(&self) -> Result<io::Result<()>, mpsc::TryRecvError> {
        self.inner.worker.try_recv().map(|r| {
            r.and_then(|r| match r {
                Reply::Transferred => Ok(()),
                _ => Err(io::Error::other("Invalid Bluetooth transfer completion")),
            })
        })
    }
}
pub(crate) fn start_send(app: &mut App, id: TransferId, request: &Transfer) -> io::Result<Job> {
    let b = app
        .desktop
        .plugins
        .binding(BLUETOOTH_TRANSFERS)
        .unwrap_or(Binding {
            plugin: BLUETOOTH_TRANSFERS,
            owner: Owner::Document(crate::desktop::WindowId::MANAGER),
        });
    let files = request.files.clone();
    let device = request.device.clone();
    let inner = super::make_job(app, b, true, false, move |ctx| {
        ctx.waiting("Connecting Bluetooth device", 0);
        let mut link = platform::connect(&device.address, &ctx.cancel)?;
        engine::send(link.as_mut(), &files, &ctx)?;
        Ok(Reply::Transferred)
    })?;
    Ok(Job::adopt(id, inner, request.total()))
}
pub(crate) fn unconfirmed(error: &io::Error) -> bool {
    error.get_ref().is_some_and(|e| e.is::<Unconfirmed>())
}
#[derive(Debug)]
pub(super) struct Unconfirmed(pub io::Error);
impl std::fmt::Display for Unconfirmed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Delivery unconfirmed: the receiver may have published this file. {}",
            self.0
        )
    }
}
impl std::error::Error for Unconfirmed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}
