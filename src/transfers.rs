//! Workspace-owned transfer queue. The optional document is only a view; App.job
//! remains the active-worker adapter for existing operation callers.
mod view;
use crate::{
    app::App,
    operations::{self, CopyOptions, Event, Job, OperationFailure, Plan, Progress},
    requests::{Owner, RequestScope},
    task::Task,
};
use std::{
    collections::{BTreeSet, VecDeque},
    ffi::OsString,
    io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
pub(crate) use view::{key, mouse, quit_spec, spec};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransferId(u64);
impl std::fmt::Display for TransferId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Preparing,
    Queued,
    Running,
    Cancelling,
    Completed,
    Cancelled,
    Failed,
}
impl Status {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
    pub fn caption(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing",
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Cancelling => "Cancelling",
            Self::Completed => "Completed",
            Self::Cancelled => "Cancelled",
            Self::Failed => "Failed",
        }
    }
}
#[derive(Clone)]
struct Origin {
    owner: Owner,
    path: PathBuf,
    selected: BTreeSet<OsString>,
    cursor: Option<OsString>,
}
pub struct Entry {
    pub id: TransferId,
    pub source_count: usize,
    pub plan: Plan,
    pub options: CopyOptions,
    pub status: Status,
    pub progress: Progress,
    pub error: Option<String>,
    origin: Origin,
    prepared: Option<operations::transfer::Prepared>,
    started: Option<Instant>,
    ended: Option<Instant>,
}
impl Entry {
    pub fn source_summary(&self) -> String {
        let first = self
            .plan
            .sources
            .first()
            .map(|p| {
                crate::filesystem::display_name(
                    &p.file_name().unwrap_or_default().to_string_lossy(),
                )
            })
            .unwrap_or_default();
        if self.source_count > 1 {
            format!("{first} (+{})", self.source_count - 1)
        } else {
            first
        }
    }
    fn retire_payload(&mut self) {
        self.prepared = None;
        self.origin.selected.clear();
        // Finished history is a summary, not another retained full work plan.
        self.plan.sources.truncate(1);
        self.plan.sources.shrink_to_fit();
    }
    pub fn elapsed(&self, now: Instant) -> Duration {
        self.started.map_or(Duration::ZERO, |start| {
            self.ended.unwrap_or(now).saturating_duration_since(start)
        })
    }
    pub fn throughput(&self, now: Instant) -> Option<f64> {
        let seconds = self.elapsed(now).as_secs_f64();
        (self.progress.phase == operations::Phase::Copying
            && self.progress.written > 0
            && seconds >= 1.0
            && !self.status.terminal())
        .then(|| self.progress.written as f64 / seconds)
    }
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        let total = self.progress.total_bytes?;
        let rate = self.throughput(now)?;
        (total >= self.progress.bytes).then(|| {
            Duration::from_secs_f64(
                ((total - self.progress.bytes) as f64 / rate).min(u32::MAX as f64),
            )
        })
    }
    pub fn percent(&self) -> Option<u8> {
        if self.status == Status::Completed {
            return Some(100);
        }
        let total = self.progress.total_bytes?;
        if total == 0 || self.progress.bytes > total {
            return None;
        }
        Some(((u128::from(self.progress.bytes) * 100 / u128::from(total)).min(99)) as u8)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum ExitMode {
    #[default]
    None,
    Wait,
    Cancel,
}
#[derive(Default)]
pub struct State {
    pub entries: VecDeque<Entry>,
    pub paused: bool,
    pub selected: Option<TransferId>,
    pub(crate) focus: usize,
    pub(crate) offset: usize,
    pub(crate) rows: usize,
    pub(crate) now: Option<Instant>,
    pub(crate) active: Option<TransferId>,
    preparation: Option<(TransferId, Task<io::Result<operations::transfer::Prepared>>)>,
    requests: RequestScope,
    pub(crate) exit: ExitMode,
    pub(crate) quit_prompt: Option<crate::modals::ModalId>,
    pub(crate) quit_focus: usize,
}
impl State {
    pub fn has_work(&self) -> bool {
        self.entries.iter().any(|e| !e.status.terminal())
    }
    pub(crate) fn pending_io(&self) -> bool {
        self.active.is_some()
            || self.preparation.is_some()
            || self.entries.iter().any(|e| e.status == Status::Preparing)
            || (!self.paused && self.has_work())
    }
    pub fn selected(&self) -> Option<&Entry> {
        self.entries.iter().find(|e| Some(e.id) == self.selected)
    }
    pub fn counts(&self) -> (usize, usize) {
        (
            usize::from(self.active.is_some()),
            self.entries
                .iter()
                .filter(|e| !e.status.terminal() && Some(e.id) != self.active)
                .count(),
        )
    }
    pub fn summary(&self, language: crate::localization::Language) -> String {
        let (active, queued) = self.counts();
        format!(
            "{} {}/{} {}",
            language.text("Transfers"),
            active,
            queued,
            if self.paused {
                language.text("Paused")
            } else if self.entries.iter().any(|e| e.status == Status::Failed) {
                language.text("Error")
            } else {
                ""
            }
        )
    }
    fn trim(&mut self) {
        while self.entries.iter().filter(|e| e.status.terminal()).count() > 100 {
            if let Some(index) = self.entries.iter().position(|e| e.status.terminal()) {
                self.entries.remove(index);
            }
        }
        if self
            .selected
            .is_none_or(|id| !self.entries.iter().any(|e| e.id == id))
        {
            self.selected = self.entries.front().map(|e| e.id);
        }
        self.offset = self.offset.min(self.entries.len().saturating_sub(1));
        let disabled = match self.focus {
            1 => self
                .selected()
                .is_none_or(|e| e.status.terminal() || e.status == Status::Cancelling),
            2 => !self.paused,
            3 => !self.entries.iter().any(|e| e.status.terminal()),
            _ => false,
        };
        if disabled {
            self.focus = 0;
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Select(TransferId),
    Details,
    Cancel,
    Resume,
    Clear,
    Close,
    Help,
    QuitWait,
    QuitCancel,
    QuitReturn,
}
pub fn open(app: &mut App) {
    crate::tool_windows::open(app, crate::tool_windows::Kind::Transfers, None);
}
pub fn submit(app: &mut App, plan: Plan, options: CopyOptions) -> Result<TransferId, String> {
    if !matches!(plan.kind, operations::Kind::Copy | operations::Kind::Move) {
        return Err("Only Copy and Move belong to the Transfer Center".into());
    }
    app.transfers.now = Some(Instant::now());
    if app.transfers.exit != ExitMode::None || app.exit_pending {
        return Err("Cannot queue transfers while quitting".into());
    }
    if app.job.as_ref().is_some_and(|j| j.transfer.is_none()) {
        return Err("Another file operation is running".into());
    }
    if app
        .transfers
        .entries
        .iter()
        .filter(|e| !e.status.terminal())
        .count()
        >= 64
    {
        return Err("Transfer queue is full (64 requests)".into());
    }
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let value = NEXT
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
        .map_err(|_| "Transfer identities exhausted")?;
    let id = TransferId(value);
    let panel = &app.panels[app.active];
    let origin = Origin {
        owner: panel.scope.owner(),
        path: panel.path.clone(),
        selected: panel.selected.clone(),
        cursor: panel.entries.get(panel.cursor).map(|e| e.name.clone()),
    };
    let progress = Progress {
        transfer: Some(id),
        phase: operations::Phase::Preparing,
        path: plan.sources.first().cloned().unwrap_or_default(),
        ..Default::default()
    };
    app.transfers.entries.push_back(Entry {
        id,
        source_count: plan.sources.len(),
        plan,
        options,
        status: Status::Preparing,
        progress,
        error: None,
        origin,
        prepared: None,
        started: None,
        ended: None,
    });
    app.transfers.selected = Some(id);
    app.transfers.trim();
    start_next(app, Instant::now());
    open(app);
    Ok(id)
}
fn fail(app: &mut App, id: TransferId, error: String, now: Instant) {
    if let Some(e) = app
        .transfers
        .entries
        .iter_mut()
        .find(|e| e.id == id && !e.status.terminal())
    {
        e.status = Status::Failed;
        e.error = Some(error.clone());
        e.ended = Some(now);
        e.progress.detail(e.progress.path.clone(), error);
        e.retire_payload();
        app.transfers.paused = true;
    }
    if app.transfers.exit != ExitMode::None || app.quit_review.is_some() {
        app.abort_quit();
    }
}
fn start_next(app: &mut App, now: Instant) {
    if app.job.is_some() || app.transfers.paused {
        return;
    }
    let Some(index) = app
        .transfers
        .entries
        .iter()
        .position(|e| !e.status.terminal())
    else {
        return;
    };
    let e = &mut app.transfers.entries[index];
    if e.status == Status::Preparing
        && (app
            .transfers
            .preparation
            .as_ref()
            .is_some_and(|(id, _)| *id == e.id)
            || app.transfers.active.is_some())
    {
        return;
    }
    // The first request prepares in its dedicated worker. Queued requests must
    // retain their earlier asynchronous identity snapshot before execution.
    let id = e.id;
    let result = Job::start_transfer(id, e.plan.clone(), e.options, e.prepared.take());
    match result {
        Ok(job) => {
            e.status = Status::Preparing;
            e.started = Some(now);
            app.transfers.active = Some(id);
            app.job = Some(job);
        }
        Err(error) => fail(app, id, error.to_string(), now),
    }
}
fn prepare_queued(app: &mut App, now: Instant) -> bool {
    let mut changed = false;
    if let Some((id, task)) = &app.transfers.preparation {
        let result = match task.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(io::Error::other("Transfer preparation worker stopped"))),
        };
        if let Some(result) = result {
            changed = true;
            let id = *id;
            app.transfers.preparation = None;
            match result {
                Ok(prepared) => {
                    if let Some(e) = app
                        .transfers
                        .entries
                        .iter_mut()
                        .find(|e| e.id == id && e.status == Status::Preparing)
                    {
                        e.prepared = Some(prepared);
                        e.status = Status::Queued;
                    }
                }
                Err(error) => fail(app, id, error.to_string(), now),
            }
        }
    }
    if app.transfers.preparation.is_none() {
        if let Some(e) = app
            .transfers
            .entries
            .iter_mut()
            .find(|e| e.status == Status::Preparing && Some(e.id) != app.transfers.active)
        {
            changed = true;
            let id = e.id;
            let plan = e.plan.clone();
            match app
                .transfers
                .requests
                .spawn("transfer-preparation", move |cancel| {
                    cancel.check()?;
                    let prepared = operations::transfer::Prepared::capture(&plan)?;
                    cancel.check()?;
                    Ok(prepared)
                }) {
                Ok(task) => app.transfers.preparation = Some((id, task)),
                Err(error) => fail(app, id, error.to_string(), now),
            }
        }
    }
    changed
}
pub(crate) fn poll(app: &mut App, now: Instant) -> bool {
    app.transfers.now = Some(now);
    let mut changed = prepare_queued(app, now);
    if let Some(id) = app.transfers.active {
        let mut result = None;
        if let Some(job) = app.job.as_mut().filter(|j| j.transfer == Some(id)) {
            loop {
                match job.try_recv() {
                    Ok(Event::Progress(p)) => {
                        if p.transfer == Some(id) {
                            job.progress = p.clone();
                            if let Some(e) = app.transfers.entries.iter_mut().find(|e| e.id == id) {
                                if e.status != Status::Cancelling {
                                    e.status = if matches!(
                                        p.phase,
                                        operations::Phase::Preparing
                                            | operations::Phase::Calculating
                                    ) {
                                        Status::Preparing
                                    } else {
                                        Status::Running
                                    };
                                }
                                e.progress = p;
                            }
                            changed = true;
                        }
                    }
                    Ok(Event::Done(r)) => {
                        let owner = match &r {
                            Ok(p) => p.transfer,
                            Err(e) => e.progress.transfer,
                        };
                        result = Some(if owner == Some(id) {
                            r
                        } else {
                            Err(OperationFailure {
                                kind: io::ErrorKind::InvalidData,
                                progress: Box::new(job.progress.clone()),
                                message: "Transfer completion identity mismatch".into(),
                            })
                        });
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(_) => {
                        result=Some(Err(OperationFailure {kind:io::ErrorKind::BrokenPipe, progress:Box::new(job.progress.clone()), message:"File operation worker stopped unexpectedly; check partial destinations.".into()}));
                        break;
                    }
                }
            }
        }
        if let Some(result) = result {
            let cancel_requested = app
                .transfers
                .entries
                .iter()
                .find(|e| e.id == id)
                .is_some_and(|e| e.status == Status::Cancelling);
            app.last_operation = Some(result.clone());
            app.job = None;
            app.transfers.active = None;
            if let Some(e) = app.transfers.entries.iter_mut().find(|e| e.id == id) {
                e.ended = Some(now);
                match result {
                    Ok(p) => {
                        e.progress = p;
                        e.status = Status::Completed;
                    }
                    Err(error) => {
                        e.status = if error.kind == io::ErrorKind::Interrupted {
                            Status::Cancelled
                        } else {
                            Status::Failed
                        };
                        e.error = Some(error.message.clone());
                        e.progress = *error.progress;
                        e.progress.detail(e.progress.path.clone(), error.message);
                    }
                }
                if e.status != Status::Completed || cancel_requested {
                    app.transfers.paused = true;
                }
                let success = e.status == Status::Completed;
                let names = e
                    .plan
                    .sources
                    .iter()
                    .filter_map(|p| p.file_name().map(OsString::from))
                    .collect::<BTreeSet<_>>();
                for panel in &mut app.panels {
                    if success
                        && panel.scope.owner() == e.origin.owner
                        && panel.path == e.origin.path
                        && panel.selected == e.origin.selected
                        && panel.entries.get(panel.cursor).map(|v| v.name.clone())
                            == e.origin.cursor
                    {
                        panel.selected.retain(|name| !names.contains(name));
                    }
                    if e.plan.sources.iter().any(|p| {
                        p.parent() == Some(panel.path.as_path()) || panel.path.starts_with(p)
                    }) || e.plan.destination == panel.path
                        || e.plan.destination.parent() == Some(panel.path.as_path())
                    {
                        panel.refresh(app.config.show_hidden);
                        panel.space.refresh();
                    }
                }
                e.retire_payload();
                if e.status != Status::Completed
                    && !(app.transfers.exit == ExitMode::Cancel && e.status == Status::Cancelled)
                    && (app.transfers.exit != ExitMode::None || app.quit_review.is_some())
                {
                    app.abort_quit();
                }
            }
            app.transfers.trim();
            changed = true;
        }
    }
    if app.transfers.exit == ExitMode::Cancel {
        app.transfers.paused = false;
    }
    start_next(app, now);
    if app.transfers.active.is_some() {
        changed = true;
    }
    if app.transfers.exit != ExitMode::None && !app.transfers.has_work() {
        app.exit_pending = false;
        app.request_exit();
        changed = true;
    }
    app.transfers.trim();
    changed
}
pub fn action(app: &mut App, action: Action) {
    let now = Instant::now();
    match action {
        Action::Open => open(app),
        Action::Select(id) => {
            if app.transfers.entries.iter().any(|e| e.id == id) {
                app.transfers.selected = Some(id);
                app.transfers.focus = 0;
            }
        }
        Action::Cancel => cancel_selected(app, now),
        Action::Resume => {
            app.transfers.paused = false;
            start_next(app, now);
        }
        Action::Clear => app.transfers.entries.retain(|e| !e.status.terminal()),
        Action::Close => {
            let id = app.desktop.target();
            if app
                .desktop
                .tool(id)
                .is_some_and(|t| t.kind == crate::tool_windows::Kind::Transfers)
            {
                app.desktop.close(id);
                app.sync_modals();
            }
        }
        Action::Help => app.push_dialog(crate::dialog::Dialog {
            title: " Transfer Center Help ",
            message: app.config.language.text(HELP).into(),
        }),
        Action::Details => show_details(app),
        Action::QuitWait | Action::QuitCancel | Action::QuitReturn => {
            begin_exit(app, action, now);
        }
    }
    app.transfers.trim();
}
const HELP: &str = "Copy and Move run one at a time. Closing this window keeps transfers running. Cancel stops the selected request; completed items remain. Failure or cancellation pauses the queue. Resume queue starts waiting requests, without retrying failed work. Totals and remaining time are estimates. Existing destinations are never overwritten.";
fn cancel_selected(app: &mut App, now: Instant) {
    let Some(id) = app.transfers.selected else {
        return;
    };
    if app.transfers.active == Some(id) {
        if let Some(job) = &app.job {
            job.cancel();
        }
        if let Some(e) = app.transfers.entries.iter_mut().find(|e| e.id == id) {
            e.status = Status::Cancelling;
        }
    } else if let Some(e) = app
        .transfers
        .entries
        .iter_mut()
        .find(|e| e.id == id && !e.status.terminal())
    {
        e.status = Status::Cancelled;
        e.ended = Some(now);
        e.retire_payload();
        app.transfers.paused = true;
        if app
            .transfers
            .preparation
            .as_ref()
            .is_some_and(|(pending, _)| *pending == id)
        {
            app.transfers.preparation = None;
        }
    }
}
fn begin_exit(app: &mut App, action: Action, now: Instant) {
    let Some(id) = app.transfers.quit_prompt.take() else {
        return;
    };
    app.modals.remove_message(id);
    if action == Action::QuitReturn {
        app.abort_quit();
        return;
    }
    app.transfers.exit = if action == Action::QuitWait {
        ExitMode::Wait
    } else {
        ExitMode::Cancel
    };
    app.transfers.paused = false;
    if action == Action::QuitCancel {
        app.transfers.preparation = None;
        for e in &mut app.transfers.entries {
            if !e.status.terminal() {
                if Some(e.id) == app.transfers.active {
                    e.status = Status::Cancelling;
                } else {
                    e.status = Status::Cancelled;
                    e.ended = Some(now);
                    e.retire_payload();
                }
            }
        }
        if let Some(job) = &app.job {
            job.cancel();
        }
    }
    app.exit_pending = true;
    poll(app, now);
}
fn show_details(app: &mut App) {
    let Some(e) = app.transfers.selected() else {
        return;
    };
    let lang = app.config.language;
    let mut text = format!(
        "#{} · {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n",
        e.id,
        lang.text(e.status.caption()),
        lang.text("Source"),
        e.plan
            .sources
            .first()
            .map(|p| format!(
                "{} [{}]",
                crate::filesystem::display_name(&p.to_string_lossy()),
                e.source_count
            ))
            .unwrap_or_default(),
        lang.text("Destination"),
        crate::filesystem::display_name(&e.plan.destination.to_string_lossy()),
        lang.text("Items"),
        e.progress.files,
        lang.text("Skipped"),
        e.progress.skipped
    );
    if let Some(error) = &e.error {
        text.push_str(&lang.message(error));
        text.push('\n');
    }
    for detail in &e.progress.details {
        text.push_str(&format!(
            "{}: {}\n",
            crate::filesystem::display_name(&detail.path.to_string_lossy()),
            lang.message(&detail.message)
        ));
    }
    if e.progress.omitted_details > 0 {
        text.push_str(&format!(
            "{}: {}\n",
            lang.text("Additional detail records omitted"),
            e.progress.omitted_details
        ));
    }
    text.push_str(lang.text("Completed items are retained. Partial destinations may remain."));
    app.push_dialog(crate::dialog::Dialog {
        title: " Transfer details Help ",
        message: text,
    });
}
pub(crate) fn quit_dialog(app: &App) -> bool {
    app.transfers
        .quit_prompt
        .is_some_and(|id| app.modal_top().is_some_and(|m| m.id == id))
}
pub(crate) fn request_quit(app: &mut App) {
    if app.transfers.exit != ExitMode::None {
        app.exit_pending = true;
        return;
    }
    if app.transfers.quit_prompt.is_some() {
        return;
    }
    app.transfers.quit_focus = 2;
    app.push_owned_dialog(
        crate::modals::ModalOwner::Workspace,
        crate::dialog::Dialog {
            title: " Transfers pending ",
            message: "Transfers are still pending.\nChoose how to finish this session.\n\n\n\n"
                .into(),
        },
    );
    app.transfers.quit_prompt = app.modal_top().map(|m| m.id);
}
pub(crate) fn indicator_bounds(
    app: &App,
    area: crate::presentation::layout::Rect,
) -> Option<crate::presentation::layout::Rect> {
    (!app.transfers.entries.is_empty()).then(|| {
        crate::presentation::layout::Rect::new(
            area.right() - 26.min(area.width / 2),
            area.y,
            26.min(area.width / 2),
            1,
        )
    })
}
pub(crate) fn command_area(
    app: &App,
    mut area: crate::presentation::layout::Rect,
) -> crate::presentation::layout::Rect {
    if let Some(indicator) = indicator_bounds(app, area) {
        area.width = area.width.saturating_sub(indicator.width + 1);
    }
    area
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> (Temp, App, TransferId, TransferId) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dn-transfer-unit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("source"), "original").unwrap();
        let mut app = App::new(root.clone()).unwrap();
        app.config.save_on_exit = false;
        app.config_store = crate::config::Store::new(Some(root.join("config.conf")));
        app.transfers.paused = true;
        let make = |dest: &str| Plan {
            kind: operations::Kind::Copy,
            sources: vec![root.join("source")],
            destination: root.join(dest),
        };
        let first = submit(&mut app, make("first"), CopyOptions::default()).unwrap();
        let second = submit(&mut app, make("second"), CopyOptions::default()).unwrap();
        for e in &mut app.transfers.entries {
            e.status = Status::Queued;
            e.prepared = Some(operations::transfer::Prepared::capture(&e.plan).unwrap());
        }
        (Temp(root), app, first, second)
    }
    fn controlled(app: &mut App, id: TransferId) -> std::sync::mpsc::SyncSender<Event> {
        let (job, sender) = Job::controlled_transfer(id);
        app.job = Some(job);
        app.transfers.active = Some(id);
        app.transfers.paused = false;
        let e = app
            .transfers
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .unwrap();
        e.status = Status::Running;
        e.started = Some(Instant::now());
        sender
    }
    #[test]
    fn cancelling_a_closed_view_waits_for_worker_ack_and_pauses_exactly_once() {
        let (_temp, mut app, id, second) = fixture();
        let sender = controlled(&mut app, id);
        app.transfers.selected = Some(id);
        action(&mut app, Action::Close);
        action(&mut app, Action::Cancel);
        assert!(app.job.as_ref().unwrap().cancel.load(Ordering::Relaxed));
        let now = Instant::now();
        poll(&mut app, now + Duration::from_secs(30));
        assert!(app.job.is_some());
        assert_eq!(app.transfers.entries[0].status, Status::Cancelling);
        sender
            .send(Event::Done(Err(OperationFailure {
                kind: io::ErrorKind::Interrupted,
                progress: Box::new(Progress {
                    transfer: Some(id),
                    files: 2,
                    ..Default::default()
                }),
                message: "Operation cancelled".into(),
            })))
            .unwrap();
        poll(&mut app, now + Duration::from_secs(31));
        assert!(app.job.is_none());
        assert!(app.transfers.paused);
        assert_eq!(app.transfers.entries[0].status, Status::Cancelled);
        assert_eq!(app.transfers.entries[0].progress.files, 2);
        assert_eq!(app.transfers.entries[1].id, second);
        assert_eq!(app.transfers.entries[1].status, Status::Queued);
        let ended = app.transfers.entries[0].ended;
        poll(&mut app, now + Duration::from_secs(32));
        assert_eq!(app.transfers.entries[0].ended, ended);
    }
    #[test]
    fn disconnection_and_wrong_completion_identity_fail_without_advancing_queue() {
        for mismatch in [false, true] {
            let (_temp, mut app, id, other) = fixture();
            let sender = controlled(&mut app, id);
            if mismatch {
                sender
                    .send(Event::Done(Ok(Progress {
                        transfer: Some(other),
                        files: 999,
                        ..Default::default()
                    })))
                    .unwrap();
            }
            drop(sender);
            poll(&mut app, Instant::now());
            assert_eq!(app.transfers.entries[0].status, Status::Failed);
            assert_ne!(app.transfers.entries[0].progress.files, 999);
            assert!(app.transfers.paused);
            assert!(app.job.is_none());
            assert_eq!(app.transfers.entries[1].status, Status::Queued);
        }
    }
    #[test]
    fn quit_cancel_waits_for_ack_and_wait_failure_aborts_exit() {
        for cancel in [false, true] {
            let (_temp, mut app, id, _) = fixture();
            let sender = controlled(&mut app, id);
            app.request_exit();
            assert!(quit_dialog(&app));
            action(
                &mut app,
                if cancel {
                    Action::QuitCancel
                } else {
                    Action::QuitWait
                },
            );
            assert!(!app.quit);
            assert!(app.exit_pending);
            sender
                .send(Event::Done(Err(OperationFailure {
                    kind: if cancel {
                        io::ErrorKind::Interrupted
                    } else {
                        io::ErrorKind::PermissionDenied
                    },
                    progress: Box::new(Progress {
                        transfer: Some(id),
                        ..Default::default()
                    }),
                    message: "injected failure".into(),
                })))
                .unwrap();
            poll(&mut app, Instant::now());
            assert_eq!(app.quit, cancel);
            if !cancel {
                assert!(!app.exit_pending);
                assert!(app.quit_review.is_none());
                assert!(app.transfers.paused);
                assert_eq!(app.transfers.entries[1].status, Status::Queued);
            }
        }
    }
    #[test]
    fn failure_during_quit_choice_retires_only_its_owned_modal_tree() {
        for nested_help in [false, true] {
            let (_temp, mut app, id, _) = fixture();
            let sender = controlled(&mut app, id);
            app.request_exit();
            let quit = app.transfers.quit_prompt.unwrap();
            if nested_help {
                app.push_dialog(crate::dialog::Dialog {
                    title: " Transfer quit Help ",
                    message: "Owned child".into(),
                });
            }
            sender
                .send(Event::Done(Err(OperationFailure {
                    kind: io::ErrorKind::PermissionDenied,
                    progress: Box::new(Progress {
                        transfer: Some(id),
                        ..Default::default()
                    }),
                    message: "injected failure before quit choice".into(),
                })))
                .unwrap();
            poll(&mut app, Instant::now());
            app.sync_modals();
            assert!(!app.quit);
            assert!(app.quit_review.is_none());
            assert!(app.transfers.quit_prompt.is_none());
            assert!(!app
                .modal_infos()
                .iter()
                .any(|m| m.id == quit || m.parent == Some(quit)));
            assert!(app.modal_infos().is_empty());
            assert!(app.transfers.paused);
            assert_eq!(app.transfers.entries[0].status, Status::Failed);
        }
    }
    #[test]
    fn throughput_eta_percent_and_history_limits_use_explicit_time() {
        let (_temp, mut app, _, _) = fixture();
        let base = Instant::now();
        let e = &mut app.transfers.entries[0];
        e.status = Status::Running;
        e.started = Some(base);
        e.progress.written = 1024;
        e.progress.phase = operations::Phase::Copying;
        e.progress.bytes = 1024;
        e.progress.total_bytes = Some(4096);
        assert_eq!(e.throughput(base + Duration::from_secs(2)), Some(512.0));
        assert_eq!(
            e.remaining(base + Duration::from_secs(2)),
            Some(Duration::from_secs(6))
        );
        assert_eq!(e.percent(), Some(25));
        e.progress.phase = operations::Phase::Verifying;
        assert!(e.throughput(base + Duration::from_secs(2)).is_none());
        assert!(e.remaining(base + Duration::from_secs(2)).is_none());
        e.progress.phase = operations::Phase::Renaming;
        e.progress.bytes = 4096;
        e.progress.written = 0;
        assert_eq!(e.percent(), Some(99));
        assert!(e.throughput(base + Duration::from_secs(2)).is_none());
        e.progress.total_bytes = None;
        assert_eq!(e.percent(), None);
        e.status = Status::Completed;
        assert_eq!(e.percent(), Some(100));
        e.plan
            .sources
            .extend(std::iter::repeat_n(e.plan.sources[0].clone(), 2000));
        e.source_count = e.plan.sources.len();
        e.retire_payload();
        assert_eq!(e.plan.sources.len(), 1);
        assert_eq!(e.source_count, 2001);
        assert!(e.prepared.is_none());
        assert!(e.origin.selected.is_empty());
        for _ in 0..150 {
            e.progress.detail(PathBuf::new(), "detail".into());
        }
        assert_eq!(e.progress.details.len(), 100);
        assert_eq!(e.progress.omitted_details, 50);
        let (plan, options, origin) = (e.plan.clone(), e.options, e.origin.clone());
        for i in 1000..1110 {
            let mut copy = Entry {
                id: TransferId(i),
                source_count: plan.sources.len(),
                plan: plan.clone(),
                options,
                status: Status::Completed,
                progress: Progress::default(),
                error: None,
                origin: origin.clone(),
                prepared: None,
                started: None,
                ended: Some(base),
            };
            copy.progress.detail(PathBuf::new(), "history".into());
            app.transfers.entries.push_back(copy);
        }
        app.transfers.trim();
        assert_eq!(
            app.transfers
                .entries
                .iter()
                .filter(|e| e.status.terminal())
                .count(),
            100
        );
    }
    #[test]
    fn a_paused_prepared_queue_does_not_force_idle_redraws_or_io_polling() {
        let (_temp, mut app, _, _) = fixture();
        action(&mut app, Action::Close);
        assert!(app.transfers.has_work());
        assert!(!app.transfers.pending_io());
        assert!(!poll(&mut app, Instant::now()));
        assert!(!poll(&mut app, Instant::now() + Duration::from_secs(60)));
        assert!(app.job.is_none());
    }
    #[test]
    fn a_background_preparation_failure_requests_redraw_without_a_clock_or_view() {
        let (_temp, mut app, id, _) = fixture();
        app.config.show_clock = false;
        action(&mut app, Action::Close);
        app.transfers.entries[1].status = Status::Cancelled;
        let e = &mut app.transfers.entries[0];
        e.status = Status::Preparing;
        e.prepared = None;
        e.plan.sources[0] = e.plan.sources[0].with_file_name("missing");
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let changed = poll(&mut app, Instant::now());
            if app
                .transfers
                .entries
                .iter()
                .any(|e| e.id == id && e.status == Status::Failed)
            {
                assert!(changed);
                break;
            }
            assert!(Instant::now() < until, "preparation did not finish");
            std::thread::yield_now();
        }
        assert!(app.transfers.paused);
        assert_eq!(app.desktop.active().id, crate::desktop::WindowId::MANAGER);
        assert!(app.dialogs().is_empty());
    }
}
