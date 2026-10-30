//! Arrangement tests use private temporary files and production UI/input paths.
mod support;
use crossterm::event::{KeyCode as K, KeyModifiers as M};
use dn::{
    app::App,
    desktop::WindowId,
    editor, input, layout, menu,
    mouse::Target,
    presentation::layout::Rect,
    windowing::{
        self,
        geometry::{Surface, SurfaceKey},
        Arrangement,
    },
};
use std::io;

fn surfaces(a: &mut App, w: u16, h: u16) -> Vec<Surface> {
    layout::prepare(a, Rect::new(0, 0, w, h))
        .surfaces
        .into_iter()
        .filter(|s| matches!(s.id.key, SurfaceKey::Document(_)))
        .collect()
}
fn doc(a: &mut App, id: WindowId, w: u16, h: u16) -> Surface {
    surfaces(a, w, h)
        .into_iter()
        .find(|s| s.id.key == SurfaceKey::Document(id))
        .unwrap()
}
fn viewer(a: &mut App, name: &str) -> WindowId {
    a.show_viewer(dn::viewer::Viewer::from_contents(
        name.into(),
        dn::viewer::from_bytes(
            "界 e\u{301} 🦀\nsecond line".as_bytes().to_vec(),
            dn::viewer::ViewMode::Text,
        ),
    ));
    a.desktop.active().id
}
#[test]
fn mixed_tiles_preserve_focus_buffers_unicode_and_restore_geometry() -> io::Result<()> {
    let f = support::Fixture::new("arrange-mixed")?;
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        for palette in [
            dn::palette::PaletteProfile::ClassicDn,
            dn::palette::PaletteProfile::Terminal,
        ] {
            let mut a = f.app()?;
            a.config.language = language;
            a.config.palette = palette;
            editor::open(&mut a, true);
            let edit = a.desktop.active().id;
            support::draw(&mut a, 160, 40)?;
            input::paste(&mut a, "界 e\u{301} 🦀\nindependent edit");
            let first = viewer(&mut a, "first.txt");
            let second = viewer(&mut a, "second.txt");
            a.desktop.activate(edit);
            support::draw(&mut a, 160, 40)?;
            menu::execute(&mut a, menu::Action::WindowTile);
            assert_eq!(a.desktop.active().id, edit);
            let docs = [edit, first, second].map(|id| doc(&mut a, id, 160, 40));
            assert_eq!(
                docs.iter()
                    .map(|s| u32::from(s.bounds.width) * u32::from(s.bounds.height))
                    .sum::<u32>(),
                160 * 38
            );
            for (i, s) in docs.iter().enumerate() {
                assert!(!s.maximized);
                assert!(s.bounds.width >= s.caps.minimum.0 && s.bounds.height >= s.caps.minimum.1);
                for other in &docs[i + 1..] {
                    assert!(s.bounds.intersection(other.bounds).is_empty());
                }
            }
            windowing::arrange(&mut a, Arrangement::Tile);
            assert_eq!(
                docs,
                [edit, first, second].map(|id| doc(&mut a, id, 160, 40))
            );
            let snapshot = layout::prepare(&mut a, Rect::new(0, 0, 160, 40));
            let frame = dn::ui::render(&a, &snapshot);
            assert!(frame.cursor().is_some());
            assert!(frame
                .hit_regions()
                .iter()
                .all(|(r, _)| r.intersection(snapshot.clip) == *r));
            let one = dn::testing::Buffer::rendered(frame);
            assert_eq!(
                one,
                dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot))
            );
            let text = one.content.iter().map(|c| c.symbol()).collect::<String>();
            for glyph in ["界", "e\u{301}", "🦀"] {
                assert!(text.contains(glyph));
            }
            support::modified(&mut a, K::Char('z'), M::ALT);
            assert!(doc(&mut a, edit, 160, 40).maximized);
            support::modified(&mut a, K::Char('z'), M::ALT);
            assert_eq!(doc(&mut a, edit, 160, 40).bounds, docs[0].bounds);
            // Shrink clamps effective geometry; expanding retains the arranged preference.
            surfaces(&mut a, 80, 25);
            assert_eq!(doc(&mut a, edit, 160, 40).bounds, docs[0].bounds);
            assert_eq!(
                a.desktop
                    .editor(edit)
                    .unwrap()
                    .buffer
                    .as_ref()
                    .unwrap()
                    .text,
                "界 e\u{301} 🦀\nindependent edit"
            );
            editor::action(&mut a, editor::EditorAction::Undo);
            assert_eq!(
                a.desktop
                    .editor(edit)
                    .unwrap()
                    .buffer
                    .as_ref()
                    .unwrap()
                    .text,
                ""
            );
        }
    }
    Ok(())
}
#[test]
fn cascade_keeps_stacking_order_and_places_fixed_size_game() -> io::Result<()> {
    let f = support::Fixture::new("arrange-cascade")?;
    let mut a = f.app()?;
    let first = viewer(&mut a, "first.txt");
    let second = viewer(&mut a, "second.txt");
    a.desktop.activate(WindowId::MANAGER);
    support::draw(&mut a, 100, 32)?;
    dn::tetris::open(&mut a);
    let game = a.desktop.active().id;
    assert!(matches!(
        a.desktop.active().kind,
        dn::desktop::WindowKind::Plugin(_)
    ));
    support::draw(&mut a, 100, 32)?;
    a.desktop.activate(first); // second is behind first in the native stack
    a.desktop.activate(WindowId::MANAGER);
    support::draw(&mut a, 100, 32)?;
    windowing::arrange(&mut a, Arrangement::Cascade);
    assert_eq!(a.desktop.active().id, WindowId::MANAGER);
    assert_eq!(
        doc(&mut a, second, 100, 32).bounds,
        Rect::new(0, 1, 100, 30)
    );
    assert_eq!(doc(&mut a, first, 100, 32).bounds, Rect::new(2, 3, 98, 28));
    assert_eq!(doc(&mut a, game, 100, 32).bounds, Rect::new(1, 2, 54, 23));
    windowing::arrange(&mut a, Arrangement::Cascade);
    assert_eq!(doc(&mut a, first, 100, 32).bounds, Rect::new(2, 3, 98, 28));
    Ok(())
}
#[test]
fn insufficient_space_is_atomic_and_keeps_old_restore_rectangles() -> io::Result<()> {
    let f = support::Fixture::new("arrange-space")?;
    let mut a = f.app()?;
    editor::open(&mut a, true);
    let first = a.desktop.active().id;
    support::draw(&mut a, 80, 25)?;
    support::modified(&mut a, K::Char('z'), M::ALT);
    let preferred = doc(&mut a, first, 80, 25);
    support::modified(&mut a, K::Char('z'), M::ALT);
    editor::open(&mut a, true);
    let second = a.desktop.active().id;
    support::draw(&mut a, 80, 25)?;
    let before = surfaces(&mut a, 80, 25);
    windowing::arrange(&mut a, Arrangement::Tile);
    assert_eq!(surfaces(&mut a, 80, 25), before);
    assert_eq!(a.desktop.active().id, second);
    assert!(a.modal_top().is_some());
    support::key(&mut a, K::Esc);
    a.desktop.activate(first);
    support::draw(&mut a, 80, 25)?;
    support::modified(&mut a, K::Char('z'), M::ALT);
    assert_eq!(doc(&mut a, first, 80, 25).bounds, preferred.bounds);
    Ok(())
}
#[test]
fn blocking_dialogs_and_manipulation_reject_arrangements() -> io::Result<()> {
    let f = support::Fixture::new("arrange-blocked")?;
    let mut a = f.app()?;
    let id = viewer(&mut a, "first.txt");
    support::draw(&mut a, 100, 32)?;
    a.error("Do not bypass this dialog");
    support::draw(&mut a, 100, 32)?;
    let top = a.modal_top().unwrap().id;
    let before = doc(&mut a, id, 100, 32);
    windowing::arrange(&mut a, Arrangement::Tile);
    assert_eq!(a.modal_top().unwrap().id, top);
    assert_eq!(doc(&mut a, id, 100, 32), before);
    support::key(&mut a, K::Esc);
    support::modified(&mut a, K::F(5), M::CONTROL);
    let moved = doc(&mut a, id, 100, 32);
    windowing::arrange(&mut a, Arrangement::Cascade);
    assert_eq!(doc(&mut a, id, 100, 32), moved);
    support::key(&mut a, K::Esc);
    assert_eq!(doc(&mut a, id, 100, 32), before);
    Ok(())
}
#[test]
fn window_list_buttons_and_keyboard_arrange_without_activating_selected_row() -> io::Result<()> {
    let f = support::Fixture::new("arrange-list")?;
    let mut a = f.app()?;
    let id = viewer(&mut a, "first.txt");
    support::draw(&mut a, 100, 32)?;
    windowing::open(&mut a);
    support::key(&mut a, K::Home); // manager row, while viewer remains active
    support::draw(&mut a, 100, 32)?;
    support::click(&mut a, Target::Window(windowing::Action::Tile));
    assert!(a.modals.window_list().is_none());
    assert_eq!(a.desktop.active().id, id);
    assert!(!doc(&mut a, id, 100, 32).maximized);
    windowing::open(&mut a);
    support::draw(&mut a, 100, 32)?;
    // list -> Switch -> Size/Move -> Zoom -> Tile -> Cascade
    for _ in 0..5 {
        support::key(&mut a, K::Tab);
    }
    assert_eq!(a.modals.window_list().unwrap().focus, 6);
    support::modified(&mut a, K::Tab, M::SHIFT);
    assert_eq!(a.modals.window_list().unwrap().focus, 5);
    support::key(&mut a, K::Tab);
    support::key(&mut a, K::Enter);
    assert!(a.modals.window_list().is_none());
    assert_eq!(a.desktop.active().id, id);
    Ok(())
}

#[test]
fn no_documents_disables_arrangement_and_skips_buttons_in_both_tab_encodings() -> io::Result<()> {
    let f = support::Fixture::new("arrange-empty")?;
    let mut a = f.app()?;
    support::draw(&mut a, 80, 25)?;
    let before = surfaces(&mut a, 80, 25);
    windowing::open(&mut a);
    support::draw(&mut a, 80, 25)?;
    assert!(!a.mouse.regions.iter().any(|(_, t)| matches!(
        t,
        Target::Window(windowing::Action::Tile | windowing::Action::Cascade)
    )));
    support::key(&mut a, K::Tab);
    assert_eq!(a.modals.window_list().unwrap().focus, 1);
    support::key(&mut a, K::Tab);
    assert_eq!(a.modals.window_list().unwrap().focus, 2);
    support::key(&mut a, K::BackTab);
    assert_eq!(a.modals.window_list().unwrap().focus, 1);
    support::modified(&mut a, K::Tab, M::SHIFT);
    assert_eq!(a.modals.window_list().unwrap().focus, 0);
    support::key(&mut a, K::Esc);
    windowing::arrange(&mut a, Arrangement::Tile);
    assert!(a.modal_top().is_none());
    assert_eq!(surfaces(&mut a, 80, 25), before);
    assert_eq!(a.desktop.active().id, WindowId::MANAGER);
    Ok(())
}

fn list_arrange(a: &mut App, mode: Arrangement, w: u16, h: u16) -> io::Result<()> {
    // Exercise the actual shortcut and its generated button, not the controller
    // directly; the game/editor must keep ownership after the list closes.
    support::modified(a, K::Char('0'), M::ALT);
    support::draw(a, w, h)?;
    assert!(a.modals.window_list().is_some());
    support::click(
        a,
        Target::Window(match mode {
            Arrangement::Tile => windowing::Action::Tile,
            Arrangement::Cascade => windowing::Action::Cascade,
        }),
    );
    assert!(a.modals.window_list().is_none());
    Ok(())
}

#[test]
fn single_game_enables_both_actions_and_paints_at_arranged_bounds() -> io::Result<()> {
    let f = support::Fixture::new("arrange-game-only")?;
    let mut a = f.app()?;
    dn::tetris::open(&mut a);
    let game = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    for mode in [Arrangement::Tile, Arrangement::Cascade] {
        list_arrange(&mut a, mode, 100, 32)?;
        assert_eq!(a.desktop.active().id, game);
        let rect = doc(&mut a, game, 100, 32).bounds;
        assert_eq!(rect, Rect::new(0, 1, 54, 23));
        for _ in 0..3 {
            a.poll();
            let b = support::draw(&mut a, 100, 32)?;
            assert_eq!(doc(&mut a, game, 100, 32).bounds, rect);
            assert_eq!(b[(rect.x, rect.y)].symbol(), "╔");
            assert_eq!(b[(rect.right() - 1, rect.bottom() - 1)].symbol(), "╝");
        }
        assert!(a.mouse.regions.iter().any(
            |(r, t)| matches!(t,Target::Surface(id,_) if id.key==SurfaceKey::Document(game))
                && r.y == rect.y
        ));
    }
    Ok(())
}

#[test]
fn editor_and_game_tile_and_cascade_through_window_list_from_either_document() -> io::Result<()> {
    let f = support::Fixture::new("arrange-editor-game")?;
    let mut a = f.app()?;
    editor::open(&mut a, true);
    let edit = a.desktop.active().id;
    support::draw(&mut a, 160, 40)?;
    input::paste(&mut a, "EDITOR CONTENT 界");
    a.desktop.activate(WindowId::MANAGER);
    support::draw(&mut a, 160, 40)?;
    dn::tetris::open(&mut a);
    let game = a.desktop.active().id;
    for active in [game, edit] {
        a.desktop.activate(active);
        support::draw(&mut a, 160, 40)?;
        list_arrange(&mut a, Arrangement::Tile, 160, 40)?;
        let er = doc(&mut a, edit, 160, 40).bounds;
        let gr = doc(&mut a, game, 160, 40).bounds;
        assert_eq!(a.desktop.active().id, active);
        assert!(er.intersection(gr).is_empty());
        assert_eq!((gr.width, gr.height), (54, 23));
        let b = support::draw(&mut a, 160, 40)?;
        let text = b.content.iter().map(|c| c.symbol()).collect::<String>();
        assert!(text.contains("EDITOR CONTENT 界") && text.contains("Navigator's game"));
        assert_eq!(b[(er.x, er.y)].symbol(), "╔");
        assert_eq!(b[(gr.x, gr.y)].symbol(), "╔");
        list_arrange(&mut a, Arrangement::Cascade, 160, 40)?;
        assert_eq!(a.desktop.active().id, active);
        let gr = doc(&mut a, game, 160, 40).bounds;
        assert_eq!((gr.width, gr.height), (54, 23));
        let snapshot = layout::prepare(&mut a, Rect::new(0, 0, 160, 40));
        let first = dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot));
        assert_eq!(
            first,
            dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot))
        );
    }
    // At the minimum terminal size, Cascade must keep the full-height game
    // inside the desktop. Tile cannot fit both, and must fail atomically.
    a.desktop.activate(game);
    support::draw(&mut a, 80, 25)?;
    list_arrange(&mut a, Arrangement::Cascade, 80, 25)?;
    let gr = doc(&mut a, game, 80, 25).bounds;
    assert_eq!((gr.width, gr.height), (54, 23));
    assert!(gr.bottom() <= 24);
    let before = surfaces(&mut a, 80, 25);
    list_arrange(&mut a, Arrangement::Tile, 80, 25)?;
    assert_eq!(surfaces(&mut a, 80, 25), before);
    assert!(a.modal_top().is_some());
    support::key(&mut a, K::Esc);
    assert_eq!(a.desktop.active().id, game);
    // The game needs its own strip: a balanced 1+2 column split alone
    // would incorrectly refuse this valid 132x32 mixed desktop.
    let view = viewer(&mut a, "alongside.txt");
    support::draw(&mut a, 132, 32)?;
    list_arrange(&mut a, Arrangement::Tile, 132, 32)?;
    let docs = [edit, game, view].map(|id| doc(&mut a, id, 132, 32));
    for (i, s) in docs.iter().enumerate() {
        for other in &docs[i + 1..] {
            assert!(s.bounds.intersection(other.bounds).is_empty());
        }
    }
    assert!(a.modal_top().is_none());
    assert_eq!(a.desktop.active().id, view);
    Ok(())
}

#[test]
fn editor_only_arrangement_survives_polling_and_routes_input_to_clicked_editor() -> io::Result<()> {
    let f = support::Fixture::new("arrange-editors")?;
    let mut a = f.app()?;
    editor::open(&mut a, true);
    let first = a.desktop.active().id;
    support::draw(&mut a, 160, 40)?;
    for mode in [Arrangement::Tile, Arrangement::Cascade] {
        list_arrange(&mut a, mode, 160, 40)?;
        assert!(!doc(&mut a, first, 160, 40).maximized);
        assert_eq!(doc(&mut a, first, 160, 40).bounds, Rect::new(0, 1, 160, 38));
        a.poll();
        support::draw(&mut a, 160, 40)?;
    }
    editor::open(&mut a, true);
    let second = a.desktop.active().id;
    support::draw(&mut a, 160, 40)?;
    list_arrange(&mut a, Arrangement::Tile, 160, 40)?;
    let left = doc(&mut a, first, 160, 40).bounds;
    let right = doc(&mut a, second, 160, 40).bounds;
    assert!(left.intersection(right).is_empty());
    support::draw(&mut a, 160, 40)?;
    dn::mouse::handle(
        &mut a,
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: left.x + 5,
            row: left.y + 4,
            modifiers: M::NONE,
        },
    );
    assert_eq!(a.desktop.active().id, first);
    input::paste(&mut a, "first editor only");
    assert_eq!(
        a.desktop
            .editor(first)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "first editor only"
    );
    assert_eq!(
        a.desktop
            .editor(second)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        ""
    );
    list_arrange(&mut a, Arrangement::Cascade, 160, 40)?;
    assert_eq!(doc(&mut a, first, 160, 40).bounds, Rect::new(1, 2, 159, 37));
    editor::action(&mut a, editor::EditorAction::Undo);
    assert_eq!(
        a.desktop
            .editor(first)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        ""
    );
    Ok(())
}
