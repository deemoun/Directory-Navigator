mod support;
use crossterm::event::KeyCode as K;
use dn::{localization::Language, mouse::Target, session::RestoreLevel, settings::SettingsKind};
use std::io;
#[test]
fn restore_choice_drafts_child_cancel_mouse_and_immutable_frames() -> io::Result<()> {
    for language in [Language::English, Language::Russian] {
        let f = support::Fixture::new("session-settings")?;
        let mut a = f.app()?;
        support::wait(&mut a);
        a.config.language = language;
        for size in [(80, 25), (132, 40)] {
            a.open_settings(SettingsKind::System);
            for _ in 0..3 {
                let text = support::text(&mut a, size.0, size.1)?;
                assert!(text.contains(language.text("Restore previous session")));
                assert!(text.contains(language.text("Panels only")));
            }
            support::render(&mut a, size.0, size.1)?;
            support::click(&mut a, Target::OpenSettingsChoice(2));
            assert!(a.modals.settings_choice().is_some());
            support::key(&mut a, K::Down);
            support::key(&mut a, K::Esc);
            assert_eq!(
                a.modals.settings().unwrap().draft.restore_session,
                RestoreLevel::Panels
            );
            support::render(&mut a, size.0, size.1)?;
            support::click(&mut a, Target::OpenSettingsChoice(2));
            support::key(&mut a, K::Down);
            support::key(&mut a, K::Enter);
            assert_eq!(
                a.modals.settings().unwrap().draft.restore_session,
                RestoreLevel::Documents
            );
            assert_eq!(a.config.restore_session, RestoreLevel::Panels);
            support::key(&mut a, K::Esc);
            assert_eq!(a.config.restore_session, RestoreLevel::Panels);
            a.open_settings(SettingsKind::System);
            support::render(&mut a, size.0, size.1)?;
            support::key(&mut a, K::Tab);
            support::key(&mut a, K::Tab);
            support::key(&mut a, K::Char(' '));
            support::key(&mut a, K::Tab);
            support::key(&mut a, K::Enter);
            assert_eq!(a.config.restore_session, RestoreLevel::Documents);
            a.config.restore_session = RestoreLevel::Panels;
        }
    }
    Ok(())
}
#[test]
fn loading_desktop_preferences_does_not_navigate_or_open_documents() -> io::Result<()> {
    let f = support::Fixture::new("session-load")?;
    let mut a = f.app()?;
    support::wait(&mut a);
    a.save_config();
    let paths = [a.panels[0].path.clone(), a.panels[1].path.clone()];
    let count = a.desktop.windows().count();
    a.load_config();
    assert_eq!([a.panels[0].path.clone(), a.panels[1].path.clone()], paths);
    assert_eq!(a.desktop.windows().count(), count);
    Ok(())
}
