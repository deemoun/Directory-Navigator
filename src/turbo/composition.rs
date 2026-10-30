//! One native Dialog/Group per published modal, with owner-relative children.
//! Domain controllers retain guarded commands and semantic field state.
use super::{
    scene::{Glyph, Scene},
    widgets::{Appearance, Native, TextCells},
};
use crate::{
    modals::ModalInfo, mouse::Target, palette::PaletteProfile, presentation::layout::Rect,
};
use std::collections::BTreeMap;
use turbo_vision::{
    core::{draw::Cell, event::*, geometry::Rect as TvRect, palette::Attr, state::State},
    terminal::Terminal,
    views::{
        button::Button,
        dialog::{CloseOn, Dialog},
        GroupLike, View, ViewCore,
    },
};
use unicode_width::UnicodeWidthStr;
struct Plane {
    rect: Rect,
    text: TextCells,
    appearance: Appearance,
    theme_group: Option<crate::palette::theme::Group>,
    source: Option<BTreeMap<(u16, u16), Glyph>>,
    button: bool,
    original: Rect,
}
pub(crate) struct DialogGroup {
    offset: (i32, i32),
    root: Box<dyn View>,
    rect: Rect,
    planes: Vec<Plane>,
    pub scope: ModalInfo,
    buttons: Vec<(Rect, Rect, Target)>,
}
struct Image {
    core: ViewCore,
    cells: Vec<(i16, i16, Cell)>,
}
impl View for Image {
    fn core(&self) -> &ViewCore {
        &self.core
    }
    fn core_mut(&mut self) -> &mut ViewCore {
        &mut self.core
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn get_palette(&self) -> Option<turbo_vision::core::palette::Palette> {
        None
    }
    fn can_focus(&self) -> bool {
        false
    }
    fn draw(&mut self, t: &mut Terminal) {
        for (x, y, cell) in &self.cells {
            t.write_cell(*x, *y, *cell);
        }
    }
    fn handle_event(&mut self, _: &mut Event) {}
}
fn tv(r: Rect) -> TvRect {
    TvRect::new(r.x as i16, r.y as i16, r.right() as i16, r.bottom() as i16)
}
fn attribute(g: &Glyph, p: PaletteProfile) -> u8 {
    let style = g.resolved(p);
    let fg = (0..16)
        .find(|i| Some(p.color(*i)) == style.fg)
        .unwrap_or(g.attribute & 15);
    let bg = (0..16)
        .find(|i| Some(p.color(*i)) == style.bg)
        .unwrap_or(g.attribute >> 4);
    fg | (bg << 4)
}
fn decode(ch: char, text: &TextCells) -> String {
    text.0.borrow().glyphs.get(&ch).cloned().unwrap_or_else(|| {
        if ch == '\0' {
            String::new()
        } else if ch.is_control() {
            "�".into()
        } else {
            ch.to_string()
        }
    })
}
impl DialogGroup {
    pub fn new(mut control: Native, rect: Rect, scope: ModalInfo) -> Self {
        control.view.set_bounds(tv(rect));
        let dialog = control.view.as_any_mut().downcast_mut::<Dialog>().unwrap();
        dialog.set_close_on(CloseOn::Commands(Vec::new()));
        dialog.set_state_flag(State::MODAL, true);
        Self {
            offset: (0, 0),
            root: control.view,
            rect,
            planes: vec![Plane {
                rect,
                text: control.text,
                appearance: Appearance::Toolkit,
                theme_group: None,
                source: None,
                button: false,
                original: rect,
            }],
            scope,
            buttons: Vec::new(),
        }
    }
    fn dialog(&mut self) -> &mut Dialog {
        self.root.as_any_mut().downcast_mut().unwrap()
    }
    pub fn control(&mut self, mut control: Native, mut area: Rect, screen_height: u16) {
        let original = area;
        let button = control.view.as_any().is::<Button>();
        if button {
            area.height = 2;
            let required = area.bottom().saturating_add(1);
            if required > self.rect.bottom() {
                self.rect.height =
                    (required - self.rect.y).min(screen_height.saturating_sub(self.rect.y));
                self.root.set_bounds(tv(self.rect));
            }
            if area.bottom() >= self.rect.bottom() {
                area.y = self.rect.bottom().saturating_sub(3);
            }
        }
        let local = Rect::new(
            area.x.saturating_sub(self.rect.x + 1),
            area.y.saturating_sub(self.rect.y + 1),
            area.width,
            area.height,
        );
        control.view.set_bounds(tv(local));
        let focused = control.view.is_focused();
        let prefer = focused
            && (!button
                || !self
                    .planes
                    .iter()
                    .any(|p| !p.button && matches!(p.appearance, Appearance::Input(true))));
        let id = self.dialog().add_boxed(control.view);
        if prefer {
            self.dialog().group_mut().focus_by_view_id(id);
        }
        self.planes.push(Plane {
            rect: area,
            text: control.text,
            appearance: control.appearance,
            theme_group: control.theme_group,
            source: None,
            button,
            original,
        });
    }
    pub fn image(&mut self, scene: &Scene, area: Rect) {
        let interior = Rect::new(
            self.rect.x + 1,
            self.rect.y + 1,
            self.rect.width.saturating_sub(2),
            self.rect.height.saturating_sub(2),
        );
        let area = area.intersection(interior);
        if area.is_empty() {
            return;
        }
        let mut text = TextCells::default();
        let mut cells = Vec::new();
        let mut original = BTreeMap::new();
        for (&(x, y), glyph) in &scene.cells {
            if !area.contains((x, y).into()) {
                continue;
            }
            original.insert((x, y), glyph.clone());
            if glyph.text.is_empty() {
                continue;
            }
            for (offset, ch) in text.text(&glyph.text).chars().enumerate() {
                if x + (offset as u16) < area.right() {
                    cells.push((
                        (x - area.x + offset as u16) as i16,
                        (y - area.y) as i16,
                        Cell::new(ch, Attr::from_u8(attribute(glyph, scene.profile))),
                    ));
                }
            }
        }
        let local = Rect::new(
            area.x - self.rect.x - 1,
            area.y - self.rect.y - 1,
            area.width,
            area.height,
        );
        self.dialog().add(Image {
            core: ViewCore::new(tv(local)),
            cells,
        });
        self.planes.push(Plane {
            rect: area,
            text,
            appearance: Appearance::Toolkit,
            theme_group: None,
            source: Some(original),
            button: false,
            original: area,
        });
    }
    pub fn paint(&mut self, scene: &mut Scene) {
        let mut terminal = super::surface::terminal(scene.width, scene.height);
        let mut background = TextCells::default();
        for (&(x, y), glyph) in &scene.cells {
            if glyph.text.is_empty() {
                continue;
            }
            for (i, ch) in background.text(&glyph.text).chars().enumerate() {
                terminal.write_cell(
                    (x + i as u16) as i16,
                    y as i16,
                    Cell::new(ch, Attr::from_u8(attribute(glyph, scene.profile))),
                );
            }
        }
        terminal.draw_view(&mut *self.root);
        let (sx, sy) = turbo_vision::core::state::shadow_size();
        let capture = Rect::new(
            self.rect.x,
            self.rect.y,
            self.rect.width.saturating_add(sx.max(0) as u16),
            self.rect.height.saturating_add(sy.max(0) as u16),
        );
        for y in capture.y..capture.bottom().min(scene.height) {
            for x in capture.x..capture.right().min(scene.width) {
                let cell = terminal.read_cell(x as i16, y as i16).unwrap();
                let plane = self.planes.iter().rev().find(|p| {
                    p.rect.contains((x, y).into())
                        && p.source.as_ref().is_none_or(|s| s.contains_key(&(x, y)))
                });
                let glyph = if let Some(plane) = plane {
                    if let Some(source) = &plane.source {
                        source[&(x, y)].clone()
                    } else {
                        let mut text = decode(cell.ch, &plane.text);
                        if x + text.width() as u16 > plane.rect.right() {
                            text = " ".into();
                        }
                        let mut attribute =
                            plane.appearance.attribute(cell.attr.to_u8(), scene.profile);
                        plane.appearance.button_cell(
                            &mut text,
                            &mut attribute,
                            (x - plane.rect.x, y - plane.rect.y),
                            (plane.rect.width, plane.rect.height),
                        );
                        Glyph {
                            text,
                            attribute,
                            style: scene.profile.themed(
                                plane
                                    .theme_group
                                    .unwrap_or_else(|| plane.appearance.group()),
                                attribute,
                            ),
                        }
                    }
                } else {
                    Glyph {
                        text: decode(cell.ch, &background),
                        attribute: cell.attr.to_u8(),
                        style: None,
                    }
                };
                scene.cells.insert((x, y), glyph);
            }
        }
        let bindings = self
            .planes
            .iter()
            .filter(|p| p.button)
            .filter_map(|p| {
                scene
                    .regions
                    .iter()
                    .rev()
                    .find(|(r, _)| {
                        r.x == p.original.x
                            && r.y == p.original.y
                            && r.width == p.original.width
                            && r.height <= p.original.height
                    })
                    .map(|(r, target)| (p.rect, *r, *target))
            })
            .collect::<Vec<_>>();
        self.buttons = bindings
            .iter()
            .map(|(actual, original, target)| {
                let face = Rect::new(actual.x, actual.y, original.width, original.height.min(1));
                for (rect, binding) in &mut scene.regions {
                    if *rect == *original && *binding == *target {
                        *rect = face;
                    }
                }
                (*actual, face, *target)
            })
            .collect();
    }
    /// Native Group performs the button hit and command proposal. The caller then
    /// invokes the existing guarded action at its original semantic target.
    pub(crate) fn bounds(&self) -> Rect {
        self.rect
    }
    pub(crate) fn translate_hits(&mut self, dx: i32, dy: i32) {
        self.offset = (dx, dy);
    }
    pub fn button(&mut self, x: u16, y: u16) -> Option<(Rect, Target)> {
        let x = (i32::from(x) - self.offset.0).try_into().ok()?;
        let y = (i32::from(y) - self.offset.1).try_into().ok()?;
        let (_, original, target) = self
            .buttons
            .iter()
            .rev()
            .find(|(r, _, _)| r.contains((x, y).into()))
            .copied()?;
        let pos = turbo_vision::core::geometry::Point::new(
            x as i16 - self.rect.x as i16,
            y as i16 - self.rect.y as i16,
        );
        let mut down = Event::mouse(EventType::MouseDown, pos, MB_LEFT_BUTTON, false);
        self.root.handle_event(&mut down);
        let mut up = Event::mouse(EventType::MouseUp, pos, 0, false);
        self.root.handle_event(&mut up);
        (up.what == EventType::Command || down.what == EventType::Command).then_some((
            Rect::new(
                (i32::from(original.x) + self.offset.0) as u16,
                (i32::from(original.y) + self.offset.1) as u16,
                original.width,
                original.height,
            ),
            target,
        ))
    }
}

/// Native semi-transparent shadow, seeded with the actual underlying cells.
pub(crate) fn paint_shadow(scene: &mut Scene, rect: Rect) {
    let mut terminal = super::surface::terminal(scene.width, scene.height);
    let mut encoding = TextCells::default();
    for (&(x, y), glyph) in &scene.cells {
        if glyph.text.is_empty() {
            continue;
        }
        for (i, ch) in encoding.text(&glyph.text).chars().enumerate() {
            terminal.write_cell(
                (x + i as u16) as i16,
                y as i16,
                Cell::new(ch, Attr::from_u8(attribute(glyph, scene.profile))),
            );
        }
    }
    turbo_vision::views::view::draw_shadow_bounds(&mut terminal, tv(rect));
    let (sx, sy) = turbo_vision::core::state::shadow_size();
    let sx = sx.max(0) as u16;
    let sy = sy.max(0) as u16;
    for y in rect.y
        ..rect
            .bottom()
            .saturating_add(sy)
            .min(scene.height.saturating_sub(1))
    {
        for x in rect.x..rect.right().saturating_add(sx).min(scene.width) {
            if (x >= rect.right() && y >= rect.y + sy)
                || (y >= rect.bottom() && x >= rect.x + sx && x < rect.right())
            {
                let cell = terminal.read_cell(x as i16, y as i16).unwrap();
                scene.cells.insert(
                    (x, y),
                    Glyph {
                        text: decode(cell.ch, &encoding),
                        attribute: cell.attr.to_u8(),
                        style: None,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        focus::Focus,
        modals::{ModalId, ModalOwner},
    };
    #[test]
    fn sparse_captions_never_capture_the_obscured_document() {
        use crate::painting::{Frame, NativeFrame, Paragraph};
        let mut frame = NativeFrame::new(Rect::new(0, 0, 80, 25), PaletteProfile::ClassicDn);
        for y in 0..25 {
            frame.render_widget(Paragraph::new("Z".repeat(80)), Rect::new(0, y, 80, 1));
        }
        frame.modal_scope(Some(ModalInfo {
            id: ModalId(99),
            owner: ModalOwner::Workspace,
            parent: None,
            return_document: None,
            focus: Focus::Prompt,
        }));
        let rect = Rect::new(10, 5, 49, 10);
        frame.render_widget(super::super::widgets::dialog("Move / rename to"), rect);
        // Short text in a large caption area used to capture the pane behind it.
        frame.render_widget(Paragraph::new("Destination"), Rect::new(12, 7, 45, 5));
        frame.render_widget(
            super::super::widgets::button("OK", true),
            Rect::new(25, 12, 10, 1),
        );
        frame.modal_scope(None);
        let scene = frame.finish();
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                assert_ne!(scene.cells[&(x, y)].text, "Z", "leaked pane at {x},{y}");
            }
        }
        assert_eq!(scene.cells[&(5, 7)].text, "Z");
        assert_eq!(scene.cells[&(40, 10)].text, " ");
        assert_eq!(
            scene.cells[&(40, 10)].resolved(scene.profile).bg,
            Some(scene.profile.color(7))
        );
    }
    #[test]
    fn composed_buttons_have_native_children_shadow_space_and_owner_relative_hits() {
        for (root, button, screen_height) in [
            (Rect::new(7, 3, 49, 7), Rect::new(20, 8, 12, 1), 30),
            (Rect::new(0, 0, 80, 25), Rect::new(20, 23, 12, 1), 25),
        ] {
            let scope = ModalInfo {
                id: ModalId(42),
                owner: ModalOwner::Workspace,
                parent: None,
                return_document: None,
                focus: Focus::Message,
            };
            let mut group = DialogGroup::new(super::super::widgets::dialog("Test"), root, scope);
            group.control(
                super::super::widgets::button("OK", true),
                button,
                screen_height,
            );
            assert_eq!(group.dialog().group().len(), 1);
            let bounds = group.dialog().group().child_at(0).bounds();
            assert_eq!(bounds.width(), 12);
            assert_eq!(bounds.height(), 2);
            assert_eq!(group.scope, scope);
            let mut scene = Scene {
                width: 100,
                height: screen_height,
                profile: PaletteProfile::ClassicDn,
                cells: BTreeMap::new(),
                regions: vec![(button, Target::CloseDialog)],
                cursor: None,
            };
            group.paint(&mut scene);
            let (actual, face, target) = group.buttons[0];
            assert!(
                actual.bottom() < group.rect.bottom(),
                "shadow must precede bottom border"
            );
            assert_eq!(face.height, 1);
            assert_eq!(scene.regions[0], (face, target));
            assert_eq!(group.button(face.x + 2, face.y), Some((face, target)));
            assert_eq!(group.button(group.rect.x, group.rect.y), None);
        }
    }
}
