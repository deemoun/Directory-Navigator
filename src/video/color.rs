//! Portable HDR-to-SDR lookup tables for FFmpeg's built-in lut3d filter.
//! No zscale/libzimg, GPU, audio device or downloaded color profiles required.
use std::{
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
    sync::atomic::AtomicBool,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Transfer {
    Pq,
    Hlg,
}
impl Transfer {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "smpte2084" => Some(Self::Pq),
            "arib-std-b67" => Some(Self::Hlg),
            _ => None,
        }
    }
    pub fn filename(self) -> &'static str {
        match self {
            Self::Pq => "hdr-pq.cube",
            Self::Hlg => "hdr-hlg.cube",
        }
    }
}
fn pq(value: f64) -> f64 {
    // ST 2084 inverse EOTF, absolute luminance in cd/m².
    let p = value.clamp(0.0, 1.0).powf(32.0 / 2523.0);
    ((p - 3424.0 / 4096.0).max(0.0) / (2413.0 / 128.0 - 2392.0 / 128.0 * p)).powf(16384.0 / 2610.0)
        * 10000.0
}
fn hlg(value: f64) -> f64 {
    // ARIB STD-B67 inverse OETF; the OOTF is applied to RGB luminance below.
    let a: f64 = 0.17883277;
    let b = 1.0 - 4.0 * a;
    let c = 0.5 - a * (4.0 * a).ln();
    if value <= 0.5 {
        value * value / 3.0
    } else {
        (((value - c) / a).exp() + b) / 12.0
    }
}
fn bt709(value: f64) -> f64 {
    let v = value.clamp(0.0, 1.0);
    if v < 0.018 {
        4.5 * v
    } else {
        1.099 * v.powf(0.45) - 0.099
    }
}
fn convert(input: [f64; 3], transfer: Transfer) -> [f64; 3] {
    let mut rgb = input.map(|v| match transfer {
        Transfer::Pq => pq(v),
        Transfer::Hlg => hlg(v),
    });
    let mut y = 0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2];
    if transfer == Transfer::Hlg {
        // Reference 1000-nit HLG display with system gamma 1.2.
        let ootf = 1000.0 * y.max(0.0).powf(0.2);
        rgb = rgb.map(|v| v * ootf);
        y *= ootf;
    }
    if y <= 1e-12 {
        return [0.0; 3];
    }
    // Luminance-preserving Reinhard compression. 203-nit diffuse white leaves
    // room for highlights; 1000 nits maps to SDR peak white.
    let white = 1000.0 / 203.0;
    let normalized = y / 203.0;
    let mapped = normalized / (1.0 + normalized) / (white / (1.0 + white));
    let scale = mapped / y;
    // Linear BT.2020 -> BT.709/D65, followed by BT.709's display encoding.
    [
        1.660491 * rgb[0] - 0.587641 * rgb[1] - 0.072850 * rgb[2],
        -0.124550 * rgb[0] + 1.132900 * rgb[1] - 0.008350 * rgb[2],
        -0.018151 * rgb[0] - 0.100579 * rgb[1] + 1.118730 * rgb[2],
    ]
    .map(|v| bt709(v * scale))
}
pub(super) fn write(directory: &Path, transfer: Transfer, cancel: &AtomicBool) -> io::Result<()> {
    let mut file = BufWriter::new(File::create_new(directory.join(transfer.filename()))?);
    const SIZE: usize = 33;
    writeln!(
        file,
        "TITLE \"DN HDR to SDR BT709\"\nLUT_3D_SIZE {SIZE}\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1"
    )?;
    // IRIDAS Cube order: red changes fastest. Stream instead of retaining a LUT
    // for every clip. The private workspace is deleted with the job.
    for b in 0..SIZE {
        super::engine::check(cancel)?;
        for g in 0..SIZE {
            for r in 0..SIZE {
                let rgb = convert([r, g, b].map(|v| v as f64 / (SIZE - 1) as f64), transfer);
                writeln!(file, "{:.8} {:.8} {:.8}", rgb[0], rgb[1], rgb[2])?;
            }
        }
    }
    file.flush()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hdr_curves_have_finite_neutral_monotonic_output() {
        assert!(pq(0.0) < 1e-8);
        assert!((pq(1.0) - 10000.0).abs() < 0.001);
        assert!((pq(0.50807842) - 100.0).abs() < 0.001);
        for transfer in [Transfer::Pq, Transfer::Hlg] {
            let mut previous = 0.0;
            for i in 0..=1000 {
                let rgb = convert([f64::from(i) / 1000.0; 3], transfer);
                assert!(rgb.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
                assert!((rgb[0] - rgb[1]).abs() < 1e-5 && (rgb[1] - rgb[2]).abs() < 1e-5);
                assert!(rgb[0] >= previous);
                previous = rgb[0];
            }
            assert!(convert([0.75; 3], transfer)[0] > 0.5);
            let red = convert([0.75, 0.1, 0.1], transfer);
            assert!(red[0] > red[1] && red[0] > red[2]);
        }
    }
}
