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
#[cfg(target_os = "macos")]
mod metadata;
pub(super) fn supports_path(path: &std::path::Path) -> bool {
    formats::Format::from_path(path).is_some()
}
#[cfg(any(target_os = "macos", test))]
mod duration;
mod import;
mod modes;
mod playlist;
mod setup;
mod tags;
const PLAY: usize = 2;
const ZOOM: usize = 100;
const CLOSE: usize = 101;
const HELP: usize = 102;
const HELP_TEXT: &str = "MP3, FLAC, WAV and Ogg Vorbis playback is available on macOS. Enable this plugin in Options > Configuration > Plugins.\nUtilities > Add music to player, Open and List > Add use marked files or folders from the active panel; without marks they use the entry under the cursor. Folders are scanned recursively in name order. Symbolic links are skipped. Cancel discards the whole import; the queue holds up to 64 entries.\nPlay starts the selected track or resumes a pause. Pause suspends playback; Stop resets the position. Outer arrows select the previous/next track; inner arrows use the saved seek step. Set up edits the seek step and default volume. Time switches elapsed/remaining track time. Volume changes player volume only.\nMode controls Shuffle and Repeat: Off, One track or All tracks. Repeat One affects automatic track endings, not manual Next/Previous. Shuffle visits each entry once; Previous retraces that order. Repeat All repeats the same shuffled cycle. Queue edits rebuild the shuffle order. OK applies mode changes; Cancel discards them.\nEnable Play music files in internal player in Plugins settings to add and play the current supported music file with Enter; Ctrl+Enter still uses the OS default. Playlist Time shows track durations when available. ID3, Vorbis comments and RIFF artist/title replace filenames when available. Info shows title, artist, album and the file path. Missing, unsupported or oversized tags fall back to filenames.\nList edits a draft: OK applies it, Cancel discards it. Deleting the playing track stops playback. Music continues during import and while working in other windows. Closing the player, disabling the plugin or quitting DN stops playback and cancels import. The queue is session-only. The internal-opening option, seek step and default volume use normal DN settings saving.\nTab / Shift+Tab select controls; Enter / Space press a button. F1 opens Help. Alt+Z switches compact/expanded views; Ctrl+F5 moves the player; Esc closes the current page.";
const VOLUME_PAGE: u64 = 3;
const VOLUME_DOWN: usize = 300;
const VOLUME_UP: usize = 301;
const VOLUME_OK: usize = 302;
const VOLUME_CANCEL: usize = 303;
const MODE_PAGE: u64 = 4;
const MODE_SHUFFLE: usize = 400;
const MODE_OFF: usize = 401;
const MODE_ONE: usize = 402;
const MODE_ALL: usize = 403;
const MODE_OK: usize = 404;
const MODE_CANCEL: usize = 405;
const IMPORT_PAGE: u64 = 5;
const IMPORT_CANCEL: usize = 500;

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
    volume_draft: Option<u8>,
    volume_focus: usize,
    playback: modes::Order,
    mode_draft: Option<modes::Mode>,
    mode_focus: usize,
    import: Option<import::Pending>,
    requests: crate::requests::RequestScope,
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
            volume_draft: None,
            volume_focus: 0,
            playback: modes::Order::default(),
            mode_draft: None,
            mode_focus: 0,
            import: None,
            requests: crate::requests::RequestScope::new(crate::requests::Owner::Workspace),
        }
    }
}
impl Mp3Player {
    fn bounds(&self, ctx: &Context) -> Rect {
        crate::turbo::dialog_spec::centered(ctx.area, 36, if self.expanded { 18 } else { 9 })
    }
    fn count(&self) -> usize {
        if self.expanded {
            14
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
        if !cfg!(target_os = "macos") {
            return id == playlist::LIST;
        }
        match id {
            0 | 4 => self
                .playback
                .neighbor(self.current, id == 4, false)
                .is_some(),
            1 | 3 => self.transport != Transport::Stopped && self.snapshot.ready,
            PLAY => !self.playlist.order.is_empty(),
            5 => self.transport == Transport::Playing,
            6 => self.transport != Transport::Stopped,
            7 => self.playlist.can_add(),
            8..=12 => true,
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
        #[cfg(not(target_os = "macos"))]
        let _ = (paths, ctx);
        #[cfg(target_os = "macos")]
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
                        draft: self.playlist.open,
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
        if !cfg!(target_os = "macos") {
            return;
        }
        self.expanded = true;
        if self.import.is_some()
            || self.playlist.open
            || self.setup_draft.is_some()
            || self.volume_draft.is_some()
            || self.mode_draft.is_some()
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
            if pending.draft != self.playlist.open {
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
    fn mode_controls(&self, ctx: &Context) -> DialogSpec<'static> {
        let mode = self.mode_draft.unwrap_or(self.playback.mode);
        let r = crate::turbo::dialog_spec::centered(ctx.area, 46, 13);
        let mut spec = ctx.dialog(
            r,
            self.mode_focus,
            ctx.language.text("Playback mode"),
            MODE_OK,
            MODE_CANCEL,
        );
        for (id, action, y, kind) in [
            (
                0,
                MODE_SHUFFLE,
                2,
                ControlKind::Checkbox {
                    caption: ctx.language.text("Shuffle").into(),
                    checked: mode.shuffle,
                },
            ),
            (
                1,
                MODE_OFF,
                5,
                ControlKind::Radio {
                    caption: ctx.language.text("Repeat off").into(),
                    checked: mode.repeat == modes::Repeat::Off,
                },
            ),
            (
                2,
                MODE_ONE,
                6,
                ControlKind::Radio {
                    caption: ctx.language.text("Repeat one track").into(),
                    checked: mode.repeat == modes::Repeat::One,
                },
            ),
            (
                3,
                MODE_ALL,
                7,
                ControlKind::Radio {
                    caption: ctx.language.text("Repeat all tracks").into(),
                    checked: mode.repeat == modes::Repeat::All,
                },
            ),
        ] {
            spec.control(Control::new(
                id,
                Rect::new(r.x + 3, r.y + y, 40, 1),
                kind,
                ctx.target(action),
            ));
        }
        for (id, action, label, x) in [(4, MODE_OK, "OK", 8), (5, MODE_CANCEL, "Cancel", 26)] {
            spec.control(Control::new(
                id,
                Rect::new(r.x + x, r.y + 10, 12, 1),
                ControlKind::Button(ctx.language.text(label).into()),
                ctx.target(action),
            ));
        }
        spec
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
        if let Some(mode) = &mut self.mode_draft {
            match id {
                MODE_SHUFFLE => {
                    mode.shuffle = !mode.shuffle;
                    self.mode_focus = 0;
                }
                MODE_OFF => {
                    mode.repeat = modes::Repeat::Off;
                    self.mode_focus = 1;
                }
                MODE_ONE => {
                    mode.repeat = modes::Repeat::One;
                    self.mode_focus = 2;
                }
                MODE_ALL => {
                    mode.repeat = modes::Repeat::All;
                    self.mode_focus = 3;
                }
                MODE_OK => {
                    self.playback.apply(
                        *mode,
                        self.playlist.order.iter().map(|t| t.id).collect(),
                        self.current,
                    );
                    self.mode_draft = None;
                    self.focus = 10;
                }
                MODE_CANCEL => {
                    self.mode_draft = None;
                    self.focus = 10;
                }
                _ => {}
            }
            return Vec::new();
        }
        if self.volume_draft.is_some() {
            match id {
                VOLUME_DOWN => {
                    self.volume_draft = self.volume_draft.map(|n| n.saturating_sub(10));
                    self.volume_focus = 0;
                }
                VOLUME_UP => {
                    self.volume_draft = self.volume_draft.map(|n| (n + 10).min(100));
                    self.volume_focus = 1;
                }
                VOLUME_OK => {
                    self.request.volume = self.volume_draft.take().unwrap();
                    self.submit();
                    self.focus = 11;
                }
                VOLUME_CANCEL => {
                    self.volume_draft = None;
                    self.focus = 11;
                }
                _ => {}
            }
            return Vec::new();
        }
        if self.playlist.open {
            if id == playlist::ADD && cfg!(target_os = "macos") && self.playlist.can_add() {
                return vec![Effect::RequestPanelPaths];
            }
            self.playlist.action(id);
            if !self.playlist.open {
                if id == playlist::OK {
                    if self.track_index().is_none() {
                        self.stop();
                        self.current = None;
                    }
                    if self.transport == Transport::Stopped {
                        self.current = self.playlist.selected();
                    }
                    self.sync_order();
                }
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
                        if self.transport == Transport::Paused {
                            self.request.paused = false;
                            self.transport = Transport::Playing;
                            self.submit();
                        } else if self.transport == Transport::Stopped {
                            self.start(ctx.now);
                        }
                    }
                    5 => {
                        self.request.paused = true;
                        self.transport = Transport::Paused;
                        self.focus = PLAY;
                        self.submit();
                    }
                    6 => self.stop(),
                    7 => return vec![Effect::RequestPanelPaths],
                    8 => {
                        self.setup_draft = Some(self.preferences);
                        self.setup_focus = 0;
                    }
                    9 => self.remaining = !self.remaining,
                    10 => {
                        self.mode_draft = Some(self.playback.mode);
                        self.mode_focus = 0;
                    }
                    13 => return self.info(ctx),
                    11 => {
                        self.volume_draft = Some(self.request.volume);
                        self.volume_focus = 0;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Vec::new()
    }
    fn volume_controls(&self, ctx: &Context) -> DialogSpec<'static> {
        let r = crate::turbo::dialog_spec::centered(ctx.area, 36, 9);
        let mut spec = ctx.dialog(
            r,
            self.volume_focus,
            ctx.language.text("Volume"),
            VOLUME_OK,
            VOLUME_CANCEL,
        );
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        spec.caption(
            Paragraph::new(format!(
                "{}%",
                self.volume_draft.unwrap_or(self.request.volume)
            ))
            .style(style(0x70)),
            Rect::new(r.x + 15, r.y + 2, 6, 1),
        );
        for (id, label, action, x, y) in [
            (0, "-10%", VOLUME_DOWN, 3, 4),
            (1, "+10%", VOLUME_UP, 20, 4),
            (2, "OK", VOLUME_OK, 3, 6),
            (3, "Cancel", VOLUME_CANCEL, 20, 6),
        ] {
            spec.control(Control::new(
                id,
                Rect::new(r.x + x, r.y + y, 12, 1),
                ControlKind::Button(ctx.language.text(label).into()),
                ctx.target(action),
            ));
        }
        spec
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
        self.requests = crate::requests::RequestScope::new(match ctx.binding.owner {
            Owner::Document(id) => crate::requests::Owner::Document(id),
            Owner::Modal(id) => crate::requests::Owner::Modal(id),
        });
        self.playback = modes::Order::default();
        self.mode_draft = None;
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
        self.volume_draft = None;
    }
    fn close(&mut self) {
        self.import = None;
        self.mode_draft = None;
        self.setup_draft = None;
        self.stop();
        self.playlist = playlist::Playlist::default();
        self.current = None;
        self.volume_draft = None;
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
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        match event {
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
            Event::Focus(id) if self.setup_draft.is_some() => {
                self.setup_focus = id;
                Vec::new()
            }
            Event::Focus(id) if self.mode_draft.is_some() => {
                self.mode_focus = id;
                Vec::new()
            }
            Event::Focus(id) if self.volume_draft.is_some() => {
                self.volume_focus = id;
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
        if self.volume_draft.is_some() {
            let bounds = self.volume_controls(ctx).bounds;
            result.push(LocalWindow {
                id: VOLUME_PAGE,
                parent: Some(0),
                bounds,
                caps: Capabilities::fixed(bounds.width, bounds.height),
            });
        }
        for (id, spec) in [
            self.setup_draft
                .map(|_| (setup::PAGE, self.setup_controls(ctx))),
            self.mode_draft
                .map(|_| (MODE_PAGE, self.mode_controls(ctx))),
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
        result
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        if self.import.is_some() {
            self.import_controls(ctx)
        } else if self.setup_draft.is_some() {
            self.setup_controls(ctx)
        } else if self.mode_draft.is_some() {
            self.mode_controls(ctx)
        } else if self.volume_draft.is_some() {
            self.volume_controls(ctx)
        } else if self.playlist.open {
            self.playlist.controls(ctx)
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
            interactive: !self.playlist.open
                && self.volume_draft.is_none()
                && self.setup_draft.is_none()
                && self.mode_draft.is_none()
                && self.import.is_none(),
            scene: frame.finish(),
            controls: Some(self.main_controls(ctx)),
        }];
        if self.playlist.open {
            let mut layer = self.playlist.layer(ctx);
            layer.interactive = self.import.is_none();
            layers.push(layer);
        }
        if self.volume_draft.is_some() {
            let spec = self.volume_controls(ctx);
            let mut frame = NativeFrame::new(ctx.area, ctx.palette);
            frame.render_widget(
                crate::turbo::widgets::dialog(ctx.language.text("Volume")),
                spec.bounds,
            );
            layers.push(Layer {
                id: VOLUME_PAGE,
                interactive: true,
                scene: frame.finish(),
                controls: Some(spec),
            });
        }
        for (id, spec) in [
            self.setup_draft
                .map(|_| (setup::PAGE, self.setup_controls(ctx))),
            self.mode_draft
                .map(|_| (MODE_PAGE, self.mode_controls(ctx))),
            self.import
                .as_ref()
                .map(|_| (IMPORT_PAGE, self.import_controls(ctx))),
        ]
        .into_iter()
        .flatten()
        {
            let mut frame = NativeFrame::new(ctx.area, ctx.palette);
            frame.render_widget(
                crate::turbo::widgets::dialog(ctx.language.text(if id == MODE_PAGE {
                    "Playback mode"
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
        View::Layers(layers)
    }
}
impl Mp3Player {
    fn main_controls(&self, ctx: &Context) -> DialogSpec<'_> {
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
            "◄◄", "<", "►", ">", "►►", "Pause", "Stop", "Open", "Set up", "Time", "Mode", "Volume",
            "List", "Info",
        ];
        for (id, label) in labels.iter().take(self.count()).enumerate() {
            let (x, y, w) = if id < 5 {
                let (x, w) = [(1, 6), (7, 5), (12, 11), (23, 5), (28, 6)][id];
                (r.x + x, r.y + 6, w)
            } else {
                let n = id - 5;
                (
                    r.x + 1 + (n % 3) as u16 * 11,
                    r.y + 8 + (n / 3) as u16 * 2,
                    11,
                )
            };
            let mut control = Control::new(
                id,
                Rect::new(x, y, w, 1),
                ControlKind::Button(language.text(label).into()),
                ctx.target(id),
            );
            control.enabled = self.enabled(id);
            spec.control(control);
        }
        if self.expanded {
            spec.caption(
                Paragraph::new(format!(
                    "{}  {} / {}  {}%",
                    language.text(match self.transport {
                        Transport::Stopped => "Stopped",
                        Transport::Playing =>
                            if self.snapshot.ready {
                                "Playing"
                            } else {
                                "Loading"
                            },
                        Transport::Paused => "Paused",
                    }),
                    format_time(self.snapshot.position),
                    self.track_duration()
                        .map_or_else(|| "--:--".into(), format_time),
                    self.request.volume
                ))
                .style(style(0x78)),
                Rect::new(r.x + 2, r.y + 14, 32, 1),
            );
            spec.caption(
                Paragraph::new(if !cfg!(target_os = "macos") {
                    language.text("Playback requires macOS").to_string()
                } else if !self.notice.is_empty() {
                    self.notice.clone()
                } else {
                    self.track_index().map_or_else(
                        || language.text("Empty playlist").to_string(),
                        |i| self.playlist.order[i].description(),
                    )
                })
                .style(style(0x78)),
                Rect::new(r.x + 2, r.y + 15, 32, 1),
            );
            if cfg!(target_os = "macos") {
                spec.caption(
                    Paragraph::new(format!(
                        "{} {} · {} {}",
                        language.text("Shuffle"),
                        language.text(if self.playback.mode.shuffle {
                            "On"
                        } else {
                            "Off"
                        }),
                        language.text("Repeat"),
                        language.text(match self.playback.mode.repeat {
                            modes::Repeat::Off => "Off",
                            modes::Repeat::One => "One",
                            modes::Repeat::All => "All",
                        })
                    ))
                    .style(style(0x78)),
                    Rect::new(r.x + 2, r.y + 16, 32, 1),
                );
            }
        }
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
        spec
    }
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
    fn queue_cancel_keeps_current_and_commit_removal_stops_it() {
        let (mut p, fake) = player();
        let ctx = context();
        p.start(ctx.now);
        p.expanded = true;
        p.action(12, &ctx);
        p.action(202, &ctx);
        p.action(203, &ctx);
        assert_eq!(p.playlist.order.len(), 2);
        assert_eq!(p.transport, Transport::Playing);
        p.action(12, &ctx);
        p.action(202, &ctx);
        p.action(200, &ctx);
        assert_eq!(p.playlist.order.len(), 1);
        assert_eq!(p.current, Some(1));
        assert_eq!(p.transport, Transport::Stopped);
        assert!(fake.borrow().request.path.is_none());
    }
    #[cfg(target_os = "macos")]
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
        p.action(5, &ctx);
        assert_eq!(p.transport, Transport::Paused);
        assert_eq!(p.focus, PLAY);
        assert!(fake.borrow().request.paused);
        p.action(PLAY, &ctx);
        assert_eq!(p.transport, Transport::Playing);
        assert!(!fake.borrow().request.paused);
        p.action(11, &ctx);
        for _ in 0..15 {
            p.action(VOLUME_UP, &ctx);
        }
        p.action(VOLUME_CANCEL, &ctx);
        assert_eq!(p.request.volume, 50);
        p.action(11, &ctx);
        p.action(VOLUME_DOWN, &ctx);
        p.action(VOLUME_OK, &ctx);
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
    fn queue_limit_duplicate_identity_and_cancel_are_session_local() {
        let mut p = playlist::Playlist::default();
        p.show();
        assert_eq!(
            p.append((0..70).map(|id| playlist::Track {
                id,
                path: "same.mp3".into(),
                tags: tags::Tags::default(),
                duration: None,
            })),
            64
        );
        assert!(!p.can_add());
        p.action(203);
        assert!(p.order.is_empty());
        p.show();
        p.append((0..2).map(|id| playlist::Track {
            id,
            path: "same.mp3".into(),
            tags: tags::Tags::default(),
            duration: None,
        }));
        p.action(200);
        assert_eq!(p.order.len(), 2);
        assert_ne!(p.order[0].id, p.order[1].id);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn mode_cancel_preserves_order_and_repeat_one_eof_restarts_once() {
        let (mut p, fake) = player();
        let ctx = context();
        p.expanded = true;
        p.start(ctx.now);
        p.action(10, &ctx);
        p.action(MODE_SHUFFLE, &ctx);
        p.action(MODE_ONE, &ctx);
        p.action(MODE_CANCEL, &ctx);
        assert_eq!(p.playback.mode, modes::Mode::default());
        p.action(10, &ctx);
        p.action(MODE_ONE, &ctx);
        p.action(MODE_OK, &ctx);
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
            draft: false,
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
            draft: false,
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
    #[cfg(target_os = "macos")]
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
        p.playlist.action(playlist::OK);
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
    #[cfg(target_os = "macos")]
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
        assert_eq!(p.request.generation, generation);
        assert!(p.notice.contains("Finish"));
    }
    #[cfg(target_os = "macos")]
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
