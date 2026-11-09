//! Archive-as-directory navigation, adapted from DOS Navigator's ARCVIEW.PAS
//! (RIT Research Labs). Archive members never become native operation paths.
pub mod create;
pub mod dialogs;
pub mod extract;
mod password;
pub mod reader;
use crate::{app::App, panel::archive::MemberId};
pub use password::Password;
use std::{path::PathBuf, sync::Arc};
#[derive(Clone)]
pub struct Extraction {
    pub catalog: Arc<reader::Catalog>,
    pub directory: usize,
    pub members: Vec<MemberId>,
    pub password: Option<Password>,
}
pub const HELP: &str = "ZIP / TAR / TAR.GZ / TGZ in the file pane.\nEnter: folder or read-only preview · F3: preview\nInsert / Space / Ctrl+click: mark · + / -: group\nF5: extract marked items, or the current item\nFolders include descendants and empty directories.\nBackspace / ..: parent; at root returns to disk\nCtrl+PgUp: exit archive from any folder\nEsc: cancel indexing or preview · Ctrl+R: refresh\nBoth panes can browse archives independently.\nChoose a disk destination when both show archives.\nArchives are read-only; extract before editing.\nExisting files ask Replace / Skip / Cancel; folders merge.";
pub const EXTRACT_FIRST: &str = "Extract archive members to disk first (F5).";
pub fn require_native(app: &mut App) -> bool {
    if app.panels[app.active].is_archive() {
        app.error(EXTRACT_FIRST);
        false
    } else {
        true
    }
}
pub fn open(app: &mut App, path: PathBuf) {
    crate::menu::close(app);
    app.modals.set_quick_search(None);
    if crate::windowing::blocked(app) {
        return;
    }
    crate::windowing::activate(app, crate::desktop::WindowId::MANAGER);
    app.quick_view = None;
    if let Err(e) = app.panels[app.active].open_archive(path) {
        app.error(e.to_string());
    }
}
pub fn open_current(app: &mut App) {
    if !require_native(app) {
        return;
    }
    let p = &app.panels[app.active];
    if p.loading {
        return;
    }
    if let Some(e) = p
        .entries
        .get(p.cursor)
        .filter(|e| !e.directory && !e.parent && !e.symlink)
    {
        open(app, e.path.clone());
    }
}
pub fn view(app: &mut App) {
    view_mode(app, crate::viewer::ViewMode::Auto);
}
pub fn view_mode(app: &mut App, mode: crate::viewer::ViewMode) {
    let epoch = app.desktop.activation_revision();
    let command = app.command_line.text.clone();
    let p = &mut app.panels[app.active];
    if p.loading {
        return;
    }
    let Some(id) = p.entries.get(p.cursor).and_then(|e| p.member(e)) else {
        return;
    };

    let a = p.archive_mut().unwrap();
    let c = a.catalog.as_ref().unwrap().clone();
    let n = &c.nodes[id.index];
    if n.kind != reader::Kind::File {
        app.error("Only regular archive files can be viewed.");
        return;
    }
    a.preview_mode = mode;
    if n.locked && a.password.is_none() {
        dialogs::password_for_preview(app, id);
        return;
    }
    let password = a.password.clone();
    a.preview_epoch = epoch;
    a.preview_member = Some(id);
    a.preview_command = command;
    a.preview_title = PathBuf::from(format!(
        "{} :: /{}",
        a.path.display(),
        String::from_utf8_lossy(&n.path)
    ));
    let catalog = c;
    let task = p.scope.spawn("archive-preview", move |cancel| {
        reader::preview_with_mode(&catalog, id.index, password.as_ref(), mode, &cancel)
    });
    match task {
        Ok(task) => p.archive_mut().unwrap().preview = Some(task),
        Err(e) => app.error(e.to_string()),
    }
}
pub fn poll(app: &mut App) -> bool {
    let mut changed = dialogs::poll(app);
    for index in 0..app.panels.len() {
        let reply = app.panels[index]
            .archive_mut()
            .and_then(|a| crate::task::take_reply(&mut a.index));
        if let Some(reply) = reply {
            changed = true;
            match reply {
                Ok(Ok(c)) => {
                    let a = app.panels[index].archive_mut().unwrap();
                    a.directory = c
                        .nodes
                        .iter()
                        .position(|n| {
                            n.path == a.restore_directory && n.kind == reader::Kind::Directory
                        })
                        .unwrap_or(0);
                    a.generation = c.generation;
                    a.catalog = Some(Arc::new(c));
                    app.panels[index].archive_entries();
                }
                other => {
                    let message = match other {
                        Ok(Err(e)) => e.to_string(),
                        _ => "Archive worker stopped".into(),
                    };
                    app.panels[index].leave_archive(app.config.show_hidden);
                    report(app, index, message);
                }
            }
        }
        let reply = app.panels[index]
            .archive_mut()
            .and_then(|a| crate::task::take_reply(&mut a.preview));
        if let Some(reply) = reply {
            changed = true;
            match reply {
                Ok(Ok(contents)) => {
                    let a = app.panels[index].archive().unwrap();
                    // Inactive or superseded requests must never open or focus a document.
                    if preview_foreground(app, index) {
                        let mut viewer = crate::viewer::Viewer::from_archive_contents(
                            a.preview_title.clone(),
                            contents,
                            a.preview_member
                                .and_then(|id| {
                                    a.catalog.as_ref().map(|c| {
                                        crate::markdown::is_path(std::path::Path::new(
                                            String::from_utf8_lossy(&c.nodes[id.index].path)
                                                .as_ref(),
                                        ))
                                    })
                                })
                                .unwrap_or(false),
                        );
                        viewer.requested_mode = a.preview_mode;
                        viewer.markdown.requested = a.preview_mode == crate::viewer::ViewMode::Auto;
                        app.show_viewer(viewer);
                    }
                }
                Ok(Err(e)) => {
                    if preview_foreground(app, index) {
                        report(app, index, e.to_string());
                    } else {
                        app.notice = e.to_string();
                    }
                }
                _ => {}
            }
        }
    }
    changed
}

fn report(app: &mut App, index: usize, message: String) {
    if index == app.active
        && app.desktop.active().id == crate::desktop::WindowId::MANAGER
        && app.modal_top().is_none()
    {
        app.push_owned_dialog(
            crate::modals::ModalOwner::Document(crate::desktop::WindowId::MANAGER),
            crate::dialog::Dialog {
                title: " Archive error ",
                message,
            },
        );
    } else {
        app.notice = message;
    }
}

fn preview_foreground(app: &App, index: usize) -> bool {
    let p = &app.panels[index];
    let Some(a) = p.archive() else {
        return false;
    };
    index == app.active
        && a.preview_epoch == app.desktop.activation_revision()
        && app.desktop.active().id == crate::desktop::WindowId::MANAGER
        && app.modal_top().is_none()
        && app.command_line.text == a.preview_command
        && p.entries.get(p.cursor).and_then(|e| p.member(e)) == a.preview_member
}
