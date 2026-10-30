mod support;
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use dn::{
    desktop::WindowId, input, layout, menu, presentation::layout::Rect,
    windowing::geometry::SurfaceKey,
};
use std::io;

#[test]
fn tool_payload_and_blocking_child_have_distinct_lifetimes_and_request_owners() -> io::Result<()> {
    let f = support::Fixture::new("window-state-ownership")?;
    let mut app = f.app()?;
    dn::directory_tree::open(&mut app);
    let tool = app.desktop.active().id;
    assert!(app.modal_infos().is_empty());
    app.error("Owned blocking child");
    let child = app.modal_top().unwrap();
    assert_eq!(child.owner, dn::modals::ModalOwner::Document(tool));
    input::handle(&mut app, KeyEvent::new(K::F(6), M::ALT));
    assert_eq!(app.desktop.active().id, tool);
    app.pop_dialog();
    app.sync_modals();
    assert!(app.desktop.tool(tool).unwrap().tree().is_some());
    assert!(app.modal_infos().is_empty());
    input::handle(&mut app, KeyEvent::new(K::Esc, M::NONE));
    assert!(!app.desktop.contains(tool));
    Ok(())
}

#[test]
fn opening_reopening_and_close_fallback_share_focus_cleanup() -> io::Result<()> {
    let f = support::Fixture::new("window-focus-transitions")?;
    let mut app = f.app()?;
    dn::editor::open(&mut app, true);
    let editor = app.desktop.active().id;
    support::draw(&mut app, 100, 32)?;
    let revision = app.layout_revision;
    app.desktop
        .active_editor_mut()
        .unwrap()
        .modals
        .set_editor_menu(Some((0, 0)));
    // Even callers of the low-level desktop API reconcile through the same path.
    app.desktop.activate(WindowId::MANAGER);
    app.sync_modals();
    assert!(app
        .desktop
        .editor(editor)
        .unwrap()
        .modals
        .editor_menu()
        .is_none());
    assert!(app.layout_revision > revision);
    dn::directory_tree::open(&mut app);
    let tree = app.desktop.active().id;
    support::draw(&mut app, 100, 32)?;
    menu::open_root(&mut app, menu::RootId::File);
    dn::editor::open(&mut app, true);
    let second = app.desktop.active().id;
    app.sync_modals();
    assert!(app.modals.menu().is_none());
    app.desktop.close(second);
    app.sync_modals();
    assert_eq!(app.desktop.active().id, tree);
    dn::directory_tree::open(&mut app);
    assert_eq!(app.desktop.active().id, tree);
    assert!(app.desktop.contains(editor));
    Ok(())
}

#[test]
fn background_document_publication_preserves_foreground_menu_and_focus_intent() -> io::Result<()> {
    let f = support::Fixture::new("window-background-selection")?;
    let mut app = f.app()?;
    dn::directory_tree::open(&mut app);
    let foreground = app.desktop.active().id;
    support::draw(&mut app, 100, 32)?;
    menu::open_root(&mut app, menu::RootId::File);
    let intent = app.desktop.activation_revision();
    let path = f.0.join("background.txt");
    std::fs::write(&path, "owned background contents")?;
    dn::editor::open_path_background(&mut app, path);
    app.sync_modals();
    assert_eq!(app.desktop.active().id, foreground);
    assert_eq!(app.desktop.activation_revision(), intent);
    assert!(app.modals.menu().is_some());
    Ok(())
}

#[test]
fn native_windows_and_blocking_forms_consume_declared_capabilities() -> io::Result<()> {
    let f = support::Fixture::new("window-capabilities")?;
    let mut app = f.app()?;
    dn::directory_tree::open(&mut app);
    let tree = app.desktop.active().id;
    dn::utilities::open(&mut app, dn::utilities::Kind::Characters);
    let chart = app.desktop.active().id;
    let snapshot = layout::prepare(&mut app, Rect::new(0, 0, 100, 32));
    let caps = |id| {
        snapshot
            .surfaces
            .iter()
            .find(|s| s.id.key == SurfaceKey::Document(id))
            .unwrap()
            .caps
    };
    assert!(caps(tree).movable && caps(tree).resize && caps(tree).zoom);
    assert_eq!(caps(tree).minimum, (48, 10));
    assert!(caps(tree).tile && caps(tree).cascade);
    assert!(caps(chart).movable && !caps(chart).resize && !caps(chart).zoom);
    assert!(caps(chart).tile && caps(chart).cascade);
    app.prompt_goto();
    let snapshot = layout::prepare(&mut app, Rect::new(0, 0, 100, 32));
    let modal = app.modal_top().unwrap().id;
    let form = snapshot
        .surfaces
        .iter()
        .find(|s| s.id.key == SurfaceKey::Modal(modal))
        .unwrap();
    assert!(form.caps.movable);
    assert!(!form.caps.resize && !form.caps.zoom && !form.caps.tile && !form.caps.cascade);
    Ok(())
}
