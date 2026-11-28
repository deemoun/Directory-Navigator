//! Private cache fixture only. Never recursively scans the user's cache or system roots.
#![cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "support/pty.rs"]
pub mod pty;
use pty::Session;
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn click(
    s: &mut Session,
    label: &str,
    stage: &str,
    ready: impl Fn(&Session) -> bool,
) -> io::Result<()> {
    let text = s.text();
    let (y, line) = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains(label) && (label != "Analyze" || l.contains("Analyze all")))
        .ok_or_else(|| io::Error::other(format!("Missing {label}: {text}")))?;
    let x = unicode_width::UnicodeWidthStr::width(&line[..line.find(label).unwrap()]) + 2;
    let y = y + 1;
    s.send(
        format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").as_bytes(),
        stage,
        |s| {
            ready(s)
                && fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))
                    .is_ok_and(|d| d.contains("button=none") && d.contains("capture=None "))
        },
    )
}
#[test]
fn cache_analysis_help_navigation_resize_and_terminal_restoration() -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let t = Temp(std::env::temp_dir().join(format!(
        "dn-cache-pty-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&t.0)?;
    let cache = if cfg!(target_os = "macos") {
        t.0.join("Library/Caches")
    } else {
        t.0.join(".cache")
    };
    fs::create_dir_all(&cache)?;
    fs::write(cache.join(".private-tone"), b"12345")?;
    fs::write(cache.join("cache.txt"), b"visible")?;
    let home = t.0.to_str().unwrap();
    let mut s = Session::with_preferences_environment(
        "cache-directories",
        "[interface]\nshow_clock=false\n",
        &[("HOME", home), ("XDG_CACHE_HOME", "")],
    )?;
    s.wait("manager ready", Session::command_prompt_active)?;
    s.send(b"\x1bu", "Utilities menu", |s| {
        s.text().contains("Cache and temporary")
    })?;
    click(&mut s, "Cache and temporary", "cache window", |s| {
        s.text().contains("Select a location and Analyze")
    })?;
    assert!(s.text().contains("Not analyzed"));
    #[cfg(target_os = "macos")]
    {
        s.send(b"\x1b[F", "scroll to application locations", |s| {
            s.text().contains("Device backups (valuable)")
        })?;
        s.send(b"\x1b[H", "return to private cache", |s| {
            s.text().contains("User caches") && !s.text().contains("Device backups (valuable)")
        })?;
    }
    click(&mut s, "Analyze", "private analysis", |s| {
        s.text().contains("Snapshot ready") && s.text().contains("Complete")
    })?;
    click(&mut s, "Copy path", "internal clipboard", |s| {
        s.text().contains("text retained internally")
    })?;
    s.send(b"\x1bOP", "cache help", |s| {
        s.text().contains("directories Help")
    })?;
    s.send(b"\x1b", "return to cache", |s| {
        !s.text().contains("directories Help") && s.text().contains("Apparent bytes")
    })?;
    s.resize(110, 34)?;
    s.resize(80, 25)?;
    // Keep the original directory alive under another name so the replacement
    // has a different native identity on every supported Unix filesystem.
    fs::rename(&cache, cache.with_file_name("original-cache"))?;
    fs::create_dir(&cache)?;
    fs::write(cache.join(".private-tone"), b"12345")?;
    fs::write(cache.join("cache.txt"), b"visible")?;
    click(&mut s, "Analyze", "detect replaced cache directory", |s| {
        s.text().contains("Directory changed") && s.text().contains("Snapshot ready")
    })?;
    s.send(b"\x1b[Z", "focus changed cache row", |_| true)?;
    // Rejected Enter need not redraw; the following F1 gives an observable
    // result and must still belong to the cache plugin, not the manager.
    s.send(b"\r\x1bOP", "changed cache cannot open", |s| {
        s.text().contains("directories Help")
    })?;
    s.send(b"\x1b", "return from changed cache help", |s| {
        !s.text().contains("directories Help") && s.text().contains("Directory changed")
    })?;
    click(&mut s, "Refresh", "refresh replacement identity", |s| {
        s.text().contains("Select a location and Analyze") && s.text().contains("Not analyzed")
    })?;
    click(&mut s, "Analyze", "analyze replacement cache", |s| {
        s.text().contains("Snapshot ready") && s.text().contains("Complete")
    })?;
    click(&mut s, "Open", "open cache in original pane", |s| {
        Session::manager_document_ready(s)
            && fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))
                .is_ok_and(|d| d.contains("panels_loading=[false, false]"))
    })?;
    // DN keeps floating tools visible over its manager background. Open must
    // preserve this document; close it explicitly before inspecting panel pixels.
    assert!(s.text().contains("Cache and temporary directories"));
    // Focus-only switching can produce no changed pixels. Acknowledge the
    // combined switch/close by its visible result, not an artificial idle frame.
    s.send(
        b"\x1b[17;3~\x1b",
        "close retained cache window and inspect target",
        |s| Session::manager_document_ready(s) && s.text().contains("cache.txt"),
    )?;
    assert_eq!(fs::read(cache.join(".private-tone"))?, b"12345");
    s.finish()
}

#[cfg(target_os = "linux")]
#[test]
fn cache_cleanup_review_cancel_trash_and_terminal_restoration() -> io::Result<()> {
    let t = Temp(std::env::temp_dir().join(format!("dn-clean-pty-{}", std::process::id())));
    fs::create_dir(&t.0)?;
    let cache = t.0.join(".cache");
    fs::create_dir(&cache)?;
    use std::os::unix::fs::PermissionsExt;
    for directory in [&t.0, &cache] {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    let old = cache.join("old-cache");
    let recent = cache.join("recent-cache");
    let unchecked = cache.join("old-keep");
    fs::write(&old, b"old cache payload")?;
    fs::write(&recent, b"recent payload")?;
    fs::write(&unchecked, b"unchecked payload")?;
    for path in [&old, &unchecked] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        std::fs::File::options().write(true).open(path)?.set_times(
            std::fs::FileTimes::new().set_modified(
                std::time::SystemTime::now() - std::time::Duration::from_secs(9 * 24 * 60 * 60),
            ),
        )?;
    }
    let data = t.0.join("data");
    let home = t.0.to_str().unwrap();
    let data_env = data.to_str().unwrap();
    let mut s = Session::with_preferences_environment(
        "cache-cleanup",
        "[interface]\nshow_clock=false\n",
        &[
            ("HOME", home),
            ("XDG_CACHE_HOME", ""),
            ("XDG_DATA_HOME", data_env),
        ],
    )?;
    s.wait("manager ready", Session::command_prompt_active)?;
    s.send(b"\x1bu", "Utilities menu", |s| {
        s.text().contains("Cache and temporary")
    })?;
    click(&mut s, "Cache and temporary", "cache window", |s| {
        s.text().contains("Select a location and Analyze")
    })?;
    click(&mut s, "Clean…", "prepare old-cache review", |s| {
        s.text().contains("Review cache cleanup") && s.text().contains("old-cache")
    })?;
    assert!(!s.text().contains("recent-cache"));
    s.send(b"\r", "default cancel keeps cache", |s| {
        !s.text().contains("Review cache cleanup") && s.text().contains("Cache and temporary")
    })?;
    assert!(old.exists() && recent.exists());
    click(&mut s, "Clean…", "prepare reviewed cleanup again", |s| {
        s.text().contains("Review cache cleanup")
    })?;
    click(&mut s, "Select none", "uncheck the candidate list", |s| {
        s.text().contains("Selected: 0/2")
    })?;
    click(&mut s, "Select all", "check the candidate list", |s| {
        s.text().contains("Selected: 2/2")
    })?;
    click(&mut s, "old-keep", "exclude one old cache file", |s| {
        s.text().contains("Selected: 1/2")
    })?;
    click(
        &mut s,
        "Move to Trash",
        "confirm reviewed cache files",
        |s| s.text().contains("Cleanup completed") && !old.exists(),
    )?;
    assert_eq!(fs::read(&recent)?, b"recent payload");
    assert_eq!(fs::read(&unchecked)?, b"unchecked payload");
    assert!(s.text().contains("Last cleanup: 1/1"));
    assert!(cache.is_dir());
    let files = data.join("Trash/files");
    let moved = fs::read_dir(files)?.next().unwrap()?.path();
    assert_eq!(fs::read(moved)?, b"old cache payload");
    s.send(
        b"\x1b",
        "close cache cleanup",
        Session::command_prompt_active,
    )?;
    s.finish()
}
