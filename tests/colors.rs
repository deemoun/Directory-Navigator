//! Editable colors exercise real native controls; all persistence is temporary.
mod support;
use crossterm::event::{KeyCode as K, KeyModifiers as M};
use dn::{
    layout,
    localization::Language,
    mouse::Target,
    palette::{
        self,
        theme::{Group, Slot, GROUPS, ITEMS},
        Action, PaletteProfile,
    },
    presentation::layout::Rect,
    ui,
};
use std::{fs, io};
fn open(a: &mut dn::app::App) -> io::Result<()> {
    palette::open(a);
    support::draw(a, 80, 25)?;
    Ok(())
}
fn item(a: &mut dn::app::App, slot: Slot) {
    let definition = ITEMS.iter().find(|i| i.slot == slot).unwrap();
    let group = GROUPS.iter().position(|g| g.0 == definition.group).unwrap();
    palette::action(a, Action::Group(group));
    let row = palette::theme::items(group)
        .position(|i| i.slot == slot)
        .unwrap();
    palette::action(a, Action::Item(row));
}
#[test]
fn selectors_keyboard_mouse_live_preview_cancel_and_accept_agree() -> io::Result<()> {
    let f = support::Fixture::new("colors-selector")?;
    let mut a = f.app()?;
    open(&mut a)?;
    let grid = support::draw(&mut a, 80, 25)?;
    assert_eq!(
        grid[(79, 3)].bg,
        a.config.palette.color(0),
        "native dialog shadow"
    );
    for foreground in [true, false] {
        for n in 0..16u8 {
            let target = Target::Colors(if foreground {
                Action::Foreground(n)
            } else {
                Action::Background(n)
            });
            let r = a
                .mouse
                .regions
                .iter()
                .find(|(_, t)| *t == target)
                .unwrap()
                .0;
            assert_eq!(
                grid[(r.x, r.y)].bg,
                a.config.palette.color(n),
                "missing color {n}"
            );
        }
    }
    let original = a.config.palette;
    support::click(&mut a, Target::Colors(Action::Foreground(12)));
    assert_eq!(a.config.palette.slot(Slot::TimerColor), 0x7c);
    support::key(&mut a, K::Right);
    assert_eq!(a.config.palette.slot(Slot::TimerColor), 0x7d);
    support::key(&mut a, K::Char('b'));
    support::key(&mut a, K::Up);
    assert_eq!(a.config.palette.slot(Slot::TimerColor), 0x3d);
    support::key(&mut a, K::Esc);
    assert_eq!(a.config.palette, original);
    open(&mut a)?;
    support::key(&mut a, K::Char('f'));
    support::key(&mut a, K::Right);
    let chosen = a.config.palette;
    support::key(&mut a, K::Enter);
    assert!(a.modals.color_dialog().is_none());
    assert_eq!(a.config.palette, chosen);
    open(&mut a)?;
    assert_eq!(a.config.palette, chosen);
    palette::action(&mut a, Action::Color);
    support::key(&mut a, K::Esc);
    assert_eq!(a.config.palette, chosen);
    Ok(())
}
#[test]
fn colors_persist_validate_migrate_legacy_and_preserve_unknown_settings() -> io::Result<()> {
    let f = support::Fixture::new("colors-config")?;
    let path = f.0.join("config.conf");
    fs::write(&path,"config_version=2\n[interface]\npalette=\"terminal\"\n[colors]\nfuture_role=\"preserved\"\n")?;
    let mut config = dn::config::Config::read(&path)?;
    assert_eq!(config.palette, PaletteProfile::ClassicDn);
    config.palette = config
        .palette
        .with_slot(Slot::PanelText, 0x1e)
        .with_slot(Slot::DialogInput, 0x2f);
    config.save(&path)?;
    assert_eq!(dn::config::Config::read(&path)?.palette, config.palette);
    assert!(fs::read_to_string(&path)?.contains("future_role = \"preserved\""));
    for value in ["256", "-1", "\"green\"", "true", "[]"] {
        assert!(dn::config::Config::parse(
            format!("config_version=2\n[colors]\npanel_text={value}\n").as_bytes()
        )
        .is_err());
    }
    Ok(())
}
#[test]
fn same_default_attributes_are_independent_between_components() -> io::Result<()> {
    let f = support::Fixture::new("colors-scope")?;
    let mut a = f.app()?;
    let custom = PaletteProfile::ClassicDn
        .with_slot(Slot::ViewerText, 0x2e)
        .with_slot(Slot::PanelText, 0x4f);
    a.config.palette = custom;
    assert_eq!(custom.for_group(Group::Viewer, 0x1f), custom.dos(0x2e));
    assert_eq!(custom.for_group(Group::Editor, 0x1f), custom.dos(0x1f));
    let manager = support::draw(&mut a, 80, 25)?;
    assert!(manager.content.iter().any(|c| c.bg == custom.color(4)));
    a.show_viewer(dn::viewer::Viewer::from_contents(
        "fixture.txt".into(),
        dn::viewer::from_bytes(
            "界 e\u{301}".as_bytes().to_vec(),
            dn::viewer::ViewMode::Text,
        ),
    ));
    let viewer = support::draw(&mut a, 80, 25)?;
    assert!(viewer
        .content
        .iter()
        .any(|c| c.symbol() == "界" && c.bg == custom.color(2) && c.fg == custom.color(14)));
    dn::viewer::action(&mut a, dn::viewer::Action::Close);
    dn::editor::open(&mut a, true);
    support::draw(&mut a, 80, 25)?;
    dn::editor::paste(&mut a, "界");
    let editor = support::draw(&mut a, 80, 25)?;
    assert!(
        editor
            .content
            .iter()
            .any(|c| c.symbol() == "界" && c.bg == custom.color(1)),
        "active={:?}, text={:?}, cells={:?}",
        a.desktop.active(),
        a.desktop
            .active_editor()
            .and_then(|e| e.buffer.as_ref())
            .map(|b| &b.text),
        editor
            .content
            .iter()
            .filter(|c| c.symbol().contains('界'))
            .collect::<Vec<_>>()
    );
    Ok(())
}
#[test]
fn native_dialog_buttons_inputs_and_background_use_custom_roles() -> io::Result<()> {
    let f = support::Fixture::new("colors-native")?;
    let mut a = f.app()?;
    let p = PaletteProfile::ClassicDn
        .with_slot(Slot::DialogText, 0x2f)
        .with_slot(Slot::DialogDefault, 0x5e)
        .with_slot(Slot::DialogFocusedInput, 0x4f);
    a.config.palette = p;
    a.prompt_operation(dn::operations::Kind::Mkdir, false);
    let b = support::draw(&mut a, 80, 25)?;
    assert!(b.content.iter().any(|c| c.bg == p.color(2)));
    assert!(b.content.iter().any(|c| c.bg == p.color(4)));
    assert!(b.content.iter().any(|c| c.bg == p.color(5)));
    support::key(&mut a, K::Esc);
    assert!(!f.0.join("new").exists());
    Ok(())
}
#[test]
fn navigation_scroll_resize_and_painting_are_bounded_and_immutable() -> io::Result<()> {
    let f = support::Fixture::new("colors-layout")?;
    for language in [Language::English, Language::Russian] {
        let mut a = f.app()?;
        a.config.language = language;
        open(&mut a)?;
        let text = support::text(&mut a, 80, 25)?;
        assert!(!text.contains("Colors:"));
        let initial = a.modals.color_dialog().unwrap().cursor;
        support::key(&mut a, K::Tab);
        support::modified(&mut a, K::Tab, M::SHIFT);
        assert_eq!(a.modals.color_dialog().unwrap().cursor, initial);
        support::key(&mut a, K::Tab);
        support::key(&mut a, K::BackTab);
        assert_eq!(a.modals.color_dialog().unwrap().cursor, initial);
        item(&mut a, Slot::DialogText);
        support::key(&mut a, K::End);
        assert!(a.modals.color_dialog().unwrap().item_offset > 0);
        for (w, h) in [(80, 25), (132, 40)] {
            let s = layout::prepare(&mut a, Rect::new(0, 0, w, h));
            let first = ui::render(&a, &s);
            let regions = first.hit_regions();
            assert!(regions
                .iter()
                .all(|(r, _)| r.intersection(Rect::new(0, 0, w, h)) == *r));
            assert_eq!(
                dn::testing::Buffer::rendered(first),
                dn::testing::Buffer::rendered(ui::render(&a, &s))
            );
        }
        assert!(a.modals.color_dialog().is_some());
    }
    Ok(())
}
#[test]
fn presets_defaults_and_catalog_are_real_and_reversible() -> io::Result<()> {
    let f = support::Fixture::new("colors-presets")?;
    let mut a = f.app()?;
    open(&mut a)?;
    let keys = ITEMS
        .iter()
        .map(|i| i.key)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(keys.len(), ITEMS.len());
    let slots = ITEMS
        .iter()
        .map(|i| (i.group as u8, i.attribute))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(slots.len(), ITEMS.len());
    palette::action(&mut a, Action::BlackWhite);
    assert_ne!(a.config.palette, PaletteProfile::ClassicDn);
    for i in ITEMS {
        let v = a.config.palette.slot(i.slot);
        assert!([0, 7, 8, 15].contains(&(v & 15)) && [0, 7, 8, 15].contains(&(v >> 4)));
    }
    palette::action(&mut a, Action::Mono);
    assert_ne!(a.config.palette, PaletteProfile::ClassicDn);
    for (normal, selected) in [
        (Slot::PanelText, Slot::PanelCursor),
        (Slot::MenuNormal, Slot::MenuSelected),
        (Slot::EditorText, Slot::EditorSelection),
    ] {
        assert_ne!(
            a.config.palette.slot(normal),
            a.config.palette.slot(selected),
            "monochrome selection must remain visible"
        );
    }
    palette::action(&mut a, Action::Defaults);
    assert_eq!(a.config.palette, PaletteProfile::ClassicDn);
    assert!(a.modals.color_dialog().unwrap().defaults);
    support::key(&mut a, K::Esc);
    assert_eq!(a.config.palette, PaletteProfile::ClassicDn);
    Ok(())
}

#[test]
fn moving_colors_and_rejecting_stale_hits_preserves_the_selected_item() -> io::Result<()> {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let f = support::Fixture::new("colors-stale")?;
    let mut a = f.app()?;
    open(&mut a)?;
    let old = a
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::Colors(Action::Foreground(12)))
        .unwrap()
        .0;
    item(&mut a, Slot::PanelText);
    let before = a.config.palette;
    dn::mouse::handle(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: old.x,
            row: old.y,
            modifiers: M::NONE,
        },
    );
    assert_eq!(a.config.palette, before);
    support::draw(&mut a, 100, 32)?;
    support::modified(&mut a, K::F(5), M::CONTROL);
    support::key(&mut a, K::Right);
    support::key(&mut a, K::Enter);
    support::draw(&mut a, 100, 32)?;
    assert_eq!(
        a.modals.color_dialog().unwrap().selected().slot,
        Slot::PanelText
    );
    support::click(&mut a, Target::Colors(Action::Foreground(12)));
    assert_eq!(a.config.palette.slot(Slot::PanelText) & 15, 12);
    assert_eq!(a.config.palette.slot(Slot::TimerColor), 0x71);
    support::key(&mut a, K::Esc);
    assert_eq!(a.config.palette, before);
    Ok(())
}
