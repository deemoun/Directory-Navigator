//! Dialog and worker integration uses only private temporary fixtures.
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use dn::{
    app::App,
    copy_dialog::{self, Action},
    directory_tree, input,
    operations::Kind,
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
            "dn-move-dialog-{}-{}",
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
        app.prompt_operation(Kind::Move, false);
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
fn dn_move_layout_locked_removal_focus_mouse_and_resize() -> io::Result<()> {
    let t = Sandbox::new()?;
    let mut app = t.app()?;
    for (w, h) in [(80, 25), (132, 40), (80, 25)] {
        let screen = draw(&mut app, w, h)?;
        assert!(screen.contains("Rename/move"));
        assert!(screen.contains("Move/rename file source.txt to"));
        assert!(screen.contains("[X] Remove source files"));
        assert!(!app
            .mouse
            .regions
            .iter()
            .any(|(_, t)| *t == dn::mouse::Target::Copy(Action::RemoveSources)));
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
    copy_dialog::action(&mut app, Action::RemoveSources);
    assert_eq!(app.modals.prompt().unwrap().focus, 0);
    assert!(
        !app.modals
            .prompt()
            .unwrap()
            .copy
            .as_ref()
            .unwrap()
            .options
            .remove_sources
    );
    let mut focuses = Vec::new();
    for _ in 0..10 {
        key(&mut app, K::Tab);
        focuses.push(app.modals.prompt().unwrap().focus);
    }
    assert_eq!(focuses, vec![10, 1, 2, 3, 4, 6, 7, 8, 9, 0]);
    copy_dialog::action(&mut app, Action::Verify);
    assert!(
        app.modals
            .prompt()
            .unwrap()
            .copy
            .as_ref()
            .unwrap()
            .options
            .verify_writes
    );
    key(&mut app, K::Tab);
    assert_eq!(app.modals.prompt().unwrap().focus, 6);
    draw(&mut app, 50, 12)?;
    key(&mut app, K::Enter);
    assert!(app.modals.prompt().is_some() && app.job.is_none());
    key(&mut app, K::Esc);
    assert!(app.modals.prompt().is_none());
    assert_eq!(fs::read(t.0.join("source.txt"))?, b"fixture bytes");
    Ok(())
}
#[test]
fn f6_and_shift_f6_preserve_bases_and_native_rename_identity() -> io::Result<()> {
    for rename_here in [false, true] {
        let t = Sandbox::new()?;
        let mut app = t.app()?;
        app.modals.set_prompt(None);
        app.panels[app.active ^ 1].go_to(t.0.join("out"), app.config.show_hidden);
        wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
        let original = fs::symlink_metadata(t.0.join("source.txt"))?;
        input::handle(
            &mut app,
            KeyEvent::new(K::F(6), if rename_here { M::SHIFT } else { M::NONE }),
        );
        let expected = if rename_here {
            t.0.join("renamed-界.txt")
        } else {
            t.0.join("out/source.txt")
        };
        if rename_here {
            input::paste(&mut app, "renamed-界.txt");
        }
        key(&mut app, K::Enter);
        assert_eq!(app.job.as_ref().unwrap().kind, Kind::Move);
        wait(&mut app, |a| {
            a.job.is_none() && a.panels.iter().all(|p| !p.loading)
        });
        assert!(app.last_operation.as_ref().unwrap().is_ok());
        assert!(app.dialogs().is_empty());
        assert!(!t.0.join("source.txt").exists());
        assert_eq!(fs::read(&expected)?, b"fixture bytes");
        assert!(dn::platform_fs::same_identity(
            &original,
            &fs::symlink_metadata(&expected)?
        ));
        assert_eq!(
            app.copy_dialog.history[0],
            if rename_here {
                expected
            } else {
                t.0.join("out")
            }
        );
    }
    Ok(())
}
#[test]
fn move_refusal_and_skip_never_replace_destinations_or_remove_skipped_sources() -> io::Result<()> {
    let t = Sandbox::new()?;
    let mut app = t.app()?;
    fs::write(t.0.join("out/source.txt"), b"winner")?;
    let owner = copy_dialog::parent(&app).unwrap();
    copy_dialog::pick(&mut app, owner, t.0.join("out"));
    key(&mut app, K::Enter);
    wait(&mut app, |a| {
        a.job.is_none() && a.panels.iter().all(|p| !p.loading)
    });
    assert!(app.last_operation.as_ref().unwrap().is_err());
    key(&mut app, K::Esc);
    app.prompt_operation(Kind::Move, false);
    let owner = copy_dialog::parent(&app).unwrap();
    copy_dialog::pick(&mut app, owner, t.0.join("out"));
    copy_dialog::action(&mut app, Action::Skip);
    key(&mut app, K::Enter);
    wait(&mut app, |a| {
        a.job.is_none() && a.panels.iter().all(|p| !p.loading)
    });
    let p = app.last_operation.as_ref().unwrap().as_ref().unwrap();
    assert_eq!((p.files, p.skipped), (0, 1));
    let screen = draw(&mut app, 80, 25)?;
    assert!(screen.contains("Move complete") && screen.contains("Moved items: 0"));
    assert_eq!(fs::read(t.0.join("out/source.txt"))?, b"winner");
    assert_eq!(fs::read(t.0.join("source.txt"))?, b"fixture bytes");
    Ok(())
}
#[test]
fn move_children_share_history_restore_draft_and_reject_retired_receipts() -> io::Result<()> {
    let t = Sandbox::new()?;
    let mut app = t.app()?;
    let owner = copy_dialog::parent(&app).unwrap();
    let panels = app.panels.each_ref().map(|p| p.path.clone());
    app.copy_dialog.record(t.0.join("out"));
    copy_dialog::action(&mut app, Action::History);
    key(&mut app, K::Enter);
    copy_dialog::action(&mut app, Action::Verify);
    key(&mut app, K::F(1));
    assert!(draw(&mut app, 80, 25)?.contains("Native no-replace"));
    key(&mut app, K::Esc);
    assert_eq!(copy_dialog::parent(&app), Some(owner));
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
    assert!(
        app.modals
            .prompt()
            .unwrap()
            .copy
            .as_ref()
            .unwrap()
            .options
            .verify_writes
    );
    copy_dialog::action(&mut app, Action::Tree);
    app.modals.set_prompt(None);
    assert!(app.directory_tree().is_none());
    app.prompt_operation(Kind::Move, true);
    copy_dialog::pick(&mut app, owner, t.0.join("wrong"));
    app.poll();
    assert!(app.modals.prompt().unwrap().field.is_empty());
    assert!(app.job.is_none() && t.0.join("source.txt").exists());
    Ok(())
}
#[test]
fn localized_move_render_is_immutable_and_hits_translate_with_cursor() -> io::Result<()> {
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
    }
    input::handle(&mut app, KeyEvent::new(K::Char('х'), M::ALT));
    assert_eq!(app.modals.prompt().unwrap().focus, 0);
    Ok(())
}
