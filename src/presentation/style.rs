//! Terminal colors and composable presentation attributes, independent of a renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Color {
    #[default]
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
    Indexed(u8),
    Rgb(u8, u8, u8),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifier(u16);
impl Modifier {
    pub const BOLD: Self = Self(1);
    pub const DIM: Self = Self(2);
    pub const ITALIC: Self = Self(4);
    pub const UNDERLINED: Self = Self(8);
    pub const REVERSED: Self = Self(64);
    pub const CROSSED_OUT: Self = Self(256);
    pub const fn empty() -> Self {
        Self(0)
    }
    pub const fn all() -> Self {
        Self(511)
    }
    pub const fn bits(self) -> u16 {
        self.0
    }
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}
impl std::ops::BitOr for Modifier {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub add_modifier: Modifier,
    pub sub_modifier: Modifier,
}
impl Style {
    pub fn reset() -> Self {
        Self {
            fg: Some(Color::Reset),
            bg: Some(Color::Reset),
            sub_modifier: Modifier::all(),
            ..Self::default()
        }
    }
    pub fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }
    pub fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }
    pub fn add_modifier(mut self, modifier: Modifier) -> Self {
        self.sub_modifier.remove(modifier);
        self.add_modifier.insert(modifier);
        self
    }
    pub fn remove_modifier(mut self, modifier: Modifier) -> Self {
        self.add_modifier.remove(modifier);
        self.sub_modifier.insert(modifier);
        self
    }
    pub fn patch(mut self, other: Self) -> Self {
        self.fg = other.fg.or(self.fg);
        self.bg = other.bg.or(self.bg);
        self.add_modifier.remove(other.sub_modifier);
        self.add_modifier.insert(other.add_modifier);
        self.sub_modifier.remove(other.add_modifier);
        self.sub_modifier.insert(other.sub_modifier);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_resets_and_modifier_cancellation_compose_in_order() {
        let base = Style::default()
            .fg(Color::Indexed(19))
            .bg(Color::Rgb(1, 2, 3))
            .add_modifier(Modifier::BOLD | Modifier::DIM);
        let top = Style::default()
            .fg(Color::Reset)
            .remove_modifier(Modifier::BOLD)
            .add_modifier(Modifier::ITALIC);
        let combined = base.patch(top);
        assert_eq!(combined.fg, Some(Color::Reset));
        assert_eq!(combined.bg, base.bg);
        assert!(!combined.add_modifier.contains(Modifier::BOLD));
        assert!(combined
            .add_modifier
            .contains(Modifier::DIM | Modifier::ITALIC));
        let reset = combined.patch(Style::reset());
        assert_eq!(reset.add_modifier, Modifier::empty());
        assert!(reset
            .sub_modifier
            .contains(Modifier::DIM | Modifier::ITALIC));
        let bold = reset.patch(Style::default().add_modifier(Modifier::BOLD));
        assert!(bold.add_modifier.contains(Modifier::BOLD));
        assert!(!bold.sub_modifier.contains(Modifier::BOLD));
    }
}
