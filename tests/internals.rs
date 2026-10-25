use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, layout::Rect, Terminal};
use dn::{
    app::App,
    editor::buffer::Buffer,
    focus::{self, Focus},
    input,
    mouse::{self, Target},
    operations::{self, Kind, MoveStrategy, Plan},
    publication::StagedFile,
    task::{Cancellation, Task},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-internals-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.poll();
        assert!(Instant::now() < deadline, "background timeout");
        thread::sleep(Duration::from_millis(1));
    }
}
fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    input::handle(app, KeyEvent::new(code, modifiers));
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h))?;
    terminal.draw(|f| ui::draw(f, app))?;
    for (r, _) in &app.mouse.regions {
        assert!(r.right() <= w && r.bottom() <= h);
    }
    Ok(terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
fn click(app: &mut App, target: Target) {
    let r = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == target)
        .expect("mouse target")
        .0;
    mouse::handle(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}
#[test]
fn one_focus_policy_for_nested_errors_keyboard_mouse_and_paste() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.prompt_goto();
    input::paste(&mut app, "unchanged");
    dn::screensaver::open(&mut app);
    app.error("topmost error");
    assert_eq!(focus::current(&app), Focus::Message);
    input::paste(&mut app, "must not leak");
    assert_eq!(app.modals.prompt().expect("prompt").field, "unchanged");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.dialogs().is_empty());
    assert!(app.screensaver.modals.screensaver().is_some());
    app.error("topmost error");
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::CloseDialog);
    assert!(app.dialogs().is_empty());
    assert!(app.screensaver.modals.screensaver().is_some());
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(focus::current(&app), Focus::Prompt);
    Ok(())
}
#[test]
fn layout_is_available_to_commands_without_painting() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    dn::layout::prepare(&mut app, Rect::new(0, 0, 80, 25));
    key(&mut app, KeyCode::F(4), KeyModifiers::SHIFT);
    input::paste(&mut app, "text\nsecond");
    let editor = app.desktop.active_editor().expect("editor");
    assert!(editor.columns > 60);
    assert_eq!(editor.rows, 21);
    assert_eq!(editor.scroll_column, 0);
    assert_eq!(app.wait_duration(Instant::now()).as_secs(), 0); // clock deadline <= 1 second
    app.config.show_clock = false;
    assert_eq!(app.wait_duration(Instant::now()), Duration::from_secs(1));
    Ok(())
}
#[test]
fn dropped_task_cancels_work_and_suppresses_late_results() -> io::Result<()> {
    let (started, ready) = mpsc::channel();
    let ended = Arc::new(AtomicBool::new(false));
    let flag = ended.clone();
    let task = Task::spawn("test-cancel", move |cancel| {
        let _ = started.send(());
        while !cancel.cancelled() {
            thread::sleep(Duration::from_millis(1));
        }
        flag.store(true, Ordering::Relaxed);
        42
    })?;
    ready
        .recv_timeout(Duration::from_secs(5))
        .map_err(io::Error::other)?;
    drop(task);
    let end = Instant::now() + Duration::from_secs(5);
    while !ended.load(Ordering::Relaxed) {
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
    let token = Cancellation::default();
    token.cancel();
    assert_eq!(
        token.check().expect_err("cancelled").kind(),
        io::ErrorKind::Interrupted
    );
    let task = Task::spawn("test-next", |_| 7)?;
    loop {
        match task.try_recv() {
            Ok(n) => {
                assert_eq!(n, 7);
                break;
            }
            Err(mpsc::TryRecvError::Empty) => thread::sleep(Duration::from_millis(1)),
            Err(e) => panic!("{e}"),
        }
    }
    Ok(())
}
fn validate_index(buffer: &Buffer) {
    let fresh = Buffer::new(buffer.text.clone()).expect("valid reference");
    let indices = |b: &Buffer| {
        b.lines
            .iter()
            .map(|l| (l.start, l.end, l.next))
            .collect::<Vec<_>>()
    };
    assert_eq!(indices(buffer), indices(&fresh), "{:?}", buffer.text);
    assert!(buffer.text.is_char_boundary(buffer.cursor));
}
#[test]
fn incremental_index_matches_full_parse_for_unicode_and_mixed_endings() {
    let mut buffer = Buffer::new("a\r\nb\rc\n👨‍👩‍👧‍👦é\n".into()).expect("buffer");
    let mut seed = 123456789u64;
    let pieces = ["", "x", "\r", "\n", "\r\n", "界", "é", "👨‍👩‍👧‍👦", "\t"];
    for _ in 0..800 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let points: Vec<_> = buffer
            .text
            .char_indices()
            .map(|(i, _)| i)
            .chain(Some(buffer.text.len()))
            .collect();
        let a = points[seed as usize % points.len()];
        let b = points[(seed >> 21) as usize % points.len()];
        let before = buffer.text.clone();
        let text = pieces[(seed >> 40) as usize % pieces.len()];
        buffer.replace(a.min(b)..a.max(b), text).expect("replace");
        validate_index(&buffer);
        let after = buffer.text.clone();
        if before != after {
            buffer.undo(false);
            assert_eq!(buffer.text, before);
            validate_index(&buffer);
            buffer.undo(true);
            assert_eq!(buffer.text, after);
            validate_index(&buffer);
        }
    }
}
#[test]
fn move_is_exclusive_and_preserves_native_identity() -> io::Result<()> {
    let temp = Temp::new()?;
    let source = temp.0.join("source");
    let target = temp.0.join("target");
    fs::write(&source, "original")?;
    let identity = fs::symlink_metadata(&source)?;
    let plan = Plan {
        kind: Kind::Move,
        sources: vec![source.clone()],
        destination: target.clone(),
    };
    operations::run(&plan, &AtomicBool::new(false), |_| {})?;
    assert!(!source.exists());
    assert!(dn::platform_fs::same_identity(
        &identity,
        &fs::symlink_metadata(&target)?
    ));
    fs::write(&source, "second")?;
    assert_eq!(
        operations::run(&plan, &AtomicBool::new(false), |_| {})
            .expect_err("collision")
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read_to_string(&source)?, "second");
    assert_eq!(fs::read_to_string(&target)?, "original");
    Ok(())
}
#[test]
fn racing_move_destination_is_never_overwritten() -> io::Result<()> {
    let temp = Temp::new()?;
    let source = temp.0.join("source");
    let target = temp.0.join("target");
    fs::write(&source, "original")?;
    let result = operations::run(
        &Plan {
            kind: Kind::Move,
            sources: vec![source.clone()],
            destination: target.clone(),
        },
        &AtomicBool::new(false),
        |_| {
            fs::write(&target, "racing").expect("race fixture");
        },
    );
    assert_eq!(
        result.expect_err("collision").kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read_to_string(source)?, "original");
    assert_eq!(fs::read_to_string(target)?, "racing");
    Ok(())
}
#[test]
fn interrupted_copy_has_structured_failure_and_no_partial_final_name() -> io::Result<()> {
    let temp = Temp::new()?;
    let source = temp.0.join("source");
    let target = temp.0.join("target");
    fs::write(&source, vec![9; 1024 * 1024])?;
    let cancel = AtomicBool::new(false);
    let result = operations::execute(
        &Plan {
            kind: Kind::Move,
            sources: vec![source.clone()],
            destination: target.clone(),
        },
        &cancel,
        |p| {
            if p.bytes > 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        MoveStrategy::Copy,
    );
    let failure = result.expect_err("cancelled");
    assert_eq!(failure.kind, io::ErrorKind::Interrupted);
    assert!(failure.progress.bytes > 0);
    assert_eq!(fs::metadata(source)?.len(), 1024 * 1024);
    assert!(!target.exists());
    assert_eq!(fs::read_dir(&temp.0)?.count(), 1);
    Ok(())
}
#[test]
fn staging_cleanup_never_removes_a_replacement() -> io::Result<()> {
    use std::io::Write;
    let temp = Temp::new()?;
    let target = temp.0.join("target");
    let mut staged = StagedFile::create(&target)?;
    staged.file.write_all(b"new data")?;
    let own_path = staged.path().to_path_buf();
    fs::rename(&own_path, temp.0.join("original-stage"))?;
    fs::write(&own_path, "replacement")?;
    drop(staged);
    assert_eq!(fs::read_to_string(own_path)?, "replacement");
    assert!(!target.exists());
    Ok(())
}
#[test]
fn staged_ownership_tracks_the_open_file_not_its_name() -> io::Result<()> {
    let temp = Temp::new()?;
    let target = temp.0.join("target");
    let stage = StagedFile::create(&target)?;
    let path = stage.path().to_owned();
    assert!(dn::platform_fs::owns_file(&path, &stage.file));
    let held = temp.0.join("held-stage");
    fs::rename(&path, &held)?;
    fs::write(&path, b"replacement")?;
    assert!(dn::platform_fs::owns_file(&held, &stage.file));
    assert!(!dn::platform_fs::owns_file(&path, &stage.file));
    assert!(!dn::platform_fs::owns_file(
        &temp.0.join("missing"),
        &stage.file
    ));
    assert!(stage.publish(&target).is_err());
    drop(stage);
    assert_eq!(fs::read(&path)?, b"replacement");
    assert!(held.exists());
    assert!(!target.exists());
    Ok(())
}

#[cfg(windows)]
#[test]
fn windows_staging_replacement_with_identical_creation_time_is_not_owned() -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            GetFileInformationByHandle, SetFileTime, BY_HANDLE_FILE_INFORMATION,
        },
    };
    let temp = Temp::new()?;
    let target = temp.0.join("target");
    let stage = StagedFile::create(&target)?;
    let path = stage.path().to_owned();
    let mut original = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: stage owns a live file handle; original is writable.
    unsafe { GetFileInformationByHandle(HANDLE(stage.file.as_raw_handle()), &mut original) }
        .map_err(io::Error::other)?;
    fs::rename(&path, temp.0.join("held-stage"))?;
    let replacement = fs::File::create(&path)?;
    // SAFETY: replacement is live and the creation-time pointer remains valid.
    // Force the timestamp collision rather than relying on NTFS time tunneling.
    unsafe {
        SetFileTime(
            HANDLE(replacement.as_raw_handle()),
            Some(&original.ftCreationTime),
            None,
            None,
        )
    }
    .map_err(io::Error::other)?;
    assert_eq!(
        stage.file.metadata()?.created()?,
        replacement.metadata()?.created()?
    );
    assert!(!dn::platform_fs::owns_file(&path, &stage.file));
    assert!(stage.publish(&target).is_err());
    drop(stage);
    assert!(path.exists());
    assert!(!target.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn staged_ownership_does_not_follow_a_replacement_symlink() -> io::Result<()> {
    let temp = Temp::new()?;
    let stage = StagedFile::create(&temp.0.join("target"))?;
    let path = stage.path().to_owned();
    let held = temp.0.join("held-stage");
    fs::rename(&path, &held)?;
    std::os::unix::fs::symlink(&held, &path)?;
    assert!(!dn::platform_fs::owns_file(&path, &stage.file));
    drop(stage);
    assert_eq!(fs::read_link(&path)?, held);
    Ok(())
}
#[test]
fn crash_child_leaves_no_incomplete_final_file() -> io::Result<()> {
    let temp = Temp::new()?;
    let status = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "crash_publication_child", "--nocapture"])
        .env("DN_CRASH_FIXTURE", &temp.0)
        .status()?;
    assert_eq!(status.code(), Some(73));
    assert!(!temp.0.join("final").exists());
    assert_eq!(fs::read_dir(&temp.0)?.count(), 1);
    Ok(())
}
#[test]
fn crash_publication_child() -> io::Result<()> {
    use std::io::Write;
    let Some(root) = std::env::var_os("DN_CRASH_FIXTURE") else {
        return Ok(());
    };
    let mut staged = StagedFile::create(&PathBuf::from(root).join("final"))?;
    staged.file.write_all(b"incomplete")?;
    staged.file.sync_all()?;
    std::process::exit(73)
}
#[cfg(target_os = "macos")]
#[test]
fn macos_extended_metadata_survives_copy_and_editor_save() -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let temp = Temp::new()?;
    let source = temp.0.join("source");
    let target = temp.0.join("target");
    fs::write(&source, "original")?;
    let name = std::ffi::CString::new("org.dn.test").expect("literal");
    let input = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&source)?;
    assert_eq!(
        unsafe {
            libc::fsetxattr(
                input.as_raw_fd(),
                name.as_ptr(),
                b"metadata".as_ptr().cast(),
                8,
                0,
                0,
            )
        },
        0
    );
    operations::run(
        &Plan {
            kind: Kind::Copy,
            sources: vec![source.clone()],
            destination: target.clone(),
        },
        &AtomicBool::new(false),
        |_| {},
    )?;
    let check = |path: &std::path::Path| -> io::Result<()> {
        let f = fs::File::open(path)?;
        let mut value = [0u8; 8];
        assert_eq!(
            unsafe {
                libc::fgetxattr(
                    f.as_raw_fd(),
                    name.as_ptr(),
                    value.as_mut_ptr().cast(),
                    8,
                    0,
                    0,
                )
            },
            8
        );
        assert_eq!(&value, b"metadata");
        Ok(())
    };
    check(&target)?;
    let source_directory = temp.0.join("source-dir");
    let target_directory = temp.0.join("target-dir");
    fs::create_dir(&source_directory)?;
    let directory = dn::platform_fs::open_directory(&source_directory)?;
    assert_eq!(
        unsafe {
            libc::fsetxattr(
                directory.as_raw_fd(),
                name.as_ptr(),
                b"metadata".as_ptr().cast(),
                8,
                0,
                0,
            )
        },
        0
    );
    operations::run(
        &Plan {
            kind: Kind::Copy,
            sources: vec![source_directory],
            destination: target_directory.clone(),
        },
        &AtomicBool::new(false),
        |_| {},
    )?;
    check(&target_directory)?;
    let (_, snapshot, bom) = dn::editor::storage::load(&source).map_err(io::Error::other)?;
    dn::editor::storage::save(&source, "edited", bom, Some(&snapshot)).map_err(io::Error::other)?;
    check(&source)?;
    Ok(())
}
