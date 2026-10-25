use crate::presentation::{
    style::{Color, Style},
    text::{Line, Span},
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaletteProfile {
    #[default]
    ClassicDn,
    Terminal,
}
// Nearest standard xterm entries 16–255 to VGA RGB values. Squared RGB
// distance, lowest index on ties; no dependence on configurable ANSI colors.
pub const CLASSIC: [u8; 16] = [
    16, 19, 34, 37, 124, 127, 130, 248, 240, 63, 83, 87, 203, 207, 227, 231,
];
pub const TERMINAL: [u8; 16] = [0, 4, 2, 6, 1, 5, 3, 7, 8, 12, 10, 14, 9, 13, 11, 15];
impl PaletteProfile {
    pub fn key(self) -> &'static str {
        match self {
            Self::ClassicDn => "classic",
            Self::Terminal => "terminal",
        }
    }
    pub fn parse(value: &str) -> Self {
        if value == "terminal" {
            Self::Terminal
        } else {
            Self::ClassicDn
        }
    }
    pub fn color(self, index: u8) -> Color {
        Color::Indexed(match self {
            Self::ClassicDn => CLASSIC[(index & 15) as usize],
            Self::Terminal => TERMINAL[(index & 15) as usize],
        })
    }
    pub fn dos(self, attribute: u8) -> Style {
        Style::default()
            .fg(self.color(attribute & 15))
            .bg(self.color(attribute >> 4))
    }
    pub fn styles(self) -> impl Fn(u8) -> Style + Copy {
        move |attribute| self.dos(attribute)
    }
    pub fn bar(self, enabled: bool) -> Style {
        self.dos(match (self, enabled) {
            (Self::ClassicDn, true) => 0x70,
            (Self::ClassicDn, false) => 0x78,
            (Self::Terminal, true) => 0x30,
            (Self::Terminal, false) => 0x38,
        })
    }
    pub fn function(self, number: u8, label: &str, enabled: bool) -> Line<'static> {
        let key = if self == Self::ClassicDn {
            format!("F{number} ")
        } else {
            number.to_string()
        };
        self.function_key(key, label, enabled)
    }
    pub fn function_key(self, key: impl Into<String>, label: &str, enabled: bool) -> Line<'static> {
        let style = if self == Self::ClassicDn {
            self.dos(if enabled { 0x74 } else { 0x78 })
        } else {
            self.dos(0x07)
        };
        Line::from(vec![
            Span::styled(key.into(), style),
            Span::styled(label.to_string(), self.bar(enabled)),
        ])
    }
}
pub struct ColorDialog {
    pub original: PaletteProfile,
    pub cursor: usize,
}
pub fn open(app: &mut crate::app::App) {
    crate::menu::close(app);
    app.color_dialog = Some(ColorDialog {
        original: app.config.palette,
        cursor: usize::from(app.config.palette == PaletteProfile::Terminal),
    });
}
pub fn choose(app: &mut crate::app::App, profile: PaletteProfile) {
    app.config.palette = profile;
    if let Some(dialog) = &mut app.color_dialog {
        dialog.cursor = usize::from(profile == PaletteProfile::Terminal);
    }
}
pub fn cancel(app: &mut crate::app::App) {
    if let Some(dialog) = app.color_dialog.take() {
        app.config.palette = dialog.original;
    }
}
pub fn key(app: &mut crate::app::App, code: crossterm::event::KeyCode) {
    use crossterm::event::KeyCode;
    let Some(dialog) = &mut app.color_dialog else {
        return;
    };
    match code {
        KeyCode::Esc => cancel(app),
        KeyCode::Tab | KeyCode::Down => {
            dialog.cursor = crate::control_focus::cycle(dialog.cursor, 4, false)
        }
        KeyCode::BackTab | KeyCode::Up => {
            dialog.cursor = crate::control_focus::cycle(dialog.cursor, 4, true)
        }
        KeyCode::Enter if dialog.cursor == 3 => cancel(app),
        KeyCode::Enter => app.color_dialog = None,
        KeyCode::Char(' ') if dialog.cursor < 2 => {
            let profile = if dialog.cursor == 0 {
                PaletteProfile::ClassicDn
            } else {
                PaletteProfile::Terminal
            };
            choose(app, profile);
        }
        KeyCode::Char('c') => choose(app, PaletteProfile::ClassicDn),
        KeyCode::Char('t') => choose(app, PaletteProfile::Terminal),
        _ => {}
    }
}
