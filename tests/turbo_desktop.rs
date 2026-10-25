#![cfg(feature = "turbo-vision")]
use dn::{
    desktop::{Desktop, WindowId, WindowKind},
    focus::Focus,
    tetris::Game,
    turbo::desktop::{Receipt, Scope},
    viewer::{self, Viewer},
};
use std::io;
fn viewer() -> Viewer {
    Viewer::from_contents(
        "native-view".into(),
        viewer::from_bytes(b"preview".to_vec(), viewer::ViewMode::Text),
    )
}
#[test]
fn native_selection_is_the_only_document_focus_authority() -> io::Result<()> {
    let mut desktop = Desktop::default();
    let native = desktop.native();
    let game = desktop.open_game(Game::default())?;
    let preview = desktop.open_viewer(viewer())?;
    assert_eq!(native.active(), desktop.active());
    assert!(desktop.activate(game));
    assert_eq!(native.active().id, game);
    // The native input receipt names the same selected document that model
    // commands address; there is no separate event-window identity cache.
    assert_eq!(
        native.dispatch(),
        Some(Receipt {
            window: desktop.active(),
            scope: Scope::Content
        })
    );
    desktop.close_game();
    assert_eq!(native.active().id, preview);
    assert!(!desktop.activate(game));
    let replacement = desktop.open_game(Game::default())?;
    assert_ne!(replacement, game);
    assert_eq!(native.active().id, preview);
    desktop.close_viewer();
    assert_eq!(native.active().id, replacement);
    desktop.close_game();
    assert_eq!(native.active().id, WindowId::MANAGER);
    Ok(())
}
#[test]
fn native_modal_scope_keeps_input_on_top_across_switches_and_closes() -> io::Result<()> {
    let mut desktop = Desktop::default();
    let native = desktop.native();
    let game = desktop.open_game(Game::default())?;
    let preview = desktop.open_viewer(viewer())?;
    for focus in [
        Focus::Welcome,
        Focus::Settings,
        Focus::Message,
        Focus::WindowList,
        Focus::EditorModal,
        Focus::ViewerSearch,
        Focus::Prompt,
    ] {
        native.set_scope(Scope::Layer(focus));
        assert_eq!(
            native.dispatch(),
            Some(Receipt {
                window: desktop.active(),
                scope: Scope::Layer(focus)
            })
        );
        assert!(desktop.activate(game));
        assert_eq!(native.dispatch().unwrap().window.id, game);
        assert_eq!(native.dispatch().unwrap().scope, Scope::Layer(focus));
        assert!(desktop.activate(preview));
    }
    native.set_scope(Scope::Screensaver);
    desktop.close_viewer();
    assert_eq!(native.dispatch().unwrap().window.id, game);
    assert_eq!(native.dispatch().unwrap().scope, Scope::Screensaver);
    native.set_scope(Scope::Content);
    assert_eq!(
        native.dispatch(),
        Some(Receipt {
            window: desktop.active(),
            scope: Scope::Content
        })
    );
    assert_eq!(desktop.active().kind, WindowKind::Game);
    Ok(())
}
#[test]
fn repeated_modal_replacement_and_input_never_reuses_a_closed_document() -> io::Result<()> {
    let mut desktop = Desktop::default();
    let native = desktop.native();
    for _ in 0..500 {
        let id = desktop.open_viewer(viewer())?;
        native.set_scope(Scope::Layer(Focus::Message));
        assert_eq!(native.dispatch().unwrap().window.id, id);
        desktop.close_viewer();
        assert_eq!(native.dispatch().unwrap().window.id, WindowId::MANAGER);
        native.set_scope(Scope::Content);
        assert!(!desktop.activate(id));
        assert_eq!(native.dispatch().unwrap().window.id, WindowId::MANAGER);
    }
    Ok(())
}
