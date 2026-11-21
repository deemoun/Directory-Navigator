//! Portable image viewing in a private PTY; no graphics protocol or external viewer.
#![cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "support/pty.rs"]
pub mod pty;
use pty::Session;
use std::{fs, io};
#[test]
fn portable_image_zoom_rotation_ascii_resize_and_terminal_restoration() -> io::Result<()> {
    let mut s = Session::with_terminal_preferences("image-viewer", "", "linux")?;
    let name = "Ёжик 世界.png";
    image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(64, 32, |x, y| {
        image::Rgb([x as u8 * 3, y as u8 * 7, 40])
    }))
    .save(s.root.join(name))
    .unwrap();
    let original = fs::read(s.root.join(name))?;
    s.wait("manager ready", Session::command_prompt_active)?;
    s.send(b"\x12", "refresh image", Session::command_prompt_active)?;
    s.select_file(name)?;
    s.send(b"\x1b[13~", "image decoded", |s| {
        s.text().contains("PNG · 64×32") && s.text().contains("Colored-cell preview")
    })?;
    s.send(b"\x1b[13~", "actual size", |s| s.text().contains("100.00%"))?;
    s.send(b"\x1b[14~", "rotation", |s| s.text().contains("90°"))?;
    s.send(b"\x1b[15~", "zoom", |s| s.text().contains("150.00%"))?;
    s.send(b"\x1b[C\x1b[B", "pan", |s| s.text().contains("150.00%"))?;
    s.send(b"\x1b[19~", "ASCII fallback", |s| {
        s.text().contains("ASCII preview")
    })?;
    s.resize(110, 32)?;
    s.resize(80, 25)?;
    s.send(b"\x1b[19~", "return to colored cells", |s| {
        s.text().contains("Colored-cell preview")
    })?;
    s.send(b"\x1b", "close image", Session::command_prompt_active)?;
    assert_eq!(fs::read(s.root.join(name))?, original);
    s.finish()
}
