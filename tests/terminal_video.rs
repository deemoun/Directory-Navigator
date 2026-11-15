//! Real terminal scenarios; normal CI never requires installed FFmpeg.
#![cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "support/pty.rs"]
pub mod pty;
use pty::Session;
use std::{io, process::Command};
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
        // Prefer the footer button over an earlier notice mentioning Start.
        .filter(|(_, l)| l.contains(label))
        .last()
        .ok_or_else(|| io::Error::other(format!("Missing {label}: {text}")))?;
    let x = unicode_width::UnicodeWidthStr::width(&line[..line.find(label).unwrap()]) + 1;
    s.send(
        format!("\x1b[<0;{x};{}M\x1b[<0;{x};{}m", row + 1, row + 1).as_bytes(),
        stage,
        ready,
    )
}
#[test]
fn video_settings_cancel_picker_resize_and_terminal_restoration() -> io::Result<()> {
    let mut s =
        Session::with_preferences("video-settings", "[plugins]\nenabled=['video_merge']\n")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x1bu", "Utilities menu", |s| {
        s.text().contains("Video Merge")
    })?;
    click(&mut s, "Video Merge", "video window", |s| {
        s.text().contains("Output MP4")
    })?;
    click(&mut s, "Settings", "basic settings", |s| {
        s.text().contains("Match source resolution")
    })?;
    click(&mut s, "Advanced", "advanced settings", |s| {
        s.text().contains("FFmpeg path")
    })?;
    s.send(b"\x1b", "settings cancelled", |s| {
        s.text().contains("Output MP4") && !s.text().contains("FFmpeg path")
    })?;
    click(&mut s, "Add file", "native file picker", |s| {
        s.text().contains("Open file")
    })?;
    s.send(b"\x1b", "picker cancelled", |s| {
        s.text().contains("Output MP4")
    })?;
    s.resize(100, 32)?;
    s.send(b"\x1b", "video closed", |s| s.command_prompt_active())?;
    s.finish()
}
#[test]
fn video_utilities_capture_panel_marks_and_restore_terminal() -> io::Result<()> {
    let mut s = Session::with_preferences(
        "video-marked-utilities",
        "[plugins]\nenabled=['video_merge']\n",
    )?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    for name in ["a.MP4", "b.MOV"] {
        std::fs::write(s.root.join(name), b"private placeholder")?;
    }
    std::fs::create_dir(s.root.join("marked-folder"))?;
    s.send(b"\x12", "fixtures refreshed", |s| {
        s.command_prompt_active() && s.text().contains("a.MP4")
    })?;
    s.select_file("a.MP4")?;
    s.send(b"\x1b[2~", "video marked", |s| {
        s.text().contains("selected file")
    })?;
    s.select_file("marked-folder")?;
    s.send(b"\x1b[2~", "folder marked", |s| {
        s.text().contains(" + 1 folder")
    })?;
    s.send(b"\x1bu", "Utilities menu", |s| {
        s.text().contains("Video Merge")
    })?;
    click(&mut s, "Video Merge", "marked sources captured", |s| {
        let text = s.text();
        let lines = text.lines().collect::<Vec<_>>();
        let Some(i) = lines
            .iter()
            .position(|l| l.contains("Clips in merge order"))
        else {
            return false;
        };
        let list = lines
            .iter()
            .skip(i + 1)
            .take(5)
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        list.contains("a.MP4") && list.contains("marked-folder") && !list.contains("b.MOV")
    })?;
    s.send(b"\x1b", "video closed", |s| s.command_prompt_active())?;
    s.finish()
}
#[test]
#[ignore = "requires installed FFmpeg with libx264/libx265; private generated media only"]
fn real_video_analysis_merge_and_terminal_restoration() -> io::Result<()> {
    let mut s =
        Session::with_preferences("video-real-merge", "[plugins]\nenabled=['video_merge']\n")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    let folder = s.root.join("clips-Ёжик");
    std::fs::create_dir(&folder)?;
    let path = folder.join("clip-Ёжик.mp4");
    let out = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=160x90:r=30000/1001:d=0.4",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(&path)
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ));
    }
    let hdr = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=gray:s=160x90:r=25:d=0.4",
            "-vf",
            "format=yuv420p10le",
            "-c:v",
            "libx265",
            "-x265-params",
            "pools=1:frame-threads=1:colorprim=9:colormatrix=9:transfer=18",
            "-movflags",
            "+write_colr",
        ])
        .arg(folder.join("phone-HDR.MOV"))
        .output()?;
    if !hdr.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&hdr.stderr).into_owned(),
        ));
    }
    s.send(b"\x12", "generated fixture refreshed", |s| {
        s.command_prompt_active() && s.text().contains("clips-")
    })?;
    s.select_file("clips-Ёжик")?;
    s.send(b"\x1bf", "File menu", |s| s.text().contains("Merge videos"))?;
    click(&mut s, "Merge videos", "captured clip", |s| {
        s.text().contains("Output MP4") && s.text().contains("clips-Ёжик")
    })?;
    click(&mut s, "Settings", "processing settings", |s| {
        s.text().contains("Match source resolution")
    })?;
    s.wait("encoder discovery without Analyze", |s| {
        s.text().contains("Encoder detection complete")
    })?;
    click(&mut s, "Processing:", "processing list", |s| {
        s.text().contains("Software H.264")
    })?;
    let encoder = if cfg!(target_os = "macos") {
        "Apple hardware H.264"
    } else {
        "Software H.264"
    };
    click(&mut s, encoder, "selected encoder", |s| {
        s.text().contains("Match source resolution")
    })?;
    click(&mut s, "OK", "settings applied", |s| {
        s.text().contains("Output MP4")
    })?;
    click(&mut s, "Analyze", "analyzed", |s| {
        s.text().contains("Ready to merge")
    })?;
    click(&mut s, "Test speed", "private speed comparison", |s| {
        s.text().contains("conversion only")
    })?;
    click(&mut s, "Start", "published", |s| {
        s.text().contains("Video published")
    })?;
    assert!(s.root.join("merged_video.mp4").metadata()?.len() > 0);
    assert!(s.text().contains("Completed in"));
    s.send(b"\x1b", "video closed", |s| s.command_prompt_active())?;
    s.finish()
}
