//! Read-only virtual archive browser, separate from the underlying panels.
pub mod reader;
use crate::{app::App, task::Task, viewer::Contents};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use reader::{Catalog, Kind};
use std::{
    io,
    path::PathBuf,
    sync::{mpsc::TryRecvError, Arc},
    time::{Duration, Instant},
};
enum Reply {
    List(Arc<Catalog>),
    Preview(PathBuf, Contents),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Parent,
    View,
    Info,
    Refresh,
    Stop,
    Close,
    Row(usize),
    Up,
    Down,
    PageUp,
    PageDown,
}
pub struct Browser {
    preview_epoch: u64,
    pub(crate) requests: crate::requests::RequestScope,
    pub path: PathBuf,
    pub catalog: Option<Arc<Catalog>>,
    pub directory: usize,
    pub cursor: usize,
    pub offset: usize,
    pub rows: usize,
    pub focus: usize,
    pub status: String,
    task: Option<Task<io::Result<Reply>>>,
    last_click: Option<(usize, Instant)>,
}
impl Browser {
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let source = path.clone();
        let requests = crate::requests::RequestScope::default();
        let task = requests.spawn("dn-archive-index", move |cancel| {
            reader::list(source, &cancel).map(|catalog| Reply::List(Arc::new(catalog)))
        })?;
        Ok(Self {
            preview_epoch: 0,
            requests,
            path,
            catalog: None,
            directory: 0,
            cursor: 0,
            offset: 0,
            rows: 10,
            focus: 0,
            status: "Reading archive… Esc closes · Stop cancels".into(),
            task: Some(task),
            last_click: None,
        })
    }
    pub fn loading(&self) -> bool {
        self.task.is_some()
    }
    pub fn children(&self) -> &[usize] {
        self.catalog
            .as_ref()
            .and_then(|c| c.nodes.get(self.directory))
            .map(|n| n.children.as_slice())
            .unwrap_or(&[])
    }
    pub fn selected(&self) -> Option<usize> {
        self.cursor
            .checked_sub(1)
            .and_then(|i| self.children().get(i).copied())
    }
    pub fn keep_visible(&mut self) {
        self.rows = self.rows.max(1);
        self.cursor = self.cursor.min(self.children().len());
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        if self.cursor >= self.offset + self.rows {
            self.offset = self.cursor + 1 - self.rows;
        }
        self.offset = self.offset.min(self.children().len());
    }
    pub fn step(&mut self, delta: isize) {
        self.cursor = self
            .cursor
            .saturating_add_signed(delta)
            .min(self.children().len());
        self.keep_visible();
        self.last_click = None;
    }
    fn parent(&mut self) -> bool {
        if self.directory == 0 {
            return true;
        }
        if let Some(catalog) = &self.catalog {
            let previous = self.directory;
            self.directory = catalog.nodes[self.directory].parent;
            self.cursor = self
                .children()
                .iter()
                .position(|i| *i == previous)
                .map(|i| i + 1)
                .unwrap_or(0);
            self.offset = 0;
            self.keep_visible();
        }
        false
    }
}
pub fn open(app: &mut App, path: PathBuf) {
    crate::menu::close(app);
    app.modals.set_quick_search(None);
    match Browser::new(path) {
        Ok(b) => app.modals.set_archive(Some(b)),
        Err(e) => app.error(e.to_string()),
    }
}
pub fn open_current(app: &mut App) {
    let panel = &app.panels[app.active];
    if panel.loading {
        return;
    }
    if let Some(entry) = panel
        .entries
        .get(panel.cursor)
        .filter(|e| !e.directory && !e.parent)
    {
        open(app, entry.path.clone());
    }
}
pub fn poll(app: &mut App) -> bool {
    // Take the reply here so previews can open the ordinary viewer without paths
    // being handed to the filesystem or recorded in viewer history.
    let reply = app
        .modals
        .archive()
        .and_then(|a| a.task.as_ref())
        .and_then(|t| match t.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(io::Error::other("Archive worker stopped")))
            }
        });
    let Some(reply) = reply else {
        return false;
    };
    let Some(browser) = app.modals.archive_mut() else {
        return false;
    };
    browser.task = None;
    match reply {
        Ok(Reply::List(catalog)) => {
            browser.status = format!(
                "Read-only · {} entries · {} skipped{}",
                catalog.nodes.len().saturating_sub(1),
                catalog.skipped,
                catalog
                    .note
                    .as_ref()
                    .map(|s| format!(" · {s}"))
                    .unwrap_or_default()
            );
            browser.catalog = Some(catalog);
            browser.directory = 0;
            browser.cursor = 0;
            browser.offset = 0;
        }
        Ok(Reply::Preview(path, contents)) => {
            browser.status = "Read-only · Enter browse/view · F3 view · F9 info".into();
            let foreground = browser.preview_epoch == app.desktop.activation_revision()
                && app
                    .modal_top()
                    .is_some_and(|m| m.focus == crate::focus::Focus::Archive);
            let viewer = crate::viewer::Viewer::from_contents(path, contents);
            let result = if foreground {
                app.desktop.open_viewer(viewer)
            } else {
                app.desktop.open_viewer_background(viewer)
            };
            if let Err(error) = result {
                app.error(error.to_string());
            }
        }
        Err(e) => {
            browser.status = format!("Archive: {e}");
            app.error(e.to_string());
        }
    }
    true
}
pub fn action(app: &mut App, action: Action) {
    if action == Action::Close {
        app.modals.set_archive(None);
        return;
    }
    let epoch = app.desktop.activation_revision();
    let Some(browser) = app.modals.archive_mut() else {
        return;
    };
    match action {
        Action::Stop => {
            browser.task = None;
            browser.status = "Stopped · Refresh to reread the archive".into();
        }
        Action::Refresh => {
            let path = browser.path.clone();
            open(app, path);
        }
        Action::Up => browser.step(-1),
        Action::Down => browser.step(1),
        Action::PageUp => browser.step(-(browser.rows as isize)),
        Action::PageDown => browser.step(browser.rows as isize),
        Action::Parent => {
            if browser.parent() {
                app.modals.set_archive(None);
            }
        }
        Action::Row(row) => {
            if row > browser.children().len() {
                return;
            }
            let now = Instant::now();
            let double = browser.last_click.is_some_and(|(old, time)| {
                old == row && now.duration_since(time) < Duration::from_millis(400)
            });
            browser.cursor = row;
            browser.focus = 0;
            browser.keep_visible();
            browser.last_click = Some((row, now));
            if double {
                self::action(app, Action::Open);
            }
        }
        Action::Open | Action::View | Action::Info => {
            if browser.loading() {
                return;
            }
            let Some(id) = browser.selected() else {
                if action == Action::Open && browser.parent() {
                    app.modals.set_archive(None);
                }
                return;
            };
            let Some(catalog) = &browser.catalog else {
                return;
            };
            let node = &catalog.nodes[id];
            if action == Action::Open && node.kind == Kind::Directory {
                browser.directory = id;
                browser.cursor = 0;
                browser.offset = 0;
                return;
            }
            if action == Action::Info || node.kind != Kind::File || node.locked {
                let dialog=crate::dialog::Dialog{title:" Archive member information ",message:format!("{}\nType: {:?}{}\nSize: {} bytes\nModified: {}\nRead-only. Links and special entries are never followed.\nEncrypted members cannot be previewed.",crate::filesystem::display_name(&String::from_utf8_lossy(&node.path)),node.kind,if node.locked{" (encrypted)"}else{""},node.size,app.config.datetime.archive(node.modified.as_deref().unwrap_or("Unavailable")))};
                app.push_dialog(dialog);
                return;
            }
            let catalog = Arc::clone(catalog);
            let mut title = browser.path.as_os_str().to_owned();
            title.push(" :: /");
            title.push(String::from_utf8_lossy(&node.path).as_ref());
            let title = PathBuf::from(title);
            browser.preview_epoch = epoch;
            match browser.requests.spawn("dn-archive-preview", move |cancel| {
                reader::preview(&catalog, id, &cancel).map(|c| Reply::Preview(title, c))
            }) {
                Ok(task) => {
                    browser.task = Some(task);
                    browser.status = "Reading bounded preview… Stop cancels".into();
                }
                Err(e) => app.error(e.to_string()),
            }
        }
        _ => {}
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    if let Some(m) = app.modals.archive_mut() {
        m.last_click = None;
    }
    if crate::dialog_controls::key(app, key, crate::focus::Focus::Archive) {
        return;
    }
    let Some(browser) = app.modals.archive_mut() else {
        return;
    };
    browser.last_click = None;
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
        action(app, Action::Refresh);
        return;
    }
    match key.code{
  KeyCode::Backspace|KeyCode::Left=>action(app,Action::Parent),
  KeyCode::Up=>browser.step(-1),KeyCode::Down=>browser.step(1),KeyCode::PageUp=>browser.step(-(browser.rows as isize)),KeyCode::PageDown=>browser.step(browser.rows as isize),KeyCode::Home=>browser.step(isize::MIN),KeyCode::End=>browser.step(isize::MAX),KeyCode::Right=>{if browser.selected().is_some_and(|id|browser.catalog.as_ref().is_some_and(|c|c.nodes[id].kind==Kind::Directory)){action(app,Action::Open);}},
  KeyCode::F(3)=>action(app,Action::View),KeyCode::F(9)=>action(app,Action::Info),KeyCode::F(1)=>app.push_dialog(crate::dialog::Dialog{title:" Archive browser help ",message:"Read-only ZIP / TAR / TAR.GZ / TGZ browser.\nEnter/double-click opens a folder or previews a file.\nBackspace/Left goes up; Esc closes the archive.\nF3 previews; F9 shows metadata; Ctrl+R refreshes.\nStop cancels indexing or preview reads.\nLinks/special entries are never followed.\nNo extraction, editing or nested archive execution.\nIn the file manager, Ctrl+Enter uses the OS default app.".into()}),_=>{}
 }
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<crate::mouse::Target>) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(crate::mouse::Target::Archive(a)) = target {
                action(app, a);
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            if let Some(b) = app.modals.archive_mut() {
                b.step(if event.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                });
            }
        }
        _ => {}
    }
}
