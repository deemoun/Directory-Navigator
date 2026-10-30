//! CDUTIL.PAS TOrder: track/time/description list and nested Add track chooser.
//! UI demonstration only; sample rows never name or read actual files.
//! Based on Dos Navigator by RIT Research Labs; see THIRD_PARTY_NOTICES.md.
use super::*;
use crossterm::event::KeyEvent;
pub(super) const LIST: usize = 12;
const OK: usize = 200;
const ADD: usize = 201;
const DELETE: usize = 202;
const CANCEL: usize = 203;
const CHOOSE: usize = 210;
const BACK: usize = 211;
const FIELD: usize = 220;
const SAVE: usize = 221;
const SCROLL: usize = 222;
const ROW: usize = 1000;
const TRACKS: usize = 12;
const MAX_ORDER: usize = 64;
const ROWS: usize = 9;
pub(super) const ORDER_PAGE: u64 = 1;
pub(super) const PICKER_PAGE: u64 = 2;
#[derive(Default)]
struct Selection {
    cursor: usize,
    offset: usize,
    focus: usize,
}
impl Selection {
    fn fit(&mut self, count: usize) {
        crate::list_state::ListViewport::fit(
            &mut self.cursor,
            &mut self.offset,
            ROWS,
            Some(count.saturating_sub(1)),
            Some(count.saturating_sub(ROWS)),
        );
    }
    fn key(&mut self, key: KeyEvent, count: usize) {
        if self.focus == 0 {
            if let Some(row) = crate::turbo::widgets::list_key(self.cursor, count, ROWS, key.code) {
                self.cursor = row;
                self.fit(count);
            }
        }
    }
}
pub(super) struct Playlist {
    pub order: Vec<usize>,
    draft: Vec<usize>,
    selection: Selection,
    picker: Option<Selection>,
    pub open: bool,
}
impl Default for Playlist {
    fn default() -> Self {
        Self {
            order: (0..TRACKS).collect(),
            draft: Vec::new(),
            selection: Selection::default(),
            picker: None,
            open: false,
        }
    }
}
impl Playlist {
    pub fn show(&mut self) {
        self.draft = self.order.clone();
        self.selection = Selection::default();
        self.picker = None;
        self.open = true;
    }
    pub fn action(&mut self, id: usize) {
        if (SCROLL..SCROLL + 4).contains(&id) {
            let code = [K::Up, K::Down, K::PageUp, K::PageDown][id - SCROLL];
            if let Some(p) = &mut self.picker {
                p.focus = 0;
            } else {
                self.selection.focus = 0;
            }
            self.key(KeyEvent::new(code, crossterm::event::KeyModifiers::NONE));
            return;
        }
        if self.picker.is_some() {
            match id {
                CHOOSE => {
                    let selected = self.picker.as_ref().unwrap().cursor;
                    if self.draft.len() < MAX_ORDER {
                        self.draft.push(selected);
                        self.selection.cursor = self.draft.len() - 1;
                        self.selection.fit(self.draft.len());
                    }
                    self.picker = None;
                    self.selection.focus = if self.draft.len() < MAX_ORDER { 2 } else { 1 };
                }
                BACK => {
                    self.picker = None;
                    self.selection.focus = 2;
                }
                FIELD => self.picker.as_mut().unwrap().focus = 0,
                ROW.. if id < ROW + TRACKS => {
                    let p = self.picker.as_mut().unwrap();
                    p.cursor = id - ROW;
                    p.focus = 0;
                    p.fit(TRACKS);
                }
                _ => {}
            }
        } else {
            match id {
                OK => {
                    self.order = self.draft.clone();
                    self.open = false;
                }
                CANCEL => self.open = false,
                ADD if self.draft.len() < MAX_ORDER => {
                    self.selection.focus = 2;
                    self.picker = Some(Selection::default());
                }
                DELETE if !self.draft.is_empty() => {
                    self.draft.remove(self.selection.cursor);
                    self.selection.fit(self.draft.len());
                    self.selection.focus = if self.draft.is_empty() { 2 } else { 3 };
                }
                FIELD => self.selection.focus = 0,
                ROW.. if id < ROW + self.draft.len() => {
                    self.selection.cursor = id - ROW;
                    self.selection.focus = 0;
                    self.selection.fit(self.draft.len());
                }
                _ => {}
            }
        }
    }
    pub fn focus(&mut self, id: usize) {
        if let Some(p) = &mut self.picker {
            p.focus = id;
        } else {
            self.selection.focus = id;
        }
    }
    pub fn key(&mut self, key: KeyEvent) {
        if let Some(p) = &mut self.picker {
            p.key(key, TRACKS);
        } else {
            self.selection.key(key, self.draft.len());
        }
    }
    pub fn wheel(&mut self, down: bool) {
        let code = if down { K::Down } else { K::Up };
        if let Some(p) = &mut self.picker {
            p.focus = 0;
        } else {
            self.selection.focus = 0;
        }
        self.key(KeyEvent::new(code, crossterm::event::KeyModifiers::NONE));
    }
    pub fn page(&self) -> u64 {
        if self.picker.is_some() {
            PICKER_PAGE
        } else {
            ORDER_PAGE
        }
    }
    pub fn bounds(ctx: &Context) -> Rect {
        crate::turbo::dialog_spec::centered(ctx.area, 68, 20)
    }
    pub fn windows(&self, ctx: &Context) -> Vec<LocalWindow> {
        let r = Self::bounds(ctx);
        let mut result = vec![LocalWindow {
            id: ORDER_PAGE,
            parent: Some(0),
            bounds: r,
            caps: Capabilities::fixed(r.width, r.height),
        }];
        if self.picker.is_some() {
            result.push(LocalWindow {
                id: PICKER_PAGE,
                parent: Some(ORDER_PAGE),
                bounds: r,
                caps: Capabilities::fixed(r.width, r.height),
            });
        }
        result
    }
    pub fn controls(&self, ctx: &Context, picker: bool) -> DialogSpec<'static> {
        let r = Self::bounds(ctx);
        let language = ctx.language;
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let selected = if picker {
            self.picker.as_ref().unwrap()
        } else {
            &self.selection
        };
        let title = if picker { "Select track" } else { "Play order" };
        let mut spec = ctx.dialog(
            r,
            selected.focus,
            language.text(title),
            if picker { CHOOSE } else { OK },
            if picker { BACK } else { CANCEL },
        );
        spec.caption(
            Paragraph::new(language.text("Example tracks · no files loaded")).style(style(0x78)),
            Rect::new(r.x + 3, r.y + 1, 62, 1),
        );
        let header = format!(
            "{:>3} │ {:^5} │ {}",
            language.text("No."),
            language.text("Time"),
            language.text("Description")
        );
        spec.caption(
            Paragraph::new(header).style(style(0x7e)),
            Rect::new(r.x + 3, r.y + 2, 61, 1),
        );
        spec.native(
            crate::turbo::widgets::group_box(""),
            Rect::new(r.x + 2, r.y + 3, 64, 11),
        );
        let tracks = if picker {
            (0..TRACKS).collect::<Vec<_>>()
        } else {
            self.draft.clone()
        };
        let items = tracks
            .iter()
            .map(|&id| {
                let number = if picker && self.draft.contains(&id) {
                    format!("*{:>2}", id + 1)
                } else {
                    format!("{:>3}", id + 1)
                };
                format!(
                    "{number} │ --:-- │ {} {:02}",
                    language.text("Demo track"),
                    id + 1
                )
            })
            .collect::<Vec<_>>();
        let list = Rect::new(r.x + 3, r.y + 4, 61, ROWS as u16);
        let mut control = Control::new(
            0,
            list,
            ControlKind::List {
                items,
                cursor: selected.cursor,
                offset: selected.offset,
            },
            ctx.target(FIELD),
        );
        control.dn_list = true;
        control.hits = (selected.offset..tracks.len().min(selected.offset + ROWS))
            .map(|row| {
                (
                    Rect::new(
                        list.x,
                        list.y + (row - selected.offset) as u16,
                        list.width,
                        1,
                    ),
                    ctx.target(ROW + row),
                )
            })
            .collect();
        // Empty lists still have a focusable client area, but no selectable rows.
        if tracks.is_empty() {
            control.hits.push((list, ctx.target(FIELD)));
        }
        spec.control(control);
        spec.native(
            crate::turbo::widgets::scrollbar(selected.cursor, tracks.len().saturating_sub(1), ROWS),
            Rect::new(r.x + 64, r.y + 4, 1, ROWS as u16),
        );
        for (y, h, command) in [
            (4, 1, SCROLL),
            (12, 1, SCROLL + 1),
            (5, 3, SCROLL + 2),
            (8, 4, SCROLL + 3),
        ] {
            spec.hit(Rect::new(r.x + 64, r.y + y, 1, h), ctx.target(command));
        }
        if tracks.is_empty() {
            spec.caption(
                Paragraph::new(language.text("Empty demo playlist")).style(style(0x30)),
                Rect::new(list.x, list.y, list.width, 1),
            );
        }
        spec.caption(
            Paragraph::new(language.text(if picker {
                "* Already in the draft playlist"
            } else {
                "Edit the demo order · OK keeps it until this player closes"
            }))
            .style(style(0x70)),
            Rect::new(r.x + 3, r.y + 14, 62, 1),
        );
        spec.caption(
            Paragraph::new(language.text("UI prototype · no playback or file saving"))
                .style(style(0x78)),
            Rect::new(r.x + 3, r.y + 15, 62, 1),
        );
        let buttons = if picker {
            vec![
                (1, "OK", CHOOSE, 22, 12, true),
                (2, "Cancel", BACK, 35, 12, true),
            ]
        } else {
            vec![
                (1, "OK", OK, 2, 12, true),
                (2, "Add", ADD, 15, 12, self.draft.len() < MAX_ORDER),
                (3, "Delete", DELETE, 28, 12, !self.draft.is_empty()),
                (4, "Save", SAVE, 41, 12, false),
                (5, "Cancel", CANCEL, 54, 12, true),
            ]
        };
        for (id, label, action, x, width, enabled) in buttons {
            let mut c = Control::new(
                id,
                Rect::new(r.x + x, r.y + 17, width, 1),
                ControlKind::Button(language.text(label).into()),
                ctx.target(action),
            );
            c.enabled = enabled;
            spec.control(c);
        }
        spec.caption(
            Paragraph::new(Line::from(vec![
                Span::styled("[", style(0x7f)),
                Span::styled("■", style(0x7a)),
                Span::styled("]", style(0x7f)),
            ])),
            Rect::new(r.x + 2, r.y, 3, 1),
        );
        spec
    }
    pub fn layer(&self, ctx: &Context, id: u64) -> Layer<'static> {
        let picker = id == PICKER_PAGE;
        let spec = self.controls(ctx, picker);
        let mut frame = NativeFrame::new(ctx.area, ctx.palette);
        frame.render_widget(
            crate::turbo::widgets::dialog(ctx.language.text(if picker {
                "Select track"
            } else {
                "Play order"
            })),
            spec.bounds,
        );
        Layer {
            id,
            interactive: id == self.page(),
            scene: frame.finish(),
            controls: Some(spec),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn demo_order_limit_and_cancelled_picker_never_change_committed_rows() {
        let mut p = Playlist::default();
        let original = p.order.clone();
        p.show();
        p.action(ADD);
        p.action(BACK);
        assert_eq!(p.draft, original);
        while p.draft.len() < MAX_ORDER {
            p.action(ADD);
            p.action(CHOOSE);
        }
        p.action(ADD);
        assert!(p.picker.is_none());
        assert_eq!(p.draft.len(), MAX_ORDER);
        p.action(CANCEL);
        assert_eq!(p.order, original);
        p.show();
        assert_eq!(p.draft, original);
        for _ in 0..TRACKS {
            p.action(DELETE);
        }
        p.action(DELETE);
        assert!(p.draft.is_empty());
        assert_eq!(p.selection.offset, 0);
        p.action(SAVE);
        assert_eq!(p.order, original);
        p.action(OK);
        assert!(p.order.is_empty());
    }
}
