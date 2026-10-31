//! Native presentation primitives. All surfaces paint Turbo Vision scenes.
use crate::presentation::{
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Text},
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderType {
    #[default]
    Plain,
    Double,
}
#[derive(Clone, Copy, Default)]
pub struct Borders(u8);
impl Borders {
    pub const LEFT: Self = Self(1);
    pub const RIGHT: Self = Self(2);
    pub const TOP: Self = Self(4);
    pub const BOTTOM: Self = Self(8);
    pub const ALL: Self = Self(15);
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
#[derive(Clone, Copy)]
pub struct Wrap {
    pub trim: bool,
}

pub trait Frame {
    fn area(&self) -> Rect;
    fn palette(&mut self, profile: crate::palette::PaletteProfile);
    fn render_widget(&mut self, widget: impl Widget, area: Rect);
    fn set_cursor_position(&mut self, position: impl Into<crate::presentation::layout::Position>);
    fn regions(&mut self) -> &mut Vec<(Rect, crate::mouse::Target)>;
    fn compose(&mut self, scene: crate::turbo::scene::Scene);
    fn modal_scope(&mut self, _scope: Option<crate::modals::ModalInfo>) {}
    fn document_scope(&mut self, _owner: Option<crate::desktop::WindowId>) {}
    fn native_shadow(&mut self, _bounds: Rect) {}
}
pub trait Widget {
    fn is_control(&self) -> bool {
        false
    }
    fn native(self, frame: &mut NativeFrame, area: Rect);
}

#[derive(Default)]
pub struct Block<'a> {
    style: Style,
    border_style: Style,
    title_style: Style,
    borders: Borders,
    border_type: BorderType,
    title: Option<Line<'a>>,
    alignment: Alignment,
}
impl<'a> Block<'a> {
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }
    pub fn borders(mut self, borders: Borders) -> Self {
        self.borders = borders;
        self
    }
    pub fn border_type(mut self, kind: BorderType) -> Self {
        self.border_type = kind;
        self
    }
    pub fn title(mut self, title: impl Into<Line<'a>>) -> Self {
        self.title = Some(title.into());
        self
    }
    pub fn title_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }
    pub fn inner(&self, mut r: Rect) -> Rect {
        if self.borders.contains(Borders::LEFT) && r.width > 0 {
            r.x += 1;
            r.width -= 1;
        }
        if self.borders.contains(Borders::RIGHT) {
            r.width = r.width.saturating_sub(1);
        }
        if (self.borders.contains(Borders::TOP) || self.title.is_some()) && r.height > 0 {
            r.y += 1;
            r.height -= 1;
        }
        if self.borders.contains(Borders::BOTTOM) {
            r.height = r.height.saturating_sub(1);
        }
        r
    }
}
impl Widget for Block<'_> {
    fn native(self, f: &mut NativeFrame, area: Rect) {
        let r = area.intersection(f.area());
        if r.is_empty() {
            return;
        }
        f.style(r, self.style);
        let double = self.border_type == BorderType::Double;
        let (h, v, tl, tr, bl, br) = if double {
            ("═", "║", "╔", "╗", "╚", "╝")
        } else {
            ("─", "│", "┌", "┐", "└", "┘")
        };
        let top = self.borders.contains(Borders::TOP);
        let bottom = self.borders.contains(Borders::BOTTOM);
        let left = self.borders.contains(Borders::LEFT);
        let right = self.borders.contains(Borders::RIGHT);
        for x in r.x..r.right() {
            if top {
                f.put(x, r.y, h, self.border_style);
            }
            if bottom {
                f.put(x, r.bottom() - 1, h, self.border_style);
            }
        }
        for y in r.y..r.bottom() {
            if left {
                f.put(r.x, y, v, self.border_style);
            }
            if right {
                f.put(r.right() - 1, y, v, self.border_style);
            }
        }
        for (show, x, y, text) in [
            (top && left, r.x, r.y, tl),
            (top && right, r.right() - 1, r.y, tr),
            (bottom && left, r.x, r.bottom() - 1, bl),
            (bottom && right, r.right() - 1, r.bottom() - 1, br),
        ] {
            if show {
                f.put(x, y, text, self.border_style);
            }
        }
        if let Some(title) = self.title {
            let x = r.x + u16::from(left);
            let width = r.width.saturating_sub(u16::from(left) + u16::from(right));
            let title_width = title.width().min(usize::from(width)) as u16;
            let offset = match title.alignment.unwrap_or(self.alignment) {
                Alignment::Left => 0,
                Alignment::Center => (width - title_width) / 2,
                Alignment::Right => width - title_width,
            };
            let mut title = title;
            title.style = self.title_style.patch(title.style);
            title.alignment = None;
            Paragraph::new(title).native(f, Rect::new(x + offset, r.y, width - offset, 1));
        }
    }
}

pub struct Paragraph<'a> {
    text: Text<'a>,
    style: Style,
    alignment: Alignment,
    scroll: (u16, u16),
    wrap: Option<Wrap>,
}
impl<'a> Paragraph<'a> {
    pub fn new(text: impl Into<Text<'a>>) -> Self {
        Self {
            text: text.into(),
            style: Style::default(),
            alignment: Alignment::Left,
            scroll: (0, 0),
            wrap: None,
        }
    }
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }
    pub fn scroll(mut self, scroll: (u16, u16)) -> Self {
        self.scroll = scroll;
        self
    }
    pub fn wrap(mut self, wrap: Wrap) -> Self {
        self.wrap = Some(wrap);
        self
    }
}
impl Widget for Paragraph<'_> {
    fn native(self, f: &mut NativeFrame, area: Rect) {
        use unicode_width::UnicodeWidthStr;
        let r = area.intersection(f.area());
        if r.is_empty() {
            return;
        }
        f.style(r, self.style);
        let mut rows = Vec::new();
        for line in &self.text.lines {
            let glyphs: Vec<_> = line
                .styled_graphemes(self.text.style)
                .map(|g| Run {
                    width: g.symbol.width() as u16,
                    space: g.symbol == "\u{200b}"
                        || (g.symbol != "\u{a0}" && g.symbol.chars().all(char::is_whitespace)),
                    text: g.symbol.to_owned(),
                    style: g.style,
                })
                .collect();
            let alignment = line.alignment.unwrap_or(self.alignment);
            if let Some(wrap) = self.wrap {
                for line in reflow(glyphs, r.width, wrap.trim) {
                    rows.push((line, alignment));
                }
            } else {
                let (mut skipped, mut width) = (self.scroll.1, 0);
                let mut visible = Vec::new();
                for g in glyphs {
                    if g.width > r.width {
                        continue;
                    }
                    if width + g.width > r.width {
                        break;
                    }
                    if skipped >= g.width {
                        skipped -= g.width;
                        continue;
                    }
                    skipped = 0;
                    width += g.width;
                    visible.push(g);
                }
                rows.push((visible, alignment));
            }
        }
        for (row, (line, alignment)) in rows
            .into_iter()
            .skip(usize::from(self.scroll.0))
            .take(usize::from(r.height))
            .enumerate()
        {
            let width: u16 = line.iter().map(|g| g.width).sum();
            let offset = match alignment {
                Alignment::Left => 0,
                Alignment::Center => (r.width / 2).saturating_sub(width / 2),
                Alignment::Right => r.width.saturating_sub(width),
            };
            let mut x = r.x + offset;
            for g in line {
                if g.width > 0 && x < r.right() {
                    f.put(x, r.y + row as u16, &g.text, g.style);
                    x += g.width;
                }
            }
        }
    }
}
pub struct Clear;
impl Widget for Clear {
    fn native(self, f: &mut NativeFrame, area: Rect) {
        let r = area.intersection(f.area());
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                f.put(x, y, " ", Style::reset());
            }
        }
    }
}
#[derive(Clone)]
struct Run {
    text: String,
    style: Style,
    width: u16,
    space: bool,
}
mod wrapping;
use wrapping::reflow;

pub(crate) fn wrap_plain(value: &str, width: u16) -> Vec<String> {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    value
        .split('\n')
        .flat_map(|line| {
            reflow(
                line.graphemes(true)
                    .map(|g| Run {
                        text: g.into(),
                        style: Style::default(),
                        width: g.width() as u16,
                        space: g == "\u{200b}"
                            || (g != "\u{a0}" && g.chars().all(char::is_whitespace)),
                    })
                    .collect(),
                width,
                false,
            )
            .into_iter()
            .map(|row| row.into_iter().map(|g| g.text).collect())
        })
        .collect()
}

pub struct NativeFrame {
    scene: crate::turbo::scene::Scene,
    placements: Vec<crate::windowing::geometry::Surface>,
    dialog_background: Option<crate::turbo::scene::Scene>,
    composition: Option<crate::turbo::composition::DialogGroup>,
    scope: Option<crate::modals::ModalInfo>,
    document_owner: Option<crate::desktop::WindowId>,
    dialogs: Vec<std::rc::Rc<std::cell::RefCell<crate::turbo::composition::DialogGroup>>>,
}
impl NativeFrame {
    pub fn new(area: Rect, profile: crate::palette::PaletteProfile) -> Self {
        Self {
            placements: Vec::new(),
            dialog_background: None,
            composition: None,
            scope: None,
            document_owner: None,
            dialogs: Vec::new(),
            scene: crate::turbo::scene::Scene {
                width: area.width,
                height: area.height,
                profile,
                cells: Default::default(),
                cursor: None,
                regions: Vec::new(),
            },
        }
    }
    pub(crate) fn placements(&mut self, surfaces: &[crate::windowing::geometry::Surface]) {
        self.placements = surfaces.to_vec();
    }
    pub(crate) fn replace(&mut self, scene: crate::turbo::scene::Scene) {
        self.scene = scene;
    }
    pub(crate) fn finish(mut self) -> crate::turbo::scene::Scene {
        self.flush_dialog();
        self.scene
    }
    pub(crate) fn finish_dialogs(
        mut self,
    ) -> (
        crate::turbo::scene::Scene,
        Vec<std::rc::Rc<std::cell::RefCell<crate::turbo::composition::DialogGroup>>>,
    ) {
        self.flush_dialog();
        (self.scene, self.dialogs)
    }
    pub(crate) fn composing(&self) -> bool {
        self.composition.is_some()
    }
    pub(crate) fn dialog_scope(&self) -> Option<crate::modals::ModalInfo> {
        self.scope
    }
    pub(crate) fn document_owner(&self) -> Option<crate::desktop::WindowId> {
        self.document_owner
    }
    pub(crate) fn start_dialog(&mut self, widget: crate::turbo::widgets::Native, area: Rect) {
        self.flush_dialog();
        // Captions paint sparsely. Their image children must capture the dialog
        // background, never file names or borders from the obscured document.
        self.dialog_background = Some(self.scene.clone());
        self.scene.cursor = None;
        self.scene
            .fill(area.x, area.y, area.width, area.height, 0x70);
        self.composition = Some(if let Some(scope) = self.scope {
            crate::turbo::composition::DialogGroup::new(widget, area, scope)
        } else {
            crate::turbo::composition::DialogGroup::document(
                widget,
                area,
                self.document_owner.unwrap(),
            )
        });
    }
    pub(crate) fn stage_control(&mut self, widget: crate::turbo::widgets::Native, area: Rect) {
        self.composition.as_mut().unwrap().control(
            widget,
            area,
            self.scene.height.saturating_sub(1),
        );
    }
    pub(crate) fn flush_dialog(&mut self) {
        if let Some(mut dialog) = self.composition.take() {
            dialog.paint(&mut self.scene);
            if let Some(surface) = self
                .placements
                .iter()
                .find(|s| {
                    matches!(s.id.key,crate::windowing::geometry::SurfaceKey::Modal(id)|crate::windowing::geometry::SurfaceKey::ModalPage(id,_) if Some(id)==dialog.scope.map(|m|m.id))
                        && s.default.x==dialog.bounds().x && s.default.y==dialog.bounds().y
                        && s.default.width==dialog.bounds().width
                })
                .copied()
            {
                let desktop=crate::windowing::geometry::workspace((self.scene.width,self.scene.height));
                if let Some(background)=&self.dialog_background {
                    crate::presentation::placement::Placement::translated(0,0,desktop).restore_outside(&mut self.scene,background);
                }
                let from = dialog.bounds();
                let to = surface.bounds;
                if from.x != to.x || from.y != to.y {
                    let mut layer = self.scene.clone();
                    let background = self.dialog_background.take().unwrap();
                    layer
                        .cells
                        .retain(|(x, y), _| from.contains((*x, *y).into()));
                    let placement=crate::presentation::placement::Placement::translated(
                        i32::from(to.x)-i32::from(from.x),i32::from(to.y)-i32::from(from.y),desktop).with_workspace_hits();
                    placement.apply(&mut layer);
                    self.scene.cells = background.cells;
                    self.scene.cells.extend(layer.cells);
                    // The top modal alone owns client-area commands, including
                    // clicks outside a nested choice. Background images remain visible.
                    self.scene.regions.clear();
                    self.scene.regions.push((to,crate::mouse::Target::Surface(surface.id,crate::windowing::geometry::Chrome::Activate)));
                    self.scene.regions.extend(layer.regions);
                    self.scene.cursor = layer.cursor;
                    crate::turbo::composition::paint_shadow(&mut self.scene, to);
                    let (dx,dy)=placement.offset();
                    dialog.translate_hits(dx,dy);
                }
                crate::windowing::workspace::chrome(&mut self.scene, surface);
            }
            self.dialog_background = None;
            self.dialogs
                .push(std::rc::Rc::new(std::cell::RefCell::new(dialog)));
        }
    }

    pub(crate) fn cursor(&self) -> Option<(u16, u16)> {
        self.scene.cursor
    }
    pub(crate) fn clear_cursor(&mut self) {
        self.scene.cursor = None;
    }
    pub(crate) fn profile(&self) -> crate::palette::PaletteProfile {
        self.scene.profile
    }
    pub(crate) fn control_scene(&mut self, scene: crate::turbo::scene::Scene) {
        for ((x, y), glyph) in scene.cells {
            let style = Style::reset().patch(glyph.resolved(self.scene.profile));
            self.put(x, y, &glyph.text, style);
        }
    }
    fn style(&mut self, area: Rect, style: Style) {
        for y in area.y..area.bottom().min(self.scene.height) {
            for x in area.x..area.right().min(self.scene.width) {
                let glyph =
                    self.scene
                        .cells
                        .entry((x, y))
                        .or_insert_with(|| crate::turbo::scene::Glyph {
                            text: " ".into(),
                            attribute: 0x07,
                            style: Some(Style::reset()),
                        });
                glyph.style = Some(glyph.resolved(self.scene.profile).patch(style));
            }
        }
    }
    fn put(&mut self, x: u16, y: u16, text: &str, style: Style) {
        use unicode_width::UnicodeWidthStr;
        if x >= self.scene.width || y >= self.scene.height {
            return;
        }
        let previous_width = self.scene.cells.get(&(x, y)).map_or(1, |g| g.text.width());
        let incoming_width = text.width();
        for offset in incoming_width.max(1)..previous_width {
            if let Some(glyph) = self.scene.cells.get_mut(&(x + offset as u16, y)) {
                if glyph.text.is_empty() {
                    glyph.text = " ".into();
                }
            }
        }
        let base = self
            .scene
            .cells
            .get(&(x, y))
            .map(|g| g.resolved(self.scene.profile))
            .unwrap_or_else(Style::reset);
        let style = base.patch(style);
        self.scene.cells.insert(
            (x, y),
            crate::turbo::scene::Glyph {
                text: text.into(),
                attribute: 0x07,
                style: Some(style),
            },
        );
        for offset in 1..(text.width() as u16).min(self.scene.width - x) {
            self.scene.cells.insert(
                (x + offset, y),
                crate::turbo::scene::Glyph {
                    text: String::new(),
                    attribute: 0x07,
                    style: Some(style),
                },
            );
        }
    }
}
impl Frame for NativeFrame {
    fn palette(&mut self, profile: crate::palette::PaletteProfile) {
        self.scene.profile = profile;
    }
    fn area(&self) -> Rect {
        Rect::new(0, 0, self.scene.width, self.scene.height)
    }
    fn render_widget(&mut self, widget: impl Widget, area: Rect) {
        if !widget.is_control() && self.composition.is_some() {
            let mut dialog = self.composition.take().unwrap();
            widget.native(self, area);
            dialog.image(&self.scene, area);
            self.composition = Some(dialog);
        } else {
            widget.native(self, area);
        }
    }
    fn set_cursor_position(&mut self, position: impl Into<crate::presentation::layout::Position>) {
        let p = position.into();
        self.scene.cursor = Some((p.x, p.y));
    }
    fn regions(&mut self) -> &mut Vec<(Rect, crate::mouse::Target)> {
        &mut self.scene.regions
    }
    fn native_shadow(&mut self, bounds: Rect) {
        self.flush_dialog();
        crate::turbo::composition::paint_shadow(&mut self.scene, bounds);
    }
    fn modal_scope(&mut self, scope: Option<crate::modals::ModalInfo>) {
        self.flush_dialog();
        self.scope = scope;
        self.document_owner = None;
    }
    fn document_scope(&mut self, owner: Option<crate::desktop::WindowId>) {
        self.flush_dialog();
        self.scope = None;
        self.document_owner = owner;
    }
    fn compose(&mut self, scene: crate::turbo::scene::Scene) {
        if let Some(dialog) = &mut self.composition {
            dialog.image(&scene, Rect::new(0, 0, scene.width, scene.height));
        }
        self.scene.cells.extend(scene.cells);
        self.scene.regions.extend(scene.regions);
        self.scene.cursor = scene.cursor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::PaletteProfile;
    #[test]
    fn native_wrapping_preserves_frozen_spaces_words_and_unicode_contract() -> std::io::Result<()> {
        let samples = [
            "one two three four five".into(),
            "  one    two      three ".into(),
            "x".repeat(150),
            "界 e\u{301} 👩‍💻 ".repeat(12),
            "first\n\nthird\n".into(),
            "a\u{a0}b c\u{200b}d".into(),
            "/path/with-segments/and-a-long-name/next: cannot open file".into(),
            "one ".repeat(30),
        ];
        let mut expected = include_str!("../tests/fixtures/wrapping.txt").lines();
        for width in [1, 2, 5, 17, 44] {
            for sample in &samples {
                let area = Rect::new(0, 0, width, 20);
                let style = PaletteProfile::ClassicDn.dos(0x70);
                let mut native = NativeFrame::new(area, PaletteProfile::ClassicDn);
                native.render_widget(
                    Paragraph::new(sample.as_str())
                        .style(style)
                        .wrap(Wrap { trim: false }),
                    area,
                );
                let (buffer, _) = crate::testing::Buffer::capture(native);
                let actual = format!(
                    "{:?}",
                    buffer
                        .content
                        .iter()
                        .map(|c| c.symbol())
                        .collect::<Vec<_>>()
                );
                let expected = expected.next().unwrap();
                assert_eq!(actual, expected, "sample={sample:?},width={width}");
            }
        }
        Ok(())
    }
}
