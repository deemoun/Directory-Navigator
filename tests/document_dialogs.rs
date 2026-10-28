//! Document dialog snapshots and shared-control behaviour regressions.
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use dn::{
    app::App,
    editor::{
        self,
        search::{ReplaceSession, SearchDialog, SearchMemory},
        Modal, PromptKind,
    },
    localization::Language,
    palette::PaletteProfile,
    testing::{backend::TestBackend, Terminal},
    ui,
    viewer::{self, Viewer},
};
use dn::{
    editor::EditorAction as A,
    input,
    mouse::{self, Target},
    testing::{layout::Rect, Buffer},
};
use std::{
    fs, io,
    time::{Duration, Instant},
};
#[test]
fn document_dialog_appearance_capture() -> io::Result<()> {
    let path = std::env::temp_dir().join(format!("dn-doc-dialog-capture-{}", std::process::id()));
    fs::create_dir(&path)?;
    let result = (|| {
        let mut app = App::new(path.clone())?;
        let ready = Instant::now() + Duration::from_secs(5);
        while app.panels.iter().any(|p| p.loading) {
            app.poll();
            assert!(Instant::now() < ready);
            std::thread::sleep(Duration::from_millis(1));
        }
        editor::open(&mut app, true);
        editor::paste(&mut app, "alpha 界é beta alpha\n");
        let editor_id = app.desktop.active().id;
        let viewer_id = app.desktop.open_viewer(Viewer::from_contents(
            "preview".into(),
            viewer::from_bytes(b"alpha beta".to_vec(), viewer::ViewMode::Text),
        ))?;
        let end = Instant::now() + Duration::from_secs(5);
        while app.desktop.identity_pending(viewer_id) {
            app.desktop.poll_identities();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut capture = String::new();
        for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            for language in [Language::English, Language::Russian] {
                for (w, h) in [(80, 25), (132, 40)] {
                    for scene in 0..9 {
                        for cancel in [false, true] {
                            app.config.palette = profile;
                            app.config.language = language;
                            app.desktop
                                .editor_mut(editor_id)
                                .unwrap()
                                .modals
                                .set_editor_modal(None);
                            app.desktop
                                .viewer_mut(viewer_id)
                                .unwrap()
                                .modals
                                .set_viewer_search(None);
                            let (width, height) = if scene == 8 {
                                app.desktop.activate(viewer_id);
                                let query = format!("{}界é", "long-".repeat(15));
                                let cursor = query.len();
                                app.desktop
                                    .viewer_mut(viewer_id)
                                    .unwrap()
                                    .modals
                                    .set_viewer_search(Some(viewer::SearchDialog {
                                        memory: viewer::SearchMemory {
                                            query,
                                            case_sensitive: true,
                                        },
                                        cursor,
                                        focus: if cancel { 3 } else { 0 },
                                    }));
                                (58, 10)
                            } else {
                                app.desktop.activate(editor_id);
                                let modal = match scene {
                                    0..=2 => {
                                        let mut d = SearchDialog::new(
                                            scene != 0,
                                            &SearchMemory {
                                                query: "界é alpha".into(),
                                                replacement: "replacement".into(),
                                                ..Default::default()
                                            },
                                            if scene == 2 { 0..5 } else { 0..0 },
                                        );
                                        d.focus = if cancel { 10 } else { 0 };
                                        Modal::Search(d)
                                    }
                                    3 => {
                                        let mut s =
                                            ReplaceSession::new(vec![0..5, 16..21], "new".into());
                                        s.focus = if cancel { 3 } else { 0 };
                                        Modal::Replace(s)
                                    }
                                    4 => Modal::ConfirmClose {
                                        focus: if cancel { 2 } else { 0 },
                                    },
                                    5 | 6 => Modal::Prompt {
                                        kind: if scene == 5 {
                                            PromptKind::GoTo
                                        } else {
                                            PromptKind::TabWidth
                                        },
                                        field: "2".into(),
                                        cursor: 1,
                                        focus: if cancel { 2 } else { 0 },
                                    },
                                    _ => Modal::Help,
                                };
                                app.desktop
                                    .editor_mut(editor_id)
                                    .unwrap()
                                    .modals
                                    .set_editor_modal(Some(modal));
                                match scene {
                                    3 => (72, 15),
                                    4 => (66, 10),
                                    5 | 6 => (60, 7),
                                    _ => (72, 20),
                                }
                            };
                            let mut terminal = Terminal::new(TestBackend::new(w, h))?;
                            terminal.draw(|f| ui::draw(f, &mut app))?;
                            capture.push_str(&format!(
                                "{profile:?} {language:?} {w} {h} {scene} {cancel}\n"
                            ));
                            let b = terminal.backend().buffer();
                            let x = (w - width) / 2;
                            let y = (h - height) / 2;
                            for yy in y..y + height {
                                for xx in x..x + width {
                                    capture.push_str(&format!("{:?}", b[(xx, yy)]));
                                }
                            }
                            capture.push_str(&format!(
                                "\n{:?}\n{:?}\n",
                                app.mouse.regions,
                                terminal.get_cursor_position()?
                            ));
                        }
                    }
                }
            }
        }
        if let Some(output) = std::env::var_os("DN_DOCUMENT_CAPTURE") {
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
            "dn-doc-controls-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )));
        fs::create_dir(&root.0)?;
        let mut app = App::new(root.0.clone())?;
        app.config.save_on_exit = false;
        app.config_store = dn::config::Store::new(Some(root.0.join("config.conf")));
        let end = Instant::now() + Duration::from_secs(5);
        while app.panels.iter().any(|p| p.loading) {
            app.poll();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        editor::open(&mut app, true);
        editor::paste(&mut app, "alpha beta alpha\n");
        Ok((root, app))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn key(app: &mut App, code: K) {
    input::handle(app, KeyEvent::new(code, M::NONE));
}
fn focus(app: &App) -> usize {
    match app
        .desktop
        .active_editor()
        .unwrap()
        .modals
        .editor_modal()
        .unwrap()
    {
        Modal::Search(d) => d.focus,
        Modal::Replace(s) => s.focus,
        Modal::Prompt { focus, .. } | Modal::ConfirmClose { focus } => *focus,
        _ => 0,
    }
}
fn draw(app: &mut App) -> io::Result<()> {
    let mut t = Terminal::new(TestBackend::new(80, 25))?;
    t.draw(|f| ui::draw(f, app))?;
    Ok(())
}

#[test]
fn document_search_traversal_skips_unavailable_controls_and_preserves_edits() -> io::Result<()> {
    let (_root, mut app) = Fixture::app()?;
    let original = app
        .desktop
        .active_editor()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .text
        .clone();
    for (replacing, selected, expected) in [
        (false, false, vec![2, 3, 5, 7, 8, 10, 0]),
        (true, false, vec![1, 2, 3, 4, 5, 7, 8, 9, 10, 0]),
        (true, true, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 0]),
    ] {
        app.desktop
            .active_editor_mut()
            .unwrap()
            .modals
            .set_editor_modal(Some(Modal::Search(SearchDialog::new(
                replacing,
                &SearchMemory::default(),
                if selected { 0..5 } else { 0..0 },
            ))));
        for choice in &expected {
            key(&mut app, K::Tab);
            assert_eq!(focus(&app), *choice);
        }
        for choice in expected.iter().rev().skip(1).chain(std::iter::once(&0)) {
            input::handle(&mut app, KeyEvent::new(K::Tab, M::SHIFT));
            assert_eq!(focus(&app), *choice);
        }
        key(&mut app, K::BackTab);
        assert_eq!(focus(&app), 10);
        key(&mut app, K::Enter);
        assert!(app
            .desktop
            .active_editor()
            .unwrap()
            .modals
            .editor_modal()
            .is_none());
        assert_eq!(
            app.desktop
                .active_editor()
                .unwrap()
                .buffer
                .as_ref()
                .unwrap()
                .text,
            original
        );
    }
    editor::action(&mut app, A::Find);
    input::paste(&mut app, "界é");
    key(&mut app, K::Tab);
    input::paste(&mut app, "ignored");
    input::handle(&mut app, KeyEvent::new(K::Char(' '), M::ALT));
    if let Some(Modal::Search(d)) = app.desktop.active_editor().unwrap().modals.editor_modal() {
        assert_eq!(d.query, "界é");
        assert!(d.options.case_sensitive);
    }
    key(&mut app, K::Char(' '));
    if let Some(Modal::Search(d)) = app.desktop.active_editor().unwrap().modals.editor_modal() {
        assert!(!d.options.case_sensitive);
    }
    key(&mut app, K::Esc);
    editor::action(&mut app, A::Undo);
    assert!(app
        .desktop
        .active_editor()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .text
        .is_empty());
    editor::action(&mut app, A::Redo);
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        original
    );
    Ok(())
}
#[test]
fn numeric_prompts_and_unsaved_or_replace_cancel_keep_document_guards() -> io::Result<()> {
    let (root, mut app) = Fixture::app()?;
    let id = app.desktop.active().id;
    let original = app
        .desktop
        .active_editor()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .text
        .clone();
    for action in [A::GoTo, A::TabWidth] {
        editor::action(&mut app, action);
        input::handle(&mut app, KeyEvent::new(K::Char('a'), M::CONTROL));
        input::paste(&mut app, "2");
        key(&mut app, K::BackTab);
        assert_eq!(focus(&app), 2);
        key(&mut app, K::Enter);
        assert_eq!(
            app.desktop
                .active_editor()
                .unwrap()
                .buffer
                .as_ref()
                .unwrap()
                .text,
            original
        );
    }
    app.desktop
        .active_editor_mut()
        .unwrap()
        .modals
        .set_editor_modal(Some(Modal::Replace(ReplaceSession::new(
            vec![0..5, 11..16],
            "changed".into(),
        ))));
    assert_eq!(focus(&app), 3);
    key(&mut app, K::Enter);
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        original
    );
    app.desktop
        .active_editor_mut()
        .unwrap()
        .modals
        .set_editor_modal(Some(Modal::ConfirmClose { focus: usize::MAX }));
    key(&mut app, K::Enter);
    assert!(app.desktop.contains(id));
    assert!(app
        .desktop
        .active_editor()
        .unwrap()
        .modals
        .editor_modal()
        .is_none());
    let mut stale = ReplaceSession::new(std::iter::once(0..5).collect(), "must not replace".into());
    stale.focus = usize::MAX;
    app.desktop
        .active_editor_mut()
        .unwrap()
        .modals
        .set_editor_modal(Some(Modal::Replace(stale)));
    key(&mut app, K::Enter);
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        original
    );
    editor::action(&mut app, A::Close);
    assert_eq!(focus(&app), 2);
    key(&mut app, K::Enter);
    assert!(app.desktop.contains(id));
    assert!(app.desktop.active_editor().unwrap().dirty());
    editor::action(&mut app, A::Close);
    key(&mut app, K::BackTab);
    assert_eq!(focus(&app), 1);
    key(&mut app, K::Enter);
    assert!(!app.desktop.contains(id));
    assert_eq!(fs::read_dir(&root.0)?.count(), 0);
    Ok(())
}
#[test]
fn viewer_search_default_cancel_and_stale_hits_stay_with_their_owner() -> io::Result<()> {
    let (_root, mut app) = Fixture::app()?;
    let editor_id = app.desktop.active().id;
    let viewer_id = app.desktop.open_viewer(Viewer::from_contents(
        "preview".into(),
        viewer::from_bytes(b"Alpha alpha\n".to_vec(), viewer::ViewMode::Text),
    ))?;
    viewer::action(&mut app, viewer::Action::Find);
    input::paste(&mut app, "alpha");
    key(&mut app, K::Tab);
    input::paste(&mut app, "ignored");
    key(&mut app, K::Char(' '));
    key(&mut app, K::Enter);
    let viewer = app.desktop.active_viewer().unwrap();
    assert!(viewer.modals.viewer_search().is_none());
    assert!(viewer.current.is_some());
    assert!(!app.viewer_search.case_sensitive);
    viewer::action(&mut app, viewer::Action::Find);
    key(&mut app, K::BackTab);
    key(&mut app, K::Enter);
    assert!(app
        .desktop
        .active_viewer()
        .unwrap()
        .modals
        .viewer_search()
        .is_none());
    viewer::action(&mut app, viewer::Action::Find);
    draw(&mut app)?;
    let hit = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::Viewer(viewer::Action::Submit))
        .unwrap()
        .0;
    app.desktop.activate(editor_id);
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: hit.x,
            row: hit.y,
            modifiers: M::NONE,
        },
    );
    assert!(app
        .desktop
        .viewer(viewer_id)
        .unwrap()
        .modals
        .viewer_search()
        .is_some());
    assert!(app
        .desktop
        .active_editor()
        .unwrap()
        .modals
        .editor_modal()
        .is_none());
    app.desktop.close(viewer_id);
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: hit.x,
            row: hit.y,
            modifiers: M::NONE,
        },
    );
    assert!(app.desktop.active_editor().unwrap().dirty());
    editor::action(&mut app, A::Find);
    let snap = dn::layout::prepare_window(
        &mut app,
        Rect::new(7, 3, 80, 25),
        Rect::new(0, 0, 100, 35),
        Instant::now(),
    );
    let first = ui::render(&app, &snap);
    let hits = first.hit_regions().to_vec();
    let cursor = first.cursor();
    let second = ui::render(&app, &snap);
    assert_eq!(hits, second.hit_regions());
    assert_eq!(cursor, second.cursor());
    assert_eq!(Buffer::rendered(first), Buffer::rendered(second));
    assert!(hits
        .iter()
        .all(|(r, _)| r.x >= 7 && r.y >= 3 && r.right() <= 87 && r.bottom() <= 28));
    Ok(())
}
