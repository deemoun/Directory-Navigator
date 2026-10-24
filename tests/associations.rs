use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dn::{
    app::App,
    associations::{
        self,
        format::{self, Target},
    },
    config::Store,
    input, menu, ui,
};
use ratatui::{backend::TestBackend, Terminal};
use std::{
    ffi::OsString,
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
        let path = std::env::temp_dir().join(format!(
            "dn-associations-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn config(&self) -> PathBuf {
        self.0.join("settings/associations.conf")
    }
    fn write(&self, text: &str) -> io::Result<()> {
        fs::create_dir_all(self.0.join("settings"))?;
        fs::write(self.config(), text)
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        app.config_store = Store::new(Some(self.0.join("settings/config.conf")));
        wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
        associations::reload(&mut app);
        wait(&mut app, |a| !a.associations.busy());
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(app: &mut App, done: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.poll();
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
}
fn focus(app: &mut App, name: &str) {
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == name)
        .expect("fixture entry");
}
fn key(app: &mut App, code: KeyCode) {
    input::handle(app, KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn format_rules_case_comments_quotes_and_internal_targets() -> io::Result<()> {
    let rules = format::parse("# comment\n; comment\n.TXT = @editor\n.log=@viewer\n.bin=@hex\n.zip=@default\n.md=\"my app\" --wait %f\n<none>=tool %n\n")?;
    assert_eq!(rules.len(), 6);
    assert!(rules[0].matches(std::path::Path::new("文.TxT")));
    assert!(!rules[0].matches(std::path::Path::new("file.txt.bak")));
    assert_eq!(rules[0].target, Target::Editor);
    assert!(rules[5].matches(std::path::Path::new("README")));
    assert!(format::parse(format::DEFAULT)?.is_empty());
    Ok(())
}

#[test]
fn invalid_rules_limits_and_duplicates_are_rejected() {
    for text in [
        "bad",
        "txt=app",
        ".=app",
        ".tar.gz=app",
        ".*=app",
        ".txt=",
        ".txt=@wat",
        ".txt=@editor extra",
        ".txt=app 'oops",
        ".txt=app --file=%f",
        ".txt=app %f.txt",
        ".txt=%f",
        ".txt=a\n.TXT=b",
        ".txt=app\0",
    ] {
        assert!(format::parse(text).is_err(), "{text:?}");
    }
    assert!(format::parse(&" ".repeat(format::MAX_BYTES + 1)).is_err());
    let many = (0..129)
        .map(|n| format!(".x{n}=@viewer\n"))
        .collect::<String>();
    assert!(format::parse(&many).is_err());
}

#[test]
fn command_expansion_preserves_literal_native_arguments() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("文 ; $HOME 'quoted'.txt");
    let args = vec![
        "tool".into(),
        "%f".into(),
        "%d".into(),
        "%n".into(),
        "".into(),
        "|".into(),
        "$(echo nope)".into(),
    ];
    let plan = format::command(&args, &path, temp.0.clone())?;
    assert_eq!(plan.cwd, temp.0);
    assert_eq!(plan.commands.len(), 1);
    assert_eq!(
        plan.commands[0].args,
        vec![
            path.as_os_str().to_owned(),
            temp.0.as_os_str().to_owned(),
            path.file_stem().unwrap().to_owned(),
            OsString::new(),
            "|".into(),
            "$(echo nope)".into()
        ]
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn undecodable_filenames_keep_native_bytes_in_placeholders() -> io::Result<()> {
    use std::os::unix::ffi::OsStringExt;
    let temp = Temp::new()?;
    let path = temp.0.join(OsString::from_vec(b"bad\xff.TXT".to_vec()));
    let rules = format::parse(".txt=tool %f %n")?;
    assert!(rules[0].matches(&path));
    let Target::Command(args) = &rules[0].target else {
        panic!("command");
    };
    let plan = format::command(args, &path, temp.0.clone())?;
    assert_eq!(plan.commands[0].args[0], path.as_os_str());
    assert_eq!(
        plan.commands[0].args[1],
        OsString::from_vec(b"bad\xff".to_vec())
    );
    Ok(())
}

#[test]
fn missing_config_is_inactive_and_bad_config_can_be_repaired() -> io::Result<()> {
    let temp = Temp::new()?;
    assert!(format::load(&temp.config())?.is_empty());
    assert!(!temp.config().exists());
    temp.write(".txt=@viewer")?;
    fs::write(temp.0.join("file.txt"), "hello")?;
    let mut app = temp.app()?;
    assert_eq!(app.associations.rules.len(), 1);
    temp.write("invalid")?;
    associations::reload(&mut app);
    wait(&mut app, |a| !a.associations.busy());
    assert!(!app.dialogs.is_empty());
    app.dialogs.clear();
    focus(&mut app, "file.txt");
    key(&mut app, KeyCode::Enter);
    assert!(!app.dialogs.is_empty() && app.desktop.viewer().is_none());
    app.dialogs.clear();
    temp.write(".txt=@editor")?;
    associations::reload(&mut app);
    wait(&mut app, |a| !a.associations.busy());
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| a.desktop.editor().is_some_and(|e| !e.busy()));
    assert!(app.dialogs.is_empty());
    Ok(())
}

#[test]
fn enter_internal_targets_preserve_marks_and_explicit_f3_f4() -> io::Result<()> {
    let temp = Temp::new()?;
    temp.write(".txt=@editor\n.bin=@hex\n")?;
    fs::write(temp.0.join("file.txt"), "hello")?;
    fs::write(temp.0.join("raw.bin"), b"printable text, forced to hex")?;
    let mut app = temp.app()?;
    focus(&mut app, "file.txt");
    key(&mut app, KeyCode::Insert);
    focus(&mut app, "file.txt");
    let marks = app.panels[0].selected.clone();
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| a.desktop.editor().is_some_and(|e| !e.busy()));
    assert_eq!(app.panels[0].selected, marks);
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::F(3));
    wait(&mut app, |a| {
        a.desktop.viewer().is_some_and(|v| v.contents.is_some())
    });
    assert!(app.desktop.editor().is_none());
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::F(4));
    wait(&mut app, |a| a.desktop.editor().is_some_and(|e| !e.busy()));
    key(&mut app, KeyCode::Esc);
    focus(&mut app, "raw.bin");
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| {
        a.desktop.viewer().is_some_and(|v| v.contents.is_some())
    });
    assert!(
        app.desktop
            .viewer()
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .binary
    );
    Ok(())
}

#[test]
fn directories_navigate_and_ctrl_enter_bypasses_custom_rules() -> io::Result<()> {
    let temp = Temp::new()?;
    temp.write(".txt=@editor")?;
    fs::create_dir(temp.0.join("folder.txt"))?;
    fs::write(temp.0.join("file.txt"), "text")?;
    let mut app = temp.app()?;
    focus(&mut app, "folder.txt");
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| !a.panels[0].loading);
    assert!(app.panels[0].path.ends_with("folder.txt"));
    key(&mut app, KeyCode::Backspace);
    wait(&mut app, |a| !a.panels[0].loading);
    focus(&mut app, "file.txt");
    fs::remove_file(temp.0.join("file.txt"))?;
    input::handle(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
    );
    assert!(app.launcher.loading() && !app.associations.busy());
    wait(&mut app, |a| !a.launcher.loading());
    assert!(app.desktop.editor().is_none());
    Ok(())
}

#[test]
fn stale_launches_and_file_failures_do_not_queue_commands_or_poison_rules() -> io::Result<()> {
    let temp = Temp::new()?;
    temp.write(".txt=tool %f")?;
    fs::write(temp.0.join("a.txt"), "a")?;
    fs::write(temp.0.join("b.txt"), "b")?;
    let mut app = temp.app()?;
    focus(&mut app, "a.txt");
    key(&mut app, KeyCode::Enter);
    focus(&mut app, "b.txt");
    wait(&mut app, |a| !a.associations.busy());
    assert!(app.associations.pending.is_none());
    fs::remove_file(temp.0.join("b.txt"))?;
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| !a.associations.busy());
    assert!(app.associations.pending.is_none() && !app.dialogs.is_empty());
    app.dialogs.clear();
    focus(&mut app, "a.txt");
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| !a.associations.busy());
    assert!(
        app.associations.pending.is_some(),
        "file failure must not disable valid rules"
    );
    associations::reload(&mut app);
    assert!(
        app.associations.pending.is_none(),
        "reload discards queued commands"
    );
    wait(&mut app, |a| !a.associations.busy());
    assert_eq!(app.associations.rules.len(), 1);
    Ok(())
}

#[test]
fn edit_creates_only_comments_and_reloads_after_guarded_editor_close() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    assert!(!temp.config().exists());
    associations::edit(&mut app);
    wait(&mut app, |a| a.desktop.editor().is_some_and(|e| !e.busy()));
    assert_eq!(fs::read_to_string(temp.config())?, format::DEFAULT);
    assert!(app.associations.rules.is_empty());
    // Use the normal editor save path, then close and reload automatically.
    dn::editor::paste(&mut app, ".txt=@viewer\n");
    input::handle(
        &mut app,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
    );
    wait(&mut app, |a| a.desktop.editor().is_some_and(|e| !e.busy()));
    key(&mut app, KeyCode::Esc);
    wait(&mut app, |a| a.associations.rules.len() == 1);
    assert_eq!(app.associations.rules[0].target, Target::Viewer);
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_and_oversized_configs_are_rejected_without_changing_targets() -> io::Result<()> {
    let temp = Temp::new()?;
    temp.write(".txt=@viewer")?;
    let original = temp.0.join("original");
    fs::rename(temp.config(), &original)?;
    std::os::unix::fs::symlink(&original, temp.config())?;
    assert!(format::load(&temp.config()).is_err());
    let mut app = temp.app()?;
    app.dialogs.clear();
    associations::edit(&mut app);
    wait(&mut app, |a| !a.associations.busy());
    assert!(app.desktop.editor().is_none() && !app.dialogs.is_empty());
    assert_eq!(fs::read_to_string(original)?, ".txt=@viewer");
    fs::remove_file(temp.config())?;
    temp.write(&" ".repeat(format::MAX_BYTES + 1))?;
    assert!(format::load(&temp.config()).is_err());
    Ok(())
}

#[test]
fn association_menu_keyboard_mouse_targets_locales_and_resize() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        app.config.language = language;
        for (w, h) in [(80, 25), (120, 40)] {
            menu::open(&mut app, 5);
            let row = menu::items(5)
                .iter()
                .position(|i| i.action == menu::Action::Submenu(menu::MenuId::Associations))
                .unwrap();
            app.menu = Some((5, row));
            menu::key(&mut app, KeyCode::Right);
            let mut terminal = Terminal::new(TestBackend::new(w, h))?;
            terminal.draw(|f| ui::draw(f, &mut app))?;
            assert!(app
                .mouse
                .regions
                .iter()
                .all(|(r, _)| r.right() <= w && r.bottom() <= h));
            assert_eq!(app.submenus.last().unwrap().0, menu::MenuId::Associations);
            menu::close(&mut app);
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn external_commands_execute_literal_paths_and_report_missing_executables() -> io::Result<()> {
    let temp = Temp::new()?;
    let output = temp.0.join("result");
    temp.write(&format!(".txt=/bin/sh -c 'printf \"%s\" \"$1\" > \"$2\"' dn %f '{}'\n.bin=dn-nonexistent-association-executable %f\n", output.display()))?;
    let name = "文; touch HACK $HOME.txt";
    let source = temp.0.join(name);
    fs::write(&source, "contents")?;
    fs::write(temp.0.join("file.bin"), "binary")?;
    let mut app = temp.app()?;
    focus(&mut app, name);
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| !a.associations.busy());
    assert!(
        !output.exists(),
        "queueing must not execute on the UI thread"
    );
    let plan = app.associations.pending.take().expect("external plan");
    assert!(dn::user_menu::runner::run(&plan)?.success());
    assert_eq!(fs::read(&output)?, source.as_os_str().as_encoded_bytes());
    assert!(!temp.0.join("HACK").exists());
    assert_eq!(fs::read(&source)?, b"contents");
    focus(&mut app, "file.bin");
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| !a.associations.busy());
    let plan = app
        .associations
        .pending
        .take()
        .expect("missing executable plan");
    assert_eq!(
        dn::user_menu::runner::run(&plan)
            .expect_err("spawn failure")
            .kind(),
        io::ErrorKind::NotFound
    );
    assert!(
        !app.launcher.loading(),
        "failed custom commands must not silently use the OS opener"
    );
    Ok(())
}

#[test]
fn mouse_reload_double_click_and_archive_override() -> io::Result<()> {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use dn::mouse;
    let temp = Temp::new()?;
    temp.write(".zip=@viewer\n")?;
    fs::write(
        temp.0.join("archive.zip"),
        "custom viewer bypasses archive detection",
    )?;
    let mut app = temp.app()?;
    menu::open(&mut app, 5);
    let row = menu::items(5)
        .iter()
        .position(|i| i.action == menu::Action::Submenu(menu::MenuId::Associations))
        .unwrap();
    app.menu = Some((5, row));
    menu::key(&mut app, KeyCode::Right);
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == mouse::Target::MenuItem(1, 1))
        .expect("Reload target")
        .0;
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert!(app.associations.busy() && app.menu.is_none());
    wait(&mut app, |a| !a.associations.busy());
    focus(&mut app, "archive.zip");
    terminal.draw(|f| ui::draw(f, &mut app))?;
    let cursor = app.panels[0].cursor;
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == mouse::Target::Entry(0, cursor))
        .expect("file target")
        .0;
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    };
    mouse::handle(&mut app, event);
    mouse::handle(&mut app, event);
    wait(&mut app, |a| {
        a.desktop.viewer().is_some_and(|v| v.contents.is_some())
    });
    assert!(app.archive.is_none());
    assert!(app
        .desktop
        .viewer()
        .unwrap()
        .contents
        .as_ref()
        .unwrap()
        .lines[0]
        .contains("custom viewer"));
    Ok(())
}
