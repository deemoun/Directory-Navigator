//! Private restart and convenience-action walkthroughs; full-test workflow only.
#![cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "support/pty.rs"]
pub mod pty;
use pty::Session;
use std::{fs, io};
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
    let column = unicode_width::UnicodeWidthStr::width(&line[..line.find(label).unwrap()]) + 2;
    let y = row + 1;
    s.send(
        format!("\x1b[<0;{column};{y}M\x1b[<0;{column};{y}m").as_bytes(),
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
fn history_restart_recall_clear_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::with_preferences("history-restart", "save_command_history=true\n")?;
    s.wait("manager ready", Session::command_prompt_active)?;
    s.paste("cd inside", "accepted command draft", |s| {
        s.text().contains("cd inside")
    })?;
    s.send(b"\r", "record command", Session::command_prompt_active)?;
    s.wait("history published", |s| {
        fs::read_to_string(s.root.join("settings/dn/command-history.toml"))
            .is_ok_and(|text| text.contains("cd inside"))
    })?;
    s.finish()?;
    let mut s = s.restart(true)?;
    s.wait("restarted manager", Session::command_prompt_active)?;
    s.send(b"\x1b[19;3~", "history restored", |s| {
        s.text().contains("Commands History") && s.text().contains("cd inside")
    })?;
    s.resize(110, 32)?;
    s.resize(80, 25)?;
    click(&mut s, "Clear history", "clear confirmation", |s| {
        s.text().contains("Clear session and saved")
    })?;
    s.send(b"\x1b", "cancel clear", |s| {
        s.text().contains("Commands History") && !s.text().contains("Clear session and saved")
    })?;
    click(
        &mut s,
        "Use",
        "recall without executing",
        Session::manager_document_ready,
    )?;
    // The retained history window may cover the prompt; close it before inspecting.
    click(&mut s, "Commands History", "activate history", |s| {
        fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))
            .is_ok_and(|state| state.contains("focus=CommandHistory"))
    })?;
    s.send(b"\x1b", "close history", |s| {
        s.command_prompt_active() && s.text().contains("cd inside")
    })?;
    s.send(
        b"\x1b",
        "clear recalled draft",
        Session::command_prompt_active,
    )?;
    s.finish()
}
#[test]
fn completed_transfer_destination_and_details_restore_terminal() -> io::Result<()> {
    let mut s = Session::new("transfer-convenience")?;
    s.wait("manager ready", Session::command_prompt_active)?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1b[15~", "copy dialog", |s| {
        s.text().contains("Copy file item-000.txt to")
    })?;
    let target = s.root.join("inside").to_string_lossy().into_owned();
    // Ctrl+A clears the shared destination field.
    s.send(b"\x01", "clear destination", |s| {
        s.text().contains("Copy file item-000.txt to")
    })?;
    s.paste(&target, "destination", |s| s.text().contains("inside"))?;
    s.send(b"\r", "completed copy", |s| {
        s.text().contains("Transfer Center")
            && s.root.join("inside/item-000.txt").exists()
            && s.text().contains("Completed")
    })?;
    click(&mut s, "Copy details", "copy retained details", |s| {
        s.text().contains("Transfer Center")
    })?;
    click(
        &mut s,
        "Open destination",
        "open copied directory",
        Session::manager_document_ready,
    )?;
    click(&mut s, "Transfer Center", "reactivate transfers", |s| {
        fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))
            .is_ok_and(|state| state.contains("focus=Transfers"))
    })?;
    s.send(
        b"\x1b",
        "close retained transfers",
        Session::command_prompt_active,
    )?;
    s.select_file("item-000.txt")?;
    s.send(b"\x1b[13~", "view copied file", |s| {
        s.text().contains("item-000")
    })?;
    s.send(
        b"\x1b",
        "return from viewer",
        Session::command_prompt_active,
    )?;
    s.finish()
}

#[test]
fn file_types_filter_multiselect_cancel_resize_and_terminal_restoration() -> io::Result<()> {
    // These fixtures use single-cell ASCII names; at 80 columns the originating
    // pane occupies the left 40 cells. The opposite pane must remain unfiltered.
    fn left_listing(s: &Session) -> String {
        s.text()
            .lines()
            .skip(2)
            .take(16)
            .map(|line| line.chars().take(40).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
    let mut s = Session::new("file-types-filter")?;
    s.wait("manager ready", Session::command_prompt_active)?;
    fs::write(s.root.join("alpha.jpg"), b"private fixture")?;
    fs::write(s.root.join("beta.png"), b"private fixture")?;
    s.send(b"\x12", "refresh private types", |s| {
        s.text().contains("beta.png")
            && fs::read_to_string(s.root.join("settings/dn/diagnostics.txt")).is_ok_and(|state| {
                state
                    .lines()
                    .any(|line| line == "panels_loading=[false, false]")
            })
    })?;
    s.send(b"\x1b[3;3~", "filename filter", |s| {
        s.text().contains("Filename filter")
    })?;
    click(&mut s, "Filter mode", "file type mode", |s| {
        s.text().contains("Show selected")
    })?;
    click(&mut s, ".png (", "select PNG", |s| {
        s.text().contains("[x] .png")
    })?;
    click(&mut s, ".jpg (", "select JPEG", |s| {
        s.text().contains("[x] .jpg")
    })?;
    s.resize(110, 32)?;
    s.resize(80, 25)?;
    click(&mut s, "Apply", "apply types", |s| {
        s.command_prompt_active() && !s.text().contains("Filename filter")
    })?;
    let screen = left_listing(&s);
    assert!(screen.contains("alpha.jpg") && screen.contains("beta.png"));
    assert!(!screen.contains("item-000"));
    assert!(s.text().contains("item-000")); // opposite pane is unchanged.
    s.send(b"\x1b[3;3~", "reopen type filter", |s| {
        s.text().contains("[x] .png")
    })?;
    click(&mut s, "Clear", "clear draft only", |s| {
        s.text().contains("Wildcard")
    })?;
    s.send(b"\x1b", "cancel draft", |s| {
        s.command_prompt_active() && !s.text().contains("Filename filter")
    })?;
    assert!(!left_listing(&s).contains("item-000"));
    s.send(b"\x1b[3;3~", "reopen retained types", |s| {
        s.text().contains("[x] .png")
    })?;
    click(&mut s, "Clear", "reset draft", |s| {
        s.text().contains("Wildcard")
    })?;
    click(&mut s, "Apply", "clear filter", |s| {
        s.command_prompt_active() && left_listing(s).contains("item-000")
    })?;
    s.finish()
}
