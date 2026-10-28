//! Shared-control pilot regressions and optional before/after appearance capture.
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use dn::{
    app::App,
    localization::Language,
    operations::Kind,
    palette::PaletteProfile,
    settings::{Settings, SettingsKind},
    testing::{backend::TestBackend, Terminal},
    ui,
};
use dn::{
    input,
    mouse::{self, Target},
    testing::{layout::Rect, Buffer},
};
use std::{
    fs, io,
    time::{Duration, Instant},
};
#[test]
fn pilot_appearance_capture() -> io::Result<()> {
    let path = std::env::temp_dir().join(format!("dn-shared-dialogs-{}", std::process::id()));
    fs::create_dir(&path)?;
    fs::write(path.join("fixture.txt"), "safe fixture")?;
    let result = (|| {
        let mut app = App::new(path.clone())?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.panels.iter().any(|p| p.loading) {
            app.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        app.panels[0].cursor = app.panels[0]
            .entries
            .iter()
            .position(|e| e.name == "fixture.txt")
            .unwrap();
        let mut capture = String::new();
        for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            for lang in [Language::English, Language::Russian] {
                for (w, h) in [(80, 25), (132, 40)] {
                    app.config.palette = profile;
                    app.config.language = lang;
                    for scene in 0..9 {
                        app.modals.set_prompt(None);
                        app.modals.set_settings(None);
                        let (width, height) = if scene < 6 {
                            let kind = [
                                SettingsKind::System,
                                SettingsKind::Interface,
                                SettingsKind::FileManager,
                                SettingsKind::Highlight,
                                SettingsKind::Keyboard,
                                SettingsKind::Regional,
                            ][scene];
                            app.modals
                                .set_settings(Some(Settings::new(kind, &app.config)));
                            [(60, 14), (60, 13), (74, 14), (60, 18), (74, 20), (68, 17)][scene]
                        } else {
                            app.prompt_operation(
                                if scene == 6 {
                                    Kind::Mkdir
                                } else if scene == 7 {
                                    Kind::Delete
                                } else {
                                    Kind::Trash
                                },
                                false,
                            );
                            assert!(app.modals.prompt().is_some(), "fixture must open a prompt");
                            if scene == 6 {
                                (49, 7)
                            } else {
                                (59, 11)
                            }
                        };
                        let mut terminal = Terminal::new(TestBackend::new(w, h))?;
                        terminal.draw(|f| ui::draw(f, &mut app))?;
                        let b = terminal.backend().buffer();
                        assert!(app
                            .mouse
                            .regions
                            .iter()
                            .all(|(r, _)| r.right() <= w && r.bottom() <= h));
                        assert!(app.mouse.regions.iter().any(|(_, t)| *t
                            == if scene < 6 {
                                Target::CancelSettings
                            } else {
                                Target::CancelPrompt
                            }));
                        capture.push_str(&format!("{profile:?} {lang:?} {w} {h} {scene}\n"));
                        let x = (w - width) / 2;
                        let y = (h - height) / 2;
                        for yy in y..y + height {
                            if scene == 5 && yy == y + 9 {
                                continue;
                            }
                            for xx in x..x + width {
                                capture.push_str(&format!("{:?}", b[(xx, yy)]));
                            }
                        }
                        capture.push_str(&format!("\n{:?}\n", app.mouse.regions));
                    }
                }
            }
        }
        if let Some(output) = std::env::var_os("DN_PILOT_CAPTURE") {
            fs::write(output, capture)?;
        }
        Ok(())
    })();
    fs::remove_dir_all(path)?;
    result
}

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn app() -> io::Result<(Self, App)> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = Self(std::env::temp_dir().join(format!(
            "dn-shared-controls-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )));
        fs::create_dir(&root.0)?;
        fs::write(root.0.join("keep.txt"), "keep")?;
        let mut app = App::new(root.0.clone())?;
        app.config.save_on_exit = false;
        app.config_store = dn::config::Store::new(Some(root.0.join("config.conf")));
        wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
        app.panels[0].cursor = app.panels[0]
            .entries
            .iter()
            .position(|e| e.name == "keep.txt")
            .unwrap();
        Ok((root, app))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(app: &mut App, ready: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !ready(app) {
        app.poll();
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn key(app: &mut App, code: K) {
    input::handle(app, KeyEvent::new(code, M::NONE));
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<()> {
    let mut t = Terminal::new(TestBackend::new(w, h))?;
    t.draw(|f| ui::draw(f, app))?;
    Ok(())
}
fn click(app: &mut App, target: Target) {
    let r = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == target)
        .unwrap()
        .0;
    mouse::handle(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: M::NONE,
        },
    );
}

#[test]
fn prompt_unicode_paste_limits_nested_help_and_safe_cancellation() -> io::Result<()> {
    let (root, mut app) = Fixture::app()?;
    app.prompt_operation(Kind::Mkdir, false);
    for ch in "界е\u{301}👩\u{200d}💻\u{f0000}".chars() {
        key(&mut app, K::Char(ch));
    }
    let text = app.modals.prompt().unwrap().field.clone();
    assert_eq!(text, "界е\u{301}👩\u{200d}💻\u{f0000}");
    input::handle(&mut app, KeyEvent::new(K::Char('q'), M::CONTROL));
    assert_eq!(app.modals.prompt().unwrap().field, text);
    key(&mut app, K::F(1));
    key(&mut app, K::Esc);
    assert_eq!(app.modals.prompt().unwrap().field, text);
    key(&mut app, K::Tab);
    input::paste(&mut app, "ignored");
    key(&mut app, K::Char('x'));
    assert_eq!(app.modals.prompt().unwrap().field, text);
    input::handle(&mut app, KeyEvent::new(K::Tab, M::SHIFT));
    let prompt = app.modals.prompt_mut().unwrap();
    prompt.field = "a".repeat(16384);
    prompt.cursor = 16384;
    input::paste(&mut app, "b");
    assert_eq!(app.modals.prompt().unwrap().field.len(), 16384);
    key(&mut app, K::Tab);
    key(&mut app, K::Tab);
    key(&mut app, K::Enter);
    assert!(app.modals.prompt().is_none());
    assert_eq!(fs::read_to_string(root.0.join("keep.txt"))?, "keep");
    assert_eq!(fs::read_dir(&root.0)?.count(), 1);
    for kind in [Kind::Delete, Kind::Trash] {
        app.config.use_trash = false;
        app.prompt_operation(kind, false);
        key(&mut app, K::Tab);
        assert_eq!(app.modals.prompt().unwrap().focus, 2);
        key(&mut app, K::Enter);
        assert!(app.job.is_none());
        assert!(root.0.join("keep.txt").is_file());
    }
    app.prompt_operation(Kind::Mkdir, false);
    input::paste(&mut app, "created safely");
    key(&mut app, K::Enter);
    wait(&mut app, |a| a.job.is_none());
    assert!(root.0.join("created safely").is_dir());
    Ok(())
}
#[test]
fn all_settings_share_forward_reverse_actions_and_choice_drafts() -> io::Result<()> {
    let (_root, mut app) = Fixture::app()?;
    for kind in [
        SettingsKind::System,
        SettingsKind::Interface,
        SettingsKind::FileManager,
        SettingsKind::Highlight,
        SettingsKind::Keyboard,
        SettingsKind::Regional,
    ] {
        app.open_settings(kind);
        let count = app.modals.settings().unwrap().control_count();
        for expected in (1..count + 2).chain(std::iter::once(0)) {
            key(&mut app, K::Tab);
            assert_eq!(app.modals.settings().unwrap().cursor, expected);
        }
        input::handle(&mut app, KeyEvent::new(K::Tab, M::SHIFT));
        assert_eq!(app.modals.settings().unwrap().cursor, count + 1);
        draw(&mut app, 132, 40)?;
        key(&mut app, K::Enter);
        assert!(app.modals.settings().is_none());
    }
    for kind in [SettingsKind::Keyboard, SettingsKind::Regional] {
        app.open_settings(kind);
        draw(&mut app, 80, 25)?;
        let parent = app.modal_top().unwrap();
        key(&mut app, K::F(4));
        assert_ne!(app.modal_top().unwrap().id, parent.id);
        key(&mut app, K::Down);
        key(&mut app, K::Esc);
        assert_eq!(app.modal_top().unwrap().id, parent.id);
        assert_eq!(app.modals.settings().unwrap().cursor, 0);
        key(&mut app, K::F(4));
        key(&mut app, K::Down);
        key(&mut app, K::Enter);
        assert_eq!(app.modal_top().unwrap().id, parent.id);
        key(&mut app, K::Esc);
    }
    app.open_settings(SettingsKind::Interface);
    draw(&mut app, 80, 25)?;
    let previous = app.modals.settings().unwrap().draft.highlight_files;
    click(&mut app, Target::Setting(0));
    assert_ne!(
        app.modals.settings().unwrap().draft.highlight_files,
        previous
    );
    key(&mut app, K::Char(' '));
    assert_eq!(
        app.modals.settings().unwrap().draft.highlight_files,
        previous
    );
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::AcceptSettings);
    assert!(app.modals.settings().is_none());
    Ok(())
}
#[test]
fn pilot_paint_is_immutable_translated_and_stale_targets_do_not_activate() -> io::Result<()> {
    let (root, mut app) = Fixture::app()?;
    for settings in [false, true] {
        if settings {
            app.open_settings(SettingsKind::Regional);
        } else {
            app.prompt_operation(Kind::Mkdir, false);
            input::paste(&mut app, "untouched");
        }
        let snapshot = dn::layout::prepare_window(
            &mut app,
            Rect::new(7, 3, 80, 25),
            Rect::new(0, 0, 100, 35),
            Instant::now(),
        );
        let revision = app.layout_revision;
        let a = ui::render(&app, &snapshot);
        let hits = a.hit_regions().to_vec();
        let cursor = a.cursor();
        let b = ui::render(&app, &snapshot);
        assert_eq!(hits, b.hit_regions());
        assert_eq!(cursor, b.cursor());
        assert_eq!(Buffer::rendered(a), Buffer::rendered(b));
        assert_eq!(app.layout_revision, revision);
        assert!(hits
            .iter()
            .all(|(r, _)| r.x >= 7 && r.y >= 3 && r.right() <= 87 && r.bottom() <= 28));
        if settings {
            key(&mut app, K::Esc);
        } else {
            assert_eq!(app.modals.prompt().unwrap().field, "untouched");
            key(&mut app, K::Esc);
        }
    }
    app.prompt_operation(Kind::Mkdir, false);
    input::paste(&mut app, "must not create");
    draw(&mut app, 80, 25)?;
    let r = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::SubmitPrompt)
        .unwrap()
        .0;
    app.error("New modal owns input");
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: M::NONE,
        },
    );
    assert!(!root.0.join("must not create").exists());
    assert!(app.job.is_none());
    Ok(())
}

#[test]
fn settings_accept_localized_mnemonics_and_original_aliases() -> io::Result<()> {
    let (_root, mut app) = Fixture::app()?;
    for language in [Language::English, Language::Russian] {
        app.config.language = language;
        app.open_settings(SettingsKind::Interface);
        let original = app.modals.settings().unwrap().draft.highlight_files;
        key(&mut app, K::Char('h'));
        assert_ne!(
            app.modals.settings().unwrap().draft.highlight_files,
            original
        );
        let translated =
            dn::menu::mnemonic(language.text(dn::settings::Setting::Highlight.label())).unwrap();
        key(&mut app, K::Char(translated));
        assert_eq!(
            app.modals.settings().unwrap().draft.highlight_files,
            original
        );
        key(&mut app, K::Esc);
    }
    Ok(())
}
