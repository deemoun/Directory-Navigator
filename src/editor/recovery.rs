//! Checkpoint work is workspace-owned and serialized, including retirement cleanup.
pub mod store;
mod view;
use crate::{
    app::App,
    desktop::{WindowId, WindowKind},
    requests::RequestScope,
    task::Task,
};
use std::{
    collections::HashMap,
    io,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Clone)]
pub struct Record {
    pub text: String,
    pub path: Option<PathBuf>,
    pub base: PathBuf,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub bom: bool,
    pub time: i64,
}
pub struct Dialog {
    pub file: PathBuf,
    pub record: Record,
    pub focus: usize,
    pub error: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Recover,
    Inspect,
    Discard,
    Later,
}
enum Reply {
    Started(io::Result<store::Store>),
    Read(PathBuf, io::Result<Record>),
    Updated(Vec<(WindowId, u64)>, Vec<PathBuf>, io::Result<()>),
    Deleted(io::Result<()>),
}
pub struct Manager {
    scope: RequestScope,
    store: Option<store::Store>,
    job: Option<Task<Reply>>,
    written: HashMap<WindowId, u64>,
    origins: HashMap<WindowId, PathBuf>,
    cleanup: std::collections::BTreeSet<PathBuf>,
    root: PathBuf,
    next: Instant,
    ready: Option<(PathBuf, io::Result<Record>)>,
    offering: bool,
    empty_notice_requested: bool,
    exiting: bool,
    last_error: Option<String>,
    exit_message: Option<crate::modals::ModalId>,
}
impl Manager {
    fn new(root: PathBuf) -> Self {
        Self {
            cleanup: Default::default(),
            root,
            scope: RequestScope::default(),
            store: None,
            job: None,
            written: HashMap::new(),
            origins: HashMap::new(),
            next: Instant::now(),
            ready: None,
            offering: true,
            empty_notice_requested: false,
            exiting: false,
            last_error: None,
            exit_message: None,
        }
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub(crate) fn resume(&mut self) {
        self.exiting = false;
        self.exit_message = None;
        self.next = Instant::now();
    }
}
pub fn start(app: &mut App, root: PathBuf) {
    if app.editor_recovery.is_some() {
        return;
    }
    let mut m = Manager::new(root.clone());
    match m.scope.spawn("editor-recovery", move |_| {
        Reply::Started(store::Store::open(&root))
    }) {
        Ok(job) => {
            m.job = Some(job);
            app.editor_recovery = Some(m);
        }
        Err(e) => app.error(format!("Editor recovery unavailable: {e}")),
    }
}
pub fn open(app: &mut App) {
    if let Some(m) = &mut app.editor_recovery {
        m.offering = true;
        m.empty_notice_requested = true;
    } else {
        match app
            .config_store
            .path()
            .map(|p| p.with_file_name("editor-recovery"))
        {
            Ok(p) => {
                start(app, p);
                if let Some(m) = &mut app.editor_recovery {
                    m.empty_notice_requested = true;
                }
            }
            Err(e) => app.error(e.to_string()),
        }
    }
}
fn fail(app: &mut App, m: &mut Manager, e: impl ToString) {
    let message = e.to_string();
    if m.last_error.as_ref() != Some(&message) {
        app.push_owned_dialog(crate::modals::ModalOwner::Workspace,crate::dialog::Dialog {title:" Editor recovery ",message:format!("{} {message}\n\n{}",app.config.language.text("Recovery checkpoint failed:"),app.config.language.text("Your open buffers are retained. Previously completed checkpoints remain available; DN will retry."))});
        m.last_error = Some(message);
    }
}
pub fn checkpoint_now(app: &mut App) {
    if let Some(m) = &mut app.editor_recovery {
        m.next = Instant::now();
    }
    poll(app, Instant::now());
}
pub fn poll(app: &mut App, now: Instant) -> bool {
    let Some(mut m) = app.editor_recovery.take() else {
        return false;
    };
    let was_busy = m.busy();
    let mut changed = false;
    let reply = m.job.as_ref().and_then(|j| match j.try_recv() {
        Ok(r) => Some(r),
        Err(std::sync::mpsc::TryRecvError::Empty) => None,
        Err(_) => Some(Reply::Deleted(Err(io::Error::other(
            "Recovery worker stopped",
        )))),
    });
    if let Some(reply) = reply {
        m.job = None;
        changed = true;
        match reply {
            Reply::Started(Ok(s)) => {
                if s.warnings > 0 {
                    app.notice =
                        "Unsafe recovery entries were skipped; no source files were changed".into();
                }
                m.store = Some(s);
            }
            Reply::Started(Err(e)) => fail(app, &mut m, e),
            Reply::Read(file, result) => {
                m.ready = Some((file, result));
            }
            Reply::Updated(revisions, removed, Ok(())) => {
                for (id, rev) in revisions {
                    m.written.insert(id, rev);
                }
                for p in &removed {
                    m.cleanup.remove(p);
                }
                m.origins.retain(|_, p| !removed.contains(p));
                m.last_error = None;
                m.next = now;
            }
            Reply::Updated(_, _, Err(e)) => fail(app, &mut m, e),
            Reply::Deleted(Ok(())) => {
                app.modals.set_recovery(None);
            }
            Reply::Deleted(Err(e)) => fail(app, &mut m, e),
        }
    }
    if m.ready.is_some() && app.modal_top().is_none() && !m.exiting {
        m.empty_notice_requested = false;
        let (file, result) = m.ready.take().unwrap();
        let (record, error) = match result {
            Ok(r) => (r, None),
            Err(e) => (
                Record {
                    text: String::new(),
                    path: None,
                    base: app.panels[app.active].path.clone(),
                    cursor: 0,
                    anchor: None,
                    bom: false,
                    time: 0,
                },
                Some(e.to_string()),
            ),
        };
        let focus = if error.is_some() { 3 } else { 0 };
        app.modals.set_recovery(Some(Dialog {
            file,
            record,
            focus,
            error,
        }));
        changed = true;
    }
    if m.job.is_none() && !m.exiting {
        if m.store.is_none() && now >= m.next {
            m.next = now + Duration::from_secs(5);
            let root = m.root.clone();
            match m.scope.spawn("editor-recovery", move |_| {
                Reply::Started(store::Store::open(&root))
            }) {
                Ok(j) => m.job = Some(j),
                Err(e) => fail(app, &mut m, e),
            }
        }
        if m.job.is_none() && m.ready.is_none() && m.offering && app.modal_top().is_none() {
            if let Some(file) = m.store.as_mut().and_then(|s| s.pending.pop_front()) {
                let copy = file.clone();
                let leases = m.store.as_ref().unwrap().leases();
                match m.scope.spawn("editor-recovery", move |_| {
                    let _leases = leases;
                    Reply::Read(copy.clone(), store::read(&copy))
                }) {
                    Ok(j) => m.job = Some(j),
                    Err(e) => {
                        m.store.as_mut().unwrap().pending.push_front(file);
                        fail(app, &mut m, e);
                    }
                }
            } else {
                m.offering = false;
                if m.empty_notice_requested && m.store.is_some() {
                    m.empty_notice_requested = false;
                    app.push_owned_dialog(
                        crate::modals::ModalOwner::Workspace,
                        crate::dialog::Dialog {
                            title: " Editor recovery ",
                            message: app.config.language.text("No files to recover").into(),
                        },
                    );
                    changed = true;
                }
            }
        }
        if m.job.is_none() && now >= m.next {
            m.next = now + Duration::from_secs(5);
            if let Some(s) = &m.store {
                let ids = app.desktop.document_ids(WindowKind::Editor);
                let mut writes = Vec::new();
                let mut revisions = Vec::new();
                for id in &ids {
                    let e = app.desktop.editor(*id).unwrap();
                    if let Some(b) = &e.buffer {
                        if e.dirty() {
                            m.cleanup.remove(&s.path(*id));
                            if m.written.get(id) != Some(&b.revision()) {
                                writes.push((
                                    s.path(*id),
                                    Record {
                                        text: b.text.clone(),
                                        path: e.path.clone().or_else(|| e.recovery_source.clone()),
                                        base: e.base.clone(),
                                        cursor: b.cursor,
                                        anchor: b.anchor,
                                        bom: e.bom,
                                        time: SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs()
                                            as i64,
                                    },
                                ));
                                revisions.push((*id, b.revision()));
                            }
                        } else if m.written.remove(id).is_some() {
                            m.cleanup.insert(s.path(*id));
                        }
                    }
                }
                let retired = m
                    .written
                    .keys()
                    .copied()
                    .filter(|id| !ids.contains(id))
                    .collect::<Vec<_>>();
                for id in retired {
                    m.written.remove(&id);
                    m.cleanup.insert(s.path(id));
                }
                // Transfer recovered checkpoints only after the new copy is durable.
                let transferred = m
                    .origins
                    .keys()
                    .copied()
                    .filter(|id| {
                        !ids.contains(id)
                            || app.desktop.editor(*id).is_some_and(|e| !e.dirty())
                            || revisions.iter().any(|(i, _)| i == id)
                            || m.written.contains_key(id)
                    })
                    .collect::<Vec<_>>();
                for id in transferred {
                    if let Some(p) = m.origins.get(&id) {
                        m.cleanup.insert(p.clone());
                    }
                }
                let removes = m.cleanup.iter().cloned().collect::<Vec<_>>();
                if !writes.is_empty() || !removes.is_empty() {
                    let leases = s.leases();
                    match m.scope.spawn("editor-recovery", move |_| {
                        let _leases = leases;
                        let work = || -> io::Result<()> {
                            for (p, r) in writes {
                                store::write(&p, &r)?;
                            }
                            for p in &removes {
                                store::remove(p)?;
                            }
                            Ok(())
                        };
                        let result = work();
                        Reply::Updated(revisions, removes, result)
                    }) {
                        Ok(j) => m.job = Some(j),
                        Err(e) => fail(app, &mut m, e),
                    }
                }
            }
        }
    }
    changed |= m.busy() != was_busy;
    app.editor_recovery = Some(m);
    changed
}
pub fn action(app: &mut App, action: Action) {
    let Some(mut m) = app.editor_recovery.take() else {
        return;
    };
    if m.busy() {
        app.editor_recovery = Some(m);
        return;
    }
    if let Some(d) = app.modals.recovery() {
        if d.error.is_some() && matches!(action, Action::Recover | Action::Inspect) {
            app.editor_recovery = Some(m);
            return;
        }
        match action {
            Action::Inspect => app.push_dialog(crate::dialog::Dialog {
                title: " Recovery Help ",
                message: format!(
                    "{}\n\n{}",
                    d.record.path.as_ref().map_or_else(
                        || "Untitled document".into(),
                        |p| crate::filesystem::display_name(&p.to_string_lossy())
                    ),
                    d.record.text.chars().take(32000).collect::<String>()
                ),
            }),
            Action::Later => {
                let d = app.modals.take_recovery().unwrap();
                if let Some(s) = &mut m.store {
                    s.pending.push_front(d.file);
                }
                m.offering = false;
            }
            Action::Discard => {
                let file = d.file.clone();
                let leases = m.store.as_ref().unwrap().leases();
                match m.scope.spawn("editor-recovery", move |_| {
                    let _leases = leases;
                    Reply::Deleted(store::remove(&file))
                }) {
                    Ok(j) => m.job = Some(j),
                    Err(e) => fail(app, &mut m, e),
                }
            }
            Action::Recover => {
                let d = app.modals.take_recovery().unwrap();
                let recovered = || -> Result<super::Editor, String> {
                    let mut e = super::Editor::new(d.record.base.clone())?;
                    let mut b = super::buffer::Buffer::new(d.record.text.clone())?;
                    b.mark_recovered();
                    b.move_to(d.record.anchor.unwrap_or(d.record.cursor), false);
                    if d.record.anchor.is_some() {
                        b.move_to(d.record.cursor, true);
                    }
                    e.buffer = Some(b);
                    e.bom = d.record.bom;
                    e.recovery_source = d.record.path.clone();
                    e.apply_preferences(app.config.editor);
                    e.notice = format!(
                        "{} — {}",
                        app.config.language.text("Recovered unsaved copy"),
                        d.record.path.as_ref().map_or_else(
                            || "Untitled document".into(),
                            |p| crate::filesystem::display_name(&p.to_string_lossy())
                        )
                    );
                    Ok(e)
                };
                match recovered().and_then(|e| {
                    app.install_document(|desktop| desktop.open_editor(e))
                        .map_err(|e| e.to_string())
                }) {
                    Ok(id) => {
                        m.origins.insert(id, d.file);
                        m.next = Instant::now();
                    }
                    Err(e) => {
                        app.modals.set_recovery(Some(d));
                        fail(app, &mut m, e);
                    }
                }
            }
        }
    }
    app.editor_recovery = Some(m);
}
pub fn finish(app: &mut App) -> bool {
    if !app.quit {
        return false;
    }
    let Some(mut m) = app.editor_recovery.take() else {
        return true;
    };
    m.exiting = true;
    if let Some(id) = m.exit_message {
        let acknowledged = !app.modal_infos().iter().any(|m| m.id == id);
        app.editor_recovery = Some(m);
        return acknowledged;
    }
    let ready = if m.busy() {
        false
    } else {
        let result = m
            .store
            .as_ref()
            .map_or(Ok(()), store::Store::clear_current)
            .and_then(|_| {
                for path in m.origins.values() {
                    store::remove(path)?;
                }
                Ok(())
            });
        match result {
            Ok(()) => true,
            Err(e) => {
                app.push_owned_dialog(
                    crate::modals::ModalOwner::Workspace,
                    crate::dialog::Dialog {
                        title: " Editor recovery ",
                        message: format!(
                            "{} {e}\n\n{}",
                            app.config.language.text("Recovery cleanup failed:"),
                            app.config.language.text(
                                "Checkpoints were retained. Press Enter or Esc to finish exiting."
                            )
                        ),
                    },
                );
                m.exit_message = app.modal_top().map(|m| m.id);
                false
            }
        }
    };
    app.editor_recovery = Some(m);
    ready
}
pub(crate) use view::{key, mouse, spec};
