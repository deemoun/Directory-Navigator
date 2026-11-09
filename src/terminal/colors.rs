//! Output-only color conversion; logical palettes and saved colors stay intact.
use crate::presentation::style::Color;

#[derive(Clone, Copy, Default)]
pub(crate) struct OutputColors {
    console: bool,
}

impl OutputColors {
    pub(crate) fn is_console(self) -> bool {
        self.console
    }
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn linux_console() -> Self {
        Self { console: true }
    }

    pub(crate) fn parameters(self, color: Color, foreground: bool) -> String {
        if color == Color::Reset {
            return if foreground { "39" } else { "49" }.into();
        }
        let kind = if foreground { 38 } else { 48 };
        if !self.console {
            return match color {
                Color::Indexed(n) => format!("{kind};5;{n}"),
                Color::Rgb(r, g, b) => format!("{kind};2;{r};{g};{b}"),
                _ => format!("{kind};5;{}", named_index(color)),
            };
        }
        let index = self.index(color, foreground);
        let code = if foreground {
            if index < 8 {
                30 + index
            } else {
                90 + index - 8
            }
        } else if index == 8 {
            // Bright black is DN's normal dark-gray pane surface. Dropping its
            // intensity would recreate the reported black-background bug.
            47
        } else {
            40 + (index & 7)
        };
        code.to_string()
    }

    pub(crate) fn foreground(self, foreground: Color, background: Color) -> String {
        if self.console
            && foreground != Color::Reset
            && background != Color::Reset
            && self.index(foreground, true) == 7
            && self.index(background, false) == 8
        {
            // The two distinct VGA grays collapse on the console. Preserve text
            // contrast on the substituted pane surface using bright white.
            "97".into()
        } else {
            self.parameters(foreground, true)
        }
    }

    fn index(self, color: Color, foreground: bool) -> u8 {
        match color {
            Color::Indexed(n) => {
                // Recover DN's intended VGA color before quantizing arbitrary RGB.
                if let Some(dos) = crate::palette::CLASSIC.iter().position(|v| *v == n) {
                    crate::palette::TERMINAL[dos]
                } else if n < 16 {
                    n
                } else {
                    nearest(indexed_rgb(n), foreground)
                }
            }
            Color::Rgb(r, g, b) => nearest([r, g, b], foreground),
            _ => named_index(color),
        }
    }
}

fn named_index(color: Color) -> u8 {
    match color {
        Color::Black | Color::Reset => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Indexed(n) => n,
        Color::Rgb(..) => unreachable!("RGB is handled before named colors"),
    }
}

// Conventional VGA colors, in ANSI order. No console palette is changed.
const ANSI: [[u8; 3]; 16] = [
    [0, 0, 0],
    [170, 0, 0],
    [0, 170, 0],
    [170, 85, 0],
    [0, 0, 170],
    [170, 0, 170],
    [0, 170, 170],
    [170, 170, 170],
    [85, 85, 85],
    [255, 85, 85],
    [85, 255, 85],
    [255, 255, 85],
    [85, 85, 255],
    [255, 85, 255],
    [85, 255, 255],
    [255, 255, 255],
];

fn indexed_rgb(n: u8) -> [u8; 3] {
    match n {
        0..=15 => ANSI[usize::from(n)],
        16..=231 => {
            let n = n - 16;
            let levels = [0, 95, 135, 175, 215, 255];
            [
                levels[usize::from(n / 36)],
                levels[usize::from(n / 6 % 6)],
                levels[usize::from(n % 6)],
            ]
        }
        _ => [8 + (n - 232) * 10; 3],
    }
}

fn nearest(rgb: [u8; 3], foreground: bool) -> u8 {
    ANSI[..if foreground { 16 } else { 8 }]
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| {
            rgb.iter()
                .zip(candidate.iter())
                .map(|(a, b)| (i32::from(*a) - i32::from(*b)).pow(2))
                .sum::<i32>()
        })
        .expect("nonempty palette")
        .0 as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_console_palette_preserves_intended_colors() {
        let mode = OutputColors::linux_console();
        for (dos, indexed) in crate::palette::CLASSIC.iter().enumerate() {
            let ansi = crate::palette::TERMINAL[dos];
            assert_eq!(
                mode.parameters(Color::Indexed(*indexed), true),
                (if ansi < 8 { 30 + ansi } else { 90 + ansi - 8 }).to_string()
            );
            assert_eq!(
                mode.parameters(Color::Indexed(*indexed), false),
                (if ansi == 8 { 47 } else { 40 + (ansi & 7) }).to_string()
            );
        }
        assert_eq!(mode.parameters(Color::Indexed(248), false), "47");
        assert_eq!(mode.parameters(Color::Indexed(240), true), "90");
        assert_eq!(mode.parameters(Color::Indexed(240), false), "47");
        assert_eq!(
            mode.foreground(Color::Indexed(248), Color::Indexed(240)),
            "97"
        );
        assert_eq!(mode.foreground(Color::Indexed(248), Color::Black), "37");
    }

    #[test]
    fn arbitrary_colors_are_bounded_deterministic_and_reset_is_preserved() {
        let mode = OutputColors::linux_console();
        for n in 0..=255 {
            for fg in [false, true] {
                let value = mode.parameters(Color::Indexed(n), fg);
                let code: u8 = value.parse().unwrap();
                assert!(if fg {
                    (30..=37).contains(&code) || (90..=97).contains(&code)
                } else {
                    (40..=47).contains(&code)
                });
                assert_eq!(value, mode.parameters(Color::Indexed(n), fg));
            }
        }
        for (rgb, fg, expected) in [
            ([170, 170, 170], false, "47"),
            ([255, 255, 255], true, "97"),
            ([255, 255, 255], false, "47"),
            ([255, 0, 0], false, "41"),
            ([0, 0, 0], false, "40"),
        ] {
            assert_eq!(
                mode.parameters(Color::Rgb(rgb[0], rgb[1], rgb[2]), fg),
                expected
            );
        }
        for mode in [mode, OutputColors::default()] {
            assert_eq!(mode.parameters(Color::Reset, true), "39");
            assert_eq!(mode.parameters(Color::Reset, false), "49");
        }
    }

    #[test]
    fn extended_output_is_unchanged() {
        let mode = OutputColors::default();
        assert_eq!(mode.parameters(Color::Indexed(248), false), "48;5;248");
        assert_eq!(mode.parameters(Color::Rgb(1, 2, 3), true), "38;2;1;2;3");
        assert_eq!(mode.parameters(Color::LightBlue, true), "38;5;12");
    }
}
