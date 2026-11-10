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
    wait(app, |a| a.directory_tree().is_some_and(|t| !t.loading()));
}
fn key(app: &mut App, code: KeyCode) {
    input::handle(app, KeyEvent::new(code, KeyModifiers::NONE));
}
fn select(app: &mut App, path: &Path) {
    let t = app.directory_tree_mut().unwrap();
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
    let tree = a.directory_tree().unwrap();
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
        .directory_tree()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join("child/grandchild")));
    key(&mut a, KeyCode::Char(' '));
    assert!(!a
        .directory_tree()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join("child/grandchild")));
    key(&mut a, KeyCode::Right);
    settle(&mut a);
    assert_eq!(
        a.directory_tree().unwrap().selected().unwrap().path,
        t.0.join("child")
    );
    assert_eq!(a.panels[0].path, original);
    key(&mut a, KeyCode::Esc);
    assert!(a.directory_tree().is_none());
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
    select(&mut a, &t.0);
    directory_tree::action(&mut a, Action::Refresh);
    settle(&mut a);
    select(&mut a, &other);
    fs::remove_dir(&other)?;
    key(&mut a, KeyCode::Enter);
    wait(&mut a, |a| !a.panels[0].loading);
    assert_eq!(a.panels[0].path, child);
    assert!(!a.dialogs().is_empty());
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
        .directory_tree()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join(".hidden")));
    directory_tree::action(&mut a, Action::Hidden);
    settle(&mut a);
    assert!(a
        .directory_tree()
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
        .directory_tree()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join("new")));
    fs::remove_dir(t.0.join("new"))?;
    directory_tree::action(&mut a, Action::Refresh);
    settle(&mut a);
    assert!(!a
        .directory_tree()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join("new")));
    directory_tree::action(&mut a, Action::Refresh);
    key(&mut a, KeyCode::Esc);
    for _ in 0..10 {
        a.poll();
    }
    assert!(a.directory_tree().is_none());
    open(&mut a);
    assert_eq!(a.directory_tree().unwrap().selected().unwrap().path, t.0);
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
    let tree = a.directory_tree().unwrap();
    assert_eq!(tree.cursor, tree.rows.len() - 1);
    assert!(tree.offset > 0);
    key(&mut a, KeyCode::PageUp);
    let tree = a.directory_tree().unwrap();
    assert_eq!(tree.cursor, tree.rows.len() - 1 - tree.page_rows);
    key(&mut a, KeyCode::Home);
    assert_eq!(a.directory_tree().unwrap().cursor, 0);
    key(&mut a, KeyCode::F(1));
    assert_eq!(focus::current(&a), Focus::Message);
    key(&mut a, KeyCode::Esc);
    assert_eq!(focus::current(&a), Focus::Tree);
    for _ in 0..6 {
        key(&mut a, KeyCode::Tab);
    }
    assert_eq!(a.directory_tree().unwrap().focus, 4);
    key(&mut a, KeyCode::Enter);
    assert!(a.directory_tree().is_none());
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
    let index = a.directory_tree().unwrap().cursor;
    click(&mut a, Target::Tree(Action::Toggle(index)));
    settle(&mut a);
    assert!(a
        .directory_tree()
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
    assert_eq!(a.directory_tree().unwrap().cursor, index + 3);
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Tree(Action::Close));
    assert!(a.directory_tree().is_none());
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
    let i = a.directory_tree().unwrap().cursor;
    // Deliver the two presses in the same prepared frame. Painting a whole
    // terminal between them can exceed the actual double-click interval on CI;
    // redraw stability and the tracker's exact timing boundaries have separate tests.
    click(&mut a, Target::Tree(Action::Row(i)));
    click(&mut a, Target::Tree(Action::Row(i)));
    wait(&mut a, |a| !a.panels[0].loading);
    assert!(a.directory_tree().is_none());
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
    let count = a.directory_tree().unwrap().rows.len();
    key(&mut a, KeyCode::Right);
    settle(&mut a);
    assert_eq!(a.directory_tree().unwrap().rows.len(), count);
    assert!(a.directory_tree().unwrap().status().contains("link"));
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
        .directory_tree()
        .unwrap()
        .status()
        .contains("Cannot read branch"));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700))?;
    fs::create_dir(locked.join("inside"))?;
    directory_tree::action(&mut a, Action::Refresh);
    settle(&mut a);
    assert!(a
        .directory_tree()
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

#[test]
fn usage_is_explicit_includes_hidden_and_sorts_siblings_without_losing_selection() -> io::Result<()>
{
    let t = Temp::new()?;
    for (name, bytes) in [("a-small", 3), ("z-large", 30), (".hidden", 7)] {
        fs::create_dir(t.0.join(name))?;
        fs::write(t.0.join(name).join("данные"), vec![b'x'; bytes])?;
    }
    fs::create_dir_all(t.0.join("z-large/nested"))?;
    fs::write(t.0.join("direct"), b"12345")?;
    let mut app = t.app()?;
    app.config.show_hidden = false;
    open(&mut app);
    assert!(app.directory_tree().unwrap().usage.root.is_none());
    assert!(!app
        .directory_tree()
        .unwrap()
        .rows
        .iter()
        .any(|r| r.path == t.0.join(".hidden")));
    key(&mut app, KeyCode::F(5));
    assert!(app.directory_tree().unwrap().usage.running());
    key(&mut app, KeyCode::F(6)); // Ordering is requested but deferred until completion.
    assert_eq!(app.directory_tree().unwrap().selected().unwrap().path, t.0);
    wait(&mut app, |a| !a.directory_tree().unwrap().usage.running());
    let tree = app.directory_tree().unwrap();
    let result = &tree.usage.records[&t.0];
    assert_eq!(
        (result.bytes, result.direct_bytes, result.files),
        (45, 5, 4)
    );
    assert!(!result.partial);
    assert_eq!(tree.usage.records[&t.0.join(".hidden")].bytes, 7);
    let siblings = tree
        .rows
        .iter()
        .filter(|r| r.path.parent() == Some(t.0.as_path()))
        .map(|r| r.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(siblings, ["z-large", "a-small"]);
    let selected = tree.selected().unwrap().path.clone();
    let offset = tree.offset;
    key(&mut app, KeyCode::F(6));
    assert_eq!(
        app.directory_tree().unwrap().selected().unwrap().path,
        selected
    );
    assert_eq!(app.directory_tree().unwrap().offset, offset);
    key(&mut app, KeyCode::F(6));
    key(&mut app, KeyCode::Right);
    assert_eq!(
        app.directory_tree().unwrap().selected().unwrap().path,
        t.0.join("z-large")
    );
    directory_tree::action(&mut app, Action::Hidden);
    settle(&mut app);
    assert_eq!(app.directory_tree().unwrap().usage.records[&t.0].bytes, 45);
    assert!(!app.directory_tree().unwrap().usage.stale);
    directory_tree::action(&mut app, Action::Refresh);
    assert!(app.directory_tree().unwrap().usage.stale);
    assert!(app
        .directory_tree()
        .unwrap()
        .usage
        .summary(&t.0, dn::localization::Language::English)
        .contains("Stale"));
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].path, t.0.join("z-large"));
    assert_eq!(app.panels[1].path, t.0);
    Ok(())
}

#[test]
fn usage_cancel_restart_close_and_failed_capacity_preserve_navigation() -> io::Result<()> {
    let t = Temp::new()?;
    for name in ["first", "second"] {
        fs::create_dir(t.0.join(name))?;
        fs::write(t.0.join(name).join("file"), name)?;
    }
    let mut app = t.app()?;
    open(&mut app);
    select(&mut app, &t.0.join("first"));
    key(&mut app, KeyCode::F(5));
    key(&mut app, KeyCode::Esc);
    assert!(app.directory_tree().is_some());
    assert!(app.directory_tree().unwrap().usage.stopping());
    wait(&mut app, |a| !a.directory_tree().unwrap().usage.running());
    assert_eq!(
        app.directory_tree().unwrap().usage.state,
        "Stopped — partial"
    );
    select(&mut app, &t.0.join("second"));
    key(&mut app, KeyCode::F(5));
    let tree_id = app.desktop.active().id;
    input::handle(&mut app, KeyEvent::new(KeyCode::F(6), KeyModifiers::ALT));
    wait(&mut app, |a| {
        !a.desktop
            .tool(tree_id)
            .unwrap()
            .tree()
            .unwrap()
            .usage
            .running()
    });
    assert_eq!(app.desktop.active().id, dn::desktop::WindowId::MANAGER);
    input::handle(&mut app, KeyEvent::new(KeyCode::F(6), KeyModifiers::ALT));
    assert_eq!(app.desktop.active().id, tree_id);
    let usage = &app.directory_tree().unwrap().usage;
    assert_eq!(usage.root.as_deref(), Some(t.0.join("second").as_path()));
    assert!(!usage.records.contains_key(&t.0.join("first")));
    app.directory_tree_mut().unwrap().capacity.result = Some(Err("private capacity error".into()));
    draw(&mut app, 80, 25)?;
    assert!(app
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Tree(Action::Open)));
    key(&mut app, KeyCode::F(5));
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Esc);
    assert!(app.directory_tree().is_none());
    for _ in 0..3 {
        app.poll();
    }
    assert_eq!(app.panels[0].path, t.0);
    open(&mut app);
    assert!(app.directory_tree().unwrap().usage.root.is_none());
    Ok(())
}

#[test]
fn usage_controls_localized_mouse_keyboard_custom_colors_resize_and_immutable_frames(
) -> io::Result<()> {
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        let t = Temp::new()?;
        fs::create_dir(t.0.join("Ёж e\u{301} 界"))?;
        fs::write(t.0.join("Ёж e\u{301} 界/file"), b"data")?;
        let mut app = t.app()?;
        app.config.language = language;
        app.config.palette = dn::palette::PaletteProfile::ClassicDn
            .with_slot(dn::palette::theme::Slot::DialogText, 0x2e);
        open(&mut app);
        draw(&mut app, 80, 25)?;
        click(&mut app, Target::Tree(Action::Analyze));
        wait(&mut app, |a| !a.directory_tree().unwrap().usage.running());
        for (w, h) in [(80, 25), (132, 42), (30, 10), (80, 25)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h))?;
            terminal.draw(|f| ui::draw(f, &mut app))?;
            let first = terminal.backend().buffer().clone();
            for _ in 0..3 {
                terminal.draw(|f| ui::draw(f, &mut app))?;
                assert_eq!(*terminal.backend().buffer(), first);
            }
            assert!(app
                .mouse
                .regions
                .iter()
                .all(|(r, _)| r.right() <= w && r.bottom() <= h));
            if w >= 80 {
                let text = first.content.iter().map(|c| c.symbol()).collect::<String>();
                assert!(text.contains(language.text("Order: Name")));
                assert!(
                    text.contains(language.text("Apparent size")),
                    "{language:?} {w}x{h}: {text}"
                );
                assert!(text.contains("4 B"));
            }
        }
        click(&mut app, Target::Tree(Action::Order));
        assert!(app.directory_tree().unwrap().usage.size_order);
        key(&mut app, KeyCode::F(1));
        assert_eq!(focus::current(&app), Focus::Message);
        key(&mut app, KeyCode::Esc);
        assert_eq!(focus::current(&app), Focus::Tree);
        draw(&mut app, 80, 25)?;
        click(&mut app, Target::Tree(Action::Close));
        assert!(app.directory_tree().is_none());
    }
    Ok(())
}
