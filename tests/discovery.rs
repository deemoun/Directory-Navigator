use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    app::App,
    discovery::{self, Action},
    focus::{self, Focus},
    input,
    menu::{self, Action as MenuAction},
    mouse::{self, Target},
    quick_view::QuickView,
    traversal::{Event, Job, Request},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-discovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(p))
    }
    fn app(&self) -> io::Result<App> {
        let mut a = App::new(self.0.clone())?;
        wait(&mut a, |a| a.panels.iter().all(|p| !p.loading));
        Ok(a)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(a: &mut App, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !ready(a) {
        a.poll();
        assert!(Instant::now() < deadline, "worker timeout");
        thread::sleep(Duration::from_millis(1));
    }
}
fn finish(a: &mut App) {
    wait(a, |a| a.discovery().is_some_and(|m| m.job.is_none()));
}
fn key(a: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    input::handle(a, KeyEvent::new(code, modifiers));
}
fn draw(a: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut t = Terminal::new(TestBackend::new(w, h))?;
    t.draw(|f| ui::draw(f, a))?;
    for (r, _) in &a.mouse.regions {
        assert!(r.right() <= w && r.bottom() <= h, "out of bounds {r:?}");
    }
    Ok(t.backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
fn focus(a: &mut App, name: &str) {
    a.panels[a.active].cursor = a.panels[a.active]
        .entries
        .iter()
        .position(|e| e.name == name)
        .unwrap();
}
fn click(a: &mut App, target: Target) {
    let r = a
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == target)
        .unwrap()
        .0;
    mouse::handle(
        a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}
fn events(request: Request) -> io::Result<Vec<Event>> {
    let job = Job::start(request)?;
    let mut events = Vec::new();
    loop {
        let event = job
            .events
            .recv_timeout(Duration::from_secs(8))
            .expect("worker reply");
        let done = matches!(event, Event::Done { .. });
        events.push(event);
        if done {
            return Ok(events);
        }
    }
}
fn find(
    root: PathBuf,
    pattern: &str,
    recursive: bool,
    hidden: bool,
) -> io::Result<Vec<dn::traversal::Hit>> {
    let events = events(Request::Find {
        root,
        pattern: pattern.into(),
        recursive,
        hidden,
    })?;
    Ok(events
        .into_iter()
        .flat_map(|e| {
            if let Event::Found(hits) = e {
                hits
            } else {
                Vec::new()
            }
        })
        .collect())
}

#[test]
fn find_unicode_patterns_folders_hidden_and_nonrecursive_scope() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir(t.0.join("dir.rs"))?;
    fs::create_dir(t.0.join(".hidden"))?;
    for path in [
        "文.rs",
        "dir.rs/deep.rs",
        ".hidden/secret.rs",
        ".dot.rs",
        "other.txt",
    ] {
        fs::write(t.0.join(path), "data")?;
    }
    let hits = find(t.0.clone(), "*.rs", true, false)?;
    assert_eq!(hits.len(), 3);
    assert!(hits
        .iter()
        .any(|h| h.path == t.0.join("dir.rs") && !h.regular));
    assert_eq!(find(t.0.clone(), "?.rs", true, false)?.len(), 1);
    assert_eq!(find(t.0.clone(), "*.rs", false, false)?.len(), 2);
    assert_eq!(find(t.0.clone(), "*.rs", true, true)?.len(), 5);
    assert!(find(t.0.clone(), "*.RS", true, true)?.is_empty());
    assert!(!hits.iter().any(|h| h.path.file_name().unwrap() == ".."));
    Ok(())
}

#[cfg(unix)]
#[test]
fn traversal_never_follows_directory_links_and_sizes_count_hard_links_separately() -> io::Result<()>
{
    use std::os::unix::fs::symlink;
    let t = Temp::new()?;
    let root = t.0.join("root");
    fs::create_dir(&root)?;
    fs::write(t.0.join("outside"), "secret")?;
    let file = root.join("data");
    fs::write(&file, "12345")?;
    fs::hard_link(&file, root.join("hard"))?;
    fs::create_dir(root.join("nested"))?;
    symlink(&root, root.join("cycle"))?;
    symlink(t.0.join("outside"), root.join("link"))?;
    symlink("missing", root.join("broken"))?;
    let link_bytes = ["cycle", "link", "broken"]
        .iter()
        .map(|n| fs::symlink_metadata(root.join(n)).unwrap().len() as u128)
        .sum::<u128>();
    let hits = find(root.clone(), "*", true, true)?;
    assert_eq!(hits.len(), 6);
    let result = events(Request::Sizes(vec![root]))?
        .into_iter()
        .find_map(|e| {
            if let Event::Size(s) = e {
                Some(s)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(result.bytes, 10 + link_bytes);
    assert_eq!((result.files, result.directories, result.links), (2, 1, 3));
    assert!(!result.partial);
    Ok(())
}

#[test]
fn traversal_missing_roots_and_cancelled_size_snapshots_report_partial() -> io::Result<()> {
    let t = Temp::new()?;
    let result = events(Request::Sizes(vec![t.0.join("missing")]))?;
    assert!(result
        .iter()
        .any(|e| matches!(e,Event::Size(s)if s.partial&&s.skipped==1)));
    assert!(result.iter().any(
        |e| matches!(e,Event::Done{stats,..}if stats.skipped==1&&stats.first_error.is_some())
    ));
    for i in 0..1000 {
        fs::write(t.0.join(format!("file{i}")), "")?;
    }
    let job = Job::start(Request::Find {
        root: t.0.clone(),
        pattern: "*".into(),
        recursive: true,
        hidden: true,
    })?;
    job.stop();
    loop {
        let e = job.events.recv_timeout(Duration::from_secs(5)).unwrap();
        if let Event::Done { cancelled, .. } = e {
            assert!(cancelled);
            break;
        }
    }
    Ok(())
}

#[test]
fn find_limit_is_bounded_and_close_unblocks_full_delivery_queue() -> io::Result<()> {
    let t = Temp::new()?;
    for i in 0..10_020 {
        fs::write(t.0.join(format!("f{i}")), "")?;
    }
    let result = events(Request::Find {
        root: t.0.clone(),
        pattern: "*".into(),
        recursive: true,
        hidden: true,
    })?;
    let count = result
        .iter()
        .map(|e| if let Event::Found(h) = e { h.len() } else { 0 })
        .sum::<usize>();
    assert_eq!(count, 10_000);
    assert!(result
        .iter()
        .any(|e| matches!(e, Event::Done { limited: true, .. })));
    let stopped = Job::start(Request::Find {
        root: t.0.clone(),
        pattern: "*".into(),
        recursive: true,
        hidden: true,
    })?;
    let mut retained = loop {
        match stopped.events.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::Found(hits) => break hits.len(),
            _ => continue,
        }
    };
    stopped.stop();
    loop {
        match stopped.events.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::Found(hits) => retained += hits.len(),
            Event::Done { cancelled, .. } => {
                assert!(cancelled);
                break;
            }
            _ => {}
        }
    }
    assert!(retained >= 64);
    let job = Job::start(Request::Find {
        root: t.0.clone(),
        pattern: "*".into(),
        recursive: true,
        hidden: true,
    })?;
    thread::sleep(Duration::from_millis(30));
    drop(job);
    assert_eq!(find(t.0.clone(), "no-match", false, false)?.len(), 0);
    Ok(())
}

#[test]
fn find_dialog_shortcuts_memory_modal_precedence_and_result_jump() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir(t.0.join("nested"))?;
    fs::write(t.0.join("nested/文.txt"), "hello")?;
    let mut a = t.app()?;
    key(&mut a, KeyCode::F(7), KeyModifiers::ALT);
    assert!(a.modals.quick_search().is_some());
    key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut a, KeyCode::F(7), KeyModifiers::CONTROL);
    assert_eq!(focus::current(&a), Focus::Discovery);
    for (w, h) in [(80, 25), (132, 40)] {
        assert!(draw(&mut a, w, h)?.contains("Find files"));
    }
    input::paste(&mut a, "*.txt");
    a.error("top");
    input::paste(&mut a, "must not leak");
    assert_eq!(a.discovery().unwrap().prefs.pattern, "*.txt");
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
    finish(&mut a);
    assert_eq!(a.discovery().unwrap().hits.len(), 1);
    for (w, h) in [(80, 25), (132, 40)] {
        assert!(draw(&mut a, w, h)?.contains("Find results"));
    }
    let find = a.desktop.active().id;
    key(&mut a, KeyCode::F(3), KeyModifiers::NONE);
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.contents.is_some())
    });
    assert!(a.desktop.tool(find).unwrap().discovery().is_some());
    key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Discovery(Action::Jump));
    wait(&mut a, |a| !a.panels[0].loading);
    assert_eq!(a.panels[0].path, t.0.join("nested"));
    assert_eq!(a.panels[0].entries[a.panels[0].cursor].name, "文.txt");
    discovery::open_find(&mut a);
    assert_eq!(a.discovery().unwrap().prefs.pattern, "*.txt");
    key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
    assert!(a.discovery().is_none());
    Ok(())
}

#[test]
fn size_dialog_selects_visible_directories_retains_listing_and_recalculates() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir(t.0.join("dir"))?;
    fs::write(t.0.join("dir/data"), "12345")?;
    fs::write(t.0.join("file"), "")?;
    let mut a = t.app()?;
    focus(&mut a, "file");
    discovery::open_sizes(&mut a);
    assert!(a.discovery().is_none());
    a.clear_dialogs();
    focus(&mut a, "dir");
    a.panels[0].toggle_selection();
    let cursor = a.panels[0].cursor;
    let selected = a.panels[0].selected.clone();
    let sort = a.panels[0].sort;
    let layouts = a.config.layouts;
    menu::open(&mut a, 3);
    draw(&mut a, 80, 25)?;
    let row = menu::items(3)
        .iter()
        .position(|i| i.action == MenuAction::DirectorySizes)
        .unwrap();
    click(&mut a, Target::MenuItem(0, row));
    finish(&mut a);
    assert_eq!(a.discovery().unwrap().sizes[0].bytes, 5);
    for (w, h) in [(80, 25), (132, 40)] {
        assert!(draw(&mut a, w, h)?.contains("Directory sizes"));
    }
    fs::write(t.0.join("dir/data"), "123456789")?;
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Discovery(Action::Recalculate));
    finish(&mut a);
    assert_eq!(a.discovery().unwrap().sizes[0].bytes, 9);
    discovery::action(&mut a, Action::Close);
    assert_eq!(a.panels[0].cursor, cursor);
    assert_eq!(a.panels[0].selected, selected);
    assert_eq!(a.panels[0].sort, sort);
    assert_eq!(a.config.layouts, layouts);
    Ok(())
}

#[test]
fn quick_view_debounce_stale_selection_and_panel_restoration() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("a"), "first")?;
    fs::write(t.0.join("b"), "second\nline\nline\nline\nline")?;
    let mut a = t.app()?;
    focus(&mut a, "a");
    let saved = (
        a.panels[1].path.clone(),
        a.panels[1].cursor,
        a.panels[1].offset,
        a.panels[1].selected.clone(),
    );
    key(&mut a, KeyCode::Char('q'), KeyModifiers::CONTROL);
    let start = Instant::now();
    let q = a.quick_view.as_mut().unwrap();
    q.poll(&a.panels[0], start);
    assert!(q.viewer.is_none());
    q.poll(&a.panels[0], start + Duration::from_millis(149));
    assert!(q.viewer.is_none());
    focus(&mut a, "b");
    let q = a.quick_view.as_mut().unwrap();
    q.poll(&a.panels[0], start + Duration::from_millis(160));
    q.poll(&a.panels[0], start + Duration::from_millis(320));
    wait(&mut a, |a| {
        a.quick_view
            .as_ref()
            .and_then(|q| q.viewer.as_ref())
            .is_some_and(|v| v.contents.is_some())
    });
    let q = a.quick_view.as_ref().unwrap();
    assert_eq!(q.path(), Some(t.0.join("b").as_path()));
    assert_eq!(
        q.viewer.as_ref().unwrap().contents.as_ref().unwrap().lines[0],
        "second"
    );
    for (w, h) in [(80, 25), (132, 40)] {
        assert!(draw(&mut a, w, h)?.contains("Quick view"));
    }
    assert!(a
        .file_tools
        .histories
        .get(dn::file_tools::history::HistoryKind::View)
        .is_empty());
    let rect = a
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::QuickPreview)
        .unwrap()
        .0;
    mouse::handle(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(
        a.quick_view
            .as_ref()
            .unwrap()
            .viewer
            .as_ref()
            .unwrap()
            .offset,
        3
    );
    assert_eq!(a.active, 0);
    click(&mut a, Target::CloseQuickView);
    assert!(a.quick_view.is_none());
    assert_eq!(
        (
            a.panels[1].path.clone(),
            a.panels[1].cursor,
            a.panels[1].offset,
            a.panels[1].selected.clone()
        ),
        saved
    );
    key(&mut a, KeyCode::Char('q'), KeyModifiers::CONTROL);
    key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
    assert!(a.quick_view.is_none());
    assert_eq!(a.active, 1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn quick_view_links_show_information_and_preview_reader_refuses_final_links() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let t = Temp::new()?;
    fs::write(t.0.join("secret"), "SHOULD_NOT_PREVIEW")?;
    symlink("secret", t.0.join("link"))?;
    symlink("absent", t.0.join("broken"))?;
    let mut a = t.app()?;
    focus(&mut a, "link");
    a.quick_view = Some(QuickView::new(0));
    wait(&mut a, |a| {
        a.quick_view
            .as_ref()
            .is_some_and(|q| q.lines.iter().any(|l| l.contains("target not previewed")))
    });
    assert!(a.quick_view.as_ref().unwrap().viewer.is_none());
    assert!(!draw(&mut a, 80, 25)?.contains("SHOULD_NOT_PREVIEW"));
    focus(&mut a, "broken");
    wait(&mut a, |a| {
        a.quick_view
            .as_ref()
            .is_some_and(|q| q.lines.iter().any(|l| l == "Target: absent"))
    });
    let mut viewer = dn::viewer::Viewer::open_preview(t.0.join("link"))?;
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(result) = viewer.poll() {
            assert!(result.is_err());
            break;
        }
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

#[test]
fn hidden_results_jump_clears_filter_and_reveals_the_result() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join(".hidden.txt"), "")?;
    let mut a = t.app()?;
    a.toggle_hidden();
    wait(&mut a, |a| !a.panels[0].loading);
    a.panels[0].set_filter("*.rs".into());
    discovery::open_find(&mut a);
    a.discovery_mut().unwrap().prefs.hidden = true;
    discovery::action(&mut a, Action::Start);
    finish(&mut a);
    let m = a.discovery_mut().unwrap();
    m.cursor = m
        .hits
        .iter()
        .position(|h| h.path.file_name().unwrap() == ".hidden.txt")
        .unwrap();
    discovery::action(&mut a, Action::Jump);
    wait(&mut a, |a| !a.panels[0].loading);
    assert!(a.config.show_hidden);
    assert!(a.panels[0].filter.is_empty());
    assert_eq!(a.panels[0].entries[a.panels[0].cursor].name, ".hidden.txt");
    Ok(())
}

#[test]
fn small_terminal_degrades_safely_for_all_new_modes() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    discovery::open_find(&mut a);
    assert!(draw(&mut a, 40, 12)?.contains("Terminal too small"));
    discovery::action(&mut a, Action::Start);
    finish(&mut a);
    draw(&mut a, 80, 25)?;
    discovery::action(&mut a, Action::Close);
    a.quick_view = Some(QuickView::new(0));
    draw(&mut a, 40, 12)?;
    draw(&mut a, 80, 25)?;
    assert_eq!(a.panels[0].path, t.0);
    Ok(())
}

#[cfg(unix)]
#[test]
fn inaccessible_subtrees_report_errors_without_aborting_other_results() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return Ok(());
    }
    let t = Temp::new()?;
    let locked = t.0.join("locked");
    fs::create_dir(&locked)?;
    fs::write(locked.join("secret"), "secret")?;
    fs::write(t.0.join("visible"), "")?;
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        }
    }
    let _restore = Restore(locked.clone());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))?;
    let results = events(Request::Find {
        root: t.0.clone(),
        pattern: "*".into(),
        recursive: true,
        hidden: true,
    })?;
    assert!(results
        .iter()
        .any(|e| matches!(e,Event::Done{stats,..}if stats.skipped>0&&stats.first_error.is_some())));
    assert!(results
        .iter()
        .any(|e| matches!(e,Event::Found(hits)if hits.iter().any(|h|h.path==t.0.join("visible")))));
    let results = events(Request::Sizes(vec![locked]))?;
    assert!(results
        .iter()
        .any(|e| matches!(e,Event::Size(s)if s.partial&&s.skipped>0)));
    Ok(())
}

#[test]
fn quick_view_modal_precedence_full_viewer_and_disappearing_file() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("file"), "preview")?;
    let mut a = t.app()?;
    focus(&mut a, "file");
    key(&mut a, KeyCode::Char('q'), KeyModifiers::CONTROL);
    wait(&mut a, |a| {
        a.quick_view
            .as_ref()
            .and_then(|q| q.viewer.as_ref())
            .is_some_and(|v| v.contents.is_some())
    });
    key(&mut a, KeyCode::F(3), KeyModifiers::NONE);
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.contents.is_some())
    });
    assert!(a.quick_view.is_some());
    key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
    a.error("modal stays above preview");
    draw(&mut a, 80, 25)?;
    assert_eq!(focus::current(&a), Focus::Message);
    key(&mut a, KeyCode::Char('q'), KeyModifiers::CONTROL);
    assert!(a.quick_view.is_some());
    key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
    a.quick_view = Some(QuickView::new(0));
    fs::remove_file(t.0.join("file"))?;
    wait(&mut a, |a| {
        a.quick_view
            .as_ref()
            .is_some_and(|q| q.lines.iter().any(|l| l.contains("Preview unavailable")))
    });
    assert!(a.dialogs().is_empty());
    Ok(())
}
