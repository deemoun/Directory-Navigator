//! Original-style configurable menus; execution is always explicitly reviewed.
pub mod format;
pub mod runner;
use crate::{app::App, task::Task};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::{
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Auto,
    Global,
    Local,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Choose(usize),
    Back,
    Switch,
    Edit,
    Reload,
    Close,
    Run,
    Cancel,
}
pub struct Data {
    pub path: PathBuf,
    pub local: bool,
    pub error: Option<String>,
    pub items: Vec<format::Item>,
}
enum Reply {
    Loaded(Data),
    Edit(PathBuf),
}
#[derive(Default)]
pub struct State {
    open_epoch: u64,
    pub(crate) requests: crate::requests::RequestScope,
    pub modals: crate::modals::ModalStack,
    task: Option<Task<io::Result<Reply>>>,
    editing: bool,
    editing_document: Option<crate::desktop::WindowId>,
    pub pending: Option<runner::Plan>,
}
pub struct Modal {
    pub data: Option<Data>,
    pub branch: Vec<usize>,
    pub cursor: usize,
    pub offset: usize,
    pub rows: usize,
    pub focus: usize,
    pub review: Option<runner::Plan>,
    pub review_lines: Vec<String>,
    return_cursor: usize,
    pub status: String,
}
impl Default for Modal {
    fn default() -> Self {
        Self {
            data: None,
            branch: Vec::new(),
            cursor: 0,
            offset: 0,
            rows: 10,
            focus: 0,
            review: None,
            review_lines: Vec::new(),
            return_cursor: 0,
            status: "Loading menu…".into(),
        }
    }
}
impl Modal {
    pub fn items(&self) -> &[format::Item] {
        let Some(data) = &self.data else {
            return &[];
        };
        let mut items = data.items.as_slice();
        for index in &self.branch {
            let Some(item) = items.get(*index) else {
                return &[];
            };
            items = &item.children;
        }
        items
    }
    pub fn keep_visible(&mut self) {
        self.rows = self.rows.max(1);
        let len = if self.review.is_some() {
            self.review_lines.len()
        } else {
            self.items().len()
        };
        self.cursor = self.cursor.min(len.saturating_sub(1));
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        if self.cursor >= self.offset + self.rows {
            self.offset = self.cursor + 1 - self.rows;
        }
        self.offset = self.offset.min(len.saturating_sub(1));
    }
    pub fn step(&mut self, delta: isize) {
        let count = self.items().len();
        if self.review.is_some() {
            self.cursor = self
                .cursor
                .saturating_add_signed(delta)
                .min(self.review_lines.len().saturating_sub(1));
        } else {
            let direction = if delta < 0 { -1 } else { 1 };
            let mut cursor = self
                .cursor
                .saturating_add_signed(delta)
                .min(count.saturating_sub(1));
            for _ in 0..count {
                if !self.items()[cursor].separator() {
                    self.cursor = cursor;
                    break;
                }
                let next = cursor.saturating_add_signed(direction).min(count - 1);
                if next == cursor {
                    break;
                }
                cursor = next;
            }
        }
        self.keep_visible();
    }
}
fn global() -> io::Result<PathBuf> {
    crate::config::Config::path()
        .and_then(|p| p.parent().map(|p| p.join("dn.mnu")))
        .ok_or_else(|| io::Error::other("Cannot locate global configuration directory"))
}
fn read(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    crate::filesystem::open_regular(path)?
        .take(format::MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > format::MAX_BYTES {
        return Err(io::Error::other("Menu exceeds 128 KiB"));
    }
    String::from_utf8(bytes).map_err(|_| io::Error::other("Menu must be UTF-8"))
}
pub fn load(path: PathBuf, local: bool) -> io::Result<Data> {
    let parsed = match read(&path) {
        Ok(text) => format::parse(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => format::parse(format::DEFAULT),
        Err(e) => Err(e),
    };
    let (items, error) = match parsed {
        Ok(items) => (items, None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    Ok(Data {
        path,
        local,
        items,
        error,
    })
}

pub fn open_file(app: &mut App, path: PathBuf, local: bool) {
    app.user_menu.open_epoch = app.desktop.activation_revision();
    crate::menu::close(app);
    app.modals.set_quick_search(None);
    app.user_menu.modals.set_user_menu(Some(Modal::default()));
    match app.user_menu.requests.spawn("dn-user-menu", move |cancel| {
        cancel.check()?;
        load(path, local).map(Reply::Loaded)
    }) {
        Ok(task) => app.user_menu.task = Some(task),
        Err(e) => app.error(e.to_string()),
    }
}
pub fn open(app: &mut App, source: Source) {
    app.user_menu.open_epoch = app.desktop.activation_revision();
    let directory = app.panels[app.active].path.clone();
    let fallback = match global() {
        Ok(p) => p,
        Err(e) => {
            app.error(e.to_string());
            return;
        }
    };
    crate::menu::close(app);
    app.modals.set_quick_search(None);
    app.user_menu.modals.set_user_menu(Some(Modal::default()));
    let result = app.user_menu.requests.spawn("dn-user-menu", move |cancel| {
        if source != Source::Global {
            for ancestor in directory.ancestors() {
                cancel.check()?;
                let candidate = ancestor.join("dn.mnu");
                match fs::symlink_metadata(&candidate) {
                    Ok(_) => return load(candidate, true).map(Reply::Loaded),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
        }
        cancel.check()?;
        if source == Source::Local {
            load(directory.join("dn.mnu"), true).map(Reply::Loaded)
        } else {
            load(fallback, false).map(Reply::Loaded)
        }
    });
    match result {
        Ok(task) => app.user_menu.task = Some(task),
        Err(e) => app.error(e.to_string()),
    }
}
pub fn edit_definition(app: &mut App, local: bool) {
    let path = if local {
        app.panels[app.active].path.join("dn.mnu")
    } else {
        match global() {
            Ok(p) => p,
            Err(e) => {
                app.error(e.to_string());
                return;
            }
        }
    };
    open_file(app, path, local);
    app.user_menu.editing = true;
}
fn edit(app: &mut App) {
    let Some(path) = app
        .user_menu
        .modals
        .user_menu()
        .and_then(|m| m.data.as_ref())
        .map(|d| d.path.clone())
    else {
        return;
    };
    let task = app.user_menu.requests.spawn("dn-menu-edit", move |cancel| {
        cancel.check()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut f) => {
                if let Err(e) = f
                    .write_all(format::DEFAULT.as_bytes())
                    .and_then(|_| f.sync_all())
                {
                    return Err(io::Error::other(format!(
                        "Cannot finish new menu {}: {e}; incomplete file retained",
                        path.display()
                    )));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        Ok(Reply::Edit(path))
    });
    match task {
        Ok(task) => {
            app.user_menu.task = Some(task);
            app.user_menu.editing = true;
        }
        Err(e) => app.error(e.to_string()),
    }
}
pub fn poll(app: &mut App) -> bool {
    let result = app
        .user_menu
        .task
        .as_ref()
        .and_then(|t| match t.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(io::Error::other("User-menu worker stopped")))
            }
        });
    if let Some(result) = result {
        app.user_menu.task = None;
        match result {
            Ok(Reply::Loaded(data)) => {
                if let Some(m) = app.user_menu.modals.user_menu_mut() {
                    m.status = data.error.clone().unwrap_or_else(|| {
                        "Enter opens/runs · F2 Global/Local · F4 Edit · Ctrl+R reload".into()
                    });
                    m.data = Some(data);
                    m.step(0);
                }
                if app.user_menu.editing {
                    edit(app);
                }
            }
            Ok(Reply::Edit(path)) => {
                let foreground = app.user_menu.open_epoch == app.desktop.activation_revision()
                    && app
                        .modal_top()
                        .is_none_or(|m| m.focus == crate::focus::Focus::UserMenu);
                app.user_menu.editing_document =
                    crate::editor::controls::open_path_with_focus(app, path, foreground);
                app.user_menu.editing = app.user_menu.editing_document.is_some();
            }
            Err(e) => {
                app.user_menu.editing = false;
                if let Some(m) = app.user_menu.modals.user_menu_mut() {
                    m.status = e.to_string();
                }
                app.error(format!("User menu: {e}"));
            }
        }
        return true;
    }
    if app.user_menu.editing
        && app.user_menu.task.is_none()
        && app
            .user_menu
            .editing_document
            .is_some_and(|id| !app.desktop.contains(id))
    {
        app.user_menu.editing = false;
        reload(app);
        return true;
    }
    false
}
fn reload(app: &mut App) {
    if let Some((path, local)) = app
        .user_menu
        .modals
        .user_menu()
        .and_then(|m| m.data.as_ref())
        .map(|d| (d.path.clone(), d.local))
    {
        open_file(app, path, local);
    }
}
pub fn pending(state: &State) -> bool {
    state.task.is_some()
}
fn selected(panel: &crate::panel::Panel) -> io::Result<Vec<PathBuf>> {
    if panel.only_filtered_marks() {
        return Err(io::Error::other(
            "Marks exist only outside the filename filter",
        ));
    }
    if panel.selected.is_empty() {
        Ok(panel
            .entries
            .get(panel.cursor)
            .filter(|e| !e.parent)
            .map(|e| vec![e.path.clone()])
            .unwrap_or_default())
    } else {
        Ok(panel
            .entries
            .iter()
            .filter(|e| !e.parent && panel.selected.contains(&e.name))
            .take(2049)
            .map(|e| e.path.clone())
            .collect())
    }
}
pub fn plan(app: &App, commands: &[Vec<String>]) -> io::Result<runner::Plan> {
    let active = &app.panels[app.active];
    let other = &app.panels[app.active ^ 1];
    let file = active
        .entries
        .get(active.cursor)
        .filter(|e| !e.parent)
        .map(|e| e.path.as_os_str().to_owned());
    let other_file = other
        .entries
        .get(other.cursor)
        .filter(|e| !e.parent)
        .map(|e| e.path.as_os_str().to_owned());
    let mut result = runner::Plan {
        cwd: active.path.clone(),
        commands: Vec::new(),
        env: vec![
            ("DN_DIR".into(), active.path.as_os_str().to_owned()),
            ("DN_OTHER_DIR".into(), other.path.as_os_str().to_owned()),
            ("DN_FILE".into(), file.clone().unwrap_or_default()),
            (
                "DN_OTHER_FILE".into(),
                other_file.clone().unwrap_or_default(),
            ),
        ],
    };
    let mut total_bytes = 0usize;
    for command in commands {
        let mut args = Vec::<OsString>::new();
        for (position, token) in command.iter().enumerate() {
            let value = match token.as_str() {
                "{selected}" => {
                    if position == 0 {
                        return Err(io::Error::other("{selected} cannot be an executable"));
                    }
                    let paths = selected(active)?;
                    if paths.is_empty() {
                        return Err(io::Error::other("No visible marked or highlighted entry"));
                    }
                    args.extend(paths.into_iter().map(PathBuf::into_os_string));
                    continue;
                }
                "{dir}" => active.path.as_os_str().to_owned(),
                "{other_dir}" => other.path.as_os_str().to_owned(),
                "{file}" => file
                    .clone()
                    .ok_or_else(|| io::Error::other("Highlight a file or folder first"))?,
                "{name}" => active
                    .entries
                    .get(active.cursor)
                    .filter(|e| !e.parent)
                    .map(|e| e.name.clone())
                    .ok_or_else(|| io::Error::other("Highlight a file or folder first"))?,
                "{other_file}" => other_file
                    .clone()
                    .ok_or_else(|| io::Error::other("Other panel has no highlighted entry"))?,
                "{shell}" => std::env::var_os(if cfg!(windows) { "COMSPEC" } else { "SHELL" })
                    .unwrap_or_else(|| {
                        if cfg!(windows) {
                            "cmd.exe".into()
                        } else {
                            "/bin/sh".into()
                        }
                    }),
                _ => {
                    if [
                        "{selected}",
                        "{dir}",
                        "{other_dir}",
                        "{file}",
                        "{name}",
                        "{other_file}",
                        "{shell}",
                    ]
                    .iter()
                    .any(|p| token.contains(p))
                    {
                        return Err(io::Error::other(
                            "Placeholders must occupy a whole argument",
                        ));
                    }
                    token.into()
                }
            };
            args.push(value);
        }
        if args.is_empty() || args[0].is_empty() || args.len() > 2048 {
            return Err(io::Error::other(
                "Empty executable or more than 2048 arguments",
            ));
        }
        let bytes = args
            .iter()
            .map(|a| a.as_encoded_bytes().len() + 1)
            .sum::<usize>();
        total_bytes = total_bytes.saturating_add(bytes);
        if bytes > 64 * 1024 || total_bytes > 256 * 1024 {
            return Err(io::Error::other(
                "Expanded command exceeds 64 KiB, or menu action exceeds 256 KiB",
            ));
        }
        let program = args.remove(0);
        result.commands.push(runner::CommandSpec { program, args });
    }
    if result.commands.is_empty() {
        return Err(io::Error::other("Menu item has no commands"));
    }
    Ok(result)
}
pub fn action(app: &mut App, action: Action) {
    if action == Action::Close {
        app.user_menu = State::default();
        return;
    }
    if action == Action::Edit {
        edit(app);
        return;
    }
    if action == Action::Reload {
        reload(app);
        return;
    }
    if action == Action::Switch {
        let local = app
            .user_menu
            .modals
            .user_menu()
            .and_then(|m| m.data.as_ref())
            .is_some_and(|d| d.local);
        open(app, if local { Source::Global } else { Source::Local });
        return;
    }
    let Some(m) = app.user_menu.modals.user_menu_mut() else {
        return;
    };
    match action {
        Action::Cancel => {
            m.review = None;
            m.review_lines.clear();
            m.focus = 0;
            m.cursor = m.return_cursor;
            m.offset = 0;
            m.keep_visible();
        }
        Action::Run => {
            if let Some(plan) = m.review.take() {
                app.user_menu.pending = Some(plan);
                app.user_menu.modals.set_user_menu(None);
            }
        }
        Action::Back => {
            if let Some(parent) = m.branch.pop() {
                m.cursor = parent;
                m.offset = 0;
            } else {
                app.user_menu = State::default();
            }
        }
        Action::Choose(index) => {
            let Some(item) = m.items().get(index) else {
                return;
            };
            if item.separator() {
                return;
            }
            if !item.children.is_empty() {
                m.branch.push(index);
                m.cursor = 0;
                m.offset = 0;
                m.focus = 0;
                return;
            }
            let commands = item.commands.clone();
            match plan(app, &commands) {
                Ok(plan) => {
                    let Some(m) = app.user_menu.modals.user_menu_mut() else {
                        return;
                    };
                    m.return_cursor = index;
                    m.review_lines = vec![
                        format!(
                            "Working directory: {}",
                            crate::filesystem::display_name(&plan.cwd.to_string_lossy())
                        ),
                        "Arguments are passed directly; no implicit shell.".into(),
                        "".into(),
                    ];
                    for (key, value) in &plan.env {
                        m.review_lines
                            .push(format!("{key}: {}", runner::display(value)));
                    }
                    m.review_lines.push(String::new());
                    for command in &plan.commands {
                        m.review_lines
                            .push(format!("Program: {}", runner::display(&command.program)));
                        for arg in &command.args {
                            m.review_lines.push(format!("  {}", runner::display(arg)));
                        }
                    }
                    m.review_lines.push("".into());
                    m.review_lines
                        .push("Run only commands you trust. Enter / Run executes.".into());
                    m.review_lines = m
                        .review_lines
                        .iter()
                        .flat_map(|line| crate::layout::wrap(line, 72))
                        .collect();
                    m.review = Some(plan);
                    m.focus = 1;
                    m.cursor = 0;
                    m.offset = 0;
                }
                Err(e) => app.error(e.to_string()),
            }
        }
        _ => {}
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    let Some(m) = app.user_menu.modals.user_menu_mut() else {
        return;
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
        action(app, Action::Reload);
        return;
    }
    let review = m.review.is_some();
    match key.code {
        KeyCode::F(1) => app.push_dialog(crate::dialog::Dialog {title:" User menu help ",message:"Arrows/Page keys move; Enter activates; Right opens a submenu.\nLeft/Esc returns; ~letters~ are hotkeys.\nF2 switches local/global; F4 edits the definition.\nCtrl+R reloads after external changes.\nCommands use separate arguments, with quotes for spaces.\nWhole arguments: {file}, {name}, {dir}, {other_file},\n{other_dir}, {selected}, {shell}.\nUse an explicit shell for pipelines and scripts.\nEvery command action is reviewed before Run; arrows scroll.\nPrograms run in the normal terminal; Enter returns to DN.".into()}),
        KeyCode::Esc => action(app, if review { Action::Cancel } else { Action::Back }),
        KeyCode::Up => m.step(-1),
        KeyCode::Down => m.step(1),
        KeyCode::PageUp => m.step(-(m.rows as isize)),
        KeyCode::PageDown => m.step(m.rows as isize),
        KeyCode::Home => m.step(isize::MIN),
        KeyCode::End => m.step(isize::MAX),
        KeyCode::Tab => m.focus = crate::control_focus::cycle(m.focus, if review { 3 } else { 4 }, false),
        KeyCode::BackTab => {
            m.focus = crate::control_focus::cycle(m.focus, if review { 3 } else { 4 }, true)
        }
        KeyCode::F(2) if !review => action(app, Action::Switch),
        KeyCode::F(4) if !review => action(app, Action::Edit),
        KeyCode::Left | KeyCode::Backspace if !review => action(app, Action::Back),
        KeyCode::Right if !review => {
            let i = m.cursor;
            if m.items().get(i).is_some_and(|i| !i.children.is_empty()) {
                action(app, Action::Choose(i));
            }
        }
        KeyCode::Enter => {
            let a = if review {
                if m.focus == 2 {
                    Action::Cancel
                } else {
                    Action::Run
                }
            } else {
                match m.focus {
                    1 => Action::Switch,
                    2 => Action::Edit,
                    3 => Action::Close,
                    _ => Action::Choose(m.cursor),
                }
            };
            action(app, a);
        }
        KeyCode::Char(c) if !review => {
            if let Some(i) = m.items().iter().position(|i| {
                crate::menu::mnemonic(&i.label).is_some_and(|v| v.eq_ignore_ascii_case(&c))
            }) {
                action(app, Action::Choose(i));
            }
        }
        KeyCode::F(n) if !review => {
            if let Some(i) = m.items().iter().position(|i| {
                crate::menu::plain(&i.label).split_whitespace().next() == Some(&format!("F{n}"))
            }) {
                action(app, Action::Choose(i));
            }
        }
        _ => {}
    }
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<crate::mouse::Target>) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(crate::mouse::Target::UserMenu(a)) = target {
                action(app, a);
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            if let Some(m) = app.user_menu.modals.user_menu_mut() {
                m.step(if event.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                });
            }
        }
        _ => {}
    }
}
