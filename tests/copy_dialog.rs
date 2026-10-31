//! Dialog and worker integration uses only private temporary fixtures.
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use dn::{
    app::App,
    copy_dialog::{self, Action},
    directory_tree, input,
    operations::{ExistingDestination, Kind},
    testing::{backend::TestBackend, Terminal},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-copy-dialog-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(fs::canonicalize(p)?))
    }
    fn app(&self) -> io::Result<App> {
        fs::write(self.0.join("source.txt"), b"fixture bytes")?;
        fs::create_dir(self.0.join("out"))?;
        let mut app = App::new(self.0.clone())?;
        wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
        let panel = &mut app.panels[app.active];
        panel.cursor = panel
            .entries
            .iter()
            .position(|e| e.path == self.0.join("source.txt"))
            .unwrap();
        app.prompt_operation(Kind::Copy, false);
        draw(&mut app, 80, 25)?;
        Ok(app)
    }
}
impl Drop for Sandbox {
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
fn key(app: &mut App, code: K) {
    input::handle(app, KeyEvent::new(code, M::NONE));
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut t = Terminal::new(TestBackend::new(w, h))?;
    t.draw(|f| ui::draw(f, app))?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(r, _)| r.right() <= w && r.bottom() <= h));
    Ok(t.backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
#[test]
fn dn_layout_disabled_choices_focus_resize_and_cancel_never_write() -> io::Result<()> {
    let t = Sandbox::new()?;
    let mut app = t.app()?;
    for (w, h) in [(80, 25), (132, 40), (80, 25)] {
        let screen = draw(&mut app, w, h)?;
        assert!(screen.contains("Refuse existing destinations"));
        assert!(screen.contains("Skip all existing files"));
        assert!(screen.contains("Copy descriptions"));
        let field = app
            .mouse
            .regions
            .iter()
            .find(|(_, t)| *t == dn::mouse::Target::PromptField)
            .unwrap()
            .0;
        assert_eq!(
            (field.x, field.y, field.width),
            ((80 - 59) / 2 + 2, (25 - 16) / 2 + 2, 53)
        );
    }
    let mut focuses = Vec::new();
    for _ in 0..copy_dialog::FOCUS.len() {
        key(&mut app, K::Tab);
        focuses.push(app.modals.prompt().unwrap().focus);
    }
    assert_eq!(focuses, vec![10, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0]);
    copy_dialog::action(&mut app, Action::RemoveSources);
    copy_dialog::action(&mut app, Action::Verify);
    assert_eq!(app.modals.prompt().unwrap().focus, 5);
    let options = app.modals.prompt().unwrap().copy.as_ref().unwrap().options;
    assert!(options.remove_sources && options.verify_writes);
    app.modals.prompt_mut().unwrap().focus = 3;
    key(&mut app, K::Tab);
    assert_eq!(app.modals.prompt().unwrap().focus, 5);
    draw(&mut app, 50, 12)?;
    key(&mut app, K::Enter);
    assert!(app.modals.prompt().is_some() && app.job.is_none());
    draw(&mut app, 80, 25)?;
    key(&mut app, K::Esc);
    assert!(app.modals.prompt().is_none());
    assert_eq!(fs::read(t.0.join("source.txt"))?, b"fixture bytes");
    assert_eq!(fs::read_dir(t.0.join("out"))?.count(), 0);
    Ok(())
}
#[test]
fn history_help_and_tree_restore_the_same_copy_owner_without_panel_navigation() -> io::Result<()> {
    let t = Sandbox::new()?;
    let mut app = t.app()?;
    let owner = copy_dialog::parent(&app).unwrap();
    let panels = app.panels.each_ref().map(|p| p.path.clone());
    copy_dialog::action(&mut app, Action::Skip);
    app.copy_dialog.record(t.0.join("out"));
    copy_dialog::action(&mut app, Action::History);
    key(&mut app, K::Esc);
    assert_eq!(copy_dialog::parent(&app), Some(owner));
    assert_eq!(app.modals.prompt().unwrap().focus, 10);
    copy_dialog::action(&mut app, Action::History);
    key(&mut app, K::Enter);
    assert_eq!(
        app.modals.prompt().unwrap().copy.as_ref().unwrap().picked,
        Some(t.0.join("out"))
    );
    assert_eq!(
        app.modals
            .prompt()
            .unwrap()
            .copy
            .as_ref()
            .unwrap()
            .options
            .existing,
        ExistingDestination::Skip
    );
    key(&mut app, K::F(1));
    draw(&mut app, 80, 25)?;
    key(&mut app, K::Esc);
    assert_eq!(copy_dialog::parent(&app), Some(owner));
    copy_dialog::action(&mut app, Action::Tree);
    key(&mut app, K::Esc);
    assert!(app.directory_tree().is_none());
    copy_dialog::action(&mut app, Action::Tree);
    wait(&mut app, |a| {
        a.directory_tree().is_some_and(|t| !t.loading())
    });
    let tree = app.directory_tree_mut().unwrap();
    tree.cursor = tree
        .rows
        .iter()
        .position(|r| r.path == t.0.join("out"))
        .unwrap();
    directory_tree::action(&mut app, directory_tree::Action::Open);
    assert_eq!(app.panels.each_ref().map(|p| p.path.clone()), panels);
    assert_eq!(
        app.modals.prompt().unwrap().copy.as_ref().unwrap().picked,
        Some(t.0.join("out"))
    );
    // Retire a parent with an in-flight child, then reject its old path receipt.
    copy_dialog::action(&mut app, Action::Tree);
    app.modals.set_prompt(None);
    assert!(app.directory_tree().is_none());
    app.prompt_operation(Kind::Copy, false);
    copy_dialog::pick(&mut app, owner, t.0.join("wrong"));
    app.poll();
    assert!(app.modals.prompt().unwrap().field.is_empty());
    key(&mut app, K::Esc);
    assert!(app.job.is_none());
    assert_eq!(fs::read_dir(t.0.join("out"))?.count(), 0);
    Ok(())
}
#[test]
fn bounded_history_and_app_submission_preserve_native_picked_paths() -> io::Result<()> {
    let t = Sandbox::new()?;
    let mut app = t.app()?;
    for i in 0..20 {
        app.copy_dialog.record(t.0.join(i.to_string()));
    }
    app.copy_dialog.record(t.0.join("19"));
    assert_eq!(app.copy_dialog.history.len(), 16);
    assert_eq!(app.copy_dialog.history[0], t.0.join("19"));
    #[cfg(unix)]
    let out = {
        use std::os::unix::ffi::OsStringExt;
        t.0.join(std::ffi::OsString::from_vec(b"out-\xff".to_vec()))
    };
    #[cfg(not(unix))]
    let out = t.0.join("native-out");
    let owner = copy_dialog::parent(&app).unwrap();
    copy_dialog::pick(&mut app, owner, out.clone());
    key(&mut app, K::Left);
    assert_eq!(
        app.modals.prompt().unwrap().copy.as_ref().unwrap().picked,
        Some(out.clone())
    );
    key(&mut app, K::Char('x'));
    assert!(app
        .modals
        .prompt()
        .unwrap()
        .copy
        .as_ref()
        .unwrap()
        .picked
        .is_none());
    // Native byte identity is tested above without requiring filesystem support
    // for invalid UTF-8 names (APFS can reject them). Perform writes in Unicode.
    let out = t.0.join("copie-界");
    fs::create_dir(&out)?;
    copy_dialog::pick(&mut app, owner, out.clone());
    copy_dialog::action(&mut app, Action::CheckSpace);
    copy_dialog::action(&mut app, Action::Verify);
    key(&mut app, K::Enter);
    wait(&mut app, |a| {
        a.job.is_none() && a.panels.iter().all(|p| !p.loading)
    });
    assert!(app.dialogs().is_empty());
    assert_eq!(fs::read(out.join("source.txt"))?, b"fixture bytes");
    assert_eq!(fs::read(t.0.join("source.txt"))?, b"fixture bytes");
    assert_eq!(app.copy_dialog.history[0], out);
    Ok(())
}

#[test]
fn native_copy_paint_is_repeatable_translated_clipped_and_localized() -> io::Result<()> {
    use dn::{layout, localization::Language, presentation::layout::Rect, testing::Buffer};
    let t = Sandbox::new()?;
    let mut app = t.app()?;
    for language in [Language::English, Language::Russian] {
        app.config.language = language;
        let snapshot = layout::prepare_window(
            &mut app,
            Rect::new(7, 3, 80, 25),
            Rect::new(0, 0, 83, 27),
            Instant::now(),
        );
        let before = app.modals.prompt().unwrap().copy.as_ref().unwrap().options;
        let first = ui::render(&app, &snapshot);
        assert!(first
            .hit_regions()
            .iter()
            .all(|(r, _)| r.x >= 7 && r.y >= 3 && r.right() <= 83 && r.bottom() <= 27));
        let field = first
            .hit_regions()
            .iter()
            .find(|(_, t)| *t == dn::mouse::Target::PromptField)
            .unwrap()
            .0;
        assert_eq!(first.cursor(), Some((field.x, field.y)));
        assert_eq!(
            Buffer::rendered(first),
            Buffer::rendered(ui::render(&app, &snapshot))
        );
        assert_eq!(
            app.modals.prompt().unwrap().copy.as_ref().unwrap().options,
            before
        );
    }
    input::handle(&mut app, KeyEvent::new(K::Char('х'), M::ALT));
    assert!(
        app.modals
            .prompt()
            .unwrap()
            .copy
            .as_ref()
            .unwrap()
            .options
            .remove_sources
    );
    Ok(())
}

#[test]
fn skip_completion_is_visible_and_default_conflicts_still_refuse() -> io::Result<()> {
    let t = Sandbox::new()?;
    let mut app = t.app()?;
    fs::write(t.0.join("out/source.txt"), b"existing winner")?;
    let owner = copy_dialog::parent(&app).unwrap();
    copy_dialog::pick(&mut app, owner, t.0.join("out"));
    key(&mut app, K::Enter);
    wait(&mut app, |a| {
        a.job.is_none() && a.panels.iter().all(|p| !p.loading)
    });
    assert!(app.last_operation.as_ref().unwrap().is_err());
    assert!(app.dialogs().is_empty());
    assert_eq!(
        app.transfers.selected().unwrap().status,
        dn::transfers::Status::Failed
    );
    key(&mut app, K::Esc);
    let panel = &mut app.panels[app.active];
    panel.cursor = panel
        .entries
        .iter()
        .position(|e| e.path == t.0.join("source.txt"))
        .unwrap();
    app.prompt_operation(Kind::Copy, false);
    let owner = copy_dialog::parent(&app).unwrap();
    copy_dialog::pick(&mut app, owner, t.0.join("out"));
    copy_dialog::action(&mut app, Action::Skip);
    key(&mut app, K::Enter);
    dn::transfers::action(&mut app, dn::transfers::Action::Resume);
    wait(&mut app, |a| {
        !a.transfers.has_work() && a.panels.iter().all(|p| !p.loading)
    });
    let p = app.last_operation.as_ref().unwrap().as_ref().unwrap();
    assert_eq!((p.files, p.skipped), (0, 1));
    assert!(draw(&mut app, 80, 25)?.contains("Skipped: 1"));
    assert_eq!(app.transfers.selected().unwrap().progress.details.len(), 1);
    assert_eq!(fs::read(t.0.join("out/source.txt"))?, b"existing winner");
    assert_eq!(fs::read(t.0.join("source.txt"))?, b"fixture bytes");
    key(&mut app, K::Esc);
    assert!(app.dialogs().is_empty());
    Ok(())
}
