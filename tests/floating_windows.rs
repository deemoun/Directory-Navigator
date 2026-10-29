//! All file and settings effects are confined to private temporary fixtures.
mod support;
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use dn::{
    app::App,
    desktop::WindowId,
    editor, input, layout, mouse,
    presentation::layout::Rect,
    windowing::geometry::{Surface, SurfaceKey},
};
use std::{fs, io};
fn key(a: &mut App, k: K, m: M) {
    input::handle(a, KeyEvent::new(k, m));
}
fn surface(a: &mut App, id: WindowId) -> Surface {
    layout::prepare(a, Rect::new(0, 0, 100, 32))
        .surfaces
        .into_iter()
        .find(|s| s.id.key == SurfaceKey::Document(id))
        .unwrap()
}
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
#[test]
fn keyboard_restore_move_resize_cancel_and_paste_are_geometry_only() -> io::Result<()> {
    let fixture = support::Fixture::new("float-keyboard")?;
    let mut a = fixture.app()?;
    editor::open(&mut a, true);
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    assert!(surface(&mut a, id).maximized);
    key(&mut a, K::F(5), M::CONTROL);
    let start = surface(&mut a, id);
    assert!(!start.maximized);
    key(&mut a, K::Right, M::NONE);
    let moved = surface(&mut a, id);
    assert_eq!(moved.bounds.x, start.bounds.x + 1);
    input::paste(&mut a, "must not enter the buffer");
    assert_eq!(
        a.desktop.editor(id).unwrap().buffer.as_ref().unwrap().text,
        ""
    );
    key(&mut a, K::Down, M::SHIFT);
    assert_eq!(surface(&mut a, id).bounds.height, start.bounds.height + 1);
    key(&mut a, K::Esc, M::NONE);
    assert!(surface(&mut a, id).maximized);
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Right, M::NONE);
    key(&mut a, K::Enter, M::NONE);
    let saved = surface(&mut a, id);
    key(&mut a, K::Char('z'), M::ALT);
    assert!(surface(&mut a, id).maximized);
    key(&mut a, K::Char('z'), M::ALT);
    assert_eq!(surface(&mut a, id).bounds, saved.bounds);
    Ok(())
}
#[test]
fn mouse_drag_capture_release_and_cancel_keep_the_pointer_owner() -> io::Result<()> {
    let fixture = support::Fixture::new("float-mouse")?;
    let mut a = fixture.app()?;
    editor::open(&mut a, true);
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    pointer(&mut a, MouseEventKind::Down(MouseButton::Left), 40, 1);
    pointer(&mut a, MouseEventKind::Drag(MouseButton::Left), 44, 5);
    pointer(&mut a, MouseEventKind::Up(MouseButton::Left), 44, 5);
    let moved = surface(&mut a, id);
    assert!(!moved.maximized);
    assert!(moved.bounds.y > 1);
    support::draw(&mut a, 100, 32)?;
    pointer(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        moved.bounds.x + 12,
        moved.bounds.y,
    );
    pointer(&mut a, MouseEventKind::Drag(MouseButton::Left), 0, 0);
    key(&mut a, K::Esc, M::NONE);
    assert_eq!(surface(&mut a, id).bounds, moved.bounds);
    assert!(!a.desktop.editor(id).unwrap().dirty());
    Ok(())
}
#[test]
fn overlapping_documents_activate_without_clicking_through_and_manager_stays_back() -> io::Result<()>
{
    let fixture = support::Fixture::new("float-overlap")?;
    fs::write(fixture.0.join("untouched"), "fixture")?;
    let mut a = fixture.app()?;
    editor::open(&mut a, true);
    let first = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Home, M::NONE);
    key(&mut a, K::Enter, M::NONE);
    editor::open(&mut a, true);
    let second = a.desktop.active().id;
    assert_ne!(first, second);
    support::draw(&mut a, 100, 32)?;
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::End, M::NONE);
    key(&mut a, K::Enter, M::NONE);
    support::draw(&mut a, 100, 32)?;
    pointer(&mut a, MouseEventKind::Down(MouseButton::Left), 1, 8);
    assert_eq!(a.desktop.active().id, first);
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
    support::draw(&mut a, 100, 32)?;
    pointer(&mut a, MouseEventKind::Down(MouseButton::Left), 99, 30);
    assert_eq!(a.desktop.active().id, WindowId::MANAGER);
    let b = support::draw(&mut a, 100, 32)?;
    assert!(b.content.iter().any(|c| c.symbol() == "╔"));
    let snapshot = layout::prepare(&mut a, Rect::new(0, 0, 100, 32));
    assert!(
        dn::ui::render(&a, &snapshot).cursor().is_some(),
        "exposed manager command cursor"
    );
    assert_eq!(fs::read(fixture.0.join("untouched"))?, b"fixture");
    Ok(())
}
#[test]
fn moving_a_modal_consumes_escape_without_cancelling_its_draft() -> io::Result<()> {
    use dn::settings::{Settings, SettingsKind};
    let fixture = support::Fixture::new("float-modal")?;
    let mut a = fixture.app()?;
    a.modals
        .set_settings(Some(Settings::new(SettingsKind::Interface, &a.config)));
    support::draw(&mut a, 100, 32)?;
    let modal = a.modal_top().unwrap().id;
    let initial = layout::prepare(&mut a, Rect::new(0, 0, 100, 32))
        .surfaces
        .into_iter()
        .find(|s| s.id.key == SurfaceKey::Modal(modal))
        .unwrap();
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Right, M::NONE);
    key(&mut a, K::Esc, M::NONE);
    assert!(a.modals.settings().is_some());
    let restored = layout::prepare(&mut a, Rect::new(0, 0, 100, 32))
        .surfaces
        .into_iter()
        .find(|s| s.id.key == SurfaceKey::Modal(modal))
        .unwrap();
    assert_eq!(restored.bounds, initial.bounds);
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Right, M::NONE);
    key(&mut a, K::Enter, M::NONE);
    let b = support::draw(&mut a, 100, 32)?;
    let cancel = a
        .mouse
        .regions
        .iter()
        .find(|(r, t)| r.width > 3 && *t == mouse::Target::CancelSettings)
        .unwrap()
        .0;
    pointer(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        cancel.x + 3,
        cancel.y,
    );
    assert!(a.modals.settings().is_none());
    drop(b);
    assert!(!fixture.0.join("config.conf").exists());
    Ok(())
}

fn modal_surface(a: &mut App, w: u16, h: u16) -> Surface {
    let id = a.modal_top().unwrap().id;
    layout::prepare(a, Rect::new(0, 0, w, h))
        .surfaces
        .into_iter()
        .find(|s| s.id.key == SurfaceKey::Modal(id))
        .unwrap()
}
#[test]
fn resize_retains_preferred_geometry_and_cancels_capture_below_terminal_minimum() -> io::Result<()>
{
    let f = support::Fixture::new("float-resize")?;
    let mut a = f.app()?;
    editor::open(&mut a, true);
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    key(&mut a, K::Char('z'), M::ALT);
    let preferred = surface(&mut a, id).bounds;
    let small = layout::prepare(&mut a, Rect::new(0, 0, 80, 25))
        .surfaces
        .into_iter()
        .find(|s| s.id.key == SurfaceKey::Document(id))
        .unwrap();
    assert!(small.bounds.right() <= 80 && small.bounds.bottom() <= 24);
    assert_eq!(surface(&mut a, id).bounds, preferred);
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Right, M::NONE);
    support::draw(&mut a, 60, 18)?;
    assert!(a.mouse.regions.is_empty());
    support::draw(&mut a, 100, 32)?;
    input::paste(&mut a, "active again");
    assert_eq!(
        a.desktop.editor(id).unwrap().buffer.as_ref().unwrap().text,
        "active again"
    );
    assert_eq!(surface(&mut a, id).bounds, preferred);
    Ok(())
}
#[test]
fn native_help_and_file_dialog_keep_placement_and_nested_parent_focus() -> io::Result<()> {
    let f = support::Fixture::new("float-native")?;
    let mut a = f.app()?;
    editor::open(&mut a, true);
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    key(&mut a, K::Char('z'), M::ALT);
    let parent = surface(&mut a, id).bounds;
    editor::action(&mut a, editor::EditorAction::Help);
    support::draw(&mut a, 100, 32)?;
    let help = modal_surface(&mut a, 100, 32);
    assert!(help.caps.resize);
    key(&mut a, K::Char('z'), M::ALT);
    let zoomed = modal_surface(&mut a, 100, 32);
    assert_eq!(zoomed.bounds, Rect::new(0, 1, 100, 30));
    key(&mut a, K::Char('z'), M::ALT);
    assert_eq!(modal_surface(&mut a, 100, 32).bounds, help.bounds);
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Right, M::NONE);
    key(&mut a, K::Down, M::SHIFT);
    key(&mut a, K::Enter, M::NONE);
    let moved = modal_surface(&mut a, 100, 32);
    assert_ne!(moved.bounds, help.bounds);
    assert_eq!(surface(&mut a, id).bounds, parent);
    let snapshot = layout::prepare(&mut a, Rect::new(0, 0, 100, 32));
    let first = dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot));
    let second = dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot));
    assert_eq!(first, second);
    key(&mut a, K::Esc, M::NONE);
    assert!(a.modal_top().is_none());
    assert_eq!(a.desktop.active().id, id);
    editor::action(&mut a, editor::EditorAction::Open);
    support::draw(&mut a, 100, 32)?;
    let file = modal_surface(&mut a, 100, 32);
    assert!(!file.caps.resize);
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Left, M::NONE);
    key(&mut a, K::Enter, M::NONE);
    support::draw(&mut a, 100, 32)?;
    let moved = modal_surface(&mut a, 100, 32);
    assert_eq!(moved.bounds.x + 1, file.bounds.x);
    if let Some(editor::Modal::File(picker)) = a.desktop.editor(id).unwrap().modals.editor_modal() {
        let _ = picker;
        assert!(a.mouse.regions.iter().any(|(r,t)| matches!(t, mouse::Target::Surface(s, dn::windowing::geometry::Chrome::Move) if s.key==moved.id.key)&&r.y==moved.bounds.y));
    } else {
        panic!("native file dialog");
    }
    key(&mut a, K::Esc, M::NONE);
    assert!(a.modal_top().is_none());
    assert_eq!(surface(&mut a, id).bounds, parent);
    Ok(())
}
#[test]
fn mouse_resize_and_both_reverse_tab_encodings_use_the_same_native_bounds() -> io::Result<()> {
    let f = support::Fixture::new("float-corner")?;
    let mut a = f.app()?;
    editor::open(&mut a, true);
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    key(&mut a, K::Char('z'), M::ALT);
    support::draw(&mut a, 100, 32)?;
    let r = surface(&mut a, id).bounds;
    pointer(
        &mut a,
        MouseEventKind::Down(MouseButton::Left),
        r.right() - 1,
        r.bottom() - 1,
    );
    pointer(
        &mut a,
        MouseEventKind::Drag(MouseButton::Left),
        r.right() + 2,
        r.bottom(),
    );
    pointer(
        &mut a,
        MouseEventKind::Up(MouseButton::Left),
        r.right() + 2,
        r.bottom(),
    );
    let resized = surface(&mut a, id).bounds;
    assert!(resized.width > r.width && resized.height > r.height);
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Tab, M::SHIFT);
    let left = surface(&mut a, id).bounds;
    assert_eq!(left.x + 1, resized.x);
    assert_eq!(left.width, resized.width);
    key(&mut a, K::BackTab, M::NONE);
    assert_eq!(surface(&mut a, id).bounds.x + 2, resized.x);
    key(&mut a, K::Esc, M::NONE);
    assert_eq!(surface(&mut a, id).bounds, resized);
    Ok(())
}

#[test]
fn plugin_pages_have_separate_geometry_block_the_desktop_and_pause_gravity() -> io::Result<()> {
    let f = support::Fixture::new("float-game-pages")?;
    let mut a = f.app()?;
    dn::tetris::open(&mut a);
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    let game = surface(&mut a, id).bounds;
    key(&mut a, K::F(5), M::NONE);
    let snapshot = layout::prepare(&mut a, Rect::new(0, 0, 100, 32));
    let page = *snapshot
        .surfaces
        .iter()
        .find(|s| matches!(s.id.key, SurfaceKey::Page(_, 1)))
        .unwrap();
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Right, M::NONE);
    assert!(!a.desktop.game().unwrap().running());
    key(&mut a, K::Enter, M::NONE);
    let snapshot = layout::prepare(&mut a, Rect::new(0, 0, 100, 32));
    let moved = *snapshot
        .surfaces
        .iter()
        .find(|s| s.id.key == page.id.key)
        .unwrap();
    assert_eq!(moved.bounds.x, page.bounds.x + 1);
    assert_eq!(surface(&mut a, id).bounds, game);
    support::draw(&mut a, 100, 32)?;
    pointer(&mut a, MouseEventKind::Down(MouseButton::Left), 0, 2);
    assert_eq!(a.desktop.active().id, id);
    key(&mut a, K::Esc, M::NONE);
    support::draw(&mut a, 100, 32)?;
    assert!(a.desktop.game().unwrap().running());
    key(&mut a, K::F(5), M::NONE);
    let replacement = layout::prepare(&mut a, Rect::new(0, 0, 100, 32))
        .surfaces
        .into_iter()
        .find(|s| s.id.key == page.id.key)
        .unwrap();
    assert_ne!(replacement.id, page.id);
    assert_eq!(replacement.bounds, page.bounds);
    Ok(())
}
#[test]
fn retired_targets_and_changed_modal_stack_cancel_capture_without_edits() -> io::Result<()> {
    let f = support::Fixture::new("float-retirement")?;
    let mut a = f.app()?;
    editor::open(&mut a, true);
    let old = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    let regions = a.mouse.regions.clone();
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Right, M::NONE);
    a.desktop.close(old);
    editor::open(&mut a, true);
    let fresh = a.desktop.active().id;
    assert_ne!(old, fresh);
    a.mouse.regions = regions;
    pointer(&mut a, MouseEventKind::Down(MouseButton::Left), 40, 1);
    assert_eq!(a.desktop.active().id, fresh);
    assert!(surface(&mut a, fresh).maximized);
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Left, M::NONE);
    a.error("A new blocking owner");
    support::draw(&mut a, 100, 32)?;
    input::paste(&mut a, "ignored");
    assert_eq!(
        a.desktop
            .editor(fresh)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        ""
    );
    assert!(surface(&mut a, fresh).maximized);
    key(&mut a, K::Esc, M::NONE);
    input::paste(&mut a, "live");
    assert_eq!(
        a.desktop
            .editor(fresh)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "live"
    );
    Ok(())
}

#[test]
fn translated_floating_frames_preserve_unicode_cursor_clipping_and_owner_metadata() -> io::Result<()>
{
    use dn::{localization::Language, palette::PaletteProfile};
    let f = support::Fixture::new("float-frames")?;
    for language in [Language::English, Language::Russian] {
        for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            let mut a = f.app()?;
            a.config.language = language;
            a.config.palette = palette;
            editor::open(&mut a, true);
            let id = a.desktop.active().id;
            support::draw(&mut a, 100, 32)?;
            editor::paste(&mut a, "界 e\u{301} 🦀\nsecond line");
            support::draw(&mut a, 100, 32)?;
            key(&mut a, K::Char('z'), M::ALT);
            for (w, h) in [(80, 25), (132, 40)] {
                let snapshot = layout::prepare_window(
                    &mut a,
                    Rect::new(3, 2, w, h),
                    Rect::new(0, 0, w + 3, h + 2),
                    std::time::Instant::now(),
                );
                let frame = dn::ui::render(&a, &snapshot);
                let surfaces = frame.surface_frames();
                let doc = surfaces
                    .iter()
                    .find(|s| s.surface.id.key == SurfaceKey::Document(id))
                    .unwrap();
                assert_eq!(doc.revision, snapshot.context.revision);
                assert!(doc.surface.bounds.width >= 72.min(w));
                let cursor = doc.cursor.expect("floating document cursor");
                assert!(doc.clip.contains(cursor.into()));
                assert!(frame
                    .hit_regions()
                    .iter()
                    .all(|(r, _)| r.intersection(snapshot.clip) == *r));
                assert!(doc
                    .hits
                    .iter()
                    .any(|(_, t)| matches!(t, mouse::Target::Editor(_))));
                let before = a
                    .desktop
                    .editor(id)
                    .unwrap()
                    .buffer
                    .as_ref()
                    .unwrap()
                    .text
                    .clone();
                let first = dn::testing::Buffer::rendered(frame);
                let second = dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot));
                assert_eq!(first, second);
                assert_eq!(
                    a.desktop.editor(id).unwrap().buffer.as_ref().unwrap().text,
                    before
                );
                let text = first.content.iter().map(|c| c.symbol()).collect::<String>();
                for glyph in ["界", "e\u{301}", "🦀"] {
                    assert!(
                        text.contains(glyph),
                        "{language:?}/{palette:?}/{w} missing {glyph:?}"
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn title_click_does_not_commit_restore_and_double_click_toggles_zoom() -> io::Result<()> {
    let f = support::Fixture::new("float-double")?;
    let mut a = f.app()?;
    editor::open(&mut a, true);
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    for _ in 0..2 {
        pointer(&mut a, MouseEventKind::Down(MouseButton::Left), 40, 1);
        pointer(&mut a, MouseEventKind::Up(MouseButton::Left), 40, 1);
        support::draw(&mut a, 100, 32)?;
    }
    assert!(!surface(&mut a, id).maximized);
    // Zoom controls take priority over the title's drag region.
    let zoom_id = surface(&mut a, id).id;
    support::click(
        &mut a,
        mouse::Target::Surface(zoom_id, dn::windowing::geometry::Chrome::Zoom),
    );
    assert!(surface(&mut a, id).maximized);
    Ok(())
}

#[test]
fn floating_editor_menu_keeps_its_top_border_and_document_local_actions() -> io::Result<()> {
    let f = support::Fixture::new("float-menu")?;
    let mut a = f.app()?;
    editor::open(&mut a, true);
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    key(&mut a, K::Char('z'), M::ALT);
    let outer = surface(&mut a, id).bounds;
    editor::action(&mut a, editor::EditorAction::Menu);
    let b = support::draw(&mut a, 100, 32)?;
    assert_eq!(b[(outer.x + 1, outer.y + 2)].symbol(), "┌");
    assert!(a.mouse.regions.iter().any(|(r, t)| matches!(
        t,
        mouse::Target::Editor(editor::EditorAction::MenuRow(_))
    ) && r.y > outer.y + 2));
    key(&mut a, K::Esc, M::NONE);
    input::paste(&mut a, "kept");
    assert_eq!(
        a.desktop.editor(id).unwrap().buffer.as_ref().unwrap().text,
        "kept"
    );
    Ok(())
}

#[test]
fn restoring_at_desktop_edges_cannot_turn_a_title_drag_into_a_zoom_click() -> io::Result<()> {
    let f = support::Fixture::new("float-edge-restore")?;
    for x in [6, 95, 96, 97, 98] {
        let mut a = f.app()?;
        support::draw(&mut a, 100, 32)?;
        a.show_viewer(dn::viewer::Viewer::from_contents(
            "edge.txt".into(),
            dn::viewer::from_bytes(b"text".to_vec(), dn::viewer::ViewMode::Text),
        ));
        let id = a.desktop.active().id;
        support::draw(&mut a, 100, 32)?;
        pointer(&mut a, MouseEventKind::Down(MouseButton::Left), x, 0);
        pointer(&mut a, MouseEventKind::Drag(MouseButton::Left), x, 4);
        pointer(&mut a, MouseEventKind::Up(MouseButton::Left), x, 4);
        let bounds = surface(&mut a, id);
        assert!(!bounds.maximized);
        assert_eq!(bounds.bounds.width, 80, "title restoration at {x}");
        assert_eq!(bounds.bounds.height, 24);
        assert!(bounds.bounds.y > 1, "title movement at {x}");
    }
    Ok(())
}

#[test]
fn tall_dialog_keeps_its_bottom_frame_and_clips_shadows_above_the_desktop_bar() -> io::Result<()> {
    let f = support::Fixture::new("float-shadow")?;
    let mut a = f.app()?;
    let before = support::draw(&mut a, 80, 25)?;
    a.error("A bounded line\n".repeat(100));
    let b = support::draw(&mut a, 80, 25)?;
    let modal = modal_surface(&mut a, 80, 25);
    assert_eq!(modal.bounds.bottom(), 24);
    assert_eq!(
        b[(modal.bounds.right() - 1, modal.bounds.bottom() - 1)].symbol(),
        "╝"
    );
    for x in 0..80 {
        assert_eq!(
            b[(x, 24)],
            before[(x, 24)],
            "shadow leaked into the desktop bar at {x}"
        );
    }
    key(&mut a, K::Esc, M::NONE);
    assert!(a.modal_top().is_none());
    Ok(())
}

#[test]
fn moving_a_short_message_keeps_the_native_bottom_border_clear_of_its_shadow() -> io::Result<()> {
    let f = support::Fixture::new("float-message")?;
    let mut a = f.app()?;
    support::draw(&mut a, 100, 32)?;
    a.error("Short message");
    support::draw(&mut a, 100, 32)?;
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Left, M::NONE);
    key(&mut a, K::Enter, M::NONE);
    let b = support::draw(&mut a, 100, 32)?;
    let s = modal_surface(&mut a, 100, 32);
    assert_eq!(
        b[(s.bounds.right() - 1, s.bounds.bottom() - 1)].symbol(),
        "╝"
    );
    Ok(())
}
