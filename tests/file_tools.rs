use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::{
    app::App,
    editor::{self, EditorAction},
    file_tools::{
        self,
        compare::{self, CompareOptions},
        export,
        history::{Histories, HistoryKind},
        ToolAction, ToolDialog,
    },
    filesystem::Entry,
    input, menu,
    mouse::{self, Target},
    ui,
};
use ratatui::{backend::TestBackend, Terminal};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-tools-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
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
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.poll();
        assert!(Instant::now() < deadline, "background timeout");
        thread::sleep(Duration::from_millis(1));
    }
}
fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    input::handle(app, KeyEvent::new(code, modifiers));
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h))?;
    terminal.draw(|f| ui::draw(f, app))?;
    for (r, _) in &app.mouse.regions {
        assert!(r.right() <= w && r.bottom() <= h);
    }
    Ok(terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
fn click(app: &mut App, target: Target) {
    let r = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == target)
        .expect("mouse target")
        .0;
    mouse::handle(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}
fn entry(name: &str, size: Option<u64>) -> Entry {
    Entry {
        file_type: Some(
            fs::metadata(std::env::current_exe().unwrap())
                .unwrap()
                .file_type(),
        ),
        name: name.into(),
        path: PathBuf::from(name),
        directory: false,
        symlink: false,
        size,
        parent: false,
        modified: Some(UNIX_EPOCH),
        executable: false,
    }
}
#[test]
fn history_recency_deduplication_limits_and_native_paths() {
    let mut histories = Histories::default();
    for n in 0..105 {
        histories.record(HistoryKind::Directory, PathBuf::from(format!("dir{n}")));
    }
    assert_eq!(histories.get(HistoryKind::Directory).len(), 100);
    assert_eq!(
        histories.get(HistoryKind::Directory)[0],
        PathBuf::from("dir104")
    );
    histories.record(HistoryKind::Directory, PathBuf::from("dir20"));
    assert_eq!(
        histories.get(HistoryKind::Directory)[0],
        PathBuf::from("dir20")
    );
    assert_eq!(histories.get(HistoryKind::Directory).len(), 100);
    histories.record(HistoryKind::Edit, PathBuf::from("编辑.rs"));
    assert!(histories.get(HistoryKind::View).is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let native = PathBuf::from(OsString::from_vec(vec![b'x', 255]));
        histories.record(HistoryKind::View, native.clone());
        assert_eq!(histories.get(HistoryKind::View), [native]);
    }
}
#[test]
fn directory_history_records_success_not_refresh_or_failure() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir(t.0.join("子"))?;
    let mut app = t.app()?;
    app.panels[0].go_to(t.0.join("子"), false);
    wait(&mut app, |a| !a.panels[0].loading);
    let expected = vec![t.0.join("子"), t.0.clone()];
    assert_eq!(
        app.file_tools.histories.get(HistoryKind::Directory),
        expected
    );
    app.refresh();
    wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
    assert_eq!(
        app.file_tools.histories.get(HistoryKind::Directory),
        expected
    );
    app.panels[0].go_to(t.0.join("missing"), false);
    wait(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].path, t.0.join("子"));
    assert_eq!(
        app.file_tools.histories.get(HistoryKind::Directory),
        expected
    );
    Ok(())
}
#[test]
fn viewed_and_edited_histories_record_only_success_and_guard_open() -> io::Result<()> {
    let t = Temp::new()?;
    let a = t.0.join("alpha.txt");
    let b = t.0.join("beta.txt");
    fs::write(&a, "alpha")?;
    fs::write(&b, "beta")?;
    let mut app = t.app()?;
    app.file_tools
        .histories
        .record(HistoryKind::View, a.clone());
    file_tools::open_history(&mut app, HistoryKind::View);
    file_tools::accept(&mut app);
    wait(&mut app, |a| {
        a.viewer.as_ref().is_some_and(|v| v.contents.is_some())
    });
    assert_eq!(app.file_tools.histories.get(HistoryKind::View), [a.clone()]);
    app.viewer = None;
    editor::open_path(&mut app, a.clone());
    wait(&mut app, |a| a.editor.as_ref().is_some_and(|e| !e.busy()));
    assert_eq!(app.file_tools.histories.get(HistoryKind::Edit), [a.clone()]);
    editor::paste(&mut app, "changed");
    editor::open_path(&mut app, b.clone());
    assert!(matches!(
        app.editor.as_ref().unwrap().modal,
        Some(editor::Modal::ConfirmClose)
    ));
    editor::action(&mut app, EditorAction::Cancel);
    assert_eq!(app.editor.as_ref().unwrap().path.as_ref(), Some(&a));
    editor::open_path(&mut app, t.0.join("missing"));
    editor::action(&mut app, EditorAction::Discard);
    wait(&mut app, |a| a.editor.as_ref().is_some_and(|e| !e.busy()));
    assert!(app.editor.as_ref().unwrap().dirty());
    assert_eq!(app.file_tools.histories.get(HistoryKind::Edit), [a.clone()]);
    app.dialogs.clear();
    editor::open_path(&mut app, b.clone());
    editor::action(&mut app, EditorAction::Discard);
    wait(&mut app, |a| a.editor.as_ref().is_some_and(|e| !e.busy()));
    assert_eq!(
        app.file_tools.histories.get(HistoryKind::Edit),
        [b.clone(), a]
    );
    editor::action(&mut app, EditorAction::SaveAs);
    let saved = t.0.join("saved.txt");
    if let Some(editor::Modal::Prompt { field, cursor, .. }) =
        &mut app.editor.as_mut().unwrap().modal
    {
        *field = saved.to_string_lossy().into_owned();
        *cursor = field.len();
    } else {
        panic!("save as dialog")
    }
    editor::action(&mut app, EditorAction::Submit);
    wait(&mut app, |a| a.editor.as_ref().is_some_and(|e| !e.busy()));
    assert_eq!(app.file_tools.histories.get(HistoryKind::Edit)[0], saved);
    app.editor = None;
    app.dialogs.clear();
    app.file_tools
        .histories
        .record(HistoryKind::View, t.0.join("missing"));
    file_tools::open_history(&mut app, HistoryKind::View);
    file_tools::accept(&mut app);
    wait(&mut app, |a| a.viewer.is_none());
    assert!(!app.dialogs.is_empty());
    assert_eq!(
        app.file_tools.histories.get(HistoryKind::View)[0],
        t.0.join("missing")
    );
    Ok(())
}
#[test]
fn history_dialog_navigation_mouse_resize_and_empty_state() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    key(&mut app, KeyCode::PageUp, KeyModifiers::ALT);
    assert!(draw(&mut app, 80, 25)?.contains("No history"));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    for n in 0..35 {
        app.file_tools.histories.record(
            HistoryKind::Directory,
            t.0.join(format!("long path {n:03} {}", "x".repeat(120))),
        );
    }
    key(&mut app, KeyCode::Backspace, KeyModifiers::ALT);
    draw(&mut app, 80, 25)?;
    key(&mut app, KeyCode::End, KeyModifiers::NONE);
    draw(&mut app, 80, 25)?;
    assert!(
        matches!(app.file_tools.dialog,Some(ToolDialog::History {cursor:35,offset,..}) if offset>0)
    );
    draw(&mut app, 130, 40)?;
    if let Some(ToolDialog::History { cursor, offset, .. }) = &app.file_tools.dialog {
        assert!(*cursor >= *offset);
    }
    key(&mut app, KeyCode::Home, KeyModifiers::NONE);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Tool(ToolAction::HistoryRow(0)));
    assert!(app.file_tools.dialog.is_some());
    click(&mut app, Target::Tool(ToolAction::HistoryRow(0)));
    wait(&mut app, |a| !a.panels[0].loading);
    assert!(!app.dialogs.is_empty());
    assert_eq!(app.panels[0].path, t.0);
    Ok(())
}
#[test]
fn comparison_options_types_and_unknown_metadata() {
    let left = vec![
        entry("equal", Some(1)),
        entry("size", Some(2)),
        entry("date", Some(1)),
        entry("unknown", None),
        entry("only-left", Some(1)),
        entry("type", Some(0)),
    ];
    let mut right = vec![
        entry("equal", Some(1)),
        entry("size", Some(3)),
        entry("date", Some(1)),
        entry("unknown", Some(1)),
        entry("only-right", Some(1)),
        entry("type", Some(0)),
    ];
    right[2].modified = Some(UNIX_EPOCH + Duration::from_secs(1));
    right[5].symlink = true;
    let result = compare::compare(&left, &right, CompareOptions::default());
    assert_eq!(
        (
            result.left_only,
            result.right_only,
            result.different,
            result.indeterminate
        ),
        (1, 1, 3, 1)
    );
    assert!(result.equivalent[0].contains(&OsString::from("equal")));
    let result = compare::compare(
        &left,
        &right,
        CompareOptions {
            size: false,
            time: false,
            unselect: false,
        },
    );
    assert_eq!((result.different, result.indeterminate), (1, 0));
    assert_eq!(result.equivalent[0].len(), 4);
    right[5].directory = true;
    assert!(
        compare::compare(&left, &right, CompareOptions::default()).differences[0]
            .contains(&OsString::from("type"))
    );
    assert!(
        !compare::compare(&left, &right, CompareOptions::default()).differences[1]
            .contains(&OsString::from("type"))
    );
}
#[test]
fn comparison_marks_cancellation_shortcuts_and_mouse() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir(t.0.join("left"))?;
    fs::create_dir(t.0.join("right"))?;
    for side in ["left", "right"] {
        fs::write(t.0.join(side).join("equal"), "a")?;
        fs::write(
            t.0.join(side).join("different"),
            if side == "left" { "a" } else { "bbb" },
        )?;
        fs::write(t.0.join(side).join(".hidden"), "h")?;
    }
    let mut app = t.app()?;
    for (i, side) in ["left", "right"].iter().enumerate() {
        app.panels[i].go_to(t.0.join(side), false);
    }
    wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
    app.file_tools.comparison.time = false;
    app.panels[0].selected.insert("equal".into());
    let cursors = [app.panels[0].cursor, app.panels[1].cursor];
    let layouts = app.config.layouts;
    key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(!app.quit);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.panels[0].selected.contains(&OsString::from("equal")));
    key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Tool(ToolAction::Accept));
    assert_eq!(
        app.panels[0].selected,
        BTreeSet::from([OsString::from("different")])
    );
    assert_eq!([app.panels[0].cursor, app.panels[1].cursor], cursors);
    assert_eq!(app.config.layouts, layouts);
    assert!(!app.panels[0].entries.iter().any(|e| e.name == ".hidden"));
    app.dialogs.clear();
    app.panels[0].selected.insert("equal".into());
    file_tools::open_compare(&mut app);
    draw(&mut app, 120, 35)?;
    click(&mut app, Target::Tool(ToolAction::Choice(3)));
    click(&mut app, Target::Tool(ToolAction::Accept));
    assert_eq!(
        app.panels[0].selected,
        BTreeSet::from([OsString::from("different")])
    );
    app.dialogs.clear();
    app.panels[0].refresh(false);
    file_tools::open_compare(&mut app);
    assert!(app.file_tools.dialog.is_none());
    assert!(!app.dialogs.is_empty());
    Ok(())
}
#[test]
fn comparison_native_names_and_directory_exclusion() {
    let mut dir = entry("folder", Some(0));
    dir.directory = true;
    let mut parent = entry("..", Some(0));
    parent.parent = true;
    let mut native = entry("unicode子", Some(1));
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        native.name = OsString::from_vec(vec![255]);
    }
    let result = compare::compare(&[dir, parent, native], &[], CompareOptions::default());
    assert_eq!(result.left_only, 1);
    assert_eq!(result.differences[0].len(), 1);
}
#[test]
fn export_encoding_unicode_order_and_invalid_paths() -> io::Result<()> {
    let paths = vec![PathBuf::from("folder/子.txt"), PathBuf::from("folder/beta")];
    assert_eq!(export::encode(&paths, false)?, "子.txt\nbeta\n".as_bytes());
    assert_eq!(
        export::encode(&paths, true)?,
        "folder/子.txt\nfolder/beta\n".as_bytes()
    );
    assert!(export::encode(&[PathBuf::from("parent\n/file")], false).is_err());
    assert!(export::encode(&[PathBuf::from("bad\rname")], true).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        assert!(export::encode(&[PathBuf::from(OsString::from_vec(vec![255]))], false).is_err());
    }
    Ok(())
}
#[test]
fn exclusive_export_existing_paths_errors_and_no_temporary_leaks() -> io::Result<()> {
    let t = Temp::new()?;
    let paths = vec![t.0.join("α")];
    let dest = t.0.join("files.lst");
    export::write_list(&dest, &paths, false)?;
    assert_eq!(fs::read_to_string(&dest)?, "α\n");
    assert!(export::write_list(&dest, &paths, true).is_err());
    assert_eq!(fs::read_to_string(&dest)?, "α\n");
    assert!(export::write_list(&t.0, &paths, false).is_err());
    assert!(export::write_list(&t.0.join("missing/out"), &paths, false).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(t.0.join("nonexistent"), t.0.join("link"))?;
        assert!(export::write_list(&t.0.join("link"), &paths, false).is_err());
        assert!(fs::symlink_metadata(t.0.join("link"))?.is_symlink());
    }
    assert!(!fs::read_dir(&t.0)?.any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".dn-list-")));
    Ok(())
}
#[test]
fn export_dialog_background_refresh_preserves_selection_cursor_and_format() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("alpha"), "a")?;
    fs::write(t.0.join("beta"), "b")?;
    let mut app = t.app()?;
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "beta")
        .unwrap();
    app.panels[0].selected.insert("alpha".into());
    app.panels[0].selected.insert("beta".into());
    let marks = app.panels[0].selected.clone();
    key(&mut app, KeyCode::Char('l'), KeyModifiers::ALT);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Tool(ToolAction::Choice(1)));
    assert!(app.file_tools.full_paths);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!t.0.join("files.lst").exists());
    file_tools::open_export(&mut app);
    draw(&mut app, 120, 35)?;
    if let Some(ToolDialog::Export { field, cursor, .. }) = &mut app.file_tools.dialog {
        *field = "out.lst".into();
        *cursor = field.len();
    }
    click(&mut app, Target::Tool(ToolAction::Accept));
    assert!(app.file_tools.busy());
    wait(&mut app, |a| {
        !a.file_tools.busy() && a.panels.iter().all(|p| !p.loading)
    });
    assert_eq!(app.panels[0].selected, marks);
    assert_eq!(app.panels[0].entries[app.panels[0].cursor].name, "beta");
    let expected = format!(
        "{}\n{}\n",
        t.0.join("alpha").display(),
        t.0.join("beta").display()
    );
    assert_eq!(fs::read_to_string(t.0.join("out.lst"))?, expected);
    assert!(app
        .panels
        .iter()
        .all(|p| p.entries.iter().any(|e| e.name == "out.lst")));
    app.panels[0].selected.clear();
    file_tools::open_export(&mut app);
    if let Some(ToolDialog::Export { paths, .. }) = &app.file_tools.dialog {
        assert_eq!(paths, &[t.0.join("beta")]);
    } else {
        panic!("export dialog")
    }
    Ok(())
}
#[test]
fn menu_entries_and_modal_precedence() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    for action in [
        menu::Action::CompareDirectories,
        menu::Action::ExportList,
        menu::Action::History(HistoryKind::Directory),
    ] {
        assert!(menu::items(3).iter().any(|i| i.action == action));
    }
    key(&mut app, KeyCode::Char('b'), KeyModifiers::ALT);
    assert!(app.submenus.iter().any(|(id, _)| *id == menu::MenuId::Sort));
    menu::close(&mut app);
    file_tools::open_compare(&mut app);
    app.error("top dialog");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.file_tools.dialog.is_some());
    assert!(app.dialogs.is_empty());
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.file_tools.dialog.is_none());
    editor::open(&mut app, true);
    editor::paste(&mut app, "copy");
    key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
    key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(app.editor_clipboard, "copy");
    assert!(app.file_tools.dialog.is_none());
    assert!(!app.quit);
    Ok(())
}
#[cfg(unix)]
#[test]
fn export_permission_failure_leaves_no_output() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let t = Temp::new()?;
    let locked = t.0.join("locked");
    fs::create_dir(&locked)?;
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500))?;
    let result = export::write_list(&locked.join("out"), &[t.0.join("item")], false);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700))?;
    if unsafe { libc::geteuid() } != 0 {
        assert!(result.is_err());
        assert!(!locked.join("out").exists());
        assert_eq!(fs::read_dir(locked)?.count(), 0);
    }
    Ok(())
}

#[test]
fn export_worker_failure_and_exit_wait_for_publication() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("item"), "data")?;
    fs::write(t.0.join("files.lst"), "keep")?;
    let mut app = t.app()?;
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "item")
        .unwrap();
    file_tools::open_export(&mut app);
    file_tools::accept(&mut app);
    wait(&mut app, |a| !a.file_tools.busy());
    assert!(!app.dialogs.is_empty());
    assert_eq!(fs::read_to_string(t.0.join("files.lst"))?, "keep");
    app.dialogs.clear();
    file_tools::open_export(&mut app);
    if let Some(ToolDialog::Export { field, cursor, .. }) = &mut app.file_tools.dialog {
        *field = "complete.lst".into();
        *cursor = field.len();
    }
    file_tools::accept(&mut app);
    key(&mut app, KeyCode::F(10), KeyModifiers::NONE);
    assert!(!app.quit);
    assert!(app.exit_pending);
    wait(&mut app, |a| a.quit);
    assert_eq!(fs::read_to_string(t.0.join("complete.lst"))?, "item\n");
    Ok(())
}
#[cfg(unix)]
#[test]
fn directory_history_native_paths_and_permission_failures() -> io::Result<()> {
    use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};
    let t = Temp::new()?;
    let native = t.0.join(OsString::from_vec(vec![b'n', 255]));
    let mut app = t.app()?;
    // Some macOS volumes reject undecodable names. Histories must retain the
    // native value even when reopening it fails; display must never rewrite it.
    app.file_tools
        .histories
        .record(HistoryKind::Directory, native.clone());
    file_tools::open_history(&mut app, HistoryKind::Directory);
    draw(&mut app, 80, 25)?;
    assert_eq!(
        app.file_tools.histories.get(HistoryKind::Directory)[0],
        native
    );
    file_tools::accept(&mut app);
    wait(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].path, t.0);
    assert!(!app.dialogs.is_empty());
    app.dialogs.clear();
    let locked = t.0.join("locked");
    fs::create_dir(&locked)?;
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o100))?;
    app.file_tools
        .histories
        .record(HistoryKind::Directory, locked.clone());
    file_tools::open_history(&mut app, HistoryKind::Directory);
    file_tools::accept(&mut app);
    wait(&mut app, |a| !a.panels[0].loading);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700))?;
    if unsafe { libc::geteuid() } != 0 {
        assert_eq!(app.panels[0].path, t.0);
        assert!(!app.dialogs.is_empty());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn comparison_special_file_types_and_missing_timestamps() -> io::Result<()> {
    let t = Temp::new()?;
    let a = entry("item", Some(0));
    let mut b = entry("item", Some(0));
    // Compare the cached native FileType independently of convenience flags,
    // without opening a special file or requiring socket-creation permission.
    b.file_type = Some(fs::metadata(&t.0)?.file_type());
    assert_eq!(
        compare::compare(
            &[a],
            &[b],
            CompareOptions {
                size: false,
                time: false,
                unselect: false
            }
        )
        .different,
        1
    );
    let a = entry("a", Some(1));
    let mut b = entry("a", Some(1));
    b.modified = None;
    assert_eq!(
        compare::compare(&[a], &[b], CompareOptions::default()).indeterminate,
        1
    );
    Ok(())
}
