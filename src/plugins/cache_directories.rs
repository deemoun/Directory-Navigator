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
    reviewing: bool,
    review_focus: usize,
    review_cursor: usize,
    review_seen: Option<u64>,
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
            9 => Action::Clean(self.row),
            10 => Action::ToggleClean(self.review_cursor),
            13 => Action::SelectClean(true),
            14 => Action::SelectClean(false),
            11 => {
                if ctx.cache_locations.cleanup_selection.count == 0 {
                    return vec![];
                }
                self.reviewing = false;
                Action::ConfirmClean
            }
            12 => {
                self.reviewing = false;
                Action::DismissClean
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
                        "{}\n\n{}\n\n{}\n\n{}\n\n{}\n\n{}\n\n{}\n\n{}",
                        ctx.language.text(HELP),
                        ctx.language.text(DEVELOPER_HELP),
                        ctx.language.text(APPLICATION_HELP),
                        ctx.language.text(CLEANUP_HELP),
                        ctx.cache_locations
                            .cleanup_scan
                            .as_ref()
                            .map_or_else(String::new, |stats| stats.details(ctx.language)),
                        ctx.cache_locations.cleanup_report.as_ref().map_or_else(
                            String::new,
                            |report| format!(
                                "{} · {}\n{}\n{}",
                                ctx.language.text(report.outcome.label()),
                                report.summary(ctx.language),
                                crate::filesystem::display_name(&report.root.to_string_lossy()),
                                crate::filesystem::display_name(&report.error)
                            )
                        ),
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
        Some(ActionId(if self.reviewing { 12 } else { 8 }))
    }
    fn windows(&self, ctx: &Context) -> Vec<LocalWindow> {
        let mut windows = vec![LocalWindow {
            id: 0,
            parent: None,
            bounds: centered(ctx.area, 76, 23),
            caps: Capabilities::fixed_zoom(76, 23),
        }];
        if self.reviewing {
            windows.push(LocalWindow {
                id: 1,
                parent: Some(0),
                bounds: centered(ctx.area, 74, 22),
                caps: Capabilities::fixed(74, 22),
            });
        }
        windows
    }
    fn prepare(&mut self, ctx: &Context) {
        if ctx.cache_locations.review.is_none() {
            self.reviewing = false;
        }
        if ctx.active
            && ctx.cache_locations.review.is_some()
            && self.review_seen != Some(ctx.cache_locations.generation)
        {
            self.review_seen = Some(ctx.cache_locations.generation);
            self.reviewing = true;
            self.review_focus = 12;
            self.review_cursor = 0;
        }

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
        if self.reviewing {
            return !(self.review_focus == 10
                && matches!(
                    key.code,
                    K::Up | K::Down | K::Home | K::End | K::PageUp | K::PageDown | K::Char(' ')
                ));
        }
        !(self.focus == 0
            && matches!(
                key.code,
                K::Up | K::Down | K::Home | K::End | K::PageUp | K::PageDown
            ))
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        if self.reviewing {
            return self.review_spec(ctx);
        }
        self.main_spec(ctx)
    }
    fn view(&self, ctx: &Context) -> View<'_> {
        if !self.reviewing {
            return View::Dialog(Box::new(self.main_spec(ctx)));
        }
        let mut layers = Vec::new();
        for (id, spec, interactive) in [
            (0, self.main_spec(ctx), false),
            (1, self.review_spec(ctx), true),
        ] {
            let mut frame = crate::painting::NativeFrame::new(ctx.area, ctx.palette);
            use crate::painting::Frame;
            frame.render_widget(
                crate::turbo::widgets::dialog(if id == 0 {
                    ctx.language.text("Cache and temporary directories")
                } else {
                    ctx.language.text("Review cache cleanup")
                }),
                spec.bounds,
            );
            layers.push(Layer {
                id,
                interactive,
                scene: frame.finish(),
                controls: Some(spec),
            });
        }
        View::Layers(layers)
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        if self.reviewing {
            match event {
                Event::Focus(id) => self.review_focus = id,
                Event::Key(key) if key.code == K::F(1) => return self.act(7, ctx),
                Event::Action(ActionId(id)) => return self.act(id, ctx),
                Event::Key(key) if self.review_focus == 10 => {
                    if key.code == K::Char(' ') {
                        return self.act(10, ctx);
                    }
                    let last = ctx
                        .cache_locations
                        .review
                        .as_ref()
                        .map_or(0, |r| r.paths.len().saturating_sub(1));
                    self.review_cursor = match key.code {
                        K::Up => self.review_cursor.saturating_sub(1),
                        K::Down => (self.review_cursor + 1).min(last),
                        K::PageUp => self.review_cursor.saturating_sub(7),
                        K::PageDown => (self.review_cursor + 7).min(last),
                        K::Home => 0,
                        K::End => last,
                        _ => self.review_cursor,
                    };
                }
                Event::Mouse(mouse, _)
                    if matches!(
                        mouse.kind,
                        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
                    ) =>
                {
                    let last = ctx
                        .cache_locations
                        .review
                        .as_ref()
                        .map_or(0, |r| r.paths.len().saturating_sub(1));
                    self.review_cursor = if mouse.kind == MouseEventKind::ScrollDown {
                        (self.review_cursor + 1).min(last)
                    } else {
                        self.review_cursor.saturating_sub(1)
                    };
                }
                Event::Mouse(mouse, Some(ActionId(id)))
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left) && id >= 1000 =>
                {
                    self.review_focus = 10;
                    self.review_cursor = id - 1000;
                    return self.act(10, ctx);
                }
                _ => {}
            }
            return vec![];
        }
        self.main_event(event, ctx)
    }
}
impl CacheDirectories {
    fn main_spec(&self, ctx: &Context) -> DialogSpec<'_> {
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
            let detail = if view.cleanup_preparing {
                view.cleanup_scan
                    .as_ref()
                    .map_or_else(String::new, |stats| stats.progress(lang))
            } else if let Some(report) = view
                .cleanup_report
                .as_ref()
                .filter(|report| !view.busy || report.running())
            {
                report.summary(lang)
            } else if let Some(stats) = view.cleanup_scan.as_ref().filter(|_| !view.busy) {
                stats.summary(lang)
            } else if let Some(m) = &row.measurement {
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
        caption(crate::filesystem::display_name(lang.text(cause)), 15, 1);
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
            lang.text("Apparent bytes are not reclaimable space.")
                .into(),
            17,
            1,
        );
        let selected = view.rows.get(self.row);
        let mut first = vec![(1, "Analyze"), (2, "Analyze all")];
        if cfg!(target_os = "linux") {
            first.push((9, "Clean…"));
        }
        first.extend([(3, "Stop"), (4, "Refresh")]);
        for (line, buttons) in [
            first,
            vec![(5, "Open"), (6, "Copy path"), (7, "Help"), (8, "Close")],
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
                    9 => !view.busy && selected.is_some_and(|r| r.can_clean()),
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
    fn main_event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
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
pub(crate) const HELP:&str="This tool lists standard cache, temporary and diagnostic-log locations. Opening it checks locations, but never recursively scans them. Analyze measures the selection; Analyze all visits available locations sequentially. Stop or Esc cancels analysis; Esc again closes. Refresh rediscovers locations and marks retained measurements stale. Open or double-click opens the location in the originating pane; Copy path uses the OS/internal clipboard.\nSizes are apparent file lengths, not allocated or reclaimable space. Hidden entries are included, hard links count separately, and links are not followed. Each scan stays on its root filesystem, with limits of 20,000 directory records and 512 levels. Restricted entries, excluded mounts, cancellation and limits produce partial results. Locations can overlap, so there is no grand total. Snapshots can become outdated while applications run. Nothing here means safe to delete: applications can need temporary files, caches can require rebuilding, and logs aid troubleshooting.\nAnalysis never changes files. Cleanup, where offered, always requires review and explicit Trash confirmation. No elevated access, permanent-delete fallback or persistent scan cache is provided. Help and original causes remain available without switching focus from other documents. Missing, inaccessible and unresolved locations are distinct. Closing or disabling cancels background work.";

const DEVELOPER_HELP: &str = "On macOS, developer locations include archives, simulator data and installed SDKs/toolchains, not just caches. These can contain valuable work or required tools. No project search or automatic analysis is performed.";

const APPLICATION_HELP: &str = "macOS application locations include installed apps, Application Support, app/group containers and local device backups. These may contain essential software or valuable user data. Backups overlap Application Support; sizes are not safe-to-delete estimates.";

impl CacheDirectories {
    fn review_spec(&self, ctx: &Context) -> DialogSpec<'_> {
        let lang = ctx.language;
        let r = centered(ctx.area, 74, 22);
        let mut s = ctx.dialog(
            r,
            self.review_focus,
            lang.text("Review cache cleanup"),
            11,
            12,
        );
        let Some(review) = ctx.cache_locations.review.as_ref() else {
            return s;
        };
        let styles = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        s.caption(Paragraph::new(lang.text("Only reviewed regular cache files at least 7 days old. Directories and recent files are kept. Cache files may still be in use.")).style(styles(0x70)).wrap(crate::painting::Wrap { trim: false }), Rect::new(r.x+2, r.y+1, r.width-4, 3));
        s.caption(
            Paragraph::new(crate::filesystem::display_name(
                &review.root.to_string_lossy(),
            ))
            .style(styles(0x70)),
            Rect::new(r.x + 2, r.y + 4, r.width - 4, 1),
        );
        let cursor = self.review_cursor.min(review.paths.len().saturating_sub(1));
        let offset = cursor.saturating_sub(6);
        // Prepare only the visible slice; large reviews never format every path per frame.
        let items: Vec<String> = review
            .paths
            .iter()
            .skip(offset)
            .take(7)
            .enumerate()
            .map(|(visible, p)| {
                let checked = ctx
                    .cache_locations
                    .cleanup_selection
                    .mask
                    .get(offset + visible)
                    .copied()
                    .unwrap_or(false);
                format!(
                    "[{}] {}",
                    if checked { 'x' } else { ' ' },
                    crate::filesystem::display_name(
                        &p.strip_prefix(&review.root).unwrap_or(p).to_string_lossy()
                    )
                )
            })
            .collect();
        let bounds = Rect::new(r.x + 2, r.y + 6, r.width - 4, 7);
        let mut list = Control::new(
            10,
            bounds,
            ControlKind::List {
                items,
                cursor: cursor - offset,
                offset: 0,
            },
            ctx.target(10),
        );
        list.hits = (offset..(offset + 7).min(review.paths.len()))
            .map(|i| {
                (
                    Rect::new(bounds.x, bounds.y + (i - offset) as u16, bounds.width, 1),
                    ctx.target(1000 + i),
                )
            })
            .collect();
        s.control(list);
        let selection = &ctx.cache_locations.cleanup_selection;
        let summary = format!(
            "{} {}/{} · {}{}",
            lang.text("Selected:"),
            selection.count,
            review.paths.len(),
            crate::directory_tree::usage::bytes(selection.bytes),
            if review.partial() {
                lang.text(" · partial inspection")
            } else {
                ""
            }
        );
        s.caption(
            Paragraph::new(summary).style(styles(0x70)),
            Rect::new(r.x + 2, r.y + 13, r.width - 4, 1),
        );
        s.caption(
            Paragraph::new(review.stats.summary(lang))
                .style(styles(0x70))
                .wrap(crate::painting::Wrap { trim: false }),
            Rect::new(r.x + 2, r.y + 14, r.width - 4, 2),
        );
        s.caption(Paragraph::new(lang.text("Moves to Trash only. Restore with your desktop file manager. Trash may keep the same disk space occupied.")).style(styles(0x70)).wrap(crate::painting::Wrap { trim: false }), Rect::new(r.x+2, r.y+16, r.width-4, 2));
        let buttons = [
            (13, "Select all"),
            (14, "Select none"),
            (11, "Move to Trash"),
            (12, "Cancel"),
        ];
        let labels: Vec<_> = buttons.iter().map(|(_, label)| lang.text(label)).collect();
        let bounds = crate::dialog_theme::button_row(
            Rect::new(r.x + 2, r.y + 19, r.width - 4, 1),
            &labels,
            14,
            1,
        );
        for (((id, _), label), bounds) in buttons.into_iter().zip(labels).zip(bounds) {
            let mut control = Control::new(
                id,
                bounds,
                ControlKind::Button(label.into()),
                ctx.target(id),
            );
            control.enabled = id != 11 || selection.count > 0;
            s.control(control);
        }
        s
    }
}

const CLEANUP_HELP: &str = "Where Clean… is offered, select the user-cache row to review regular files unchanged for at least 7 days. Only privately controlled cache data beneath your home folder is eligible. Recent files, directories, links, hard links, mounted paths, system locations, applications and backups are kept. Review defaults to Cancel. Move to Trash rechecks identities and changes; it never permanently deletes files. Stop or closing cancels remaining work; completed moves remain in Trash. Trash can retain the same disk space until emptied separately. Reviews are bounded to 10,000 files, 100,000 entries, 20,000 directories, 512 levels and 4 MiB of paths; partial reviews are labelled. Files start selected; Space or a click toggles a file, Select all/none updates the draft, and Move to Trash requires a selection. Preparation shows scanned/eligible counts. Recent, excluded and unreadable entries have separate counters. Partial inspections and failures are distinct from an empty complete inspection. Help retains the last cleanup outcome, selected/confirmed/unselected counts, native root and original errors through Analyze and Refresh.";
