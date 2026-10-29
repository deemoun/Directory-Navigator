//! Plugin preferences use private temporary files and normal settings drafts.
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use dn::{
    app::App,
    config::Config,
    input, menu,
    mouse::Target,
    plugins::{CALCULATOR, TETRIS},
    settings::SettingsKind,
    testing::Buffer,
    ui,
};
use std::{fs, io, time::Instant};
mod support;
use support::{click, draw, key, Fixture as Temp};
fn fixture() -> io::Result<(Temp, App)> {
    let t = Temp::new("plugin-settings")?;
    let mut a = t.app()?;
    a.config = Config::default();
    a.config.save_on_exit = false;
    Ok((t, a))
}
#[test]
fn plugin_preferences_default_roundtrip_unknown_ids_and_reject_bad_types() -> io::Result<()> {
    let (t, mut a) = fixture()?;
    assert!(a.config.plugin_enabled(CALCULATOR) && a.config.plugin_enabled(TETRIS));
    let path = t.0.join("config.conf");
    fs::write(
        &path,
        "config_version=2\n[plugins]\ndisabled=['future.tool','tetris']\nunknown='keep me'\n",
    )?;
    let mut c = Config::read(&path)?;
    assert!(!c.plugin_enabled(TETRIS));
    assert!(c.plugin_enabled(CALCULATOR));
    c.set_plugin_enabled(CALCULATOR, false);
    c.save(&path)?;
    let mut again = Config::read(&path)?;
    assert_eq!(again.disabled_plugins, c.disabled_plugins);
    assert!(fs::read_to_string(&path)?.contains("keep me"));
    again.set_plugin_enabled(TETRIS, true);
    again.save(&path)?;
    let bytes = fs::read(&path)?;
    let again = Config::parse(&bytes)?;
    assert!(again.plugin_enabled(TETRIS));
    assert!(again.disabled_plugins.contains("future.tool"));
    a.load_config();
    input::handle(&mut a, KeyEvent::new(K::Char('k'), M::CONTROL));
    assert!(a.modals.plugin_dialog().is_none());
    for bad in [
        "disabled=true",
        "disabled='tetris'",
        "disabled=[1]",
        "disabled=['']",
    ] {
        assert!(Config::parse(format!("config_version=2\n[plugins]\n{bad}").as_bytes()).is_err());
    }
    let mut invalid = again;
    invalid.disabled_plugins.insert("bad\nID".into());
    assert!(invalid.save(&path).is_err());
    assert_eq!(fs::read(&path)?, bytes);
    Ok(())
}
#[test]
fn settings_draft_cancel_commit_disable_live_game_and_reenable_keep_scores() -> io::Result<()> {
    let (t, mut a) = fixture()?;
    dn::tetris::open(&mut a);
    let old = a.desktop.active().id;
    a.desktop
        .plugins
        .scores_mut()
        .add(dn::tetris::Record {
            name: "Pending".into(),
            score: 42,
            lines: Some(0),
            start_level: 1,
            level: 1,
        })
        .unwrap();
    a.desktop.activate(dn::desktop::WindowId::MANAGER);
    a.open_settings(SettingsKind::Plugins);
    assert_eq!(a.modals.settings().unwrap().control_count(), 2);
    key(&mut a, K::Tab);
    key(&mut a, K::Char(' '));
    assert!(a.config.plugin_enabled(TETRIS));
    assert!(a.desktop.game().is_some());
    key(&mut a, K::Esc);
    assert!(a.config.plugin_enabled(TETRIS));
    assert!(a.desktop.contains(old));
    a.open_settings(SettingsKind::Plugins);
    key(&mut a, K::Tab);
    key(&mut a, K::Char(' '));
    key(&mut a, K::Enter);
    assert!(!a.config.plugin_enabled(TETRIS));
    assert!(a.desktop.game().is_none());
    assert!(!a.desktop.contains(old));
    assert!(a.desktop.plugins.scores().has_pending());
    assert!(!t.0.join("tetris.conf").exists());
    input::handle(&mut a, KeyEvent::new(K::F(9), M::ALT));
    assert!(a.desktop.game().is_none());
    dn::tetris::open(&mut a);
    assert!(a.desktop.game().is_none());
    menu::open(&mut a, 0);
    assert!(!menu::level_items(&a, 0)
        .iter()
        .any(|i| i.action == menu::Action::Plugin(TETRIS)));
    menu::close(&mut a);
    a.open_settings(SettingsKind::Plugins);
    key(&mut a, K::Tab);
    key(&mut a, K::Char(' '));
    key(&mut a, K::Enter);
    assert!(a.desktop.game().is_none());
    menu::open(&mut a, 0);
    assert!(menu::level_items(&a, 0)
        .iter()
        .any(|i| i.action == menu::Action::Plugin(TETRIS) && i.selectable()));
    menu::close(&mut a);
    dn::tetris::open(&mut a);
    assert_ne!(a.desktop.active().id, old);
    assert_eq!(a.desktop.game().unwrap().records[0].name, "Pending");
    Ok(())
}
#[test]
fn plugins_dialog_localized_native_controls_mouse_keyboard_and_immutable_render() -> io::Result<()>
{
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        for palette in [
            dn::palette::PaletteProfile::ClassicDn,
            dn::palette::PaletteProfile::Terminal,
        ] {
            let (_t, mut a) = fixture()?;
            a.config.language = language;
            a.config.palette = palette;
            assert!(menu::submenu_items(menu::MenuId::Configuration)
                .iter()
                .any(|i| i.action == menu::Action::Settings(SettingsKind::Plugins)));
            a.open_settings(SettingsKind::Plugins);
            for (w, h) in [(80, 25), (132, 40)] {
                let b = draw(&mut a, w, h)?;
                let text = b.content.iter().map(|c| c.symbol()).collect::<String>();
                assert!(text.contains("calculator") && text.contains("tetris"));
            }
            click(&mut a, Target::Setting(0));
            assert!(a.config.plugin_enabled(CALCULATOR));
            assert!(!a
                .modals
                .settings()
                .unwrap()
                .draft
                .plugin_enabled(CALCULATOR));
            key(&mut a, K::Tab);
            assert_eq!(a.modals.settings().unwrap().cursor, 1);
            key(&mut a, K::Char(' '));
            assert!(!a.modals.settings().unwrap().draft.plugin_enabled(TETRIS));
            input::handle(&mut a, KeyEvent::new(K::Tab, M::SHIFT));
            assert_eq!(a.modals.settings().unwrap().cursor, 0);
            key(&mut a, K::BackTab);
            assert_eq!(a.modals.settings().unwrap().cursor, 3);
            let snap = dn::layout::prepare_window(
                &mut a,
                dn::testing::layout::Rect::new(7, 3, 80, 25),
                dn::testing::layout::Rect::new(0, 0, 100, 35),
                Instant::now(),
            );
            let f = ui::render(&a, &snap);
            let hits = f.hit_regions().to_vec();
            assert_eq!(Buffer::rendered(f), Buffer::rendered(ui::render(&a, &snap)));
            assert!(hits
                .iter()
                .all(|(r, _)| r.x >= 7 && r.y >= 3 && r.right() <= 87 && r.bottom() <= 28));
            key(&mut a, K::Enter);
            assert!(a.config.plugin_enabled(CALCULATOR) && a.config.plugin_enabled(TETRIS));
            assert!(a.modals.settings().is_none());
            a.open_settings(SettingsKind::Plugins);
            draw(&mut a, 80, 25)?;
            click(&mut a, Target::Setting(0));
            click(&mut a, Target::AcceptSettings);
            assert!(!a.config.plugin_enabled(CALCULATOR));
            input::handle(&mut a, KeyEvent::new(K::Char('k'), M::CONTROL));
            assert!(a.modals.plugin_dialog().is_none());
            menu::open(&mut a, 2);
            assert!(!menu::level_items(&a, 0)
                .iter()
                .any(|i| i.action == menu::Action::Plugin(CALCULATOR)));
            let hidden = draw(&mut a, 80, 25)?;
            assert!(!hidden
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .contains(language.text("Calculator")));
            menu::close(&mut a);
            a.open_settings(SettingsKind::Plugins);
            key(&mut a, K::Char(' '));
            key(&mut a, K::Enter);
            menu::open(&mut a, 2);
            assert_eq!(
                menu::level_items(&a, 0)
                    .iter()
                    .filter(|i| i.action == menu::Action::Plugin(CALCULATOR) && i.selectable())
                    .count(),
                1
            );
            menu::close(&mut a);
        }
    }
    Ok(())
}
#[test]
fn config_reload_disables_owned_calculator_help_without_losing_session_state() -> io::Result<()> {
    let (_t, mut a) = fixture()?;
    input::handle(&mut a, KeyEvent::new(K::Char('k'), M::CONTROL));
    input::paste(&mut a, "6*7");
    key(&mut a, K::Enter);
    key(&mut a, K::F(1));
    assert!(a.last_dialog().is_some());
    let mut config = a.config.clone();
    config.set_plugin_enabled(CALCULATOR, false);
    a.apply_config(config);
    assert!(a.modals.plugin_dialog().is_none());
    assert!(a.last_dialog().is_none());
    assert_eq!(
        a.desktop.plugins.calculator().result,
        Some(dn::utilities::calculator::Value::Integer(42))
    );
    let mut config = a.config.clone();
    config.set_plugin_enabled(CALCULATOR, true);
    a.apply_config(config);
    input::handle(&mut a, KeyEvent::new(K::Char('k'), M::CONTROL));
    assert_eq!(a.desktop.plugins.calculator().expression, "6*7");
    Ok(())
}
