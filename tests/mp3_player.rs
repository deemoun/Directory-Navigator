//! The player has no audio/file services; all fixtures stay in private temp dirs.
mod support;
use crossterm::event::{KeyCode as K, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind};
use dn::{
    app::App,
    desktop::{WindowId, WindowKind},
    input, layout, menu,
    mouse::{self, Target},
    palette::{theme::Slot, PaletteProfile},
    plugins::{ActionId, MP3_PLAYER},
    presentation::layout::Rect,
    windowing::{
        self,
        geometry::{Chrome, Surface, SurfaceKey},
        Arrangement,
    },
};
use std::{fs, io};
fn open(a: &mut App) -> WindowId {
    a.config.set_plugin_enabled(MP3_PLAYER, true);
    menu::execute(a, menu::Action::Plugin(MP3_PLAYER));
    let id = a.desktop.active().id;
    assert_eq!(a.desktop.active().kind, WindowKind::Plugin(MP3_PLAYER));
    id
}
fn surface(a: &mut App, id: WindowId, w: u16, h: u16) -> Surface {
    layout::prepare(a, Rect::new(0, 0, w, h))
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
fn text(b: &dn::testing::Buffer) -> String {
    b.content.iter().map(|c| c.symbol()).collect()
}
#[test]
fn registered_singleton_has_declared_centered_geometry_and_resets_on_reopen() -> io::Result<()> {
    let f = support::Fixture::new("mp3-open")?;
    let mut a = f.app()?;
    support::draw(&mut a, 100, 32)?;
    let id = open(&mut a);
    support::draw(&mut a, 100, 32)?;
    let s = surface(&mut a, id, 100, 32);
    assert_eq!(s.bounds, Rect::new(32, 11, 36, 9));
    assert!(!s.maximized && !s.caps.resize && s.caps.zoom);
    assert_eq!(open(&mut a), id);
    support::modified(&mut a, K::Char('z'), M::ALT);
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 16);
    let expanded = surface(&mut a, id, 100, 32).bounds;
    let small = surface(&mut a, id, 80, 25).bounds;
    assert_eq!((small.width, small.height), (36, 16));
    assert!(small.bottom() <= 24 && small.right() <= 80);
    assert_eq!(surface(&mut a, id, 100, 32).bounds, expanded);
    support::key(&mut a, K::Esc);
    assert!(!a.desktop.contains(id));
    let new = open(&mut a);
    assert_ne!(new, id);
    assert_eq!(
        surface(&mut a, new, 100, 32).bounds,
        Rect::new(32, 11, 36, 9)
    );
    Ok(())
}
#[test]
fn every_zoom_entry_point_toggles_presentation_without_maximizing() -> io::Result<()> {
    let f = support::Fixture::new("mp3-zoom")?;
    let mut a = f.app()?;
    let id = open(&mut a);
    support::draw(&mut a, 100, 32)?;
    support::modified(&mut a, K::Char('z'), M::ALT);
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 16);
    support::draw(&mut a, 100, 32)?;
    let sid = surface(&mut a, id, 100, 32).id;
    support::click(&mut a, Target::Surface(sid, Chrome::Zoom));
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 9);
    support::draw(&mut a, 100, 32)?;
    let r = surface(&mut a, id, 100, 32).bounds;
    for _ in 0..2 {
        pointer(
            &mut a,
            MouseEventKind::Down(MouseButton::Left),
            r.x + 12,
            r.y,
        );
        pointer(&mut a, MouseEventKind::Up(MouseButton::Left), r.x + 12, r.y);
        support::draw(&mut a, 100, 32)?;
    }
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 16);
    windowing::open(&mut a);
    support::draw(&mut a, 100, 32)?;
    support::click(&mut a, Target::Window(windowing::Action::Zoom));
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 9);
    menu::execute(&mut a, menu::Action::WindowZoom);
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 16);
    assert!(!surface(&mut a, id, 100, 32).maximized);
    Ok(())
}
#[test]
fn inert_controls_unicode_paste_and_help_preserve_files_clipboard_and_owner() -> io::Result<()> {
    let f = support::Fixture::new("mp3-inert")?;
    fs::write(f.0.join("untouched.mp3"), b"not audio; never read")?;
    let mut a = f.app()?;
    a.editor_clipboard = "keep".into();
    let id = open(&mut a);
    support::draw(&mut a, 100, 32)?;
    support::modified(&mut a, K::Char('z'), M::ALT);
    for n in (0..14).filter(|n| *n != 12) {
        support::draw(&mut a, 100, 32)?;
        support::click(&mut a, Target::Plugin(MP3_PLAYER, ActionId(n)));
        support::key(&mut a, K::Enter);
        support::key(&mut a, K::Char(' '));
        assert!(a.modal_top().is_none() && a.job.is_none());
        assert_eq!(a.desktop.active().id, id);
    }
    input::paste(&mut a, "界 fake.mp3");
    assert_eq!(a.editor_clipboard, "keep");
    assert_eq!(
        fs::read(f.0.join("untouched.mp3"))?,
        b"not audio; never read"
    );
    assert_eq!(fs::read_dir(&f.0)?.count(), 1);
    support::key(&mut a, K::F(1));
    support::draw(&mut a, 100, 32)?;
    assert!(a.modal_top().is_none());
    assert_eq!(
        a.desktop.active().kind,
        WindowKind::Tool(dn::tool_windows::Kind::Help)
    );
    support::key(&mut a, K::Esc);
    assert_eq!(a.desktop.active().id, id);
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 16);
    support::key(&mut a, K::Esc);
    assert!(!a.desktop.contains(id));
    Ok(())
}
#[test]
fn localized_frames_controls_and_palette_are_clipped_and_paint_is_immutable() -> io::Result<()> {
    let f = support::Fixture::new("mp3-frames")?;
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        for palette in [
            PaletteProfile::ClassicDn,
            PaletteProfile::ClassicDn.with_slot(Slot::DialogText, 0x1e),
        ] {
            let mut a = f.app()?;
            a.config.language = language;
            a.config.palette = palette;
            let id = open(&mut a);
            for (w, h) in [(80, 25), (132, 40)] {
                for expanded in [false, true] {
                    let snapshot = layout::prepare(&mut a, Rect::new(0, 0, w, h));
                    let frame = dn::ui::render(&a, &snapshot);
                    assert!(frame
                        .hit_regions()
                        .iter()
                        .all(|(r, _)| r.right() <= w && r.bottom() <= h));
                    let count=frame.hit_regions().iter().filter(|(_,t)|matches!(t,Target::Plugin(p,ActionId(n)) if *p==MP3_PLAYER && *n<14)).count();
                    assert_eq!(count, if expanded { 14 } else { 5 });
                    let b = dn::testing::Buffer::rendered(frame);
                    assert_eq!(
                        b,
                        dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot))
                    );
                    let s = surface(&mut a, id, w, h);
                    assert_eq!(b[(s.bounds.x, s.bounds.y)].symbol(), "╔");
                    assert_eq!(
                        b[(s.bounds.right() - 1, s.bounds.bottom() - 1)].symbol(),
                        "╝"
                    );
                    assert!(text(&b).contains(language.text("MP3 Player")));
                    if expanded {
                        assert!(text(&b).contains(language.text("Set up")));
                    }
                    support::draw(&mut a, w, h)?;
                    support::modified(&mut a, K::Char('z'), M::ALT);
                }
            }
        }
    }
    Ok(())
}
#[test]
fn fixed_player_shares_arrangement_movement_and_stale_owner_guards() -> io::Result<()> {
    let f = support::Fixture::new("mp3-desktop")?;
    let mut a = f.app()?;
    dn::editor::open(&mut a, true);
    let editor = a.desktop.active().id;
    support::draw(&mut a, 160, 40)?;
    a.desktop.activate(WindowId::MANAGER);
    support::draw(&mut a, 160, 40)?;
    dn::tetris::open(&mut a);
    let game = a.desktop.active().id;
    let id = open(&mut a);
    support::draw(&mut a, 160, 40)?;
    for mode in [Arrangement::Tile, Arrangement::Cascade] {
        windowing::arrange(&mut a, mode);
        assert!(a.modal_top().is_none());
        assert_eq!(surface(&mut a, id, 160, 40).bounds.width, 36);
        assert_eq!(surface(&mut a, game, 160, 40).bounds.width, 54);
        assert!(a.desktop.contains(editor));
    }
    support::draw(&mut a, 100, 32)?;
    support::modified(&mut a, K::F(5), M::CONTROL);
    let s = surface(&mut a, id, 100, 32);
    let b = support::draw(&mut a, 100, 32)?;
    assert_eq!(b[(s.bounds.x, s.bounds.y)].fg, a.config.palette.color(10));
    support::key(&mut a, K::Esc);
    support::draw(&mut a, 100, 32)?;
    let stale = a.mouse.regions.clone();
    support::key(&mut a, K::Esc);
    assert!(!a.desktop.contains(id));
    let fresh = open(&mut a);
    support::draw(&mut a, 100, 32)?;
    a.mouse.regions = stale;
    support::click(&mut a, Target::Surface(s.id, Chrome::Close));
    assert!(a.desktop.contains(fresh));
    Ok(())
}

#[test]
fn plugin_settings_disable_retires_player_and_help_and_reenable_restores_menu() -> io::Result<()> {
    let f = support::Fixture::new("mp3-enable")?;
    let mut a = f.app()?;
    let old = open(&mut a);
    support::draw(&mut a, 100, 32)?;
    support::key(&mut a, K::F(1));
    support::draw(&mut a, 100, 32)?;
    assert!(a.modal_top().is_none());
    assert_eq!(
        a.desktop.active().kind,
        WindowKind::Tool(dn::tool_windows::Kind::Help)
    );
    let mut config = a.config.clone();
    config.set_plugin_enabled(MP3_PLAYER, false);
    a.apply_config(config);
    support::draw(&mut a, 100, 32)?;
    assert!(!a.desktop.contains(old) && a.modal_top().is_none());
    menu::open(&mut a, menu::root_index(menu::RootId::Utilities).unwrap());
    assert!(!menu::level_items(&a, 0)
        .iter()
        .any(|i| i.action == menu::Action::Plugin(MP3_PLAYER)));
    menu::close(&mut a);
    a.open_settings(dn::settings::SettingsKind::Plugins);
    support::draw(&mut a, 100, 32)?;
    support::click(&mut a, Target::Setting(1)); // calculator, MP3 player, Tetris
    support::click(&mut a, Target::AcceptSettings);
    assert!(a.config.plugin_enabled(MP3_PLAYER));
    assert!(!a
        .desktop
        .windows()
        .any(|w| w.kind == WindowKind::Plugin(MP3_PLAYER)));
    menu::open(&mut a, menu::root_index(menu::RootId::Utilities).unwrap());
    assert!(menu::level_items(&a, 0)
        .iter()
        .any(|i| i.action == menu::Action::Plugin(MP3_PLAYER)));
    menu::close(&mut a);
    assert_ne!(open(&mut a), old);
    Ok(())
}

fn show_playlist(a: &mut App) -> io::Result<WindowId> {
    let id = open(a);
    support::draw(a, 100, 32)?;
    support::modified(a, K::Char('z'), M::ALT);
    support::draw(a, 100, 32)?;
    support::click(a, Target::Plugin(MP3_PLAYER, ActionId(12)));
    support::draw(a, 100, 32)?;
    Ok(id)
}
fn action(a: &mut App, id: usize) -> io::Result<()> {
    support::draw(a, 100, 32)?;
    support::click(a, Target::Plugin(MP3_PLAYER, ActionId(id)));
    Ok(())
}
fn page(a: &mut App, owner: WindowId, n: u64) -> bool {
    layout::prepare(a, Rect::new(0, 0, 100, 32))
        .surfaces
        .iter()
        .any(|s| s.id.key == SurfaceKey::Page(owner, n))
}
#[test]
fn demo_playlist_add_picker_commit_cancel_and_reopening_are_ui_only() -> io::Result<()> {
    let f = support::Fixture::new("mp3-playlist")?;
    fs::write(f.0.join("untouched.mp3"), b"do not read")?;
    let mut a = f.app()?;
    let id = show_playlist(&mut a)?;
    assert!(page(&mut a, id, 1));
    assert!(support::text(&mut a, 100, 32)?.contains("Example tracks"));
    assert!(!a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(221))));
    action(&mut a, 201)?;
    assert!(page(&mut a, id, 2));
    support::key(&mut a, K::Down);
    support::key(&mut a, K::Enter);
    assert!(!page(&mut a, id, 2));
    support::draw(&mut a, 100, 32)?;
    assert!(a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(1012))));
    action(&mut a, 200)?;
    assert!(!page(&mut a, id, 1));
    action(&mut a, 12)?;
    support::key(&mut a, K::End);
    support::draw(&mut a, 100, 32)?;
    assert!(a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(1012))));
    action(&mut a, 202)?;
    support::key(&mut a, K::Esc);
    action(&mut a, 12)?;
    support::key(&mut a, K::End);
    support::draw(&mut a, 100, 32)?;
    assert!(
        a.mouse
            .regions
            .iter()
            .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(1012))),
        "Cancel restores the accepted demo order"
    );
    support::key(&mut a, K::Esc);
    support::key(&mut a, K::Esc);
    assert!(!a.desktop.contains(id));
    let fresh = show_playlist(&mut a)?;
    assert_ne!(fresh, id);
    support::key(&mut a, K::End);
    support::draw(&mut a, 100, 32)?;
    assert!(
        !a.mouse
            .regions
            .iter()
            .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(1012))),
        "New openings reset demo state"
    );
    assert!(a.job.is_none());
    assert_eq!(fs::read(f.0.join("untouched.mp3"))?, b"do not read");
    assert_eq!(fs::read_dir(&f.0)?.count(), 1);
    Ok(())
}
#[test]
fn playlist_empty_state_focus_scrollbar_and_nested_help_remain_owned() -> io::Result<()> {
    let f = support::Fixture::new("mp3-playlist-focus")?;
    let mut a = f.app()?;
    let id = show_playlist(&mut a)?;
    for _ in 0..3 {
        support::key(&mut a, K::Tab);
    } // Delete
    support::key(&mut a, K::F(1));
    assert!(a.modal_top().is_some());
    assert_eq!(a.desktop.active().id, id);
    support::key(&mut a, K::Esc);
    assert!(page(&mut a, id, 1));
    support::key(&mut a, K::Tab); // Cancel, skipping disabled Save.
    support::key(&mut a, K::Enter);
    assert!(!page(&mut a, id, 1));
    action(&mut a, 12)?;
    action(&mut a, 223)?;
    support::draw(&mut a, 100, 32)?;
    for _ in 0..12 {
        action(&mut a, 202)?;
    }
    assert!(support::text(&mut a, 100, 32)?.contains("Empty demo playlist"));
    assert!(!a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(202))));
    support::key(&mut a, K::Enter);
    assert!(page(&mut a, id, 2), "focused Add remains usable when empty");
    support::key(&mut a, K::BackTab);
    support::key(&mut a, K::Enter);
    assert!(!page(&mut a, id, 2));
    assert!(page(&mut a, id, 1));
    support::key(&mut a, K::Esc);
    assert!(a.desktop.contains(id));
    Ok(())
}
#[test]
fn playlist_pages_localized_palettes_bounds_and_paint_are_consistent() -> io::Result<()> {
    let f = support::Fixture::new("mp3-playlist-frames")?;
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        for palette in [
            PaletteProfile::ClassicDn,
            PaletteProfile::ClassicDn.with_slot(Slot::DialogText, 0x1e),
        ] {
            let mut a = f.app()?;
            a.config.language = language;
            a.config.palette = palette;
            let id = show_playlist(&mut a)?;
            for picker in [false, true] {
                if picker {
                    action(&mut a, 201)?;
                }
                for (w, h) in [(80, 25), (132, 40)] {
                    let snapshot = layout::prepare(&mut a, Rect::new(0, 0, w, h));
                    let first = dn::ui::render(&a, &snapshot);
                    let second = dn::ui::render(&a, &snapshot);
                    assert!(first
                        .hit_regions()
                        .iter()
                        .all(|(r, _)| r.right() <= w && r.bottom() <= h));
                    assert_eq!(
                        dn::testing::Buffer::rendered(first),
                        dn::testing::Buffer::rendered(second)
                    );
                    support::draw(&mut a, w, h)?;
                    assert!(
                        !a.mouse
                            .regions
                            .iter()
                            .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(2))),
                        "Covered transport buttons cannot receive clicks"
                    );
                    let child = snapshot
                        .surfaces
                        .iter()
                        .find(|s| s.id.key == SurfaceKey::Page(id, if picker { 2 } else { 1 }))
                        .unwrap();
                    assert_eq!((child.bounds.width, child.bounds.height), (68, 20));
                }
            }
        }
    }
    Ok(())
}
