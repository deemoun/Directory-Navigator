mod support;
use crossterm::event::{KeyCode as K, KeyEvent, KeyEventKind, KeyModifiers as M};
use dn::{
    desktop::{WindowId, WindowKind},
    editor::{self, EditorAction as A},
    input, menu,
    windowing::geometry::SurfaceKey,
};
use std::{fs, io, time::Duration};
use support::{key, modified, Fixture};
fn surface(
    app: &mut dn::app::App,
    id: WindowId,
    width: u16,
    height: u16,
) -> dn::windowing::geometry::Surface {
    dn::layout::prepare(
        app,
        dn::presentation::layout::Rect::new(0, 0, width, height),
    )
    .surfaces
    .into_iter()
    .find(|s| s.id.key == SurfaceKey::Document(id))
    .unwrap()
}
fn pad(app: &mut dn::app::App) {
    modified(app, K::Char('q'), M::ALT);
}
fn notes(app: &dn::app::App) -> String {
    app.desktop
        .active_editor()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .text
        .clone()
}
#[test]
fn singleton_notes_toggle_back_to_each_caller_without_touching_panel_or_editor() -> io::Result<()> {
    let f = Fixture::new("smartpad-toggle")?;
    let mut app = f.app()?;
    let panel = app.panels[0].path.clone();
    pad(&mut app);
    let id = app.desktop.active().id;
    assert!(app.desktop.editor(id).unwrap().is_smartpad());
    editor::paste(&mut app, "Заметка 界 e\u{301}\n");
    let text = notes(&app);
    pad(&mut app);
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    pad(&mut app);
    assert_eq!(app.desktop.active().id, id);
    assert_eq!(notes(&app), text);
    // Held-key repeat must not bounce between documents.
    input::handle(
        &mut app,
        KeyEvent::new_with_kind(K::Char('q'), M::ALT, KeyEventKind::Repeat),
    );
    assert_eq!(app.desktop.active().id, id);
    pad(&mut app);
    editor::open(&mut app, true);
    let other = app.desktop.active().id;
    editor::paste(&mut app, "other draft");
    pad(&mut app);
    assert_eq!(app.desktop.active().id, id);
    pad(&mut app);
    assert_eq!(app.desktop.active().id, other);
    assert_eq!(notes(&app), "other draft");
    assert_eq!(app.desktop.document_ids(WindowKind::Editor).len(), 2);
    assert_eq!(app.panels[0].path, panel);
    Ok(())
}
#[test]
fn smartpad_is_floating_stable_translated_and_uses_normal_window_controls() -> io::Result<()> {
    let f = Fixture::new("smartpad-geometry")?;
    let mut app = f.app()?;
    for lang in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        app.config.language = lang;
        if lang == dn::localization::Language::Russian {
            app.config.palette = app
                .config
                .palette
                .with_slot(dn::palette::theme::Slot::DialogText, 0x1e);
        }
        menu::execute(&mut app, menu::Action::SmartPad);
        let id = app.desktop.active().id;
        for (w, h) in [(80, 25), (132, 40), (80, 25)] {
            let before = support::draw(&mut app, w, h)?;
            let initial = surface(&mut app, id, w, h);
            assert!(!initial.maximized);
            assert!(initial.bounds.height < h);
            assert!(before
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .contains("SmartPad"));
            for _ in 0..3 {
                assert_eq!(before, support::draw(&mut app, w, h)?);
            }
            assert_eq!(initial.bounds, surface(&mut app, id, w, h).bounds);
        }
        dn::windowing::action(&mut app, dn::windowing::Action::Zoom);
        support::draw(&mut app, 80, 25)?;
        assert!(surface(&mut app, id, 80, 25).maximized);
        assert_eq!(surface(&mut app, id, 80, 25).title_row, 1);
        dn::windowing::action(&mut app, dn::windowing::Action::Zoom);
        support::draw(&mut app, 80, 25)?;
        key(&mut app, K::Esc);
        assert!(!app.desktop.contains(id));
    }
    Ok(())
}
#[test]
fn dialogs_block_smartpad_dirty_close_cancel_discard_and_reopen_are_safe() -> io::Result<()> {
    let f = Fixture::new("smartpad-guards")?;
    let mut app = f.app()?;
    app.prompt_goto();
    pad(&mut app);
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    key(&mut app, K::Esc);
    pad(&mut app);
    let id = app.desktop.active().id;
    editor::paste(&mut app, "keep draft");
    key(&mut app, K::Esc);
    assert!(matches!(
        app.desktop.editor(id).unwrap().modals.editor_modal(),
        Some(editor::Modal::ConfirmClose { .. })
    ));
    pad(&mut app);
    assert_eq!(app.desktop.active().id, id);
    key(&mut app, K::Esc);
    assert_eq!(notes(&app), "keep draft");
    key(&mut app, K::Esc);
    editor::action(&mut app, A::Discard);
    assert!(!app.desktop.contains(id));
    pad(&mut app);
    assert_ne!(app.desktop.active().id, id);
    assert_eq!(notes(&app), "");
    let reopened = app.desktop.active().id;
    editor::paste(&mut app, "protect hidden notes on quit");
    pad(&mut app);
    app.request_exit();
    assert!(!app.quit);
    assert_eq!(app.desktop.active().id, reopened);
    assert!(matches!(
        app.desktop.editor(reopened).unwrap().modals.editor_modal(),
        Some(editor::Modal::ConfirmClose { .. })
    ));
    editor::action(&mut app, A::Cancel);
    assert!(!app.quit);
    assert_eq!(
        app.desktop
            .editor(reopened)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "protect hidden notes on quit"
    );
    Ok(())
}
#[test]
fn save_as_is_standard_editor_storage_and_keeps_singleton_identity() -> io::Result<()> {
    let f = Fixture::new("smartpad-save")?;
    let mut app = f.app()?;
    pad(&mut app);
    let id = app.desktop.active().id;
    let path = f.0.join("notes.txt");
    editor::paste(&mut app, "notes 界\n");
    editor::action(&mut app, A::Save);
    let Some(editor::Modal::File(picker)) = app
        .desktop
        .active_editor_mut()
        .unwrap()
        .modals
        .editor_modal_mut()
    else {
        panic!("Save As picker");
    };
    picker.set_filename(path.to_string_lossy().into_owned());
    editor::action(&mut app, A::Submit);
    support::wait_until(&mut app, Duration::from_secs(5), |a| {
        a.desktop.editor(id).is_some_and(|e| !e.busy())
    });
    assert_eq!(fs::read_to_string(&path)?, "notes 界\n");
    assert!(!app.desktop.editor(id).unwrap().dirty());
    assert!(app.desktop.editor(id).unwrap().is_smartpad());
    pad(&mut app);
    pad(&mut app);
    assert_eq!(app.desktop.active().id, id);
    Ok(())
}
#[test]
fn mac_prefix_keeps_existing_q_mapping_and_smartpad_uses_semicolon() -> io::Result<()> {
    let f = Fixture::new("smartpad-mac")?;
    let mut app = f.app()?;
    app.config.shortcuts = dn::shortcuts::Profile::Mac;
    modified(&mut app, K::Char('o'), M::CONTROL);
    key(&mut app, K::Char(';'));
    let id = app.desktop.active().id;
    assert!(app.desktop.editor(id).unwrap().is_smartpad());
    modified(&mut app, K::Char('o'), M::CONTROL);
    key(&mut app, K::Char(';'));
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    assert_eq!(
        dn::shortcuts::label(app.config.shortcuts, dn::focus::Screen::Editor, "Alt+Q"),
        "^O;"
    );
    Ok(())
}
#[test]
fn closed_origin_falls_back_to_manager_and_recovery_captures_hidden_notes() -> io::Result<()> {
    let f = Fixture::new("smartpad-recovery")?;
    let root = f.0.join("recovery");
    let mut app = f.app()?;
    editor::recovery::start(&mut app, root.clone());
    support::wait_until(&mut app, Duration::from_secs(5), |a| {
        a.editor_recovery.as_ref().is_some_and(|m| !m.busy())
    });
    editor::open(&mut app, true);
    let origin = app.desktop.active().id;
    pad(&mut app);
    let id = app.desktop.active().id;
    editor::paste(&mut app, "recover hidden notes");
    app.desktop.close(origin);
    pad(&mut app);
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    editor::recovery::checkpoint_now(&mut app);
    support::wait_until(&mut app, Duration::from_secs(5), |a| {
        a.editor_recovery.as_ref().is_some_and(|m| !m.busy())
    });
    let records = fs::read_dir(&root)?
        .flat_map(|s| fs::read_dir(s.unwrap().path()).unwrap())
        .filter_map(Result::ok)
        .filter(|f| f.path().extension().is_some_and(|e| e == "rec"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    assert_eq!(
        editor::recovery::store::read(&records[0].path())?.text,
        "recover hidden notes"
    );
    assert!(app.desktop.editor(id).unwrap().dirty());
    Ok(())
}
