use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    app::App,
    archive::{
        self,
        reader::{self, Kind},
        Action,
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
    wait(a, |a| a.archive().is_some_and(|b| !b.loading()));
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
    let b = a.archive_mut().unwrap();
    let c = b.catalog.as_ref().unwrap();
    b.cursor = b
        .children()
        .iter()
        .position(|i| c.nodes[*i].name() == name)
        .unwrap()
        + 1;
    b.keep_visible();
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
    assert_eq!(focus::current(&a), Focus::Archive);
    select(&mut a, "folder");
    key(&mut a, KeyCode::Enter);
    select(&mut a, "文.txt");
    key(&mut a, KeyCode::F(3));
    wait(&mut a, |a| a.desktop.active_viewer().is_some());
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
    assert_eq!(focus::current(&a), Focus::Archive);
    key(&mut a, KeyCode::Backspace);
    assert_eq!(a.archive().unwrap().directory, 0);
    key(&mut a, KeyCode::Esc);
    assert!(a.archive().is_none());
    assert_eq!(a.panels[0].selected, marks);
    assert_eq!(a.panels[0].cursor, cursor);
    assert_eq!(a.panels[1].path, t.0);
    Ok(())
}
#[test]
fn mouse_double_click_modals_buttons_and_resizing() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("test.zip");
    zip(&p, false)?;
    let mut a = t.app()?;
    archive::open(&mut a, p);
    finish(&mut a);
    for (w, h) in [(80, 25), (132, 40), (30, 10), (80, 25)] {
        draw(&mut a, w, h)?;
    }
    select(&mut a, "folder");
    let row = a.archive().unwrap().cursor;
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Archive(Action::Row(row)));
    click(&mut a, Target::Archive(Action::Row(row)));
    assert_ne!(a.archive().unwrap().directory, 0);
    select(&mut a, "文.txt");
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Archive(Action::Info));
    assert_eq!(focus::current(&a), Focus::Message);
    key(&mut a, KeyCode::Esc);
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Archive(Action::View));
    wait(&mut a, |a| a.desktop.active_viewer().is_some());
    key(&mut a, KeyCode::Esc);
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Archive(Action::Close));
    assert!(a.archive().is_none());
    Ok(())
}
#[test]
fn close_stop_refresh_and_failed_open_do_not_publish_stale_replies() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("test.zip");
    zip(&p, true)?;
    let mut a = t.app()?;
    archive::open(&mut a, p.clone());
    archive::action(&mut a, Action::Close);
    for _ in 0..10 {
        a.poll();
    }
    assert!(a.archive().is_none());
    archive::open(&mut a, p.clone());
    archive::action(&mut a, Action::Stop);
    assert!(!a.archive().unwrap().loading());
    archive::action(&mut a, Action::Refresh);
    finish(&mut a);
    assert!(a.archive().unwrap().catalog.is_some());
    select(&mut a, "root.bin");
    archive::action(&mut a, Action::View);
    archive::action(&mut a, Action::Close);
    for _ in 0..10 {
        a.poll();
    }
    assert!(a.desktop.active_viewer().is_none());
    archive::open(&mut a, t.0.join("missing.zip"));
    finish(&mut a);
    assert!(!a.dialogs().is_empty());
    assert_eq!(a.panels[0].path, t.0);
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
    assert!(app.archive().is_none());
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
    assert!(app.archive().is_none());
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
