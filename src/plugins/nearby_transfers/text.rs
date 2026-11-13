//! Session-only composer and received preview. Filesystem/clipboard access stays in the host.
use super::*;
use crate::{
    editor::buffer::Buffer,
    presentation::text::{Line, Span},
    turbo::dialog_spec::centered,
};
use crossterm::event::{KeyModifiers as M, MouseEvent};
use unicode_segmentation::UnicodeSegmentation;
pub(super) const BODY: usize = 30;
pub(super) const OPEN: usize = 31;
pub(super) const SEND: usize = 32;
pub(super) const CANCEL: usize = 33;
pub(super) const PASTE: usize = 34;
pub(super) const CLEAR: usize = 35;
pub(super) const ACCEPT: usize = 36;
pub(super) const DECLINE: usize = 37;
pub(super) const COPY: usize = 38;
pub(super) const DISMISS: usize = 39;
pub(super) struct TextUi {
    pub draft: Buffer,
    pub composing: bool,
    pub session: u64,
    pub address: String,
    preview: Option<(u64, Buffer)>,
    offset: usize,
    horizontal: usize,
    pub notice: String,
}
impl Default for TextUi {
    fn default() -> Self {
        Self {
            draft: Buffer::new(String::new()).expect("empty buffer"),
            composing: false,
            session: 0,
            address: String::new(),
            preview: None,
            offset: 0,
            horizontal: 0,
            notice: String::new(),
        }
    }
}
impl TextUi {
    pub fn open(&mut self) {
        self.composing = true;
        self.session = self.session.wrapping_add(1);
        self.offset = 0;
        self.horizontal = 0;
        self.notice.clear();
    }
    pub fn token(&self) -> ClipboardToken {
        ClipboardToken {
            session: self.session,
            revision: self.draft.revision(),
            cursor: self.draft.cursor,
            anchor: self.draft.anchor,
        }
    }
    pub fn insert(&mut self, text: &str) {
        if self.draft.text.len() - self.draft.selected().len() + text.len()
            > crate::peer::wire::TEXT_LIMIT
        {
            self.notice = "Text limit is 64 KiB; nothing was inserted.".into();
            return;
        }
        if text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
        {
            self.notice = "Control characters cannot be composed; nothing was inserted.".into();
            return;
        }
        match self.draft.replace(self.draft.selected(), text) {
            Ok(()) => self.notice.clear(),
            Err(error) => self.notice = error,
        }
    }
    pub fn prepare(&mut self, ctx: &Context, page: u8) {
        if let Some(received) = &ctx.nearby.received_text {
            if self.preview.as_ref().map(|p| p.0) != Some(received.serial) {
                // Display escapes controls; the separate received payload stays exact.
                let display = received
                    .text
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .split('\n')
                    .map(crate::filesystem::display_name)
                    .collect::<Vec<_>>()
                    .join("\n");
                self.preview = Some((
                    received.serial,
                    Buffer::new(display).expect("bounded escaped text"),
                ));
                self.offset = 0;
                self.horizontal = 0;
                self.notice.clear();
            }
        } else {
            self.preview = None;
        }
        if page == 3 {
            let row = self.draft.row();
            let col = self.draft.column();
            if row < self.offset {
                self.offset = row;
            } else if row >= self.offset + 10 {
                self.offset = row.saturating_sub(9);
            }
            if col < self.horizontal {
                self.horizontal = col;
            } else if col >= self.horizontal + 62 {
                self.horizontal = col.saturating_sub(61);
            }
        }
    }
    pub fn key(&mut self, key: KeyEvent, page: u8) -> Vec<Effect> {
        if page == 5 {
            let rows = self.preview.as_ref().map_or(0, |p| p.1.lines.len());
            match key.code {
                K::Up => self.offset = self.offset.saturating_sub(1),
                K::Down => self.offset = (self.offset + 1).min(rows.saturating_sub(1)),
                K::PageUp => self.offset = self.offset.saturating_sub(10),
                K::PageDown => self.offset = (self.offset + 10).min(rows.saturating_sub(1)),
                K::Home => {
                    self.offset = 0;
                    self.horizontal = 0;
                }
                K::End => self.offset = rows.saturating_sub(10),
                K::Left => self.horizontal = self.horizontal.saturating_sub(8),
                K::Right => {
                    self.horizontal = (self.horizontal + 8).min(crate::peer::wire::TEXT_LIMIT * 4)
                }
                _ => {}
            }
            return vec![];
        }
        let shift = key.modifiers.contains(M::SHIFT);
        let ctrl = key.modifiers.contains(M::CONTROL);
        match key.code {
            K::Insert if shift => return vec![Effect::PasteText(self.token())],
            K::Char('v') if ctrl => return vec![Effect::PasteText(self.token())],
            K::Char('a') if ctrl => {
                self.draft.move_to(0, false);
                self.draft.move_to(self.draft.text.len(), true);
            }
            K::Char('c') if ctrl => {
                return vec![Effect::CopyText(
                    self.draft.text[self.draft.selected()].to_owned(),
                )]
            }
            K::Char('z') if ctrl => self.draft.undo(false),
            K::Char('y') if ctrl => self.draft.undo(true),
            K::Left => self.draft.horizontal(false, shift),
            K::Right => self.draft.horizontal(true, shift),
            K::Up => self.draft.vertical(-1, shift),
            K::Down => self.draft.vertical(1, shift),
            K::PageUp => self.draft.vertical(-10, shift),
            K::PageDown => self.draft.vertical(10, shift),
            K::Home => {
                let pos = if ctrl {
                    0
                } else {
                    self.draft.lines[self.draft.row()].start
                };
                self.draft.move_to(pos, shift);
            }
            K::End => {
                let pos = if ctrl {
                    self.draft.text.len()
                } else {
                    self.draft.lines[self.draft.row()].end
                };
                self.draft.move_to(pos, shift);
            }
            K::Backspace | K::Delete => {
                if let Err(error) = self.draft.delete(key.code == K::Backspace) {
                    self.notice = error;
                }
            }
            K::Enter => self.insert("\n"),
            K::Char(c) if !key.modifiers.intersects(M::CONTROL | M::ALT) => {
                self.insert(&c.to_string())
            }
            _ => {}
        }
        vec![]
    }
    pub fn mouse(&mut self, event: MouseEvent, ctx: &Context, page: u8) {
        if page == 3
            && matches!(
                event.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            )
        {
            self.draft.vertical(
                if event.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                },
                false,
            );
            return;
        }
        if event.kind == MouseEventKind::ScrollUp {
            self.offset = self.offset.saturating_sub(3);
            return;
        }
        if event.kind == MouseEventKind::ScrollDown {
            self.offset = self.offset.saturating_add(3).min(
                self.preview
                    .as_ref()
                    .map_or(self.draft.lines.len(), |p| p.1.lines.len())
                    .saturating_sub(1),
            );
            return;
        }
        if page != 3
            || !matches!(
                event.kind,
                MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)
            )
        {
            return;
        }
        let r = centered(ctx.area, 70, 20);
        let row = self.offset + usize::from(event.row.saturating_sub(r.y + 4)).min(9);
        let col = self.horizontal + usize::from(event.column.saturating_sub(r.x + 3));
        let pos = self
            .draft
            .position_at(row.min(self.draft.lines.len() - 1), col);
        self.draft.move_to(
            pos,
            event.modifiers.contains(M::SHIFT) || matches!(event.kind, MouseEventKind::Drag(_)),
        );
    }
    pub fn controls<'a>(&'a self, ctx: &Context, page: u8, focus: usize) -> DialogSpec<'a> {
        let r = centered(ctx.area, 70, 20);
        let (title, cancel) = match page {
            3 => ("Send text", CANCEL),
            4 => ("Incoming text", DECLINE),
            _ => ("Received text", DISMISS),
        };
        let mut spec = ctx.dialog(r, focus, title, cancel, cancel);
        let caption = |spec: &mut DialogSpec<'a>, value: String, y: u16, h: u16| {
            spec.caption(
                Paragraph::new(value).wrap(Wrap { trim: false }),
                Rect::new(r.x + 3, r.y + y, r.width - 6, h),
            )
        };
        let button = |spec: &mut DialogSpec<'a>, id, label: &str, x, width, enabled| {
            let mut control = Control::new(
                id,
                Rect::new(r.x + x, r.y + 17, width, 1),
                ControlKind::Button(ctx.language.text(label).into()),
                ctx.target(id),
            );
            control.enabled = enabled;
            spec.control(control);
        };
        if page == 4 {
            if let Some((peer, bytes)) = &ctx.nearby.text_offer {
                caption(&mut spec, crate::filesystem::display_name(peer), 2, 2);
                caption(
                    &mut spec,
                    format!("{}: {bytes}", ctx.language.text("Text bytes")),
                    5,
                    1,
                );
                caption(
                    &mut spec,
                    ctx.language
                        .text("Accept to receive and preview. Your clipboard will not change.")
                        .into(),
                    8,
                    3,
                );
            }
            button(&mut spec, ACCEPT, "Accept", 8, 22, true);
            button(&mut spec, DECLINE, "Decline", 39, 22, true);
        } else {
            let buffer = if page == 3 {
                Some(&self.draft)
            } else {
                self.preview.as_ref().map(|p| &p.1)
            };
            let heading = if page == 3 {
                format!(
                    "{} / 65536 {}",
                    self.draft.text.len(),
                    ctx.language.text("UTF-8 bytes")
                )
            } else {
                ctx.nearby
                    .received_text
                    .as_ref()
                    .map(|p| {
                        format!(
                            "{} · {} {}",
                            crate::filesystem::display_name(&p.peer),
                            p.text.len(),
                            ctx.language.text("UTF-8 bytes")
                        )
                    })
                    .unwrap_or_default()
            };
            caption(&mut spec, heading, 2, 1);
            let bounds = Rect::new(r.x + 3, r.y + 4, r.width - 6, 10);
            spec.control(Control::new(
                BODY,
                bounds,
                ControlKind::Surface,
                ctx.target(BODY),
            ));
            if let Some(buffer) = buffer {
                let styles = ctx
                    .palette
                    .styles_for(crate::palette::theme::Group::Dialogs);
                let selected = buffer.selected();
                for (dy, row) in (self.offset..buffer.lines.len()).take(10).enumerate() {
                    let mut column = 0;
                    let mut spans = Vec::new();
                    for (index, g) in buffer.line_text(row).grapheme_indices(true) {
                        let width = buffer.cell_width(g, column);
                        let start = column;
                        column += width;
                        if column <= self.horizontal {
                            continue;
                        }
                        if start >= self.horizontal + usize::from(bounds.width) {
                            break;
                        }
                        let shown = if g == "\t"
                            || start < self.horizontal
                            || column > self.horizontal + usize::from(bounds.width)
                        {
                            " ".repeat(
                                column.min(self.horizontal + usize::from(bounds.width))
                                    - start.max(self.horizontal),
                            )
                        } else {
                            g.to_owned()
                        };
                        let pos = buffer.lines[row].start + index;
                        spans.push(Span::styled(
                            shown,
                            styles(if page == 3 && selected.contains(&pos) {
                                0x1f
                            } else {
                                0x70
                            }),
                        ));
                    }
                    spec.caption(
                        Paragraph::new(Line::from(spans)),
                        Rect::new(bounds.x, bounds.y + dy as u16, bounds.width, 1),
                    );
                }
                if page == 3 && focus == BODY {
                    spec.cursor = Some((
                        bounds.x
                            + buffer
                                .column()
                                .saturating_sub(self.horizontal)
                                .min(usize::from(bounds.width) - 1)
                                as u16,
                        bounds.y + buffer.row().saturating_sub(self.offset).min(9) as u16,
                    ));
                }
            }
            caption(
                &mut spec,
                ctx.language
                    .text(if page == 3 {
                        "Enter: newline · Tab: controls · Ctrl+A: select all"
                    } else {
                        "Arrows / PgUp / PgDn: scroll · Links are plain text"
                    })
                    .into(),
                14,
                1,
            );
            caption(
                &mut spec,
                crate::filesystem::display_name(ctx.language.text(&self.notice)),
                15,
                2,
            );
            if page == 3 {
                button(&mut spec, PASTE, "Paste", 3, 14, true);
                button(&mut spec, CLEAR, "Clear", 19, 14, true);
                button(
                    &mut spec,
                    SEND,
                    "Send",
                    35,
                    14,
                    !ctx.nearby.busy && !self.draft.text.is_empty(),
                );
                button(&mut spec, CANCEL, "Cancel", 51, 14, true);
            } else {
                button(&mut spec, COPY, "Copy", 8, 22, true);
                button(&mut spec, DISMISS, "Dismiss", 39, 22, true);
            }
        }
        spec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composer_limits_preserve_exact_text_and_clipboard_tokens_track_all_edits() {
        let mut ui = TextUi::default();
        ui.open();
        let original = "https://example.test/Ёжик?q=世界\r\nsecond\tline";
        ui.insert(original);
        assert_eq!(ui.draft.text, original);
        let token = ui.token();
        ui.draft.horizontal(false, false);
        assert_ne!(ui.token(), token);
        let before = ui.draft.text.clone();
        ui.insert(&"x".repeat(crate::peer::wire::TEXT_LIMIT));
        assert_eq!(ui.draft.text, before);
        ui.insert("\x1b[31m");
        assert_eq!(ui.draft.text, before);
        ui.draft.move_to(0, false);
        ui.draft.move_to(ui.draft.text.len(), true);
        ui.insert(&"Ё".repeat(crate::peer::wire::TEXT_LIMIT / 2));
        assert_eq!(ui.draft.text.len(), crate::peer::wire::TEXT_LIMIT);
        let before = ui.draft.text.clone();
        ui.insert("x");
        assert_eq!(ui.draft.text, before);
        ui.composing = false;
        ui.open();
        assert_ne!(ui.token().session, token.session);
        assert_eq!(ui.draft.text, before);
    }
}
