//! Live session-only queue. Navigation never changes the playing track.
//! Layout adapted from DN CDUTIL.PAS; see THIRD_PARTY_NOTICES.md.
use super::*;
use crossterm::event::KeyEvent;
pub(super) const LIST: usize = 12;
pub(super) const PLAY_TRACK: usize = 200;
pub(super) const ADD: usize = 201;
pub(super) const DELETE: usize = 202;
pub(super) const CLOSE_LIST: usize = 203;
const FIELD: usize = 220;
const SCROLL: usize = 222;
const ROW: usize = 1000;
pub(super) const MAX_ORDER: usize = 64;
const ROWS: usize = 9;
pub(super) const ORDER_PAGE: u64 = 1;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Track {
    pub id: u64,
    pub path: PathBuf,
    pub tags: tags::Tags,
    pub duration: Option<Duration>,
}
impl Track {
    pub fn description(&self) -> String {
        self.tags.description(&self.name())
    }
    pub fn name(&self) -> String {
        crate::filesystem::display_name(
            &self.path.file_name().unwrap_or_default().to_string_lossy(),
        )
    }
}
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
#[derive(Default)]
pub(super) struct Playlist {
    pub order: Vec<Track>,
    selection: Selection,
    pub open: bool,
}
impl Playlist {
    pub fn show(&mut self) {
        self.selection = Selection::default();
        self.open = true;
    }
    pub fn selected(&self) -> Option<u64> {
        self.order.get(self.selection.cursor).map(|t| t.id)
    }
    pub fn append(&mut self, tracks: impl Iterator<Item = Track>) -> usize {
        let rows = &mut self.order;
        let before = rows.len();
        rows.extend(tracks.take(MAX_ORDER.saturating_sub(before)));
        rows.len() - before
    }
    pub fn update_duration(&mut self, id: u64, duration: Duration) {
        for t in self.order.iter_mut().filter(|t| t.id == id) {
            t.duration = Some(duration);
        }
    }
    pub fn available(&self) -> usize {
        MAX_ORDER.saturating_sub(self.order.len())
    }
    pub fn can_add(&self) -> bool {
        self.available() > 0
    }
    pub fn action(&mut self, id: usize) {
        if (SCROLL..SCROLL + 4).contains(&id) {
            self.selection.focus = 0;
            self.key(KeyEvent::new(
                [K::Up, K::Down, K::PageUp, K::PageDown][id - SCROLL],
                crossterm::event::KeyModifiers::NONE,
            ));
            return;
        }
        match id {
            CLOSE_LIST => self.open = false,
            DELETE if !self.order.is_empty() => {
                self.order.remove(self.selection.cursor);
                self.selection.fit(self.order.len());
                self.selection.focus = if self.order.is_empty() { 2 } else { 3 };
            }
            FIELD => self.selection.focus = 0,
            ROW.. if id < ROW + self.order.len() => {
                self.selection.cursor = id - ROW;
                self.selection.focus = 0;
                self.selection.fit(self.order.len());
            }
            _ => {}
        }
    }
    pub fn focus(&mut self, id: usize) {
        self.selection.focus = id;
    }
    pub fn key(&mut self, key: KeyEvent) {
        self.selection.key(key, self.order.len());
    }
    pub fn wheel(&mut self, down: bool) {
        self.selection.focus = 0;
        self.key(KeyEvent::new(
            if down { K::Down } else { K::Up },
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    pub fn bounds(ctx: &Context) -> Rect {
        crate::turbo::dialog_spec::centered(ctx.area, 68, 20)
    }
    pub fn windows(&self, ctx: &Context) -> Vec<LocalWindow> {
        let r = Self::bounds(ctx);
        vec![LocalWindow {
            id: ORDER_PAGE,
            parent: Some(0),
            bounds: r,
            caps: Capabilities::fixed(r.width, r.height),
        }]
    }
    pub fn controls(&self, ctx: &Context) -> DialogSpec<'static> {
        let r = Self::bounds(ctx);
        let language = ctx.language;
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let selected = &self.selection;
        let mut spec = ctx.dialog(
            r,
            selected.focus,
            language.text("Playlist"),
            PLAY_TRACK,
            CLOSE_LIST,
        );
        spec.caption(
            Paragraph::new(language.text("Music queue · current session")).style(style(0x78)),
            Rect::new(r.x + 3, r.y + 1, 62, 1),
        );
        let time_width = self
            .order
            .iter()
            .filter_map(|t| t.duration)
            .map(|d| format_time(d).len())
            .max()
            .unwrap_or(5)
            .max(5);
        let header = format!(
            "{:>3} │ {:^time_width$} │ {}",
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
        let tracks = &self.order;
        let items = tracks
            .iter()
            .enumerate()
            .map(|(n, track)| {
                format!(
                    "{:>3} │ {:>time_width$} │ {}",
                    n + 1,
                    track.duration.map_or_else(|| "--:--".into(), format_time),
                    track.description()
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
                Paragraph::new(language.text("Empty playlist")).style(style(0x30)),
                Rect::new(list.x, list.y, list.width, 1),
            );
        }
        spec.caption(
            Paragraph::new(
                language.text("Add… chooses a music file without interrupting playback"),
            )
            .style(style(0x70)),
            Rect::new(r.x + 3, r.y + 14, 62, 1),
        );
        spec.caption(
            Paragraph::new(language.text("Enter plays · Changes apply immediately"))
                .style(style(0x78)),
            Rect::new(r.x + 3, r.y + 15, 62, 1),
        );
        let buttons = vec![
            (
                1,
                "Play",
                PLAY_TRACK,
                3,
                14,
                !self.order.is_empty() && cfg!(target_os = "macos"),
            ),
            (
                2,
                "Add…",
                ADD,
                19,
                14,
                cfg!(target_os = "macos") && self.can_add(),
            ),
            (3, "Remove", DELETE, 35, 14, !self.order.is_empty()),
            (4, "Close", CLOSE_LIST, 51, 14, true),
        ];
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
    pub fn layer(&self, ctx: &Context) -> Layer<'static> {
        let spec = self.controls(ctx);
        let mut frame = NativeFrame::new(ctx.area, ctx.palette);
        frame.render_widget(
            crate::turbo::widgets::dialog(ctx.language.text("Playlist")),
            spec.bounds,
        );
        Layer {
            id: ORDER_PAGE,
            interactive: true,
            scene: frame.finish(),
            controls: Some(spec),
        }
    }
}
