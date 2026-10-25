use crate::{
    app::App,
    dialog,
    traversal::{self, Event, Hit, Job, Request, Size, Stats},
};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use std::{
    path::PathBuf,
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};
#[derive(Clone)]
pub struct Preferences {
    pub pattern: String,
    pub recursive: bool,
    pub hidden: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            pattern: "*".into(),
            recursive: true,
            hidden: true,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    FindForm,
    FindResults,
    Sizes,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Field,
    Toggle(usize),
    Start,
    Stop,
    Close,
    Recalculate,
    Row(usize),
    Jump,
    View,
    Edit,
}
pub struct Modal {
    pub(crate) requests: crate::requests::RequestScope,
    pub kind: Kind,
    pub root: PathBuf,
    pub panel: usize,
    pub prefs: Preferences,
    pub input_cursor: usize,
    pub replace_input: bool,
    pub focus: usize,
    pub hits: Vec<Hit>,
    pub sizes: Vec<Size>,
    pub sources: Vec<PathBuf>,
    pub cursor: usize,
    pub offset: usize,
    pub rows: usize,
    pub stats: Stats,
    pub status: String,
    pub job: Option<Job>,
    pub last_click: Option<(usize, Instant)>,
}
#[derive(Default)]
pub struct Discovery {
    pub modals: crate::modals::ModalStack,
    pub prefs: Preferences,
    pub initialized: bool,
}
impl Modal {
    fn new(app: &App, kind: Kind) -> Self {
        let prefs = app.discovery.prefs.clone();
        Self {
            requests: Default::default(),
            kind,
            root: app.panels[app.active].path.clone(),
            panel: app.active,
            input_cursor: prefs.pattern.len(),
            replace_input: true,
            prefs,
            focus: 0,
            hits: Vec::new(),
            sizes: Vec::new(),
            sources: Vec::new(),
            cursor: 0,
            offset: 0,
            rows: 10,
            stats: Stats::default(),
            status: String::new(),
            job: None,
            last_click: None,
        }
    }
    pub fn count(&self) -> usize {
        if self.kind == Kind::Sizes {
            self.sizes.len()
        } else {
            self.hits.len()
        }
    }
    pub fn step(&mut self, amount: isize) {
        self.cursor = self
            .cursor
            .saturating_add_signed(amount)
            .min(self.count().saturating_sub(1));
        self.keep_visible();
    }
    pub fn keep_visible(&mut self) {
        self.rows = self.rows.max(1);
        self.cursor = self.cursor.min(self.count().saturating_sub(1));
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        if self.cursor >= self.offset + self.rows {
            self.offset = self.cursor + 1 - self.rows;
        }
        self.offset = self.offset.min(self.count().saturating_sub(1));
    }
}
fn prepare(app: &mut App) {
    crate::menu::close(app);
    app.modals.set_quick_search(None);
}
pub fn open_find(app: &mut App) {
    if app.panels[app.active].loading {
        return;
    }
    prepare(app);
    if !app.discovery.initialized {
        app.discovery.prefs.hidden = app.config.show_hidden;
        app.discovery.initialized = true;
    }
    app.discovery
        .modals
        .set_discovery(Some(Modal::new(app, Kind::FindForm)));
}
pub fn open_sizes(app: &mut App) {
    prepare(app);
    let panel = &app.panels[app.active];
    if panel.loading {
        return;
    }
    if panel.only_filtered_marks() {
        app.error("All marked entries are filtered out. Clear the filter or marks first.");
        return;
    }
    let targets: std::collections::HashSet<_> = panel.targets().into_iter().collect();
    let sources: Vec<_> = panel
        .entries
        .iter()
        .filter(|e| e.directory && !e.parent && !e.symlink && targets.contains(&e.path))
        .map(|e| e.path.clone())
        .collect();
    if sources.is_empty() {
        app.error("Select one or more visible directories first.");
        return;
    }
    let mut modal = Modal::new(app, Kind::Sizes);
    modal.sources = sources;
    app.discovery.modals.set_discovery(Some(modal));
    action(app, Action::Recalculate);
}
pub fn paste(app: &mut App, text: &str) {
    if let Some(m) = app.discovery.modals.discovery_mut() {
        if m.kind == Kind::FindForm && m.focus == 0 {
            if m.replace_input {
                m.prefs.pattern.clear();
                m.input_cursor = 0;
                m.replace_input = false;
            }
            dialog::paste_input(&mut m.prefs.pattern, &mut m.input_cursor, text);
        }
    }
}
pub fn action(app: &mut App, command: Action) {
    if command == Action::Close {
        app.discovery.modals.set_discovery(None);
        return;
    }
    let Some(m) = app.discovery.modals.discovery_mut() else {
        return;
    };
    match command {
        Action::Field => m.focus = 0,
        Action::Toggle(i) => {
            m.focus = i;
            if i == 1 {
                m.prefs.recursive = !m.prefs.recursive;
            } else if i == 2 {
                m.prefs.hidden = !m.prefs.hidden;
            }
        }
        Action::Start if m.kind == Kind::FindForm => {
            if m.prefs.pattern.is_empty() {
                app.error("Enter a filename pattern.");
                return;
            }
            app.discovery.prefs = m.prefs.clone();
            match Job::start(Request::Find {
                root: m.root.clone(),
                pattern: m.prefs.pattern.clone(),
                recursive: m.prefs.recursive,
                hidden: m.prefs.hidden,
            }) {
                Ok(mut job) => {
                    job.bind(m.requests.owner());
                    m.job = Some(job);
                    m.kind = Kind::FindResults;
                    m.focus = 0;
                    m.status = "Searching…".into();
                }
                Err(e) => app.error(e.to_string()),
            }
        }
        Action::Recalculate if m.kind == Kind::Sizes && m.job.is_none() => {
            m.job = None;
            m.sizes.clear();
            m.stats = Stats::default();
            m.cursor = 0;
            m.offset = 0;
            match Job::start(Request::Sizes(m.sources.clone())) {
                Ok(mut job) => {
                    job.bind(m.requests.owner());
                    m.job = Some(job);
                    m.status = "Calculating…".into();
                }
                Err(e) => app.error(e.to_string()),
            }
        }
        Action::Stop => {
            if let Some(job) = &m.job {
                job.stop();
                m.status = "Stopping…".into();
            }
        }
        Action::Row(index) => {
            m.focus = 0;
            if index < m.count() {
                m.cursor = index;
                let now = Instant::now();
                let double = m.last_click.is_some_and(|(old, when)| {
                    old == index && now.duration_since(when) <= Duration::from_millis(400)
                });
                m.last_click = Some((index, now));
                if double && m.kind == Kind::FindResults {
                    action(app, Action::Jump);
                }
            }
        }
        Action::Jump | Action::View | Action::Edit if m.kind == Kind::FindResults => {
            let Some(hit) = m.hits.get(m.cursor) else {
                return;
            };
            let path = hit.path.clone();
            let panel = m.panel;
            if command == Action::Jump {
                let Some(parent) = path.parent() else {
                    return;
                };
                let parent = parent.to_owned();
                app.discovery.modals.set_discovery(None);
                app.panels[panel].set_filter(String::new());
                if path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                {
                    app.config.show_hidden = true;
                }
                app.panels[panel].go_to(parent, app.config.show_hidden);
                app.panels[panel].focus_after_refresh = path.file_name().map(|n| n.to_owned());
                app.active = panel;
            } else if hit.regular {
                if command == Action::View {
                    match crate::viewer::Viewer::open(path) {
                        Ok(v) => app.show_viewer(v),
                        Err(e) => app.error(e.to_string()),
                    }
                } else {
                    crate::editor::open_path(app, path);
                }
            }
        }
        _ => {}
    }
}
pub fn key(app: &mut App, event: KeyEvent) {
    let Some(m) = app.discovery.modals.discovery_mut() else {
        return;
    };
    m.last_click = None;
    if event.code == KeyCode::Esc {
        action(app, Action::Close);
        return;
    }
    if m.kind == Kind::FindForm {
        match event.code {
            KeyCode::Tab => m.focus = crate::control_focus::cycle(m.focus, 5, false),
            KeyCode::BackTab => m.focus = crate::control_focus::cycle(m.focus, 5, true),
            KeyCode::Enter => {
                let command = match m.focus {
                    1 | 2 => Action::Toggle(m.focus),
                    4 => Action::Close,
                    _ => Action::Start,
                };
                action(app, command);
            }
            KeyCode::Char(' ') if m.focus != 0 => {
                let command = match m.focus {
                    1 | 2 => Action::Toggle(m.focus),
                    3 => Action::Start,
                    _ => Action::Close,
                };
                action(app, command);
            }
            code if m.focus == 0 => {
                if m.replace_input
                    && matches!(
                        code,
                        KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete
                    )
                {
                    m.prefs.pattern.clear();
                    m.input_cursor = 0;
                }
                m.replace_input = false;
                dialog::edit_input(&mut m.prefs.pattern, &mut m.input_cursor, code);
            }
            _ => {}
        }
    } else {
        let buttons = if m.kind == Kind::Sizes { 3 } else { 5 };
        match event.code {
            KeyCode::Tab => m.focus = crate::control_focus::cycle(m.focus, buttons, false),
            KeyCode::BackTab => m.focus = crate::control_focus::cycle(m.focus, buttons, true),
            KeyCode::Up => m.step(-1),
            KeyCode::Down => m.step(1),
            KeyCode::PageUp => m.step(-(m.rows as isize)),
            KeyCode::PageDown => m.step(m.rows as isize),
            KeyCode::Home => m.step(isize::MIN),
            KeyCode::End => m.step(isize::MAX),
            KeyCode::F(3) => action(app, Action::View),
            KeyCode::F(4) => action(app, Action::Edit),
            KeyCode::Enter | KeyCode::Char(' ') => {
                let command = if m.kind == Kind::Sizes {
                    match m.focus {
                        1 => Action::Stop,
                        2 => Action::Close,
                        _ => Action::Recalculate,
                    }
                } else {
                    match m.focus {
                        1 => Action::View,
                        2 => Action::Edit,
                        3 => Action::Stop,
                        4 => Action::Close,
                        _ => Action::Jump,
                    }
                };
                action(app, command);
            }
            _ => {}
        }
    }
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<crate::mouse::Target>) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(crate::mouse::Target::Discovery(command)) = target {
                action(app, command)
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            if let Some(m) = app.discovery.modals.discovery_mut() {
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
pub fn poll(app: &mut App) -> bool {
    let Some(m) = app.discovery.modals.discovery_mut() else {
        return false;
    };
    let mut changed = false;
    let mut done = false;
    if let Some(job) = &m.job {
        for _ in 0..16 {
            match job.try_recv() {
                Ok(Event::Found(mut hits)) => {
                    m.hits.append(&mut hits);
                    changed = true;
                }
                Ok(Event::Size(size)) => {
                    m.sizes.push(size);
                    changed = true;
                }
                Ok(Event::Progress(stats)) => {
                    m.stats = stats;
                    changed = true;
                }
                Ok(Event::Done {
                    stats,
                    cancelled,
                    limited,
                }) => {
                    m.stats = stats;
                    m.status = if cancelled {
                        "Stopped — partial results".into()
                    } else if limited {
                        format!("Limit reached: {} results", traversal::RESULT_LIMIT)
                    } else if m.stats.skipped > 0 {
                        "Complete — partial: entries skipped".into()
                    } else {
                        "Complete".into()
                    };
                    changed = true;
                    done = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    m.status = "Worker stopped — partial results".into();
                    changed = true;
                    done = true;
                    break;
                }
            }
        }
    }
    if done {
        m.job = None;
    }
    m.keep_visible();
    changed
}
