//! Session-only directory browser. Branches load on demand, never recursively.
use crate::{
    app::App,
    task::{Cancellation, Task},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};
pub const BRANCH_LIMIT: usize = 10_000;
pub const CACHE_LIMIT: usize = 20_000;
#[derive(Clone, Debug)]
pub struct Child {
    pub path: PathBuf,
    pub link: bool,
}
#[derive(Default)]
struct Branch {
    children: Vec<Child>,
    expanded: bool,
    loaded: bool,
    note: Option<String>,
}
pub struct Scan {
    pub children: Vec<Child>,
    pub skipped: usize,
    pub first_error: Option<String>,
    pub limited: bool,
}
pub fn scan(
    path: &Path,
    hidden: bool,
    reveal: &[PathBuf],
    limit: usize,
    cancel: &Cancellation,
) -> io::Result<Scan> {
    cancel.check()?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return Err(io::Error::other(
            "Entry is no longer a directory; directory links are not expanded",
        ));
    }
    let mut result = Scan {
        children: Vec::new(),
        skipped: 0,
        first_error: None,
        limited: false,
    };
    for entry in fs::read_dir(path)? {
        cancel.check()?;
        let item = (|| -> io::Result<Option<Child>> {
            let entry = entry?;
            let child = entry.path();
            if !hidden
                && entry.file_name().to_string_lossy().starts_with('.')
                && !reveal.contains(&child)
            {
                return Ok(None);
            }
            let metadata = fs::symlink_metadata(&child)?;
            if metadata.is_dir() {
                Ok(Some(Child {
                    path: child,
                    link: false,
                }))
            } else if metadata.is_symlink() && fs::metadata(&child)?.is_dir() {
                Ok(Some(Child {
                    path: child,
                    link: true,
                }))
            } else {
                Ok(None)
            }
        })();
        match item {
            Ok(Some(child)) => {
                if result.children.len() >= limit {
                    result.limited = true;
                    break;
                }
                result.children.push(child);
            }
            Ok(None) => {}
            Err(e) => {
                result.skipped += 1;
                if result.first_error.is_none() {
                    result.first_error = Some(e.to_string());
                }
            }
        }
    }
    // Keep the explicitly located ancestry visible even when a parent is capped.
    if result.limited && limit > 0 {
        if let Some(current) = reveal.iter().find(|p| p.parent() == Some(path)) {
            cancel.check()?;
            if !result.children.iter().any(|c| &c.path == current)
                && fs::symlink_metadata(current).is_ok_and(|m| m.is_dir())
            {
                result.children.pop();
                result.children.push(Child {
                    path: current.clone(),
                    link: false,
                });
            }
        }
    }
    result
        .children
        .sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    Ok(result)
}
pub struct Row {
    pub path: PathBuf,
    pub guide: String,
    pub depth: usize,
    pub link: bool,
}
enum Pending {
    Locate(Task<io::Result<PathBuf>>),
    Branch(PathBuf, Task<io::Result<Scan>>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Refresh,
    Hidden,
    Close,
    Row(usize),
    Thumb,
    Toggle(usize),
    Up,
    Down,
    PageUp,
    PageDown,
}
pub struct Tree {
    pub panel: usize,
    pub original: PathBuf,
    pub root: Option<PathBuf>,
    pub rows: Vec<Row>,
    pub cursor: usize,
    pub offset: usize,
    pub page_rows: usize,
    pub focus: usize,
    pub hidden: bool,
    pub notice: String,
    branches: HashMap<PathBuf, Branch>,
    reveal: Vec<PathBuf>,
    pending: Option<Pending>,
    last_click: Option<(PathBuf, Instant)>,
}
impl Tree {
    pub fn new(panel: usize, path: PathBuf, hidden: bool) -> io::Result<Self> {
        let mut tree = Self {
            panel,
            original: path,
            root: None,
            rows: Vec::new(),
            cursor: 0,
            offset: 0,
            page_rows: 10,
            focus: 0,
            hidden,
            notice: "Locating current directory…".into(),
            branches: HashMap::new(),
            reveal: Vec::new(),
            pending: None,
            last_click: None,
        };
        tree.locate()?;
        Ok(tree)
    }
    fn locate(&mut self) -> io::Result<()> {
        let path = self.original.clone();
        self.pending = Some(Pending::Locate(Task::spawn(
            "dn-tree-locate",
            move |cancel| {
                cancel.check()?;
                let path = fs::canonicalize(path)?;
                if !fs::metadata(&path)?.is_dir() {
                    return Err(io::Error::other("Current directory is unavailable"));
                }
                Ok(path)
            },
        )?));
        Ok(())
    }
    fn seed(&mut self, path: PathBuf, link: bool) {
        self.pending = None;
        self.branches.clear();
        self.reveal = path.ancestors().map(Path::to_owned).collect();
        self.reveal.reverse();
        self.root = self.reveal.first().cloned();
        for (index, ancestor) in self.reveal.iter().enumerate() {
            let mut branch = Branch {
                expanded: index + 1 < self.reveal.len() || !link,
                ..Branch::default()
            };
            if let Some(next) = self.reveal.get(index + 1) {
                branch.children.push(Child {
                    path: next.clone(),
                    link: link && index + 2 == self.reveal.len(),
                });
            }
            self.branches.insert(ancestor.clone(), branch);
        }
        self.rebuild(Some(path));
    }
    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }
    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }
    pub fn marker(&self, row: &Row) -> &'static str {
        if row.link {
            "↗"
        } else {
            match self.branches.get(&row.path) {
                Some(b) if b.loaded && b.children.is_empty() => " ",
                Some(b) if b.expanded => "−",
                _ => "+",
            }
        }
    }
    pub fn expanded(&self, path: &Path) -> bool {
        self.branches.get(path).is_some_and(|b| b.expanded)
    }
    pub fn status(&self) -> String {
        if let Some(row) = self.selected() {
            if let Some(note) = self.branches.get(&row.path).and_then(|b| b.note.as_ref()) {
                return note.clone();
            }
            if row.link {
                return "Directory link · Enter follows explicitly; expansion disabled".into();
            }
        }
        if let Some(Pending::Branch(path, _)) = &self.pending {
            return format!("Reading {}…", path.display());
        }
        self.notice.clone()
    }
    pub fn keep_visible(&mut self) {
        self.page_rows = self.page_rows.max(1);
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        if self.cursor >= self.offset + self.page_rows {
            self.offset = self.cursor + 1 - self.page_rows;
        }
        self.offset = self.offset.min(self.rows.len().saturating_sub(1));
    }
    pub fn step(&mut self, delta: isize) {
        self.cursor = self
            .cursor
            .saturating_add_signed(delta)
            .min(self.rows.len().saturating_sub(1));
        self.keep_visible();
    }
    fn rebuild(&mut self, selected: Option<PathBuf>) {
        let anchor = self.selected().map(|r| r.path.clone());
        let visual_row = self.cursor.saturating_sub(self.offset);
        self.rows.clear();
        let Some(root) = &self.root else {
            return;
        };
        let mut stack = vec![(
            Child {
                path: root.clone(),
                link: false,
            },
            Vec::<bool>::new(),
        )];
        while let Some((child, continuations)) = stack.pop() {
            let depth = continuations.len();
            let mut guide = String::new();
            if depth > 0 {
                if depth > 20 {
                    guide.push_str("… ");
                }
                for continuation in continuations
                    .iter()
                    .skip(depth.saturating_sub(20))
                    .take(depth.min(20).saturating_sub(1))
                {
                    guide.push_str(if *continuation { "│ " } else { "  " });
                }
                guide.push_str(if continuations.last() == Some(&true) {
                    "├─"
                } else {
                    "└─"
                });
            }
            if !child.link {
                if let Some(branch) = self.branches.get(&child.path).filter(|b| b.expanded) {
                    for (index, next) in branch.children.iter().enumerate().rev() {
                        let mut continuation = continuations.clone();
                        continuation.push(index + 1 < branch.children.len());
                        stack.push((next.clone(), continuation));
                    }
                }
            }
            self.rows.push(Row {
                path: child.path,
                guide,
                depth,
                link: child.link,
            });
        }
        if let Some(selected) = selected {
            self.cursor = selected
                .ancestors()
                .find_map(|p| self.rows.iter().position(|r| r.path == p))
                .unwrap_or(0);
        }
        if self.selected().map(|r| &r.path) == anchor.as_ref() {
            self.offset = self.cursor.saturating_sub(visual_row);
        }
        self.keep_visible();
    }
    pub fn right(&mut self) {
        let Some(row) = self.selected() else {
            return;
        };
        if row.link {
            self.notice = "Directory links are opened with Enter, not expanded".into();
            return;
        }
        let path = row.path.clone();
        let branch = self.branches.entry(path.clone()).or_default();
        if branch.expanded {
            if let Some(child) = branch.children.first() {
                let path = child.path.clone();
                self.rebuild(Some(path));
            }
        } else {
            branch.expanded = true;
            self.rebuild(Some(path));
        }
    }
    pub fn left(&mut self) {
        let Some(row) = self.selected() else {
            return;
        };
        let path = row.path.clone();
        if let Some(branch) = self.branches.get_mut(&path).filter(|b| b.expanded) {
            branch.expanded = false;
            self.rebuild(Some(path));
        } else if let Some(parent) = path.parent() {
            self.rebuild(Some(parent.into()));
        }
    }
    pub fn toggle(&mut self) {
        let Some(row) = self.selected() else {
            return;
        };
        if row.link {
            return;
        }
        let path = row.path.clone();
        let branch = self.branches.entry(path.clone()).or_default();
        branch.expanded = !branch.expanded;
        self.rebuild(Some(path));
    }
    pub fn refresh(&mut self) {
        self.pending = None;
        let Some(path) = self.selected().map(|r| r.path.clone()) else {
            if let Err(e) = self.locate() {
                self.notice = e.to_string();
            }
            return;
        };
        if self.selected().is_some_and(|r| r.link) {
            self.notice = "Refresh the parent branch to recheck this link".into();
            return;
        }
        self.branches
            .retain(|p, _| p == &path || !p.starts_with(&path));
        self.branches.insert(
            path.clone(),
            Branch {
                expanded: true,
                ..Branch::default()
            },
        );
        self.notice = "Re-reading selected branch…".into();
        self.rebuild(Some(path));
    }
    pub fn toggle_hidden(&mut self) {
        self.hidden = !self.hidden;
        let selected = self.selected().map(|r| (r.path.clone(), r.link));
        if let Some((path, link)) = selected {
            self.seed(path, link);
        }
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let located = match &self.pending {
            Some(Pending::Locate(task)) => match task.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err(io::Error::other("Tree worker stopped")))
                }
            },
            _ => None,
        };
        if let Some(result) = located {
            self.pending = None;
            changed = true;
            match result {
                Ok(path) => {
                    self.seed(path, false);
                    self.notice =
                        "Right/+ expand · Left/− collapse · Enter open · Ctrl+R refresh".into();
                }
                Err(e) => self.notice = format!("Cannot locate current directory: {e}"),
            }
        }
        let response = match &self.pending {
            Some(Pending::Branch(path, task)) => match task.try_recv() {
                Ok(result) => Some((path.clone(), result)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some((path.clone(), Err(io::Error::other("Tree worker stopped"))))
                }
            },
            _ => None,
        };
        if let Some((path, result)) = response {
            self.pending = None;
            changed = true;
            let selected = self.selected().map(|r| r.path.clone());
            let branch = self.branches.entry(path).or_default();
            branch.loaded = true;
            match result {
                Ok(scan) => {
                    branch.children = scan.children;
                    branch.note = if scan.limited {
                        Some(format!(
                            "Listing limited to {} directories; close/reopen to reset cache",
                            branch.children.len()
                        ))
                    } else if scan.skipped > 0 {
                        Some(format!(
                            "{} entries skipped: {}",
                            scan.skipped,
                            scan.first_error.unwrap_or_default()
                        ))
                    } else {
                        None
                    };
                }
                Err(e) => branch.note = Some(format!("Cannot read branch: {e}")),
            }
            self.rebuild(selected);
        }
        if self.pending.is_none() {
            let candidate = self
                .rows
                .iter()
                .find(|r| {
                    !r.link
                        && self
                            .branches
                            .get(&r.path)
                            .is_some_and(|b| b.expanded && !b.loaded)
                })
                .map(|r| r.path.clone());
            if let Some(path) = candidate {
                let count = self
                    .branches
                    .values()
                    .map(|b| b.children.len())
                    .sum::<usize>();
                let limit = CACHE_LIMIT.saturating_sub(count).min(BRANCH_LIMIT);
                if limit == 0 {
                    if let Some(branch) = self.branches.get_mut(&path) {
                        branch.loaded = true;
                        branch.note =
                            Some("Tree cache limit reached; close and reopen to reset".into());
                    }
                    return true;
                }
                let source = path.clone();
                let hidden = self.hidden;
                let reveal = self.reveal.clone();
                match Task::spawn("dn-tree-branch", move |cancel| {
                    scan(&source, hidden, &reveal, limit, &cancel)
                }) {
                    Ok(task) => self.pending = Some(Pending::Branch(path, task)),
                    Err(e) => {
                        if let Some(branch) = self.branches.get_mut(&path) {
                            branch.loaded = true;
                            branch.note = Some(e.to_string());
                        }
                    }
                }
                changed = true;
            }
        }
        changed
    }
}
pub fn open(app: &mut App) {
    if app.panels[app.active].loading {
        return;
    }
    crate::menu::close(app);
    app.quick_search = None;
    match Tree::new(
        app.active,
        app.panels[app.active].path.clone(),
        app.config.show_hidden,
    ) {
        Ok(tree) => app.directory_tree = Some(tree),
        Err(e) => app.error(e.to_string()),
    }
}
pub fn action(app: &mut App, command: Action) {
    if command == Action::Close {
        app.directory_tree = None;
        return;
    }
    let Some(tree) = &mut app.directory_tree else {
        return;
    };
    match command {
        Action::Open => {
            let Some(row) = tree.selected() else {
                return;
            };
            let path = row.path.clone();
            let panel = tree.panel;
            app.directory_tree = None;
            app.active = panel;
            app.panels[panel].go_to(path, app.config.show_hidden);
        }
        Action::Refresh => tree.refresh(),
        Action::Hidden => tree.toggle_hidden(),
        Action::Up => tree.step(-1),
        Action::Down => tree.step(1),
        Action::PageUp => tree.step(-(tree.page_rows as isize)),
        Action::PageDown => tree.step(tree.page_rows as isize),
        Action::Toggle(index) => {
            if index < tree.rows.len() {
                tree.cursor = index;
                tree.focus = 0;
                tree.toggle();
                tree.last_click = None;
            }
        }
        Action::Row(index) if index < tree.rows.len() => {
            tree.cursor = index;
            tree.focus = 0;
            tree.keep_visible();
            let now = Instant::now();
            let path = tree.rows[index].path.clone();
            let double = tree.last_click.as_ref().is_some_and(|(old, when)| {
                old == &path && now.duration_since(*when) < Duration::from_millis(400)
            });
            tree.last_click = Some((path, now));
            if double {
                action(app, Action::Open);
            }
        }
        _ => {}
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    let Some(tree) = &mut app.directory_tree else {
        return;
    };
    tree.last_click = None;
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('r') => tree.refresh(),
            KeyCode::Char('h') => tree.toggle_hidden(),
            KeyCode::F(4) => app.directory_tree = None,
            _ => {}
        }
        return;
    }
    match key.code{
        KeyCode::Esc=>app.directory_tree=None,KeyCode::Tab=>tree.focus = crate::control_focus::cycle(tree.focus, 5, false),KeyCode::BackTab=>tree.focus = crate::control_focus::cycle(tree.focus, 5, true),
        KeyCode::Up=>tree.step(-1),KeyCode::Down=>tree.step(1),KeyCode::PageUp=>tree.step(-(tree.page_rows as isize)),KeyCode::PageDown=>tree.step(tree.page_rows as isize),KeyCode::Home=>tree.step(isize::MIN),KeyCode::End=>tree.step(isize::MAX),
        KeyCode::Left|KeyCode::Backspace|KeyCode::Char('-')=>tree.left(),KeyCode::Right|KeyCode::Char('+')=>tree.right(),
        KeyCode::Char(' ') if tree.focus==0=>tree.toggle(),
        KeyCode::Enter|KeyCode::Char(' ')=>{let command=match tree.focus{2=>Action::Refresh,3=>Action::Hidden,4=>Action::Close,_=>Action::Open};action(app,command);},
        KeyCode::F(1)=>app.dialogs.push(crate::dialog::Dialog{title:" Directory tree help ",message:"Up/Down and Page keys move through visible directories.\nRight/+ expands; Left/− collapses or selects the parent.\nSpace toggles a branch; Enter/double-click opens it.\nCtrl+R refreshes the selected branch; Ctrl+H shows hidden.\nLinks are leaves: Enter follows one explicitly.\nEsc/Ctrl+F4 closes without changing either panel.".into()}),_=>{}
    }
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<crate::mouse::Target>) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(crate::mouse::Target::Tree(command)) = target {
                action(app, command)
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            if let Some(tree) = &mut app.directory_tree {
                tree.step(if event.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                });
            }
        }
        _ => {}
    }
}

pub fn geometry(area: crate::presentation::layout::Rect) -> crate::presentation::layout::Rect {
    let width = area.width.saturating_sub(4).min(90);
    let height = area.height.saturating_sub(4).min(28);
    crate::presentation::layout::Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}
