//! Nearby DN transfers: session-only UI state over an explicitly enabled LAN service.
pub(crate) mod lan;
pub mod manifest;
pub(crate) mod service;
pub(crate) mod session;
pub(crate) mod wire;

use crate::{
    app::App,
    plugins::{Binding, Owner, NEARBY_TRANSFERS},
    transfers,
};
use service::{Command, Event, Peer, Service};
use session::Session;
use std::{io, path::PathBuf, sync::Arc};
#[derive(Clone, Default)]
pub(crate) struct Snapshot {
    pub receiving: bool,
    pub notice: String,
    pub details: String,
    pub peers: Vec<Peer>,
    pub pairing: Option<(String, String)>,
    pub offer: Vec<String>,
    pub count: usize,
    pub bytes: u64,
    pub selected: usize,
    pub busy: bool,
}
#[derive(Default)]
pub(crate) struct State {
    service: Option<Service>,
    requests: crate::requests::RequestScope,
    preparing: Option<crate::task::Task<Result<manifest::Selection, String>>>,
    owner: Option<Binding>,
    generation: u64,
    pub view: Snapshot,
    confirmation: Option<tokio::sync::oneshot::Sender<bool>>,
    offer: Option<Arc<Session>>,
    selection: Option<manifest::Selection>,
    origin: Option<transfers::Origin>,
    base: Option<PathBuf>,
    pending_send: bool,
}
impl State {
    fn stop(&mut self) {
        if let Some(service) = &self.service {
            let _ = service.command(Command::Stop);
        }
        self.confirmation.take();
        if let Some(offer) = self.offer.take() {
            offer.abort();
        }
        self.owner = None;
        self.pending_send = false;
        self.preparing = None;
        self.view = Snapshot::default();
        self.selection = None;
        self.origin = None;
    }
    fn command(&mut self, command: Command) -> io::Result<()> {
        if self.service.as_ref().is_some_and(|s| !s.alive()) {
            self.service = None;
        }
        if self.service.is_none() {
            self.service = Some(Service::start()?);
        }
        self.service.as_ref().unwrap().command(command)
    }
}
pub const HELP: &str = "Nearby transfers send selected real files and folders directly between DN instances on the same LAN.\nReceive advertises this instance only while its window remains open.\nChoose panel selection, select a nearby receiver, then Send. Manual connection details work when multicast is blocked.\nCompare all eight verification digits on both screens before confirming. Device names alone are not trusted. Pairing lasts for this DN session only.\nThe receiver reviews the offer and chooses a local directory. Existing files use the normal replacement prompts; folders merge under Ask.\nTransfer Center owns accepted jobs: closing Nearby transfers stops new offers, not accepted transfers. Cancel a job in Transfer Center.\nNo public discovery, relay, internet probing, automatic acceptance, remote browsing or restart/resume is used. Links and special files are not transferred.";
pub(crate) fn opened(app: &mut App, binding: Binding) {
    if app.peer.owner == Some(binding) {
        return;
    }
    app.peer.stop();
    app.peer.generation = app.peer.generation.wrapping_add(1);
    app.peer.owner = Some(binding);
    if let Owner::Document(id) = binding.owner {
        app.peer.requests =
            crate::requests::RequestScope::new(crate::requests::Owner::Document(id));
    }
    app.peer.base =
        (!app.panels[app.active].is_virtual()).then(|| app.panels[app.active].path.clone());
    app.peer.view.notice = "Choose Receive or use the panel selection to send files.".into();
}
pub(crate) fn capture(app: &mut App) {
    if busy(app) {
        app.peer.view.notice = "Another peer session is active.".into();
        return;
    }
    if !crate::archive::require_native(app) {
        return;
    }
    let panel = &app.panels[app.active];
    if panel.loading || panel.only_filtered_marks() {
        app.peer.view.notice = "Wait for the listing, or clear filtered marks first.".into();
        return;
    }
    let paths = panel.targets();
    if paths.is_empty() {
        app.peer.view.notice = "Select real files or directories first".into();
        return;
    }
    app.peer.view.selected = paths.len();
    app.peer.origin = Some(transfers::capture_origin(app));
    app.peer.selection = None;
    match app.peer.requests.spawn("nearby-selection", move |cancel| {
        manifest::Selection::capture_cancellable(paths, &cancel).map_err(|e| e.to_string())
    }) {
        Ok(task) => {
            app.peer.preparing = Some(task);
            discover(app, false);
        }
        Err(e) => app.peer.view.notice = e.to_string(),
    }
}

pub(crate) fn discover(app: &mut App, receive: bool) {
    let command = Command::Discovery {
        generation: app.peer.generation,
        receive,
    };
    match app.peer.command(command) {
        Ok(()) => {
            app.peer.view.receiving = receive;
            app.peer.view.notice = "Starting local discovery…".into();
        }
        Err(e) => app.peer.view.notice = e.to_string(),
    }
}
pub(crate) fn send(app: &mut App, address: String) {
    if busy(app) {
        app.peer.view.notice = "Another peer session is active.".into();
        return;
    }
    let Some(selection) = app.peer.selection.clone() else {
        capture(app);
        return;
    };
    match app.peer.command(Command::Connect {
        generation: app.peer.generation,
        address,
        selection,
    }) {
        Ok(()) => {
            app.peer.pending_send = true;
            app.peer.view.notice = "Connecting to nearby DN…".into();
        }
        Err(e) => app.peer.view.notice = e.to_string(),
    }
}
pub(crate) fn confirm(app: &mut App, yes: bool) {
    if let Some(answer) = app.peer.confirmation.take() {
        let _ = answer.send(yes);
    }
    app.peer.view.pairing = None;
    app.peer.view.notice = if yes {
        "Waiting for the other DN to confirm…"
    } else {
        "Pairing was declined."
    }
    .into();
}
pub(crate) fn accept(app: &mut App, destination: PathBuf) {
    let result = (|| {
        let destination = if destination.is_absolute() {
            destination
        } else {
            app.peer
                .base
                .as_ref()
                .ok_or_else(|| io::Error::other("Choose an absolute local receive directory."))?
                .join(destination)
        };
        if crate::platform_fs::directory_link(&std::fs::symlink_metadata(&destination)?) {
            return Err(io::Error::other("Receive directory cannot be a link"));
        }
        let destination = std::fs::canonicalize(destination)?;
        let receipt = crate::archive::extract::Destination::capture(&destination)?;
        if !destination.is_dir() {
            return Err(io::Error::other(
                "Choose an existing local receive directory.",
            ));
        }
        let session = app
            .peer
            .offer
            .as_ref()
            .cloned()
            .ok_or_else(|| io::Error::other("The peer offer is no longer available."))?;
        let request = session::Transfer {
            session,
            destination: Some(destination),
            receipt: Some(receipt),
        };
        transfers::submit_peer(app, request, None, true).map_err(io::Error::other)?;
        Ok::<_, io::Error>(())
    })();
    match result {
        Ok(()) => {
            app.peer.offer = None;
            app.peer.view.offer.clear();
            app.peer.view.notice = "Receive job queued in Transfer Center.".into();
        }
        Err(e) => app.peer.view.notice = e.to_string(),
    }
}
pub(crate) fn decline(app: &mut App) {
    if let Some(session) = app.peer.offer.take() {
        session.abort();
    }
    app.peer.view.offer.clear();
    app.peer.view.notice = "Transfer offer declined.".into();
}
pub(crate) fn open_send(app: &mut App) {
    if !crate::archive::require_native(app) {
        return;
    }
    crate::plugins::host::open(app, NEARBY_TRANSFERS);
    if app.config.plugin_enabled(NEARBY_TRANSFERS) && app.peer.owner.is_some() {
        capture(app);
    }
}
pub(crate) fn poll(app: &mut App) -> bool {
    if !app.config.plugin_enabled(NEARBY_TRANSFERS) && app.peer.service.is_some() {
        app.peer.stop();
        app.peer.service = None;
        return true;
    }
    if app.peer.owner.is_some_and(|b| {
        app.desktop.plugins.binding(NEARBY_TRANSFERS) != Some(b)
            || !app.config.plugin_enabled(NEARBY_TRANSFERS)
    }) {
        app.peer.stop();
        if !app.config.plugin_enabled(NEARBY_TRANSFERS) {
            app.peer.service = None;
        }
        return true;
    }
    let mut changed = false;
    if let Some(result) = app
        .peer
        .preparing
        .as_ref()
        .and_then(|task| match task.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Source selection worker stopped.".into())),
        })
    {
        app.peer.preparing = None;
        changed = true;
        match result {
            Ok(selection) => {
                app.peer.selection = Some(selection);
                app.peer.view.notice = "Choose a nearby receiver, then Send.".into();
            }
            Err(message) => {
                app.peer.view.selected = 0;
                app.peer.origin = None;
                app.peer.view.notice = message;
            }
        }
    }
    while let Some(update) = app.peer.service.as_ref().and_then(Service::next_update) {
        if app.peer.owner.is_none()
            || update.generation != app.peer.generation && update.generation != 0
        {
            continue;
        }
        let previous_notice = app.peer.view.notice.clone();
        match update.event {
            Event::Info(message) => {
                // Discovery hints must not replace a failed source-selection notice.
                if app.peer.view.selected > 0 || app.peer.view.receiving {
                    app.peer.view.notice = message;
                }
            }
            Event::Failure(message) => {
                app.peer.pending_send = false;
                app.peer.confirmation = None;
                app.peer.view.pairing = None;
                app.peer.view.notice = message;
            }
            Event::Details(details) => {
                changed |= details != app.peer.view.details;
                app.peer.view.details = details;
            }
            Event::Found(peer) => {
                if let Some(old) = app
                    .peer
                    .view
                    .peers
                    .iter_mut()
                    .find(|p| p.address.id == peer.address.id)
                {
                    changed |= old.address != peer.address || old.name != peer.name;
                    *old = peer;
                } else if app.peer.view.peers.len() < 128 {
                    changed = true;
                    app.peer.view.peers.push(peer);
                    app.peer.view.peers.sort_by_key(|p| p.address.id);
                }
            }
            Event::Expired(id) => {
                let count = app.peer.view.peers.len();
                app.peer.view.peers.retain(|p| p.address.id != id);
                changed |= count != app.peer.view.peers.len();
            }
            Event::Pair { code, peer, answer } => {
                changed = true;
                app.peer.confirmation = Some(answer);
                app.peer.view.pairing = Some((code, peer));
            }
            Event::Ready(session) if session.sending() => {
                changed = true;
                app.peer.pending_send = false;
                let show = app
                    .peer
                    .owner
                    .is_some_and(|b| crate::plugins::host::active(app) == Some(b));
                let request = session::Transfer {
                    session,
                    destination: None,
                    receipt: None,
                };
                let origin = app.peer.origin.take();
                if let Err(e) = transfers::submit_peer(app, request, origin, show) {
                    app.peer.view.notice = e;
                } else {
                    app.peer.view.notice = "Send job queued in Transfer Center.".into();
                }
            }
            Event::Ready(session) => {
                changed = true;
                app.peer.view.count = session.manifest.entries.len();
                app.peer.view.bytes = session.manifest.total;
                app.peer.view.offer = session
                    .manifest
                    .entries
                    .iter()
                    .take(100)
                    .map(|e| e.path.clone())
                    .collect();
                app.peer.offer = Some(session);
                app.peer.view.notice =
                    "Review the offer and choose a local receive directory.".into();
            }
        }
        changed |= previous_notice != app.peer.view.notice;
    }
    if app.peer.offer.as_ref().is_some_and(|s| s.ended()) {
        decline(app);
        app.peer.view.notice = "The peer disconnected; the offer was closed.".into();
        changed = true;
    }
    let busy_now = busy(app);
    changed |= busy_now != app.peer.view.busy;
    app.peer.view.busy = busy_now;
    changed
}
fn busy(app: &App) -> bool {
    app.peer.preparing.is_some()
        || app.peer.service.as_ref().is_some_and(|s| s.busy())
        || app.peer.pending_send
        || app.peer.confirmation.is_some()
        || app.peer.offer.is_some()
        || app
            .transfers
            .entries
            .iter()
            .any(|e| !e.status.terminal() && matches!(e.request, transfers::Request::Peer(_)))
}

#[derive(Clone, Debug)]
pub(crate) enum Action {
    Receive(bool),
    Capture,
    Send(String),
    Confirm(bool),
    Accept(PathBuf),
    Decline,
    Tree,
}
pub(crate) fn base(app: &App) -> Option<PathBuf> {
    app.peer.base.clone()
}
pub(crate) fn active(app: &App) -> bool {
    app.peer.owner.is_some()
}
pub(crate) fn action(app: &mut App, binding: Binding, action: Action) {
    if binding.plugin != NEARBY_TRANSFERS || app.peer.owner != Some(binding) {
        return;
    }
    match action {
        Action::Receive(receive) => {
            if receive {
                discover(app, true);
            } else {
                if let Some(service) = &app.peer.service {
                    let _ = service.command(Command::Stop);
                }
                app.peer.view.receiving = false;
                app.peer.view.notice = "Receiving stopped.".into();
            }
        }
        Action::Capture => capture(app),
        Action::Send(address) => send(app, address),
        Action::Confirm(yes) => confirm(app, yes),
        Action::Accept(destination) => accept(app, destination),
        Action::Decline => decline(app),
        Action::Tree => {
            let Owner::Document(owner) = binding.owner else {
                return;
            };
            let path = app
                .peer
                .base
                .clone()
                .unwrap_or_else(|| app.panels[app.active].path.clone());
            match crate::directory_tree::Tree::new(app.active, path, app.config.show_hidden) {
                Ok(mut tree) => {
                    tree.destination_peer = Some((owner, app.peer.generation));
                    app.modals.push_peer_tree(tree, owner);
                    app.sync_modals();
                }
                Err(e) => app.peer.view.notice = e.to_string(),
            }
        }
    }
}
pub(crate) fn pick(app: &mut App, owner: crate::desktop::WindowId, generation: u64, path: PathBuf) {
    if app.peer.generation != generation {
        return;
    }
    if let Some(binding) = app.peer.owner.filter(|b| b.owner == Owner::Document(owner)) {
        crate::plugins::host::dispatch(app, binding, crate::plugins::Event::NearbyDirectory(path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_drawing_and_reopening_do_not_start_networking_or_accept_stale_actions(
    ) -> io::Result<()> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-peer-owner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _fixture = Fixture(path.clone());
        let mut app = App::new(path)?;
        assert!(app.config.plugin_enabled(NEARBY_TRANSFERS));
        assert!(app.peer.service.is_none());
        crate::plugins::host::open(&mut app, NEARBY_TRANSFERS);
        let binding = app.peer.owner.unwrap();
        let generation = app.peer.generation;
        assert!(app.peer.service.is_none());
        assert!(!app.peer.view.receiving);
        app.peer.stop();
        action(&mut app, binding, Action::Receive(true));
        assert!(app.peer.service.is_none());
        app.peer.generation = generation + 1;
        pick(
            &mut app,
            crate::desktop::WindowId::MANAGER,
            generation,
            PathBuf::from("stale"),
        );
        assert!(app.peer.service.is_none());
        Ok(())
    }
}
