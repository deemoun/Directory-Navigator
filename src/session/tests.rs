use super::*;
use crate::{
    app::App,
    config::{Config, Store},
    desktop::WindowId,
    testing::{backend::TestBackend, Terminal},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(fs::canonicalize(p)?))
    }
    fn app(&self) -> io::Result<App> {
        let mut a = App::new(self.0.clone())?;
        a.config_store = Store::new(Some(self.0.join("config.conf")));
        Ok(a)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn pump(a: &mut App) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    loop {
        a.poll();
        terminal.draw(|f| ui::draw(f, a))?;
        if !a.session.busy() && a.panels.iter().all(|p| !p.loading) {
            break;
        }
        if Instant::now() > deadline {
            return Err(io::Error::other("Session did not settle"));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}
fn record(path: PathBuf, kind: Kind) -> DocumentRecord {
    DocumentRecord {
        path,
        kind,
        geometry: Some(Geometry {
            bounds: Rect::new(3, 3, 60, 18),
            maximized: false,
        }),
        format: Some(TextFormat::default()),
        position: 1,
        column: 2,
        scroll: 1,
        horizontal: 0,
        mode: crate::viewer::ViewMode::Text,
        markdown: false,
        wrap: true,
    }
}
fn snapshot(t: &Temp) -> Snapshot {
    Snapshot {
        panels: std::array::from_fn(|_| PanelRecord {
            path: t.0.clone(),
            cursor: None,
            filter: String::new(),
            offset: 0,
        }),
        active_panel: 1,
        documents: vec![],
        active_document: None,
    }
}
#[test]
fn session_codec_roundtrip_defaults_validation_and_unknown_settings() -> io::Result<()> {
    let t = Temp::new()?;
    let mut c = Config::default();
    assert_eq!(c.restore_session, RestoreLevel::Panels);
    assert!(c.session.is_none());
    let mut snap = snapshot(&t);
    snap.documents
        .push(record(t.0.join("Ёжик.txt"), Kind::Editor));
    snap.active_document = Some(0);
    c.restore_session = RestoreLevel::Documents;
    c.session = Some(snap.clone());
    let path = t.0.join("config.conf");
    fs::write(
        &path,
        "config_version=2\n[custom]\nkeep='yes'\n[session]\nfuture='untouched'\n",
    )?;
    c.save(&path)?;
    assert_eq!(Config::read(&path)?.session, Some(snap));
    let text = fs::read_to_string(&path)?;
    assert!(text.contains("untouched") && text.contains("yes"));
    assert_eq!(
        Config::read(&path)?.restore_session,
        RestoreLevel::Documents
    );
    for source in [
        "[session]\nrestore=17\nsnapshot='broken'",
        "session='broken'",
        "[session]\nrestore='documents'\n[session.snapshot]\nactive_panel=99",
    ] {
        let c = Config::parse(
            format!("config_version=2\n{source}\n[interface]\nshow_clock=false\n").as_bytes(),
        )?;
        assert!(!c.show_clock);
        assert!(c.invalid_session);
        assert!(c.session.is_none());
    }
    Ok(())
}
#[test]
fn panel_restoration_preserves_filter_cursor_scroll_and_clears_marks() -> io::Result<()> {
    let t = Temp::new()?;
    let dir = t.0.join("Ёжик");
    fs::create_dir(&dir)?;
    for i in 0..50 {
        fs::write(dir.join(format!("item-{i:02}.txt")), "text")?;
    }
    let mut a = t.app()?;
    pump(&mut a)?;
    let mut snap = snapshot(&t);
    snap.panels[0] = PanelRecord {
        path: dir.clone(),
        cursor: Some("item-37.txt".into()),
        filter: "*.txt".into(),
        offset: 30,
    };
    a.config.session = Some(snap);
    begin(&mut a, false);
    pump(&mut a)?;
    assert_eq!(a.panels[0].path, dir);
    assert_eq!(a.panels[0].filter, "*.txt");
    assert_eq!(a.panels[0].entries[a.panels[0].cursor].name, "item-37.txt");
    assert!(a.panels[0].selected.is_empty());
    assert_eq!(a.active, 1);
    assert!(a.panels[0].offset > 0);
    Ok(())
}
#[test]
fn missing_directories_fall_back_and_bypass_keeps_launch_location() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    pump(&mut a)?;
    let mut snap = snapshot(&t);
    snap.panels[0].path = t.0.join("gone/child");
    a.config.session = Some(snap.clone());
    begin(&mut a, true);
    pump(&mut a)?;
    assert_eq!(a.panels[0].path, t.0);
    begin(&mut a, false);
    pump(&mut a)?;
    assert_eq!(a.panels[0].path, t.0);
    assert!(a.notice.contains("skipped"));
    a.config.restore_session = RestoreLevel::Off;
    begin(&mut a, false);
    assert!(capture(&a)?.is_none());
    Ok(())
}
#[test]
fn documents_reopen_current_contents_positions_geometry_and_background_focus() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("notes.txt");
    fs::write(&path, "first\nsecond Ёжик\nthird\n")?;
    let mut a = t.app()?;
    pump(&mut a)?;
    let mut snap = snapshot(&t);
    snap.documents = vec![
        record(path.clone(), Kind::Viewer),
        record(path.clone(), Kind::Editor),
    ];
    snap.active_document = Some(1);
    a.config.restore_session = RestoreLevel::Documents;
    a.config.session = Some(snap);
    begin(&mut a, false);
    pump(&mut a)?;
    let eid = a.desktop.id(crate::desktop::WindowKind::Editor).unwrap();
    let e = a.desktop.editor(eid).unwrap();
    assert_eq!(e.buffer.as_ref().unwrap().row(), 1);
    assert_eq!(e.buffer.as_ref().unwrap().column(), 2);
    assert!(!e.dirty());
    assert_eq!(a.desktop.active().id, eid);
    let g = a.desktop.native().session_geometry(eid).unwrap();
    assert!(!g.maximized);
    assert_eq!(g.bounds.width, 60);
    let before = capture(&a)?.unwrap();
    assert_eq!(before.documents.len(), 2);
    assert_eq!(before.active_document, Some(1));
    assert_eq!(before.documents[0].position, 1);
    let mut next = t.app()?;
    pump(&mut next)?;
    next.config.restore_session = RestoreLevel::Documents;
    next.config.session = Some(before);
    begin(&mut next, false);
    interaction(&mut next);
    pump(&mut next)?;
    assert_eq!(next.desktop.active().id, WindowId::MANAGER);
    assert!(next
        .desktop
        .id(crate::desktop::WindowKind::Editor)
        .is_some());
    Ok(())
}
#[test]
fn save_rules_off_snapshot_clearing_and_concurrent_instance_protection() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    pump(&mut a)?;
    a.config.save_on_exit = false;
    assert!(a.save_settings_on_exit());
    assert!(!t.0.join("config.conf").exists());
    a.save_config();
    let original = fs::read(t.0.join("config.conf"))?;
    assert!(Config::read(&t.0.join("config.conf"))?.session.is_some());
    let mut other = t.app()?;
    other.load_config();
    other.config.restore_session = RestoreLevel::Off;
    other.save_config();
    assert!(Config::read(&t.0.join("config.conf"))?.session.is_none());
    a.config.save_on_exit = true;
    assert!(!a.save_settings_on_exit());
    assert!(!a.quit);
    assert_ne!(fs::read(t.0.join("config.conf"))?, original);
    Ok(())
}
#[test]
fn cancelled_quit_does_not_save_and_preserves_original_focus_layout() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("file.txt");
    fs::write(&path, "original")?;
    let mut a = t.app()?;
    pump(&mut a)?;
    let loaded = crate::editor::storage::load(&path).map_err(io::Error::other)?;
    let e = crate::editor::Editor::session_document(path, loaded, a.config.editor)
        .map_err(io::Error::other)?;
    let id = a.desktop.open_editor(e)?;
    crate::windowing::activate(&mut a, WindowId::MANAGER);
    a.config.restore_session = RestoreLevel::Documents;
    a.save_config();
    let saved = fs::read(t.0.join("config.conf"))?;
    a.desktop
        .editor_mut(id)
        .unwrap()
        .buffer
        .as_mut()
        .unwrap()
        .insert("draft")
        .map_err(io::Error::other)?;
    a.request_exit();
    assert_eq!(a.session.quit_active, Some(WindowId::MANAGER));
    assert!(!a.quit);
    assert_eq!(capture(&a)?.unwrap().active_document, None);
    a.abort_quit();
    assert!(a.session.quit_layout.is_none());
    assert_eq!(fs::read(t.0.join("config.conf"))?, saved);
    assert!(a.desktop.editor(id).unwrap().dirty());
    Ok(())
}
#[test]
fn oversized_snapshot_preserves_the_existing_configuration() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("config.conf");
    let mut c = Config::default();
    c.save(&path)?;
    let old = fs::read(&path)?;
    let mut snap = snapshot(&t);
    snap.documents = (0..=MAX_DOCUMENTS)
        .map(|_| record(t.0.join("x"), Kind::Viewer))
        .collect();
    c.session = Some(snap);
    assert!(c.save(&path).is_err());
    assert_eq!(fs::read(&path)?, old);
    let mut snap = snapshot(&t);
    snap.panels[0].filter = "x".repeat(1024 * 1024);
    c.session = Some(snap);
    assert!(c.save(&path).is_err());
    assert_eq!(fs::read(&path)?, old);
    Ok(())
}

#[test]
fn unavailable_documents_are_skipped_and_legacy_encoding_is_retained() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("legacy.txt");
    let format = crate::text_encoding::TextEncoding::Cp866.default_format();
    fs::write(
        &path,
        format
            .encode("Ёжик\r\nВторая\n")
            .map_err(io::Error::other)?,
    )?;
    let mut snap = snapshot(&t);
    let mut legacy = record(path, Kind::Editor);
    legacy.format = Some(format);
    snap.documents = vec![record(t.0.join("missing.txt"), Kind::Viewer), legacy];
    snap.active_document = Some(1);
    let mut a = t.app()?;
    pump(&mut a)?;
    a.config.restore_session = RestoreLevel::Documents;
    a.config.session = Some(snap);
    begin(&mut a, false);
    pump(&mut a)?;
    let e = a.desktop.active_editor().unwrap();
    assert_eq!(e.text_format(), format);
    assert_eq!(e.buffer.as_ref().unwrap().text, "Ёжик\r\nВторая\n");
    assert!(a.notice.contains("skipped"));
    assert_eq!(a.desktop.windows().count(), 2);
    Ok(())
}
#[test]
fn archive_panes_capture_only_outer_directory_and_plain_file_highlight() -> io::Result<()> {
    use std::io::Write;
    let t = Temp::new()?;
    let path = t.0.join("fixture.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path)?);
    zip.start_file("docs/file.txt", zip::write::SimpleFileOptions::default())?;
    zip.write_all(b"hello")?;
    zip.finish()?;
    let mut a = t.app()?;
    pump(&mut a)?;
    crate::archive::open(&mut a, path.clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    while a.panels[0].archive().is_none_or(|a| a.catalog.is_none()) {
        a.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let snap = capture(&a)?.unwrap();
    assert_eq!(snap.panels[0].path, t.0);
    assert_eq!(snap.panels[0].cursor, Some("fixture.zip".into()));
    let mut next = t.app()?;
    pump(&mut next)?;
    next.config.session = Some(snap);
    begin(&mut next, false);
    pump(&mut next)?;
    assert!(!next.panels[0].is_archive());
    assert_eq!(
        next.panels[0].entries[next.panels[0].cursor].name,
        "fixture.zip"
    );
    Ok(())
}

#[path = "../../tests/support/pdf.rs"]
mod pdf_fixtures;
#[test]
fn restored_encrypted_pdf_defers_its_owned_password_until_interaction() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("locked.pdf");
    fs::write(
        &path,
        pdf_fixtures::encrypted(&[b"Private PDF"], "secret", true)?,
    )?;
    let mut r = record(path, Kind::Viewer);
    r.mode = crate::viewer::ViewMode::Auto;
    r.format = None;
    let mut snap = snapshot(&t);
    snap.documents = vec![r];
    snap.active_document = Some(0);
    let mut a = t.app()?;
    pump(&mut a)?;
    a.config.restore_session = RestoreLevel::Documents;
    a.config.session = Some(snap);
    begin(&mut a, false);
    pump(&mut a)?;
    let id = a.desktop.active().id;
    let v = a.desktop.viewer(id).unwrap();
    assert!(v.contents.as_ref().unwrap().pdf.as_ref().unwrap().locked);
    assert!(v.modals.viewer_pdf_password().is_none());
    interaction(&mut a);
    a.poll();
    assert!(a
        .desktop
        .viewer(id)
        .unwrap()
        .modals
        .viewer_pdf_password()
        .is_some());
    assert_eq!(a.desktop.active().id, id);
    Ok(())
}
#[test]
fn viewer_modes_anchors_and_encodings_roundtrip_without_mutating_saved_text() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("file.md");
    fs::write(&path, "# Heading\n\nSecond paragraph\n\nThird paragraph\n")?;
    for (mode, markdown) in [
        (crate::viewer::ViewMode::Auto, true),
        (crate::viewer::ViewMode::Text, false),
        (crate::viewer::ViewMode::Hex, false),
    ] {
        let mut a = t.app()?;
        pump(&mut a)?;
        let mut r = record(path.clone(), Kind::Viewer);
        r.mode = mode;
        r.markdown = markdown;
        r.position = if mode == crate::viewer::ViewMode::Hex {
            16
        } else {
            2
        };
        let mut snap = snapshot(&t);
        snap.documents = vec![r];
        snap.active_document = Some(0);
        a.config.restore_session = RestoreLevel::Documents;
        a.config.session = Some(snap);
        begin(&mut a, false);
        pump(&mut a)?;
        let saved = capture(&a)?.unwrap();
        assert_eq!(saved.documents[0].mode, mode);
        assert_eq!(saved.documents[0].markdown, markdown);
        assert_eq!(
            saved.documents[0].position,
            if mode == crate::viewer::ViewMode::Hex {
                16
            } else {
                2
            }
        );
        let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
        for _ in 0..3 {
            terminal.draw(|f| ui::draw(f, &mut a))?;
            assert_eq!(capture(&a)?.unwrap(), saved);
        }
    }
    Ok(())
}
#[test]
fn quit_capture_uses_the_final_saved_path_and_never_contains_buffer_text() -> io::Result<()> {
    let t = Temp::new()?;
    let old = t.0.join("old.txt");
    let new = t.0.join("new.txt");
    fs::write(&old, "on disk")?;
    fs::write(&new, "new disk")?;
    let mut a = t.app()?;
    pump(&mut a)?;
    let loaded = crate::editor::storage::load(&old).map_err(io::Error::other)?;
    let e = crate::editor::Editor::session_document(old, loaded, a.config.editor)
        .map_err(io::Error::other)?;
    let id = a.desktop.open_editor(e)?;
    a.config.restore_session = RestoreLevel::Documents;
    remember_quit(&mut a);
    let e = a.desktop.editor_mut(id).unwrap();
    e.path = Some(new.clone());
    e.buffer
        .as_mut()
        .unwrap()
        .insert("private unsaved text")
        .map_err(io::Error::other)?;
    let saved = capture(&a)?.unwrap();
    assert_eq!(saved.documents[0].path, new);
    assert!(!encode(&saved)?.to_string().contains("private unsaved text"));
    Ok(())
}
