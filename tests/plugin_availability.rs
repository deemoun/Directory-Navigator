//! Availability presentation uses private fixtures and never starts devices or tools.
mod support;
use dn::{
    localization::Language,
    menu::{self, Action},
    palette::PaletteProfile,
    plugins::{APK_WORKSHOP, BLUETOOTH_TRANSFERS, DOS_LAUNCHER, REMOTE_CONNECTIONS, VIDEO_MERGE},
};
use std::io;

#[test]
fn unchecked_plugins_explain_requirements_without_starting_checks() -> io::Result<()> {
    let fixture = support::Fixture::new("plugin-availability")?;
    for language in [Language::English, Language::Russian] {
        for palette in [
            PaletteProfile::ClassicDn,
            PaletteProfile::ClassicDn.with_slot(dn::palette::theme::Slot::DialogText, 0x1e),
        ] {
            for (plugin, expected) in [
                (
                    DOS_LAUNCHER,
                    "Not checked. Use Find emulators or Check DOSBox.",
                ),
                (
                    VIDEO_MERGE,
                    "FFmpeg and FFprobe not checked. Open Settings to check tools.",
                ),
                (
                    BLUETOOTH_TRANSFERS,
                    "Bluetooth not checked. Refresh reads paired devices.",
                ),
                (
                    REMOTE_CONNECTIONS,
                    "File transfers are built in. SSH terminal requires OpenSSH.",
                ),
            ] {
                let mut app = fixture.app()?;
                app.config.language = language;
                app.config.palette = palette;
                app.config.set_plugin_enabled(plugin, true);
                menu::execute(&mut app, Action::Plugin(plugin));
                let owner = app.desktop.active().id;
                let text = support::text(&mut app, 80, 25)?;
                assert_ne!(
                    Language::English.text(expected),
                    Language::Russian.text(expected)
                );
                assert!(text.contains(language.text(expected)), "{plugin:?}: {text}");
                for (w, h) in [(80, 25), (120, 35), (50, 15), (80, 25)] {
                    let first = support::draw(&mut app, w, h)?;
                    assert_eq!(first, support::draw(&mut app, w, h)?);
                    assert_eq!(owner, app.desktop.active().id);
                }
                assert!(app.last_dialog().is_none());
            }
        }
    }
    Ok(())
}

#[test]
fn missing_video_tools_show_next_steps_and_keep_the_original_cause() -> io::Result<()> {
    use std::time::{Duration, Instant};
    let fixture = support::Fixture::new("missing-video-tools")?;
    for language in [Language::English, Language::Russian] {
        let mut app = fixture.app()?;
        app.config.language = language;
        app.config.video.ffmpeg = fixture
            .0
            .join("missing-ffmpeg")
            .to_string_lossy()
            .into_owned();
        app.config.video.ffprobe = fixture
            .0
            .join("missing-ffprobe")
            .to_string_lossy()
            .into_owned();
        app.config.set_plugin_enabled(VIDEO_MERGE, true);
        menu::execute(&mut app, Action::Plugin(VIDEO_MERGE));
        support::draw(&mut app, 80, 25)?;
        support::click(
            &mut app,
            dn::mouse::Target::Plugin(VIDEO_MERGE, dn::plugins::ActionId(9)),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.poll();
            let text = support::text(&mut app, 80, 25)?;
            if text
                .contains(language.text("Tool check failed. Install FFmpeg/FFprobe. F1: details."))
            {
                assert!(
                    text.contains(
                        language.text("Set executable paths in Advanced, then Check tools.")
                    ),
                    "{text}"
                );
                break;
            }
            assert!(Instant::now() < deadline, "{text}");
            std::thread::sleep(Duration::from_millis(1));
        }
        support::key(&mut app, crossterm::event::KeyCode::Esc);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.poll();
            let text = support::text(&mut app, 80, 25)?;
            if text.contains("FFmpeg/FFprobe") {
                break;
            }
            assert!(Instant::now() < deadline, "{text}");
            std::thread::sleep(Duration::from_millis(1));
        }
        support::key(&mut app, crossterm::event::KeyCode::F(1));
        assert!(app
            .last_dialog()
            .unwrap()
            .message
            .contains("missing-ffmpeg"));
    }
    Ok(())
}

#[test]
fn apk_tools_explain_optional_roles_and_unverified_paths() -> io::Result<()> {
    let fixture = support::Fixture::new("apk-tool-availability")?;
    for language in [Language::English, Language::Russian] {
        let mut app = fixture.app()?;
        app.config.language = language;
        app.config.set_plugin_enabled(APK_WORKSHOP, true);
        menu::execute(&mut app, Action::Plugin(APK_WORKSHOP));
        support::draw(&mut app, 80, 25)?;
        support::key(&mut app, crossterm::event::KeyCode::Esc); // Dismiss the initial Open picker.
        support::draw(&mut app, 80, 25)?;
        assert!(
            app.mouse.regions.iter().any(|(_, target)| *target
                == dn::mouse::Target::Plugin(APK_WORKSHOP, dn::plugins::ActionId(22))),
            "{}",
            support::text(&mut app, 80, 25)?
        );
        support::click(
            &mut app,
            dn::mouse::Target::Plugin(APK_WORKSHOP, dn::plugins::ActionId(22)),
        );
        let text = support::text(&mut app, 80, 25)?;
        assert!(
            text.contains(language.text("Inspection works without external tools.")),
            "{text}"
        );
        assert!(
            text.contains(language.text("Not configured"))
                || text.contains(language.text("Path set; not verified")),
            "{text}"
        );
    }
    Ok(())
}
