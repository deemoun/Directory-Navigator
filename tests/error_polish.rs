mod support;
use dn::{config::Store, editor, localization::Language, viewer::Viewer};
use std::{fs, io, time::Duration};
#[test]
fn missing_viewer_reports_action_path_cause_and_restores_focus_in_both_languages() -> io::Result<()>
{
    for language in [Language::English, Language::Russian] {
        let f = support::Fixture::new("error-polish-read")?;
        let mut a = f.app()?;
        support::wait(&mut a);
        a.config.language = language;
        if language == Language::Russian {
            a.config.palette = a
                .config
                .palette
                .with_slot(dn::palette::theme::Slot::DialogText, 0x1e);
        }
        let path = f.0.join("missing Ёжик.txt");
        a.show_viewer(Viewer::open(path.clone())?);
        support::wait_until(&mut a, Duration::from_secs(5), |a| {
            a.last_dialog().is_some()
        });
        let d = a.last_dialog().unwrap();
        assert!(d.message.contains(language.text("Cannot open the file.")));
        assert!(d.message.contains(&path.to_string_lossy().to_string()));
        assert!(d.message.contains(language.text("Cause")));
        for (w, h) in [(80, 25), (132, 40)] {
            let first = support::draw(&mut a, w, h)?;
            for _ in 0..3 {
                assert_eq!(support::draw(&mut a, w, h)?, first);
            }
        }
        support::key(&mut a, crossterm::event::KeyCode::Esc);
        assert_eq!(a.desktop.active().id, dn::desktop::WindowId::MANAGER);
    }
    Ok(())
}
#[test]
fn config_exit_error_retains_preferences_and_names_the_configuration_without_promising_rollback(
) -> io::Result<()> {
    let f = support::Fixture::new("error-polish-config")?;
    let mut a = f.app()?;
    support::wait(&mut a);
    let path = f.0.join("blocked.conf");
    fs::create_dir(&path)?;
    a.config_store = Store::new(Some(path.clone()));
    a.config.save_on_exit = true;
    a.config.show_clock = false;
    a.request_exit();
    assert!(!a.save_settings_on_exit());
    assert!(!a.quit);
    assert!(!a.config.show_clock);
    let message = &a.last_dialog().unwrap().message;
    assert!(message.contains("Exit cancelled"));
    assert!(message.contains("Cause"));
    assert!(message.contains(&path.to_string_lossy().to_string()));
    assert!(!message.contains("did not publish"));
    Ok(())
}
#[test]
fn corrupt_archive_has_context_and_never_replaces_its_source() -> io::Result<()> {
    let f = support::Fixture::new("error-polish-archive")?;
    let mut a = f.app()?;
    support::wait(&mut a);
    let path = f.0.join("broken.zip");
    fs::write(&path, b"not a zip")?;
    dn::archive::open(&mut a, path.clone());
    support::wait_until(&mut a, Duration::from_secs(5), |a| {
        a.last_dialog().is_some()
    });
    let text = &a.last_dialog().unwrap().message;
    assert!(text.contains("Cannot read the archive."));
    assert!(text.contains("Cause"));
    assert!(text.contains("integrity"));
    assert_eq!(fs::read(&path)?, b"not a zip");
    assert!(!a.panels[0].is_archive());
    Ok(())
}
#[test]
fn readonly_save_failure_keeps_dirty_text_original_bytes_and_utf8_advice() -> io::Result<()> {
    let f = support::Fixture::new("error-polish-encoding")?;
    let path = f.0.join("text.txt");
    fs::write(&path, "original")?;
    let mut a = f.app()?;
    support::wait(&mut a);
    editor::open_path(&mut a, path.clone());
    support::wait(&mut a);
    editor::paste(&mut a, "draft ");
    let mut permissions = fs::metadata(&path)?.permissions();
    let original = permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions)?;
    editor::action(&mut a, editor::EditorAction::Save);
    support::wait_until(&mut a, Duration::from_secs(5), |a| {
        a.last_dialog().is_some()
    });
    fs::set_permissions(&path, original)?;
    assert!(a.desktop.active_editor().unwrap().dirty());
    let message = &a.last_dialog().unwrap().message;
    assert!(message.contains("retained"));
    assert!(message.contains("Save As"));
    assert!(message.contains("UTF-8"));
    assert_eq!(fs::read_to_string(&path)?, "original");
    Ok(())
}

#[test]
fn unrepresentable_encoding_reports_utf8_without_changing_the_file() -> io::Result<()> {
    let f = support::Fixture::new("error-polish-unrepresentable")?;
    let path = f.0.join("text.txt");
    fs::write(&path, "original")?;
    let mut a = f.app()?;
    support::wait(&mut a);
    editor::open_path(&mut a, path.clone());
    support::wait(&mut a);
    editor::paste(&mut a, "🙂");
    editor::action(&mut a, editor::EditorAction::SaveEncoding);
    editor::action(&mut a, editor::EditorAction::EncodingChoice(3));
    editor::action(&mut a, editor::EditorAction::EncodingAccept);
    let message = &a.last_dialog().unwrap().message;
    assert!(message.contains("Cannot save the file."));
    assert!(message.contains("UTF-8"));
    assert!(message.contains("Cause"));
    assert!(a.desktop.active_editor().unwrap().dirty());
    assert_eq!(fs::read_to_string(&path)?, "original");
    Ok(())
}
#[test]
fn wrong_archive_password_has_inline_advice_and_owned_help_without_echoing_secret() -> io::Result<()>
{
    use std::io::Write;
    let f = support::Fixture::new("error-polish-password")?;
    let path = f.0.join("secret.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path)?);
    zip.start_file(
        "secret.txt",
        zip::write::SimpleFileOptions::default()
            .with_aes_encryption(zip::AesMode::Aes256, "fixture"),
    )?;
    zip.write_all(b"payload")?;
    zip.finish()?;
    let mut a = f.app()?;
    support::wait(&mut a);
    dn::archive::open(&mut a, path);
    support::wait_until(&mut a, Duration::from_secs(5), |a| {
        a.panels[0].archive().is_some_and(|v| v.catalog.is_some())
    });
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "secret.txt")
        .unwrap();
    dn::archive::view(&mut a);
    dn::archive::dialogs::paste(&mut a, "entered-private-password-42");
    dn::archive::dialogs::action(&mut a, dn::archive::dialogs::Action::Confirm);
    support::wait_until(&mut a, Duration::from_secs(5), |a| {
        a.modals.archive_dialog().is_some_and(|d| !d.loading())
    });
    let text = support::text(&mut a, 80, 25)?;
    assert!(text.contains("Archive could not be unlocked."));
    assert!(text.contains("Check the password"));
    assert!(!text.contains("entered-private-password-42"));
    dn::archive::dialogs::action(&mut a, dn::archive::dialogs::Action::Help);
    let full = &a.last_dialog().unwrap().message;
    assert!(full.contains("Cause"));
    assert!(full.contains("secret.zip"));
    assert!(!full.contains("entered-private-password-42"));
    support::key(&mut a, crossterm::event::KeyCode::Esc);
    assert!(a.modals.archive_dialog().is_some());
    Ok(())
}
