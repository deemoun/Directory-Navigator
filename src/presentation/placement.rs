//! One coordinate path for surface cells, cursor, hit regions and pointer input.
use super::layout::Rect;
use crate::turbo::scene::Scene;

#[derive(Clone, Copy)]
pub(crate) struct Placement {
    offset: (i32, i32),
    clip: Rect,
    editor_header: bool,
    intersect_hits: bool,
    status_row: Option<u16>,
    workspace_hit: bool,
    wide_boundary: Option<u16>,
}
impl Placement {
    pub fn translated(dx: i32, dy: i32, clip: Rect) -> Self {
        Self {
            offset: (dx, dy),
            clip,
            editor_header: false,
            intersect_hits: false,
            status_row: None,
            workspace_hit: false,
            wide_boundary: None,
        }
    }
    pub fn screen(bounds: Rect, clip: Rect) -> Self {
        let mut p = Self::translated(i32::from(bounds.x), i32::from(bounds.y), clip);
        p.intersect_hits = true;
        p.wide_boundary = Some(bounds.x);
        p
    }
    pub fn document(bounds: Rect, editor: bool) -> Self {
        let mut p = Self::translated(
            i32::from(bounds.x + 1),
            i32::from(bounds.y),
            Rect::new(
                bounds.x + 1,
                bounds.y + 1,
                bounds.width.saturating_sub(2),
                bounds.height.saturating_sub(2),
            ),
        );
        p.editor_header = editor;
        p
    }
    pub fn with_workspace_hits(mut self) -> Self {
        self.workspace_hit = true;
        self
    }
    pub fn with_status_row(mut self, row: Option<u16>) -> Self {
        self.status_row = row;
        self
    }
    pub fn offset(self) -> (i32, i32) {
        self.offset
    }
    fn raw(self, x: u16, y: u16) -> Option<(u16, u16)> {
        if self.status_row == Some(y) {
            return Some((x, y));
        }
        let y = if self.editor_header {
            match y {
                0 => 1,
                1 => 0,
                _ => y,
            }
        } else {
            y
        };
        let x = i32::from(x) + self.offset.0;
        let y = i32::from(y) + self.offset.1;
        Some((u16::try_from(x).ok()?, u16::try_from(y).ok()?))
    }
    pub fn point(self, x: u16, y: u16) -> Option<(u16, u16)> {
        let p = self.raw(x, y)?;
        (self.status_row == Some(y) || self.clip.contains(p.into())).then_some(p)
    }
    pub fn rect(self, r: Rect) -> Rect {
        let anchor = if self.intersect_hits {
            self.raw(r.x, r.y)
        } else {
            self.point(r.x, r.y)
        };
        let Some((x, y)) = anchor else {
            return Rect::default();
        };
        Rect::new(x, y, r.width, r.height).intersection(self.clip)
    }
    /// Capture may continue outside the clip. Only the workspace origin is removed.
    pub fn local_point(self, x: u16, y: u16) -> (u16, u16) {
        debug_assert!(!self.editor_header);
        (
            (i32::from(x) - self.offset.0).clamp(0, i32::from(u16::MAX)) as u16,
            (i32::from(y) - self.offset.1).clamp(0, i32::from(u16::MAX)) as u16,
        )
    }
    pub fn local_mouse(
        self,
        mut event: crossterm::event::MouseEvent,
    ) -> crossterm::event::MouseEvent {
        (event.column, event.row) = self.local_point(event.column, event.row);
        event
    }
    pub fn apply(self, scene: &mut Scene) {
        scene.cells = std::mem::take(&mut scene.cells)
            .into_iter()
            .filter_map(|((x, y), mut glyph)| {
                let p = self.point(x, y)?;
                if let Some(origin) = self.wide_boundary {
                    use unicode_width::UnicodeWidthStr;
                    if glyph.text.width() > usize::from(self.clip.right() - p.0)
                        || (glyph.text.is_empty() && p.0 == self.clip.x && self.clip.x > origin)
                    {
                        glyph.text = " ".into();
                    }
                }
                Some((p, glyph))
            })
            .collect();
        scene.regions = std::mem::take(&mut scene.regions)
            .into_iter()
            .filter_map(|(r, t)| {
                let r = if self.workspace_hit
                    && r == Rect::new(0, 0, scene.width, scene.height)
                    && self.clip.x == 0
                    && self.clip.y == 1
                    && self.clip.width == scene.width
                {
                    r
                } else if self.status_row == Some(r.y) {
                    r.intersection(Rect::new(0, r.y, scene.width, 1))
                } else {
                    self.rect(r)
                };
                (!r.is_empty()).then_some((r, t))
            })
            .collect();
        scene.cursor = scene.cursor.and_then(|(x, y)| self.point(x, y));
    }
    pub fn restore_outside(self, scene: &mut Scene, background: &Scene) {
        let outside = scene
            .cells
            .keys()
            .filter(|(x, y)| !self.clip.contains((*x, *y).into()))
            .copied()
            .collect::<Vec<_>>();
        for point in outside {
            if let Some(glyph) = background.cells.get(&point) {
                scene.cells.insert(point, glyph.clone());
            } else {
                scene.cells.remove(&point);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mouse::Target, palette::PaletteProfile, turbo::scene::Glyph};
    fn scene() -> Scene {
        Scene {
            width: 80,
            height: 25,
            profile: PaletteProfile::ClassicDn,
            cells: Default::default(),
            cursor: None,
            regions: Vec::new(),
        }
    }
    fn glyph(text: &str) -> Glyph {
        Glyph {
            text: text.into(),
            attribute: 0x1f,
            style: None,
        }
    }
    #[test]
    fn screen_projection_clips_wide_cells_hits_and_cursor_and_roundtrips_pointer() {
        let mut s = scene();
        s.cells.extend([
            ((0, 0), glyph("界")),
            ((1, 0), glyph("")),
            ((3, 0), glyph("界")),
        ]);
        s.regions.push((Rect::new(0, 0, 4, 2), Target::PromptField));
        s.cursor = Some((2, 0));
        let p = Placement::screen(Rect::new(5, 4, 80, 25), Rect::new(6, 4, 3, 3));
        p.apply(&mut s);
        assert_eq!(s.cells.len(), 2);
        assert_eq!(s.cells[&(6, 4)].text, " ");
        assert_eq!(s.cells[&(8, 4)].text, " ");
        assert_eq!(
            s.regions,
            vec![(Rect::new(6, 4, 3, 2), Target::PromptField)]
        );
        assert_eq!(s.cursor, Some((7, 4)));
        assert_eq!(p.local_point(7, 4), (2, 0));
        assert_eq!(p.point(2, 0), s.cursor);
    }
    #[test]
    fn owned_page_moves_its_controls_without_moving_workspace_cancel_or_status() {
        let mut s = scene();
        s.cells
            .extend([((10, 4), glyph("e\u{301}")), ((2, 24), glyph("F1"))]);
        s.cursor = Some((10, 4));
        s.regions = vec![
            (Rect::new(0, 0, 80, 25), Target::CancelSettingsChoice),
            (Rect::new(10, 4, 8, 1), Target::PromptField),
            (Rect::new(0, 24, 10, 1), Target::CloseDialog),
        ];
        let p = Placement::translated(-5, 3, Rect::new(0, 1, 80, 23))
            .with_workspace_hits()
            .with_status_row(Some(24));
        p.apply(&mut s);
        assert_eq!(s.cells[&(5, 7)].text, "e\u{301}");
        assert!(s.cells.contains_key(&(2, 24)));
        assert_eq!(s.cursor, Some((5, 7)));
        assert_eq!(
            s.regions,
            vec![
                (Rect::new(0, 0, 80, 25), Target::CancelSettingsChoice),
                (Rect::new(5, 7, 8, 1), Target::PromptField),
                (Rect::new(0, 24, 10, 1), Target::CloseDialog)
            ]
        );
    }
    #[test]
    fn editor_menu_content_and_cursor_use_the_same_interior_mapping() {
        let mut s = scene();
        s.cells.extend([
            ((0, 0), glyph("M")),
            ((0, 1), glyph("H")),
            ((3, 2), glyph("界")),
        ]);
        s.cursor = Some((3, 2));
        s.regions.push((Rect::new(3, 2, 2, 1), Target::PromptField));
        Placement::document(Rect::new(10, 5, 72, 12), true).apply(&mut s);
        assert_eq!(s.cells[&(11, 6)].text, "M");
        assert!(!s.cells.values().any(|g| g.text == "H"));
        assert_eq!(s.cursor, Some((14, 7)));
        assert_eq!(
            s.regions,
            vec![(Rect::new(14, 7, 2, 1), Target::PromptField)]
        );
    }
}
