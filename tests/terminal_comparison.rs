//! Real terminal scenarios for the read-only comparison and plugin diagnostics.
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
        ready,
    )
}
#[test]
fn comparison_report_navigation_resize_and_restoration() -> io::Result<()> {
    let mut s = Session::new("comparison-report")?;
    fs::write(
        s.root.join("inside/unique.txt"),
        b"private comparison fixture",
    )?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x0c", "go to directory", |s| {
        s.text().contains("Go to directory")
    })?;
    let target = s.root.join("inside").to_string_lossy().into_owned();
    s.send(
        format!("\x1b[200~{target}\x1b[201~").as_bytes(),
        "directory input",
        |s| s.text().contains("inside"),
    )?;
    s.send(b"\r", "inside left panel", |s| {
        s.command_prompt_active() && s.text().contains("unique.txt")
    })?;
    s.send(b"\x03", "quick comparison", |s| {
        s.text().contains("Compare size")
    })?;
    click(&mut s, "Report…", "detailed report", |s| {
        s.text().contains("Directory comparison report")
    })?;
    s.wait("comparison complete", |s| {
        s.text().contains("Comparison complete") && s.text().contains("Left only")
    })?;
    s.resize(110, 32)?;
    s.resize(80, 25)?;
    s.send(b"\x1b[F", "last result", |s| {
        s.text().contains("unique.txt")
    })?;
    click(&mut s, "Open left", "result navigation", |s| {
        // A retained floating report may cover the command cursor after resize.
        // Its visibility is not evidence of which document owns keyboard input.
        s.manager_document_ready()
            && fs::read_to_string(s.root.join("settings/dn/diagnostics.txt"))
                .is_ok_and(|state| state.contains("panels_loading=[false, false]"))
    })?;
    click(
        &mut s,
        "Directory comparison report",
        "reactivate retained report",
        |s| {
            fs::read_to_string(s.root.join("settings/dn/diagnostics.txt")).is_ok_and(|state| {
                state.contains(" focus=FileTools ")
                    && state.contains("capture=None ")
                    && state.contains("button=none")
            })
        },
    )?;
    s.send(b"\x1b", "close retained report", |s| {
        s.command_prompt_active() && !s.text().contains("Directory comparison report")
    })?;
    s.send(b"\x1b[13~", "navigated result opens in viewer", |s| {
        s.text().contains("private comparison fixture")
    })?;
    s.send(b"\x1b", "return from result viewer", |s| {
        s.command_prompt_active()
    })?;
    assert_eq!(
        fs::read(s.root.join("inside/unique.txt"))?,
        b"private comparison fixture"
    );
    s.finish()
}
#[test]
fn plugin_diagnostics_refresh_scroll_and_restoration() -> io::Result<()> {
    let mut s=Session::with_preferences("plugin-diagnostics","[plugins.dos_launcher]\nexecutable='missing-dn-emulator-test'\n[plugins.video_merge]\nffmpeg='missing-dn-ffmpeg-test'\nffprobe='missing-dn-ffprobe-test'\n")?;
    s.wait("manager ready", |s| s.command_prompt_active())?;
    s.send(b"\x1bu", "Utilities", |s| {
        s.text().contains("Plugin diagnostics")
    })?;
    click(&mut s, "Plugin diagnostics", "diagnostics", |s| {
        s.text().contains("Plugin diagnostics") && !s.text().contains("Loading native information")
    })?;
    s.wait("plugin information snapshot", |s| {
        s.text().contains("Music playback")
    })?;
    s.send(b"\x1b[F", "tool paths and setup help", |s| {
        s.text().contains("Setup help")
    })?;
    s.send(b"\x1b[15~", "refresh diagnostics", |s| {
        s.text().contains("Loading native information") || s.text().contains("Checks:")
    })?;
    s.wait("refreshed snapshot", |s| {
        s.text().contains("Music playback")
    })?;
    s.resize(110, 32)?;
    s.resize(80, 25)?;
    s.send(b"\x1b", "close diagnostics", |s| s.command_prompt_active())?;
    s.finish()
}
