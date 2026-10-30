//! Original-style configurable menus; execution is always explicitly reviewed.
pub mod expand;
pub mod format;
pub mod runner;
use crate::{app::App, task::Task};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::{
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
    Parameter(usize),
    ParametersAccept,
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
pub struct Parameters {
    pub definitions: Vec<format::Parameter>,
    pub values: Vec<String>,
    pub cursors: Vec<usize>,
    commands: Vec<Vec<String>>,
    context: expand::Context,
}
pub struct Modal {
    pub data: Option<Data>,
    pub branch: Vec<usize>,
    pub cursor: usize,
    pub offset: usize,
    pub rows: usize,
    pub focus: usize,
    pub review: Option<runner::Plan>,
    pub parameters: Option<Parameters>,
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
            parameters: None,
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
        let last = if self.review.is_some() {
            self.review_lines.len()
        } else {
            self.items().len()
        }
        .saturating_sub(1);
        crate::list_state::ListViewport::fit(
            &mut self.cursor,
            &mut self.offset,
            self.rows,
            Some(last),
            Some(last),
        );
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
                    m.status = data
                        .error
                        .clone()
                        .unwrap_or_else(|| "Enter opens/runs · → submenu · Ctrl+R reload".into());
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
    expand::Context::capture(app).plan(commands, &Default::default())
}
pub fn paste(app: &mut App, text: &str) {
    if let Some(m) = app.user_menu.modals.user_menu_mut() {
        if let Some(p) = m.parameters.as_mut().filter(|p| m.focus < p.values.len()) {
            let i = m.focus;
            let old = p.values[i].clone();
            let cursor = p.cursors[i];
            crate::dialog::paste_input(&mut p.values[i], &mut p.cursors[i], text);
            if p.values[i].len() > 4096 {
                p.values[i] = old;
                p.cursors[i] = cursor;
            }
        }
    }
}
fn review(app: &mut App, plan: runner::Plan) {
    let Some(m) = app.user_menu.modals.user_menu_mut() else {
        return;
    };
    m.review_lines = vec![
        format!(
            "Working directory: {}",
            crate::filesystem::display_name(&plan.cwd.to_string_lossy())
        ),
        "Arguments are passed directly; no implicit shell.".into(),
        String::new(),
    ];
    for (k, v) in &plan.env {
        m.review_lines.push(format!("{k}: {}", runner::display(v)));
    }
    m.review_lines.push(String::new());
    for list in &plan.list_files {
        m.review_lines.push(format!(
            "Temporary file list (created only on Run): {}",
            runner::display(list.path().as_os_str())
        ));
        m.review_lines.extend(
            list.entries()
                .iter()
                .map(|p| format!("  {}", runner::display(p.as_os_str()))),
        );
    }
    for c in &plan.commands {
        m.review_lines
            .push(format!("Program: {}", runner::display(&c.program)));
        m.review_lines
            .extend(c.args.iter().map(|a| format!("  {}", runner::display(a))));
    }
    m.review_lines.push(String::new());
    m.review_lines
        .push("Run only commands you trust. Enter / Run executes.".into());
    m.review_lines = m
        .review_lines
        .iter()
        .flat_map(|s| crate::layout::wrap(s, 72))
        .collect();
    m.review = Some(plan);
    m.parameters = None;
    m.focus = 1;
    m.cursor = 0;
    m.offset = 0;
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
        Action::Parameter(i) => {
            if m.parameters.as_ref().is_some_and(|p| i < p.values.len()) {
                m.focus = i;
            }
        }
        Action::ParametersAccept => {
            let Some(p) = m.parameters.as_ref() else {
                return;
            };
            let values = p
                .definitions
                .iter()
                .zip(&p.values)
                .map(|(d, v)| (d.name.clone(), v.clone()))
                .collect();
            match p.context.plan(&p.commands, &values) {
                Ok(plan) => review(app, plan),
                Err(e) => app.error(e.to_string()),
            }
        }
        Action::Cancel => {
            m.parameters = None;
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
            let prompts = item.prompts.clone();
            let context = expand::Context::capture(app);
            app.user_menu.modals.user_menu_mut().unwrap().return_cursor = index;
            if !prompts.is_empty() {
                let values: Vec<_> = prompts.iter().map(|p| p.default.clone()).collect();
                let cursors = values.iter().map(String::len).collect();
                let m = app.user_menu.modals.user_menu_mut().unwrap();
                m.parameters = Some(Parameters {
                    definitions: prompts,
                    values,
                    cursors,
                    commands,
                    context,
                });
                m.focus = 0;
            } else {
                match context.plan(&commands, &Default::default()) {
                    Ok(plan) => review(app, plan),
                    Err(e) => app.error(e.to_string()),
                }
            }
        }
        _ => {}
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    if crate::dialog_controls::key(app, key, crate::focus::Focus::UserMenu) {
        return;
    }
    let Some(m) = app.user_menu.modals.user_menu_mut() else {
        return;
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
        action(app, Action::Reload);
        return;
    }
    if let Some(p) = m.parameters.as_mut() {
        if key.code == KeyCode::F(1) {
            app.push_dialog(crate::dialog::Dialog {title:" User menu parameters help ",message:"Enter values as single arguments. Review shows the expanded command before Run. Esc cancels without creating lists or running commands.".into()});
            return;
        }
        if m.focus < p.values.len() {
            let i = m.focus;
            let old = p.values[i].clone();
            let cursor = p.cursors[i];
            crate::dialog::edit_input(&mut p.values[i], &mut p.cursors[i], key.code);
            if p.values[i].len() > 4096 {
                p.values[i] = old;
                p.cursors[i] = cursor;
            }
        }
        return;
    }
    let review = m.review.is_some();
    match key.code {
        KeyCode::F(1) => app.push_dialog(crate::dialog::Dialog {title:" User menu help ",message:"Arrows/Page keys move; Enter activates; Right opens a submenu.\nLeft/Esc returns; ~letters~ are hotkeys.\nF2 switches local/global; F4 edits the definition.\nCtrl+R reloads after external changes.\nCommands use separate arguments, with quotes for spaces.\nWhole arguments: {file}, {name}, {dir}, {other_file},\n{other_dir}, {selected}, {other_selected}, {shell}.\n{selected_list}/{other_selected_list} provide temporary lists.\n? name 'Label' 'default' declares {param:name}.\nUse an explicit shell for pipelines and scripts.\nEvery command action is reviewed before Run; arrows scroll.\nPrograms run in the normal terminal; Enter returns to DN.".into()}),
        KeyCode::Up => m.step(-1),
        KeyCode::Down => m.step(1),
        KeyCode::PageUp => m.step(-(m.rows as isize)),
        KeyCode::PageDown => m.step(m.rows as isize),
        KeyCode::Home => m.step(isize::MIN),
        KeyCode::End => m.step(isize::MAX),
        KeyCode::F(2) if !review => action(app, Action::Switch),
        KeyCode::F(4) if !review => action(app, Action::Edit),
        KeyCode::Left | KeyCode::Backspace if !review => action(app, Action::Back),
        KeyCode::Right if !review => {
            let i = m.cursor;
            if m.items().get(i).is_some_and(|i| !i.children.is_empty()) {
                action(app, Action::Choose(i));
            }
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
                m.step(crate::list_state::wheel_delta(event.kind));
            }
        }
        _ => {}
    }
}
