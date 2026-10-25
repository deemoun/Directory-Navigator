use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    config::Config,
    config_import::{self, dos},
    panel::SortMode,
    task::Cancellation,
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
        let path = std::env::temp_dir().join(format!(
            "dn-import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cfg(blocks: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = dos::SIGNATURE.to_vec();
    for (id, block) in blocks {
        bytes.extend(id.to_le_bytes());
        bytes.extend((block.len() as u16).to_le_bytes());
        bytes.extend(block);
    }
    bytes.extend([0; 4]);
    bytes
}
fn all_blocks() -> Vec<(u16, Vec<u8>)> {
    let mut editor = vec![0; 22];
    editor[18] = 1;
    editor[19] = b'4';
    let mut panel = vec![0; 6];
    panel[0] = 1;
    vec![
        (4, vec![0; 2]),
        (5, vec![3, 0, 0, 0]),
        (7, panel),
        (27, vec![0; 264]),
        (30, editor),
        (32, vec![0; 120]),
        (15, vec![255; 4]),
    ]
}
fn wait(app: &mut dn::app::App) {
    let end = Instant::now() + Duration::from_secs(5);
    while app.config_import.as_ref().is_some_and(|p| p.loading()) {
        app.poll();
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(2));
    }
}
fn key(app: &mut dn::app::App, code: KeyCode) {
    dn::input::handle(app, KeyEvent::new(code, KeyModifiers::NONE));
}
fn draw(app: &mut dn::app::App, width: u16, height: u16) -> io::Result<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| dn::ui::draw(frame, app))?;
    for (rect, _) in &app.mouse.regions {
        assert!(rect.right() <= width && rect.bottom() <= height);
    }
    Ok(terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect())
}
fn click(app: &mut dn::app::App, target: dn::mouse::Target) {
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == target)
        .unwrap()
        .0;
    dn::mouse::handle(
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
fn documented_blocks_map_preferences_and_preserve_modern_safeguards() -> io::Result<()> {
    let report = dos::parse(&cfg(&all_blocks()))?;
    let original = Config::default();
    let draft = report.draft(&original);
    assert!(!draft.show_clock);
    assert!(!draft.show_hidden);
    assert!(!draft.highlight_files);
    assert!(!draft.space_select);
    assert!(!draft.delete_key);
    assert!(!draft.show_tags);
    assert!(!draft.editor.auto_indent);
    assert_eq!(draft.editor.tab_size, 4);
    assert_eq!(draft.sorts, [SortMode::Extension; 2]);
    assert_eq!(draft.saver_minutes, 5);
    assert_eq!(draft.use_trash, original.use_trash);
    assert_eq!(draft.shortcuts, original.shortcuts);
    assert_eq!(draft.layouts, original.layouts);
    assert_eq!(draft.palette, original.palette);
    assert_eq!(draft.datetime, original.datetime);
    assert_eq!(draft.language, original.language);
    assert_eq!(draft.editor.expand_tabs, original.editor.expand_tabs);
    assert_eq!(draft.editor.syntax, original.editor.syntax);
    assert_eq!(report.settings.len(), 10);
    assert_eq!(report.skipped.len(), 1);
    Ok(())
}
#[test]
fn selectors_sort_modes_and_flags_are_not_guessed() -> io::Result<()> {
    for (selector, minutes) in [0, 1, 2, 5, 10].into_iter().enumerate() {
        let report = dos::parse(&cfg(&[(5, vec![selector as u8, 0, 0, 0])]))?;
        assert_eq!(report.draft(&Config::default()).saver_minutes, minutes);
    }
    for (selector, mode) in [
        SortMode::Name,
        SortMode::Extension,
        SortMode::Size,
        SortMode::Date,
    ]
    .into_iter()
    .enumerate()
    {
        let report = dos::parse(&cfg(&[(7, vec![selector as u8, 0, 0x20, 0, 0, 0])]))?;
        assert_eq!(report.draft(&Config::default()).sorts, [mode; 2]);
    }
    let report = dos::parse(&cfg(&[
        (4, vec![1, 0]),
        (27, {
            let mut b = vec![0; 264];
            b[0] = 0x30;
            b[2] = 8;
            b
        }),
    ]))?;
    let draft = report.draft(&Config::default());
    assert!(draft.show_clock && draft.show_tags && draft.space_select && draft.delete_key);
    let report = dos::parse(&cfg(&[(5, vec![255; 4]), (7, vec![5, 0, 0, 0, 0, 0])]))?;
    assert_eq!(report.skipped.len(), 2);
    assert_eq!(
        report.draft(&Config::default()).sorts,
        Config::default().sorts
    );
    Ok(())
}
#[test]
fn bounds_signatures_duplicates_and_termination_are_strict() {
    let bytes = cfg(&all_blocks());
    for end in 0..bytes.len() {
        assert!(dos::parse(&bytes[..end]).is_err(), "prefix {end}");
    }
    let mut trailing = bytes.clone();
    trailing.push(1);
    assert!(dos::parse(&trailing).is_err());
    assert!(dos::parse(&vec![0; dos::LIMIT + 1]).is_err());
    assert!(dos::parse(b"DN2 Desktop\x1a\x01\x28").is_err());
    let duplicate = cfg(&[(4, vec![0; 2]), (4, vec![1, 0])]);
    assert!(dos::parse(&duplicate).is_err());
    let mut invalid = cfg(&[]);
    *invalid.last_mut().unwrap() = 1;
    assert!(dos::parse(&invalid).is_err());
}
#[test]
fn unfamiliar_layouts_and_invalid_pascal_strings_are_skipped() -> io::Result<()> {
    let report = dos::parse(&cfg(&[
        (4, vec![1; 3]),
        (100, vec![1, 2]),
        (30, vec![0xff; 22]),
    ]))?;
    assert_eq!(report.skipped.len(), 3);
    let draft = report.draft(&Config::default());
    assert!(draft.show_clock);
    assert_eq!(draft.editor.tab_size, 8);
    let mut editor = vec![0; 22];
    editor[18] = 2;
    editor[19..21].copy_from_slice(b"17");
    let report = dos::parse(&cfg(&[(30, editor)]))?;
    assert_eq!(report.skipped.len(), 1);
    Ok(())
}
#[test]
fn background_read_is_readonly_bounded_and_cancellable() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("DN.CFG");
    let bytes = cfg(&all_blocks());
    fs::write(&path, &bytes)?;
    let original = fs::metadata(&path)?.permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&path, readonly)?;
    assert!(dos::read(&path, &Cancellation::default()).is_ok());
    assert_eq!(fs::read(&path)?, bytes);
    fs::set_permissions(&path, original)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let original = fs::metadata(&path)?.permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000))?;
        let denied = fs::File::open(&path).is_err();
        let result = dos::read(&path, &Cancellation::default());
        fs::set_permissions(&path, original)?;
        // Root can bypass mode bits; ordinary-user runs exercise PermissionDenied.
        if denied {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        }
    }
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        dos::read(&path, &cancel).unwrap_err().kind(),
        io::ErrorKind::Interrupted
    );
    assert!(dos::read(&t.0.join("missing"), &Cancellation::default()).is_err());
    assert!(dos::read(&t.0, &Cancellation::default()).is_err());
    fs::write(&path, vec![0; dos::LIMIT + 1])?;
    assert!(dos::read(&path, &Cancellation::default()).is_err());
    Ok(())
}
#[cfg(unix)]
#[test]
fn symlink_input_is_refused_without_touching_target() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("original");
    let bytes = cfg(&all_blocks());
    fs::write(&path, &bytes)?;
    let link = t.0.join("DN.CFG");
    std::os::unix::fs::symlink(&path, &link)?;
    assert!(dos::read(&link, &Cancellation::default()).is_err());
    assert_eq!(fs::read(path)?, bytes);
    Ok(())
}
#[test]
fn menu_prompt_preview_cancel_apply_and_mouse_preserve_source_and_unrelated_settings(
) -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("DN.CFG");
    let bytes = cfg(&all_blocks());
    fs::write(&path, &bytes)?;
    let mut app = dn::app::App::new(t.0.clone())?;
    let original = app.config.clone();
    dn::menu::execute(&mut app, dn::menu::Action::ImportDosSettings);
    assert_eq!(dn::focus::current(&app), dn::focus::Focus::Prompt);
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert_eq!(dn::focus::current(&app), dn::focus::Focus::ConfigImport);
    assert_eq!(app.config, original);
    dn::input::paste(&mut app, "must not reach command line");
    assert!(app.command_line.text.is_empty());
    for (w, h) in [(80, 25), (120, 40), (60, 20), (80, 25)] {
        draw(&mut app, w, h)?;
    }
    draw(&mut app, 80, 25)?;
    click(&mut app, dn::mouse::Target::CancelImport);
    assert!(app.config_import.is_none());
    assert_eq!(app.config, original);
    config_import::start(&mut app, path.clone())?;
    wait(&mut app);
    app.error("topmost");
    key(&mut app, KeyCode::Enter);
    assert!(app.config_import.is_some());
    key(&mut app, KeyCode::End);
    draw(&mut app, 80, 25)?;
    key(&mut app, KeyCode::Home);
    draw(&mut app, 80, 25)?;
    click(&mut app, dn::mouse::Target::ApplyImport);
    assert!(app.config_import.is_none());
    assert!(!app.config.show_clock);
    assert!(app.config.use_trash);
    assert_eq!(app.config.datetime, original.datetime);
    assert_eq!(fs::read(&path)?, bytes);
    assert!(!t.0.join("config.conf").exists());
    Ok(())
}
#[test]
fn closing_loading_no_change_and_errors_disable_apply_and_reject_stale_results() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("old");
    fs::write(&path, cfg(&all_blocks()))?;
    let mut app = dn::app::App::new(t.0.clone())?;
    config_import::start(&mut app, path)?;
    key(&mut app, KeyCode::Esc);
    assert!(app.config_import.is_none());
    let empty = t.0.join("empty");
    fs::write(&empty, cfg(&[]))?;
    config_import::start(&mut app, empty.clone())?;
    wait(&mut app);
    assert_eq!(app.config_import.as_ref().unwrap().path, empty);
    assert!(!app.config_import.as_ref().unwrap().can_apply());
    for lang in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        app.config.language = lang;
        draw(&mut app, 80, 25)?;
        assert!(!app
            .mouse
            .regions
            .iter()
            .any(|(_, t)| *t == dn::mouse::Target::ApplyImport));
    }
    key(&mut app, KeyCode::Enter);
    assert!(app.config_import.is_none());
    config_import::start(&mut app, t.0.join("missing"))?;
    wait(&mut app);
    assert!(app
        .config_import
        .as_ref()
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .is_err());
    config_import::apply(&mut app);
    assert!(app.config_import.is_some());
    draw(&mut app, 80, 25)?;
    click(&mut app, dn::mouse::Target::CancelImport);
    assert!(app.config_import.is_none());
    Ok(())
}

#[test]
fn cancel_and_apply_do_not_reset_unrelated_live_panel_sorts() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("DN.CFG");
    fs::write(&path, cfg(&[(4, vec![0; 2])]))?;
    let mut app = dn::app::App::new(t.0.clone())?;
    app.panels[0].set_sort(SortMode::Date);
    app.panels[1].set_sort(SortMode::Size);
    let original = app.config.clone();
    config_import::start(&mut app, path.clone())?;
    wait(&mut app);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.config, original);
    assert_eq!(
        [app.panels[0].sort, app.panels[1].sort],
        [SortMode::Date, SortMode::Size]
    );
    config_import::start(&mut app, path)?;
    wait(&mut app);
    key(&mut app, KeyCode::Enter);
    assert!(!app.config.show_clock);
    assert_eq!(
        [app.panels[0].sort, app.panels[1].sort],
        [SortMode::Date, SortMode::Size]
    );
    Ok(())
}

#[test]
fn unsupported_record_reports_are_bounded_and_counted() -> io::Result<()> {
    let blocks = vec![(100, Vec::new()); 200];
    let report = dos::parse(&cfg(&blocks))?;
    assert_eq!(report.skipped.len(), 101);
    assert!(report.skipped.last().unwrap().ends_with("200"));
    assert!(!report.changed(&Config::default()));
    Ok(())
}
