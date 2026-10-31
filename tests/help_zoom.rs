mod support;
use dn::{
    app::App,
    dialog::Dialog,
    layout,
    mouse::Target,
    presentation::layout::Rect,
    windowing::geometry::{Chrome, Surface, SurfaceKey},
};
use std::io;
fn surface(app: &mut App) -> Surface {
    layout::prepare(app, Rect::new(0, 0, 100, 35))
        .surfaces
        .into_iter()
        .find(|s| {
            s.id.key
                == app
                    .modal_top()
                    .map_or(SurfaceKey::Document(app.desktop.active().id), |m| {
                        SurfaceKey::Modal(m.id)
                    })
        })
        .unwrap()
}
#[test]
fn help_expand_button_changes_native_content_bounds_and_restores() -> io::Result<()> {
    for modal in [false, true] {
        let f = support::Fixture::new("help-zoom")?;
        let mut a = f.app()?;
        if modal {
            a.open_settings(dn::settings::SettingsKind::Interface);
        }
        a.push_dialog(Dialog {
            title: " Help ",
            message: "context line\n".repeat(80),
        });
        support::draw(&mut a, 100, 35)?;
        let before = surface(&mut a);
        assert!(before.caps.zoom);
        support::click(&mut a, Target::Surface(before.id, Chrome::Zoom));
        let pixels = support::draw(&mut a, 100, 35)?;
        let expanded = surface(&mut a);
        assert!(expanded.maximized);
        assert!(expanded.bounds.width > before.bounds.width);
        assert_eq!(
            pixels[(expanded.bounds.x, expanded.bounds.bottom() - 1)].symbol(),
            "└"
        );
        assert_eq!(
            pixels[(expanded.bounds.right() - 1, expanded.bounds.bottom() - 1)].symbol(),
            "┘"
        );
        let row = (expanded.bounds.x..expanded.bounds.right())
            .map(|x| pixels[(x, expanded.bounds.y)].symbol())
            .collect::<String>();
        assert_eq!(
            row.matches('[').count(),
            2,
            "only close and zoom controls: {row}"
        );
        support::click(&mut a, Target::Surface(expanded.id, Chrome::Zoom));
        support::draw(&mut a, 100, 35)?;
        assert_eq!(surface(&mut a).bounds, before.bounds);
        support::key(&mut a, crossterm::event::KeyCode::Esc);
        if modal {
            assert!(a.modals.settings().is_some());
            assert_eq!(dn::focus::current(&a), dn::focus::Focus::Settings);
        }
    }
    Ok(())
}

#[test]
fn help_zoom_keeps_topic_scroll_and_keyboard_zoom_in_sync() -> io::Result<()> {
    use crossterm::event::{KeyCode as K, KeyModifiers as M};
    let f = support::Fixture::new("help-zoom-state")?;
    let mut a = f.app()?;
    a.push_dialog(Dialog {
        title: " Help ",
        message: "context 界 e\u{301}\n".repeat(100),
    });
    support::draw(&mut a, 100, 35)?;
    support::key(&mut a, K::PageDown);
    let offset = a.native_help().unwrap().offset();
    assert!(offset > 0);
    let before = surface(&mut a);
    support::modified(&mut a, K::Char('z'), M::ALT);
    support::draw(&mut a, 100, 35)?;
    assert!(surface(&mut a).maximized);
    assert_eq!(a.native_help().unwrap().offset(), offset);
    support::click(&mut a, Target::Surface(before.id, Chrome::Zoom));
    support::draw(&mut a, 100, 35)?;
    assert_eq!(surface(&mut a).bounds, before.bounds);
    assert_eq!(a.native_help().unwrap().offset(), offset);
    support::key(&mut a, K::F(1));
    assert_eq!(
        a.native_help().unwrap().topic().as_deref(),
        Some("contents")
    );
    support::modified(&mut a, K::Char('z'), M::ALT);
    support::draw(&mut a, 80, 25)?;
    assert_eq!(
        a.native_help().unwrap().topic().as_deref(),
        Some("contents")
    );
    support::modified(&mut a, K::F(1), M::ALT);
    assert_eq!(a.native_help().unwrap().topic().as_deref(), Some("context"));
    assert_eq!(a.native_help().unwrap().offset(), offset);
    support::draw(&mut a, 100, 35)?;
    support::modified(&mut a, K::Char('z'), M::ALT);
    support::draw(&mut a, 100, 35)?;
    assert_eq!(surface(&mut a).bounds, before.bounds);
    Ok(())
}
#[test]
fn welcome_uses_help_zoom_resize_and_preserves_nested_settings() -> io::Result<()> {
    use crossterm::event::{KeyCode as K, KeyModifiers as M};
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        let f = support::Fixture::new("welcome-zoom-resize")?;
        let mut a = f.app()?;
        a.config.language = language;
        dn::welcome::open(&mut a);
        a.welcome.modals.welcome_mut().unwrap().focus = 1;
        support::draw(&mut a, 100, 35)?;
        let original = surface(&mut a);
        assert!(original.caps.zoom && original.caps.resize);
        support::click(&mut a, Target::Surface(original.id, Chrome::Zoom));
        let pixels = support::draw(&mut a, 100, 35)?;
        let expanded = surface(&mut a);
        assert!(expanded.maximized);
        assert!(expanded.bounds.width > original.bounds.width);
        assert_eq!(a.welcome.modals.welcome().unwrap().focus, 1);
        let title = (expanded.bounds.x..expanded.bounds.right())
            .map(|x| pixels[(x, expanded.bounds.y)].symbol())
            .collect::<String>();
        assert_eq!(
            title.matches('[').count(),
            2,
            "One close and one working zoom control"
        );
        support::modified(&mut a, K::Char('z'), M::ALT);
        support::draw(&mut a, 100, 35)?;
        assert_eq!(surface(&mut a).bounds, original.bounds);
        support::modified(&mut a, K::F(5), M::CONTROL);
        support::modified(&mut a, K::Right, M::SHIFT);
        support::modified(&mut a, K::Down, M::SHIFT);
        support::key(&mut a, K::Enter);
        support::draw(&mut a, 100, 35)?;
        let resized = surface(&mut a);
        assert!(resized.bounds.width > original.bounds.width);
        assert!(resized.bounds.height > original.bounds.height);
        assert!(a
            .mouse
            .regions
            .iter()
            .any(|(_, t)| *t == Target::Surface(resized.id, Chrome::Resize)));
        let grip = a
            .mouse
            .regions
            .iter()
            .find(|(_, t)| *t == Target::Surface(resized.id, Chrome::Resize))
            .unwrap()
            .0;
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let (x, y) = (grip.right() - 1, grip.y);
        for (kind, column, row) in [
            (MouseEventKind::Down(MouseButton::Left), x, y),
            (MouseEventKind::Drag(MouseButton::Left), x + 2, y + 1),
            (MouseEventKind::Up(MouseButton::Left), x + 2, y + 1),
        ] {
            dn::mouse::handle(
                &mut a,
                MouseEvent {
                    kind,
                    column,
                    row,
                    modifiers: M::NONE,
                },
            );
        }
        support::draw(&mut a, 100, 35)?;
        let dragged = surface(&mut a);
        assert!(dragged.bounds.width > resized.bounds.width);
        assert!(dragged.bounds.height > resized.bounds.height);
        let resized = dragged;
        dn::welcome::action(&mut a, dn::welcome::Action::Keyboard);
        support::draw(&mut a, 100, 35)?;
        support::key(&mut a, K::Esc);
        support::draw(&mut a, 100, 35)?;
        assert_eq!(surface(&mut a).bounds, resized.bounds);
        assert_eq!(a.welcome.modals.welcome().unwrap().focus, 1);
        support::click(&mut a, Target::Surface(resized.id, Chrome::Zoom));
        support::draw(&mut a, 80, 25)?;
        assert!(a.welcome.modals.welcome().is_some());
        support::draw(&mut a, 100, 35)?;
        support::modified(&mut a, K::Char('z'), M::ALT);
        support::draw(&mut a, 100, 35)?;
        assert_eq!(surface(&mut a).bounds, resized.bounds);
        support::key(&mut a, K::Esc);
        assert!(a.welcome.modals.welcome().is_none());
    }
    Ok(())
}
