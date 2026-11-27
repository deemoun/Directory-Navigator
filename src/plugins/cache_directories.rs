//! Read-only presentation; the host owns discovery, scans and navigation.
use super::*;
use crate::{
    cache_locations::Action,
    painting::Paragraph,
    turbo::dialog_spec::{centered, Control, ControlKind},
    windowing::geometry::Capabilities,
};
use crossterm::event::{KeyCode as K, MouseButton, MouseEventKind};
#[derive(Default)]
pub(crate) struct CacheDirectories {
    clipboard_notice: String,
    focus: usize,
    row: usize,
    offset: usize,
    selected: Option<PathBuf>,
    observed: u64,
    last_click: Option<(Option<PathBuf>, Instant)>,
}
impl CacheDirectories {
    fn act(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        self.clipboard_notice.clear();
        let action = match id {
            0 | 5 => {
                if !ctx
                    .cache_locations
                    .rows
                    .get(self.row)
                    .is_some_and(|r| r.can_open())
                {
                    return vec![];
                }
                Action::Open(self.row)
            }
            1 => Action::Analyze(Some(self.row)),
            2 => Action::Analyze(None),
            3 => Action::Stop,
            4 => Action::Refresh,
            6 => Action::Copy(self.row),
            7 => {
                return vec![Effect::Help {
                    title: " Cache and temporary directories Help ",
                    text: format!(
                        "{}\n\n{}\n\n{}\n\n{}\n\n{}",
                        ctx.language.text(HELP),
                        ctx.language.text(DEVELOPER_HELP),
                        ctx.language.text(APPLICATION_HELP),
                        ctx.cache_locations.rows.get(self.row).map_or_else(
                            String::new,
                            |r| format!(
                                "{}\n{}",
                                r.path.as_ref().map_or_else(String::new, |p| {
                                    crate::filesystem::display_name(&p.to_string_lossy())
                                }),
                                crate::filesystem::display_name(&r.cause)
                            )
                        ),
                        crate::filesystem::display_name(&ctx.cache_locations.error)
                    ),
                }]
            }
            8 => {
                return vec![
                    Effect::CacheLocations {
                        generation: ctx.cache_locations.generation,
                        action: Action::Stop,
                    },
                    Effect::Close,
                ]
            }
            _ => return vec![],
        };
        vec![Effect::CacheLocations {
            generation: ctx.cache_locations.generation,
            action,
        }]
    }
    fn select(&mut self, row: usize, ctx: &Context) {
        self.row = row.min(ctx.cache_locations.rows.len().saturating_sub(1));
        self.selected = ctx
            .cache_locations
            .rows
            .get(self.row)
            .and_then(|r| r.path.clone());
        if self.row < self.offset {
            self.offset = self.row;
        }
        if self.row >= self.offset + 7 {
            self.offset = self.row - 6;
        }
    }
}
impl ToolPlugin for CacheDirectories {
    fn definition(&self) -> Definition {
        Definition::builtin(
            CACHE_LOCATIONS,
            "Cache and temporary directories",
            Presentation::Document,
        )
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn open(&mut self, _: &Context) {
        *self = Self::default();
    }
    fn close(&mut self) {
        *self = Self::default();
    }
    fn close_action(&self) -> Option<ActionId> {
        Some(ActionId(8))
    }
    fn windows(&self, ctx: &Context) -> Vec<LocalWindow> {
        vec![LocalWindow {
            id: 0,
            parent: None,
            bounds: centered(ctx.area, 76, 23),
            caps: Capabilities::fixed_zoom(76, 23),
        }]
    }
    fn prepare(&mut self, ctx: &Context) {
        if self.observed != ctx.cache_locations.generation {
            self.observed = ctx.cache_locations.generation;
            self.last_click = None;
        }
        let row = self
            .selected
            .as_ref()
            .and_then(|p| {
                ctx.cache_locations
                    .rows
                    .iter()
                    .position(|r| r.path.as_ref() == Some(p))
            })
            .unwrap_or(self.row);
        self.select(row, ctx);
    }
    fn uses_controls(&self, key: KeyEvent) -> bool {
        !(self.focus == 0
            && matches!(
                key.code,
                K::Up | K::Down | K::Home | K::End | K::PageUp | K::PageDown
            ))
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        let view = &ctx.cache_locations;
        let lang = ctx.language;
        let r = centered(ctx.area, 76, 23);
        let mut s = ctx.dialog(
            r,
            self.focus,
            lang.text("Cache and temporary directories"),
            0,
            if view.busy && !view.stopping { 3 } else { 8 },
        );
        let styles = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let mut caption = |text: String, y, height| {
            s.caption(
                Paragraph::new(text)
                    .style(styles(0x70))
                    .wrap(crate::painting::Wrap { trim: false }),
                Rect::new(r.x + 2, r.y + y, r.width.saturating_sub(4), height),
            )
        };
        caption(
            lang.text("Location                         Apparent size / status")
                .into(),
            1,
            1,
        );
        let items = view
            .rows
            .iter()
            .map(|row| {
                let size = row.measurement.as_ref().map_or_else(
                    || lang.text("Unknown").into(),
                    |m| crate::directory_tree::usage::bytes(m.bytes),
                );
                format!(
                    "{:<31} {} · {}{}",
                    crate::formatting::tail(lang.text(row.name), 29),
                    size,
                    lang.text(row.status.label()),
                    if row.stale {
                        lang.text(" · stale")
                    } else {
                        ""
                    }
                )
            })
            .collect();
        let list = Rect::new(r.x + 2, r.y + 2, r.width.saturating_sub(4), 7);
        let mut control = Control::new(
            0,
            list,
            ControlKind::List {
                items,
                cursor: self.row,
                offset: self.offset,
            },
            ctx.target(0),
        );
        control.hits = (self.offset..(self.offset + 7).min(view.rows.len()))
            .map(|i| {
                (
                    Rect::new(list.x, list.y + (i - self.offset) as u16, list.width, 1),
                    ctx.target(100 + i),
                )
            })
            .collect();
        s.control(control);
        let mut caption = |text: String, y, height| {
            s.caption(
                Paragraph::new(text)
                    .style(styles(0x70))
                    .wrap(crate::painting::Wrap { trim: false }),
                Rect::new(r.x + 2, r.y + y, r.width.saturating_sub(4), height),
            )
        };
        if let Some(row) = view.rows.get(self.row) {
            caption(
                row.path.as_ref().map_or_else(
                    || lang.text("Path could not be resolved").into(),
                    |p| crate::filesystem::display_name(&p.to_string_lossy()),
                ),
                10,
                2,
            );
            caption(lang.text(row.purpose).into(), 12, 2);
            let detail = if let Some(m) = &row.measurement {
                format!(
                    "{} {} · {} {} · {} {}",
                    lang.text("Files:"),
                    m.files,
                    lang.text("Skipped:"),
                    m.skipped,
                    lang.text("Excluded mounts:"),
                    m.excluded
                )
            } else {
                String::new()
            };
            caption(detail, 14, 1);
        }
        let cause = if !view.error.is_empty() {
            view.error.as_str()
        } else {
            view.rows.get(self.row).map_or("", |r| r.cause.as_str())
        };
        caption(crate::filesystem::display_name(cause), 15, 1);
        caption(
            if self.clipboard_notice.is_empty() {
                lang.text(&view.notice).into()
            } else {
                self.clipboard_notice.clone()
            },
            16,
            1,
        );
        caption(
            lang.text("Read-only · Apparent bytes are not reclaimable space.")
                .into(),
            17,
            1,
        );
        let selected = view.rows.get(self.row);
        for (line, buttons) in [
            [
                (1, "Analyze"),
                (2, "Analyze all"),
                (3, "Stop"),
                (4, "Refresh"),
            ],
            [(5, "Open"), (6, "Copy path"), (7, "Help"), (8, "Close")],
        ]
        .into_iter()
        .enumerate()
        {
            let labels: Vec<_> = buttons.iter().map(|(_, label)| lang.text(label)).collect();
            let bounds = crate::dialog_theme::button_row(
                Rect::new(
                    r.x + 2,
                    r.y + 18 + line as u16 * 2,
                    r.width.saturating_sub(4),
                    1,
                ),
                &labels,
                14,
                1,
            );
            for ((id, label), bounds) in buttons.into_iter().zip(bounds) {
                let mut c = Control::new(
                    id,
                    bounds,
                    ControlKind::Button(lang.text(label).into()),
                    ctx.target(id),
                );
                c.enabled = match id {
                    1 => !view.busy && selected.is_some_and(|r| r.identity.is_some()),
                    2 => !view.busy && view.rows.iter().any(|r| r.identity.is_some()),
                    3 => view.busy && !view.stopping,
                    4 => !view.busy,
                    5 => selected.is_some_and(|r| r.can_open()),
                    6 => selected.is_some_and(|r| r.path.is_some()),
                    _ => true,
                };
                s.control(c);
            }
        }
        s
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        match event {
            Event::ClipboardCopied { serial, message }
                if serial == ctx.cache_locations.generation =>
            {
                self.clipboard_notice = message
            }
            Event::Focus(id) => {
                self.focus = id;
                self.last_click = None;
            }
            Event::Action(ActionId(id)) => {
                self.last_click = None;
                return self.act(id, ctx);
            }
            Event::Key(key) => {
                self.last_click = None;
                if key.code == K::F(1) {
                    return self.act(7, ctx);
                }
                if self.focus == 0 {
                    let row = match key.code {
                        K::Up => self.row.saturating_sub(1),
                        K::Down => self.row + 1,
                        K::PageUp => self.row.saturating_sub(7),
                        K::PageDown => self.row + 7,
                        K::Home => 0,
                        K::End => usize::MAX,
                        _ => self.row,
                    };
                    self.select(row, ctx);
                }
            }
            Event::Mouse(mouse, Some(ActionId(id)))
                if mouse.kind == MouseEventKind::Down(MouseButton::Left) =>
            {
                if id >= 100 {
                    self.focus = 0;
                    self.select(id - 100, ctx);
                    let path = self.selected.clone();
                    let twice = self.last_click.take().is_some_and(|(p, when)| {
                        p == path
                            && ctx.now.saturating_duration_since(when) <= Duration::from_millis(400)
                    });
                    if twice {
                        return self.act(5, ctx);
                    }
                    self.last_click = Some((path, ctx.now));
                } else {
                    self.last_click = None;
                    self.focus = id;
                    return self.act(id, ctx);
                }
            }
            Event::Mouse(mouse, _)
                if matches!(
                    mouse.kind,
                    MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
                ) =>
            {
                self.last_click = None;
                self.select(
                    if mouse.kind == MouseEventKind::ScrollDown {
                        self.row + 1
                    } else {
                        self.row.saturating_sub(1)
                    },
                    ctx,
                );
            }
            _ => {}
        }
        vec![]
    }
}
pub(crate) const HELP:&str="This read-only tool lists standard cache, temporary and diagnostic-log locations. Opening it checks locations, but never recursively scans them. Analyze measures the selection; Analyze all visits available locations sequentially. Stop or Esc cancels analysis; Esc again closes. Refresh rediscovers locations and marks retained measurements stale. Open or double-click opens the location in the originating pane; Copy path uses the OS/internal clipboard.\nSizes are apparent file lengths, not allocated or reclaimable space. Hidden entries are included, hard links count separately, and links are not followed. Each scan stays on its root filesystem, with limits of 20,000 directory records and 512 levels. Restricted entries, excluded mounts, cancellation and limits produce partial results. Locations can overlap, so there is no grand total. Snapshots can become outdated while applications run. Nothing here means safe to delete: applications can need temporary files, caches can require rebuilding, and logs aid troubleshooting.\nNo deletion, elevated access, cleanup commands or persistent scan cache is provided. Help and original causes remain available without switching focus from other documents. Missing, inaccessible and unresolved locations are distinct. Closing or disabling cancels background work.";

const DEVELOPER_HELP: &str = "On macOS, developer locations include archives, simulator data and installed SDKs/toolchains, not just caches. These can contain valuable work or required tools. No project search or automatic analysis is performed.";

const APPLICATION_HELP: &str = "macOS application locations include installed apps, Application Support, app/group containers and local device backups. These may contain essential software or valuable user data. Backups overlap Application Support; sizes are not safe-to-delete estimates.";
