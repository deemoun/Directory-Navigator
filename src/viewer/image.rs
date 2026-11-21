//! Bounded portable image decoding and prepared cell rendering; no terminal I/O.
mod filter;
use super::{Action, Contents, Viewer};
use crate::{
    presentation::{
        style::{Color, Style},
        text::{Line, Span, Text},
    },
    task::Cancellation,
};
use ::image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbaImage};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub(crate) use filter::cell_ratio;
use std::{
    io::{self, BufReader, Cursor, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};
pub const MAX_PIXELS: u64 = 32 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 32768;
pub const HELP: &str = "Image viewer: PNG, JPEG and WebP. F2 fits the image; F3 sets one source pixel per column (100%), with aspect correction. F4 rotates clockwise without changing the file. F5/F6 zoom in/out; arrows pan. F8 toggles ASCII for fonts without half-block glyphs. F10/Esc closes. Colored cells are portable, including Linux tty; this is a low-resolution preview, not terminal pixel graphics. Image colors use the existing terminal color conversion. Filtered downscaling reduces aliasing. Proportions use OS-reported cell dimensions when available, otherwise a 1:2 cell estimate. Transparency uses a checkerboard. Input is limited to 64 MiB and 33,554,432 pixels; malformed or oversized images are rejected. Enter associations and Quick View are unchanged.";
pub fn is_path(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        ["png", "jpg", "jpeg", "webp"]
            .iter()
            .any(|e| s.eq_ignore_ascii_case(e))
    })
}
pub fn signature(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xff\xd8\xff")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
}
pub struct Decoded {
    pub pixels: RgbaImage,
    pub format: &'static str,
    mips: Vec<RgbaImage>,
}
struct Input<'a> {
    bytes: Cursor<&'a [u8]>,
    cancel: &'a Cancellation,
}
impl Read for Input<'_> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.cancel.check()?;
        self.bytes.read(b)
    }
}
impl Seek for Input<'_> {
    fn seek(&mut self, s: SeekFrom) -> io::Result<u64> {
        self.cancel.check()?;
        self.bytes.seek(s)
    }
}
pub(super) fn contents(bytes: Vec<u8>, cancel: &Cancellation) -> io::Result<Contents> {
    cancel.check()?;
    if bytes.len() > super::MAX_BYTES {
        return Err(io::Error::other("Image exceeds the 64 MiB input limit."));
    }
    let mut reader = ImageReader::new(BufReader::new(Input {
        bytes: Cursor::new(&bytes),
        cancel,
    }))
    .with_guessed_format()?;
    let format = match reader.format() {
        Some(ImageFormat::Png) => "PNG",
        Some(ImageFormat::Jpeg) => "JPEG",
        Some(ImageFormat::WebP) => "WebP",
        _ => {
            return Err(io::Error::other(
                "Unsupported or corrupt image. Supported formats: PNG, JPEG, WebP.",
            ))
        }
    };
    let mut limits = ::image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(192 * 1024 * 1024);
    reader.limits(limits);
    let fail = |e: ::image::ImageError| io::Error::other(format!("Cannot decode image: {e}"));
    let mut decoder = reader.into_decoder().map_err(fail)?;
    let (w, h) = decoder.dimensions();
    if w == 0
        || h == 0
        || u64::from(w) * u64::from(h) > MAX_PIXELS
        || decoder.total_bytes() > 192 * 1024 * 1024
    {
        return Err(io::Error::other(
            "Image dimensions exceed the viewing limit (33,554,432 pixels / 32768 per side).",
        ));
    }
    let orientation = decoder.orientation().map_err(fail)?;
    cancel.check()?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(fail)?;
    cancel.check()?;
    image.apply_orientation(orientation);
    let mut c = super::text_contents("");
    c.loaded_bytes = bytes.len();
    c.raw = Some(bytes.into());
    c.format = None;
    c.image = Some(Arc::new(Decoded::new(image.into_rgba8(), format, cancel)?));
    Ok(c)
}
// Use the same indexed-color transport as DN, without requiring true-color support.
fn palette(rgb: [u8; 3]) -> Color {
    let levels = [0u8, 95, 135, 175, 215, 255];
    let index: [usize; 3] = std::array::from_fn(|i| {
        (0..6)
            .min_by_key(|j| (i32::from(rgb[i]) - i32::from(levels[*j])).pow(2))
            .unwrap()
    });
    let cube: [u8; 3] = std::array::from_fn(|i| levels[index[i]]);
    let distance = |c: [u8; 3]| {
        (0..3)
            .map(|i| (i32::from(rgb[i]) - i32::from(c[i])).pow(2))
            .sum::<i32>()
    };
    let gray = (0u8..24)
        .min_by_key(|n| distance([8 + *n * 10; 3]))
        .unwrap();
    Color::Indexed(if distance([8 + gray * 10; 3]) < distance(cube) {
        232 + gray
    } else {
        16 + (36 * index[0] + 6 * index[1] + index[2]) as u8
    })
}
#[derive(Default)]
pub struct State {
    pub text: Text<'static>,
    pub turns: u8,
    pub ascii: bool,
    scale: Option<f64>,
    effective: f64,
    ratio: f64,
    pan: (i64, i64),
    cache: Option<(u16, u16)>,
}
impl State {
    pub fn action(&mut self, action: Action) {
        match action {
            Action::ImageFit => {
                self.scale = None;
                self.pan = (0, 0);
            }
            Action::ImageActual => {
                self.scale = Some(1.0);
                self.pan = (0, 0);
            }
            Action::ImageZoomIn => {
                self.scale = Some(
                    (self.scale.unwrap_or(self.effective.max(0.0001)) * 1.5).clamp(0.0001, 16.0),
                )
            }
            Action::ImageZoomOut => {
                self.scale = Some(
                    (self.scale.unwrap_or(self.effective.max(0.0001)) / 1.5).clamp(0.0001, 16.0),
                )
            }
            Action::ImageRotate => {
                self.turns = (self.turns + 1) % 4;
                self.pan = (0, 0);
            }
            Action::ImageAscii => self.ascii = !self.ascii,
            _ => return,
        }
        self.cache = None;
    }
    pub fn pan_key(&mut self, key: KeyEvent) {
        if key.modifiers != KeyModifiers::NONE {
            return;
        }
        let (dx, dy) = match key.code {
            KeyCode::Left => (-4, 0),
            KeyCode::Right => (4, 0),
            KeyCode::Up => (0, -4),
            KeyCode::Down => (0, 4),
            _ => return,
        };
        self.pan.0 = self.pan.0.saturating_add(dx);
        self.pan.1 = self.pan.1.saturating_add(dy);
        self.cache = None;
    }
    fn dimensions(&self, image: &Decoded) -> (u32, u32) {
        if self.turns.is_multiple_of(2) {
            image.pixels.dimensions()
        } else {
            (image.pixels.height(), image.pixels.width())
        }
    }
    fn sample(&self, image: &Decoded, rect: [f64; 4], checker: u8) -> [u8; 3] {
        let [x0, y0, x1, y1] = rect;
        let (w, h) = (
            f64::from(image.pixels.width()),
            f64::from(image.pixels.height()),
        );
        let rect = match self.turns {
            0 => rect,
            1 => [y0, h - x1, y1, h - x0],
            2 => [w - x1, h - y1, w - x0, h - y0],
            _ => [w - y1, x0, w - y0, x1],
        };
        image.filtered(rect, checker)
    }
    fn set_ratio(&mut self, ratio: f64) {
        let ratio = if (0.25..=4.0).contains(&ratio) {
            ratio
        } else {
            1.0
        };
        if self.ratio != ratio {
            self.ratio = ratio;
            self.cache = None;
        }
    }
    fn prepare(&mut self, image: &Decoded, width: u16, height: u16) {
        // Bound layout work even for pathological terminal dimensions.
        let width = width.min(512);
        let height = height.min(256);
        if self.cache == Some((width, height)) {
            return;
        }
        let (iw, ih) = self.dimensions(image);
        let ratio = if self.ratio == 0.0 { 1.0 } else { self.ratio };
        let scale = self
            .scale
            .unwrap_or(
                (f64::from(width) / f64::from(iw))
                    .min(f64::from(height) * 2.0 * ratio / f64::from(ih)),
            )
            .max(0.0001);
        self.effective = scale;
        let sw = (f64::from(iw) * scale).round().max(1.0) as i64;
        let sh = (f64::from(ih) * scale / ratio).round().max(1.0) as i64;
        self.pan.0 = self.pan.0.clamp(0, (sw - i64::from(width)).max(0));
        self.pan.1 = self.pan.1.clamp(0, (sh - i64::from(height) * 2).max(0));
        let left = ((i64::from(width) - sw) / 2).max(0);
        let top = ((i64::from(height) * 2 - sh) / 2).max(0);
        let pixel = |x: i64, y: i64| {
            let sx = x - left + self.pan.0;
            let sy = y - top + self.pan.1;
            if sx < 0 || sy < 0 || sx >= sw || sy >= sh {
                return [0, 0, 0];
            }
            let x0 = sx as f64 * f64::from(iw) / sw as f64;
            let y0 = sy as f64 * f64::from(ih) / sh as f64;
            let x1 = (sx + 1) as f64 * f64::from(iw) / sw as f64;
            let y1 = (sy + 1) as f64 * f64::from(ih) / sh as f64;
            self.sample(
                image,
                [x0, y0, x1, y1],
                if ((x0 as u32) / 8 + (y0 as u32) / 8).is_multiple_of(2) {
                    112
                } else {
                    160
                },
            )
        };
        let mut lines = Vec::with_capacity(height as usize);
        for y in 0..height {
            let mut spans = Vec::with_capacity(width as usize);
            for x in 0..width {
                let a = pixel(i64::from(x), i64::from(y) * 2);
                let b = pixel(i64::from(x), i64::from(y) * 2 + 1);
                let (glyph, fg, bg) = if self.ascii {
                    let brightness = (u32::from(a[0])
                        + u32::from(a[1])
                        + u32::from(a[2])
                        + u32::from(b[0])
                        + u32::from(b[1])
                        + u32::from(b[2]))
                        / 6;
                    (
                        [" ", ".", ":", "-", "=", "+", "*", "#", "%", "@"]
                            [((brightness * 9) / 255) as usize],
                        a,
                        [0, 0, 0],
                    )
                } else {
                    ("▀", a, b)
                };
                spans.push(Span::styled(
                    glyph,
                    Style::default().fg(palette(fg)).bg(palette(bg)),
                ));
            }
            lines.push(Line::from(spans));
        }
        self.text = Text::from(lines);
        self.cache = Some((width, height));
    }
}
impl Viewer {
    pub fn is_image(&self) -> bool {
        self.contents.as_ref().is_some_and(|c| c.image.is_some())
    }
    pub(crate) fn prepare_image(&mut self, width: u16, height: u16, ratio: f64) {
        self.image_view.set_ratio(ratio);
        if let Some(image) = self.contents.as_ref().and_then(|c| c.image.as_ref()) {
            self.image_view.prepare(image, width, height);
        }
    }
    pub(crate) fn image_status(&self, l: crate::localization::Language) -> Option<String> {
        let image = self.contents.as_ref()?.image.as_ref()?;
        Some(format!(
            " {} · {}×{} · {:.2}% · {}° · {}",
            image.format,
            image.pixels.width(),
            image.pixels.height(),
            self.image_view.effective * 100.0,
            u16::from(self.image_view.turns) * 90,
            l.text(if self.image_view.ascii {
                "ASCII preview"
            } else {
                "Colored-cell preview"
            })
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encoded(format: ImageFormat) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(::image::RgbImage::from_fn(4, 2, |x, y| {
            ::image::Rgb([x as u8 * 60, y as u8 * 180, 40])
        }));
        let mut b = Cursor::new(Vec::new());
        image.write_to(&mut b, format).unwrap();
        b.into_inner()
    }
    #[test]
    fn all_three_codecs_are_bounded_and_explicit_hex_keeps_original_bytes() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let bytes = encoded(format);
            assert!(signature(&bytes));
            let c = contents(bytes.clone(), &Cancellation::default()).unwrap();
            assert_eq!(c.image.unwrap().pixels.dimensions(), (4, 2));
            assert_eq!(c.raw.unwrap().as_ref(), bytes);
            let c = super::super::from_bytes(bytes.clone(), super::super::ViewMode::Hex);
            assert!(c.image.is_none());
            assert!(c.binary);
            assert_eq!(c.raw.unwrap().as_ref(), bytes);
        }
        assert!(contents(b"bad image".to_vec(), &Cancellation::default()).is_err());
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(contents(encoded(ImageFormat::Png), &cancel).is_err());
        let mut bomb = encoded(ImageFormat::Png);
        bomb[16..20].copy_from_slice(&32768u32.to_be_bytes());
        bomb[20..24].copy_from_slice(&32768u32.to_be_bytes());
        let crc = crc32fast::hash(&bomb[12..29]);
        bomb[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(contents(bomb, &Cancellation::default()).is_err());
    }
    #[test]
    fn jpeg_exif_orientation_is_applied_before_preparing_cells() {
        let mut bytes = encoded(ImageFormat::Jpeg);
        let mut payload =
            b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".to_vec();
        let mut app1 = vec![0xff, 0xe1];
        app1.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        app1.append(&mut payload);
        bytes.splice(2..2, app1);
        let c = contents(bytes, &Cancellation::default()).unwrap();
        assert_eq!(c.image.unwrap().pixels.dimensions(), (2, 4));
    }
    #[test]
    fn prepared_cells_rotation_pan_ascii_and_repeated_layout_are_stable() {
        let image = Decoded::new(
            RgbaImage::from_fn(2, 2, |x, y| {
                ::image::Rgba([x as u8 * 255, y as u8 * 255, 0, 255])
            }),
            "PNG",
            &Cancellation::default(),
        )
        .unwrap();
        let original = image.pixels.clone();
        let mut state = State::default();
        state.action(Action::ImageActual);
        state.prepare(&image, 2, 1);
        assert_eq!(
            state.text.lines[0].spans[0].style.fg,
            Some(Color::Indexed(16))
        );
        assert_eq!(
            state.text.lines[0].spans[0].style.bg,
            Some(Color::Indexed(46))
        );
        let console = crate::terminal::colors::OutputColors::linux_console();
        for span in &state.text.lines[0].spans {
            for (color, foreground) in [
                (span.style.fg.unwrap(), true),
                (span.style.bg.unwrap(), false),
            ] {
                assert!(matches!(color, Color::Indexed(_)));
                let code = console.parameters(color, foreground).parse::<u8>().unwrap();
                assert!(if foreground {
                    (30..=37).contains(&code) || (90..=97).contains(&code)
                } else {
                    (40..=47).contains(&code)
                });
            }
        }
        let text = state.text.clone();
        state.prepare(&image, 2, 1);
        assert_eq!(state.text, text);
        for _ in 0..4 {
            state.action(Action::ImageRotate);
            state.prepare(&image, 2, 1);
        }
        assert_eq!(state.text, text);
        assert_eq!(image.pixels, original);
        state.action(Action::ImageZoomIn);
        state.pan = (i64::MAX, i64::MAX);
        state.prepare(&image, 1, 1);
        assert!(state.pan.0 < 10);
        state.action(Action::ImageAscii);
        state.prepare(&image, 1, 1);
        assert!(state.text.lines[0].spans[0].content.is_ascii());
        state.action(Action::ImageFit);
        state.prepare(&image, 80, 20);
        assert!(state.scale.is_none());
        assert_eq!(state.pan, (0, 0));
    }
    #[test]
    fn shrinking_checkerboard_averages_pixels_instead_of_skipping_them() {
        let image = Decoded::new(
            RgbaImage::from_fn(128, 128, |x, y| {
                let value = if (x + y).is_multiple_of(2) { 0 } else { 255 };
                ::image::Rgba([value, value, value, 255])
            }),
            "PNG",
            &Cancellation::default(),
        )
        .unwrap();
        let mut state = State::default();
        state.prepare(&image, 2, 1);
        for span in &state.text.lines[0].spans {
            assert_eq!(span.style.fg, Some(Color::Indexed(244)));
            assert_eq!(span.style.bg, Some(Color::Indexed(244)));
        }
    }
    #[test]
    fn transparent_pixels_use_a_checkerboard_and_signatures_are_strict() {
        let image = Decoded::new(RgbaImage::new(1, 1), "PNG", &Cancellation::default()).unwrap();
        let state = State::default();
        assert_eq!(state.sample(&image, [0.0, 0.0, 1.0, 1.0], 112), [112; 3]);
        assert!(!signature(b"RIFF1234WAVE"));
        assert!(is_path(Path::new("Фото.JPEG")));
        assert!(!is_path(Path::new("photo.svg")));
    }
}
