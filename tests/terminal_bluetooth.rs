//! Bluetooth UI walkthroughs do not require or enable a Bluetooth adapter.
#![cfg(any(target_os = "macos", target_os = "linux"))]
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
    let (y, line) = text
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains(label) && !l.contains("F1 "))
        .last()
        .ok_or_else(|| io::Error::other(format!("Missing {label}: {text}")))?;
    let x = unicode_width::UnicodeWidthStr::width(&line[..line.find(label).unwrap()]) + 1;
    s.send(
        format!("\x1b[<0;{x};{}M\x1b[<0;{x};{}m", y + 1, y + 1).as_bytes(),
        stage,
        ready,
    )
}
#[test]
fn bluetooth_queue_help_resize_and_close_restore_terminal_without_radio_use() -> io::Result<()> {
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        let preferences = format!("[regional]\nlanguage='{}'\n", language.key());
        let mut s = Session::with_preferences("bluetooth-queue", &preferences)?;
        s.wait("manager ready", |s| {
            s.command_prompt_active() && s.text().contains("item-000.")
        })?;
        s.select_file_with_language("item-000.txt", language)?;
        s.send(b"\x1b[2~", "mark file", |s| {
            s.text().contains(language.text("selected file"))
                || s.text().contains(language.text("Selected"))
        })?;
        s.send(b"\x1bu", "Utilities", |s| {
            s.text().contains(language.text("Bluetooth transfers..."))
        })?;
        click(
            &mut s,
            language.text("Bluetooth transfers..."),
            "Bluetooth queue",
            |s| s.text().contains(language.text("Bluetooth queue captured")),
        )?;
        assert!(s.text().contains("item-000.txt"));
        click(&mut s, language.text("Help"), "Bluetooth help", |s| {
            s.text().contains("OBEX")
        })?;
        s.send(b"\x1b", "return from help", |s| {
            s.text().contains(language.text("Files in send order"))
        })?;
        s.resize(110, 35)?;
        s.resize(80, 25)?;
        click(&mut s, language.text("Remove"), "remove queued file", |s| {
            !s.text().contains("item-000.txt ·")
        })?;
        s.send(b"\x1b", "close Bluetooth", |s| s.command_prompt_active())?;
        s.finish()?;
    }
    Ok(())
}
