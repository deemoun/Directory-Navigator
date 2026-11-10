//! ZIP editor workspaces own their cleanup and never impersonate native members.
use super::{create::Creation, reader, Password};
use crate::{app::App, task::Cancellation};
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
pub(crate) struct Origin {
    root: PathBuf,
    identity: crate::platform_fs::EntryIdentity,
    pub path: PathBuf,
    pub label: String,
    pub base: PathBuf,
    archive_path: PathBuf,
    catalog: Mutex<Option<Arc<reader::Catalog>>>,
    member: Vec<u8>,
    password: Option<Password>,
}
impl Drop for Origin {
    fn drop(&mut self) {
        if crate::platform_fs::entry_identity(&self.root).ok().as_ref() == Some(&self.identity) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
impl Origin {
    fn prepare(
        catalog: Arc<reader::Catalog>,
        id: usize,
        password: Option<Password>,
        cancel: &Cancellation,
    ) -> io::Result<Arc<Self>> {
        let node = catalog
            .nodes
            .get(id)
            .filter(|n| {
                n.kind == reader::Kind::File && n.size <= crate::limits::EDITOR_BYTES as u64
            })
            .ok_or_else(|| {
                io::Error::other("Select a regular ZIP text member within the 64 MiB editor limit")
            })?;
        let leaf = node.path.rsplit(|b| *b == b'/').next().unwrap();
        let name = super::extract::component(leaf)?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut root = None;
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "dn-zip-edit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let builder = &mut fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {
                    root = Some(path);
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        let root = fs::canonicalize(
            root.ok_or_else(|| io::Error::other("Cannot create private ZIP editor workspace"))?,
        )?;
        let origin = Arc::new(Self {
            identity: crate::platform_fs::entry_identity(&root)?,
            path: root.join(name),
            root,
            archive_path: catalog.path.clone(),
            label: format!(
                "{} :: /{}",
                catalog.path.display(),
                String::from_utf8_lossy(&node.path)
            ),
            base: catalog.path.parent().unwrap().to_path_buf(),
            catalog: Mutex::new(Some(catalog.clone())),
            member: node.path.clone(),
            password,
        });
        reader::extract_to(
            &catalog,
            &[(id, origin.path.clone())],
            origin.password.as_ref(),
            cancel,
            |_, _| {},
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&origin.path, fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(windows)]
        {
            let mut permissions = fs::metadata(&origin.path)?.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(&origin.path, permissions)?;
        }
        Ok(origin)
    }
    pub fn confirm_durability(&self) -> Result<Option<String>, String> {
        let catalog = self
            .catalog
            .lock()
            .unwrap()
            .as_ref()
            .ok_or("ZIP was updated but could not be refreshed; reopen it or export with Save As")?
            .clone();
        catalog.validate().map_err(|e| e.to_string())?;
        Ok(crate::platform_fs::sync_parent(&catalog.path)
            .err()
            .map(|e| format!("ZIP written; directory synchronization failed: {e}")))
    }
    pub fn save(&self, cancel: &Cancellation) -> Result<Option<String>, String> {
        if crate::platform_fs::entry_identity(&self.root).map_err(|e| e.to_string())?
            != self.identity
        {
            return Err("ZIP editor workspace changed".into());
        }
        let mut saved = self.catalog.lock().unwrap();
        let catalog = saved
            .as_ref()
            .ok_or(
                "Archive was updated but could not be refreshed; reopen the member or use Save As",
            )?
            .clone();
        let id = catalog
            .nodes
            .iter()
            .position(|n| n.path == self.member)
            .ok_or("ZIP member disappeared")?;
        let request = Creation::update(
            catalog.clone(),
            vec![self.path.clone()],
            catalog.nodes[id].parent,
            Default::default(),
            self.password.clone(),
        )
        .map_err(|e| e.to_string())?;
        let request = request.preserving_member_mode(catalog.nodes[id].mode);
        let result = request.run(cancel, |_| {}).map_err(|e| e.to_string())?;
        // Publication has succeeded. A refresh failure must be reported as such,
        // not as an ordinary pre-publication save failure inviting another write.
        match reader::list(catalog.path.clone(), &Cancellation::default()) {
            Ok(new) => *saved = Some(Arc::new(new)),
            Err(error) => {
                *saved = None;
                return Ok(Some(format!(
                    "ZIP updated; catalog refresh failed: {error}"
                )));
            }
        }
        Ok((!result.details.is_empty())
            .then(|| "ZIP updated; review transfer durability warnings".to_string()))
    }
}
pub(crate) fn open(app: &mut App) {
    let panel = &app.panels[app.active];
    let Some(a) = panel.archive() else {
        return;
    };
    let Some(catalog) = a
        .catalog
        .clone()
        .filter(|c| c.format == reader::Format::Zip && c.note.is_none() && c.skipped == 0)
    else {
        app.error(super::EXTRACT_FIRST);
        return;
    };
    let Some(id) = panel
        .entries
        .get(panel.cursor)
        .and_then(|e| panel.member(e))
    else {
        return;
    };
    let existing = app
        .desktop
        .document_ids(crate::desktop::WindowKind::Editor)
        .into_iter()
        .find(|window| {
            app.desktop
                .editor(*window)
                .and_then(|e| e.archive_origin.as_ref())
                .is_some_and(|origin| {
                    origin.archive_path == catalog.path
                        && origin.member == catalog.nodes[id.index].path
                })
        });
    if let Some(window) = existing {
        crate::windowing::activate(app, window);
        return;
    }
    if catalog.nodes[id.index].locked && a.password.is_none() {
        super::dialogs::password_for_edit(app, id);
        return;
    }
    let password = a.password.clone();
    let epoch = app.desktop.activation_revision();
    let command = app.command_line.text.clone();
    let task = app.panels[app.active]
        .scope
        .spawn("archive-edit", move |cancel| {
            Origin::prepare(catalog, id.index, password, &cancel)
        });
    match task {
        Ok(task) => {
            let a = app.panels[app.active].archive_mut().unwrap();
            a.edit_pending = Some(task);
            a.preview_epoch = epoch;
            a.preview_member = Some(id);
            a.preview_command = command;
        }
        Err(error) => app.error(error.to_string()),
    }
}
pub(super) fn poll(app: &mut App, panel: usize) -> bool {
    let reply = app.panels[panel]
        .archive_mut()
        .and_then(|a| crate::task::take_reply(&mut a.edit_pending));
    let Some(reply) = reply else {
        return false;
    };
    if !super::preview_foreground(app, panel) {
        return true;
    }
    match reply {
        Ok(Ok(origin)) => {
            if let Some(id) = crate::editor::open_path_with_focus(app, origin.path.clone(), true) {
                if let Some(editor) = app.desktop.editor_mut(id) {
                    editor.archive_origin = Some(origin);
                }
            }
        }
        Ok(Err(error)) => app.error(error.to_string()),
        Err(_) => app.error("Archive editor worker stopped"),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::test_support::{self, Temp};
    #[test]
    fn private_editor_save_updates_zip_and_workspace_is_removed_on_close() -> io::Result<()> {
        let t = Temp::new()?;
        let path = t.0.join("edit.zip");
        test_support::zip(
            &path,
            &[("docs/Ёжик.txt", "old".as_bytes()), ("keep.txt", b"keep")],
            None,
        )?;
        let catalog = Arc::new(reader::list(path.clone(), &Cancellation::default())?);
        let id = catalog
            .nodes
            .iter()
            .position(|n| n.path == "docs/Ёжик.txt".as_bytes())
            .unwrap();
        let origin = Origin::prepare(catalog, id, None, &Cancellation::default())?;
        let workspace = origin.root.clone();
        fs::write(&origin.path, "edited\r\nЁжик")?;
        origin
            .save(&Cancellation::default())
            .map_err(io::Error::other)?;
        let catalog = reader::list(path.clone(), &Cancellation::default())?;
        let id = catalog
            .nodes
            .iter()
            .position(|n| n.path == "docs/Ёжик.txt".as_bytes())
            .unwrap();
        assert_eq!(
            reader::preview(&catalog, id, &Cancellation::default())?
                .source
                .as_deref(),
            Some("edited\r\nЁжик")
        );
        fs::write(&origin.path, "second save")?;
        origin
            .save(&Cancellation::default())
            .map_err(io::Error::other)?;
        assert!(origin.label.contains(":: /docs/Ёжик.txt"));
        let identity = crate::platform_fs::entry_identity(&path)?;
        let bytes = fs::read(&path)?;
        assert!(origin
            .confirm_durability()
            .map_err(io::Error::other)?
            .is_none());
        assert_eq!(identity, crate::platform_fs::entry_identity(&path)?);
        assert_eq!(bytes, fs::read(&path)?);
        drop(origin);
        assert!(!workspace.exists());
        assert!(path.is_file());
        Ok(())
    }
    #[test]
    fn external_zip_change_refuses_save_and_keeps_working_text() -> io::Result<()> {
        let t = Temp::new()?;
        let path = t.0.join("edit.zip");
        test_support::zip(&path, &[("a.txt", b"original")], None)?;
        let catalog = Arc::new(reader::list(path.clone(), &Cancellation::default())?);
        let id = catalog
            .nodes
            .iter()
            .position(|n| n.path == b"a.txt")
            .unwrap();
        let origin = Origin::prepare(catalog, id, None, &Cancellation::default())?;
        fs::write(&origin.path, "unsaved draft")?;
        test_support::zip(&path, &[("a.txt", b"external change")], None)?;
        let bytes = fs::read(&path)?;
        assert!(origin.save(&Cancellation::default()).is_err());
        assert_eq!(bytes, fs::read(path)?);
        assert_eq!(fs::read_to_string(&origin.path)?, "unsaved draft");
        Ok(())
    }
}
