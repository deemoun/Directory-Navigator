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
        !s.text().contains("directories Help") && s.text().contains("Read-only")
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
