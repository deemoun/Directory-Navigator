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
        .filter(|(_, line)| line.contains(label))
        .last()
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
        s.text().contains("Emulator executable")
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
        s.text().contains("Emulator executable") && !s.text().contains("does-not-exist")
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
    fs::write(&executable,format!("#!/bin/sh\ncase \"${{2:-$1}}\" in --version|-version) echo 'DOSBox Staging 0.83.0';; --help|-help) echo '--nolocalconf';; *) printf '%s\\000' \"$@\" > '{quoted}';; esac\n"))?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
    let preferences=format!("[plugins]\nenabled=['dos_launcher']\n[plugins.dos_launcher]\nopen_on_enter=true\nexecutable={}\n",toml::Value::String(executable.to_string_lossy().into_owned()));
    let mut s = Session::with_preferences_environment(
        "dos-literal-launch",
        &preferences,
        &[("DISPLAY", ":dn-private-mock")],
    )?;
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

#[test]
fn dos_check_draft_and_failed_launch_show_diagnostics_and_restore_terminal() -> io::Result<()> {
    use std::{fs, os::unix::fs::PermissionsExt};
    let mut s = Session::with_preferences_environment(
        "dos-check-and-failure",
        "[plugins]\nenabled=['dos_launcher']\n",
        &[("DISPLAY", ":dn-private-mock")],
    )?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    let executable = s.root.join("private dosbox-staging");
    fs::write(&executable,"#!/bin/sh\ncase \"${2:-$1}\" in --version|-version) echo 'DOSBox Staging 0.83';; --help|-help) echo '--nolocalconf';; *) echo 'SDL fixture: video initialization failed' >&2; exit 37;; esac\n")?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
    s.send(b"\x1bu", "Utilities", |s| s.text().contains("DOS launcher"))?;
    click(&mut s, "DOS launcher", "DOS path draft", |s| {
        s.text().contains("Check DOSBox")
    })?;
    s.paste(executable.to_str().unwrap(), "type draft tool path", |s| {
        s.text().contains("private dosbox-staging")
    })?;
    click(&mut s, "Check DOSBox", "verify unsaved tool", |s| {
        s.text().contains("DOSBox emulator verified")
    })?;
    s.send(b"\x1b", "return from verification", |s| {
        s.text().contains("Check DOSBox") && !s.text().contains("DOSBox check")
    })?;
    click(&mut s, "Cancel", "discard tool draft", |s| {
        s.command_prompt_active()
    })?;
    s.send(b"\x1bu", "Utilities again", |s| {
        s.text().contains("DOS launcher")
    })?;
    click(&mut s, "DOS launcher", "fresh unsaved draft", |s| {
        s.text().contains("Check DOSBox") && !s.text().contains("private dosbox-staging")
    })?;
    s.paste(executable.to_str().unwrap(), "tool path", |s| {
        s.text().contains("private dosbox-staging")
    })?;
    click(&mut s, "OK", "commit tool path", |s| {
        s.command_prompt_active()
    })?;
    let game = s.root.join("private.COM");
    fs::write(&game, b"\xcd\x20")?;
    s.send(b"\x12", "refresh game", |s| s.command_prompt_active())?;
    s.select_file("private.COM")?;
    s.send(b"\x1bf", "File menu", |s| {
        s.text().contains("Run in DOSBox")
    })?;
    click(&mut s, "Run in DOSBox", "launch failure diagnostics", |s| {
        s.text()
            .contains("SDL fixture: video initialization failed")
            && s.text().contains("37")
    })?;
    s.send(b"\x1b", "dismiss owned launch error", |s| {
        s.command_prompt_active()
    })?;
    s.finish()
}

#[test]
fn dos_installed_emulator_list_selects_x_and_discards_cancelled_drafts() -> io::Result<()> {
    use std::{fs, os::unix::fs::PermissionsExt};
    // Only the child sees the private PATH; no process-global environment changes.
    let mut s = Session::with_preferences(
        "dos-installed-list",
        "[plugins]\nenabled=['dos_launcher']\n",
    )?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    let bin = s.root.join("bin");
    fs::create_dir(&bin)?;
    for (name, banner) in [
        ("dosbox-staging", "DOSBox Staging 0.83"),
        ("dosbox-x", "DOSBox-X version test"),
        ("dosbox", "DOSBox version test"),
    ] {
        let path = bin.join(name);
        fs::write(&path, format!("#!/bin/sh\ncase \"$1\" in -version|--version) echo '{banner}';; -help|--help) echo '--nolocalconf';; *) exit 91;; esac\n"))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    }
    // More discovered tools than fit in four rows must not make this test
    // depend on which entries happen to be initially visible.
    let mut folders = vec![bin.clone()];
    for name in ["extra-one", "extra-two"] {
        let folder = s.root.join(name);
        fs::create_dir(&folder)?;
        let path = folder.join("dosbox-staging");
        fs::copy(bin.join("dosbox-staging"), &path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        folders.push(folder);
    }
    let path = std::env::join_paths(folders).map_err(io::Error::other)?;
    // Restart the private fixture with the tools visible to the child only.
    s.finish()?;
    let mut child = Session::with_preferences_environment(
        "dos-list-child",
        "[plugins]\nenabled=['dos_launcher']\n",
        &[("PATH", path.to_str().unwrap())],
    )?;
    child.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    child.send(b"\x1bu", "Utilities", |s| s.text().contains("DOS launcher"))?;
    click(&mut child, "DOS launcher", "launcher with detection", |s| {
        s.text().contains("Find emulators")
    })?;
    click(
        &mut child,
        "Find emulators",
        "installed emulators found",
        |s| s.text().contains("Select an installed emulator"),
    )?;
    click(
        &mut child,
        "Automatic (first available)",
        "focus emulator list",
        |s| s.text().contains("Installed emulators"),
    )?;
    let receiver = "DOSBox-X · DOSBox-X version test";
    // Discovery retains at most 16 tools. Navigate through the real list instead
    // of assuming a particular number of installed Staging variants.
    for _ in 0..16 {
        if child.text().contains(receiver) {
            break;
        }
        child.send(b"\x1b[B", "scroll installed emulators", |s| {
            s.text().contains("Installed emulators")
        })?;
    }
    assert!(
        child.text().contains(receiver),
        "Private DOSBox-X fixture missing from detected list"
    );
    // Keyboard scrolling selects each row. Move to the following classic
    // fixture so the mouse click below changes selection and commits a frame.
    child.send(b"\x1b[B", "select following emulator", |s| {
        s.text().contains("DOSBox version test")
    })?;
    click(&mut child, receiver, "select DOSBox-X", |s| {
        s.text().contains("/bin/dosbox-x")
    })?;
    click(&mut child, "Cancel", "discard selection", |s| {
        s.command_prompt_active()
    })?;
    child.send(b"\x1bu", "Utilities again", |s| {
        s.text().contains("DOS launcher")
    })?;
    click(
        &mut child,
        "DOS launcher",
        "automatic draft retained",
        |s| s.text().contains("Automatic (first available)"),
    )?;
    // The retained discovery list is session state; the path input remains blank.
    let text = child.text();
    let path_row = text
        .lines()
        .position(|line| line.contains("Emulator executable"))
        .unwrap()
        + 1;
    assert!(!text.lines().nth(path_row).unwrap().contains("dosbox-x"));
    child.send(b"\x1b", "close launcher", |s| s.command_prompt_active())?;
    child.finish()?;
    // Keep the source fixture alive until every child probe has exited.
    drop(s);
    Ok(())
}

#[cfg(feature = "acoustic")]
#[test]
fn simplex_estimate_wav_export_import_and_review_need_no_audio_devices() -> io::Result<()> {
    let mut s =
        Session::with_preferences("simplex-wav", "[plugins]\nenabled=['acoustic_transfers']\n")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x1bu", "Utilities", |s| {
        s.text().contains("Acoustic transfers")
    })?;
    click(&mut s, "Acoustic transfers", "one-way modem", |s| {
        s.text().contains("max 1 MiB")
    })?;
    s.paste("https://example.org/one-way", "text payload", |s| {
        s.text().contains("https://example.org/one-way")
    })?;
    let row = s
        .text()
        .lines()
        .position(|line| line.contains("Shared passphrase"))
        .unwrap()
        + 2;
    s.send(
        format!("\x1b[<0;6;{row}M\x1b[<0;6;{row}m").as_bytes(),
        "password focused",
        |s| s.text().contains("Shared passphrase"),
    )?;
    s.paste("private WAV test passphrase", "masked passphrase", |s| {
        s.text().contains("********") && !s.text().contains("private WAV test passphrase")
    })?;
    click(&mut s, "Speed: Normal", "fast modulation", |s| {
        s.text().contains("Speed: Fast")
    })?;
    click(&mut s, "Speed: Fast", "fastest modulation", |s| {
        s.text().contains("Speed: Fastest")
    })?;
    click(&mut s, "Estimate", "estimate without devices", |s| {
        s.text().contains("Playback estimate ready") && s.text().contains("Estimated playback:")
    })?;
    assert!(s.text().contains("Devices closed"));
    click(&mut s, "Export WAV...", "native WAV save picker", |s| {
        s.text().contains("Save As")
    })?;
    s.send(b"\r", "export recording", |s| {
        s.text()
            .contains("WAV exported. Delivery is not confirmed.")
    })?;
    let recording = s.root.join("acoustic-transfer.wav");
    assert!(std::fs::metadata(&recording)?.len() > 80);
    s.wait("published directory scans complete", |s| {
        std::fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))
            .is_ok_and(|state| state.contains("panels_loading=[false, false]"))
    })?;
    click(&mut s, "Receive WAV...", "native WAV open picker", |s| {
        s.text().contains("Open file")
    })?;
    s.paste("acoustic-transfer.wav", "recording filename", |s| {
        s.text().contains("acoustic-transfer.wav")
    })?;
    s.send(b"\r", "recording decode started", |s| {
        s.text().contains("Preparing one-way acoustic operation")
            || s.text().contains("Verified incoming data:")
    })?;
    // Decoding is background DSP work. Observe actual block milestones rather
    // than requiring the entire recording to decode in one UI-step deadline.
    let mut completed = 0_u64;
    while !s.text().contains("Verified incoming data:") {
        s.wait("decoded acoustic block advances", |s| {
            s.text().contains("Verified incoming data:") || decoded_blocks(&s.text()) > completed
        })?;
        if s.text().contains("Verified incoming data:") {
            break;
        }
        completed = decoded_blocks(&s.text());
        assert!(completed <= 10_000, "unbounded acoustic fixture progress");
    }
    assert!(s.text().contains("Devices closed"));
    assert!(!s.text().contains("Codes match"));
    click(&mut s, "Accept", "retain verified text", |s| {
        s.text().contains("Text received and verified")
    })?;
    click(&mut s, "Preview", "preview received text", |s| {
        s.text().contains("Acoustic text preview")
    })?;
    assert!(s.text().contains("https://example.org/one-way"));
    s.send(b"\x1b", "return from preview", |s| {
        s.text().contains("Text received and verified")
    })?;
    s.resize(100, 32)?;
    s.resize(80, 25)?;
    s.send(b"\x1b", "close modem", |s| s.command_prompt_active())?;
    s.finish()
}

#[cfg(feature = "acoustic")]
fn decoded_blocks(screen: &str) -> u64 {
    screen
        .lines()
        .find_map(|line| {
            line.split_once("Blocks ")
                .and_then(|(_, v)| v.split_whitespace().next())
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}
