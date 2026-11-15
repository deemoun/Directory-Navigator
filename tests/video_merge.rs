//! Portable plugin checks do not require FFmpeg or audio/video hardware.
mod support;
use crossterm::event::KeyCode as K;
use dn::{
    config::Config,
    localization::Language,
    menu::{self, Action},
    mouse::Target,
    plugins::{ActionId, VIDEO_MERGE},
};
use std::{fs, io};
use support::{click, draw, key, Fixture};
#[test]
fn defaults_preferences_and_menu_gates() -> io::Result<()> {
    let t = Fixture::new("video-preferences")?;
    fs::write(
        t.0.join("prefs"),
        b"config_version=2\n[future]\nvalue='preserved'\n",
    )?;
    let mut c = Config::parse(b"config_version=2\n[future]\nvalue='preserved'\n")?;
    assert!(!c.plugin_enabled(VIDEO_MERGE));
    assert_eq!(c.video, Default::default());
    c.set_plugin_enabled(VIDEO_MERGE, true);
    c.video.ffmpeg = "/tools/Ёжик ffmpeg".into();
    c.video.ffprobe = "C:\\tools\\ffprobe.exe".into();
    c.save(&t.0.join("prefs"))?;
    let loaded = Config::read(&t.0.join("prefs"))?;
    assert_eq!(loaded.video, c.video);
    assert!(loaded.plugin_enabled(VIDEO_MERGE));
    assert!(fs::read_to_string(t.0.join("prefs"))?.contains("preserved"));
    let original = fs::read(t.0.join("prefs"))?;
    let mut invalid = c.clone();
    invalid.video.ffmpeg = "bad\tpath".into();
    assert!(invalid.save(&t.0.join("prefs")).is_err());
    assert_eq!(fs::read(t.0.join("prefs"))?, original);
    for value in ["ffmpeg=true", "ffprobe='bad\tpath'"] {
        assert!(Config::parse(format!("[plugins.video_merge]\n{value}").as_bytes()).is_err());
    }
    let mut a = t.app()?;
    menu::execute(&mut a, Action::Plugin(VIDEO_MERGE));
    assert_eq!(a.desktop.active().id, dn::desktop::WindowId::MANAGER);
    a.config.set_plugin_enabled(VIDEO_MERGE, true);
    menu::execute(&mut a, Action::Plugin(VIDEO_MERGE));
    assert_ne!(a.desktop.active().id, dn::desktop::WindowId::MANAGER);
    Ok(())
}
#[test]
fn source_controls_settings_cancel_and_immutable_frames() -> io::Result<()> {
    for language in [Language::English, Language::Russian] {
        let t = Fixture::new("video-ui")?;
        fs::write(t.0.join("Ёжик 2.MP4"), b"private media placeholder")?;
        let mut a = t.app()?;
        a.config.language = language;
        a.config.set_plugin_enabled(VIDEO_MERGE, true);
        a.panels[0].cursor = a.panels[0]
            .entries
            .iter()
            .position(|e| e.name == "Ёжик 2.MP4")
            .unwrap();
        menu::execute(&mut a, Action::MergeVideos);
        let owner = a.desktop.active().id;
        for size in [(80, 25), (110, 35), (60, 18), (80, 25)] {
            let first = draw(&mut a, size.0, size.1)?;
            for _ in 0..3 {
                assert_eq!(draw(&mut a, size.0, size.1)?, first);
            }
        }
        click(&mut a, Target::Plugin(VIDEO_MERGE, ActionId(21)));
        click(&mut a, Target::Plugin(VIDEO_MERGE, ActionId(9)));
        draw(&mut a, 80, 25)?;
        assert!(support::text(&mut a, 80, 25)?.contains(language.text("Match source resolution")));
        assert!(!support::text(&mut a, 80, 25)?.contains("FFmpeg path"));
        click(&mut a, Target::Plugin(VIDEO_MERGE, ActionId(40)));
        key(&mut a, K::Down);
        key(&mut a, K::Down);
        key(&mut a, K::Enter);
        assert!(support::text(&mut a, 80, 25)?.contains(language.text("Software H.265")));
        click(&mut a, Target::Plugin(VIDEO_MERGE, ActionId(49)));
        assert!(support::text(&mut a, 80, 25)?
            .contains(language.text("Software quality (CRF; lower is better)")));
        assert!(!support::text(&mut a, 80, 25)?
            .contains(language.text("Hardware bitrate (Mbps; blank: Auto)")));
        key(&mut a, K::Esc);
        assert_eq!(a.config.video, Default::default());
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(VIDEO_MERGE, ActionId(3)));
        let frame = draw(&mut a, 80, 25)?;
        assert!(!a
            .mouse
            .regions
            .iter()
            .any(|(_, t)| *t == Target::Plugin(VIDEO_MERGE, ActionId(10))));
        for _ in 0..3 {
            assert_eq!(draw(&mut a, 80, 25)?, frame);
        }
        key(&mut a, K::Esc);
        assert!(!a.desktop.contains(owner));
    }
    Ok(())
}
#[test]
fn source_mouse_wheel_and_keyboard_navigation_reach_long_lists() -> io::Result<()> {
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    let t = Fixture::new("video-wheel")?;
    for i in 0..7 {
        fs::write(t.0.join(format!("clip{i}.mp4")), b"private placeholder")?;
    }
    let mut a = t.app()?;
    a.config.set_plugin_enabled(VIDEO_MERGE, true);
    a.panels[0].selected = a.panels[0]
        .entries
        .iter()
        .filter(|e| {
            e.path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"))
        })
        .map(|e| e.name.clone())
        .collect();
    menu::execute(&mut a, Action::MergeVideos);
    draw(&mut a, 80, 25)?;
    let r = a
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::Plugin(VIDEO_MERGE, ActionId(100)))
        .unwrap()
        .0;
    for _ in 0..6 {
        dn::mouse::handle(
            &mut a,
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: r.x,
                row: r.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        draw(&mut a, 80, 25)?;
    }
    assert!(a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Plugin(VIDEO_MERGE, ActionId(106))));
    key(&mut a, K::Home);
    draw(&mut a, 80, 25)?;
    assert!(a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Plugin(VIDEO_MERGE, ActionId(100))));
    assert!(!a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Plugin(VIDEO_MERGE, ActionId(106))));
    Ok(())
}

fn source_rows(app: &mut dn::app::App) -> io::Result<Vec<String>> {
    let frame = draw(app, 80, 25)?;
    let mut rows = std::collections::BTreeMap::new();
    for (r, target) in &app.mouse.regions {
        if let Target::Plugin(VIDEO_MERGE, ActionId(id)) = target {
            if (100..200).contains(id) {
                rows.insert(
                    *id,
                    (r.x..r.x + r.width)
                        .map(|x| frame[(x, r.y)].symbol())
                        .collect::<String>(),
                );
            }
        }
    }
    Ok(rows.into_values().collect())
}
#[test]
fn utilities_open_captures_marked_files_and_folders_from_active_pane() -> io::Result<()> {
    for active in [0, 1] {
        let t = Fixture::new("video-marked")?;
        fs::write(t.0.join("marked-Ёжик.MOV"), b"private placeholder")?;
        fs::write(t.0.join("unmarked.mp4"), b"private placeholder")?;
        fs::create_dir(t.0.join("marked-folder"))?;
        let mut app = t.app()?;
        app.active = active;
        app.config.set_plugin_enabled(VIDEO_MERGE, true);
        app.panels[active].selected = ["marked-Ёжик.MOV".into(), "marked-folder".into()].into();
        app.panels[1 - active].selected = ["unmarked.mp4".into()].into();
        let other = app.panels[1 - active].selected.clone();
        let selected = app.panels[active].selected.clone();
        menu::execute(&mut app, Action::Plugin(VIDEO_MERGE));
        let rows = source_rows(&mut app)?;
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows[0].contains("marked-folder"));
        assert!(rows[1].contains("marked-Ёжик.MOV"));
        assert!(!rows.iter().any(|s| s.contains("unmarked.mp4")));
        assert_eq!(app.panels[active].selected, selected);
        assert_eq!(app.panels[1 - active].selected, other);
    }
    Ok(())
}
#[test]
fn file_open_does_not_duplicate_prefill_and_utilities_reactivation_preserves_list() -> io::Result<()>
{
    let t = Fixture::new("video-prefill-once")?;
    for name in ["one.mp4", "two.mov"] {
        fs::write(t.0.join(name), b"private placeholder")?;
    }
    let mut app = t.app()?;
    app.config.set_plugin_enabled(VIDEO_MERGE, true);
    app.panels[0].selected = ["one.mp4".into()].into();
    menu::execute(&mut app, Action::MergeVideos);
    assert_eq!(source_rows(&mut app)?.len(), 1);
    menu::execute(&mut app, Action::MergeVideos);
    assert_eq!(source_rows(&mut app)?.len(), 1);
    app.panels[0].selected = ["two.mov".into()].into();
    menu::execute(&mut app, Action::Plugin(VIDEO_MERGE));
    let rows = source_rows(&mut app)?;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains("one.mp4"));
    menu::execute(&mut app, Action::MergeVideos);
    let rows = source_rows(&mut app)?;
    assert_eq!(rows.len(), 2);
    assert!(rows[1].contains("two.mov"));
    Ok(())
}
#[test]
fn utilities_prefill_ignores_filtered_marks_and_keeps_empty_unmarked_opening() -> io::Result<()> {
    let t = Fixture::new("video-visible-marks")?;
    for name in ["visible.mov", "hidden.mp4"] {
        fs::write(t.0.join(name), b"private placeholder")?;
    }
    let mut app = t.app()?;
    app.config.set_plugin_enabled(VIDEO_MERGE, true);
    menu::execute(&mut app, Action::Plugin(VIDEO_MERGE));
    assert!(source_rows(&mut app)?.is_empty());
    key(&mut app, K::Esc);
    app.panels[0].selected = ["visible.mov".into(), "hidden.mp4".into()].into();
    app.panels[0].set_filter("*.mov".into());
    menu::execute(&mut app, Action::Plugin(VIDEO_MERGE));
    let rows = source_rows(&mut app)?;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains("visible.mov"));
    Ok(())
}
