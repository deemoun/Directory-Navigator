//! Portable image controls use the existing viewer document and native composition.
use crossterm::event::KeyCode as K;
use dn::{
    localization::Language,
    mouse::Target,
    palette::PaletteProfile,
    viewer::{self, Action, ViewMode, Viewer},
};
use image::{DynamicImage, ImageFormat, RgbImage};
use std::{
    fs,
    io::{self, Cursor},
};
mod support;
use support::{click, draw, key, wait, Fixture};
#[test]
fn file_formats_modes_and_quick_view_preserve_sources() -> io::Result<()> {
    let fixture = Fixture::new("image-codecs")?;
    for (ext, format) in [
        ("PNG", ImageFormat::Png),
        ("JPEG", ImageFormat::Jpeg),
        ("webp", ImageFormat::WebP),
    ] {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(RgbImage::from_pixel(12, 8, image::Rgb([100, 200, 30])))
            .write_to(&mut bytes, format)
            .unwrap();
        let bytes = bytes.into_inner();
        let path = fixture.0.join(format!("Ёжик 世界.{ext}"));
        fs::write(&path, &bytes)?;
        assert!(viewer::load(&path)?.image.is_some());
        assert!(viewer::load_mode(&path, ViewMode::Hex)?.image.is_none());
        let mut preview = Viewer::open_preview(path.clone())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while preview.contents.is_none() {
            preview.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(!preview.is_image());
        assert_eq!(fs::read(path)?, bytes);
    }
    let path = fixture.0.join("broken.png");
    fs::write(&path, b"not a PNG")?;
    assert!(viewer::load(&path).is_err());
    Ok(())
}
#[test]
fn image_controls_mouse_keyboard_resize_and_inactive_documents_keep_frames_immutable(
) -> io::Result<()> {
    let fixture = Fixture::new("image-controls")?;
    let path = fixture.0.join("Тест.png");
    DynamicImage::ImageRgb8(RgbImage::from_fn(64, 32, |x, y| {
        image::Rgb([x as u8 * 3, y as u8 * 7, 80])
    }))
    .save(&path)
    .unwrap();
    let original = fs::read(&path)?;
    for language in [Language::English, Language::Russian] {
        for palette in [
            PaletteProfile::ClassicDn,
            PaletteProfile::Custom(dn::palette::theme::DEFAULTS),
        ] {
            let mut a = fixture.app()?;
            a.config.language = language;
            a.config.palette = palette;
            a.show_viewer(Viewer::open(path.clone())?);
            wait(&mut a);
            assert!(a.desktop.active_viewer().unwrap().is_image());
            let image_id = a.desktop.target();
            a.show_viewer(Viewer::from_contents(
                "other document".into(),
                viewer::text_contents("Other text"),
            ));
            assert!(!a.desktop.active_viewer().unwrap().is_image());
            draw(&mut a, 80, 25)?;
            a.poll();
            key(&mut a, K::Esc);
            assert_eq!(a.desktop.target(), image_id);

            assert!(!a.desktop.active_viewer().unwrap().encoding_available());
            for (w, h) in [(80, 25), (120, 40), (80, 25)] {
                draw(&mut a, w, h)?;
                let first = support::text(&mut a, w, h)?;
                assert_eq!(first, support::text(&mut a, w, h)?);
                assert!(first.contains("PNG"));
                click(&mut a, Target::Viewer(Action::ImageActual));
                draw(&mut a, w, h)?;
                assert!(support::text(&mut a, w, h)?.contains("100.00%"));
                key(&mut a, K::F(4));
                draw(&mut a, w, h)?;
                assert!(support::text(&mut a, w, h)?.contains("90°"));
                key(&mut a, K::F(5));
                key(&mut a, K::Right);
                key(&mut a, K::Down);
                draw(&mut a, w, h)?;
                key(&mut a, K::F(8));
                assert!(support::text(&mut a, w, h)?.contains("ASCII"));
                key(&mut a, K::F(8));
                key(&mut a, K::F(2));
                // Restore rotation between size checks.
                for angle in [180, 270, 0] {
                    key(&mut a, K::F(4));
                    assert!(support::text(&mut a, w, h)?.contains(&format!("{angle}°")));
                }
            }
            key(&mut a, K::Esc);
            assert!(a.desktop.active_viewer().is_none());
        }
    }
    assert_eq!(fs::read(path)?, original);
    Ok(())
}
