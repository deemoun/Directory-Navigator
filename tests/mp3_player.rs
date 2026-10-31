//! Player UI and panel import use private fixtures; audio state has a fake backend in unit tests.
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
#[cfg(target_os = "macos")]
use std::fs;
use std::io;
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
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 18);
    let expanded = surface(&mut a, id, 100, 32).bounds;
    let small = surface(&mut a, id, 80, 25).bounds;
    assert_eq!((small.width, small.height), (36, 18));
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
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 18);
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
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 18);
    windowing::open(&mut a);
    support::draw(&mut a, 100, 32)?;
    support::click(&mut a, Target::Window(windowing::Action::Zoom));
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 9);
    menu::execute(&mut a, menu::Action::WindowZoom);
    assert_eq!(surface(&mut a, id, 100, 32).bounds.height, 18);
    assert!(!surface(&mut a, id, 100, 32).maximized);
    Ok(())
}
#[test]
fn unavailable_actions_are_not_mouse_targets_and_help_keeps_owner() -> io::Result<()> {
    let f = support::Fixture::new("mp3-controls")?;
    let mut a = f.app()?;
    a.editor_clipboard = "keep".into();
    let id = open(&mut a);
    support::modified(&mut a, K::Char('z'), M::ALT);
    support::draw(&mut a, 100, 32)?;
    for n in [0, 1, 2, 3, 4, 5, 6, 8, 9, 13] {
        assert!(!a
            .mouse
            .regions
            .iter()
            .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(n))));
    }
    input::paste(&mut a, "界 fake.mp3");
    assert_eq!(a.editor_clipboard, "keep");
    support::key(&mut a, K::F(1));
    support::draw(&mut a, 100, 32)?;
    assert_eq!(
        a.desktop.active().kind,
        WindowKind::Tool(dn::tool_windows::Kind::Help)
    );
    support::key(&mut a, K::Esc);
    assert_eq!(a.desktop.active().id, id);
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
                    assert_eq!(
                        count,
                        if expanded {
                            if cfg!(target_os = "macos") {
                                4
                            } else {
                                1
                            }
                        } else {
                            0
                        }
                    );
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
    if matches!(id, 7 | 201) {
        support::finish_mp3_import(a)?;
    }
    Ok(())
}
fn page(a: &mut App, owner: WindowId, n: u64) -> bool {
    layout::prepare(a, Rect::new(0, 0, 100, 32))
        .surfaces
        .iter()
        .any(|s| s.id.key == SurfaceKey::Page(owner, n))
}
#[cfg(target_os = "macos")]
#[test]
fn panel_import_draft_cancel_commit_and_reopen_use_real_paths_without_reading() -> io::Result<()> {
    let f = support::Fixture::new("mp3-import")?;
    for name in ["one.mp3", "界.MP3", "ignore.txt"] {
        fs::write(f.0.join(name), b"not decoded until Play")?;
    }
    fs::create_dir(f.0.join("folder.mp3"))?;
    let mut a = f.app()?;
    a.config.set_plugin_enabled(MP3_PLAYER, true);
    a.panels[a.active].selected = ["one.mp3", "界.MP3", "ignore.txt", "folder.mp3"]
        .into_iter()
        .map(Into::into)
        .collect();
    menu::execute(&mut a, menu::Action::AddMp3);
    support::finish_mp3_import(&mut a)?;
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    support::modified(&mut a, K::Char('z'), M::ALT);
    action(&mut a, 12)?;
    let rows = support::text(&mut a, 100, 32)?;
    assert!(
        rows.contains("one.mp3") && rows.contains("界") && rows.contains(".MP3"),
        "{rows}"
    );
    assert_eq!(a.mouse.regions.iter().filter(|(_,t)|matches!(t,Target::Plugin(p,ActionId(n)) if *p==MP3_PLAYER && (1000..1064).contains(n))).count(),2);
    assert!(!rows.contains("--:-- │ ignore.txt") && !rows.contains("--:-- │ folder.mp3"));
    action(&mut a, 202)?;
    action(&mut a, 203)?;
    action(&mut a, 12)?;
    assert!(support::text(&mut a, 100, 32)?.contains("one.mp3"));
    action(&mut a, 201)?;
    action(&mut a, 200)?;
    action(&mut a, 12)?;
    support::draw(&mut a, 100, 32)?;
    assert!(a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(1003))));
    action(&mut a, 203)?;
    support::key(&mut a, K::Esc);
    assert!(!a.desktop.contains(id));
    show_playlist(&mut a)?;
    assert!(support::text(&mut a, 100, 32)?.contains("Empty playlist"));
    assert_eq!(fs::read(f.0.join("one.mp3"))?, b"not decoded until Play");
    Ok(())
}
#[test]
fn empty_playlist_and_volume_pages_keep_focus_bounds_and_immutable_frames() -> io::Result<()> {
    let f = support::Fixture::new("mp3-pages")?;
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
            assert!(page(&mut a, id, 1));
            for (w, h) in [(80, 25), (132, 40)] {
                let snapshot = layout::prepare(&mut a, Rect::new(0, 0, w, h));
                let first = dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot));
                for _ in 0..3 {
                    assert_eq!(
                        first,
                        dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot))
                    );
                }
                support::draw(&mut a, w, h)?;
                for n in [2, 202, 221] {
                    assert!(!a
                        .mouse
                        .regions
                        .iter()
                        .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(n))));
                }
            }
            action(&mut a, 203)?;
            if cfg!(target_os = "macos") {
                action(&mut a, 11)?;
                assert!(page(&mut a, id, 3));
                action(&mut a, 301)?;
                action(&mut a, 303)?;
                assert!(!page(&mut a, id, 3));
                assert!(support::text(&mut a, 100, 32)?.contains("50%"));
            }
            support::key(&mut a, K::Esc);
            assert!(!a.desktop.contains(id));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn folder_tags_modes_info_and_cancel_use_owned_shared_controls() -> io::Result<()> {
    let f = support::Fixture::new("mp3-features")?;
    fs::create_dir(f.0.join("album"))?;
    let mut body = Vec::new();
    for (id, value) in [
        (b"TIT2", "Track title"),
        (b"TPE1", "Artist name"),
        (b"TALB", "Album name"),
    ] {
        let text = [vec![3], value.as_bytes().to_vec()].concat();
        body.extend(id);
        body.extend((text.len() as u32).to_be_bytes());
        body.extend([0, 0]);
        body.extend(text);
    }
    let size = body.len();
    let mut data = vec![
        b'I',
        b'D',
        b'3',
        3,
        0,
        0,
        ((size >> 21) & 127) as u8,
        ((size >> 14) & 127) as u8,
        ((size >> 7) & 127) as u8,
        (size & 127) as u8,
    ];
    data.extend(body);
    fs::write(f.0.join("album/song.MP3"), data)?;
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
            a.config.set_plugin_enabled(MP3_PLAYER, true);
            a.panels[a.active].selected.insert("album".into());
            menu::execute(&mut a, menu::Action::AddMp3);
            let id = a.desktop.active().id;
            assert!(page(&mut a, id, 5));
            for (w, h) in [(80, 25), (132, 40)] {
                let snapshot = layout::prepare(&mut a, Rect::new(0, 0, w, h));
                let first = dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot));
                for _ in 0..3 {
                    assert_eq!(
                        first,
                        dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot))
                    );
                }
            }
            action(&mut a, 500)?;
            a.poll();
            assert!(!page(&mut a, id, 5));
            support::draw(&mut a, 100, 32)?;
            support::modified(&mut a, K::Char('z'), M::ALT);
            assert!(!a
                .mouse
                .regions
                .iter()
                .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(2))));
            action(&mut a, 7)?;
            assert!(support::text(&mut a, 100, 32)?.contains("Artist name"));
            action(&mut a, 13)?;
            assert!(a.modal_top().is_some());
            let info = support::text(&mut a, 100, 32)?;
            assert!(info.contains("Album name") && info.contains("Track title"));
            support::key(&mut a, K::Esc);
            assert_eq!(a.desktop.active().id, id);
            action(&mut a, 10)?;
            assert!(page(&mut a, id, 4));
            for (w, h) in [(80, 25), (132, 40)] {
                let snapshot = layout::prepare(&mut a, Rect::new(0, 0, w, h));
                let first = dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot));
                for _ in 0..3 {
                    assert_eq!(
                        first,
                        dn::testing::Buffer::rendered(dn::ui::render(&a, &snapshot))
                    );
                }
                support::draw(&mut a, w, h)?;
                assert!(!a
                    .mouse
                    .regions
                    .iter()
                    .any(|(_, t)| *t == Target::Plugin(MP3_PLAYER, ActionId(2))));
            }
            action(&mut a, 400)?;
            action(&mut a, 403)?;
            action(&mut a, 405)?;
            action(&mut a, 10)?;
            action(&mut a, 400)?;
            action(&mut a, 402)?;
            action(&mut a, 404)?;
            assert!(!page(&mut a, id, 4));
            let text = support::text(&mut a, 100, 32)?;
            assert!(text.contains(language.text("Shuffle")) && text.contains(language.text("One")));
            action(&mut a, 12)?;
            assert!(support::text(&mut a, 100, 32)?.contains("Artist name"));
            support::key(&mut a, K::Esc);
            support::key(&mut a, K::Esc);
            assert!(!a.desktop.contains(id));
        }
    }
    Ok(())
}
