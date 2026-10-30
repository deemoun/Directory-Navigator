mod support;
use crossterm::event::{KeyCode as K, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind};
use dn::{
    app::App,
    editor, layout, mouse,
    palette::{theme::Slot, PaletteProfile},
    presentation::layout::Rect,
    testing::Buffer,
    windowing::geometry::{Surface, SurfaceKey},
};
use std::io;
fn pointer(a: &mut App, kind: MouseEventKind, x: u16, y: u16) {
    mouse::handle(
        a,
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: M::NONE,
        },
    );
}
fn captured(a: &mut App) -> (Surface, Buffer) {
    let mut snapshot = layout::prepare(a, Rect::new(0, 0, 100, 32));
    let id = snapshot.manipulation.expect("active manipulation");
    let surface = *snapshot.surfaces.iter().find(|s| s.id == id).unwrap();
    let frame = dn::ui::render(a, &snapshot);
    let one = Buffer::rendered(frame);
    assert_eq!(one, Buffer::rendered(dn::ui::render(a, &snapshot)));
    snapshot.manipulation = None;
    let normal = Buffer::rendered(dn::ui::render(a, &snapshot));
    for (x, y) in [
        (surface.bounds.x, surface.bounds.y),
        (surface.bounds.x + 10, surface.bounds.y),
        (surface.bounds.x, surface.bounds.y + 2),
        (surface.bounds.right() - 1, surface.bounds.bottom() - 1),
    ] {
        assert_eq!(
            one[(x, y)].bg,
            normal[(x, y)].bg,
            "dragging must preserve the background at {x},{y}"
        );
        assert_eq!(one[(x, y)].fg, a.config.palette.color(10));
    }
    (surface, one)
}
#[test]
fn mouse_drag_restores_original_and_custom_frame_colors_on_release_and_escape() -> io::Result<()> {
    let f = support::Fixture::new("drag-editor")?;
    for palette in [
        PaletteProfile::ClassicDn,
        PaletteProfile::ClassicDn.with_slot(Slot::DialogFrame, 0x4e),
    ] {
        let mut a = f.app()?;
        a.config.palette = palette;
        editor::open(&mut a, true);
        support::draw(&mut a, 100, 32)?;
        pointer(&mut a, MouseEventKind::Down(MouseButton::Left), 40, 1);
        let (s, b) = captured(&mut a);
        assert_ne!(
            b[(s.bounds.x + 4, s.bounds.y + 5)].bg,
            palette.color(2),
            "editor contents stay normal"
        );
        pointer(&mut a, MouseEventKind::Drag(MouseButton::Left), 44, 4);
        let (moved, _) = captured(&mut a);
        pointer(&mut a, MouseEventKind::Up(MouseButton::Left), 44, 4);
        let normal = support::draw(&mut a, 100, 32)?;
        assert_eq!(
            normal[(moved.bounds.x, moved.bounds.y)].bg,
            palette.dos(palette.slot(Slot::DialogFrame)).bg.unwrap()
        );
        assert_eq!(a.config.palette, palette);
        pointer(
            &mut a,
            MouseEventKind::Down(MouseButton::Left),
            moved.bounds.x + 12,
            moved.bounds.y,
        );
        captured(&mut a);
        support::key(&mut a, K::Esc);
        let normal = support::draw(&mut a, 100, 32)?;
        assert_eq!(
            normal[(moved.bounds.x, moved.bounds.y)].bg,
            palette.dos(palette.slot(Slot::DialogFrame)).bg.unwrap()
        );
        assert!(layout::prepare(&mut a, Rect::new(0, 0, 100, 32))
            .manipulation
            .is_none());
    }
    Ok(())
}
#[test]
fn game_and_native_and_composed_dialogs_share_the_transient_frame_highlight() -> io::Result<()> {
    let f = support::Fixture::new("drag-dialogs")?;
    let mut a = f.app()?;
    dn::tetris::open(&mut a);
    support::draw(&mut a, 100, 32)?;
    support::modified(&mut a, K::F(5), M::CONTROL);
    let (game, b) = captured(&mut a);
    assert_eq!(
        b[(game.bounds.x + 2, game.bounds.y + 3)].bg,
        a.config.palette.color(0),
        "board remains black"
    );
    support::key(&mut a, K::Enter);
    let b = support::draw(&mut a, 100, 32)?;
    assert_ne!(
        b[(game.bounds.x, game.bounds.y)].bg,
        a.config.palette.color(2)
    );
    support::key(&mut a, K::F(5)); // document-owned Setup page
    support::draw(&mut a, 100, 32)?;
    support::modified(&mut a, K::F(5), M::CONTROL);
    let (page, _) = captured(&mut a);
    assert!(matches!(page.id.key, SurfaceKey::Page(..)));
    support::key(&mut a, K::Esc); // cancel movement, retain Setup
    let b = support::draw(&mut a, 100, 32)?;
    assert_ne!(
        b[(page.bounds.x, page.bounds.y)].bg,
        a.config.palette.color(2)
    );
    support::key(&mut a, K::Esc);
    support::key(&mut a, K::Esc);
    editor::open(&mut a, true);
    support::draw(&mut a, 100, 32)?;
    for action in [editor::EditorAction::Help, editor::EditorAction::Open] {
        editor::action(&mut a, action);
        support::draw(&mut a, 100, 32)?;
        let owner = a
            .modal_top()
            .map(|m| SurfaceKey::Modal(m.id))
            .unwrap_or(SurfaceKey::Document(a.desktop.active().id));
        support::modified(&mut a, K::F(5), M::CONTROL);
        let (s, _) = captured(&mut a);
        assert_eq!(s.id.key, owner);
        support::key(&mut a, K::Enter);
        let b = support::draw(&mut a, 100, 32)?;
        assert_ne!(b[(s.bounds.x, s.bounds.y)].bg, a.config.palette.color(2));
        support::key(&mut a, K::Esc);
    }
    Ok(())
}
#[test]
fn resize_and_owner_retirement_clear_the_highlight_without_persisting_palette_changes(
) -> io::Result<()> {
    let f = support::Fixture::new("drag-retirement")?;
    let mut a = f.app()?;
    editor::open(&mut a, true);
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    support::modified(&mut a, K::F(5), M::CONTROL);
    captured(&mut a);
    support::draw(&mut a, 80, 25)?;
    assert!(layout::prepare(&mut a, Rect::new(0, 0, 80, 25))
        .manipulation
        .is_none());
    support::draw(&mut a, 100, 32)?;
    support::modified(&mut a, K::F(5), M::CONTROL);
    captured(&mut a);
    a.desktop.close(id);
    editor::open(&mut a, true);
    let snapshot = layout::prepare(&mut a, Rect::new(0, 0, 100, 32));
    assert!(snapshot.manipulation.is_none());
    assert_eq!(a.config.palette, PaletteProfile::ClassicDn);
    Ok(())
}
