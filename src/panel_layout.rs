use ratatui::layout::{Constraint, Layout, Rect};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelLayout {
    #[default]
    Brief,
    Details,
}
impl PanelLayout {
    pub fn key(self) -> &'static str {
        match self {
            Self::Brief => "brief",
            Self::Details => "details",
        }
    }
    pub fn parse(value: &str) -> Self {
        match value {
            "brief" => Self::Brief,
            _ => Self::Details,
        }
    }
}

/// The same rectangles drive painting, scrolling capacity and entry hit targets.
pub struct PanelGeometry {
    pub header: Rect,
    pub scrollbar: Rect,
    pub list: Rect,
    pub separator: Rect,
    pub detail: Rect,
    pub totals: Rect,
    pub space: Rect,
    pub columns: Vec<Rect>,
    pub dividers: Vec<Rect>,
}
impl PanelGeometry {
    pub fn new(inner: Rect, layout: PanelLayout) -> Self {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
        // Always reserve a cell: selecting, scrolling or changing counts must
        // never move the filename columns sideways.
        let header = Rect::new(
            chunks[0].x,
            chunks[0].y,
            chunks[0].width.saturating_sub(1),
            chunks[0].height,
        );
        let list = Rect::new(
            chunks[1].x,
            chunks[1].y,
            chunks[1].width.saturating_sub(1),
            chunks[1].height,
        );
        let scrollbar = Rect::new(
            inner.right().saturating_sub(1),
            header.y,
            u16::from(inner.width > 0),
            header.height + list.height,
        );
        let mut columns = vec![list];
        let mut dividers = Vec::new();
        if layout == PanelLayout::Brief {
            let parts = Layout::horizontal([
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .split(list);
            columns = vec![parts[0], parts[2], parts[4]];
            for part in [parts[1], parts[3]] {
                dividers.push(Rect::new(part.x, chunks[0].y, part.width, part.height + 1));
            }
        }
        Self {
            header,
            list,
            scrollbar,
            separator: chunks[2],
            detail: chunks[3],
            totals: chunks[4],
            space: chunks[5],
            columns,
            dividers,
        }
    }
    pub fn rows(&self) -> usize {
        usize::from(self.list.height)
    }
    pub fn capacity(&self) -> usize {
        self.rows() * self.columns.len()
    }
    pub fn entry_rect(&self, index: usize, offset: usize) -> Option<Rect> {
        let slot = index.checked_sub(offset)?;
        if self.rows() == 0 || slot >= self.capacity() {
            return None;
        }
        let column = self.columns.get(slot / self.rows())?;
        Some(Rect::new(
            column.x,
            column.y + (slot % self.rows()) as u16,
            column.width,
            1,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAction {
    Up,
    Down,
    Track,
    Thumb,
}
impl PanelGeometry {
    pub fn scroll_track(&self) -> Rect {
        Rect::new(
            self.scrollbar.x,
            self.scrollbar.y.saturating_add(1),
            self.scrollbar.width,
            self.scrollbar.height.saturating_sub(2),
        )
    }
    pub fn scroll_thumb(&self, cursor: usize, count: usize) -> Option<Rect> {
        let track = self.scroll_track();
        if count <= 1 || track.is_empty() {
            return None;
        }
        let span = u128::from(track.height - 1);
        let maximum = count.saturating_sub(1) as u128;
        let row = (cursor.min(count - 1) as u128 * span + maximum / 2)
            .checked_div(maximum)
            .unwrap_or(0) as u16;
        Some(Rect::new(track.x, track.y + row, track.width, 1))
    }
}
/// Inverse of the thumb position, using wide integer arithmetic for large lists.
pub fn scroll_position(track: Rect, y: u16, count: usize) -> usize {
    if track.height <= 1 || count <= 1 {
        return 0;
    }
    let row = u128::from(y.saturating_sub(track.y).min(track.height - 1));
    let span = u128::from(track.height - 1);
    ((row * count.saturating_sub(1) as u128 + span / 2) / span) as usize
}
