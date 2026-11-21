//! Cancellable, alpha-aware mipmaps prepared by the existing decoding worker.
//! Viewport work then touches a bounded number of samples per terminal cell.
use super::*;
impl Decoded {
    pub(super) fn new(
        pixels: RgbaImage,
        format: &'static str,
        cancel: &Cancellation,
    ) -> io::Result<Self> {
        let mut mips: Vec<RgbaImage> = Vec::new();
        let (base_w, base_h) = pixels.dimensions();
        let mut factor = 1u32;
        loop {
            cancel.check()?;
            let source = mips.last().unwrap_or(&pixels);
            let (w, h) = source.dimensions();
            if w <= 1 && h <= 1 {
                break;
            }
            let mut next = RgbaImage::new(w.div_ceil(2), h.div_ceil(2));
            for y in 0..next.height() {
                cancel.check()?;
                for x in 0..next.width() {
                    let mut sums = [0u64; 4];
                    let mut area = 0u64;
                    for sy in y * 2..(y * 2 + 2).min(h) {
                        for sx in x * 2..(x * 2 + 2).min(w) {
                            // Odd edge pixels cover less original image area.
                            let weight = u64::from(factor.min(base_w - sx * factor))
                                * u64::from(factor.min(base_h - sy * factor));
                            let p = source.get_pixel(sx, sy).0;
                            area += weight;
                            sums[3] += u64::from(p[3]) * weight;
                            for i in 0..3 {
                                sums[i] += u64::from(p[i]) * u64::from(p[3]) * weight;
                            }
                        }
                    }
                    let mut rgba = [0; 4];
                    for i in 0..3 {
                        rgba[i] = (sums[i] + sums[3] / 2).checked_div(sums[3]).unwrap_or(0) as u8;
                    }
                    rgba[3] = ((sums[3] + area / 2) / area) as u8;
                    next.put_pixel(x, y, ::image::Rgba(rgba));
                }
            }
            mips.push(next);
            factor *= 2;
        }
        Ok(Self {
            pixels,
            format,
            mips,
        })
    }
    fn level(&self, index: usize) -> &RgbaImage {
        if index == 0 {
            &self.pixels
        } else {
            &self.mips[index - 1]
        }
    }
    pub(super) fn filtered(&self, rect: [f64; 4], checker: u8) -> [u8; 3] {
        let [x0, y0, x1, y1] = rect;
        let footprint = (x1 - x0).min(y1 - y0);
        let mut index = 0;
        let mut factor = 1.0;
        while index < self.mips.len() && factor * 2.0 <= footprint {
            index += 1;
            factor *= 2.0;
        }
        let source = self.level(index);
        let (x0, y0, x1, y1) = (x0 / factor, y0 / factor, x1 / factor, y1 / factor);
        let mut area = 0.0;
        let mut alpha = 0.0;
        let mut sums = [0.0; 3];
        for y in y0.floor() as u32..(y1.ceil() as u32).min(source.height()) {
            for x in x0.floor() as u32..(x1.ceil() as u32).min(source.width()) {
                let weight = (x1.min(f64::from(x + 1)) - x0.max(f64::from(x))).max(0.0)
                    * (y1.min(f64::from(y + 1)) - y0.max(f64::from(y))).max(0.0);
                let p = source.get_pixel(x, y).0;
                area += weight;
                alpha += f64::from(p[3]) * weight;
                for i in 0..3 {
                    sums[i] += f64::from(p[i]) * f64::from(p[3]) * weight;
                }
            }
        }
        if area <= 0.0 {
            return [checker; 3];
        }
        std::array::from_fn(|i| {
            ((sums[i] + f64::from(checker) * (255.0 * area - alpha)) / (255.0 * area))
                .round()
                .clamp(0.0, 255.0) as u8
        })
    }
}
/// Physical height of a half-cell sample relative to its width. No terminal query.
pub(crate) fn cell_ratio(size: Option<crossterm::terminal::WindowSize>) -> f64 {
    let Some(size) = size.filter(|s| s.columns > 0 && s.rows > 0 && s.width > 0 && s.height > 0)
    else {
        return 1.0;
    };
    let ratio = f64::from(size.height) * f64::from(size.columns)
        / (2.0 * f64::from(size.width) * f64::from(size.rows));
    if (0.25..=4.0).contains(&ratio) {
        ratio
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn odd_edges_and_transparency_are_area_weighted_without_color_halos() {
        let image = Decoded::new(
            RgbaImage::from_fn(3, 1, |x, _| {
                if x == 0 {
                    ::image::Rgba([255, 0, 0, 255])
                } else {
                    ::image::Rgba([0, 0, 255, 255])
                }
            }),
            "PNG",
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(image.filtered([0.0, 0.0, 3.0, 1.0], 0), [85, 0, 170]);
        let image = Decoded::new(
            RgbaImage::from_fn(2, 2, |x, _| {
                if x == 0 {
                    ::image::Rgba([255, 0, 0, 0])
                } else {
                    ::image::Rgba([0, 0, 255, 255])
                }
            }),
            "PNG",
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(image.filtered([0.0, 0.0, 2.0, 2.0], 0), [0, 0, 128]);
        let odd = Decoded::new(
            RgbaImage::from_fn(3, 3, |x, y| {
                if x == 0 && y == 0 {
                    ::image::Rgba([255, 0, 0, 255])
                } else {
                    ::image::Rgba([0, 0, 255, 255])
                }
            }),
            "PNG",
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(odd.filtered([0.0, 0.0, 3.0, 3.0], 0), [28, 0, 227]);
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(Decoded::new(RgbaImage::new(4, 4), "PNG", &cancel).is_err());
        assert!(image.mips.len() < 4);
    }
    #[test]
    fn physical_cell_measurements_are_validated_and_invalidate_prepared_layout() {
        let size = |width, height| {
            Some(crossterm::terminal::WindowSize {
                columns: 80,
                rows: 25,
                width,
                height,
            })
        };
        assert_eq!(cell_ratio(size(800, 500)), 1.0);
        assert_eq!(cell_ratio(size(800, 750)), 1.5);
        assert_eq!(cell_ratio(size(0, 750)), 1.0);
        assert_eq!(cell_ratio(size(1, 65000)), 1.0);
        assert_eq!(cell_ratio(None), 1.0);
        let image = Decoded::new(
            RgbaImage::from_pixel(8, 8, ::image::Rgba([255; 4])),
            "PNG",
            &Cancellation::default(),
        )
        .unwrap();
        let mut state = State::default();
        state.prepare(&image, 8, 4);
        let original = state.text.clone();
        state.set_ratio(2.0);
        state.prepare(&image, 8, 4);
        assert_ne!(state.text, original);
        let measured = state.text.clone();
        state.prepare(&image, 8, 4);
        assert_eq!(state.text, measured);
        state.set_ratio(1.0);
        state.prepare(&image, 8, 4);
        assert_eq!(state.text, original);
    }
}
