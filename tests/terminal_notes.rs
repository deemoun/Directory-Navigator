//! Optional Note Trainer UI; never presses Listen or opens a real microphone.
#![cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "support/pty.rs"]
pub mod pty;
use pty::Session;
use std::{fs, io};
fn plugin_ready(s: &Session) -> bool {
    fs::read_to_string(s.root.join("settings/dn/diagnostics.txt")).is_ok_and(|state| {
        state
            .lines()
            .next()
            .is_some_and(|line| line.contains("focus=Plugin"))
    })
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
        .find(|(_, line)| line.contains(label))
        .ok_or_else(|| io::Error::other(format!("Missing {label}: {text}")))?;
    let x = unicode_width::UnicodeWidthStr::width(&line[..line.find(label).unwrap()]) + 2;
    let y = y + 1;
    s.send(
        format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").as_bytes(),
        stage,
        |s| {
            ready(s)
                && fs::read_to_string(s.root.join("settings/dn/diagnostics.txt")).is_ok_and(
                    |state| state.contains("button=none") && state.contains("capture=None "),
                )
        },
    )
}
#[test]
fn note_board_practice_help_focus_resize_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::with_preferences(
        "note-trainer-board",
        "[plugins]\nenabled=['note_trainer']\n[interface]\nshow_clock=false\n",
    )?;
    s.wait("manager ready", Session::command_prompt_active)?;
    s.send(b"\x1bu", "Utilities menu", |s| {
        s.text().contains("Note Trainer")
    })?;
    click(&mut s, "Note Trainer", "note board", |s| {
        plugin_ready(s) && s.text().contains("Mode: Live")
    })?;
    assert!(!s.text().contains("Opening microphone"));
    click(&mut s, "Mode: Live", "practice mode", |s| {
        s.text().contains("Play: C4")
    })?;
    click(&mut s, "Next note", "next challenge", |s| {
        s.text().contains("Play: E4")
    })?;
    click(&mut s, "C4", "manual C4 target", |s| {
        s.text().contains("Play: C4")
    })?;
    s.send(b"\x1bOP", "note help", |s| {
        s.text().contains("Note Trainer Help")
    })?;
    s.send(b"\x1b", "close owned help", plugin_ready)?;
    s.resize(45, 16)?;
    s.wait("small board guidance", |s| {
        s.text().contains("Terminal too small")
    })?;
    s.resize(100, 32)?;
    s.resize(80, 25)?;
    s.send(
        b"\x1b[17;3~",
        "switch to manager",
        Session::manager_document_ready,
    )?;
    click(&mut s, "Note Trainer", "return to board", plugin_ready)?;
    click(
        &mut s,
        "Close",
        "close note board",
        Session::manager_document_ready,
    )?;
    s.finish()
}
