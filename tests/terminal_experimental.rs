//! Real terminal exercise; enabling experimental plugins must not open devices.
#![cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "support/pty.rs"]
pub mod pty;
use pty::Session;
use std::io;
fn click(
    s: &mut Session,
    label: &str,
    stage: &str,
    ready: impl Fn(&Session) -> bool,
) -> io::Result<()> {
    let text = s.text();
    let (row, line) = text
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(label))
        .ok_or_else(|| io::Error::other(format!("Missing {label}: {text}")))?;
    let x = unicode_width::UnicodeWidthStr::width(&line[..line.find(label).unwrap()]) + 1;
    let y = row + 1;
    s.send(
        format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").as_bytes(),
        stage,
        ready,
    )
}
#[test]
fn dos_launcher_draft_cancel_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::with_preferences(
        "dos-launcher-settings",
        "[plugins]\nenabled=['dos_launcher']\n",
    )?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x1bu", "Utilities menu", |s| {
        s.text().contains("DOS launcher")
    })?;
    click(&mut s, "DOS launcher", "DOS settings", |s| {
        s.text().contains("DOSBox Staging executable")
    })?;
    s.send(b"/private/does-not-exist/DOSBox", "draft path", |s| {
        s.text().contains("does-not-exist")
    })?;
    s.resize(100, 32)?;
    s.send(b"\x1b", "cancel draft", |s| s.command_prompt_active())?;
    s.send(b"\x1bu", "Utilities reopened", |s| {
        s.text().contains("DOS launcher")
    })?;
    click(&mut s, "DOS launcher", "fresh draft", |s| {
        s.text().contains("DOSBox Staging executable") && !s.text().contains("does-not-exist")
    })?;
    s.send(b"\x1b", "close settings", |s| s.command_prompt_active())?;
    s.finish()
}
#[cfg(feature = "acoustic")]
#[test]
fn acoustic_window_without_device_activation_restores_terminal() -> io::Result<()> {
    let mut s = Session::with_preferences(
        "acoustic-no-devices",
        "[plugins]\nenabled=['acoustic_transfers']\n",
    )?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1bu", "Utilities menu", |s| {
        s.text().contains("Acoustic transfers")
    })?;
    click(&mut s, "Acoustic transfers", "modem idle", |s| {
        s.text().contains("Devices closed") && s.text().contains("File mode")
    })?;
    click(
        &mut s,
        "Use current file",
        "captured tiny file without devices",
        |s| s.text().contains("item-000.txt (") && s.text().contains("Devices closed"),
    )?;
    click(&mut s, "File mode", "return to text mode", |s| {
        !s.text().contains("item-000.txt (") && s.text().contains("Devices closed")
    })?;
    s.send(b"\t\t\t", "text input focused", |s| {
        s.text().contains("Devices closed")
    })?;
    s.resize(100, 32)?;
    s.send(b"https://example.org/test", "text draft", |s| {
        s.text().contains("https://example.org/test")
    })?;
    s.send(b"\x1b", "close without microphone", |s| {
        s.command_prompt_active()
    })?;
    s.finish()
}

#[cfg(target_os = "macos")]
#[test]
fn automatic_dos_launch_passes_literal_paths_and_keeps_manager_focus() -> io::Result<()> {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let temp = Temp(std::env::temp_dir().join(format!(
        "dn-terminal-dos-mock-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&temp.0)?;
    let log = temp.0.join("arguments");
    let executable = temp.0.join("DOSBox Staging Ёжик");
    let quoted = log.to_string_lossy().replace('\'', "'\\''");
    fs::write(&executable,format!("#!/bin/sh\ncase \"$2\" in --version) echo 'DOSBox Staging 0.83.0';; --help) echo '--nolocalconf';; *) printf '%s\\000' \"$@\" > '{quoted}';; esac\n"))?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
    let preferences=format!("[plugins]\nenabled=['dos_launcher']\n[plugins.dos_launcher]\nopen_on_enter=true\nexecutable={}\n",toml::Value::String(executable.to_string_lossy().into_owned()));
    let mut s = Session::with_preferences("dos-literal-launch", &preferences)?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    let source = s.root.join("Ёжик ; $(false).COM");
    fs::write(&source, b"\xb4\x4c\xcd\x21")?;
    let source = fs::canonicalize(source)?;
    s.send(b"\x12", "refresh program", |s| s.command_prompt_active())?;
    s.select_file("Ёжик ; $(false).COM")?;
    let mut expected = b"--nolocalconf\0".to_vec();
    expected.extend(source.to_string_lossy().as_bytes());
    expected.push(0);
    s.send(b"\r", "launch literal DOS path", |s| {
        s.command_prompt_active() && fs::read(&log).is_ok_and(|bytes| bytes == expected)
    })?;
    assert_eq!(fs::read(log)?, expected);
    s.finish()
}
