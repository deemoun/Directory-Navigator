#[path = "support/pdf.rs"]
mod fixture;
mod support;
use dn::{
    task::Cancellation,
    viewer::{pdf, SearchMemory, ViewMode, Viewer},
};
use std::{
    fs, io,
    time::{Duration, Instant},
};
fn loaded(path: std::path::PathBuf, mode: ViewMode) -> io::Result<Viewer> {
    let mut v = Viewer::open_mode(path, mode)?;
    let until = Instant::now() + Duration::from_secs(8);
    while v.contents.is_none() {
        if let Some(r) = v.poll() {
            r.map_err(io::Error::other)?;
        }
        assert!(Instant::now() < until);
        std::thread::yield_now();
    }
    v.layout(76);
    Ok(v)
}
#[test]
fn compressed_pages_unicode_search_navigation_and_original_bytes() -> io::Result<()> {
    let bytes = fixture::bytes(&[b"first page", b"second page \x80\x81"])?;
    let c = pdf::extract(bytes.clone(), &Cancellation::default())?;
    assert!(!c.binary);
    assert_eq!(c.raw.as_deref(), Some(bytes.as_slice()));
    assert!(c.format.is_none());
    assert_eq!(c.pdf.as_ref().unwrap().page_lines.len(), 2);
    assert!(c.source.as_deref().unwrap().contains("Жя"));
    let mut v = Viewer::from_contents("PDF snapshot".into(), c);
    assert!(v.wrap);
    assert!(!v.encoding_available());
    v.layout(20);
    v.go_to("2").unwrap();
    let page = v
        .contents
        .as_ref()
        .unwrap()
        .pdf
        .as_ref()
        .unwrap()
        .page(v.source_line());
    assert_eq!(page, 2);
    assert!(v.go_to("0").is_err());
    assert!(v.go_to("3").is_err());
    assert!(v
        .find(
            &SearchMemory {
                query: "Жя".into(),
                case_sensitive: true
            },
            false,
            false
        )
        .unwrap());
    v.wrap = false;
    v.layout(132);
    assert!(v
        .contents
        .as_ref()
        .unwrap()
        .source
        .as_deref()
        .unwrap()
        .contains("first page"));
    Ok(())
}
#[test]
fn automatic_f3_is_pdf_but_explicit_text_hex_and_quick_view_stay_raw() -> io::Result<()> {
    let f = support::Fixture::new("pdf-routing")?.canonicalized()?;
    let path = f.0.join("document.PDF");
    let bytes = fixture::bytes(&[b"internal PDF fixture"])?;
    fs::write(&path, &bytes)?;
    assert!(loaded(path.clone(), ViewMode::Auto)?
        .contents
        .unwrap()
        .pdf
        .is_some());
    assert!(loaded(path.clone(), ViewMode::Hex)?
        .contents
        .unwrap()
        .pdf
        .is_none());
    assert!(loaded(path.clone(), ViewMode::Text)?
        .contents
        .unwrap()
        .pdf
        .is_none());
    let mut quick = Viewer::open_preview(path)?;
    let end = Instant::now() + Duration::from_secs(5);
    while quick.contents.is_none() {
        quick.poll();
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    assert!(quick.contents.unwrap().pdf.is_none());
    Ok(())
}
#[test]
fn scanned_password_invalid_cancelled_and_oversized_pdfs_are_explicit() -> io::Result<()> {
    let c = pdf::extract(fixture::bytes(&[b""])?, &Cancellation::default())?;
    assert!(c.pdf.as_ref().unwrap().empty);
    let v = Viewer::from_contents("scan".into(), c);
    assert_eq!(v.notice, pdf::EMPTY_NOTICE);
    assert!(pdf::extract(b"%PDF-1.7\ngarbage".to_vec(), &Cancellation::default()).is_err());
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        pdf::extract(fixture::bytes(&[b"hello"])?, &cancel)
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::Interrupted
    );
    let mut doc = fixture::document(&[b"secret"]);
    let state = pdf_extract::EncryptionState::try_from(pdf_extract::EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner",
        user_password: "secret",
        permissions: pdf_extract::Permissions::COPYABLE,
    })
    .map_err(io::Error::other)?;
    doc.encrypt(&state).map_err(io::Error::other)?;
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes)?;
    assert!(pdf::extract(bytes, &Cancellation::default())
        .err()
        .unwrap()
        .to_string()
        .contains("password"));
    let many = vec![b"x".as_slice(); pdf::MAX_PAGES + 1];
    assert!(
        pdf::extract(fixture::bytes(&many)?, &Cancellation::default())
            .err()
            .unwrap()
            .to_string()
            .contains("limit")
    );
    Ok(())
}
#[test]
fn zip_member_is_extracted_text_in_auto_and_original_bytes_in_hex() -> io::Result<()> {
    use dn::archive::reader;
    use std::io::Write;
    let f = support::Fixture::new("pdf-archive")?.canonicalized()?;
    let archive = f.0.join("docs.zip");
    let bytes = fixture::bytes(&[b"PDF archive text"])?;
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive)?);
    zip.start_file("docs/inside.pdf", zip::write::SimpleFileOptions::default())?;
    zip.write_all(&bytes)?;
    zip.finish()?;
    let c = reader::list(archive, &Cancellation::default())?;
    let id = c
        .nodes
        .iter()
        .position(|n| n.path == b"docs/inside.pdf")
        .unwrap();
    let preview = reader::preview(&c, id, &Cancellation::default())?;
    assert!(preview.pdf.is_some());
    assert!(preview.source.unwrap().contains("PDF archive text"));
    let raw = reader::preview_with_mode(&c, id, None, ViewMode::Hex, &Cancellation::default())?;
    assert!(raw.pdf.is_none());
    assert!(raw.binary);
    assert_eq!(raw.raw.as_deref(), Some(bytes.as_slice()));
    Ok(())
}

#[test]
fn pdf_ui_is_localized_immutable_resize_safe_and_encoding_is_disabled() -> io::Result<()> {
    use crossterm::event::KeyCode as K;
    use dn::{editor, localization::Language, mouse::Target, viewer::Action};
    let f = support::Fixture::new("pdf-ui")?.canonicalized()?;
    for lang in [Language::English, Language::Russian] {
        let mut app = f.app()?;
        app.config.language = lang;
        app.config.show_clock = false;
        if lang == Language::Russian {
            app.config.palette = dn::palette::PaletteProfile::ClassicDn
                .with_slot(dn::palette::theme::Slot::ViewerText, 0x1e);
        }
        let c = pdf::extract(
            fixture::bytes(&[b"first", b"second"])?,
            &Cancellation::default(),
        )?;
        app.show_viewer(Viewer::from_contents("PDF fixture".into(), c));
        for (w, h) in [(80, 25), (132, 40), (50, 12), (80, 25)] {
            let first = support::draw(&mut app, w, h)?;
            assert_eq!(first, support::draw(&mut app, w, h)?);
        }
        assert!(support::text(&mut app, 80, 25)?.contains(lang.text("Page")));
        assert!(!app
            .mouse
            .regions
            .iter()
            .any(|(_, t)| *t == Target::Viewer(Action::Encoding)));
        dn::viewer::action(&mut app, Action::GoTo);
        assert!(support::text(&mut app, 80, 25)?.contains(lang.text("Page number (from 1)")));
        support::key(&mut app, K::Esc);
        dn::viewer::action(&mut app, Action::Help);
        assert!(support::text(&mut app, 80, 25)?.contains("PDF"));
        support::key(&mut app, K::Esc);
        let original = app.desktop.active().id;
        support::key(&mut app, K::Esc);
        editor::open(&mut app, true);
        assert_ne!(app.desktop.active().id, original);
    }
    Ok(())
}
#[test]
fn encrypted_zip_member_can_be_read_without_exposing_archive_password() -> io::Result<()> {
    use dn::archive::{self, reader};
    use std::io::Write;
    let f = support::Fixture::new("pdf-encrypted-archive")?.canonicalized()?;
    let path = f.0.join("protected.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path)?);
    zip.start_file(
        "inside.pdf",
        zip::write::SimpleFileOptions::default()
            .with_aes_encryption(zip::AesMode::Aes256, "private"),
    )?;
    zip.write_all(&fixture::bytes(&[b"authenticated PDF"])?)?;
    zip.finish()?;
    let c = reader::list(path, &Cancellation::default())?;
    let text = reader::preview_with_password(
        &c,
        1,
        Some(&archive::Password::new("private")),
        &Cancellation::default(),
    )?;
    assert!(text.pdf.is_some());
    assert!(text.source.unwrap().contains("authenticated PDF"));
    Ok(())
}

#[test]
fn inactive_pdf_completion_does_not_steal_editor_focus() -> io::Result<()> {
    let f = support::Fixture::new("pdf-inactive")?.canonicalized()?;
    let path = f.0.join("background.pdf");
    fs::write(&path, fixture::bytes(&[b"background PDF"])?)?;
    let mut app = f.app()?;
    app.show_viewer(Viewer::open(path)?);
    let pdf_id = app.desktop.active().id;
    dn::editor::open(&mut app, true);
    dn::editor::paste(&mut app, "retained draft");
    let editor_id = app.desktop.active().id;
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        a.desktop
            .viewer(pdf_id)
            .is_some_and(|v| v.contents.is_some())
    });
    assert_eq!(app.desktop.active().id, editor_id);
    assert_eq!(
        app.desktop
            .editor(editor_id)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "retained draft"
    );
    Ok(())
}

#[test]
fn encrypted_pdf_f3_offers_masked_password_prompt() -> io::Result<()> {
    let f = support::Fixture::new("pdf-password-prompt")?.canonicalized()?;
    let path = f.0.join("private.pdf");
    fs::write(
        &path,
        fixture::encrypted(&[b"private PDF text"], "private secret", false)?,
    )?;
    let mut app = f.app()?;
    app.show_viewer(Viewer::open(path)?);
    support::wait_until(&mut app, Duration::from_secs(5), |a| {
        !a.modal_infos().is_empty()
    });
    assert!(support::text(&mut app, 80, 25)?.contains("PDF password"));
    Ok(())
}

#[test]
fn pdf_passwords_unlock_rc4_aes128_aes256_and_keep_encrypted_original_bytes() -> io::Result<()> {
    use dn::archive::Password;
    let utf8_password = "пароль Ω 隠密";
    for (bytes, password) in [
        (
            fixture::encrypted(&[b"private \x80\x81 text"], "legacy", false)?,
            "legacy",
        ),
        (
            fixture::encrypted(&[b"private \x80\x81 text"], "aes128", true)?,
            "aes128",
        ),
        (
            fixture::encrypted_modern(&[b"private \x80\x81 text"], utf8_password)?,
            utf8_password,
        ),
    ] {
        let wrong = pdf::extract_with_password(
            bytes.clone(),
            &Password::new("wrong"),
            &Cancellation::default(),
        )
        .err()
        .expect("wrong password rejected");
        assert_eq!(wrong.kind(), io::ErrorKind::PermissionDenied);
        assert!(!wrong.to_string().contains(password));
        let unlocked = pdf::extract_with_password(
            bytes.clone(),
            &Password::new(password),
            &Cancellation::default(),
        )?;
        assert!(!unlocked.pdf.as_ref().unwrap().locked);
        assert!(unlocked
            .source
            .as_deref()
            .unwrap()
            .contains("private Жя text"));
        assert_eq!(unlocked.raw.as_deref(), Some(bytes.as_slice()));
    }
    let bytes = fixture::encrypted(&[b"no user password"], "", false)?;
    assert!(
        !pdf::extract(bytes, &Cancellation::default())?
            .pdf
            .unwrap()
            .locked
    );
    Ok(())
}
#[test]
fn pdf_password_prompt_retries_is_masked_translated_and_uses_loaded_snapshot() -> io::Result<()> {
    use crossterm::event::KeyCode as K;
    use dn::{localization::Language, modals::ModalOwner};
    let f = support::Fixture::new("pdf-password-retry")?.canonicalized()?;
    for language in [Language::English, Language::Russian] {
        let path = f.0.join("protected.pdf");
        let secret = "unique-password-value";
        let encrypted = fixture::encrypted(&[b"loaded private document"], secret, true)?;
        fs::write(&path, &encrypted)?;
        let mut app = f.app()?;
        app.config.language = language;
        app.config.show_clock = false;
        if language == Language::Russian {
            app.config.palette = dn::palette::PaletteProfile::ClassicDn
                .with_slot(dn::palette::theme::Slot::DialogText, 0x1e);
        }
        app.show_viewer(Viewer::open(path.clone())?);
        let id = app.desktop.active().id;
        support::wait_until(&mut app, Duration::from_secs(8), |a| {
            a.desktop
                .viewer(id)
                .is_some_and(|v| v.modals.viewer_pdf_password().is_some())
        });
        let modal = app.modal_top().unwrap();
        assert_eq!(modal.owner, ModalOwner::Document(id));
        assert!(!app.desktop.viewer(id).unwrap().text_ready());
        for c in "wrong-attempt".chars() {
            support::key(&mut app, K::Char(c));
        }
        let masked = support::text(&mut app, 80, 25)?;
        assert!(!masked.contains("wrong-attempt"));
        assert!(masked.contains("********"));
        support::key(&mut app, K::Enter);
        support::wait_until(&mut app, Duration::from_secs(8), |a| {
            a.desktop
                .viewer(id)
                .unwrap()
                .modals
                .viewer_pdf_password()
                .is_some_and(|d| !d.loading())
        });
        assert!(support::text(&mut app, 80, 25)?
            .contains(language.text("Incorrect PDF password. Try again.")));
        assert_eq!(app.modal_top().unwrap().id, modal.id);
        for (w, h) in [(80, 25), (132, 40), (50, 12), (80, 25)] {
            let first = support::draw(&mut app, w, h)?;
            assert_eq!(first, support::draw(&mut app, w, h)?);
        }
        dn::input::paste(&mut app, &"x".repeat(1025));
        assert!(support::text(&mut app, 80, 25)?
            .contains(language.text("Input is too long or contains control characters")));
        dn::input::paste(&mut app, secret);
        support::key(&mut app, K::F(1));
        assert!(!support::text(&mut app, 80, 25)?.contains(secret));
        support::key(&mut app, K::Esc);
        assert_eq!(app.modal_top().unwrap().id, modal.id);
        assert!(!support::text(&mut app, 80, 25)?.contains(secret));
        // Disk replacement cannot redirect a password retry to another document.
        fs::write(&path, fixture::bytes(&[b"replacement on disk"])?)?;
        support::click(
            &mut app,
            dn::mouse::Target::Viewer(dn::viewer::Action::Submit),
        );
        support::wait_until(&mut app, Duration::from_secs(8), |a| {
            a.desktop.viewer(id).is_some_and(|v| {
                v.contents
                    .as_ref()
                    .is_some_and(|c| c.pdf.as_ref().is_some_and(|p| !p.locked))
            })
        });
        assert!(app.modal_infos().is_empty());
        assert_eq!(app.desktop.active().id, id);
        let text = app
            .desktop
            .viewer(id)
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .source
            .as_deref()
            .unwrap();
        assert!(text.contains("loaded private document"));
        assert!(!text.contains("replacement on disk"));
        assert!(!text.contains(secret));
        assert_eq!(
            app.desktop
                .viewer(id)
                .unwrap()
                .contents
                .as_ref()
                .unwrap()
                .raw
                .as_deref(),
            Some(encrypted.as_slice())
        );
        let config_path = f.0.join(format!("{}.conf", language.key()));
        app.config.save(&config_path)?;
        let config = fs::read_to_string(config_path)?;
        assert!(!config.contains(secret));
        assert!(!config.contains("wrong-attempt"));
    }
    Ok(())
}
#[test]
fn cancelling_pdf_password_restores_previous_editor_and_reopening_prompts_again() -> io::Result<()>
{
    use crossterm::event::KeyCode as K;
    let f = support::Fixture::new("pdf-password-cancel")?.canonicalized()?;
    let path = f.0.join("private.pdf");
    fs::write(
        &path,
        fixture::encrypted(&[b"hidden text"], "secret", false)?,
    )?;
    let mut app = f.app()?;
    dn::editor::open(&mut app, true);
    dn::editor::paste(&mut app, "retained dirty draft");
    let editor = app.desktop.active().id;
    for cancel_in_flight in [false, true] {
        app.show_viewer(Viewer::open(path.clone())?);
        let viewer = app.desktop.active().id;
        support::wait_until(&mut app, Duration::from_secs(8), |a| {
            a.desktop
                .viewer(viewer)
                .is_some_and(|v| v.modals.viewer_pdf_password().is_some())
        });
        dn::viewer::paste(&mut app, "secret");
        if cancel_in_flight {
            dn::viewer::action(&mut app, dn::viewer::Action::Submit);
        }
        support::key(&mut app, K::Esc);
        assert!(!app.desktop.contains(viewer));
        assert_eq!(app.desktop.active().id, editor);
        for _ in 0..3 {
            app.poll();
        }
        assert_eq!(
            app.desktop
                .editor(editor)
                .unwrap()
                .buffer
                .as_ref()
                .unwrap()
                .text,
            "retained dirty draft"
        );
        assert!(app.desktop.editor(editor).unwrap().dirty());
    }
    Ok(())
}

#[test]
fn encrypted_pdf_inside_encrypted_zip_asks_two_distinct_passwords_and_returns_to_archive(
) -> io::Result<()> {
    use crossterm::event::KeyCode as K;
    use dn::{
        archive,
        focus::{self, Focus},
        input,
    };
    use std::io::Write;
    let f = support::Fixture::new("pdf-two-passwords")?.canonicalized()?;
    let path = f.0.join("protected.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path)?);
    zip.start_file(
        "inside.pdf",
        zip::write::SimpleFileOptions::default()
            .with_aes_encryption(zip::AesMode::Aes256, "zip-only"),
    )?;
    zip.write_all(&fixture::encrypted(
        &[b"doubly protected text"],
        "pdf-only",
        true,
    )?)?;
    zip.finish()?;
    let mut app = f.app()?;
    archive::open(&mut app, path);
    support::wait_until(&mut app, Duration::from_secs(8), |a| !a.panels[0].loading);
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "inside.pdf")
        .unwrap();
    support::key(&mut app, K::F(3));
    assert_eq!(focus::current(&app), Focus::ArchiveDialog);
    input::paste(&mut app, "zip-only");
    support::key(&mut app, K::Enter);
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.modals.viewer_pdf_password().is_some())
    });
    assert!(app.modals.archive_dialog().is_none());
    assert!(support::text(&mut app, 80, 25)?.contains("PDF password"));
    input::paste(&mut app, "zip-only");
    support::key(&mut app, K::Enter);
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        a.desktop
            .active_viewer()
            .unwrap()
            .modals
            .viewer_pdf_password()
            .is_some_and(|d| !d.loading())
    });
    assert!(support::text(&mut app, 80, 25)?.contains("Incorrect PDF password"));
    input::paste(&mut app, "pdf-only");
    support::key(&mut app, K::Enter);
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        a.desktop
            .active_viewer()
            .unwrap()
            .modals
            .viewer_pdf_password()
            .is_none()
    });
    assert!(support::text(&mut app, 80, 25)?.contains("doubly protected text"));
    support::key(&mut app, K::Esc);
    assert_eq!(focus::current(&app), Focus::Manager);
    assert!(app.panels[0].is_archive());
    assert_eq!(
        app.panels[0].entries[app.panels[0].cursor].name,
        "inside.pdf"
    );
    Ok(())
}
#[test]
fn inactive_locked_pdf_completion_does_not_open_a_prompt_over_an_editor() -> io::Result<()> {
    let f = support::Fixture::new("pdf-locked-inactive")?.canonicalized()?;
    let path = f.0.join("private.pdf");
    fs::write(&path, fixture::encrypted(&[b"private"], "pw", false)?)?;
    let mut app = f.app()?;
    app.show_viewer(Viewer::open(path)?);
    let pdf_id = app.desktop.active().id;
    dn::editor::open(&mut app, true);
    dn::editor::paste(&mut app, "foreground draft");
    let editor_id = app.desktop.active().id;
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        a.desktop
            .viewer(pdf_id)
            .is_some_and(|v| v.modals.viewer_pdf_password().is_some())
    });
    assert_eq!(app.desktop.active().id, editor_id);
    assert!(app.visible_modals().is_empty());
    assert_eq!(
        app.desktop
            .editor(editor_id)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "foreground draft"
    );
    Ok(())
}
