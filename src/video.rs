//! Host-owned Video Merge jobs. A plugin never starts a process or publishes a file.
use crate::{
    app::App,
    plugins::{Binding, Owner, VIDEO_MERGE},
};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
};
mod color;
mod engine;
pub(crate) fn encoder_name(name: &str) -> &'static str {
    engine::encoder_name(name)
}
mod timing;
use std::time::{Duration, Instant};
pub(crate) use timing::{media_time, time, Timing};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preferences {
    pub ffmpeg: String,
    pub ffprobe: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Codec {
    #[default]
    Auto,
    H264,
    H265,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    pub codec: Codec,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: String,
    pub quality: Option<u8>,
    pub preset: String,
    pub hardware: String,
    pub bitrate_kbps: u32,
    pub audio: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            codec: Codec::Auto,
            width: None,
            height: None,
            fps: String::new(),
            quality: None,
            preset: "medium".into(),
            hardware: String::new(),
            bitrate_kbps: 0,
            audio: true,
        }
    }
}
impl Settings {
    pub(crate) fn validate(&self) -> io::Result<()> {
        if self.width.is_some() != self.height.is_some()
            || self
                .width
                .is_some_and(|n| !(2..=16384).contains(&n) || !n.is_multiple_of(2))
            || self
                .height
                .is_some_and(|n| !(2..=16384).contains(&n) || !n.is_multiple_of(2))
            || (!self.fps.is_empty() && !engine::valid_rate(&self.fps))
            || self.quality.is_some_and(|n| n > 51)
            || !["fast", "medium", "slow"].contains(&self.preset.as_str())
            || (self.bitrate_kbps != 0 && !(100..=500_000).contains(&self.bitrate_kbps))
            || ![
                "",
                "h264_videotoolbox",
                "hevc_videotoolbox",
                "h264_nvenc",
                "hevc_nvenc",
                "h264_qsv",
                "hevc_qsv",
            ]
            .contains(&self.hardware.as_str())
        {
            return Err(io::Error::other("Invalid video settings: use even dimensions 2–16384, a positive FPS up to 1000, CRF 0–51 and bitrate 100–500000 kbps"));
        }
        if !self.hardware.is_empty()
            && ((self.codec == Codec::H265) != self.hardware.starts_with("hevc"))
        {
            return Err(io::Error::other(
                "Hardware encoder does not match the selected codec",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub sources: Vec<PathBuf>,
    pub output: PathBuf,
    pub recursive: bool,
    pub csv: bool,
    pub settings: Settings,
}
#[derive(Clone, Default)]
pub(crate) struct Snapshot {
    pub generation: u64,
    pub busy: bool,
    pub ready: bool,
    pub revision: u64,
    pub sources: Vec<PathBuf>,
    pub notice: String,
    pub summary: String,
    pub encoders: Vec<String>,
    pub conflict: Option<crate::operations::conflict::Conflict>,
    pub capabilities: Vec<engine::capabilities::Encoder>,
    pub checking: bool,
    pub discovery_error: String,
    pub timing: Option<Timing>,
}
pub(crate) enum Action {
    Analyze(Request),
    Discover(Preferences),
    CheckTools(Preferences),
    TestSpeed,
    Start,
    Stop,
    Invalidate,
    Tree,
    Preferences(Preferences),
    Conflict(crate::operations::conflict::Decision),
}
#[derive(Clone, Default)]
pub(crate) struct Progress {
    pub stage: &'static str,
    pub detail: String,
    pub processed: Option<f64>,
    pub clip: usize,
    pub clips: usize,
}
impl Progress {
    pub fn new(stage: &'static str, detail: String) -> Self {
        Self {
            stage,
            detail,
            ..Default::default()
        }
    }
}
impl From<&'static str> for Progress {
    fn from(stage: &'static str) -> Self {
        Self::new(stage, String::new())
    }
}
enum Reply {
    Prepared(Box<engine::Plan>),
    Finished(engine::Report),
    Speed(Vec<engine::SpeedResult>, f64),
}
struct Job {
    owner: Binding,
    generation: u64,
    worker: crate::workers::Worker<Reply>,
    progress: Arc<Mutex<Progress>>,
    conflicts: crate::operations::conflict::Mailbox,
}
#[derive(Default)]
pub(crate) struct State {
    pub view: Snapshot,
    plan: Option<engine::Plan>,
    job: Option<Job>,
    discovery: Option<Discovery>,
    requests: crate::requests::RequestScope,
    capabilities: Option<engine::Capabilities>,
    started: Option<Instant>,
    clock: Option<timing::Clock>,
}
struct Discovery {
    owner: Binding,
    worker: crate::workers::Worker<engine::Capabilities>,
}
fn discovery(app: &mut App, b: Binding, p: Preferences) {
    let cached = app.video.capabilities.clone();
    spawn_discovery(app, b, move |cancel| {
        engine::discover(&p, cached.as_ref(), cancel)
    });
}
fn spawn_discovery(
    app: &mut App,
    b: Binding,
    run: impl FnOnce(&AtomicBool) -> io::Result<engine::Capabilities> + Send + 'static,
) {
    app.video.discovery = None;
    app.video.view.checking = true;
    app.video.view.discovery_error.clear();
    let Owner::Document(owner) = b.owner else {
        return;
    };
    match app.workers.spawn(
        "dn-video-tools",
        &app.video.requests,
        crate::requests::Owner::Document(owner),
        "video-tools",
        move |cancel| run(&cancel.flag()),
    ) {
        Ok(worker) => app.video.discovery = Some(Discovery { owner: b, worker }),
        Err(e) => {
            app.video.view.checking = false;
            app.video.view.discovery_error = app.config.language.message(&e.to_string());
        }
    }
}
fn poll_discovery(app: &mut App) -> bool {
    let Some(d) = &app.video.discovery else {
        return false;
    };
    if !alive(app, d.owner) {
        app.video.discovery = None;
        app.video.view.checking = false;
        return true;
    }
    match d.worker.try_recv() {
        Ok(result) => {
            app.video.view.checking = false;
            app.video.discovery = None;
            match result {
                Ok(c) => {
                    app.video.view.encoders = c
                        .encoders
                        .iter()
                        .filter(|e| e.available())
                        .map(|e| e.name.clone())
                        .collect();
                    app.video.view.capabilities = c.encoders.clone();
                    app.video.capabilities = Some(c);
                }
                Err(e) => {
                    app.video.view.encoders.clear();
                    app.video.view.capabilities.clear();
                    app.video.capabilities = None;
                    app.video.view.discovery_error = app.config.language.message(&e.to_string());
                }
            }
            true
        }
        Err(mpsc::TryRecvError::Empty) => false,
        Err(_) => {
            app.video.discovery = None;
            app.video.view.checking = false;
            app.video.view.discovery_error = "Tool discovery stopped".into();
            true
        }
    }
}
fn alive(app: &App, b: Binding) -> bool {
    app.config.plugin_enabled(VIDEO_MERGE)
        && app.desktop.plugins.binding(VIDEO_MERGE) == Some(b)
        && matches!(b.owner,Owner::Document(id) if app.desktop.contains(id))
}
pub(crate) fn opened(app: &mut App) {
    app.video = State::default();
}
pub(crate) fn action(app: &mut App, b: Binding, generation: u64, action: Action) {
    if !alive(app, b) || generation != app.video.view.generation {
        return;
    }
    match action {
        Action::Discover(p) if !app.video.view.busy => discovery(app, b, p),
        Action::CheckTools(p) if !app.video.view.busy => {
            app.video.capabilities = None;
            discovery(app, b, p);
        }
        Action::Stop => {
            if let Some(j) = &app.video.job {
                j.worker.cancel();
            }
        }
        Action::Conflict(d) => {
            if let Some(j) = &app.video.job {
                j.conflicts.answer(d);
            }
        }
        Action::Invalidate if !app.video.view.busy => {
            app.video.plan = None;
            app.video.view.ready = false;
            app.video.view.summary.clear();
            app.video.view.notice = app
                .config
                .language
                .text("Analyze before starting a new merge")
                .into();
            app.video.view.generation = app.video.view.generation.wrapping_add(1);
        }
        Action::Preferences(p) if !app.video.view.busy => {
            if [&p.ffmpeg, &p.ffprobe]
                .iter()
                .any(|p| p.len() > 4096 || p.chars().any(char::is_control))
            {
                app.video.view.notice = "Invalid video executable path".into();
                return;
            }
            if app.config.video != p {
                app.video.discovery = None;
                app.video.capabilities = None;
                app.video.view.encoders.clear();
                app.video.view.capabilities.clear();
                app.video.view.checking = false;
            }
            app.config.video = p;
            app.video.plan = None;
            app.video.view.ready = false;
        }
        Action::Tree if !app.video.view.busy => {
            if let Owner::Document(owner) = b.owner {
                match crate::directory_tree::Tree::new(
                    app.active,
                    app.panels[app.active].path.clone(),
                    app.config.show_hidden,
                ) {
                    Ok(mut tree) => {
                        tree.destination_video = Some(owner);
                        app.modals.push_peer_tree(tree, owner);
                        app.sync_modals();
                    }
                    Err(e) => app.video.view.notice = e.to_string(),
                }
            }
        }
        Action::Analyze(request) if !app.video.view.busy => {
            app.video.plan = None;
            app.video.view.ready = false;
            let prefs = app.config.video.clone();
            let cached = app.video.capabilities.clone();
            spawn(app, b, move |cancel, _, progress| {
                engine::prepare_cached(request, prefs, cached, cancel, progress)
                    .map(|p| Reply::Prepared(Box::new(p)))
            });
        }
        Action::Start if !app.video.view.busy && app.video.view.ready => {
            if let Some(plan) = app.video.plan.take() {
                app.video.view.ready = false;
                app.video.started = Some(Instant::now());
                app.video.clock = Some(timing::Clock::new(
                    plan.clips.iter().map(|c| c.duration).sum(),
                ));
                app.video.view.timing = app.video.clock.as_ref().map(|c| c.view.clone());
                spawn(app, b, move |cancel, conflicts, progress| {
                    engine::execute(&plan, cancel, conflicts, progress).map(Reply::Finished)
                });
            }
        }
        Action::TestSpeed if !app.video.view.busy && app.video.view.ready => {
            if let Some(plan) = app.video.plan.clone() {
                let total = plan.clips.iter().map(|c| c.duration).sum();
                spawn(app, b, move |cancel, _, progress| {
                    engine::benchmark(&plan, cancel, progress).map(|r| Reply::Speed(r, total))
                });
            }
        }
        _ => (),
    }
}
fn spawn(
    app: &mut App,
    b: Binding,
    run: impl FnOnce(
            &AtomicBool,
            &crate::operations::conflict::Mailbox,
            &mut dyn FnMut(Progress),
        ) -> io::Result<Reply>
        + Send
        + 'static,
) {
    let progress = Arc::new(Mutex::new(Progress::default()));
    let conflicts = crate::operations::conflict::Mailbox::default();
    let (p, m) = (progress.clone(), conflicts.clone());
    let Owner::Document(owner) = b.owner else {
        return;
    };
    match app.workers.spawn(
        "dn-video",
        &app.video.requests,
        crate::requests::Owner::Document(owner),
        "video-job",
        move |cancel| {
            run(&cancel.flag(), &m, &mut |s| {
                *p.lock().unwrap_or_else(|e| e.into_inner()) = s;
            })
        },
    ) {
        Ok(worker) => {
            app.video.view.generation = app.video.view.generation.wrapping_add(1);
            app.video.job = Some(Job {
                generation: app.video.view.generation,
                owner: b,
                worker,
                progress,
                conflicts,
            });
            app.video.view.busy = true;
            app.video.view.notice = app.config.language.text("Preparing video job").into();
        }
        Err(e) => {
            app.video.view.notice = app.config.language.message(&e.to_string());
            if let (Some(mut c), Some(start)) = (app.video.clock.take(), app.video.started.take()) {
                c.finish(start.elapsed(), "Failed after");
                app.video.view.timing = Some(c.view);
            }
        }
    }
}
pub(crate) fn poll(app: &mut App) -> bool {
    let discovery_changed = poll_discovery(app);
    let Some(job) = &app.video.job else {
        return discovery_changed;
    };
    if !alive(app, job.owner) || job.generation != app.video.view.generation {
        app.video.job = None;
        app.video.view.busy = false;
        return true;
    }
    let progress = job.progress.lock().unwrap().clone();
    let notice = if progress.stage.is_empty() {
        String::new()
    } else {
        format!(
            "{} {}",
            app.config.language.text(progress.stage),
            progress.detail
        )
    };
    let conflict = job.conflicts.pending();
    let mut changed = discovery_changed;
    if let (Some(clock), Some(start)) = (&mut app.video.clock, app.video.started) {
        clock.update(&progress, start.elapsed(), conflict.is_some());
        let mut view = clock.view.clone();
        view.elapsed = Duration::from_secs(view.elapsed.as_secs());
        view.remaining = view.remaining.map(|d| Duration::from_secs(d.as_secs()));
        if app.video.view.timing.as_ref() != Some(&view) {
            app.video.view.timing = Some(view);
            changed = true;
        }
    }
    if !notice.is_empty() && notice != app.video.view.notice {
        app.video.view.notice = notice;
        changed = true;
    }
    if conflict.is_some() != app.video.view.conflict.is_some() {
        app.video.view.conflict = conflict;
        changed = true;
    }
    match job.worker.try_recv() {
        Ok(result) => {
            app.video.job = None;
            app.video.view.busy = false;
            app.video.view.conflict = None;
            app.video.view.generation = app.video.view.generation.wrapping_add(1);
            if let (Some(mut clock), Some(start)) =
                (app.video.clock.take(), app.video.started.take())
            {
                let outcome = match &result {
                    Ok(Reply::Finished(_)) => "Completed in",
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => "Stopped after",
                    _ => "Failed after",
                };
                clock.finish(start.elapsed(), outcome);
                app.video.view.timing = Some(clock.view);
            }
            match result {
                Ok(Reply::Prepared(plan)) => {
                    app.video.view.sources = plan.clips.iter().map(|c| c.path.clone()).collect();
                    app.video.view.revision = app.video.view.revision.wrapping_add(1);
                    app.video.view.encoders = plan.encoders.clone();
                    if let Some(c) = &plan.capabilities {
                        app.video.view.capabilities = c.encoders.clone();
                        app.video.capabilities = Some(c.clone());
                    }
                    let total = plan.clips.iter().map(|c| c.duration).sum::<f64>();
                    let quality = if plan.copy {
                        String::new()
                    } else if plan.encoder.starts_with("lib") {
                        format!(
                            " · CRF {} {}",
                            plan.request
                                .settings
                                .quality
                                .unwrap_or(if plan.encoder == "libx265" { 20 } else { 18 }),
                            plan.request.settings.preset
                        )
                    } else {
                        format!(
                            " · {:.1} Mbps",
                            f64::from(plan.request.settings.bitrate_kbps) / 1000.0
                        )
                    };
                    let size = if plan.copy {
                        String::new()
                    } else if plan.encoder.starts_with("lib") {
                        format!(" · {}", app.config.language.text("Size varies"))
                    } else {
                        format!(
                            " · ~{:.1} GB",
                            f64::from(plan.request.settings.bitrate_kbps) * 1000.0 * total / 8e9
                        )
                    };
                    let hdr = if !plan.copy && plan.clips.iter().any(|c| c.hdr) {
                        format!(" · {}", app.config.language.text("HDR → SDR"))
                    } else {
                        String::new()
                    };
                    app.video.view.summary = format!(
                        "{} · {}×{} · {} fps{quality}\n{} · {} {} · {} {}{size}{hdr}",
                        app.config.language.text(if plan.copy {
                            "Stream copy: no re-encoding"
                        } else {
                            engine::encoder_name(&plan.encoder)
                        }),
                        plan.width,
                        plan.height,
                        plan.fps,
                        media_time(total),
                        plan.clips.len(),
                        app.config.language.text("clips"),
                        plan.duplicates,
                        app.config.language.text("duplicates excluded")
                    );
                    app.video.view.notice = app
                        .config
                        .language
                        .text("Review settings and clip order, then Start")
                        .into();
                    if !plan.copy && plan.clips.iter().any(|c| c.hdr) {
                        app.video.view.notice = app
                            .config
                            .language
                            .text("HDR/10-bit will be converted to SDR. Ready to merge.")
                            .into();
                    }
                    if !plan.warning.is_empty() {
                        app.video.view.notice = format!(
                            "{}: {}",
                            app.config
                                .language
                                .text("Hardware unavailable for this output; using software"),
                            plan.warning
                        );
                    }
                    app.video.view.ready = plan.copy || plan.encoders.contains(&plan.encoder);
                    if !app.video.view.ready {
                        app.video.view.notice = format!(
                            "{}: {}",
                            app.config.language.text("Required encoder is unavailable"),
                            plan.encoder
                        );
                    }
                    app.video.plan = Some(*plan);
                }
                Ok(Reply::Speed(results, total)) => {
                    app.video.view.notice = results
                        .iter()
                        .map(|r| {
                            let label = app.config.language.text(engine::encoder_name(&r.encoder));
                            if let Some(speed) = r.speed {
                                format!(
                                    "{label}: {speed:.2}× · ~{} {}{}",
                                    time(Duration::from_secs_f64(
                                        (total / speed).min(365.0 * 86400.0)
                                    )),
                                    app.config.language.text("conversion only"),
                                    if r.incomplete {
                                        format!(
                                            " · {}",
                                            app.config.language.text("Incomplete sample")
                                        )
                                    } else {
                                        String::new()
                                    }
                                )
                            } else {
                                format!(
                                    "{label}: {}",
                                    r.reason.chars().take(160).collect::<String>()
                                )
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                }
                Ok(Reply::Finished(message)) => {
                    app.video.view.notice = format!(
                        "{} {}",
                        app.config.language.text("Video published:"),
                        display(&message.path)
                    );
                    for (key, cause) in message.warnings {
                        app.video.view.notice.push_str(&format!(
                            "\n{}{}",
                            app.config.language.text(key),
                            if cause.is_empty() {
                                String::new()
                            } else {
                                format!(": {cause}")
                            }
                        ));
                    }
                }
                Err(e) => {
                    app.video.view.notice = if e.kind() == io::ErrorKind::Interrupted {
                        app.config
                            .language
                            .text("Cancelled; completed publications remain")
                            .into()
                    } else {
                        format!("{}: {e}", app.config.language.text("Video job stopped"))
                    };
                }
            }
            true
        }
        Err(mpsc::TryRecvError::Disconnected) => {
            app.video.job = None;
            app.video.view.busy = false;
            app.video.view.conflict = None;
            app.video.view.generation = app.video.view.generation.wrapping_add(1);
            if let (Some(mut c), Some(start)) = (app.video.clock.take(), app.video.started.take()) {
                c.finish(start.elapsed(), "Failed after");
                app.video.view.timing = Some(c.view);
            }
            app.video.view.notice = app
                .config
                .language
                .text("Video worker stopped unexpectedly")
                .into();
            true
        }
        Err(mpsc::TryRecvError::Empty) => changed,
    }
}
pub(crate) fn picked(app: &mut App, owner: crate::desktop::WindowId, path: PathBuf) {
    let b = Binding {
        plugin: VIDEO_MERGE,
        owner: Owner::Document(owner),
    };
    if alive(app, b) {
        crate::plugins::host::dispatch(app, b, crate::plugins::Event::VideoFolder(path));
    }
}
pub(crate) fn current(app: &mut App) {
    if !crate::archive::require_native(app) {
        return;
    }
    let sources = app.panels[app.active].targets();
    let base = app.panels[app.active].path.clone();
    crate::plugins::host::open(app, VIDEO_MERGE);
    if let Some(b) = app.desktop.plugins.binding(VIDEO_MERGE) {
        crate::plugins::host::dispatch(
            app,
            b,
            crate::plugins::Event::VideoSources { sources, base },
        );
    }
}
pub(crate) fn display(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::ActionId;
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn retired_worker_does_not_block_ui_or_update_reopened_video() -> io::Result<()> {
        let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let mut app = App::new(work.root.clone())?;
        app.config = crate::config::Config::default();
        app.config.save_on_exit = false;
        app.config_store = crate::config::Store::new(Some(work.root.join("config.conf")));
        app.config.set_plugin_enabled(VIDEO_MERGE, true);
        crate::plugins::host::open(&mut app, VIDEO_MERGE);
        let owner = app.desktop.plugins.binding(VIDEO_MERGE).unwrap();
        let (started, ready) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let (continued, observed) = mpsc::sync_channel(1);
        let watchdog = std::thread::spawn(move || {
            let result = observed.recv_timeout(Duration::from_secs(3));
            let _ = release.send(());
            result
        });
        spawn(&mut app, owner, move |cancel, _, _| {
            started.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(cancel.load(Ordering::Relaxed));
            Err(io::ErrorKind::Interrupted.into())
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let Owner::Document(id) = owner.owner else {
            panic!("document");
        };
        app.desktop.close(id);
        app.poll();
        crate::plugins::host::open(&mut app, VIDEO_MERGE);
        let fresh = app.desktop.active().id;
        let notice = app.video.view.notice.clone();
        assert!(!app.poll_background_shutdown());
        let _ = continued.send(());
        assert!(
            watchdog.join().unwrap().is_ok(),
            "UI retirement waited for the worker's cleanup gate"
        );
        let end = Instant::now() + Duration::from_secs(5);
        while !app.poll_background_shutdown() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        app.poll();
        assert_ne!(id, fresh);
        assert_eq!(app.desktop.active().id, fresh);
        assert_eq!(app.video.view.notice, notice);
        Ok(())
    }
    #[test]
    fn replacing_discovery_does_not_wait_or_deliver_the_old_reply() -> io::Result<()> {
        let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let mut app = App::new(work.root.clone())?;
        app.config = crate::config::Config::default();
        app.config.save_on_exit = false;
        app.config_store = crate::config::Store::new(Some(work.root.join("config.conf")));
        app.config.set_plugin_enabled(VIDEO_MERGE, true);
        crate::plugins::host::open(&mut app, VIDEO_MERGE);
        let owner = app.desktop.plugins.binding(VIDEO_MERGE).unwrap();
        let (started, ready) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let (continued, observed) = mpsc::sync_channel(1);
        let watchdog = std::thread::spawn(move || {
            let result = observed.recv_timeout(Duration::from_secs(3));
            let _ = release.send(());
            result
        });
        spawn_discovery(&mut app, owner, move |cancel| {
            started.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(cancel.load(Ordering::Relaxed));
            Err(io::Error::other("retired discovery"))
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        spawn_discovery(&mut app, owner, |_| {
            Err(io::Error::other("current discovery"))
        });
        let _ = continued.send(());
        assert!(
            watchdog.join().unwrap().is_ok(),
            "Replacing discovery joined a running worker"
        );
        let end = Instant::now() + Duration::from_secs(5);
        while app.video.view.checking || app.workers.pending() {
            app.poll();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(app.video.view.discovery_error, "current discovery");
        Ok(())
    }
    #[test]
    fn inactive_worker_failure_retires_controls_without_stealing_focus() -> io::Result<()> {
        let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let mut app = App::new(work.root.clone())?;
        app.config = crate::config::Config::default();
        app.config.save_on_exit = false;
        app.config_store = crate::config::Store::new(Some(work.root.join("config.conf")));
        app.config.set_plugin_enabled(VIDEO_MERGE, true);
        crate::plugins::host::open(&mut app, VIDEO_MERGE);
        let owner = app.desktop.plugins.binding(VIDEO_MERGE).unwrap();
        let target = work.root.join("private.mp4");
        spawn(&mut app, owner, move |cancel, conflicts, _| {
            conflicts.ask_pumping(
                &crate::operations::conflict::Conflict {
                    source: PathBuf::new(),
                    source_label: Some("private output".into()),
                    destination: target,
                    destination_label: None,
                    source_size: 0,
                    destination_size: 0,
                    source_modified: None,
                    destination_modified: None,
                },
                cancel,
                || panic!("intentional video worker failure"),
            )?;
            unreachable!("the injected worker must fail")
        });
        let generation = app.video.view.generation;
        crate::utilities::open(&mut app, crate::utilities::Kind::Calculator);
        let active = app.desktop.active().id;
        let end = Instant::now() + Duration::from_secs(5);
        while app.video.view.busy {
            app.poll();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(app.video.view.conflict.is_none());
        assert_ne!(app.video.view.generation, generation);
        assert_eq!(app.desktop.active().id, active);
        assert!(app.video.view.notice.contains("worker stopped"));
        Ok(())
    }
    #[test]
    fn settings_discover_before_analysis_show_hardware_and_timing_frames_are_immutable(
    ) -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("dn-video-selection-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        let t = Temp(std::fs::canonicalize(root)?);
        let mut app = App::new(t.0.clone())?;
        app.config.save_on_exit = false;
        app.config.video.ffmpeg = t.0.join("missing-tool").to_string_lossy().into();
        app.config.set_plugin_enabled(VIDEO_MERGE, true);
        crate::plugins::host::open(&mut app, VIDEO_MERGE);
        let b = app.desktop.plugins.binding(VIDEO_MERGE).unwrap();
        crate::plugins::host::dispatch(&mut app, b, crate::plugins::Event::Action(ActionId(9)));
        assert!(app.video.view.checking);
        assert!(!app.video.view.ready);
        // Simulate discovery completing after the settings draft already exists.
        app.video.view.capabilities = vec![engine::capabilities::Encoder {
            name: "h264_videotoolbox".into(),
            reason: String::new(),
        }];

        crate::plugins::host::dispatch(&mut app, b, crate::plugins::Event::Action(ActionId(40)));
        let snap = crate::layout::prepare(
            &mut app,
            crate::presentation::layout::Rect::new(0, 0, 80, 25),
        );
        let frame = crate::testing::Buffer::rendered(crate::ui::render(&app, &snap));
        let text = frame.content.iter().map(|c| c.symbol()).collect::<String>();
        assert!(text.contains("Apple hardware H.264"));
        crate::plugins::host::dispatch(&mut app, b, crate::plugins::Event::Action(ActionId(503)));
        app.video.view.capabilities.clear();
        app.video.view.discovery_error = "Temporary tool failure".into();
        let snap = crate::layout::prepare(
            &mut app,
            crate::presentation::layout::Rect::new(0, 0, 80, 25),
        );
        let frame = crate::testing::Buffer::rendered(crate::ui::render(&app, &snap));
        let text = frame.content.iter().map(|c| c.symbol()).collect::<String>();
        assert!(text.contains("Apple hardware H.264"));

        crate::input::handle(
            &mut app,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        for language in [
            crate::localization::Language::English,
            crate::localization::Language::Russian,
        ] {
            app.config.language = language;
            app.video.view.timing = Some(Timing {
                elapsed: Duration::from_secs(61),
                remaining: Some(Duration::from_secs(120)),
                processed: 30.0,
                total: 100.0,
                stage: "Converting",
                clip: 2,
                clips: 3,
                ..Default::default()
            });
            for (w, h) in [(80, 25), (110, 35), (60, 18), (80, 25)] {
                let snap = crate::layout::prepare(
                    &mut app,
                    crate::presentation::layout::Rect::new(0, 0, w, h),
                );
                let frame = crate::testing::Buffer::rendered(crate::ui::render(&app, &snap));
                for _ in 0..3 {
                    assert_eq!(
                        frame,
                        crate::testing::Buffer::rendered(crate::ui::render(&app, &snap))
                    );
                }
                if w >= 80 {
                    let text = frame.content.iter().map(|c| c.symbol()).collect::<String>();
                    assert!(text.contains(language.text("Elapsed:")));
                    assert!(text.contains("00:01:01"));
                    assert!(text.contains("00:02:00"));
                }
            }
        }
        Ok(())
    }
    #[test]
    fn native_close_confirms_cancels_and_retired_results_cannot_update_a_new_owner(
    ) -> io::Result<()> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dn-video-owner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root)?;
        let t = Temp(std::fs::canonicalize(root)?);
        let mut app = App::new(t.0.clone())?;
        app.config.save_on_exit = false;
        app.config_store = crate::config::Store::new(Some(t.0.join("config.conf")));
        app.config.set_plugin_enabled(VIDEO_MERGE, true);
        crate::plugins::host::open(&mut app, VIDEO_MERGE);
        let b = app.desktop.plugins.binding(VIDEO_MERGE).unwrap();
        let Owner::Document(owner) = b.owner else {
            panic!("document plugin")
        };
        spawn(&mut app, b, |cancel, _, _| {
            while !cancel.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
        });
        assert!(crate::plugins::host::request_close(&mut app, owner));
        assert!(app.desktop.contains(owner));
        assert!(app.video.view.busy);
        let snap = crate::layout::prepare(
            &mut app,
            crate::presentation::layout::Rect::new(0, 0, 80, 25),
        );
        let frame = crate::ui::render(&app, &snap);
        let first = crate::testing::Buffer::rendered(frame);
        assert!(first
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .contains("Stop the active job?"));
        assert_eq!(
            first,
            crate::testing::Buffer::rendered(crate::ui::render(&app, &snap))
        );
        crate::plugins::host::dispatch(&mut app, b, crate::plugins::Event::Action(ActionId(81)));
        assert!(app.video.view.busy);
        assert!(crate::plugins::host::request_close(&mut app, owner));
        crate::input::handle(
            &mut app,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        crate::input::handle(
            &mut app,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.video.view.busy {
            poll(&mut app);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        crate::plugins::host::dispatch(&mut app, b, crate::plugins::Event::Action(ActionId(0)));
        assert!(!app.desktop.contains(owner));
        crate::plugins::host::open(&mut app, VIDEO_MERGE);
        let fresh = app.desktop.active().id;
        assert_ne!(fresh, owner);
        let fresh_notice = app.video.view.notice.clone();
        action(
            &mut app,
            b,
            0,
            Action::Analyze(Request {
                sources: vec![],
                output: t.0.join("stale.mp4"),
                recursive: false,
                csv: false,
                settings: Default::default(),
            }),
        );
        assert!(!app.video.view.busy);
        assert_eq!(app.video.view.notice, fresh_notice);
        assert_eq!(app.desktop.active().id, fresh);
        Ok(())
    }
}
