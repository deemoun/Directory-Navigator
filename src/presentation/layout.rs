//! DN's one-dimensional viewport allocation and bounded terminal geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Position {
    pub x: u16,
    pub y: u16,
}
impl Position {
    pub const ORIGIN: Self = Self::new(0, 0);
    pub const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }
}
impl From<(u16, u16)> for Position {
    fn from((x, y): (u16, u16)) -> Self {
        Self { x, y }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}
impl Size {
    pub const ZERO: Self = Self::new(0, 0);
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}
impl Rect {
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    pub fn right(self) -> u16 {
        self.x.saturating_add(self.width)
    }
    pub fn bottom(self) -> u16 {
        self.y.saturating_add(self.height)
    }
    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
    pub fn area(self) -> u32 {
        u32::from(self.width) * u32::from(self.height)
    }
    pub fn contains(self, p: Position) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }
    pub fn intersection(self, r: Self) -> Self {
        let x = self.x.max(r.x);
        let y = self.y.max(r.y);
        Self::new(
            x,
            y,
            self.right().min(r.right()).saturating_sub(x),
            self.bottom().min(r.bottom()).saturating_sub(y),
        )
    }
    pub fn inner(self, m: Margin) -> Self {
        if m.horizontal > self.width / 2 || m.vertical > self.height / 2 {
            return Self::default();
        }
        Self::new(
            self.x.saturating_add(m.horizontal),
            self.y.saturating_add(m.vertical),
            self.width - 2 * m.horizontal,
            self.height - 2 * m.vertical,
        )
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Margin {
    pub horizontal: u16,
    pub vertical: u16,
}
impl Margin {
    pub const fn new(horizontal: u16, vertical: u16) -> Self {
        Self {
            horizontal,
            vertical,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constraint {
    Length(u16),
    Min(u16),
    Percentage(u16),
    Fill(u16),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Flex {
    #[default]
    FillLast,
    Start,
}
#[derive(Clone, Debug)]
pub struct Layout {
    vertical: bool,
    constraints: Vec<Constraint>,
    spacing: u16,
    flex: Flex,
}
impl Layout {
    pub fn horizontal(c: impl IntoIterator<Item = Constraint>) -> Self {
        Self {
            vertical: false,
            constraints: c.into_iter().collect(),
            spacing: 0,
            flex: Flex::FillLast,
        }
    }
    pub fn vertical(c: impl IntoIterator<Item = Constraint>) -> Self {
        Self {
            vertical: true,
            ..Self::horizontal(c)
        }
    }
    pub fn flex(mut self, flex: Flex) -> Self {
        self.flex = flex;
        self
    }
    pub fn spacing(mut self, spacing: u16) -> Self {
        self.spacing = spacing;
        self
    }
    pub fn split(&self, area: Rect) -> Vec<Rect> {
        let count = self.constraints.len();
        if count == 0 {
            return Vec::new();
        }
        let length = f64::from(if self.vertical {
            area.height
        } else {
            area.width
        });
        let gap = f64::from(self.spacing).min(length / (count.saturating_sub(1).max(1) as f64));
        let available = (length - gap * count.saturating_sub(1) as f64).max(0.0);
        let mut sizes = vec![0.0; count];
        let mut remaining = available;
        // Minimum controls survive shortage first, followed by fixed labels.
        for priority in 0..3 {
            for (i, c) in self.constraints.iter().enumerate() {
                let desired = match (priority, c) {
                    (0, Constraint::Min(n)) | (1, Constraint::Length(n)) => f64::from(*n),
                    (2, Constraint::Percentage(p)) => length * f64::from(*p) / 100.0,
                    _ => continue,
                };
                sizes[i] = desired.min(remaining);
                remaining -= sizes[i];
            }
        }
        let growers: Vec<_> = self
            .constraints
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match c {
                Constraint::Fill(w) => Some((i, f64::from(*w).max(0.000001))),
                Constraint::Min(_) if self.flex == Flex::Start => Some((i, 1.0)),
                _ => None,
            })
            .collect();
        if !growers.is_empty() {
            let mut active = growers;
            let mut pool = remaining + active.iter().map(|(i, _)| sizes[*i]).sum::<f64>();
            // Water-fill flexible controls while honoring each minimum. A large
            // minimum must not be squeezed just to equalize its flexible neighbor.
            while !active.is_empty() {
                let total: f64 = active.iter().map(|(_, w)| w).sum();
                if let Some(n) = active.iter().position(|(i, w)| {
                    matches!(self.constraints[*i], Constraint::Min(_))
                        && pool * w / total < sizes[*i]
                }) {
                    let (i, _) = active.remove(n);
                    pool -= sizes[i];
                } else {
                    for (i, w) in active {
                        sizes[i] = pool * w / total;
                    }
                    break;
                }
            }
        } else if let Some(i) = self
            .constraints
            .iter()
            .rposition(|c| matches!(c, Constraint::Min(_)))
        {
            sizes[i] += remaining;
        } else if self.flex == Flex::FillLast {
            sizes[count - 1] += remaining;
        }
        let origin = f64::from(if self.vertical { area.y } else { area.x });
        let mut cursor = origin;
        let round = |v: f64| ((v * 100.0).round() / 100.0).round() as u16;
        sizes
            .into_iter()
            .map(|size| {
                let start = round(cursor);
                cursor += size;
                let end = round(cursor);
                cursor += gap;
                if self.vertical {
                    Rect::new(area.x, start, area.width, end.saturating_sub(start))
                } else {
                    Rect::new(start, area.y, end.saturating_sub(start), area.height)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flexible_minima_and_large_margins_are_bounded() {
        let r = Layout::horizontal([Constraint::Min(20), Constraint::Fill(1)])
            .flex(Flex::Start)
            .split(Rect::new(0, 0, 30, 1));
        assert_eq!(r[0].width, 20);
        assert_eq!(r[1].width, 10);
        assert!(Rect::new(0, 0, u16::MAX, 1)
            .inner(Margin::new(u16::MAX, 0))
            .is_empty());
        assert_eq!(Layout::horizontal([]).split(Rect::default()), Vec::new());
    }
    #[test]
    fn frozen_allocation_and_tiny_viewport_bounds() {
        use Constraint as C;
        let cases = [
            vec![C::Length(1), C::Min(1), C::Length(1), C::Length(1)],
            vec![C::Percentage(50); 2],
            vec![C::Percentage(10); 10],
            vec![C::Fill(1); 8],
            vec![
                C::Fill(1),
                C::Length(1),
                C::Fill(1),
                C::Length(1),
                C::Fill(1),
            ],
            vec![C::Length(12), C::Length(12), C::Length(16), C::Min(1)],
            vec![
                C::Length(1),
                C::Min(0),
                C::Length(1),
                C::Length(1),
                C::Length(1),
                C::Length(1),
            ],
            vec![C::Min(1), C::Length(9), C::Length(12)],
            vec![C::Percentage(40), C::Length(5), C::Min(1)],
        ];
        let mut golden = include_str!("../../tests/fixtures/layout.txt").lines();
        for width in 0..=160 {
            for (i, c) in cases.iter().enumerate() {
                for spacing in [0, 1] {
                    let actual = Layout::horizontal(c.clone())
                        .flex(Flex::Start)
                        .spacing(spacing)
                        .split(Rect::new(3, 2, width, 1));
                    let expected = golden.next().unwrap();
                    for r in &actual {
                        assert!(
                            r.x >= 3 && r.right() <= 3 + width,
                            "{width}:{i}:{spacing}:{actual:?}"
                        );
                    }
                    let required: f64 = c
                        .iter()
                        .map(|c| match c {
                            C::Length(n) | C::Min(n) => f64::from(*n),
                            C::Percentage(p) => f64::from(width) * f64::from(*p) / 100.0,
                            C::Fill(_) => 0.0,
                        })
                        .sum::<f64>()
                        + f64::from(spacing) * c.len().saturating_sub(1) as f64;
                    // Over-constrained tiny viewports have an explicit bounded degradation
                    // policy. DN suppresses content below 80x25; preserve feasible layouts.
                    if required <= f64::from(width) {
                        assert_eq!(
                            format!(
                                "{width}:{i}:{spacing}:{:?}",
                                actual.iter().map(|r| (r.x, r.width)).collect::<Vec<_>>()
                            ),
                            expected
                        );
                    }
                }
            }
        }
    }
}
