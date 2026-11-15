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
    thread,
};
mod engine;
mod obex;
mod platform;
#[cfg(test)]
mod tests;
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
}
enum Reply {
    Devices(Vec<Device>),
    Sources(Vec<Source>),
    Transferred,
}
struct Job {
    owner: Binding,
    cancel: obex::Cancel,
    receive: mpsc::Receiver<io::Result<Reply>>,
    progress: Arc<Mutex<Progress>>,
    approvals: mpsc::SyncSender<(u64, bool, PathBuf)>,
    conflicts: crate::operations::conflict::Mailbox,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
#[derive(Default)]
pub struct State {
    pub view: Snapshot,
    queue: Vec<Source>,
    job: Option<Job>,
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
pub const HELP:&str="Bluetooth transfers uses standard OBEX Object Push over Bluetooth Classic, without Wi-Fi or internet.\nThe plugin is enabled by default; a saved disabled choice is preserved. Pair the computers in system Bluetooth settings, then open Bluetooth transfers in both DN instances. Refresh lists paired devices; it does not change discoverability or pair devices automatically.\nUse selected files captures visible marked native files, or the current file. Only regular files are supported: no folders, links, archive or remote members. The queue is session-only, up to 256 files, each at most 4 GiB minus one byte.\nStart Receive on the destination DN, select its paired device on the sender, then Send queue. Review each incoming filename and size and explicitly Accept or Decline. Device names do not prove identity; verify system pairing.\nChoose an existing local destination directory. Existing regular files use shared replacement confirmation; links and directory conflicts are refused. Incomplete files are staged privately and cleaned up on failure or Stop. Completed files remain.\nClosing this window or disabling the plugin stops listening and cancels active work. Switching documents leaves it running. There is no automatic retry or resume. A lost final acknowledgement means delivery is unconfirmed: check the destination before retrying.\nLinux requires BlueZ and its system D-Bus service, including in tty mode. macOS requires Bluetooth access for the terminal running DN. Windows requires a Bluetooth Classic adapter and its system drivers. BLE-only adapters are insufficient.\nDN uses a private discovery service identifier with standard OBEX framing, so this version connects only to another DN receiver. System/Android interoperability is a later milestone. Bluetooth transfers is separate from Nearby and Acoustic transfers. Android compatibility and actual hardware/platform reliability require separate testing; iPhone OBEX compatibility is not promised.";
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
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(Mutex::new(Progress::default()));
    let conflicts = crate::operations::conflict::Mailbox::default();
    let (approvals, receiver) = mpsc::sync_channel(1);
    let (send, receive) = mpsc::sync_channel(1);
    let context = engine::JobContext {
        cancel: cancel.clone(),
        progress: progress.clone(),
        approvals: receiver,
        conflicts: conflicts.clone(),
    };
    match thread::Builder::new()
        .name("dn-bluetooth".into())
        .spawn(move || {
            #[cfg(target_os = "macos")]
            let result = objc2::rc::autoreleasepool(|_| run(context));
            #[cfg(not(target_os = "macos"))]
            let result = run(context);
            let _ = send.send(result);
        }) {
        Ok(_) => {
            app.bluetooth.view.busy = true;
            app.bluetooth.view.notice.clear();
            app.bluetooth.view.progress = Default::default();
            app.bluetooth.view.generation = app.bluetooth.view.generation.wrapping_add(1);
            app.bluetooth.job = Some(Job {
                owner: b,
                cancel,
                receive,
                progress,
                approvals,
                conflicts,
            });
        }
        Err(e) => app.bluetooth.view.notice = e.to_string(),
    }
}
pub(crate) fn opened(app: &mut App, b: Binding) {
    app.bluetooth = State::default();
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
        Action::Stop => {
            if let Some(job) = &app.bluetooth.job {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
        Action::Approve {
            offer,
            accept,
            destination,
        } => {
            if app
                .bluetooth
                .view
                .progress
                .offer
                .as_ref()
                .is_some_and(|o| o.id == offer)
            {
                if let Some(j) = &app.bluetooth.job {
                    let _ = j.approvals.try_send((offer, accept, destination));
                }
            }
        }
        Action::Conflict(d) => {
            if let Some(j) = &app.bluetooth.job {
                if d == crate::operations::conflict::Decision::Cancel {
                    j.cancel.store(true, Ordering::Relaxed);
                }
                j.conflicts.answer(d);
            }
        }
        _ if app.bluetooth.view.busy => (),
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
            start(app, b, move |ctx| {
                ctx.update("Connecting Bluetooth device", 0, 0);
                let mut link = platform::connect(&device.address, &ctx.cancel)?;
                engine::send(link.as_mut(), &files, &ctx)?;
                Ok(Reply::Transferred)
            });
        }
        Action::Receive => start(app, b, |ctx| {
            ctx.update("Starting Bluetooth receiver", 0, 0);
            let mut listener = platform::Listener::new(&ctx.cancel)?;
            ctx.update("Waiting for Bluetooth sender", 0, 0);
            let (mut link, peer) = listener.accept(&ctx.cancel)?;
            engine::receive(link.as_mut(), &peer, &ctx)?;
            Ok(Reply::Transferred)
        }),
    }
}
pub(crate) fn poll(app: &mut App) -> bool {
    let Some(job) = &app.bluetooth.job else {
        return false;
    };
    if !alive(app, job.owner) {
        app.bluetooth.job = None;
        app.bluetooth.view.busy = false;
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
    match job.receive.try_recv() {
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
                    app.bluetooth.view.devices = devices;
                    app.bluetooth.view.notice = app
                        .config
                        .language
                        .text("Pair devices in system settings, then select a receiver")
                        .into();
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
