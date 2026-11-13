//! Nearby DN transfers: session-only UI state over an explicitly enabled LAN service.
pub(crate) mod discovery;
pub(crate) mod identity;
pub(crate) mod lan;
pub mod manifest;
pub(crate) mod service;
pub(crate) mod session;
pub(crate) mod text;
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
    pub network: discovery::Snapshot,
    pub local_name: String,
    pub local_id: String,
    pub test_pairing: bool,
    pub text_offer: Option<(String, usize)>,
    pub received_text: Option<text::Received>,
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
    identity: identity::Identity,
    service: Option<Service>,
    requests: crate::requests::RequestScope,
    preparing: Option<crate::task::Task<Result<manifest::Selection, String>>>,
    owner: Option<Binding>,
    generation: u64,
    network_request: u64,
    pub view: Snapshot,
    confirmation: Option<tokio::sync::oneshot::Sender<bool>>,
    offer: Option<Arc<Session>>,
    selection: Option<manifest::Selection>,
    origin: Option<transfers::Origin>,
    base: Option<PathBuf>,
    pending_send: bool,
    text_answer: Option<tokio::sync::oneshot::Sender<bool>>,
    text_guard: Option<Arc<service::Reservation>>,
    text_serial: u64,
}
impl State {
    fn stop(&mut self) {
        self.stop_network();
        self.confirmation.take();
        self.text_answer.take();
        self.text_guard.take();
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
    fn stop_network(&mut self) {
        self.network_request = self.network_request.wrapping_add(1);
        if let Some(service) = &self.service {
            let _ = service.command(Command::Stop {
                generation: self.generation,
                network: self.network_request,
            });
        }
        self.view.receiving = false;
        self.view.peers.clear();
        self.view.network = discovery::Snapshot {
            request: self.network_request,
            ready: self.view.network.ready,
            details: self.view.details.clone(),
            ..Default::default()
        };
    }
    fn network(&mut self, state: discovery::Snapshot) -> bool {
        if state.request != self.network_request || state.revision < self.view.network.revision {
            return false;
        }
        if self.view.network == state {
            return false;
        }
        self.view.details = state.details.clone();
        self.view.peers = state.peers.clone();
        if state.ready && self.view.notice == "Starting local discovery…" {
            self.view.notice.clear();
        }
        if !state.cause.is_empty() {
            self.view.notice = state.cause.clone();
        }
        self.view.network = state;
        true
    }
    fn command(&mut self, command: Command) -> io::Result<()> {
        if self.service.as_ref().is_some_and(|s| !s.alive()) {
            self.service = None;
        }
        if self.service.is_none() {
            self.service = Some(Service::start_named(
                self.identity.key.clone(),
                self.identity.name.clone(),
            )?);
        }
        self.service.as_ref().unwrap().command(command)
    }
}
pub const HELP: &str = "Nearby transfers send selected real files and folders directly between DN instances on the same LAN.\nReceive advertises this instance only while its window remains open.\nChoose panel selection, select a nearby receiver, then Send. Manual connection details work when multicast is blocked.\nCompare all eight verification digits on both screens before confirming. Device names alone are not trusted. Pairing lasts for this DN session only.\nThe receiver reviews the offer and chooses a local directory. Existing files use the normal replacement prompts; folders merge under Ask.\nTransfer Center owns accepted jobs: closing Nearby transfers stops new offers, not accepted transfers. Cancel a job in Transfer Center.\nNo public discovery, relay, internet probing, automatic acceptance, remote browsing or restart/resume is used. Links and special files are not transferred.\nRefresh rescans interfaces and restarts discovery without changing the selected files. New VM interfaces are bound automatically when no peer session is active; otherwise refresh waits until it finishes.\nManual details can contain several addresses. DN keeps eligible connected-LAN addresses and translates IPv6 scopes locally. Routed and NAT-only paths remain unsupported.\nNo announcements is not proof that multicast or a firewall is blocked.\nTest connection checks reachability and pairing without selecting or sending files. The other DN must have Receive enabled and support connection tests.\nEdit This DN and press Enter to apply a session-only nickname; blank restores the anonymous default. Device info shows the public identity and local interfaces/addresses. Nicknames never prove identity.\nSend text shares up to 64 KiB of Unicode text or links. Enter inserts a newline; Paste reads your clipboard and Send transmits only after pairing and receiver approval.\nThe receiver accepts the offer, previews the text, then chooses Copy or Dismiss. Nothing is copied or opened automatically. The console uses DN’s internal clipboard when a system clipboard is unavailable. Text drafts and previews are session-only; no files or transfer jobs are created.";
pub(crate) fn opened(app: &mut App, binding: Binding) {
    if app.peer.owner == Some(binding) {
        return;
    }
    app.peer.stop();
    app.peer.generation = app.peer.generation.wrapping_add(1);
    app.peer.owner = Some(binding);
    app.peer.view.local_name = app.peer.identity.name.clone();
    app.peer.view.local_id = app.peer.identity.key.public().to_string();
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
    app.peer.network_request = app.peer.network_request.wrapping_add(1);
    app.peer.view.network.request = app.peer.network_request;
    app.peer.view.network.revision = 0;
    app.peer.view.network.ready = false;
    app.peer.view.network.phase = discovery::Phase::Starting;
    let command = Command::Discovery {
        generation: app.peer.generation,
        network: app.peer.network_request,
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
    if !app.peer.view.network.ready
        || matches!(
            app.peer.view.network.phase,
            discovery::Phase::Starting | discovery::Phase::Pending
        )
    {
        app.peer.view.notice = "Wait for network refresh, then connect.".into();
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
                app.peer.view.text_offer = None;
                app.peer.text_answer.take();
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
            Event::TextOffer {
                peer,
                bytes,
                answer,
            } => {
                if app.peer.view.received_text.is_some() || app.peer.text_answer.is_some() {
                    let _ = answer.send(false);
                    continue;
                }
                app.peer.text_answer = Some(answer);
                app.peer.view.text_offer = Some((peer, bytes));
                changed = true;
            }
            Event::TextReceived {
                peer,
                text,
                stored,
                reservation,
            } => {
                if app.peer.view.received_text.is_some() {
                    let _ = stored.send(false);
                    continue;
                }
                app.peer.text_serial = app.peer.text_serial.wrapping_add(1);
                app.peer.view.received_text = Some(text::Received {
                    serial: app.peer.text_serial,
                    peer,
                    text,
                });
                app.peer.text_guard = Some(reservation);
                app.peer.view.text_offer = None;
                let _ = stored.send(true);
                changed = true;
            }
            Event::TextSent => {
                app.peer.pending_send = false;
                app.peer.view.notice =
                    "Text received by the other DN; clipboard copying is their choice.".into();
                changed = true;
            }

            Event::Failure(message) => {
                app.peer.view.text_offer = None;
                app.peer.text_answer.take();
                app.peer.pending_send = false;
                app.peer.confirmation = None;
                app.peer.view.pairing = None;
                app.peer.view.notice = message;
            }
            Event::ConnectFailure {
                text,
                testing,
                timed_out,
                cause,
            } => {
                app.peer.pending_send = false;
                let message = if text {
                    "Text connection failed; enable Receive and use a DN version supporting text sharing"
                } else if testing {
                    "Connection test failed; enable Receive and use a DN version supporting connection tests"
                } else if timed_out {
                    "Connection timed out; check Receive and the UDP path"
                } else {
                    "Could not connect to nearby DN"
                };
                app.peer.view.notice = format!("{message}: {cause}");
            }
            Event::Network(state) => changed |= app.peer.network(state),
            Event::Pair {
                code,
                peer,
                answer,
                testing,
            } => {
                app.peer.view.test_pairing = testing;
                changed = true;
                app.peer.confirmation = Some(answer);
                app.peer.view.pairing = Some((code, peer));
            }
            Event::Tested { peer } => {
                changed = true;
                app.peer.pending_send = false;
                app.peer.view.pairing = None;
                app.peer.confirmation = None;
                app.peer.view.test_pairing = false;
                app.peer.view.notice = app
                    .config
                    .language
                    .text("Connection verified with {}. No files were sent.")
                    .replace("{}", &peer);
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
    app.peer.view.received_text.is_some()
        || app.peer.text_answer.is_some()
        || app.peer.preparing.is_some()
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
    SendText { address: String, text: String },
    TextAccept(bool),
    DismissText,
    Receive(bool),
    Test(String),
    Nickname(String),
    Refresh,
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
        Action::SendText { address, text } => {
            if busy(app) {
                app.peer.view.notice = "Another peer session is active.".into();
                return;
            }
            if text.is_empty() || text.len() > wire::TEXT_LIMIT {
                app.peer.view.notice = "Text must contain 1–65536 UTF-8 bytes.".into();
                return;
            }
            if !app.peer.view.network.ready {
                let receive = app.peer.view.receiving;
                discover(app, receive);
            }
            match app.peer.command(Command::Text {
                generation: app.peer.generation,
                address,
                text,
            }) {
                Ok(()) => {
                    app.peer.pending_send = true;
                    app.peer.view.notice = "Sending text; waiting for approval…".into();
                }
                Err(error) => app.peer.view.notice = error.to_string(),
            }
        }
        Action::TextAccept(accept) => {
            if let Some(answer) = app.peer.text_answer.take() {
                let _ = answer.send(accept);
            }
            app.peer.view.text_offer = None;
            app.peer.view.notice = if accept {
                "Receiving approved text…"
            } else {
                "Text offer declined; no text was received."
            }
            .into();
        }
        Action::DismissText => {
            app.peer.view.received_text = None;
            app.peer.text_guard = None;
            // Publish the released preview and its command availability together.
            app.peer.view.busy = busy(app);
        }
        Action::Receive(receive) => {
            if receive {
                discover(app, true);
            } else {
                app.peer.stop_network();
                app.peer.view.notice = "Receiving stopped.".into();
            }
        }
        Action::Nickname(value) => {
            if busy(app) {
                app.peer.view.notice = "Finish the peer session before changing its name.".into();
                return;
            }
            match identity::validate(&value) {
                Ok(value) => {
                    let name = if value.is_empty() {
                        identity::anonymous(app.peer.identity.key.public())
                    } else {
                        value
                    };
                    if let Some(service) = &app.peer.service {
                        if let Err(error) = service.command(Command::Rename(name.clone())) {
                            app.peer.view.notice = error.to_string();
                            return;
                        }
                    }
                    app.peer.identity.name = name.clone();
                    app.peer.view.local_name = name;
                    app.peer.view.notice =
                        "Session nickname applied. It is not saved to configuration.".into();
                }
                Err(error) => app.peer.view.notice = error.to_string(),
            }
        }
        Action::Test(address) => {
            if busy(app) {
                app.peer.view.notice = "Another peer session is active.".into();
                return;
            }
            if !app.peer.view.network.ready {
                let receive = app.peer.view.receiving;
                discover(app, receive);
            }
            match app.peer.command(Command::Test {
                generation: app.peer.generation,
                address,
            }) {
                Ok(()) => {
                    app.peer.pending_send = true;
                    app.peer.view.notice = "Testing connection; no files will be sent.".into();
                }
                Err(error) => app.peer.view.notice = error.to_string(),
            }
        }
        Action::Refresh => {
            let receive = app.peer.view.receiving;
            discover(app, receive);
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
        let public_id = app.peer.view.local_id.clone();
        action(&mut app, binding, Action::Nickname("Лаптоп".into()));
        assert_eq!(app.peer.view.local_name, "Лаптоп");
        assert_eq!(app.peer.identity.name, "Лаптоп");
        assert!(
            app.peer.service.is_none(),
            "A nickname must not start networking"
        );
        app.peer.stop();
        opened(&mut app, binding);
        assert_eq!(app.peer.view.local_name, "Лаптоп");
        assert_eq!(app.peer.view.local_id, public_id);
        action(&mut app, binding, Action::Nickname("bad\nname".into()));
        assert_eq!(app.peer.identity.name, "Лаптоп");
        action(&mut app, binding, Action::Nickname(String::new()));
        assert_eq!(
            app.peer.view.local_name,
            identity::anonymous(app.peer.identity.key.public())
        );
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
    #[test]
    fn stale_refreshes_cannot_replace_current_network_or_captured_selection() {
        let mut state = State {
            network_request: 7,
            ..Default::default()
        };
        state.view.selected = 3;
        let current = discovery::Snapshot {
            request: 7,
            revision: 2,
            details: "current".into(),
            ..Default::default()
        };
        assert!(state.network(current.clone()));
        assert!(!state.network(discovery::Snapshot {
            request: 6,
            revision: 99,
            details: "stale request".into(),
            ..Default::default()
        }));
        assert!(!state.network(discovery::Snapshot {
            request: 7,
            revision: 1,
            details: "stale binding".into(),
            ..Default::default()
        }));
        assert_eq!(state.view.details, "current");
        assert_eq!(state.view.selected, 3);
        assert!(!state.network(current));
        state.stop_network();
        assert!(!state.network(discovery::Snapshot {
            request: 7,
            revision: 100,
            ..Default::default()
        }));
    }
}
