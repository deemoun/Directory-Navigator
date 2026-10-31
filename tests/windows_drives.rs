mod support;
use crossterm::event::{KeyCode, KeyModifiers};
use std::io;
use support::{modified, Fixture};

#[cfg(not(windows))]
#[test]
fn drive_shortcuts_and_strip_are_absent_on_other_platforms() -> io::Result<()> {
    let fixture = Fixture::new("no-drive-strip")?;
    let mut app = fixture.app()?;
    let paths = [app.panels[0].path.clone(), app.panels[1].path.clone()];
    for key in [KeyCode::F(1), KeyCode::F(2)] {
        modified(&mut app, key, KeyModifiers::ALT);
        assert!(app.navigation_dialog().is_none());
        assert_eq!(app.panels[0].path, paths[0]);
        assert_eq!(app.panels[1].path, paths[1]);
    }
    let text = support::text(&mut app, 80, 25)?;
    assert!(!text.contains("[ C"));
    assert!(!text.contains("Change drive"));
    Ok(())
}

#[cfg(windows)]
#[test]
fn both_drive_pickers_capture_the_panel_and_cancel_without_navigation() -> io::Result<()> {
    use dn::{mouse::Target, navigation_tools::Modal};
    let fixture = Fixture::new("drive-picker")?;
    let mut app = fixture.app()?;
    let paths = [app.panels[0].path.clone(), app.panels[1].path.clone()];
    for (panel, key) in [KeyCode::F(1), KeyCode::F(2)].into_iter().enumerate() {
        modified(&mut app, key, KeyModifiers::ALT);
        let Some(Modal::Drives {
            panel: target,
            letters,
            ..
        }) = app.navigation_dialog()
        else {
            panic!("drive picker did not open");
        };
        assert_eq!(*target, panel);
        assert!(!letters.is_empty());
        assert!(letters.windows(2).all(|pair| pair[0] < pair[1]));
        for (width, height) in [(80, 25), (120, 40), (60, 20), (80, 25)] {
            for language in [
                dn::localization::Language::English,
                dn::localization::Language::Russian,
            ] {
                app.config.language = language;
                let before = support::draw(&mut app, width, height)?;
                let after = support::draw(&mut app, width, height)?;
                assert_eq!(before, after);
                assert!(app
                    .mouse
                    .regions
                    .iter()
                    .all(|(r, _)| r.right() <= width && r.bottom() <= height));
            }
        }
        support::key(&mut app, KeyCode::Esc);
        assert!(app.navigation_dialog().is_none());
        assert_eq!(app.panels[0].path, paths[0]);
        assert_eq!(app.panels[1].path, paths[1]);
        support::render(&mut app, 80, 25)?;
        support::click(&mut app, Target::Drive(panel, None));
        assert!(
            matches!(app.navigation_dialog(),Some(Modal::Drives { panel: p, .. }) if *p == panel)
        );
        support::key(&mut app, KeyCode::Esc);
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn full_alphabet_picker_clips_resizes_and_keeps_all_letters_reachable() -> io::Result<()> {
    use dn::{mouse::Target, navigation_tools::Modal};
    let fixture = Fixture::new("all-drive-picker")?;
    let mut app = fixture.app()?;
    let path = app.panels[0].path.clone();
    for (width, height) in [(80, 25), (120, 40), (80, 25)] {
        app.modals.set_navigation_dialog(Some(Modal::Drives {
            panel: 0,
            letters: (b'A'..=b'Z').collect(),
            focus: 25,
        }));
        for language in [
            dn::localization::Language::English,
            dn::localization::Language::Russian,
        ] {
            app.config.language = language;
            support::draw(&mut app, width, height)?;
            assert!(app
                .mouse
                .regions
                .iter()
                .any(|(_, t)| *t == Target::Navigation(dn::navigation_tools::Action::Drive(b'Z'))));
            let before = support::draw(&mut app, width, height)?;
            assert_eq!(before, support::draw(&mut app, width, height)?);
        }
        support::key(&mut app, KeyCode::Esc);
        assert_eq!(app.panels[0].path, path);
    }
    Ok(())
}
