use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    app::App,
    directory_tree::{self, Action, Tree},
    focus::{self, Focus},
    input,
    mouse::{self, Target},
    task::Cancellation,
    ui,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-tree-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(fs::canonicalize(p)?))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(app: &mut App, done: impl Fn(&App) -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        app.poll();
        if done(app) {
            break;
        }
        assert!(Instant::now() < until, "worker timeout");
        thread::sleep(Duration::from_millis(1));
    }
}
fn settle(app: &mut App) {
    wait(app, |a| {
        a.directory_tree.as_ref().is_some_and(|t| !t.loading())
    });
}
fn key(app: &mut App, code: KeyCode) {
    input::handle(app, KeyEvent::new(code, KeyModifiers::NONE));
}
fn select(app: &mut App, path: &Path) {
    let t = app.directory_tree.as_mut().unwrap();
    t.cursor = t.rows.iter().position(|r| r.path == path).unwrap();
    t.keep_visible();
}
fn open(app: &mut App) {
    input::handle(
        app,
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
    );
    settle(app);
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<()> {
    let mut term = Terminal::new(TestBackend::new(w, h))?;
    term.draw(|f| ui::draw(f, app))?;
    Ok(())
}
fn click(app: &mut App, target: Target) {
    let rect = app
        .mouse
        .regions
        .iter()
        .rev()
        .find(|(_, t)| *t == target)
        .unwrap()
        .0;
    mouse::handle(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}
#[test]
fn one_level_native_sorted_hidden_and_limits() -> io::Result<()> {
    let t = Temp::new()?;
    for n in ["z", "a", "文", ".secret"] {
        fs::create_dir(t.0.join(n))?;
    }
    fs::write(t.0.join("file"), b"x")?;
    fs::create_dir(t.0.join("a/nested"))?;
    let scan = directory_tree::scan(&t.0, false, &[], 100, &Cancellation::default())?;
    assert_eq!(
        scan.children
            .iter()
            .map(|c| c.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["a", "z", "文"]
    );
    assert_eq!(scan.skipped, 0);
    assert_eq!(
        directory_tree::scan(&t.0, true, &[], 100, &Cancellation::default())?
            .children
            .len(),
        4
    );
    let reveal = vec![t.0.join(".secret")];
    assert_eq!(
        directory_tree::scan(&t.0, false, &reveal, 100, &Cancellation::default())?
            .children
            .len(),
        4
    );
    let scan = directory_tree::scan(&t.0, true, &[], 2, &Cancellation::default())?;
    assert!(scan.limited);
    assert_eq!(scan.children.len(), 2);
    Ok(())
}
#[test]
fn scan_errors_and_cancellation() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("file"), b"x")?;
    for p in [t.0.join("missing"), t.0.join("file")] {
        assert!(directory_tree::scan(&p, true, &[], 10, &Cancellation::default()).is_err());
    }
    let token = Cancellation::default();
    token.cancel();
    assert_eq!(
        directory_tree::scan(&t.0, true, &[], 10, &token)
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::Interrupted
    );
    Ok(())
}
#[test]
fn lazy_expansion_locates_current_and_preserves_panels() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir_all(t.0.join("child/grandchild"))?;
    let mut a = t.app()?;
    let original = a.panels[0].path.clone();
    open(&mut a);
    let tree = a.directory_tree.as_ref().unwrap();
    assert_eq!(tree.selected().unwrap().path, t.0);
    assert_eq!(tree.root.as_deref(), t.0.ancestors().last());
    assert!(tree.rows.iter().any(|r| r.path == t.0.join("child")));
    assert!(!tree
        .rows
        .iter()
        .any(|r| r.path == t.0.join("child/grandchild")));
    select(&mut a, &t.0.join("child"));
    key(&mut a, KeyCode::Right);
    settle(&mut a);
    assert!(a
        .directory_tree
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join("child/grandchild")));
    key(&mut a, KeyCode::Char(' '));
    assert!(!a
        .directory_tree
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join("child/grandchild")));
    key(&mut a, KeyCode::Right);
    settle(&mut a);
    assert_eq!(
        a.directory_tree.as_ref().unwrap().selected().unwrap().path,
        t.0.join("child")
    );
    assert_eq!(a.panels[0].path, original);
    key(&mut a, KeyCode::Esc);
    assert!(a.directory_tree.is_none());
    assert_eq!(a.panels[1].path, original);
    Ok(())
}
#[test]
fn opening_and_failed_navigation_preserve_state() -> io::Result<()> {
    let t = Temp::new()?;
    let child = t.0.join("child");
    fs::create_dir(&child)?;
    let mut a = t.app()?;
    open(&mut a);
    select(&mut a, &child);
    key(&mut a, KeyCode::Enter);
    wait(&mut a, |a| !a.panels[0].loading);
    assert_eq!(a.panels[0].path, child);
    assert_eq!(a.panels[1].path, t.0);
    let other = t.0.join("other");
    fs::create_dir(&other)?;
    open(&mut a);
    select(&mut a, &other);
    fs::remove_dir(&other)?;
    key(&mut a, KeyCode::Enter);
    wait(&mut a, |a| !a.panels[0].loading);
    assert_eq!(a.panels[0].path, child);
    assert!(!a.dialogs.is_empty());
    Ok(())
}
#[test]
fn refresh_hidden_and_stale_close() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir(t.0.join(".hidden"))?;
    let mut a = t.app()?;
    a.config.show_hidden = false;
    open(&mut a);
    assert!(!a
        .directory_tree
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join(".hidden")));
    directory_tree::action(&mut a, Action::Hidden);
    settle(&mut a);
    assert!(a
        .directory_tree
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join(".hidden")));
    assert!(!a.config.show_hidden);
    fs::create_dir(t.0.join("new"))?;
    select(&mut a, &t.0);
    directory_tree::action(&mut a, Action::Refresh);
    settle(&mut a);
    assert!(a
        .directory_tree
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join("new")));
    fs::remove_dir(t.0.join("new"))?;
    directory_tree::action(&mut a, Action::Refresh);
    settle(&mut a);
    assert!(!a
        .directory_tree
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join("new")));
    directory_tree::action(&mut a, Action::Refresh);
    key(&mut a, KeyCode::Esc);
    for _ in 0..10 {
        a.poll();
    }
    assert!(a.directory_tree.is_none());
    open(&mut a);
    assert_eq!(
        a.directory_tree.as_ref().unwrap().selected().unwrap().path,
        t.0
    );
    Ok(())
}
#[test]
fn keyboard_paging_buttons_help_and_resize() -> io::Result<()> {
    let t = Temp::new()?;
    for i in 0..40 {
        fs::create_dir(t.0.join(format!("dir{i:02}")))?;
    }
    let mut a = t.app()?;
    open(&mut a);
    for (w, h) in [(80, 25), (132, 42), (30, 10), (80, 25)] {
        draw(&mut a, w, h)?;
    }
    key(&mut a, KeyCode::End);
    let tree = a.directory_tree.as_ref().unwrap();
    assert_eq!(tree.cursor, tree.rows.len() - 1);
    assert!(tree.offset > 0);
    key(&mut a, KeyCode::PageUp);
    let tree = a.directory_tree.as_ref().unwrap();
    assert_eq!(tree.cursor, tree.rows.len() - 1 - tree.page_rows);
    key(&mut a, KeyCode::Home);
    assert_eq!(a.directory_tree.as_ref().unwrap().cursor, 0);
    key(&mut a, KeyCode::F(1));
    assert_eq!(focus::current(&a), Focus::Message);
    key(&mut a, KeyCode::Esc);
    assert_eq!(focus::current(&a), Focus::Tree);
    for _ in 0..4 {
        key(&mut a, KeyCode::Tab);
    }
    key(&mut a, KeyCode::Enter);
    assert!(a.directory_tree.is_none());
    Ok(())
}
#[test]
fn mouse_rows_expanders_wheel_and_close() -> io::Result<()> {
    let t = Temp::new()?;
    for i in 0..22 {
        fs::create_dir_all(t.0.join(format!("child{i:02}/nested")))?;
    }
    let mut a = t.app()?;
    open(&mut a);
    select(&mut a, &t.0.join("child00"));
    draw(&mut a, 80, 25)?;
    let index = a.directory_tree.as_ref().unwrap().cursor;
    click(&mut a, Target::Tree(Action::Toggle(index)));
    settle(&mut a);
    assert!(a
        .directory_tree
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join("child00/nested")));
    draw(&mut a, 80, 25)?;
    mouse::handle(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(a.directory_tree.as_ref().unwrap().cursor, index + 3);
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Tree(Action::Close));
    assert!(a.directory_tree.is_none());
    Ok(())
}
#[test]
fn mouse_double_click_opens_selected_folder() -> io::Result<()> {
    let t = Temp::new()?;
    let child = t.0.join("child");
    fs::create_dir(&child)?;
    let mut a = t.app()?;
    open(&mut a);
    select(&mut a, &child);
    draw(&mut a, 80, 25)?;
    let i = a.directory_tree.as_ref().unwrap().cursor;
    click(&mut a, Target::Tree(Action::Row(i)));
    click(&mut a, Target::Tree(Action::Row(i)));
    wait(&mut a, |a| !a.panels[0].loading);
    assert!(a.directory_tree.is_none());
    assert_eq!(a.panels[0].path, child);
    Ok(())
}
#[cfg(unix)]
#[test]
fn links_are_visible_leaves_and_cycles_never_expand() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let t = Temp::new()?;
    fs::create_dir(t.0.join("real"))?;
    fs::write(t.0.join("file"), b"x")?;
    symlink(&t.0, t.0.join("cycle"))?;
    symlink(t.0.join("missing"), t.0.join("broken"))?;
    symlink(t.0.join("file"), t.0.join("file-link"))?;
    let scan = directory_tree::scan(&t.0, true, &[], 100, &Cancellation::default())?;
    assert_eq!(scan.skipped, 1);
    assert_eq!(scan.children.len(), 2);
    assert!(scan
        .children
        .iter()
        .any(|r| r.path == t.0.join("cycle") && r.link));
    assert!(
        directory_tree::scan(&t.0.join("cycle"), true, &[], 100, &Cancellation::default()).is_err()
    );
    let mut a = t.app()?;
    open(&mut a);
    select(&mut a, &t.0.join("cycle"));
    let count = a.directory_tree.as_ref().unwrap().rows.len();
    key(&mut a, KeyCode::Right);
    settle(&mut a);
    assert_eq!(a.directory_tree.as_ref().unwrap().rows.len(), count);
    assert!(a.directory_tree.as_ref().unwrap().status().contains("link"));
    key(&mut a, KeyCode::Enter);
    wait(&mut a, |a| !a.panels[0].loading);
    assert_eq!(a.panels[0].path, t.0.join("cycle"));
    Ok(())
}
#[test]
fn hidden_current_ancestry_retained_and_missing_initial_error() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join(".hidden/deep");
    fs::create_dir_all(&p)?;
    let mut tree = Tree::new(0, p.clone(), false)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        tree.poll();
        if !tree.loading() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(tree.selected().unwrap().path, p);
    let mut tree = Tree::new(0, t.0.join("missing"), false)?;
    while tree.loading() {
        tree.poll();
        thread::sleep(Duration::from_millis(1));
    }
    assert!(tree.rows.is_empty());
    assert!(tree.status().contains("Cannot locate"));
    Ok(())
}

#[test]
fn menu_access_and_cancel_keep_marks_filter_layout_and_cursor() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir(t.0.join("child"))?;
    let mut a = t.app()?;
    a.panels[0].select_visible();
    a.panels[0].set_filter("child".into());
    let selected = a.panels[0].selected.clone();
    let cursor = a.panels[0].cursor;
    let layouts = a.config.layouts;
    {
        let root = 4;
        let item = dn::menu::items(root)
            .into_iter()
            .find(|i| i.action == dn::menu::Action::DirectoryTree)
            .unwrap();
        assert_eq!(item.shortcut, "Ctrl+T");
        dn::menu::execute(&mut a, item.action);
        settle(&mut a);
        key(&mut a, KeyCode::Esc);
        assert_eq!(a.panels[0].selected, selected);
        assert_eq!(a.panels[0].cursor, cursor);
        assert_eq!(a.config.layouts, layouts);
        assert_eq!(a.panels[0].filter, "child");
    }
    Ok(())
}
#[cfg(unix)]
#[test]
fn unreadable_branch_reports_error_and_can_refresh() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let t = Temp::new()?;
    let locked = t.0.join("locked");
    fs::create_dir(&locked)?;
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        }
    }
    let _restore = Restore(locked.clone());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))?;
    if fs::read_dir(&locked).is_ok() {
        return Ok(());
    }
    let mut a = t.app()?;
    open(&mut a);
    select(&mut a, &locked);
    key(&mut a, KeyCode::Right);
    settle(&mut a);
    assert!(a
        .directory_tree
        .as_ref()
        .unwrap()
        .status()
        .contains("Cannot read branch"));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700))?;
    fs::create_dir(locked.join("inside"))?;
    directory_tree::action(&mut a, Action::Refresh);
    settle(&mut a);
    assert!(a
        .directory_tree
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == locked.join("inside")));
    Ok(())
}

#[test]
fn background_sibling_loading_keeps_highlight_screen_position() -> io::Result<()> {
    let t = Temp::new()?;
    for i in 0..30 {
        fs::create_dir(t.0.join(format!("a{i:02}")))?;
    }
    let current = t.0.join("z-current");
    fs::create_dir(&current)?;
    let mut tree = Tree::new(0, current.clone(), false)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while tree.rows.is_empty() {
        tree.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    let visual = tree.cursor - tree.offset;
    while tree.loading() {
        tree.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(tree.selected().unwrap().path, current);
    assert_eq!(tree.cursor - tree.offset, visual);
    let scan = directory_tree::scan(
        &t.0,
        false,
        std::slice::from_ref(&current),
        1,
        &Cancellation::default(),
    )?;
    assert!(scan.limited);
    assert_eq!(scan.children.len(), 1);
    assert_eq!(scan.children[0].path, current);
    Ok(())
}
