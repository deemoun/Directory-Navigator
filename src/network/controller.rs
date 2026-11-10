use super::{
    remote::{Connection, Member, RemotePath},
    task::Task,
    transfer::Transfer,
};
use crate::{
    app::App,
    copy_dialog::Draft,
    dialog::{Prompt, PromptKind},
    operations::Kind,
};
use std::{io, path::PathBuf, sync::Arc};
#[derive(Clone)]
pub struct Mutation {
    pub connection: Arc<Connection>,
    pub members: Vec<Member>,
    pub directory: RemotePath,
    pub kind: Kind,
    pub target: Option<RemotePath>,
}
impl Mutation {
    pub fn title(&self) -> &'static str {
        match self.kind {
            Kind::Mkdir => " Make remote directory ",
            Kind::Move => " Rename remote item ",
            _ => " Confirm permanent remote deletion ",
        }
    }
    pub fn summary(&self) -> String {
        format!(
            "{} · {} item(s)",
            self.connection.label(&self.directory),
            self.members.len()
        )
    }
    pub fn run(
        &self,
        cancel: &std::sync::atomic::AtomicBool,
        mut notify: impl FnMut(&crate::operations::Progress),
    ) -> io::Result<crate::operations::Progress> {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Remote operation cancelled",
            ));
        }
        let mut progress = crate::operations::Progress::default();
        let token = crate::task::Cancellation::default();
        self.connection.with(|backend| {
            for member in &self.members {
                if backend.stat(&member.path, &token)?.as_ref() != Some(member) {
                    return Err(io::Error::other(
                        "Remote selection changed before execution",
                    ));
                }
            }
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Remote operation cancelled",
                ));
            }
            match self.kind {
                Kind::Mkdir => backend.mkdir(
                    self.target
                        .as_ref()
                        .ok_or_else(|| io::Error::other("Enter a remote directory name"))?,
                )?,
                Kind::Move => {
                    let target = self
                        .target
                        .as_ref()
                        .ok_or_else(|| io::Error::other("Enter a remote destination"))?;
                    if self.members.len() != 1 {
                        return Err(io::Error::other("Rename one remote item at a time"));
                    }
                    if backend.stat(target, &token)?.is_some() {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "Remote rename target exists",
                        ));
                    }
                    backend.rename(&self.members[0].path, target)?;
                }
                Kind::Delete | Kind::Trash => {
                    let mut stack = self
                        .members
                        .clone()
                        .into_iter()
                        .map(|m| (m, 0))
                        .collect::<Vec<_>>();
                    let mut items = Vec::new();
                    while let Some((member, depth)) = stack.pop() {
                        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                            return Err(io::Error::new(
                                io::ErrorKind::Interrupted,
                                "Remote deletion cancelled",
                            ));
                        }
                        if items.len() >= 100_000 || depth >= 512 {
                            return Err(io::Error::other(
                                "Remote deletion exceeds traversal limits",
                            ));
                        }
                        if member.directory && !member.link {
                            for child in backend.list(&member.path, &token)? {
                                if stack.len() + items.len() >= 100_000 {
                                    return Err(io::Error::other(
                                        "Remote deletion exceeds traversal limits",
                                    ));
                                }
                                stack.push((child, depth + 1));
                            }
                        }
                        items.push((member, depth));
                    }
                    items.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));
                    for (member, _) in items {
                        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                            return Err(io::Error::new(
                                io::ErrorKind::Interrupted,
                                "Remote deletion cancelled",
                            ));
                        }
                        let current = backend.stat(&member.path, &token)?.ok_or_else(|| {
                            io::Error::other("Remote item disappeared before deletion")
                        })?;
                        if current.directory != member.directory
                            || current.link != member.link
                            || !member.directory && current != member
                        {
                            return Err(io::Error::other("Remote item changed before deletion"));
                        }
                        backend.remove(&member.path, member.directory && !member.link)?;
                        progress.files += 1;
                        progress.member = Some(self.connection.label(&member.path));
                        notify(&progress);
                    }
                }
                _ => return Err(io::Error::other("Unsupported remote mutation")),
            }
            Ok(progress)
        })
    }
}
enum Prepared {
    View {
        label: String,
        contents: crate::viewer::Contents,
        markdown: bool,
    },
    Edit(Arc<super::editor::Origin>),
}
struct Pending {
    panel: usize,
    key: (u64, usize),
    task: Task<io::Result<Prepared>>,
}
#[derive(Default)]
pub struct State {
    pending: Option<Pending>,
    scope: crate::requests::RequestScope,
}
pub fn view(app: &mut App, edit: bool, mode: crate::viewer::ViewMode) {
    let panel = &app.panels[app.active];
    if panel.loading {
        return;
    }
    let Some(remote) = panel.remote() else {
        return;
    };
    let Some(entry) = panel
        .entries
        .get(panel.cursor)
        .filter(|e| !e.parent && !e.directory && !e.symlink)
    else {
        return;
    };
    let Some(member) = remote
        .members
        .get(&entry.name)
        .filter(|m| m.regular)
        .cloned()
    else {
        return;
    };
    let connection = remote.connection.clone();
    let base = panel.path.clone();
    let key = panel.location_key();
    let index = app.active;
    let task = Task::spawn(&app.network.scope, "dn-remote-preview", move |cancel| {
        if edit {
            return super::editor::Origin::prepare(connection, member, base, &cancel)
                .map(Prepared::Edit);
        }
        let mut bytes = Vec::new();
        let limit = crate::limits::VIEWER_BYTES as u64;
        let result = connection.with(|b| b.read(&member.path, &mut bytes, &cancel, limit));
        let truncated = match result {
            Ok(_) => false,
            Err(e) if e.kind() == io::ErrorKind::FileTooLarge => true,
            Err(e) => return Err(e),
        };
        let contents = crate::viewer::from_remote_bytes(bytes, mode, truncated, &cancel)?;
        Ok(Prepared::View {
            label: connection.label(&member.path),
            contents,
            markdown: member.path.name().ends_with(".md")
                || member.path.name().ends_with(".markdown"),
        })
    });
    match task {
        Ok(task) => {
            app.network.pending = Some(Pending {
                panel: index,
                key,
                task,
            });
            app.notice = "Loading remote file…".into();
        }
        Err(e) => app.error(e.to_string()),
    }
}
pub fn poll(app: &mut App) -> bool {
    let reply = match app.network.pending.as_ref() {
        None => return false,
        Some(p) => match p.task.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(io::Error::other(
                "Remote preview cancelled or worker stopped",
            ))),
        },
    };
    let Some(reply) = reply else {
        return false;
    };
    let pending = app.network.pending.take().unwrap();
    if app.panels[pending.panel].location_key() != pending.key {
        return true;
    }
    let foreground = app.desktop.active().id == crate::desktop::WindowId::MANAGER
        && app.active == pending.panel
        && app.modal_top().is_none();
    match reply {
        Ok(Prepared::View {
            label,
            contents,
            markdown,
        }) => {
            let viewer =
                crate::viewer::Viewer::from_archive_contents(label.into(), contents, markdown);
            if foreground {
                app.show_viewer(viewer);
            } else {
                let _ = app.desktop.open_viewer_background(viewer);
            }
        }
        Ok(Prepared::Edit(origin)) => {
            if let Some(id) =
                crate::editor::controls::open_path_with_focus(app, origin.path.clone(), foreground)
            {
                if let Some(e) = app.desktop.editor_mut(id) {
                    e.remote_origin = Some(origin);
                }
            }
        }
        Err(e) => {
            if foreground {
                app.error(format!("Cannot open remote file. {}", e));
            } else {
                app.notice = format!("Remote file: {e}");
            }
        }
    }
    true
}
pub fn prompt_copy(app: &mut App, override_destination: Option<PathBuf>) {
    if app.panels[app.active].loading {
        return;
    }
    let active = app.active;
    let other = active ^ 1;
    let (remote, base, field, sources, require_destination) = if let Some(r) =
        app.panels[active].remote()
    {
        let members = app.panels[active].remote_targets();
        if members.is_empty() {
            return;
        }
        let base = override_destination
            .clone()
            .unwrap_or_else(|| app.panels[other].path.clone());
        let explicit = override_destination.is_none() && app.panels[other].is_virtual();
        (
            Transfer::Download {
                connection: r.connection.clone(),
                members,
                destination: base.clone(),
            },
            base.clone(),
            if explicit {
                String::new()
            } else {
                base.to_string_lossy().into_owned()
            },
            vec![],
            explicit,
        )
    } else {
        if app.panels[active].is_virtual() {
            app.error("Copy archive members to disk before uploading");
            return;
        }
        let Some(r) = app.panels[other].remote() else {
            return;
        };
        let sources = app.panels[active].targets();
        if sources.is_empty() {
            return;
        }
        let transfer =
            match Transfer::upload(r.connection.clone(), sources.clone(), r.directory.clone()) {
                Ok(t) => t,
                Err(e) => {
                    app.error(e.to_string());
                    return;
                }
            };
        (
            transfer,
            app.panels[active].path.clone(),
            r.directory.as_str().into(),
            sources,
            false,
        )
    };
    let cursor = field.len();
    app.modals.set_prompt(Some(Prompt {
        kind: PromptKind::Operation(Kind::Copy),
        sources,
        base,
        field,
        cursor,
        focus: 0,
        copy: Some(Draft {
            remote: Some(remote),
            require_destination,
            ..Default::default()
        }),
    }));
}
pub fn submit_copy(app: &mut App, mut prompt: Prompt) {
    let mut copy = prompt.copy.take().unwrap();
    let mut transfer = copy.remote.take().unwrap();
    let result = (|| -> io::Result<()> {
        match &mut transfer {
            Transfer::Download { destination, .. } => {
                *destination = copy.picked.unwrap_or_else(|| {
                    if prompt.field.is_empty() {
                        prompt.base.clone()
                    } else {
                        prompt.base.join(&prompt.field)
                    }
                });
                if !destination.is_absolute() || copy.require_destination && prompt.field.is_empty()
                {
                    return Err(io::Error::other(
                        "Choose an explicit absolute filesystem destination",
                    ));
                }
                if let Some(root) = &copy.temp_drive {
                    root.validate()?;
                }
            }
            Transfer::Upload { directory, .. } => {
                *directory = RemotePath::new(&prompt.field)?;
            }
            Transfer::Mutation(_) => return Err(io::Error::other("Invalid copy request")),
        }
        copy.options.remove_sources = false;
        crate::transfers::submit_remote_root(app, transfer, copy.options, copy.temp_drive)
            .map_err(io::Error::other)?;
        Ok(())
    })();
    if let Err(e) = result {
        app.error(e.to_string());
    }
}
pub fn prompt_mutation(app: &mut App, kind: Kind, _rename: bool) {
    let panel = &app.panels[app.active];
    let Some(remote) = panel.remote() else {
        return;
    };
    if panel.loading {
        return;
    }
    let members = panel.remote_targets();
    if kind != Kind::Mkdir && members.is_empty() {
        return;
    }
    if kind == Kind::Move && members.len() != 1 {
        app.error("Rename one remote item at a time");
        return;
    }
    let kind = if kind == Kind::Trash {
        Kind::Delete
    } else {
        kind
    };
    let field = if kind == Kind::Move {
        members[0].path.name().into()
    } else {
        String::new()
    };
    let cursor = field.len();
    let mutation = Mutation {
        connection: remote.connection.clone(),
        members,
        directory: remote.directory.clone(),
        kind,
        target: None,
    };
    app.modals.set_prompt(Some(Prompt {
        kind: PromptKind::Remote(mutation),
        sources: vec![],
        base: panel.path.clone(),
        field,
        cursor,
        focus: if kind == Kind::Delete { 2 } else { 0 },
        copy: None,
    }));
}
pub fn submit_mutation(app: &mut App, mut mutation: Mutation, field: &str) {
    if matches!(mutation.kind, Kind::Move | Kind::Mkdir) {
        let target = if field.starts_with('/') {
            RemotePath::new(field)
        } else {
            mutation.directory.child(field)
        };
        match target {
            Ok(p) => mutation.target = Some(p),
            Err(e) => {
                app.error(e.to_string());
                return;
            }
        }
    }
    if let Err(e) =
        crate::transfers::submit_remote(app, Transfer::Mutation(mutation), Default::default())
    {
        app.error(e);
    }
}

impl State {
    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
    }
}
