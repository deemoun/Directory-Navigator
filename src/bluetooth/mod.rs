//! Host-owned Bluetooth Object Push. Devices and files are touched only by workers.
use crate::{
    app::App,
    plugins::{Binding, Owner, BLUETOOTH_TRANSFERS},
};
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
};
mod engine;
mod obex;
mod platform;
#[cfg(test)]
mod tests;
pub(crate) mod transfer;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub address: String,
}
#[derive(Clone)]
pub struct Source {
    pub path: PathBuf,
    pub name: String,
    pub metadata: fs::Metadata,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub peer: String,
}
#[derive(Default, Clone, PartialEq, Eq)]
pub struct Progress {
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub completed: usize,
    pub skipped: usize,
    pub offer: Option<Offer>,
    pub warning: String,
    pub bytes: u64,
    pub destinations: Vec<PathBuf>,
    pub member: Option<String>,
    pub operation: crate::operations::Phase,
}
#[derive(Default, Clone)]
pub struct Snapshot {
    pub generation: u64,
    pub busy: bool,
    pub notice: String,
    pub devices: Vec<Device>,
    pub queue: Vec<String>,
    pub destination: PathBuf,
    pub progress: Progress,
    pub conflict: Option<crate::operations::conflict::Conflict>,
    pub transfer: Option<crate::transfers::TransferId>,
}
enum Reply {
    Devices(Vec<Device>),
    Sources(Vec<Source>),
    Transferred,
}
struct Job {
    owner: Binding,
    worker: crate::workers::Worker<Reply>,
    progress: Arc<Mutex<Progress>>,
    approvals: mpsc::SyncSender<(u64, bool, PathBuf)>,
    conflicts: crate::operations::conflict::Mailbox,
    flow: transfer::Flow,
    receiving: bool,
}
impl Job {
    fn answer_offer(&self, id: u64, accept: bool, path: PathBuf) -> io::Result<()> {
        if !self.flow.open() {
            return Err(io::Error::other("Bluetooth offer is no longer pending"));
        }
        let mut progress = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        if progress.offer.as_ref().is_none_or(|o| o.id != id) {
            return Err(io::Error::other("Bluetooth offer is no longer pending"));
        }
        let previous = self.flow.approved.load(Ordering::Acquire);
        if accept {
            self.flow.approved.store(id, Ordering::Release);
        }
        if let Err(error) = self.approvals.try_send((id, accept, path.clone())) {
            self.flow.approved.store(previous, Ordering::Release);
            return Err(io::Error::other(error));
        }
        if accept && progress.destinations.len() < 256 && !progress.destinations.contains(&path) {
            progress.destinations.push(path);
        }
        progress.offer = None;
        Ok(())
    }
}
#[derive(Default)]
pub struct State {
    pub view: Snapshot,
    queue: Vec<Source>,
    job: Option<Job>,
    requests: crate::requests::RequestScope,
}
pub enum Action {
    Refresh,
    Capture,
    Remove(usize),
    Send(usize),
    Receive,
    Stop,
    Approve {
        offer: u64,
        accept: bool,
        destination: PathBuf,
    },
    Conflict(crate::operations::conflict::Decision),
}
pub const HELP:&str="Bluetooth transfers uses standard OBEX Object Push over Bluetooth Classic, without Wi-Fi or internet.\nThe plugin is enabled by default; a saved disabled choice is preserved. Pair the computers in system Bluetooth settings, then open Bluetooth transfers in both DN instances. Refresh lists paired devices; it does not change discoverability or pair devices automatically.\nUse selected files captures visible marked native files, or the current file. Only regular files are supported: no folders, links, archive or remote members. The queue is session-only, up to 256 files, each at most 4 GiB minus one byte.\nStart Receive on the destination DN, select its paired device on the sender, then Send queue. Review each incoming filename and size and explicitly Accept or Decline. Device names do not prove identity; verify system pairing.\nChoose an existing local destination directory. Existing regular files use shared replacement confirmation; links and directory conflicts are refused. Incomplete files are staged privately and cleaned up on failure or Stop. Completed files remain.\nSend queue and accepted incoming files run through Transfer Center. Closing this window or disabling the plugin stops new offers; accepted work continues. Use Transfers to open Transfer Center for progress, cancellation and replacement decisions. Incoming approval remains in this window. Waiting in the queue shares the receiver decision timeout. There is no automatic retry or resume. A lost final acknowledgement means delivery is unconfirmed: check the destination before retrying.\nLinux requires BlueZ and its system D-Bus service, including in tty mode. macOS requires Bluetooth access for the terminal running DN. Windows requires a Bluetooth Classic adapter and its system drivers. BLE-only adapters are insufficient.\nDN uses a private discovery service identifier with standard OBEX framing, so this version connects only to another DN receiver. System/Android interoperability is a later milestone. Bluetooth transfers is separate from Nearby and Acoustic transfers. Android compatibility and actual hardware/platform reliability require separate testing; iPhone OBEX compatibility is not promised.";
fn alive(app: &App, b: Binding) -> bool {
    app.config.plugin_enabled(BLUETOOTH_TRANSFERS)
        && app.desktop.plugins.binding(BLUETOOTH_TRANSFERS) == Some(b)
        && matches!(b.owner,Owner::Document(id) if app.desktop.contains(id))
}
fn regular(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::other(
            "Bluetooth sources must be regular files, not links",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other(
                "Bluetooth sources cannot be Windows reparse points",
            ));
        }
    }
    Ok(())
}
fn capture(paths: Vec<PathBuf>, cancel: &AtomicBool) -> io::Result<Vec<Source>> {
    if paths.is_empty() || paths.len() > 256 {
        return Err(io::Error::other("Select 1–256 regular files for Bluetooth"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut sources = Vec::new();
    for path in paths {
        obex::check(cancel)?;
        if !seen.insert(path.clone()) {
            continue;
        }
        let file = crate::filesystem::open_regular(&path)?;
        let metadata = file.metadata()?;
        regular(&metadata)?;
        if metadata.len() > obex::MAX_FILE {
            return Err(io::Error::other(
                "Bluetooth OBEX files must be smaller than 4 GiB",
            ));
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                io::Error::other("Bluetooth filenames must be representable as Unicode")
            })?
            .to_owned();
        obex::filename(&name)?;
        if !crate::platform_fs::owns_file(&path, &file) {
            return Err(io::Error::other("Bluetooth source changed during capture"));
        }
        sources.push(Source {
            path,
            name,
            metadata,
        });
    }
    Ok(sources)
}
fn start(
    app: &mut App,
    b: Binding,
    run: impl FnOnce(engine::JobContext) -> io::Result<Reply> + Send + 'static,
) {
    if app.bluetooth.job.is_some() {
        return;
    }
    match make_job(app, b, false, false, run) {
        Ok(job) => {
            app.bluetooth.view.busy = true;
            app.bluetooth.view.notice.clear();
            app.bluetooth.view.progress = Default::default();
            app.bluetooth.view.generation = app.bluetooth.view.generation.wrapping_add(1);
            app.bluetooth.job = Some(job);
        }
        Err(e) => app.bluetooth.view.notice = app.config.language.message(&e.to_string()),
    }
}
fn make_job(
    app: &mut App,
    b: Binding,
    workspace: bool,
    receiving: bool,
    run: impl FnOnce(engine::JobContext) -> io::Result<Reply> + Send + 'static,
) -> io::Result<Job> {
    let progress = Arc::new(Mutex::new(Progress::default()));
    let conflicts = crate::operations::conflict::Mailbox::default();
    let flow = if receiving {
        transfer::Flow::receiver()
    } else {
        transfer::Flow::default()
    };
    let (approvals, receiver) = mpsc::sync_channel(1);
    let (p, m, f) = (progress.clone(), conflicts.clone(), flow.clone());
    let Owner::Document(owner) = b.owner else {
        return Err(io::Error::other("Bluetooth requires a document owner"));
    };
    let scope = if workspace {
        &app.transfers.requests
    } else {
        &app.bluetooth.requests
    };
    let owner = if workspace {
        crate::requests::Owner::Workspace
    } else {
        crate::requests::Owner::Document(owner)
    };
    let worker = app.workers.spawn(
        "dn-bluetooth",
        scope,
        owner,
        if workspace {
            "bluetooth-session"
        } else {
            "bluetooth-control"
        },
        move |cancel| {
            let ctx = engine::JobContext {
                cancel: cancel.flag(),
                progress: p,
                approvals: receiver,
                conflicts: m,
                flow: f,
            };
            #[cfg(target_os = "macos")]
            let r = objc2::rc::autoreleasepool(|_| run(ctx));
            #[cfg(not(target_os = "macos"))]
            let r = run(ctx);
            r
        },
    )?;
    Ok(Job {
        owner: b,
        worker,
        progress,
        approvals,
        conflicts,
        flow,
        receiving,
    })
}
pub(crate) fn opened(app: &mut App, b: Binding) {
    app.bluetooth = State::default();
    if app.transfers.bluetooth_pending().is_some() {
        return;
    }
    if !app.panels[app.active].is_virtual() {
        app.bluetooth.view.destination = app.panels[app.active].path.clone();
        if !app.panels[app.active].selected.is_empty()
            || app.panels[app.active]
                .entries
                .get(app.panels[app.active].cursor)
                .is_some_and(|e| !e.parent && !e.directory && !e.symlink)
        {
            action(app, b, 0, Action::Capture);
        }
    }
}
pub(crate) fn action(app: &mut App, b: Binding, generation: u64, action: Action) {
    if !alive(app, b) || generation != app.bluetooth.view.generation {
        return;
    }
    match action {
        Action::Stop if app.transfers.bluetooth_pending().is_some() => {
            app.transfers.selected = app.transfers.bluetooth_pending();
            crate::transfers::open(app);
        }
        Action::Stop => {
            if let Some(job) = &app.bluetooth.job {
                job.worker.cancel();
            }
        }
        Action::Approve {
            offer,
            accept,
            destination,
        } => {
            let current = app
                .bluetooth
                .view
                .progress
                .offer
                .as_ref()
                .is_some_and(|o| o.id == offer);
            if !current {
                return;
            }
            if let Some(job) = &app.transfers.bluetooth {
                if !job.belongs_to(b) {
                    return;
                }
                if let Err(e) = job.answer(offer, accept, destination) {
                    app.bluetooth.view.notice = e.to_string();
                }
            } else if accept && app.bluetooth.job.as_ref().is_some_and(|j| j.receiving) {
                let offered = app.bluetooth.view.progress.offer.clone().unwrap();
                let request = transfer::Transfer {
                    direction: transfer::Direction::Receive,
                    device: Device {
                        name: offered.peer.clone(),
                        address: String::new(),
                    },
                    files: vec![],
                    name: offered.name,
                    destination: destination.clone(),
                };
                match crate::transfers::submit_bluetooth(app, request, false) {
                    Ok(id) => {
                        let job = app.bluetooth.job.take().unwrap();
                        let job = transfer::Job::adopt(id, job, None);
                        if let Err(e) = job.approve(offer, destination) {
                            job.cancel();
                            app.bluetooth.view.notice = e.to_string();
                        }
                        app.transfers.bluetooth = Some(job);
                        app.bluetooth.view.progress.offer = None;
                        app.bluetooth.view.generation =
                            app.bluetooth.view.generation.wrapping_add(1);
                    }
                    Err(e) => app.bluetooth.view.notice = e,
                }
            } else if let Some(job) = &app.bluetooth.job {
                if let Err(e) = job.answer_offer(offer, accept, destination) {
                    app.bluetooth.view.notice = e.to_string();
                }
            }
        }
        Action::Conflict(d) => {
            if let Some(j) = &app.bluetooth.job {
                if d == crate::operations::conflict::Decision::Cancel {
                    j.worker.cancel();
                }
                j.conflicts.answer(d);
            }
        }
        _ if app.bluetooth.view.busy || app.transfers.bluetooth_pending().is_some() => (),
        Action::Refresh => start(app, b, |ctx| {
            ctx.update("Reading paired Bluetooth devices", 0, 0);
            platform::devices(&ctx.cancel).map(Reply::Devices)
        }),
        Action::Capture => {
            let panel = &app.panels[app.active];
            if panel.is_virtual() || panel.loading {
                app.bluetooth.view.notice =
                    "Download or extract files to disk before Bluetooth transfer".into();
                return;
            }
            let paths = panel.targets();
            start(app, b, move |ctx| {
                capture(paths, &ctx.cancel).map(Reply::Sources)
            });
        }
        Action::Remove(i) => {
            if i < app.bluetooth.queue.len() {
                app.bluetooth.queue.remove(i);
                app.bluetooth.view.queue.remove(i);
            }
        }
        Action::Send(index) => {
            let Some(device) = app.bluetooth.view.devices.get(index).cloned() else {
                return;
            };
            let files = app.bluetooth.queue.clone();
            if files.is_empty() {
                app.bluetooth.view.notice = "Select regular files before sending".into();
                return;
            }
            let request = transfer::Transfer {
                direction: transfer::Direction::Send,
                device,
                files,
                name: String::new(),
                destination: PathBuf::new(),
            };
            match crate::transfers::submit_bluetooth(app, request, true) {
                Ok(id) => app.bluetooth.view.transfer = Some(id),
                Err(e) => app.bluetooth.view.notice = e,
            }
        }
        Action::Receive => {
            match make_job(app, b, true, true, |ctx| {
                ctx.waiting("Starting Bluetooth receiver", 0);
                let mut listener = platform::Listener::new(&ctx.cancel)?;
                ctx.waiting("Waiting for Bluetooth sender", 0);
                let (mut link, peer) = listener.accept(&ctx.cancel)?;
                engine::receive(link.as_mut(), &peer, &ctx)?;
                Ok(Reply::Transferred)
            }) {
                Ok(job) => {
                    app.bluetooth.job = Some(job);
                    app.bluetooth.view.busy = true;
                    app.bluetooth.view.generation = app.bluetooth.view.generation.wrapping_add(1);
                }
                Err(e) => app.bluetooth.view.notice = e.to_string(),
            }
        }
    }
}
pub(crate) fn poll(app: &mut App) -> bool {
    if let Some(job) = &app.transfers.bluetooth {
        job.stop_offers_if_closed(app);
    }
    let pending = app.transfers.bluetooth_pending();
    let snapshot = app.transfers.bluetooth.as_ref().map(|j| j.snapshot());
    let transfer_changed = app.bluetooth.view.transfer != pending;
    app.bluetooth.view.transfer = pending;
    if app.bluetooth.job.is_none() {
        let changed = transfer_changed
            || app.bluetooth.view.busy != pending.is_some()
            || snapshot
                .as_ref()
                .is_some_and(|p| *p != app.bluetooth.view.progress);
        app.bluetooth.view.busy = pending.is_some();
        if let Some(p) = snapshot {
            app.bluetooth.view.progress = p;
        }
        app.bluetooth.view.conflict = None;
        if transfer_changed {
            app.bluetooth.view.generation = app.bluetooth.view.generation.wrapping_add(1);
        }
        return changed;
    }
    let Some(job) = &app.bluetooth.job else {
        return false;
    };
    if !alive(app, job.owner) {
        app.bluetooth.job = None;
        app.bluetooth.view.busy = false;
        app.bluetooth.view.conflict = None;
        app.bluetooth.view.progress.offer = None;
        app.bluetooth.view.generation = app.bluetooth.view.generation.wrapping_add(1);
        return true;
    }
    let progress = job
        .progress
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let conflict = job.conflicts.pending();
    let changed = progress != app.bluetooth.view.progress
        || conflict.is_some() != app.bluetooth.view.conflict.is_some();
    app.bluetooth.view.progress = progress;
    app.bluetooth.view.conflict = conflict;
    match job.worker.try_recv() {
        Ok(reply) => {
            // Completion may arrive after the snapshot taken above. Retain the
            // final counts/warnings from the same completed worker.
            app.bluetooth.view.progress = job
                .progress
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let p = &app.bluetooth.view.progress;
            let summary = format!(
                "{} {} · {} {}",
                p.completed,
                app.config.language.text("Completed"),
                p.skipped,
                app.config.language.text("Skipped")
            );
            match reply {
                Ok(Reply::Devices(devices)) => {
                    let message = if devices.is_empty() {
                        "No paired Bluetooth devices reported. Pair in system settings, then Refresh."
                    } else {
                        "Pair devices in system settings, then select a receiver"
                    };
                    app.bluetooth.view.devices = devices;
                    app.bluetooth.view.notice = app.config.language.text(message).into();
                }
                Ok(Reply::Sources(files)) => {
                    app.bluetooth.view.queue = files
                        .iter()
                        .map(|f| format!("{} · {} B", f.name, f.metadata.len()))
                        .collect();
                    app.bluetooth.queue = files;
                    app.bluetooth.view.notice =
                        app.config.language.text("Bluetooth queue captured").into();
                }
                Ok(Reply::Transferred) => {
                    app.bluetooth.view.notice = format!("{summary}\n{}", p.warning);
                    app.refresh();
                }
                Err(error) => {
                    app.bluetooth.view.notice = format!(
                        "{}: {}\n{summary}\n{}",
                        app.config
                            .language
                            .text(if error.kind() == io::ErrorKind::Interrupted {
                                "Bluetooth stopped"
                            } else {
                                "Bluetooth operation failed"
                            }),
                        error.to_string().chars().take(8192).collect::<String>(),
                        p.warning
                    );
                }
            }
            app.bluetooth.job = None;
            app.bluetooth.view.busy = false;
            app.bluetooth.view.conflict = None;
            app.bluetooth.view.progress.offer = None;
            app.bluetooth.view.generation = app.bluetooth.view.generation.wrapping_add(1);
            true
        }
        Err(mpsc::TryRecvError::Disconnected) => {
            app.bluetooth.job = None;
            app.bluetooth.view.busy = false;
            app.bluetooth.view.conflict = None;
            app.bluetooth.view.progress.offer = None;
            app.bluetooth.view.generation = app.bluetooth.view.generation.wrapping_add(1);
            app.bluetooth.view.notice = app.config.language.text("Bluetooth worker stopped").into();
            true
        }
        Err(mpsc::TryRecvError::Empty) => changed,
    }
}
