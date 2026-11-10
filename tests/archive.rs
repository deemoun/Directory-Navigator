use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    app::App,
    archive::{
        self,
        reader::{self, Kind},
    },
    focus::{self, Focus},
    input,
    mouse::{self, Target},
    task::Cancellation,
    ui,
};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-archive-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(fs::canonicalize(p)?))
    }
    fn app(&self) -> io::Result<App> {
        let mut a = App::new(self.0.clone())?;
        a.config_store = dn::config::Store::new(Some(self.0.join("config.conf")));
        a.config.save_on_exit = false;
        a.config.show_clock = false;
        wait(&mut a, |a| a.panels.iter().all(|p| !p.loading));
        Ok(a)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(a: &mut App, done: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(12);
    loop {
        a.poll();
        if done(a) {
            break;
        }
        assert!(Instant::now() < end, "worker timeout");
        thread::sleep(Duration::from_millis(1));
    }
}
fn finish(a: &mut App) {
    wait(a, |a| {
        a.panels[a.active].archive().is_some() && !a.panels[a.active].loading
    });
}
fn key(a: &mut App, k: KeyCode) {
    input::handle(a, KeyEvent::new(k, KeyModifiers::NONE));
}
fn zip(path: &Path, deflate: bool) -> io::Result<()> {
    let mut w = zip::ZipWriter::new(fs::File::create(path)?);
    let options = zip::write::SimpleFileOptions::default().compression_method(if deflate {
        zip::CompressionMethod::Deflated
    } else {
        zip::CompressionMethod::Stored
    });
    for (name, data) in [
        ("folder/文.txt", b"ARCHIVE PREVIEW\n".as_slice()),
        ("root.bin", b"\0\xff\x1b"),
        ("empty", b""),
    ] {
        w.start_file(name, options)?;
        w.write_all(data)?;
    }
    w.add_directory("empty-dir/", options)?;
    w.add_symlink("link", "../../outside", options)?;
    w.start_file("../unsafe", options)?;
    w.write_all(b"never extracted")?;
    w.finish()?;
    Ok(())
}
fn tar(path: &Path, gzip: bool) -> io::Result<()> {
    let file = fs::File::create(path)?;
    let output: Box<dyn Write> = if gzip {
        Box::new(flate2::write::GzEncoder::new(
            file,
            flate2::Compression::fast(),
        ))
    } else {
        Box::new(file)
    };
    let mut w = tar::Builder::new(output);
    let mut header = tar::Header::new_gnu();
    header.set_size(16);
    header.set_mode(0o644);
    header.set_mtime(1700000000);
    header.set_cksum();
    w.append_data(
        &mut header,
        "folder/文.txt",
        b"ARCHIVE PREVIEW\n".as_slice(),
    )?;
    let mut link = tar::Header::new_gnu();
    link.set_size(0);
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_mode(0o777);
    link.set_link_name("../../outside")?;
    link.set_cksum();
    w.append_data(&mut link, "link", io::empty())?;
    w.finish()?;
    drop(w.into_inner()?);
    Ok(())
}
fn id(c: &reader::Catalog, path: &[u8]) -> usize {
    c.nodes.iter().position(|n| n.path == path).unwrap()
}
fn select(a: &mut App, name: &str) {
    let p = &mut a.panels[a.active];
    p.cursor = p.entries.iter().position(|e| e.name == name).unwrap();
    p.keep_visible_grid(p.viewport_rows, p.viewport_columns);
}
fn draw(a: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut term = Terminal::new(TestBackend::new(w, h))?;
    term.draw(|f| ui::draw(f, a))?;
    Ok(term
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
fn click(a: &mut App, target: Target) {
    let r = a
        .mouse
        .regions
        .iter()
        .rev()
        .find(|(_, t)| *t == target)
        .unwrap()
        .0;
    mouse::handle(
        a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}
#[test]
fn zip_stored_deflated_implicit_folders_links_and_binary_previews() -> io::Result<()> {
    let t = Temp::new()?;
    for d in [false, true] {
        let p = t.0.join(format!("sample-{d}.zip"));
        zip(&p, d)?;
        let c = reader::list(p, &Cancellation::default())?;
        assert_eq!(c.skipped, 1);
        assert_eq!(c.nodes[id(&c, b"folder")].kind, Kind::Directory);
        assert_eq!(c.nodes[id(&c, b"link")].kind, Kind::Link);
        let content = reader::preview(
            &c,
            id(&c, "folder/文.txt".as_bytes()),
            &Cancellation::default(),
        )?;
        assert!(!content.binary);
        assert_eq!(content.lines, ["ARCHIVE PREVIEW"]);
        assert!(reader::preview(&c, id(&c, b"root.bin"), &Cancellation::default())?.binary);
        assert!(reader::preview(&c, id(&c, b"link"), &Cancellation::default()).is_err());
        assert_eq!(
            reader::preview(&c, id(&c, b"empty"), &Cancellation::default())?.lines,
            [""]
        );
    }
    assert!(!t.0.join("unsafe").exists());
    Ok(())
}
#[test]
fn tar_and_gzip_navigation_links_metadata_and_preview() -> io::Result<()> {
    let t = Temp::new()?;
    for (name, gzip) in [
        ("test.tar", false),
        ("test.tar.gz", true),
        ("test.tgz", true),
    ] {
        let p = t.0.join(name);
        tar(&p, gzip)?;
        let c = reader::list(p, &Cancellation::default())?;
        let file = id(&c, "folder/文.txt".as_bytes());
        assert_eq!(c.nodes[file].size, 16);
        assert!(c.nodes[file].modified.as_ref().unwrap().contains("UTC"));
        assert_eq!(
            reader::preview(&c, file, &Cancellation::default())?.lines,
            ["ARCHIVE PREVIEW"]
        );
        assert!(reader::preview(&c, id(&c, b"link"), &Cancellation::default()).is_err());
    }
    Ok(())
}
#[test]
fn virtual_paths_reject_traversal_absolute_drives_and_excessive_depth() {
    for p in [
        b"../evil".as_slice(),
        b"a/../../evil",
        b"/tmp/evil",
        b"C:/evil",
        b"\\\\server\\evil",
        b"a\\..\\evil",
        b"null\0name",
    ] {
        assert!(reader::normalize(p).is_err(), "{p:?}");
    }
    assert_eq!(reader::normalize(b"./a//b/").unwrap(), b"a/b");
    assert!(reader::normalize("a/".repeat(65).as_bytes()).is_err());
    assert!(reader::normalize(&vec![b'a'; 4097]).is_err());
}
#[test]
fn malformed_missing_zip64_and_oversized_directory_are_errors() -> io::Result<()> {
    let t = Temp::new()?;
    for (name, bytes) in [
        ("bad.zip", b"not a zip".to_vec()),
        ("bad.tar", vec![1; 512]),
        ("bad.tgz", b"not gzip".to_vec()),
    ] {
        let p = t.0.join(name);
        fs::write(&p, bytes)?;
        assert!(reader::list(p, &Cancellation::default()).is_err());
    }
    assert!(reader::list(t.0.join("missing.zip"), &Cancellation::default()).is_err());
    for count in [10001u16, 65535] {
        let mut end = vec![0; 22];
        end[..4].copy_from_slice(b"PK\x05\x06");
        end[8..10].copy_from_slice(&count.to_le_bytes());
        end[10..12].copy_from_slice(&count.to_le_bytes());
        let p = t.0.join("huge.zip");
        fs::write(&p, end)?;
        assert!(reader::list(p, &Cancellation::default()).is_err());
    }
    Ok(())
}
#[test]
fn bounded_preview_changed_archive_and_cancellation() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("large.zip");
    let mut w = zip::ZipWriter::new(fs::File::create(&p)?);
    w.start_file(
        "large",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )?;
    w.write_all(&vec![b'x'; dn::viewer::MAX_BYTES + 100])?;
    w.finish()?;
    let c = reader::list(p.clone(), &Cancellation::default())?;
    assert!(reader::preview(&c, id(&c, b"large"), &Cancellation::default())?.truncated);
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        reader::list(p.clone(), &cancel).err().unwrap().kind(),
        io::ErrorKind::Interrupted
    );
    assert_eq!(
        reader::preview(&c, id(&c, b"large"), &cancel)
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::Interrupted
    );
    fs::write(p, b"replaced")?;
    assert!(
        reader::preview(&c, id(&c, b"large"), &Cancellation::default())
            .err()
            .unwrap()
            .to_string()
            .contains("changed")
    );
    Ok(())
}
#[test]
fn browser_enter_f3_return_parent_and_panel_state_preservation() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("test.zip");
    zip(&p, true)?;
    let mut a = t.app()?;
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.path == p)
        .unwrap();
    a.panels[0].select_visible();
    let marks = a.panels[0].selected.clone();
    let cursor = a.panels[0].cursor;
    key(&mut a, KeyCode::Enter);
    finish(&mut a);
    assert_eq!(focus::current(&a), Focus::Manager);
    select(&mut a, "folder");
    key(&mut a, KeyCode::Enter);
    select(&mut a, "文.txt");
    key(&mut a, KeyCode::F(3));
    wait(&mut a, |a| a.desktop.active_viewer().is_some());
    assert!(!a.desktop.identity_pending(a.desktop.active().id));
    assert_eq!(
        a.desktop
            .active_viewer()
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .lines,
        ["ARCHIVE PREVIEW"]
    );
    assert!(a
        .file_tools
        .histories
        .get(dn::file_tools::history::HistoryKind::View)
        .is_empty());
    key(&mut a, KeyCode::F(2));
    assert!(a.desktop.active_viewer().unwrap().wrap);
    key(&mut a, KeyCode::Esc);
    assert_eq!(focus::current(&a), Focus::Manager);
    key(&mut a, KeyCode::Backspace);
    assert_eq!(a.panels[a.active].archive().unwrap().directory, 0);
    key(&mut a, KeyCode::Backspace);
    wait(&mut a, |a| !a.panels[0].loading);
    assert!(!a.panels[a.active].is_archive());
    assert_eq!(a.panels[0].selected, marks);
    assert_eq!(a.panels[0].cursor, cursor);
    assert_eq!(a.panels[1].path, t.0);
    Ok(())
}
#[test]
fn pane_mouse_navigation_resizes_and_immutable_redraws() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("test.zip");
    zip(&path, false)?;
    let mut a = t.app()?;
    archive::open(&mut a, path);
    finish(&mut a);
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        a.config.language = language;
        for palette in [
            dn::palette::PaletteProfile::ClassicDn,
            dn::palette::PaletteProfile::Custom(dn::palette::theme::DEFAULTS.map(|v| v ^ 1)),
        ] {
            a.config.palette = palette;
            for (w, h) in [(80, 25), (132, 40), (30, 10), (80, 25)] {
                let first = draw(&mut a, w, h)?;
                assert_eq!(first, draw(&mut a, w, h)?);
                assert_eq!(first, draw(&mut a, w, h)?);
                assert_eq!(focus::current(&a), Focus::Manager);
            }
        }
    }
    select(&mut a, "folder");
    draw(&mut a, 80, 25)?;
    let row = a.panels[0].cursor;
    click(&mut a, Target::Entry(0, row));
    click(&mut a, Target::Entry(0, row));
    assert_ne!(a.panels[0].archive().unwrap().directory, 0);
    assert!(a.panels[0].targets().is_empty());
    Ok(())
}
#[test]
fn cancelled_index_and_preview_cannot_publish_after_leaving() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("test.zip");
    zip(&path, true)?;
    let mut a = t.app()?;
    archive::open(&mut a, path.clone());
    key(&mut a, KeyCode::Esc);
    wait(&mut a, |a| !a.panels[0].loading);
    assert!(!a.panels[0].is_archive());
    archive::open(&mut a, path);
    finish(&mut a);
    select(&mut a, "root.bin");
    key(&mut a, KeyCode::F(3));
    a.panels[0].leave_archive(false);
    wait(&mut a, |a| !a.panels[0].loading);
    assert!(a.desktop.active_viewer().is_none());
    archive::open(&mut a, t.0.join("missing.zip"));
    wait(&mut a, |a| {
        !a.panels[0].is_archive() && !a.panels[0].loading
    });
    assert!(!a.dialogs().is_empty());
    Ok(())
}
#[cfg(unix)]
#[test]
fn source_symlinks_special_files_and_permission_errors_are_not_followed() -> io::Result<()> {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let t = Temp::new()?;
    let p = t.0.join("test.tar");
    tar(&p, false)?;
    symlink(&p, t.0.join("link.tar"))?;
    assert!(reader::list(t.0.join("link.tar"), &Cancellation::default()).is_err());
    fs::set_permissions(&p, fs::Permissions::from_mode(0o000))?;
    let denied = fs::File::open(&p).is_err();
    let result = reader::list(p.clone(), &Cancellation::default());
    fs::set_permissions(&p, fs::Permissions::from_mode(0o600))?;
    if denied {
        assert!(result.is_err());
    }
    // Keep the opener occupied so routing can be checked without launching an
    // actual desktop application during automated tests.
    let mut app = t.app()?;
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "link.tar")
        .unwrap();
    app.launcher
        .request_with(0, p.clone(), |_| Err(io::Error::other("test opener")))?;
    app.open_current();
    assert!(!app.panels[app.active].is_archive());
    assert!(!app.dialogs().is_empty());
    app.clear_dialogs();
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.path == p)
        .unwrap();
    input::handle(
        &mut app,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
    );
    assert!(!app.panels[app.active].is_archive());
    assert!(!app.dialogs().is_empty());
    Ok(())
}

#[test]
fn encrypted_entries_list_without_reading_and_crc_failures_are_reported() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("one.zip");
    let mut w = zip::ZipWriter::new(fs::File::create(&path)?);
    w.start_file(
        "file",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )?;
    w.write_all(b"CHECKSUM PAYLOAD")?;
    w.finish()?;
    let original = fs::read(&path)?;
    let central = original
        .windows(4)
        .position(|b| b == b"PK\x01\x02")
        .unwrap();
    let mut encrypted = original.clone();
    encrypted[6] |= 1;
    encrypted[central + 8] |= 1;
    fs::write(&path, encrypted)?;
    let c = reader::list(path.clone(), &Cancellation::default())?;
    let i = id(&c, b"file");
    assert!(c.nodes[i].locked);
    assert!(reader::preview(&c, i, &Cancellation::default()).is_err());
    let mut damaged = original;
    let position = damaged
        .windows(16)
        .position(|b| b == b"CHECKSUM PAYLOAD")
        .unwrap();
    damaged[position] = b'X';
    fs::write(&path, damaged)?;
    let c = reader::list(path, &Cancellation::default())?;
    assert!(reader::preview(&c, id(&c, b"file"), &Cancellation::default()).is_err());
    Ok(())
}
#[test]
fn tar_duplicate_names_conflicting_parents_and_truncated_members() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("duplicates.tar");
    let mut builder = tar::Builder::new(fs::File::create(&p)?);
    for name in ["file", "file", "file/child"] {
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, b"x".as_slice())?;
    }
    builder.finish()?;
    let c = reader::list(p, &Cancellation::default())?;
    assert_eq!(c.skipped, 2);
    assert_eq!(c.nodes.len(), 2);
    let p = t.0.join("short.tar");
    let mut header = tar::Header::new_gnu();
    header.set_path("huge")?;
    header.set_size(u64::MAX / 2);
    header.set_mode(0o644);
    header.set_cksum();
    fs::write(&p, header.as_bytes())?;
    let c = reader::list(p, &Cancellation::default())?;
    assert!(c.note.as_ref().unwrap().contains("Partial"));
    assert!(reader::preview(&c, id(&c, b"huge"), &Cancellation::default()).is_err());
    Ok(())
}

#[test]
fn duplicate_zip_directory_records_are_rejected_before_ambiguous_preview() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("duplicate.zip");
    let mut w = zip::ZipWriter::new(fs::File::create(&p)?);
    w.start_file("file", zip::write::SimpleFileOptions::default())?;
    w.write_all(b"x")?;
    w.finish()?;
    let original = fs::read(&p)?;
    let start = original
        .windows(4)
        .position(|b| b == b"PK\x01\x02")
        .unwrap();
    let end = original
        .windows(4)
        .rposition(|b| b == b"PK\x05\x06")
        .unwrap();
    let mut bytes = original[..end].to_vec();
    bytes.extend_from_slice(&original[start..end]);
    let mut footer = original[end..].to_vec();
    footer[8..10].copy_from_slice(&2u16.to_le_bytes());
    footer[10..12].copy_from_slice(&2u16.to_le_bytes());
    footer[12..16].copy_from_slice(&((end - start) as u32 * 2).to_le_bytes());
    bytes.extend(footer);
    fs::write(&p, bytes)?;
    assert!(reader::list(p, &Cancellation::default())
        .err()
        .unwrap()
        .to_string()
        .contains("Duplicate"));
    Ok(())
}

fn clean_zip(path: &Path, large: bool) -> io::Result<()> {
    let mut w = zip::ZipWriter::new(fs::File::create(path)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (name, data) in [
        ("docs/a.txt", b"alpha".as_slice()),
        ("docs/b.txt", b"beta"),
        ("root.txt", b"root"),
    ] {
        w.start_file(name, options)?;
        w.write_all(data)?;
    }
    w.add_directory("empty/", options)?;
    if large {
        w.start_file("large.bin", options)?;
        w.write_all(&vec![b'x'; dn::viewer::MAX_BYTES + 100])?;
    }
    w.finish()?;
    Ok(())
}
fn extract(a: &mut App, destination: &Path, skip: bool) {
    a.prompt_operation(dn::operations::Kind::Copy, false);
    let p = a.modals.prompt_mut().expect("copy draft");
    assert!(
        p.sources.is_empty(),
        "virtual members must never enter native plans"
    );
    let draft = p.copy.as_mut().unwrap();
    assert!(!draft.options.remove_sources);
    draft.picked = Some(destination.to_path_buf());
    draft.options.existing = if skip {
        dn::operations::ExistingDestination::Skip
    } else {
        dn::operations::ExistingDestination::Refuse
    };
    a.submit_prompt();
    wait(a, |a| !a.transfers.has_work());
    key(a, KeyCode::Esc);
}
#[test]
fn selected_extraction_keeps_relative_hierarchy_and_streams_large_files() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("clean.zip");
    clean_zip(&path, true)?;
    let out = t.0.join("out");
    fs::create_dir(&out)?;
    let mut a = t.app()?;
    archive::open(&mut a, path.clone());
    finish(&mut a);
    for name in ["docs", "empty", "large.bin"] {
        select(&mut a, name);
        a.panels[0].toggle_selection();
    }
    extract(&mut a, &out, false);
    assert_eq!(
        a.transfers.entries.back().unwrap().status,
        dn::transfers::Status::Completed,
        "{:?}",
        a.transfers.entries.back().unwrap().error
    );
    assert_eq!(fs::read(out.join("docs/a.txt"))?, b"alpha");
    assert_eq!(fs::read(out.join("docs/b.txt"))?, b"beta");
    assert!(out.join("empty").is_dir());
    assert!(!out.join("root.txt").exists());
    assert_eq!(
        fs::metadata(out.join("large.bin"))?.len(),
        (dn::viewer::MAX_BYTES + 100) as u64
    );
    assert!(path.is_file());
    a.panels[0].clear_selection();
    select(&mut a, "docs");
    key(&mut a, KeyCode::Enter);
    select(&mut a, "a.txt");
    let out2 = t.0.join("out2");
    fs::create_dir(&out2)?;
    extract(&mut a, &out2, false);
    assert_eq!(fs::read(out2.join("a.txt"))?, b"alpha");
    assert!(!out2.join("docs").exists());
    assert!(!fs::read_dir(&out)?.any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".dn-")));
    Ok(())
}
#[test]
fn two_archive_panes_require_native_destination_and_unsupported_commands_are_guarded(
) -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("clean.zip");
    clean_zip(&path, false)?;
    let mut a = t.app()?;
    archive::open(&mut a, path.clone());
    finish(&mut a);
    select(&mut a, "root.txt");
    a.active = 1;
    archive::open(&mut a, path);
    finish(&mut a);
    select(&mut a, "root.txt");
    a.prompt_operation(dn::operations::Kind::Copy, false);
    assert!(
        a.modals
            .prompt()
            .unwrap()
            .copy
            .as_ref()
            .unwrap()
            .require_destination
    );
    a.submit_prompt();
    assert!(!a.transfers.has_work());
    assert!(!a.dialogs().is_empty());
    a.clear_dialogs();
    for kind in [dn::operations::Kind::Move, dn::operations::Kind::Mkdir] {
        a.prompt_operation(kind, false);
        assert!(a.modals.prompt().is_none());
        assert!(!a.dialogs().is_empty());
        a.clear_dialogs();
    }
    dn::menu::execute(&mut a, dn::menu::Action::CreateArchive);
    assert!(a.modals.archive_dialog().is_none());
    let out = t.0.join("out");
    fs::create_dir(&out)?;
    extract(&mut a, &out, false);
    assert_eq!(fs::read(out.join("root.txt"))?, b"root");
    assert!(a.panels.iter().all(|p| p.is_archive()));
    Ok(())
}
#[test]
fn extraction_refuses_conflicts_skips_whole_folders_and_cancel_is_draft_only() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("clean.zip");
    clean_zip(&path, false)?;
    let out = t.0.join("out");
    fs::create_dir_all(out.join("docs"))?;
    fs::write(out.join("docs/a.txt"), b"keep")?;
    let mut a = t.app()?;
    archive::open(&mut a, path);
    finish(&mut a);
    select(&mut a, "docs");
    a.prompt_operation(dn::operations::Kind::Copy, false);
    key(&mut a, KeyCode::Esc);
    assert!(!a.transfers.has_work());
    extract(&mut a, &out, false);
    assert_eq!(
        a.transfers.entries.back().unwrap().status,
        dn::transfers::Status::Failed
    );
    assert!(!out.join("docs/b.txt").exists());
    assert_eq!(fs::read(out.join("docs/a.txt"))?, b"keep");
    a.transfers.paused = false;
    extract(&mut a, &out, true);
    assert_eq!(
        a.transfers.entries.back().unwrap().status,
        dn::transfers::Status::Completed
    );
    assert!(!out.join("docs/b.txt").exists());
    assert_eq!(fs::read(out.join("docs/a.txt"))?, b"keep");
    Ok(())
}
#[test]
fn corrupt_selected_payload_is_not_published() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("broken.zip");
    let mut w = zip::ZipWriter::new(fs::File::create(&path)?);
    w.start_file(
        "file",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )?;
    w.write_all(b"CHECKSUM PAYLOAD")?;
    w.finish()?;
    let mut bytes = fs::read(&path)?;
    let pos = bytes
        .windows(16)
        .position(|b| b == b"CHECKSUM PAYLOAD")
        .unwrap();
    bytes[pos] = b'X';
    fs::write(&path, bytes)?;
    let out = t.0.join("out");
    fs::create_dir(&out)?;
    let mut a = t.app()?;
    archive::open(&mut a, path);
    finish(&mut a);
    select(&mut a, "file");
    extract(&mut a, &out, false);
    assert_eq!(
        a.transfers.entries.back().unwrap().status,
        dn::transfers::Status::Failed
    );
    assert!(fs::read_dir(&out)?.next().is_none());
    Ok(())
}
#[test]
fn unsafe_catalog_links_and_portable_name_collisions_never_extract() -> io::Result<()> {
    let t = Temp::new()?;
    let out = t.0.join("out");
    fs::create_dir(&out)?;
    for (index, names) in [
        vec!["../escape", "ok"],
        vec!["CON.txt"],
        vec!["A", "a"],
        vec!["dir./file"],
    ]
    .iter()
    .enumerate()
    {
        let path = t.0.join(format!("bad{index}.zip"));
        let mut w = zip::ZipWriter::new(fs::File::create(&path)?);
        for name in names {
            w.start_file(*name, zip::write::SimpleFileOptions::default())?;
            w.write_all(b"never")?;
        }
        w.finish()?;
        let mut a = t.app()?;
        archive::open(&mut a, path);
        finish(&mut a);
        a.panels[0].select_visible();
        a.prompt_operation(dn::operations::Kind::Copy, false);
        a.modals.prompt_mut().unwrap().copy.as_mut().unwrap().picked = Some(out.clone());
        a.submit_prompt();
        wait(&mut a, |a| !a.transfers.has_work());
        assert!(a
            .transfers
            .entries
            .back()
            .is_none_or(|e| e.status == dn::transfers::Status::Failed));
        assert!(fs::read_dir(&out)?.next().is_none());
    }
    Ok(())
}

#[test]
fn tar_gzip_extract_selected_tree_and_empty_folders() -> io::Result<()> {
    let t = Temp::new()?;
    for (name, gzip) in [
        ("tree.tar", false),
        ("tree.tar.gz", true),
        ("tree.tgz", true),
    ] {
        let path = t.0.join(name);
        tar(&path, gzip)?;
        let out = t.0.join(format!("out-{name}"));
        fs::create_dir(&out)?;
        let mut a = t.app()?;
        archive::open(&mut a, path);
        finish(&mut a);
        select(&mut a, "folder");
        extract(&mut a, &out, false);
        assert_eq!(
            a.transfers.entries.back().unwrap().status,
            dn::transfers::Status::Completed,
            "{:?}",
            a.transfers.entries.back().unwrap().error
        );
        assert_eq!(fs::read(out.join("folder/文.txt"))?, b"ARCHIVE PREVIEW\n");
        assert!(!out.join("link").exists());
    }
    Ok(())
}
#[test]
fn archive_marks_are_member_ids_and_mouse_clicks_do_not_alias() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("clean.zip");
    clean_zip(&path, false)?;
    let mut a = t.app()?;
    archive::open(&mut a, path);
    finish(&mut a);
    select(&mut a, "docs");
    a.panels[0].toggle_selection();
    assert!(
        a.panels[0].selected.is_empty(),
        "native name marks are not archive identities"
    );
    let selection = a.panels[0].extraction().unwrap();
    assert_eq!(selection.members.len(), 1);
    assert_eq!(
        selection.members[0].generation,
        selection.catalog.generation
    );
    let docrow = a.panels[0].cursor;
    select(&mut a, "empty");
    let emptyrow = a.panels[0].cursor;
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Entry(0, docrow));
    click(&mut a, Target::Entry(0, emptyrow));
    assert_eq!(
        a.panels[0].archive().unwrap().directory,
        0,
        "different members cannot double-click"
    );
    dn::pattern::apply(&mut a.panels[0], "*", true);
    assert_eq!(a.panels[0].extraction().unwrap().members.len(), 3);
    a.panels[0].set_filter("*.txt".into());
    assert_eq!(a.panels[0].filtered_marks(), 2);
    assert_eq!(a.panels[0].extraction().unwrap().members.len(), 1);
    let old = selection.members[0].generation;
    a.panels[0].refresh(false);
    finish(&mut a);
    assert_ne!(a.panels[0].archive().unwrap().generation, old);
    assert!(!a.panels[0].has_marks());
    Ok(())
}
#[test]
fn inactive_preview_and_queued_extraction_do_not_retarget_panes() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("clean.zip");
    clean_zip(&path, false)?;
    let mut a = t.app()?;
    archive::open(&mut a, path);
    finish(&mut a);
    select(&mut a, "root.txt");
    key(&mut a, KeyCode::F(3));
    a.active = 1;
    wait(&mut a, |a| {
        a.panels[0].archive().is_some_and(|p| !p.preview_pending())
    });
    assert!(a.desktop.active_viewer().is_none());
    a.active = 0;
    a.transfers.paused = true;
    a.prompt_operation(dn::operations::Kind::Copy, false);
    let out = t.0.join("out");
    fs::create_dir(&out)?;
    a.modals.prompt_mut().unwrap().copy.as_mut().unwrap().picked = Some(out.clone());
    a.submit_prompt();
    assert!(a.transfers.has_work());
    a.panels[0].leave_archive(false);
    wait(&mut a, |a| !a.panels[0].loading);
    a.transfers.paused = false;
    wait(&mut a, |a| !a.transfers.has_work());
    assert_eq!(fs::read(out.join("root.txt"))?, b"root");
    assert!(!a.panels[0].is_archive());
    Ok(())
}
#[cfg(unix)]
#[test]
fn extraction_rejects_redirected_destinations_and_replaced_archive() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let t = Temp::new()?;
    let path = t.0.join("clean.zip");
    clean_zip(&path, false)?;
    let out = t.0.join("out");
    fs::create_dir(&out)?;
    let link = t.0.join("link");
    symlink(&out, &link)?;
    let mut a = t.app()?;
    archive::open(&mut a, path.clone());
    finish(&mut a);
    select(&mut a, "root.txt");
    a.prompt_operation(dn::operations::Kind::Copy, false);
    a.modals.prompt_mut().unwrap().copy.as_mut().unwrap().picked = Some(link);
    a.submit_prompt();
    assert!(!a.transfers.has_work());
    a.clear_dialogs();
    assert!(fs::read_dir(&out)?.next().is_none());
    a.transfers.paused = true;
    a.prompt_operation(dn::operations::Kind::Copy, false);
    a.modals.prompt_mut().unwrap().copy.as_mut().unwrap().picked = Some(out.clone());
    a.submit_prompt();
    fs::rename(&path, t.0.join("old.zip"))?;
    clean_zip(&path, false)?;
    a.transfers.paused = false;
    wait(&mut a, |a| !a.transfers.has_work());
    assert_eq!(
        a.transfers.entries.back().unwrap().status,
        dn::transfers::Status::Failed
    );
    assert!(fs::read_dir(&out)?.next().is_none());
    Ok(())
}

#[test]
fn copy_to_temp_drive_captures_archive_members_and_cancel_does_not_write() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("clean.zip");
    clean_zip(&path, false)?;
    let mut a = t.app()?;
    archive::open(&mut a, path);
    finish(&mut a);
    select(&mut a, "root.txt");
    dn::menu::execute(&mut a, dn::menu::Action::CopyToTempDrive);
    let root = t.0.join("temp-drive");
    assert_eq!(
        a.modals
            .prompt()
            .unwrap()
            .copy
            .as_ref()
            .unwrap()
            .picked
            .as_ref(),
        Some(&root)
    );
    key(&mut a, KeyCode::Esc);
    assert!(fs::read_dir(&root)?.next().is_none());
    dn::menu::execute(&mut a, dn::menu::Action::CopyToTempDrive);
    // Changing the pane after the draft was captured cannot retarget its member IDs.
    select(&mut a, "docs");
    a.submit_prompt();
    wait(&mut a, |a| !a.transfers.has_work());
    assert_eq!(fs::read(root.join("root.txt"))?, b"root");
    assert!(!root.join("docs").exists());
    Ok(())
}

#[test]
fn selected_link_is_rejected_and_native_sources_can_choose_disk_when_other_pane_is_archive(
) -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("source.tar");
    tar(&path, false)?;
    let mut a = t.app()?;
    archive::open(&mut a, path);
    finish(&mut a);
    select(&mut a, "link");
    a.prompt_operation(dn::operations::Kind::Copy, false);
    a.submit_prompt();
    assert!(!a.transfers.has_work());
    assert!(!a.dialogs().is_empty());
    a.clear_dialogs();
    a.active = 1;
    fs::write(t.0.join("native.txt"), b"native")?;
    a.panels[1].refresh(false);
    wait(&mut a, |a| !a.panels[1].loading);
    a.panels[1].cursor = a.panels[1]
        .entries
        .iter()
        .position(|e| e.name == "native.txt")
        .unwrap();
    dn::menu::execute(&mut a, dn::menu::Action::CopyToTempDrive);
    assert!(a
        .modals
        .prompt()
        .unwrap()
        .copy
        .as_ref()
        .unwrap()
        .picked
        .is_some());
    a.submit_prompt();
    wait(&mut a, |a| !a.transfers.has_work());
    assert_eq!(fs::read(t.0.join("temp-drive/native.txt"))?, b"native");
    assert!(a.panels[0].is_archive());
    Ok(())
}

#[test]
fn leaving_archive_restores_outer_filter_marks_scroll_and_highlights_archive() -> io::Result<()> {
    let t = Temp::new()?;
    for n in 0..80 {
        fs::write(t.0.join(format!("a-{n:03}.txt")), b"fixture")?;
    }
    let path = t.0.join("a.zip");
    clean_zip(&path, false)?;
    let mut a = t.app()?;
    a.panels[0].set_filter("a*".into());
    a.panels[0]
        .selected
        .extend(["a-050.txt".into(), "a.zip".into()]);
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.path == path)
        .unwrap();
    draw(&mut a, 80, 25)?;
    let offset = a.panels[0].offset;
    key(&mut a, KeyCode::Enter);
    finish(&mut a);
    a.panels[0].set_filter("*.txt".into());
    fs::remove_file(t.0.join("a-050.txt"))?;
    input::handle(
        &mut a,
        KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL),
    );
    wait(&mut a, |a| !a.panels[0].loading);
    assert!(!a.panels[0].is_archive());
    assert_eq!(a.panels[0].filter, "a*");
    assert_eq!(a.panels[0].entries[a.panels[0].cursor].path, path);
    assert_eq!(a.panels[0].selected.len(), 1);
    assert!(a.panels[0].selected.contains(std::ffi::OsStr::new("a.zip")));
    assert_eq!(a.panels[0].offset, offset);
    assert_eq!(a.panels[1].path, t.0);
    Ok(())
}

#[test]
fn explicit_browse_activates_manager_without_closing_other_documents() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("clean.zip");
    clean_zip(&path, false)?;
    let mut a = t.app()?;
    a.show_viewer(dn::viewer::Viewer::from_contents(
        t.0.join("memory.txt"),
        dn::viewer::from_bytes(b"kept".to_vec(), dn::viewer::ViewMode::Text),
    ));
    let viewer = a.desktop.active().id;
    archive::open(&mut a, path);
    finish(&mut a);
    assert_eq!(a.desktop.active().id, dn::desktop::WindowId::MANAGER);
    assert!(a.desktop.viewer(viewer).is_some());
    assert_eq!(a.panels[1].path, t.0);
    Ok(())
}

#[test]
fn reindex_blocks_old_catalog_navigation_and_inactive_failures_do_not_take_focus() -> io::Result<()>
{
    let t = Temp::new()?;
    let path = t.0.join("clean.zip");
    clean_zip(&path, false)?;
    let mut a = t.app()?;
    archive::open(&mut a, path);
    finish(&mut a);
    select(&mut a, "docs");
    a.panels[0].refresh(false);
    a.panels[0].enter(false);
    assert!(a.panels[0].loading);
    assert_eq!(a.panels[0].archive().unwrap().directory, 0);
    finish(&mut a);
    select(&mut a, "docs");
    a.panels[0].enter(false);
    let directory = a.panels[0].archive().unwrap().directory;
    a.panels[0].refresh(false);
    a.panels[0].parent(false);
    assert_eq!(a.panels[0].archive().unwrap().directory, directory);
    finish(&mut a);
    a.panels[0].leave_archive(false);
    wait(&mut a, |a| !a.panels[0].loading);
    archive::open(&mut a, t.0.join("missing.zip"));
    a.active = 1;
    wait(&mut a, |a| {
        !a.panels[0].is_archive() && !a.panels[0].loading
    });
    assert_eq!(a.active, 1);
    assert_eq!(focus::current(&a), Focus::Manager);
    assert!(a.dialogs().is_empty());
    assert!(!a.notice.is_empty());
    Ok(())
}

#[test]
fn changing_current_member_retires_the_pending_preview_focus_request() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("clean.zip");
    clean_zip(&path, false)?;
    let mut a = t.app()?;
    archive::open(&mut a, path);
    finish(&mut a);
    select(&mut a, "root.txt");
    key(&mut a, KeyCode::F(3));
    select(&mut a, "docs");
    wait(&mut a, |a| {
        !a.panels[0].archive().unwrap().preview_pending()
    });
    assert_eq!(focus::current(&a), Focus::Manager);
    assert!(a.desktop.active_viewer().is_none());
    Ok(())
}

#[test]
fn archive_escape_clears_command_drafts_and_preview_never_interrupts_typing() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("clean.zip");
    clean_zip(&path, false)?;
    let mut a = t.app()?;
    archive::open(&mut a, path);
    finish(&mut a);
    select(&mut a, "root.txt");
    key(&mut a, KeyCode::F(3));
    dn::command_line::paste(&mut a, "must not run");
    wait(&mut a, |a| {
        !a.panels[0].archive().unwrap().preview_pending()
    });
    assert!(a.desktop.active_viewer().is_none());
    assert_eq!(a.command_line.text, "must not run");
    key(&mut a, KeyCode::Esc);
    assert!(a.command_line.text.is_empty());
    assert!(a.command_line.pending.is_none());
    assert!(a.panels[0].is_archive());
    key(&mut a, KeyCode::F(3));
    dn::command_line::paste(&mut a, "another draft");
    key(&mut a, KeyCode::Esc);
    assert!(!a.panels[0].archive().unwrap().preview_pending());
    assert!(a.command_line.text.is_empty());
    assert!(a.command_line.pending.is_none());
    assert!(a.desktop.active_viewer().is_none());
    Ok(())
}

fn protected_zip(path: &Path) -> io::Result<()> {
    let mut writer = zip::ZipWriter::new(fs::File::create(path)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .with_aes_encryption(zip::AesMode::Aes256, "secret 界");
    writer.start_file("private.txt", options)?;
    writer.write_all(b"AUTHENTICATED PRIVATE PREVIEW\n")?;
    writer.start_file("second.txt", options)?;
    writer.write_all(b"second member\n")?;
    writer.finish()?;
    Ok(())
}
#[test]
fn archive_creation_dialog_masks_passwords_validates_confirmation_and_queues_real_zip(
) -> io::Result<()> {
    use archive::dialogs::{self, Action};
    let t = Temp::new()?;
    fs::write(t.0.join("source.txt"), b"original contents")?;
    let mut a = t.app()?;
    select(&mut a, "source.txt");
    dialogs::create(&mut a);
    assert_eq!(focus::current(&a), Focus::ArchiveDialog);
    key(&mut a, KeyCode::Tab);
    input::paste(&mut a, "secret 界");
    key(&mut a, KeyCode::Tab);
    input::paste(&mut a, "different");
    key(&mut a, KeyCode::Enter);
    assert!(draw(&mut a, 80, 25)?.contains("Passwords do not match"));
    key(&mut a, KeyCode::Home);
    for _ in 0..9 {
        key(&mut a, KeyCode::Delete);
    }
    input::paste(&mut a, "secret 界");
    for lang in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        a.config.language = lang;
        for palette in [
            dn::palette::PaletteProfile::ClassicDn,
            dn::palette::PaletteProfile::Custom([0x3f; dn::palette::theme::COUNT]),
        ] {
            a.config.palette = palette;
            for (w, h) in [(80, 25), (132, 40), (50, 15), (80, 25)] {
                let before = draw(&mut a, w, h)?;
                assert!(!before.contains("secret") && !before.contains("different"));
                let hits = a.mouse.regions.clone();
                for _ in 0..3 {
                    assert_eq!(before, draw(&mut a, w, h)?);
                    assert_eq!(hits, a.mouse.regions);
                }
            }
        }
    }
    a.config.language = dn::localization::Language::English;
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Archive(Action::Confirm));
    wait(&mut a, |a| !a.transfers.has_work());
    assert!(a.modals.archive_dialog().is_none());
    assert!(a.transfers.entries.back().unwrap().packing);
    assert_eq!(
        a.transfers.entries.back().unwrap().status,
        dn::transfers::Status::Completed,
        "{:?}",
        a.transfers.entries.back().unwrap().error
    );
    let catalog = reader::list(t.0.join("source.txt.zip"), &Cancellation::default())?;
    assert!(catalog.nodes[1].locked);
    assert_eq!(fs::read(t.0.join("source.txt"))?, b"original contents");
    assert!(a.copy_dialog.history.is_empty());
    Ok(())
}
#[test]
fn encrypted_preview_retries_cancels_caches_per_pane_and_refresh_forgets() -> io::Result<()> {
    use archive::dialogs::{self, Action};
    let t = Temp::new()?;
    protected_zip(&t.0.join("protected.zip"))?;
    let mut a = t.app()?;
    archive::open(&mut a, t.0.join("protected.zip"));
    finish(&mut a);
    select(&mut a, "private.txt");
    key(&mut a, KeyCode::F(3));
    assert_eq!(focus::current(&a), Focus::ArchiveDialog);
    input::paste(&mut a, "wrong");
    key(&mut a, KeyCode::Enter);
    wait(&mut a, |a| {
        a.modals.archive_dialog().is_some_and(|d| !d.loading())
    });
    assert!(draw(&mut a, 80, 25)?.contains("Incorrect archive password"));
    assert!(a.desktop.active_viewer().is_none());
    input::paste(&mut a, "secret 界");
    key(&mut a, KeyCode::Enter);
    wait(&mut a, |a| a.desktop.active_viewer().is_some());
    assert!(draw(&mut a, 80, 25)?.contains("AUTHENTICATED PRIVATE PREVIEW"));
    key(&mut a, KeyCode::Esc);
    select(&mut a, "second.txt");
    key(&mut a, KeyCode::F(3));
    assert!(a.modals.archive_dialog().is_none());
    wait(&mut a, |a| a.desktop.active_viewer().is_some());
    key(&mut a, KeyCode::Esc);
    a.active = 1;
    archive::open(&mut a, t.0.join("protected.zip"));
    finish(&mut a);
    select(&mut a, "private.txt");
    key(&mut a, KeyCode::F(3));
    assert_eq!(focus::current(&a), Focus::ArchiveDialog);
    dialogs::action(&mut a, Action::Cancel);
    assert!(a.panels[1].is_archive());
    assert!(a.desktop.active_viewer().is_none());
    a.active = 0;
    input::handle(
        &mut a,
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
    );
    finish(&mut a);
    select(&mut a, "private.txt");
    key(&mut a, KeyCode::F(3));
    assert_eq!(focus::current(&a), Focus::ArchiveDialog);
    key(&mut a, KeyCode::Esc);
    assert!(a.panels[0].is_archive());
    Ok(())
}
#[test]
fn encrypted_multiselect_extraction_survives_leaving_pane_and_retires_credentials() -> io::Result<()>
{
    let t = Temp::new()?;
    protected_zip(&t.0.join("protected.zip"))?;
    fs::create_dir(t.0.join("out"))?;
    let mut a = t.app()?;
    archive::open(&mut a, t.0.join("protected.zip"));
    finish(&mut a);
    for name in ["private.txt", "second.txt"] {
        select(&mut a, name);
        a.panels[0].toggle_selection();
    }
    a.prompt_operation(dn::operations::Kind::Copy, false);
    a.modals.prompt_mut().unwrap().copy.as_mut().unwrap().picked = Some(t.0.join("out"));
    a.submit_prompt();
    assert_eq!(focus::current(&a), Focus::ArchiveDialog);
    a.transfers.paused = true;
    input::paste(&mut a, "secret 界");
    key(&mut a, KeyCode::Enter);
    wait(&mut a, |a| a.modals.archive_dialog().is_none());
    assert!(a.transfers.has_work());
    a.panels[0].leave_archive(false);
    a.transfers.paused = false;
    wait(&mut a, |a| !a.transfers.has_work());
    assert_eq!(
        a.transfers.entries.back().unwrap().status,
        dn::transfers::Status::Completed
    );
    assert!(matches!(
        a.transfers.entries.back().unwrap().request,
        dn::transfers::Request::Filesystem
    ));
    assert_eq!(
        fs::read(t.0.join("out/private.txt"))?,
        b"AUTHENTICATED PRIVATE PREVIEW\n"
    );
    assert_eq!(fs::read(t.0.join("out/second.txt"))?, b"second member\n");
    assert_eq!(a.copy_dialog.history, [t.0.join("out")]);
    Ok(())
}
#[test]
fn cancelled_or_stale_password_reply_never_opens_a_viewer_or_queues_work() -> io::Result<()> {
    let t = Temp::new()?;
    protected_zip(&t.0.join("protected.zip"))?;
    let mut a = t.app()?;
    archive::open(&mut a, t.0.join("protected.zip"));
    finish(&mut a);
    select(&mut a, "private.txt");
    key(&mut a, KeyCode::F(3));
    input::paste(&mut a, "secret 界");
    key(&mut a, KeyCode::Enter);
    key(&mut a, KeyCode::Esc);
    for _ in 0..4 {
        a.poll();
    }
    assert!(a.desktop.active_viewer().is_none());
    key(&mut a, KeyCode::F(3));
    input::paste(&mut a, "secret 界");
    key(&mut a, KeyCode::Enter);
    a.panels[0].leave_archive(false);
    wait(&mut a, |a| a.modals.archive_dialog().is_none());
    assert!(a.desktop.active_viewer().is_none());
    assert!(!a.transfers.has_work());
    Ok(())
}

#[test]
fn legacy_zipcrypto_fixture_authenticates_without_an_external_archiver() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("legacy.zip");
    // Original synthetic fixture generated independently with Info-ZIP.
    // Keep bytes in source so no generated ZIP needs to be tracked.
    const LEGACY_ZIP: &[u8] = &[
        0x50, 0x4b, 0x03, 0x04, 0x0a, 0x00, 0x09, 0x00, 0x00, 0x00, 0xc7, 0x80, 0x64, 0x5d, 0x9a,
        0x5d, 0x51, 0x9c, 0x38, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x1c, 0x00,
        0x6c, 0x65, 0x67, 0x61, 0x63, 0x79, 0x2e, 0x74, 0x78, 0x74, 0x55, 0x54, 0x09, 0x00, 0x03,
        0xe6, 0x49, 0xeb, 0x6a, 0xe6, 0x49, 0xeb, 0x6a, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5,
        0x01, 0x00, 0x00, 0x04, 0x14, 0x00, 0x00, 0x00, 0xff, 0x3a, 0x84, 0x80, 0xb5, 0xdd, 0xce,
        0x49, 0x50, 0x1f, 0x08, 0x13, 0xf4, 0x0a, 0x62, 0x04, 0x52, 0x81, 0x09, 0xfc, 0x23, 0x4f,
        0x4f, 0xba, 0x38, 0x45, 0xcb, 0x80, 0xc6, 0xd2, 0xe5, 0x69, 0x3f, 0x90, 0xaf, 0x98, 0xf0,
        0x2a, 0xe3, 0x08, 0xc8, 0x71, 0x59, 0xf0, 0xe8, 0x33, 0x2e, 0xe5, 0x0c, 0x7f, 0x53, 0x66,
        0xc9, 0x1d, 0x71, 0x4b, 0x50, 0x4b, 0x07, 0x08, 0x9a, 0x5d, 0x51, 0x9c, 0x38, 0x00, 0x00,
        0x00, 0x2c, 0x00, 0x00, 0x00, 0x50, 0x4b, 0x01, 0x02, 0x1e, 0x03, 0x0a, 0x00, 0x09, 0x00,
        0x00, 0x00, 0xc7, 0x80, 0x64, 0x5d, 0x9a, 0x5d, 0x51, 0x9c, 0x38, 0x00, 0x00, 0x00, 0x2c,
        0x00, 0x00, 0x00, 0x0a, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0xa4, 0x81, 0x00, 0x00, 0x00, 0x00, 0x6c, 0x65, 0x67, 0x61, 0x63, 0x79, 0x2e, 0x74, 0x78,
        0x74, 0x55, 0x54, 0x05, 0x00, 0x03, 0xe6, 0x49, 0xeb, 0x6a, 0x75, 0x78, 0x0b, 0x00, 0x01,
        0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x14, 0x00, 0x00, 0x00, 0x50, 0x4b, 0x05, 0x06, 0x00,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x50, 0x00, 0x00, 0x00, 0x8c, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];
    fs::write(&path, LEGACY_ZIP)?;
    let c = reader::list(path, &Cancellation::default())?;
    let id = id(&c, b"legacy.txt");
    assert!(c.nodes[id].locked);
    assert!(reader::preview_with_password(
        &c,
        id,
        Some(&archive::Password::new("wrong")),
        &Cancellation::default()
    )
    .is_err());
    reader::preview_with_password(
        &c,
        id,
        Some(&archive::Password::new("fixture-password")),
        &Cancellation::default(),
    )?;
    Ok(())
}
#[test]
fn encrypted_preview_authenticates_beyond_preview_limit_and_keeps_bounded_contents(
) -> io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let t = Temp::new()?;
    let path = t.0.join("large-encrypted.zip");
    let mut writer = zip::ZipWriter::new(fs::File::create(&path)?);
    writer.start_file(
        "large.txt",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .with_aes_encryption(zip::AesMode::Aes256, "secret"),
    )?;
    writer.write_all(&vec![b'x'; dn::viewer::MAX_BYTES + 100])?;
    writer.finish()?;
    let c = reader::list(path.clone(), &Cancellation::default())
        .map_err(|e| io::Error::other(format!("encrypted listing: {e}")))?;
    reader::preview_with_password(
        &c,
        1,
        Some(&archive::Password::new("secret")),
        &Cancellation::default(),
    )
    .map_err(|e| io::Error::other(format!("encrypted preview: {e}")))?;
    let mut zip = zip::ZipArchive::new(fs::File::open(&path)?)?;
    let raw = zip.by_index_raw(0)?;
    let at = raw.data_start().unwrap() + raw.compressed_size() - 1;
    drop(raw);
    drop(zip);
    let mut file = fs::OpenOptions::new().read(true).write(true).open(&path)?;
    file.seek(SeekFrom::Start(at))?;
    let mut b = [0];
    file.read_exact(&mut b)?;
    b[0] ^= 1;
    file.seek(SeekFrom::Start(at))?;
    file.write_all(&b)?;
    drop(file);
    let c = reader::list(path, &Cancellation::default())?;
    assert!(reader::preview_with_password(
        &c,
        1,
        Some(&archive::Password::new("secret")),
        &Cancellation::default()
    )
    .is_err());
    Ok(())
}

#[test]
fn encrypted_temp_drive_extraction_password_cancel_keeps_destination_empty() -> io::Result<()> {
    let t = Temp::new()?;
    protected_zip(&t.0.join("protected.zip"))?;
    let mut a = t.app()?;
    archive::open(&mut a, t.0.join("protected.zip"));
    finish(&mut a);
    select(&mut a, "private.txt");
    dn::menu::execute(&mut a, dn::menu::Action::CopyToTempDrive);
    a.submit_prompt();
    assert_eq!(focus::current(&a), Focus::ArchiveDialog);
    key(&mut a, KeyCode::Esc);
    let root = t.0.join("temp-drive");
    assert!(fs::read_dir(&root)?.next().is_none());
    assert!(!a.transfers.has_work());
    dn::menu::execute(&mut a, dn::menu::Action::CopyToTempDrive);
    a.submit_prompt();
    input::paste(&mut a, "secret 界");
    key(&mut a, KeyCode::Enter);
    wait(&mut a, |a| {
        a.modals.archive_dialog().is_none() && !a.transfers.has_work()
    });
    assert_eq!(
        a.transfers.entries.back().unwrap().status,
        dn::transfers::Status::Completed
    );
    assert_eq!(
        fs::read(root.join("private.txt"))?,
        b"AUTHENTICATED PRIVATE PREVIEW\n"
    );
    assert!(!root.join("second.txt").exists());
    Ok(())
}

#[test]
fn creation_from_desktop_menu_activates_manager_and_preserves_other_documents() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("source.txt"), b"keep")?;
    let mut a = t.app()?;
    select(&mut a, "source.txt");
    a.show_viewer(dn::viewer::Viewer::from_contents(
        t.0.join("memory.txt"),
        dn::viewer::from_bytes(b"open document".to_vec(), dn::viewer::ViewMode::Text),
    ));
    let viewer = a.desktop.active().id;
    dn::menu::execute(&mut a, dn::menu::Action::CreateArchive);
    assert_eq!(a.desktop.active().id, dn::desktop::WindowId::MANAGER);
    assert_eq!(focus::current(&a), Focus::ArchiveDialog);
    assert!(a.desktop.viewer(viewer).is_some());
    key(&mut a, KeyCode::Esc);
    assert!(a.modals.archive_dialog().is_none());
    assert!(a.desktop.viewer(viewer).is_some());
    assert_eq!(fs::read(t.0.join("source.txt"))?, b"keep");
    assert!(!t.0.join("source.txt.zip").exists());
    Ok(())
}

#[test]
fn extraction_ask_merges_existing_folders_and_confirms_file_replacement() -> io::Result<()> {
    use dn::{
        operations::conflict::Decision,
        transfers::{self, Action, Status},
    };
    let t = Temp::new()?;
    let archive_path = t.0.join("merge.zip");
    clean_zip(&archive_path, false)?;
    let out = t.0.join("out");
    fs::create_dir_all(out.join("docs"))?;
    fs::write(out.join("docs/a.txt"), b"old")?;
    fs::write(out.join("docs/keep.txt"), b"unrelated")?;
    let mut app = t.app()?;
    archive::open(&mut app, archive_path);
    finish(&mut app);
    select(&mut app, "docs");
    app.prompt_operation(dn::operations::Kind::Copy, false);
    app.modals
        .prompt_mut()
        .unwrap()
        .copy
        .as_mut()
        .unwrap()
        .picked = Some(out.clone());
    app.submit_prompt();
    wait(&mut app, |a| {
        a.transfers
            .entries
            .back()
            .is_some_and(|e| e.status == Status::Waiting)
    });
    let id = app.transfers.entries.back().unwrap().id;
    transfers::action(&mut app, Action::Conflict(id, Decision::ReplaceAll));
    wait(&mut app, |a| !a.transfers.has_work());
    assert_eq!(
        app.transfers.entries.back().unwrap().status,
        Status::Completed
    );
    assert_ne!(fs::read(out.join("docs/a.txt"))?, b"old");
    assert!(out.join("docs/b.txt").is_file());
    assert_eq!(fs::read(out.join("docs/keep.txt"))?, b"unrelated");
    Ok(())
}

#[test]
fn zip_update_and_removal_dialogs_keep_cancel_default_localization_and_immutable_frames(
) -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("edit.zip");
    clean_zip(&path, false)?;
    let original = fs::read(&path)?;
    fs::write(t.0.join("add.txt"), "add")?;
    let mut a = t.app()?;
    archive::open(&mut a, path.clone());
    finish(&mut a);
    select(&mut a, "root.txt");
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        a.config.language = language;
        for palette in [
            dn::palette::PaletteProfile::ClassicDn,
            dn::palette::PaletteProfile::Custom([0x3f; dn::palette::theme::COUNT]),
        ] {
            a.config.palette = palette;
            a.prompt_operation(dn::operations::Kind::Delete, false);
            assert!(a.modals.archive_dialog().is_some());
            for (w, h) in [(80, 25), (132, 40), (40, 12), (80, 25)] {
                let before = draw(&mut a, w, h)?;
                let hits = a.mouse.regions.clone();
                for _ in 0..3 {
                    assert_eq!(before, draw(&mut a, w, h)?);
                    assert_eq!(hits, a.mouse.regions);
                }
                for (rect, _) in &hits {
                    assert!(rect.right() <= w && rect.bottom() <= h);
                }
            }
            key(&mut a, KeyCode::Enter);
            assert!(a.modals.archive_dialog().is_none());
            assert!(!a.transfers.has_work());
            assert_eq!(original, fs::read(&path)?);
            a.active = 1;
            a.panels[1].refresh(false);
            wait(&mut a, |a| !a.panels[1].loading);
            select(&mut a, "add.txt");
            a.prompt_operation(dn::operations::Kind::Copy, false);
            assert!(a.modals.archive_dialog().is_some());
            let first = draw(&mut a, 80, 25)?;
            assert_eq!(first, draw(&mut a, 80, 25)?);
            assert_eq!(first, draw(&mut a, 80, 25)?);
            key(&mut a, KeyCode::Enter);
            assert!(!a.transfers.has_work());
            assert_eq!(original, fs::read(&path)?);
            a.active = 0;
        }
    }
    Ok(())
}
