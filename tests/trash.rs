use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dn::{
    app::App,
    config::Config,
    dialog::PromptKind,
    input,
    operations::{Kind, Progress},
    settings::{self, Setting, Settings, SettingsKind},
    trash_support, ui,
};
use ratatui::{backend::TestBackend, Terminal};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-trash-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        fs::write(self.0.join("file.txt"), "retained")?;
        let mut app = App::new(self.0.clone())?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.panels.iter().any(|p| p.loading) {
            app.poll();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        app.panels[0].cursor = app.panels[0]
            .entries
            .iter()
            .position(|e| e.name == "file.txt")
            .unwrap();
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut t = Terminal::new(TestBackend::new(w, h))?;
    t.draw(|f| ui::draw(f, app))?;
    for (rect, _) in &app.mouse.regions {
        assert!(rect.right() <= w && rect.bottom() <= h);
    }
    Ok(t.backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}

#[test]
fn deletion_defaults_legacy_settings_roundtrip_and_cancel() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("desktop");
    assert!(Config::default().use_trash);
    assert!(Config::read(&path)?.use_trash);
    fs::write(&path, "hidden=true\n")?;
    assert!(Config::read(&path)?.use_trash);
    fs::write(&path, "use_trash=unknown\n")?;
    assert!(Config::read(&path)?.use_trash);
    let config = Config {
        use_trash: false,
        ..Default::default()
    };
    config.save(&path)?;
    assert!(!Config::read(&path)?.use_trash);
    let mut app = temp.app()?;
    for code in [KeyCode::F(8), KeyCode::Delete] {
        input::handle(&mut app, KeyEvent::new(code, KeyModifiers::NONE));
        assert!(matches!(
            app.prompt.as_ref().unwrap().kind,
            PromptKind::Operation(Kind::Trash)
        ));
        let text = draw(&mut app, 80, 25)?;
        assert!(text.contains("Move to Trash"));
        assert!(!text.contains("This cannot be undone"));
        input::handle(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.job.is_none());
    }
    assert!(temp.0.join("file.txt").exists());
    app.settings = Some(Settings::new(SettingsKind::FileManager, &app.config));
    let index = app
        .settings
        .as_ref()
        .unwrap()
        .rows()
        .iter()
        .position(|s| matches!(s, Setting::Trash))
        .unwrap();
    app.settings.as_mut().unwrap().toggle(index);
    settings::key(&mut app, KeyCode::Esc);
    assert!(app.config.use_trash);
    app.settings = Some(Settings::new(SettingsKind::FileManager, &app.config));
    app.settings.as_mut().unwrap().toggle(index);
    for (w, h) in [(80, 25), (132, 40)] {
        let text = draw(&mut app, w, h)?;
        assert!(text.contains("Move deleted files to Trash"));
    }
    settings::accept(&mut app);
    assert!(!app.config.use_trash);
    app.prompt_operation(Kind::Delete, false);
    assert!(matches!(
        app.prompt.as_ref().unwrap().kind,
        PromptKind::Operation(Kind::Delete)
    ));
    assert!(draw(&mut app, 80, 25)?.contains("This cannot be undone"));
    Ok(())
}

#[test]
fn trash_failure_partial_progress_cancellation_and_native_paths_are_safe() -> io::Result<()> {
    let temp = Temp::new()?;
    let bin = temp.0.join("isolated-bin");
    fs::create_dir(&bin)?;
    let one = temp.0.join("one文");
    let two = temp.0.join("two");
    fs::write(&one, "one")?;
    fs::write(&two, "two")?;
    let cancel = AtomicBool::new(false);
    let mut progress = Progress::default();
    let error = trash_support::move_items(
        &[one.clone(), two.clone()],
        &cancel,
        &mut progress,
        |_| {},
        |path| {
            if path.file_name().unwrap() == "two" {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unavailable",
                ))
            } else {
                fs::rename(path, bin.join(path.file_name().unwrap()))
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.to_string().contains("No permanent-delete fallback"));
    assert_eq!(progress.files, 1);
    assert!(two.exists());
    assert_eq!(fs::read(bin.join("one文"))?, b"one");
    cancel.store(true, Ordering::Relaxed);
    let error = trash_support::move_items(
        std::slice::from_ref(&two),
        &cancel,
        &mut progress,
        |_| {},
        |_| panic!("cancelled operation called backend"),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(two.exists());
    cancel.store(false, Ordering::Relaxed);
    let error = trash_support::move_items(
        std::slice::from_ref(&two),
        &cancel,
        &mut progress,
        |_| cancel.store(true, Ordering::Relaxed),
        |_| panic!("cancellation at entry notification must not reach Trash backend"),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert_eq!(fs::read(&two)?, b"two");
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_broken_links_and_undecodable_names_are_not_followed_or_rewritten() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let temp = Temp::new()?;
    let bin = temp.0.join("bin");
    fs::create_dir(&bin)?;
    let outside = temp.0.join("outside");
    fs::write(&outside, "safe")?;
    let link = temp.0.join("link");
    let broken = temp.0.join("broken");
    symlink(&outside, &link)?;
    symlink("missing", &broken)?;
    #[cfg(target_os = "linux")]
    let name = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(b"native-\xff".to_vec())
    };
    #[cfg(not(target_os = "linux"))]
    let name = std::ffi::OsString::from("native-文");
    let native = temp.0.join(name);
    fs::write(&native, "bytes")?;
    trash_support::move_items(
        &[link, broken, native.clone()],
        &AtomicBool::new(false),
        &mut Progress::default(),
        |_| {},
        |path| fs::rename(path, bin.join(path.file_name().unwrap())),
    )?;
    assert_eq!(fs::read(&outside)?, b"safe");
    assert!(fs::symlink_metadata(bin.join("link"))?.is_symlink());
    assert_eq!(fs::read_link(bin.join("broken"))?, PathBuf::from("missing"));
    assert_eq!(fs::read(bin.join(native.file_name().unwrap()))?, b"bytes");
    Ok(())
}

#[test]
fn missing_and_parent_paths_never_reach_backend() -> io::Result<()> {
    let temp = Temp::new()?;
    for path in [
        temp.0.join("missing"),
        temp.0.join(".."),
        PathBuf::from("."),
    ] {
        assert!(trash_support::move_items(
            &[path],
            &AtomicBool::new(false),
            &mut Progress::default(),
            |_| {},
            |_| panic!("unsafe input reached backend")
        )
        .is_err());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn native_macos_unavailable_paths_fail_safely() -> io::Result<()> {
    use std::os::unix::ffi::OsStringExt;
    let temp = Temp::new()?;
    assert!(trash_support::send(&temp.0.join("missing")).is_err());
    let native = temp
        .0
        .join(std::ffi::OsString::from_vec(b"native-\xff".to_vec()));
    assert_eq!(
        trash_support::send(&native).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "native integration: moves only a disposable fixture to the real macOS Trash"]
fn native_macos_trash() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let temp = Temp::new()?;
    let name = format!(
        "dn-native-trash-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let source = temp.0.join(&name);
    fs::create_dir(&source)?;
    fs::write(source.join("文.txt"), "recoverable")?;
    let outside = temp.0.join("outside");
    fs::write(&outside, "safe")?;
    symlink(&outside, source.join("link"))?;
    symlink("absent", source.join("broken"))?;
    trash_support::send(&source)?;
    assert!(!source.exists());
    assert_eq!(fs::read(&outside)?, b"safe");
    let trashed = PathBuf::from(std::env::var_os("HOME").unwrap())
        .join(".Trash")
        .join(&name);
    assert_eq!(fs::read(trashed.join("文.txt"))?, b"recoverable");
    assert!(fs::symlink_metadata(trashed.join("link"))?.is_symlink());
    assert_eq!(
        fs::read_link(trashed.join("broken"))?,
        PathBuf::from("absent")
    );
    // Restore only this test's uniquely named fixture before normal temp cleanup.
    fs::rename(&trashed, &source)?;
    Ok(())
}
