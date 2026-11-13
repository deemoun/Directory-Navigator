//! Session music player, adapted from DN 1.51 CDPLAYER.PAS's counter and layout.
//! Copyright (C) 1991-99 RIT Research Labs.
//! Based on Dos Navigator by RIT Research Labs; see THIRD_PARTY_NOTICES.md.
use super::*;
use crate::{
    painting::{Frame, NativeFrame, Paragraph},
    presentation::text::{Line, Span},
    turbo::dialog_spec::{Control, ControlKind},
    windowing::geometry::Capabilities,
};
use crossterm::event::{KeyCode as K, MouseButton, MouseEventKind};

mod audio;
mod formats;
#[cfg(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
mod metadata;
pub(super) fn supports_path(path: &std::path::Path) -> bool {
    formats::Format::from_path(path).is_some()
}
#[cfg(any(
    all(
        feature = "audio",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ),
    test
))]
mod duration;
mod import;
mod modes;
mod picker;
mod playlist;
mod setup;
mod tags;
const PLAY: usize = 2;
const ZOOM: usize = 100;
const CLOSE: usize = 101;
const HELP: usize = 102;
const HELP_TEXT: &str = "MP3, FLAC, WAV and Ogg Vorbis playback is available on macOS, Linux and Windows in builds with audio support. Enable this plugin in Options > Configuration > Plugins.\nOpen… uses DN's file picker to add and play one music file, keeping the queue and reusing an existing entry. Playlist… opens the live queue: Enter or Play starts the highlighted track; Add… chooses a file without interrupting playback. Remove applies immediately and stops playback if the current track is removed. Close/Esc keeps queue changes. Playlists are session-only; there is no playlist-file saving.\nUtilities > Add music to player imports marked files or folders from the active panel; without marks it uses the cursor entry. Folders are scanned recursively in name order, skipping links. The queue holds 64 entries. Cancel discards the whole import; closing the player cancels it too.\nPlay/Pause toggles playback; Stop resets the position. Outer arrows select previous/next tracks; −5s/+5s use the saved seek step. Volume −/+ adjusts by 10 percentage points immediately for the session only. Settings… edits the saved seek step and default volume with OK/Cancel. The elapsed/remaining control switches the single time display; remaining time has a minus sign and unknown time shows --:--. The progress bar always shows elapsed position and is display-only. Artist/title and a volume meter appear in the expanded view; the compact counter stays unchanged.\nShuffle applies immediately. Repeat opens an owned Off/One/All selector: Enter or click applies, Esc cancels. Repeat One affects automatic endings, not manual Previous/Next. Shuffle visits each track once; Previous retraces the order. Queue changes rebuild the shuffled cycle.\nInfo… shows title, artist, album and path. Track lengths and supported tags are read in the background without an audio device. Errors remain visible. Music continues while working in other windows. Closing the player, disabling the plugin or quitting DN stops audio.\nTab/Shift+Tab select controls; Enter/Space activates them. F1 opens Help. Alt+Z switches compact/expanded views; Ctrl+F5 moves the player. Open and Add file pickers can be cancelled without changing the queue or playback.";
const PICKER_PAGE: u64 = 7;
const PICKER_INPUT: usize = 700;
const PICKER_CANCEL: usize = 701;
const IMPORT_PAGE: u64 = 5;
const IMPORT_CANCEL: usize = 500;
const REPEAT_PAGE: u64 = 8;
const REPEAT_APPLY: usize = 800;
const REPEAT_CANCEL: usize = 801;
const REPEAT_SET: usize = 810;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Transport {
    #[default]
    Stopped,
    Playing,
    Paused,
}

pub(crate) struct Mp3Player {
    playlist: playlist::Playlist,
    expanded: bool,
    preferences: crate::config::Mp3Preferences,
    remaining: bool,
    setup_draft: Option<crate::config::Mp3Preferences>,
    setup_focus: usize,
    focus: usize,
    current: Option<u64>,
    next_track: u64,
    transport: Transport,
    request: audio::Request,
    snapshot: audio::Snapshot,
    engine: Box<dyn audio::Engine>,
    wake: Option<Instant>,
    notice: String,
    playback: modes::Order,
    import: Option<import::Pending>,
    requests: crate::requests::RequestScope,
    picker: Option<crate::turbo::file_dialog::Picker>,
    picker_play: bool,
    picker_directory: Option<PathBuf>,
    repeat_cursor: Option<usize>,
    repeat_focus: usize,
}
impl Default for Mp3Player {
    fn default() -> Self {
        Self {
            playlist: playlist::Playlist::default(),
            expanded: false,
            preferences: Default::default(),
            remaining: false,
            setup_draft: None,
            setup_focus: 0,
            focus: PLAY,
            current: None,
            next_track: 0,
            transport: Transport::Stopped,
            request: audio::Request {
                volume: 50,
                ..Default::default()
            },
            snapshot: Default::default(),
            engine: audio::engine(),
            wake: None,
            notice: String::new(),
            playback: modes::Order::default(),
            import: None,
            requests: crate::requests::RequestScope::new(crate::requests::Owner::Workspace),
            picker: None,
            picker_play: true,
            picker_directory: None,
            repeat_cursor: None,
            repeat_focus: 0,
        }
    }
}
impl Mp3Player {
    fn bounds(&self, ctx: &Context) -> Rect {
        crate::turbo::dialog_spec::centered(
            ctx.area,
            if self.expanded { 66 } else { 36 },
            if self.expanded { 18 } else { 9 },
        )
    }
    fn count(&self) -> usize {
        if self.expanded {
            15
        } else {
            5
        }
    }
    fn track_index(&self) -> Option<usize> {
        self.playlist
            .order
            .iter()
            .position(|t| Some(t.id) == self.current)
    }
    fn track_duration(&self) -> Option<Duration> {
        self.snapshot.duration.or_else(|| {
            self.track_index()
                .and_then(|i| self.playlist.order[i].duration)
        })
    }
    fn enabled(&self, id: usize) -> bool {
        if !crate::plugins::music_playback_available() {
            return id == playlist::LIST;
        }
        match id {
            0 | 4 => self
                .playback
                .neighbor(self.current, id == 4, false)
                .is_some(),
            1 | 3 => self.transport != Transport::Stopped && self.snapshot.ready,
            PLAY => !self.playlist.order.is_empty(),
            5 => self.request.volume > 0,
            6 => self.transport != Transport::Stopped,
            7 => self.import.is_none(),
            8..=10 | 12 | 14 => true,
            11 => self.request.volume < 100,
            13 => self.track_index().is_some(),
            _ => false,
        }
    }
    fn submit(&mut self) {
        self.engine.submit(self.request.clone());
    }
    fn stop(&mut self) {
        self.request.generation = self.request.generation.wrapping_add(1);
        self.request.path = None;
        self.request.seek = None;
        self.transport = Transport::Stopped;
        if self.focus < self.count() && !self.enabled(self.focus) && self.enabled(PLAY) {
            self.focus = PLAY;
        }
        self.snapshot = audio::Snapshot::default();
        self.wake = None;
        self.submit();
    }
    fn sync_order(&mut self) {
        self.playback.sync(
            self.playlist.order.iter().map(|t| t.id).collect(),
            self.current,
        );
    }
    fn start(&mut self, now: Instant) {
        self.sync_order();
        let Some(index) = self.track_index() else {
            return;
        };
        self.request.generation = self.request.generation.wrapping_add(1);
        self.request.path = Some(self.playlist.order[index].path.clone());
        self.request.paused = false;
        self.request.seek = None;
        self.snapshot = audio::Snapshot::default();
        self.transport = Transport::Playing;
        self.notice.clear();
        self.submit();
        self.wake = Some(now);
    }
    fn poll_audio(&mut self, now: Instant) {
        let snapshot = self.engine.snapshot();
        if snapshot.generation == self.request.generation {
            if let Some(error) = &snapshot.error {
                self.notice = error.clone();
                self.expanded = true;
                self.stop();
                return;
            }
            let finished = snapshot.finished;
            if let (Some(id), Some(duration)) = (self.current, snapshot.duration) {
                self.playlist.update_duration(id, duration);
            }
            self.snapshot = snapshot;
            if finished && self.transport == Transport::Playing {
                if let Some(next) = self.playback.neighbor(self.current, true, true) {
                    self.current = Some(next);
                    self.start(now);
                } else {
                    self.stop();
                }
            }
        }
        if self.transport != Transport::Stopped {
            self.wake = Some(now + Duration::from_millis(200));
        }
    }
    fn add_files(&mut self, paths: Vec<PathBuf>, ctx: &Context) {
        #[cfg(not(all(
            feature = "audio",
            any(target_os = "macos", target_os = "linux", target_os = "windows")
        )))]
        let _ = (paths, ctx);
        #[cfg(all(
            feature = "audio",
            any(target_os = "macos", target_os = "linux", target_os = "windows")
        ))]
        {
            if self.import.is_some() {
                return;
            }
            if paths.is_empty() || !self.playlist.can_add() {
                self.notice = ctx
                    .language
                    .text(if self.playlist.can_add() {
                        "Select music files or folders in the active panel"
                    } else {
                        "Queue is full (64 tracks)"
                    })
                    .into();
                return;
            }
            let capacity = self.playlist.available();
            let progress = std::sync::Arc::new(import::Progress::default());
            let worker = progress.clone();
            match crate::task::Task::spawn_owned(&self.requests, "mp3-import", move |cancel| {
                import::scan(paths, capacity, &cancel, &worker)
            }) {
                Ok(task) => {
                    self.notice.clear();
                    self.import = Some(import::Pending {
                        task: Some(task),
                        progress,
                        wake: ctx.now,
                        play: None,
                        visited: 0,
                        found: 0,
                    })
                }
                Err(error) => self.notice = error.to_string(),
            }
        }
    }
    fn play_file(&mut self, path: PathBuf, ctx: &Context) {
        if !crate::plugins::music_playback_available() {
            return;
        }
        self.expanded = true;
        if self.import.is_some()
            || self.setup_draft.is_some()
            || self.picker.is_some()
            || self.repeat_cursor.is_some()
        {
            self.notice = ctx
                .language
                .text("Finish the current player operation first")
                .into();
            return;
        }
        if let Some(track) = self.playlist.order.iter().find(|t| t.path == path) {
            self.current = Some(track.id);
            self.start(ctx.now);
            return;
        }
        self.add_files(vec![path.clone()], ctx);
        if let Some(pending) = &mut self.import {
            pending.play = Some(path);
        }
    }
    fn poll_import(&mut self, ctx: &Context) {
        let reply = self
            .import
            .as_mut()
            .and_then(|pending| crate::task::take_reply(&mut pending.task));
        if let Some(reply) = reply {
            let pending = self.import.take().unwrap();
            let owner = match ctx.binding.owner {
                Owner::Document(id) => crate::requests::Owner::Document(id),
                Owner::Modal(id) => crate::requests::Owner::Modal(id),
            };
            if self.requests.owner() != owner {
                return;
            }
            match reply {
                Ok(result) => {
                    let errors = result.errors;
                    let limited = result.limited;
                    let count = self
                        .playlist
                        .append(import::Pending::tracks(result, &mut self.next_track));
                    if self.current.is_none() {
                        self.current = self.playlist.order.first().map(|t| t.id);
                    }
                    self.sync_order();
                    if let Some(path) = pending.play {
                        if let Some(track) = self.playlist.order.iter().find(|t| t.path == path) {
                            self.current = Some(track.id);
                            self.start(ctx.now);
                        }
                    }
                    self.notice = if errors > 0 {
                        format!(
                            "{}: {errors}",
                            ctx.language
                                .text("Import skipped unreadable or unsupported entries")
                        )
                    } else if limited {
                        ctx.language.text("Import limit reached").into()
                    } else if count == 0 {
                        ctx.language.text("No supported music files found").into()
                    } else {
                        String::new()
                    };
                }
                Err(_) => self.notice = ctx.language.text("Music import stopped").into(),
            }
        }
        if let Some(pending) = &mut self.import {
            use std::sync::atomic::Ordering;
            pending.visited = pending.progress.visited.load(Ordering::Relaxed);
            pending.found = pending.progress.found.load(Ordering::Relaxed);
            pending.wake = ctx.now + Duration::from_millis(50);
        }
    }
    fn import_controls(&self, ctx: &Context) -> DialogSpec<'static> {
        let r = crate::turbo::dialog_spec::centered(ctx.area, 52, 10);
        let mut spec = ctx.dialog(
            r,
            0,
            ctx.language.text("Adding music files"),
            IMPORT_CANCEL,
            IMPORT_CANCEL,
        );
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let (visited, found) = self
            .import
            .as_ref()
            .map_or((0, 0), |p| (p.visited, p.found));
        for (y, text) in [
            (
                2,
                format!("{}: {visited}", ctx.language.text("Entries checked")),
            ),
            (
                3,
                format!("{}: {found}", ctx.language.text("Music files found")),
            ),
            (5, ctx.language.text("Cancel discards this import").into()),
        ] {
            spec.caption(
                Paragraph::new(text).style(style(0x70)),
                Rect::new(r.x + 3, r.y + y, 46, 1),
            );
        }
        if !self.notice.is_empty() {
            spec.caption(
                Paragraph::new(self.notice.clone()).style(style(0x70)),
                Rect::new(r.x + 3, r.y + 4, 46, 1),
            );
        }
        spec.control(Control::new(
            0,
            Rect::new(r.x + 20, r.y + 7, 12, 1),
            ControlKind::Button(ctx.language.text("Cancel").into()),
            ctx.target(IMPORT_CANCEL),
        ));
        spec
    }
    fn info(&self, ctx: &Context) -> Vec<Effect> {
        let Some(index) = self.track_index() else {
            return Vec::new();
        };
        let track = &self.playlist.order[index];
        let value = |text: &str| {
            if text.is_empty() {
                ctx.language.text("Unknown").to_string()
            } else {
                text.to_string()
            }
        };
        let text = format!(
            "{}: {}\n{}: {}\n{}: {}\n\n{}: {}",
            ctx.language.text("Title"),
            value(&track.tags.title),
            ctx.language.text("Artist"),
            value(&track.tags.artist),
            ctx.language.text("Album"),
            value(&track.tags.album),
            ctx.language.text("File"),
            crate::filesystem::display_name(&track.path.to_string_lossy())
        );
        vec![Effect::Help {
            title: " Music Track Info ",
            text,
        }]
    }
    fn action(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        if id == HELP {
            return vec![Effect::Help {
                title: " Music Player Help ",
                text: ctx.language.text(HELP_TEXT).into(),
            }];
        }
        if self.import.is_some() {
            if id == IMPORT_CANCEL {
                self.import = None;
                self.notice = ctx.language.text("Music import cancelled").into();
            }
            return Vec::new();
        }
        if self.setup_draft.is_some() {
            return self.setup_action(id);
        }
        if self.picker.is_some() {
            if id == PICKER_CANCEL || id == CLOSE {
                self.picker = None;
                self.repeat_cursor = None;
            }
            return Vec::new();
        }
        if self.repeat_cursor.is_some() {
            self.repeat_action(id);
            return Vec::new();
        }
        if self.playlist.open {
            if id == playlist::ADD
                && self.playlist.can_add()
                && crate::plugins::music_playback_available()
            {
                return vec![Effect::RequestMusicFile { play: false }];
            }
            if id == playlist::PLAY_TRACK && crate::plugins::music_playback_available() {
                if let Some(id) = self.playlist.selected() {
                    self.current = Some(id);
                    self.start(ctx.now);
                }
                return Vec::new();
            }
            self.playlist.action(id);
            if self.track_index().is_none() {
                self.stop();
                self.current = self.playlist.order.first().map(|t| t.id);
            }
            self.sync_order();
            if !self.playlist.open {
                self.focus = playlist::LIST;
            }
            return Vec::new();
        }
        match id {
            playlist::LIST if self.expanded => {
                self.focus = playlist::LIST;
                self.playlist.show();
            }
            CLOSE => return vec![Effect::Close],
            ZOOM => {
                self.expanded = !self.expanded;
                if self.focus >= self.count() {
                    self.focus = PLAY;
                }
            }
            id if id < self.count() && self.enabled(id) => {
                self.focus = id;
                match id {
                    0 | 4 => {
                        let playing = self.transport == Transport::Playing;
                        self.current = self.playback.neighbor(self.current, id == 4, false);
                        self.stop();
                        if playing {
                            self.start(ctx.now);
                        }
                    }
                    1 | 3 => {
                        let base = self
                            .request
                            .seek
                            .filter(|(serial, _)| self.snapshot.seek_serial != Some(*serial))
                            .map_or(self.snapshot.position, |(_, position)| position);
                        let position = if id == 1 {
                            base.saturating_sub(Duration::from_secs(u64::from(
                                self.preferences.seek_seconds,
                            )))
                        } else {
                            base.saturating_add(Duration::from_secs(u64::from(
                                self.preferences.seek_seconds,
                            )))
                        };
                        let position = self.track_duration().map_or(position, |d| position.min(d));
                        let serial = self.request.seek.map_or(0, |(n, _)| n + 1);
                        self.request.seek = Some((serial, position));
                        self.submit();
                    }
                    PLAY => {
                        if self.transport == Transport::Playing {
                            self.request.paused = true;
                            self.transport = Transport::Paused;
                            self.submit();
                        } else if self.transport == Transport::Paused {
                            self.request.paused = false;
                            self.transport = Transport::Playing;
                            self.submit();
                        } else if self.transport == Transport::Stopped {
                            self.start(ctx.now);
                        }
                    }
                    5 | 11 => {
                        self.request.volume = if id == 5 {
                            self.request.volume.saturating_sub(10)
                        } else {
                            self.request.volume.saturating_add(10).min(100)
                        };
                        self.submit();
                    }
                    6 => self.stop(),
                    7 => return vec![Effect::RequestMusicFile { play: true }],
                    8 => {
                        self.setup_draft = Some(self.preferences);
                        self.setup_focus = 0;
                    }
                    9 => self.remaining = !self.remaining,
                    10 => {
                        let mut mode = self.playback.mode;
                        mode.shuffle = !mode.shuffle;
                        self.playback.apply(
                            mode,
                            self.playlist.order.iter().map(|t| t.id).collect(),
                            self.current,
                        );
                    }
                    14 => {
                        self.repeat_cursor = Some(match self.playback.mode.repeat {
                            modes::Repeat::Off => 0,
                            modes::Repeat::One => 1,
                            modes::Repeat::All => 2,
                        });
                        self.repeat_focus = 0;
                    }
                    13 => return self.info(ctx),
                    _ => {}
                }
            }
            _ => {}
        }
        Vec::new()
    }
}
impl ToolPlugin for Mp3Player {
    fn definition(&self) -> Definition {
        Definition::builtin(MP3_PLAYER, "Music Player", Presentation::Document)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn open(&mut self, ctx: &Context) {
        self.import = None;
        self.picker = None;
        self.repeat_cursor = None;
        self.picker_directory = None;
        self.requests = crate::requests::RequestScope::new(match ctx.binding.owner {
            Owner::Document(id) => crate::requests::Owner::Document(id),
            Owner::Modal(id) => crate::requests::Owner::Modal(id),
        });
        self.playback = modes::Order::default();
        self.setup_draft = None;
        self.stop();
        self.playlist = playlist::Playlist::default();
        self.current = None;
        self.expanded = false;
        self.focus = PLAY;
        self.notice.clear();
        self.preferences = ctx.mp3;
        self.request.volume = ctx.mp3.default_volume;
        self.remaining = false;
        self.setup_draft = None;
    }
    fn close(&mut self) {
        self.import = None;
        self.picker = None;
        self.repeat_cursor = None;
        self.setup_draft = None;
        self.stop();
        self.playlist = playlist::Playlist::default();
        self.current = None;
    }
    fn next_wake(&self, now: Instant) -> Option<Duration> {
        self.wake
            .into_iter()
            .chain(self.import.as_ref().map(|p| p.wake))
            .min()
            .map(|w| w.saturating_duration_since(now))
    }
    fn zoom_action(&self) -> Option<ActionId> {
        Some(ActionId(ZOOM))
    }
    fn uses_controls(&self, _: crossterm::event::KeyEvent) -> bool {
        self.picker.is_none()
    }
    fn native_input_page(&self) -> Option<u64> {
        self.picker.as_ref().map(|_| PICKER_PAGE)
    }
    fn prepare(&mut self, ctx: &Context) {
        if let Some(picker) = &mut self.picker {
            picker.prepare(
                crate::presentation::layout::Size::new(ctx.area.right(), ctx.area.bottom()),
                ctx.palette,
            );
        }
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        if self.picker.is_some()
            && matches!(
                event,
                Event::Key(_) | Event::Mouse(_, _) | Event::Paste(_) | Event::Action(_)
            )
        {
            return self.picker_event(event, ctx);
        }
        match event {
            Event::MusicPicker { base, hidden, play } => {
                self.open_picker(base, hidden, play, ctx);
                Vec::new()
            }
            Event::PlayMp3(path) => {
                self.play_file(path, ctx);
                Vec::new()
            }
            Event::PanelPaths(paths) => {
                self.add_files(paths, ctx);
                Vec::new()
            }
            Event::Tick => {
                if self.transport != Transport::Stopped {
                    self.poll_audio(ctx.now);
                }
                self.poll_import(ctx);
                Vec::new()
            }
            Event::Action(ActionId(id)) => self.action(id, ctx),
            Event::Focus(_) if self.import.is_some() => Vec::new(),
            Event::Focus(id) if self.repeat_cursor.is_some() => {
                self.repeat_focus = id;
                Vec::new()
            }
            Event::Key(k) if self.repeat_cursor.is_some() && k.code != K::F(1) => {
                if self.repeat_focus == 0 {
                    if k.code == K::Char(' ') {
                        self.repeat_action(REPEAT_APPLY);
                    } else if let Some(row) =
                        crate::turbo::widgets::list_key(self.repeat_cursor.unwrap(), 3, 3, k.code)
                    {
                        self.repeat_cursor = Some(row);
                    }
                }
                Vec::new()
            }
            Event::Focus(id) if self.setup_draft.is_some() => {
                self.setup_focus = id;
                Vec::new()
            }
            Event::Focus(id) if self.playlist.open => {
                self.playlist.focus(id);
                Vec::new()
            }
            Event::Key(k) if self.playlist.open && self.import.is_none() && k.code != K::F(1) => {
                self.playlist.key(k);
                Vec::new()
            }
            Event::Mouse(e, _)
                if self.playlist.open
                    && self.import.is_none()
                    && matches!(
                        e.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    ) =>
            {
                self.playlist.wheel(e.kind == MouseEventKind::ScrollDown);
                Vec::new()
            }
            Event::Focus(id) if id < self.count() => {
                self.focus = id;
                Vec::new()
            }
            Event::Key(k) if k.code == K::Esc => self.action(CLOSE, ctx),
            Event::Key(k) if k.code == K::F(1) => self.action(HELP, ctx),
            Event::Mouse(e, Some(ActionId(id)))
                if e.kind == MouseEventKind::Down(MouseButton::Left) =>
            {
                self.action(id, ctx)
            }
            _ => Vec::new(),
        }
    }
    fn windows(&self, ctx: &Context) -> Vec<LocalWindow> {
        let bounds = self.bounds(ctx);
        let mut result = vec![LocalWindow {
            id: 0,
            parent: None,
            bounds,
            caps: Capabilities::fixed_zoom(bounds.width, bounds.height),
        }];
        if self.playlist.open {
            result.extend(self.playlist.windows(ctx));
        }
        for (id, spec) in [
            self.repeat_cursor
                .map(|_| (REPEAT_PAGE, self.repeat_controls(ctx))),
            self.setup_draft
                .map(|_| (setup::PAGE, self.setup_controls(ctx))),
            self.import
                .as_ref()
                .map(|_| (IMPORT_PAGE, self.import_controls(ctx))),
        ]
        .into_iter()
        .flatten()
        {
            let bounds = spec.bounds;
            result.push(LocalWindow {
                id,
                parent: Some(if self.playlist.open {
                    playlist::ORDER_PAGE
                } else {
                    0
                }),
                bounds,
                caps: Capabilities::fixed(bounds.width, bounds.height),
            });
        }
        if let Some(picker) = &self.picker {
            let bounds = picker.bounds();
            result.push(LocalWindow {
                id: PICKER_PAGE,
                parent: Some(if self.playlist.open {
                    playlist::ORDER_PAGE
                } else {
                    0
                }),
                bounds,
                caps: Capabilities::fixed(bounds.width, bounds.height),
            });
        }
        result
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        if let Some(picker) = &self.picker {
            ctx.dialog(
                picker.bounds(),
                0,
                ctx.language.text("Open file"),
                PICKER_INPUT,
                PICKER_CANCEL,
            )
        } else if self.repeat_cursor.is_some() {
            self.repeat_controls(ctx)
        } else if self.import.is_some() {
            self.import_controls(ctx)
        } else if self.setup_draft.is_some() {
            self.setup_controls(ctx)
        } else if self.playlist.open {
            self.queue_controls(ctx)
        } else {
            self.main_controls(ctx)
        }
    }
    fn view(&self, ctx: &Context) -> View<'_> {
        let mut frame = NativeFrame::new(ctx.area, ctx.palette);
        frame.render_widget(
            crate::turbo::widgets::dialog(ctx.language.text("Music Player")),
            self.bounds(ctx),
        );
        let mut layers = vec![Layer {
            id: 0,
            interactive: self.repeat_cursor.is_none()
                && self.picker.is_none()
                && !self.playlist.open
                && self.setup_draft.is_none()
                && self.import.is_none(),
            scene: frame.finish(),
            controls: Some(self.main_controls(ctx)),
        }];
        if self.playlist.open {
            let mut layer = self.playlist.layer(ctx);
            layer.controls = Some(self.queue_controls(ctx));
            layer.interactive = self.import.is_none() && self.picker.is_none();
            layers.push(layer);
        }
        for (id, spec) in [
            self.repeat_cursor
                .map(|_| (REPEAT_PAGE, self.repeat_controls(ctx))),
            self.setup_draft
                .map(|_| (setup::PAGE, self.setup_controls(ctx))),
            self.import
                .as_ref()
                .map(|_| (IMPORT_PAGE, self.import_controls(ctx))),
        ]
        .into_iter()
        .flatten()
        {
            let mut frame = NativeFrame::new(ctx.area, ctx.palette);
            frame.render_widget(
                crate::turbo::widgets::dialog(ctx.language.text(if id == REPEAT_PAGE {
                    "Repeat"
                } else if id == setup::PAGE {
                    "Music player setup"
                } else {
                    "Adding music files"
                })),
                spec.bounds,
            );
            layers.push(Layer {
                id,
                interactive: true,
                scene: frame.finish(),
                controls: Some(spec),
            });
        }
        if let Some((picker, mut scene)) = self
            .picker
            .as_ref()
            .and_then(|p| p.scene().cloned().map(|scene| (p, scene)))
        {
            // Blocking pages retain only hits anchored inside their own bounds.
            scene.regions = vec![(picker.bounds(), ctx.target(PICKER_INPUT))];
            layers.push(Layer {
                id: PICKER_PAGE,
                interactive: true,
                scene,
                controls: None,
            });
        }
        View::Layers(layers)
    }
}
impl Mp3Player {
    fn queue_controls(&self, ctx: &Context) -> DialogSpec<'static> {
        let mut spec = self.playlist.controls(ctx);
        if !self.notice.is_empty() {
            let r = spec.bounds;
            spec.caption(
                Paragraph::new(self.notice.clone()).style(ctx
                    .palette
                    .styles_for(crate::palette::theme::Group::Dialogs)(
                    0x70
                )),
                Rect::new(r.x + 3, r.y + 14, 62, 1),
            );
        }
        spec
    }
    fn repeat_action(&mut self, id: usize) {
        let Some(cursor) = self.repeat_cursor else {
            return;
        };
        let selected = if id == REPEAT_APPLY {
            Some(cursor)
        } else if (REPEAT_SET..REPEAT_SET + 3).contains(&id) {
            Some(id - REPEAT_SET)
        } else {
            None
        };
        if let Some(selected) = selected {
            let mut mode = self.playback.mode;
            mode.repeat = [modes::Repeat::Off, modes::Repeat::One, modes::Repeat::All][selected];
            self.playback.apply(
                mode,
                self.playlist.order.iter().map(|t| t.id).collect(),
                self.current,
            );
        }
        if selected.is_some() || id == REPEAT_CANCEL {
            self.repeat_cursor = None;
            self.focus = 14;
        }
    }
    fn repeat_controls(&self, ctx: &Context) -> DialogSpec<'static> {
        let parent = self.bounds(ctx);
        let r = crate::windowing::geometry::clamp(
            Rect::new(parent.x + 40, parent.y + 12, 24, 9),
            crate::windowing::geometry::workspace((ctx.area.width, ctx.area.height)),
        );
        let mut spec = ctx.dialog(
            r,
            self.repeat_focus,
            ctx.language.text("Repeat"),
            REPEAT_APPLY,
            REPEAT_CANCEL,
        );
        let list = Rect::new(r.x + 2, r.y + 2, 20, 3);
        let mut control = Control::new(
            0,
            list,
            ControlKind::List {
                items: ["Off", "One", "All"]
                    .into_iter()
                    .map(|s| ctx.language.text(s).into())
                    .collect(),
                cursor: self.repeat_cursor.unwrap_or(0),
                offset: 0,
            },
            ctx.target(REPEAT_APPLY),
        );
        control.dn_list = true;
        control.enter_action = true;
        control.hits = (0..3)
            .map(|i| {
                (
                    Rect::new(list.x, list.y + i, 20, 1),
                    ctx.target(REPEAT_SET + i as usize),
                )
            })
            .collect();
        spec.control(control);
        spec.control(Control::new(
            1,
            Rect::new(r.x + 4, r.y + 6, 15, 1),
            ControlKind::Button(ctx.language.text("Cancel").into()),
            ctx.target(REPEAT_CANCEL),
        ));
        spec
    }
    fn expanded_controls(&self, ctx: &Context) -> DialogSpec<'static> {
        let r = self.bounds(ctx);
        let language = ctx.language;
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let mut spec = ctx.dialog(r, self.focus, language.text("Music Player"), PLAY, CLOSE);
        let track = self.track_index().map(|i| &self.playlist.order[i]);
        let artist = track.map_or("", |t| t.tags.artist.as_str());
        let title = track.map_or_else(
            || language.text("Empty playlist").into(),
            |t| {
                if t.tags.title.is_empty() {
                    t.name()
                } else {
                    t.tags.title.clone()
                }
            },
        );
        for (y, text) in [(2, artist.to_string()), (3, title)] {
            spec.caption(
                Paragraph::new(fit_music_text(&text, 62)).style(style(0x70)),
                Rect::new(r.x + 2, r.y + y, 62, 1),
            );
        }
        let duration = self.track_duration();
        let position = if self.remaining {
            duration.map(|d| d.saturating_sub(self.snapshot.position))
        } else {
            Some(self.snapshot.position)
        };
        let clock = format!(
            "{}{} / {}",
            if self.remaining && position.is_some() {
                "−"
            } else {
                ""
            },
            position.map_or_else(|| "--:--".into(), format_time),
            duration.map_or_else(|| "--:--".into(), format_time)
        );
        let clock_width = crate::presentation::text::Span::raw(&clock).width();
        // Very long minute counts use the otherwise empty row above; no digits
        // are lost and there is still exactly one readout.
        let (clock_y, clock_x, clock_w) = if clock_width > 20 {
            (4, 2, 47)
        } else {
            (5, 29, 20)
        };
        spec.caption(
            Paragraph::new(clock).style(style(0x70)),
            Rect::new(r.x + clock_x, r.y + clock_y, clock_w, 1),
        );
        spec.caption(
            Paragraph::new(fit_music_text(
                &format!(
                    "{} {:02}/{:02}",
                    language.text("Track"),
                    self.track_index().map_or(0, |i| i + 1),
                    self.playlist.order.len()
                ),
                14,
            ))
            .style(style(0x70)),
            Rect::new(r.x + 2, r.y + 5, 14, 1),
        );
        let status = language.text(match self.transport {
            Transport::Stopped => "Stopped",
            Transport::Paused => "Paused",
            Transport::Playing => {
                if self.snapshot.ready {
                    "Playing"
                } else {
                    "Loading"
                }
            }
        });
        spec.caption(
            Paragraph::new(fit_music_text(status, 11)).style(style(0x70)),
            Rect::new(r.x + 17, r.y + 5, 11, 1),
        );
        let mut progress = vec![Span::styled("├", style(0x78))];
        if duration.is_some_and(|d| !d.is_zero()) {
            let thumb = meter_cells(self.snapshot.position, duration, 60).min(59);
            progress.push(Span::styled("━".repeat(thumb), style(0x30)));
            progress.push(Span::styled("■", style(0x7f)));
            progress.push(Span::styled("─".repeat(59 - thumb), style(0x78)));
        } else {
            progress.push(Span::styled("─".repeat(60), style(0x78)));
        }
        progress.push(Span::styled("┤", style(0x78)));
        spec.caption(
            Paragraph::new(Line::from(progress)),
            Rect::new(r.x + 2, r.y + 6, 62, 1),
        );
        // Add controls in visual reading order. Stable IDs preserve commands,
        // enabled states and the existing focus restoration paths.
        let mut add = |id, x, y, w, kind| {
            let mut control =
                Control::new(id, Rect::new(r.x + x, r.y + y, w, 1), kind, ctx.target(id));
            control.enabled = self.enabled(id);
            control.enter_action = true;
            spec.control(control);
        };
        add(
            9,
            50,
            clock_y,
            14,
            ControlKind::Button(
                language
                    .text(if self.remaining {
                        "Remaining"
                    } else {
                        "Elapsed"
                    })
                    .into(),
            ),
        );
        for (id, x, w, label) in [
            (0, 4, 7, "◄◄".into()),
            (1, 12, 9, format!("−{}s", self.preferences.seek_seconds)),
            (
                PLAY,
                22,
                11,
                language
                    .text(if self.transport == Transport::Playing {
                        "Pause"
                    } else {
                        "Play"
                    })
                    .into(),
            ),
            (6, 34, 9, language.text("Stop").into()),
            (3, 44, 9, format!("+{}s", self.preferences.seek_seconds)),
            (4, 54, 7, "►►".into()),
        ] {
            add(id, x, 8, w, ControlKind::Button(label));
        }
        add(5, 25, 10, 5, ControlKind::Button("−".into()));
        add(11, 51, 10, 5, ControlKind::Button("+".into()));
        add(
            10,
            2,
            11,
            26,
            ControlKind::Checkbox {
                caption: language.text("Shuffle").into(),
                checked: self.playback.mode.shuffle,
            },
        );
        add(
            14,
            47,
            11,
            16,
            ControlKind::Choice(vec![language
                .text(match self.playback.mode.repeat {
                    modes::Repeat::Off => "Off",
                    modes::Repeat::One => "One",
                    modes::Repeat::All => "All",
                })
                .into()]),
        );
        for (id, x, w, label) in [
            (7, 2, 12, "Open…"),
            (12, 16, 16, "Playlist…"),
            (13, 35, 13, "Info…"),
            (8, 49, 15, "Settings…"),
        ] {
            add(
                id,
                x,
                15,
                w,
                ControlKind::Button(language.text(label).into()),
            );
        }
        spec.caption(
            Paragraph::new(language.text("Volume")).style(style(0x70)),
            Rect::new(r.x + 12, r.y + 10, 12, 1),
        );
        let filled = usize::from(self.request.volume.min(100)) / 10;
        spec.caption(
            Paragraph::new(Line::from(vec![
                Span::styled("▰".repeat(filled), style(0x30)),
                Span::styled("·".repeat(10 - filled), style(0x78)),
            ])),
            Rect::new(r.x + 32, r.y + 10, 10, 1),
        );
        spec.caption(
            Paragraph::new(format!("{}%", self.request.volume)).style(style(0x70)),
            Rect::new(r.x + 44, r.y + 10, 5, 1),
        );
        spec.caption(
            Paragraph::new(format!("{}:", language.text("Repeat"))).style(style(0x70)),
            Rect::new(r.x + 38, r.y + 11, 8, 1),
        );
        let notice = if !crate::plugins::music_playback_available() {
            language
                .text("Audio playback is not included in this build")
                .into()
        } else if !self.notice.is_empty() {
            self.notice.clone()
        } else if self.transport == Transport::Playing && !self.snapshot.ready {
            language.text("Loading audio…").into()
        } else {
            String::new()
        };
        spec.caption(
            Paragraph::new(fit_music_text(&notice, 62)).style(style(0x78)),
            Rect::new(r.x + 2, r.y + 13, 62, 1),
        );
        spec.caption(
            Paragraph::new("─".repeat(64)).style(style(0x7f)),
            Rect::new(r.x + 1, r.y + 14, 64, 1),
        );
        self.title_indicator(&mut spec, ctx);
        spec
    }
    fn main_controls(&self, ctx: &Context) -> DialogSpec<'_> {
        if self.expanded {
            self.expanded_controls(ctx)
        } else {
            self.compact_controls(ctx)
        }
    }
    fn compact_controls(&self, ctx: &Context) -> DialogSpec<'static> {
        let r = self.bounds(ctx);
        let language = ctx.language;
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let mut spec = ctx.dialog(r, self.focus, language.text("Music Player"), PLAY, CLOSE);
        spec.native(
            crate::turbo::widgets::group_box(""),
            Rect::new(r.x + 1, r.y + 1, 34, 5),
        );
        for (label, x, w) in [
            ("Track", r.x + 4, 8),
            (
                if self.remaining {
                    "Remaining"
                } else {
                    "Elapsed"
                },
                r.x + 17,
                12,
            ),
        ] {
            spec.caption(
                Paragraph::new(language.text(label)).style(style(0x70)),
                Rect::new(x, r.y + 1, w, 1),
            );
        }
        const DIGITS: [[&str; 3]; 10] = [
            ["▄▄▄", "█ █", "█▄█"],
            [" ▄ ", " █ ", "▄█▄"],
            ["▄▄▄", " ▄█", "█▄▄"],
            ["▄▄▄", " ▄█", "▄▄█"],
            ["▄ ▄", "█▄█", "  █"],
            ["▄▄▄", "█▄ ", "▄▄█"],
            ["▄▄▄", "█▄ ", "█▄█"],
            ["▄▄▄", "  █", "  █"],
            ["▄▄▄", "█▄█", "█▄█"],
            ["▄▄▄", "█▄█", "▄▄█"],
        ];
        let track = self.track_index().map_or(0, |i| i + 1);
        let time = if self.remaining {
            self.track_duration()
                .map(|d| d.saturating_sub(self.snapshot.position))
        } else {
            Some(self.snapshot.position)
        };
        let seconds = time.unwrap_or_default().as_secs().min(5999);
        let digits = [
            track / 10,
            track % 10,
            (seconds / 600) as usize,
            (seconds / 60 % 10) as usize,
            (seconds / 10 % 6) as usize,
            (seconds % 10) as usize,
        ];
        for (x, digit) in [r.x + 3, r.x + 7, r.x + 12, r.x + 16, r.x + 22, r.x + 26]
            .into_iter()
            .zip(digits)
        {
            for (row, text) in DIGITS[digit].into_iter().enumerate() {
                spec.caption(
                    Paragraph::new(if x >= r.x + 12 && time.is_none() {
                        "   "
                    } else {
                        text
                    })
                    .style(style(0x70)),
                    Rect::new(x, r.y + 2 + row as u16, 3, 1),
                );
            }
        }
        if time.is_none() {
            spec.caption(
                Paragraph::new("--:--").style(style(0x70)),
                Rect::new(r.x + 17, r.y + 3, 5, 1),
            );
        }
        for y in [r.y + 3, r.y + 4].into_iter().filter(|_| time.is_some()) {
            spec.caption(
                Paragraph::new("▪").style(style(0x70)),
                Rect::new(r.x + 20, y, 1, 1),
            );
        }
        for (row, text) in ["M", "U", "S"].into_iter().enumerate() {
            spec.caption(
                Paragraph::new(text).style(style(0x70)),
                Rect::new(r.x + 32, r.y + 2 + row as u16, 1, 1),
            );
        }
        let labels = [
            "◄◄".into(),
            format!("−{}s", self.preferences.seek_seconds),
            language
                .text(if self.transport == Transport::Playing {
                    "Pause"
                } else {
                    "Play"
                })
                .into(),
            format!("+{}s", self.preferences.seek_seconds),
            "►►".into(),
        ];
        for (id, label) in labels.into_iter().enumerate() {
            let (x, w) = [(1, 6), (7, 5), (12, 11), (23, 5), (28, 6)][id];
            let mut control = Control::new(
                id,
                Rect::new(r.x + x, r.y + 6, w, 1),
                ControlKind::Button(label),
                ctx.target(id),
            );
            control.enabled = self.enabled(id);
            spec.control(control);
        }
        self.title_indicator(&mut spec, ctx);
        spec
    }
    fn title_indicator(&self, spec: &mut DialogSpec<'_>, ctx: &Context) {
        let r = self.bounds(ctx);
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        spec.caption(
            Paragraph::new(Line::from(vec![
                Span::styled("[", style(0x7f)),
                Span::styled(
                    match self.transport {
                        Transport::Playing => "►",
                        Transport::Paused => "Ⅱ",
                        Transport::Stopped => "■",
                    },
                    style(0x7a),
                ),
                Span::styled("]", style(0x7f)),
            ])),
            Rect::new(r.x + 2, r.y, 3, 1),
        );
    }
}

fn fit_music_text(text: &str, width: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    if text.width() <= width {
        return text.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut cells = 0;
    for grapheme in text.graphemes(true) {
        let next = grapheme.width();
        if cells + next > width - 1 {
            break;
        }
        out.push_str(grapheme);
        cells += next;
    }
    out.push('…');
    out
}
fn meter_cells(position: Duration, duration: Option<Duration>, width: usize) -> usize {
    duration.filter(|d| !d.is_zero()).map_or(0, |d| {
        ((position.min(d).as_nanos() * width as u128) / d.as_nanos()) as usize
    })
}

fn format_time(duration: Duration) -> String {
    let s = duration.as_secs();
    format!("{:02}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    #[derive(Default)]
    struct FakeState {
        request: audio::Request,
        snapshot: audio::Snapshot,
    }
    struct Fake(Rc<RefCell<FakeState>>);
    impl audio::Engine for Fake {
        fn submit(&mut self, request: audio::Request) {
            self.0.borrow_mut().request = request;
        }
        fn snapshot(&self) -> audio::Snapshot {
            self.0.borrow().snapshot.clone()
        }
    }
    fn context() -> Context {
        Context {
            binding: Binding {
                plugin: MP3_PLAYER,
                owner: Owner::Document(crate::desktop::WindowId::MANAGER),
            },
            scope: ControlScope::Document {
                window: crate::desktop::WindowId::MANAGER,
                revision: 0,
            },
            area: Rect::new(0, 0, 100, 32),
            language: crate::localization::Language::English,
            palette: crate::palette::PaletteProfile::ClassicDn,
            active: true,
            now: Instant::now(),
            storage: None,
            mp3: Default::default(),
            nearby: Default::default(),
        }
    }
    fn player() -> (Mp3Player, Rc<RefCell<FakeState>>) {
        let fake = Rc::new(RefCell::new(FakeState::default()));
        let mut p = Mp3Player {
            engine: Box::new(Fake(fake.clone())),
            next_track: 1,
            ..Default::default()
        };
        p.playlist.append(
            ["one.mp3", "two.mp3"]
                .into_iter()
                .enumerate()
                .map(|(id, path)| playlist::Track {
                    id: id as u64,
                    path: path.into(),
                    tags: tags::Tags::default(),
                    duration: None,
                }),
        );
        p.current = Some(0);
        (p, fake)
    }
    #[test]
    fn background_completion_advances_once_and_stale_results_cannot_restart() {
        let (mut p, fake) = player();
        let mut ctx = context();
        ctx.active = false;
        p.start(ctx.now);
        let old = p.request.generation;
        fake.borrow_mut().snapshot = audio::Snapshot {
            generation: old,
            ready: true,
            finished: true,
            ..Default::default()
        };
        p.event(Event::Tick, &ctx);
        assert_eq!(p.current, Some(1));
        let next = p.request.generation;
        p.event(Event::Tick, &ctx);
        assert_eq!(p.request.generation, next);
        fake.borrow_mut().snapshot.generation = next;
        p.event(Event::Tick, &ctx);
        assert_eq!(p.transport, Transport::Stopped);
        p.event(Event::Tick, &ctx);
        assert_eq!(p.transport, Transport::Stopped);
        assert!(p.next_wake(ctx.now).is_none());
        assert!(fake.borrow().request.path.is_none());
    }
    #[test]
    fn device_errors_stop_visibly_retry_and_ignore_retired_generations() {
        for message in [
            "No default output device",
            "Permission denied",
            "Device disconnected",
        ] {
            let (mut p, fake) = player();
            let mut ctx = context();
            ctx.active = false;
            p.start(ctx.now);
            let failed = p.request.generation;
            fake.borrow_mut().snapshot = audio::Snapshot {
                generation: failed,
                error: Some(message.into()),
                ..Default::default()
            };
            p.event(Event::Tick, &ctx);
            assert_eq!(p.transport, Transport::Stopped);
            assert_eq!(p.notice, message);
            assert!(p.expanded);
            assert!(fake.borrow().request.path.is_none());
            assert_eq!(p.playlist.order.len(), 2);
            p.start(ctx.now);
            let retried = p.request.generation;
            assert_ne!(retried, failed);
            assert!(p.notice.is_empty());
            // The old error cannot stop a newer request while its device opens.
            p.event(Event::Tick, &ctx);
            assert_eq!(p.request.generation, retried);
            assert_eq!(p.transport, Transport::Playing);
            p.close();
            fake.borrow_mut().snapshot = audio::Snapshot {
                generation: retried,
                ready: true,
                ..Default::default()
            };
            p.event(Event::Tick, &ctx);
            assert_eq!(p.transport, Transport::Stopped);
            assert!(fake.borrow().request.path.is_none());
            assert!(p.playlist.order.is_empty());
        }
    }
    #[cfg(all(
        feature = "audio",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    #[test]
    fn open_picker_plays_existing_entry_and_cancel_keeps_audio_and_queue() {
        use crate::turbo::file_dialog::{Mode, Outcome};
        let (mut p, fake) = player();
        let ctx = context();
        p.expanded = true;
        assert!(matches!(
            p.action(7, &ctx).as_slice(),
            [Effect::RequestMusicFile { play: true }]
        ));
        p.picker_play = true;
        p.finish_picker(Outcome::Selected(Mode::Open, "two.mp3".into()), &ctx);
        assert_eq!(p.current, Some(1));
        assert_eq!(p.transport, Transport::Playing);
        assert_eq!(p.playlist.order.len(), 2);
        assert_eq!(fake.borrow().request.path, Some("two.mp3".into()));
        let generation = p.request.generation;
        p.finish_picker(Outcome::Cancelled, &ctx);
        assert_eq!(p.request.generation, generation);
        p.picker_play = false;
        p.finish_picker(Outcome::Selected(Mode::Open, "one.mp3".into()), &ctx);
        assert_eq!(p.request.generation, generation);
        assert_eq!(p.playlist.order.len(), 2);
        p.finish_picker(Outcome::Selected(Mode::Open, "wrong.txt".into()), &ctx);
        assert!(p.notice.contains("Choose an MP3"));
        assert_eq!(p.request.generation, generation);
        p.playlist.show();
        p.playlist.action(1000);
        p.action(playlist::PLAY_TRACK, &ctx);
        assert_eq!(p.current, Some(0));
        assert!(p.playlist.open);
    }
    fn rendered_player(p: &Mp3Player, ctx: &Context) -> crate::turbo::scene::Scene {
        let mut f = NativeFrame::new(ctx.area, ctx.palette);
        if let Owner::Document(id) = ctx.binding.owner {
            f.document_scope(Some(id));
        }
        f.render_widget(crate::turbo::widgets::dialog("Music Player"), p.bounds(ctx));
        p.main_controls(ctx).append(&mut f);
        f.finish()
    }
    fn player_row(scene: &crate::turbo::scene::Scene, r: Rect, y: u16) -> String {
        (r.x..r.right())
            .map(|x| {
                scene
                    .cells
                    .get(&(x, r.y + y))
                    .map_or(" ", |c| c.text.as_str())
            })
            .collect()
    }
    #[test]
    fn cyrillic_metadata_and_filename_survive_player_layouts_queue_and_info() {
        let (mut p, _) = player();
        p.current = Some(0);
        p.playlist.order[0].path = "Каталог/Ёжик-е\u{308}.mp3".into();
        p.playlist.order[0].tags = tags::Tags {
            title: "Ёжик Mix е\u{308}".into(),
            artist: "Группа Ёлка".into(),
            album: "Альбом Ёё".into(),
        };
        for language in [
            crate::localization::Language::English,
            crate::localization::Language::Russian,
        ] {
            let mut ctx = context();
            ctx.language = language;
            for expanded in [true, false, true] {
                p.expanded = expanded;
                for _ in 0..3 {
                    let frame = rendered_player(&p, &ctx);
                    if expanded {
                        assert!(player_row(&frame, p.bounds(&ctx), 2).contains("Группа Ёлка"));
                        assert!(player_row(&frame, p.bounds(&ctx), 3).contains("Ёжик Mix е\u{308}"));
                    }
                    let [Effect::Help { text, .. }] = &p.info(&ctx)[..] else {
                        panic!("track Info")
                    };
                    assert!(text.contains("Ёжик Mix е\u{308}"));
                    assert!(text.contains("Каталог/Ёжик-е\u{308}.mp3"));
                    assert_eq!(p.playlist.order[0].name(), "Ёжик-е\u{308}.mp3");
                }
            }
        }
        p.playlist.order[0].tags = Default::default();
        let ctx = context();
        p.expanded = true;
        assert!(
            player_row(&rendered_player(&p, &ctx), p.bounds(&ctx), 3).contains("Ёжик-е\u{308}.mp3")
        );
        assert_eq!(p.playlist.order[0].description(), "Ёжик-е\u{308}.mp3");
        let long = "Ёе\u{308}".repeat(80);
        let clipped = fit_music_text(&long, 62);
        assert!(unicode_width::UnicodeWidthStr::width(clipped.as_str()) <= 62);
        assert!(!clipped.contains('\u{fffd}'));
    }
    #[test]
    fn expanded_design_has_one_timer_separate_metadata_and_readonly_meters() {
        let (mut p, _) = player();
        let ctx = context();
        p.expanded = true;
        p.current = Some(0);
        p.playlist.order[0].tags.artist = "Artist 界".into();
        p.playlist.order[0].tags.title = "Title é".into();
        p.snapshot.position = Duration::from_secs(35);
        p.snapshot.duration = Some(Duration::from_secs(233));
        p.snapshot.ready = true;
        let r = p.bounds(&ctx);
        assert_eq!((r.width, r.height), (66, 18));
        for state in [Transport::Stopped, Transport::Playing, Transport::Paused] {
            p.transport = state;
            let frame = rendered_player(&p, &ctx);
            assert!(player_row(&frame, r, 2).contains("Artist 界"));
            assert!(player_row(&frame, r, 3).contains("Title é"));
            let all = (0..r.height)
                .map(|y| player_row(&frame, r, y))
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(all.matches("00:35 / 03:53").count(), 1, "{all}");
            assert!(player_row(&frame, r, 6).contains('■'));
            let spec = p.main_controls(&ctx);
            assert!(spec.controls.iter().all(|c| c.bounds.y != r.y + 6));
            assert!(spec
                .controls
                .iter()
                .any(|c| c.id == 10 && matches!(c.kind, ControlKind::Checkbox { .. })));
            assert!(spec
                .controls
                .iter()
                .any(|c| c.id == 14 && matches!(c.kind, ControlKind::Choice(_))));
            let pixels = |scene: crate::turbo::scene::Scene| {
                scene
                    .cells
                    .into_iter()
                    .map(|(pos, c)| (pos, c.text, c.attribute, c.style))
                    .collect::<Vec<_>>()
            };
            assert_eq!(pixels(frame), pixels(rendered_player(&p, &ctx)));
        }
        p.remaining = true;
        p.snapshot.duration = None;
        p.playlist.order[0].duration = None;
        let f = rendered_player(&p, &ctx);
        assert!(player_row(&f, r, 5).contains("--:-- / --:--"));
        assert!(!player_row(&f, r, 6).contains('■'));
        p.notice = "A visible error".into();
        assert!(player_row(&rendered_player(&p, &ctx), r, 13).contains(
            if crate::plugins::music_playback_available() {
                "A visible error"
            } else {
                "Audio playback is not included in this build"
            }
        ));
        p.snapshot.duration = Some(Duration::from_secs(60_001));
        p.remaining = false;
        assert!(player_row(&rendered_player(&p, &ctx), r, 5).contains("1000:01"));
        p.snapshot.duration = Some(Duration::MAX);
        p.snapshot.position = Duration::MAX;
        assert!(player_row(&rendered_player(&p, &ctx), r, 4).contains(&format_time(Duration::MAX)));
    }
    #[test]
    fn music_meters_and_unicode_truncation_are_bounded() {
        assert_eq!(
            meter_cells(Duration::from_secs(5), Some(Duration::from_secs(10)), 60),
            30
        );
        assert_eq!(
            meter_cells(Duration::MAX, Some(Duration::from_secs(10)), 60),
            60
        );
        assert_eq!(meter_cells(Duration::MAX, Some(Duration::MAX), 60), 60);
        assert_eq!(
            meter_cells(Duration::from_secs(5), Some(Duration::ZERO), 60),
            0
        );
        assert_eq!(meter_cells(Duration::from_secs(5), None, 60), 0);
        assert_eq!(fit_music_text("界é👩‍💻 abc", 6), "界é👩‍💻…");
        assert_eq!(fit_music_text("界", 1), "…");
        assert_eq!(fit_music_text("abc", 0), "");
    }
    #[cfg(all(
        feature = "audio",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    #[test]
    fn repeat_popup_selection_cancel_and_parent_focus_preserve_playback() {
        let (mut p, fake) = player();
        let ctx = context();
        p.expanded = true;
        p.start(ctx.now);
        let generation = p.request.generation;
        p.action(14, &ctx);
        p.event(
            Event::Key(crossterm::event::KeyEvent::new(
                K::Down,
                crossterm::event::KeyModifiers::NONE,
            )),
            &ctx,
        );
        assert_eq!(p.repeat_cursor, Some(1));
        assert_eq!(p.playback.mode.repeat, modes::Repeat::Off);
        p.action(REPEAT_CANCEL, &ctx);
        assert_eq!(p.focus, 14);
        assert!(p.repeat_cursor.is_none());
        assert_eq!(p.playback.mode.repeat, modes::Repeat::Off);
        p.action(14, &ctx);
        p.action(REPEAT_SET + 2, &ctx);
        assert_eq!(p.playback.mode.repeat, modes::Repeat::All);
        assert_eq!(p.focus, 14);
        assert_eq!(p.request.generation, generation);
        assert_eq!(fake.borrow().request.path, Some("one.mp3".into()));
        p.action(14, &ctx);
        assert_eq!(p.repeat_cursor, Some(2));
        p.close();
        assert!(p.repeat_cursor.is_none());
    }
    #[test]
    fn load_error_and_close_cancel_loading_without_losing_queue_on_error() {
        let (mut p, fake) = player();
        let ctx = context();
        p.start(ctx.now);
        fake.borrow_mut().snapshot = audio::Snapshot {
            generation: p.request.generation,
            error: Some("bad MP3".into()),
            ..Default::default()
        };
        p.event(Event::Tick, &ctx);
        assert_eq!(p.transport, Transport::Stopped);
        assert_eq!(p.notice, "bad MP3");
        assert_eq!(p.playlist.order.len(), 2);
        p.start(ctx.now);
        let old = p.request.generation;
        p.close();
        assert!(fake.borrow().request.path.is_none());
        fake.borrow_mut().snapshot = audio::Snapshot {
            generation: old,
            ready: true,
            finished: true,
            ..Default::default()
        };
        p.open(&ctx);
        p.event(Event::Tick, &ctx);
        assert_eq!(p.transport, Transport::Stopped);
        assert!(p.playlist.order.is_empty());
    }
    #[test]
    fn live_queue_removal_stops_current_and_close_keeps_changes() {
        let (mut p, fake) = player();
        let ctx = context();
        p.start(ctx.now);
        p.expanded = true;
        p.action(12, &ctx);
        p.action(202, &ctx);
        assert_eq!(p.playlist.order.len(), 1);
        assert_eq!(p.current, Some(1));
        assert_eq!(p.transport, Transport::Stopped);
        assert!(fake.borrow().request.path.is_none());
        p.action(203, &ctx);
        p.action(12, &ctx);
        assert_eq!(p.playlist.order.len(), 1);
    }
    #[cfg(all(
        feature = "audio",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    #[test]
    fn pause_seek_volume_and_neighbor_selection_have_distinct_semantics() {
        let (mut p, fake) = player();
        let ctx = context();
        p.expanded = true;
        p.action(PLAY, &ctx);
        fake.borrow_mut().snapshot = audio::Snapshot {
            generation: p.request.generation,
            ready: true,
            position: Duration::from_secs(2),
            duration: Some(Duration::from_secs(8)),
            ..Default::default()
        };
        p.event(Event::Tick, &ctx);
        p.action(1, &ctx);
        assert_eq!(fake.borrow().request.seek.unwrap().1, Duration::ZERO);
        p.action(3, &ctx);
        assert_eq!(
            fake.borrow().request.seek.unwrap().1,
            Duration::from_secs(5)
        );
        p.action(3, &ctx);
        assert_eq!(
            fake.borrow().request.seek.unwrap().1,
            Duration::from_secs(8)
        );
        p.action(PLAY, &ctx);
        assert_eq!(p.transport, Transport::Paused);
        assert_eq!(p.focus, PLAY);
        assert!(fake.borrow().request.paused);
        p.action(PLAY, &ctx);
        assert_eq!(p.transport, Transport::Playing);
        assert!(!fake.borrow().request.paused);
        for _ in 0..15 {
            p.action(11, &ctx);
        }
        assert_eq!(p.request.volume, 100);
        for _ in 0..6 {
            p.action(5, &ctx);
        }
        assert_eq!(fake.borrow().request.volume, 40);
        p.action(4, &ctx);
        assert_eq!(p.current, Some(1));
        assert_eq!(p.transport, Transport::Playing);
        p.action(6, &ctx);
        p.action(0, &ctx);
        assert_eq!(p.current, Some(0));
        assert_eq!(p.transport, Transport::Stopped);
    }
    #[test]
    fn queue_limit_and_live_removal_are_session_local() {
        let mut p = playlist::Playlist::default();
        p.show();
        assert_eq!(
            p.append((0..70).map(|id| playlist::Track {
                id,
                path: "same.mp3".into(),
                tags: Default::default(),
                duration: None
            })),
            64
        );
        assert!(!p.can_add());
        p.action(playlist::DELETE);
        p.action(playlist::CLOSE_LIST);
        assert_eq!(p.order.len(), 63);
        p.show();
        assert_eq!(p.selected(), Some(1));
        assert_ne!(p.order[0].id, p.order[1].id);
    }
    #[cfg(all(
        feature = "audio",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    #[test]
    fn inline_modes_apply_and_repeat_one_eof_restarts_once() {
        let (mut p, fake) = player();
        let ctx = context();
        p.expanded = true;
        p.start(ctx.now);
        p.action(10, &ctx);
        assert!(p.playback.mode.shuffle);
        p.action(14, &ctx);
        assert_eq!(p.playback.mode.repeat, modes::Repeat::Off);
        p.action(REPEAT_SET + 1, &ctx);
        assert_eq!(p.playback.mode.repeat, modes::Repeat::One);
        let before = p.request.generation;
        fake.borrow_mut().snapshot = audio::Snapshot {
            generation: before,
            ready: true,
            finished: true,
            ..Default::default()
        };
        p.event(Event::Tick, &ctx);
        assert_eq!(p.current, Some(0));
        assert_ne!(p.request.generation, before);
        let after = p.request.generation;
        p.event(Event::Tick, &ctx);
        assert_eq!(p.request.generation, after);
        p.close();
        p.open(&ctx);
        assert_eq!(p.playback.mode, modes::Mode::default());
    }
    #[test]
    fn pending_import_cancellation_and_close_discard_a_late_batch() {
        let (mut p, _) = player();
        let ctx = context();
        p.requests = crate::requests::RequestScope::new(crate::requests::Owner::Document(
            crate::desktop::WindowId::MANAGER,
        ));
        let (ready, started) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let task = crate::task::Task::spawn_owned(&p.requests, "mp3-import", move |cancel| {
            ready.send(cancel).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            import::Result {
                tracks: vec![("late.mp3".into(), tags::Tags::default(), None)],
                ..Default::default()
            }
        })
        .unwrap();
        let cancel = started.recv_timeout(Duration::from_secs(5)).unwrap();
        p.import = Some(import::Pending {
            task: Some(task),
            progress: Default::default(),
            wake: ctx.now,
            play: None,
            visited: 0,
            found: 0,
        });
        p.action(IMPORT_CANCEL, &ctx);
        assert!(cancel.cancelled());
        assert_eq!(p.playlist.order.len(), 2);
        release.send(()).unwrap();
        p.close();
        p.open(&ctx);
        p.event(Event::Tick, &ctx);
        assert!(p.playlist.order.is_empty());
    }
    #[test]
    fn inactive_import_completion_updates_only_its_owner_and_keeps_audio_running() {
        let (mut p, fake) = player();
        let mut ctx = context();
        ctx.active = false;
        p.start(ctx.now);
        p.requests = crate::requests::RequestScope::new(crate::requests::Owner::Document(
            crate::desktop::WindowId::MANAGER,
        ));
        fake.borrow_mut().snapshot = audio::Snapshot {
            generation: p.request.generation,
            ready: true,
            position: Duration::from_secs(2),
            ..Default::default()
        };
        let task = crate::task::Task::spawn_owned(&p.requests, "mp3-import", |_| import::Result {
            tracks: vec![(
                "new.mp3".into(),
                tags::Tags {
                    title: "Song".into(),
                    artist: "Artist".into(),
                    album: "Album".into(),
                },
                None,
            )],
            ..Default::default()
        })
        .unwrap();
        p.import = Some(import::Pending {
            task: Some(task),
            progress: Default::default(),
            wake: ctx.now,
            play: None,
            visited: 0,
            found: 0,
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while p.import.is_some() {
            assert!(p.event(Event::Tick, &ctx).is_empty());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(p.playlist.order.len(), 3);
        assert_eq!(p.playlist.order[2].description(), "Artist — Song");
        assert_eq!(p.transport, Transport::Playing);
        assert_eq!(p.current, Some(0));
        assert_eq!(p.snapshot.position, Duration::from_secs(2));
    }
    #[cfg(all(
        feature = "audio",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    #[test]
    fn setup_is_a_draft_time_toggles_and_duration_updates_live_and_draft_rows() {
        let (mut p, fake) = player();
        let ctx = context();
        p.expanded = true;
        assert!(p.enabled(8) && p.enabled(9));
        p.action(8, &ctx);
        p.setup_action(603);
        p.setup_action(605);
        p.setup_action(607);
        assert_eq!(p.preferences.seek_seconds, 5);
        assert_eq!(p.request.volume, 50);
        p.action(8, &ctx);
        p.setup_action(603);
        p.setup_action(605);
        assert!(matches!(
            p.setup_action(606).as_slice(),
            [Effect::Mp3Setup {
                seek_seconds: 30,
                default_volume: 60
            }]
        ));
        assert_eq!(p.request.volume, 60);
        p.action(9, &ctx);
        assert!(p.remaining);
        p.start(ctx.now);
        p.playlist.show();
        fake.borrow_mut().snapshot = audio::Snapshot {
            generation: p.request.generation,
            ready: true,
            duration: Some(Duration::from_secs(60001)),
            position: Duration::from_secs(2),
            ..Default::default()
        };
        p.poll_audio(ctx.now);
        assert_eq!(
            p.playlist.order[0].duration,
            Some(Duration::from_secs(60001))
        );
        let spec = p.playlist.controls(&ctx);
        assert!(spec.controls.iter().any(|c| matches!(&c.kind,ControlKind::List{items,..} if items.iter().any(|s| s.contains("1000:01")))));
        p.playlist.action(playlist::CLOSE_LIST);
        assert_eq!(
            p.playlist.order[0].duration,
            Some(Duration::from_secs(60001))
        );
        p.action(3, &ctx);
        assert_eq!(
            fake.borrow().request.seek.unwrap().1,
            Duration::from_secs(32)
        );
        p.close();
        let mut ctx = context();
        ctx.mp3.seek_seconds = 10;
        ctx.mp3.default_volume = 70;
        p.open(&ctx);
        assert!(!p.remaining && p.setup_draft.is_none());
        assert_eq!((p.preferences.seek_seconds, p.request.volume), (10, 70));
    }
    #[cfg(all(
        feature = "audio",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    #[test]
    fn enter_reuses_entries_when_full_and_busy_or_failed_import_never_stops_audio() {
        let (mut p, fake) = player();
        let ctx = context();
        p.playlist.append((2..64).map(|id| playlist::Track {
            id,
            path: format!("{id}.mp3").into(),
            tags: Default::default(),
            duration: None,
        }));
        p.play_file("two.mp3".into(), &ctx);
        assert_eq!(p.current, Some(1));
        assert_eq!(p.playlist.order.len(), 64);
        assert_eq!(p.transport, Transport::Playing);
        let generation = p.request.generation;
        p.play_file("new.mp3".into(), &ctx);
        assert_eq!(p.request.generation, generation);
        assert!(p.notice.contains("Queue is full"));
        assert_eq!(fake.borrow().request.path, Some("two.mp3".into()));
        p.playlist.show();
        p.play_file("one.mp3".into(), &ctx);
        assert_eq!(p.current, Some(0));
        assert_ne!(p.request.generation, generation);
    }
    #[cfg(all(
        feature = "audio",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    #[test]
    fn seek_and_remaining_use_imported_duration_when_decoder_has_no_total() {
        let (mut p, fake) = player();
        let ctx = context();
        p.expanded = true;
        p.playlist.order[0].duration = Some(Duration::from_secs(8));
        p.start(ctx.now);
        fake.borrow_mut().snapshot = audio::Snapshot {
            generation: p.request.generation,
            ready: true,
            position: Duration::from_secs(2),
            ..Default::default()
        };
        p.poll_audio(ctx.now);
        assert_eq!(p.track_duration(), Some(Duration::from_secs(8)));
        p.action(3, &ctx);
        p.action(3, &ctx);
        assert_eq!(
            fake.borrow().request.seek.unwrap().1,
            Duration::from_secs(8)
        );
    }
}
