//! Global trusted file associations; local folders cannot define commands.
pub mod format;
use crate::{app::App, focus::Focus, task::Task, user_menu::runner};
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    sync::mpsc::TryRecvError,
};

enum Reply {
    Loaded(Vec<format::Rule>),
    Edit(PathBuf),
    Open {
        panel: usize,
        path: PathBuf,
        cwd: PathBuf,
        target: format::Target,
        command: Option<runner::Plan>,
    },
}
#[derive(Default)]
pub struct State {
    pub rules: Vec<format::Rule>,
    pub pending: Option<runner::Plan>,
    error: Option<String>,
    task: Option<Task<io::Result<Reply>>>,
    loading: bool,
    editing: bool,
}
impl State {
    pub fn busy(&self) -> bool {
        self.task.is_some()
    }
}
pub fn path(app: &App) -> io::Result<PathBuf> {
    Ok(app.config_store.path()?.with_file_name("associations.conf"))
}
pub fn reload(app: &mut App) {
    crate::menu::close(app);
    app.associations.task = None;
    app.associations.pending = None;
    app.associations.loading = false;
    let result = path(app).and_then(|path| {
        Task::spawn("dn-associations", move |cancel| {
            cancel.check()?;
            format::load(&path).map(Reply::Loaded)
        })
    });
    match result {
        Ok(task) => {
            app.associations.task = Some(task);
            app.associations.loading = true;
        }
        Err(e) => {
            app.associations.error = Some(e.to_string());
            app.error(format!("File associations: {e}"));
        }
    }
}
pub fn edit(app: &mut App) {
    crate::menu::close(app);
    app.associations.task = None;
    app.associations.pending = None;
    app.associations.loading = false;
    let result = path(app).and_then(|path| {
        Task::spawn("dn-associations-edit", move |cancel| {
            cancel.check()?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    let mut staged = crate::publication::StagedFile::create(&path)?;
                    staged.file.write_all(format::DEFAULT.as_bytes())?;
                    cancel.check()?;
                    match staged.publish(&path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(e) => return Err(e),
                    }
                }
                Err(e) => return Err(e),
                Ok(_) => {}
            }
            format::read(&path)?; // Editing can repair invalid syntax, but never follows a link.
            cancel.check()?;
            Ok(Reply::Edit(path))
        })
    });
    match result {
        Ok(task) => {
            app.associations.task = Some(task);
            app.associations.loading = false;
        }
        Err(e) => app.error(format!("File associations: {e}")),
    }
}
/// Returns true when custom handling claimed this Enter action.
pub fn open(app: &mut App, path: PathBuf) -> bool {
    if app.associations.busy() {
        app.error("File associations are loading; try again shortly.");
        return true;
    }
    if let Some(error) = &app.associations.error {
        app.error(format!(
            "File associations: {error}\nUse Ctrl+Enter for the OS default application."
        ));
        return true;
    }
    let Some(target) = app
        .associations
        .rules
        .iter()
        .find(|r| r.matches(&path))
        .map(|r| r.target.clone())
    else {
        return false;
    };
    if target == format::Target::Default {
        app.open_external_current();
        return true;
    }
    if app.job.is_some() {
        app.error("Wait for the file operation to finish before running an association.");
        return true;
    }
    let panel = app.active;
    let cwd = app.panels[panel].path.clone();
    match Task::spawn("dn-associated-open", move |cancel| {
        cancel.check()?;
        let file = crate::filesystem::open_regular(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        if !crate::platform_fs::same_identity(&metadata, &file.metadata()?) {
            return Err(io::Error::other("File changed before association launch"));
        }
        let command = if let format::Target::Command(args) = &target {
            Some(format::command(args, &path, cwd.clone())?)
        } else {
            None
        };
        cancel.check()?;
        Ok(Reply::Open {
            panel,
            path,
            cwd,
            target,
            command,
        })
    }) {
        Ok(task) => {
            app.associations.task = Some(task);
            app.associations.loading = false;
        }
        Err(e) => app.error(format!("File associations: {e}")),
    }
    true
}
pub fn poll(app: &mut App) -> bool {
    let result = app
        .associations
        .task
        .as_ref()
        .and_then(|task| match task.try_recv() {
            Ok(reply) => Some(reply),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(io::Error::other("Association worker stopped")))
            }
        });
    if let Some(result) = result {
        app.associations.task = None;
        let loading = std::mem::take(&mut app.associations.loading);
        match result {
            Ok(Reply::Loaded(rules)) => {
                app.associations.rules = rules;
                app.associations.error = None;
                app.notice = "File associations loaded".into();
            }
            Ok(Reply::Edit(path)) => {
                crate::editor::open_path(app, path);
                app.associations.editing = app.editor.is_some();
            }
            Ok(Reply::Open {
                panel,
                path,
                cwd,
                target,
                command,
            }) => {
                let current = &app.panels[panel];
                if crate::focus::current(app) != Focus::Manager
                    || app.active != panel
                    || current.path != cwd
                    || !current
                        .entries
                        .get(current.cursor)
                        .is_some_and(|e| e.path == path)
                {
                    app.notice = "Association opening cancelled after navigation".into();
                    return true;
                }
                match target {
                    format::Target::Editor => crate::editor::open_path(app, path),
                    format::Target::Viewer | format::Target::Hex => {
                        app.open_viewer_mode(if target == format::Target::Hex {
                            crate::viewer::ViewMode::Hex
                        } else {
                            crate::viewer::ViewMode::Auto
                        })
                    }
                    format::Target::Command(_) => app.associations.pending = command,
                    format::Target::Default => {}
                }
            }
            Err(e) => {
                // Failed loading also disables old rules instead of executing stale definitions.
                if loading {
                    app.associations.error = Some(e.to_string());
                }
                app.error(format!("File associations: {e}"));
            }
        }
        return true;
    }
    if app.associations.editing && app.editor.is_none() {
        app.associations.editing = false;
        reload(app);
        return true;
    }
    false
}
