//! Persistent real files, opened through the ordinary manager panel and operations.
use crate::{
    app::App,
    desktop::WindowId,
    dialog::{Prompt, PromptKind},
    operations::{self, Kind, Plan},
    platform_fs::{self, EntryIdentity},
};
use std::{fs, io, path::PathBuf};

#[derive(Clone)]
pub(crate) struct Root {
    path: PathBuf,
    identity: EntryIdentity,
}
fn directory(path: &std::path::Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let redirected = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let redirected = {
        use std::os::windows::fs::MetadataExt;
        redirected || metadata.file_attributes() & 0x400 != 0
    };
    if redirected || !metadata.is_dir() {
        return Err(io::Error::other(
            "Temp Drive must be a real directory, not a link or file",
        ));
    }
    Ok(())
}
impl Root {
    fn locate(app: &App, create: bool) -> io::Result<Option<Self>> {
        let parent = app
            .config_store
            .path()?
            .parent()
            .filter(|p| p.is_absolute())
            .ok_or_else(|| io::Error::other("Cannot determine the Temp Drive directory"))?;
        if create {
            fs::create_dir_all(parent)?;
        }
        let parent = match fs::canonicalize(parent) {
            Ok(path) => path,
            Err(e) if !create && e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let path = parent.join("temp-drive");
        if create {
            let builder = &mut fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        } else if let Err(e) = fs::symlink_metadata(&path) {
            if e.kind() == io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(e);
        }
        directory(&path)?;
        let root = Self {
            identity: platform_fs::entry_identity(&path)?,
            path,
        };
        root.validate()?;
        Ok(Some(root))
    }
    pub(crate) fn validate(&self) -> io::Result<()> {
        directory(&self.path)?;
        if platform_fs::entry_identity(&self.path)? != self.identity {
            return Err(io::Error::other("Temp Drive changed; reopen the command"));
        }
        Ok(())
    }
}

pub(crate) fn available(app: &App, clearing: bool) -> bool {
    !crate::windowing::blocked(app)
        && !app.blocking_operation()
        && (clearing || !app.panels[app.active].loading)
        && (!clearing || !app.transfers.has_work())
}
pub(crate) fn open(app: &mut App) {
    if !available(app, false) {
        return;
    }
    let result = (|| {
        let root = Root::locate(app, true)?.expect("created root");
        // Fail before replacing the old panel location if the directory cannot be read.
        fs::read_dir(&root.path)?;
        root.validate()?;
        crate::windowing::activate(app, WindowId::MANAGER);
        app.panels[app.active].go_to(root.path, app.config.show_hidden);
        Ok::<_, io::Error>(())
    })();
    if let Err(error) = result {
        app.error(error.to_string());
    }
}
pub(crate) fn copy(app: &mut App) {
    if !available(app, false) {
        return;
    }
    if !app.panels[app.active].is_remote()
        && app.panels[app.active].targets().is_empty()
        && app.panels[app.active].extraction().is_none()
    {
        app.prompt_native_copy();
        return;
    }
    let root = match Root::locate(app, true) {
        Ok(Some(root)) => root,
        Ok(None) => return,
        Err(error) => {
            app.error(error.to_string());
            return;
        }
    };
    crate::windowing::activate(app, WindowId::MANAGER);
    if app.panels[app.active].is_remote() {
        crate::network::prompt_copy(app, Some(root.path.clone()));
    } else {
        app.prompt_native_copy();
    }
    if let Some(prompt) = app.modals.prompt_mut() {
        prompt.base = root.path.clone();
        prompt.field = crate::filesystem::display_name(&root.path.to_string_lossy());
        prompt.cursor = prompt.field.len();
        if let Some(copy) = &mut prompt.copy {
            copy.picked = Some(root.path.clone());
            copy.temp_drive = Some(root);
        }
    }
}

pub struct Clear {
    pub(crate) kind: Kind,
    root: Root,
    prepared: operations::transfer::Prepared,
}
impl Clear {
    pub(crate) fn start(self, app: &App, sources: Vec<PathBuf>) -> io::Result<operations::Job> {
        if !available(app, true) {
            return Err(io::Error::other("Another file operation is running"));
        }
        self.root.validate()?;
        let plan = Plan {
            kind: self.kind,
            sources,
            destination: self.root.path,
        };
        self.prepared.validate(&plan)?;
        operations::Job::start_prepared(plan, self.prepared)
    }
}
pub(crate) fn clear(app: &mut App) {
    if !available(app, true) {
        return;
    }
    let result = (|| {
        let Some(root) = Root::locate(app, false)? else {
            return Ok(None);
        };
        let mut sources = fs::read_dir(&root.path)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<io::Result<Vec<_>>>()?;
        sources.sort();
        root.validate()?;
        if sources.is_empty() {
            return Ok(None);
        }
        let kind = if app.config.use_trash {
            Kind::Trash
        } else {
            Kind::Delete
        };
        let plan = Plan {
            kind,
            sources: sources.clone(),
            destination: root.path.clone(),
        };
        let prepared = operations::transfer::Prepared::capture(&plan)?;
        root.validate()?;
        Ok::<_, io::Error>(Some(Prompt {
            kind: PromptKind::ClearTempDrive(Clear {
                kind,
                root,
                prepared,
            }),
            sources,
            base: plan.destination,
            field: String::new(),
            cursor: 0,
            focus: 2,
            copy: None,
        }))
    })();
    match result {
        Ok(Some(prompt)) => app.modals.set_prompt(Some(prompt)),
        Ok(None) => {
            app.notice = "Temp Drive is empty.".into();
            app.push_dialog(crate::dialog::Dialog {
                title: " Temp Drive ",
                message: "Temp Drive is empty.".into(),
            });
        }
        Err(error) => app.error(error.to_string()),
    }
}
