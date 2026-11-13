//! One lazy runtime owns the endpoint, discovery and short-lived pairing work.
use super::{
    lan::{self, Lan},
    manifest::{Manifest, Selection},
    session::Session,
    wire::{self, Message, ALPN},
};
use futures_util::StreamExt;
use iroh::{
    endpoint::{presets, IncomingAddr, NetReportConfig, PortmapperConfig, RelayMode},
    Endpoint, EndpointAddr, EndpointId,
};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use std::{
    collections::BTreeSet,
    io,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc as channel, oneshot};
#[derive(Clone, Debug)]
pub struct Peer {
    pub name: String,
    pub address: EndpointAddr,
}
pub enum Event {
    Info(String),
    Failure(String),
    Details(String),
    Found(Peer),
    Expired(EndpointId),
    Pair {
        code: String,
        peer: String,
        answer: oneshot::Sender<bool>,
    },
    Ready(Arc<Session>),
}
pub struct Update {
    pub generation: u64,
    pub event: Event,
}
#[derive(Clone)]
struct Updates {
    queue: mpsc::SyncSender<Update>,
    failure: Arc<Mutex<Option<Update>>>,
}
impl Updates {
    fn new(capacity: usize) -> (Self, mpsc::Receiver<Update>) {
        let (queue, receiver) = mpsc::sync_channel(capacity);
        (
            Self {
                queue,
                failure: Default::default(),
            },
            receiver,
        )
    }
}
pub enum Command {
    Discovery {
        generation: u64,
        receive: bool,
    },
    Connect {
        generation: u64,
        address: String,
        selection: Selection,
    },
    Stop,
}
pub struct Service {
    send: channel::Sender<Command>,
    pub events: mpsc::Receiver<Update>,
    failure: Arc<Mutex<Option<Update>>>,
    busy: Arc<AtomicBool>,
}
impl Service {
    pub fn start() -> io::Result<Self> {
        let (send, commands) = channel::channel(16);
        let busy = Arc::new(AtomicBool::new(false));
        let worker_busy = busy.clone();
        let (updates, events) = Updates::new(32);
        let failure = updates.failure.clone();
        std::thread::Builder::new()
            .name("dn-nearby".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = emit(&updates, 0, Event::Failure(e.to_string()));
                        return;
                    }
                };
                runtime.block_on(async move {
                    if let Err(e) = run(commands, updates.clone(), worker_busy).await {
                        let _ = emit(
                            &updates,
                            0,
                            Event::Failure(format!("Nearby transfers unavailable: {e}")),
                        );
                    }
                });
            })?;
        Ok(Self {
            send,
            events,
            busy,
            failure,
        })
    }
    pub fn next_update(&self) -> Option<Update> {
        self.events.try_recv().ok().or_else(|| {
            self.failure
                .lock()
                .ok()
                .and_then(|mut failure| failure.take())
        })
    }
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
    pub fn alive(&self) -> bool {
        !self.send.is_closed()
    }
    pub fn command(&self, command: Command) -> io::Result<()> {
        self.send
            .try_send(command)
            .map_err(|_| io::Error::other("Nearby service is busy or unavailable"))
    }
}
fn emit(updates: &Updates, generation: u64, event: Event) -> io::Result<()> {
    match updates.queue.try_send(Update { generation, event }) {
        Ok(()) => Ok(()),
        Err(mpsc::TrySendError::Full(update)) => {
            // A full bounded discovery queue must not leave pairing permanently
            // pending. Drop rejected offers and retain one final failure notice.
            let event = match update.event {
                Event::Failure(message) => Event::Failure(message),
                _ => Event::Failure("Nearby event queue is full; retry the connection.".into()),
            };
            if let Ok(mut failure) = updates.failure.lock() {
                *failure = Some(Update { generation, event });
            }
            Err(io::Error::other("Nearby event queue is full"))
        }
        Err(mpsc::TrySendError::Disconnected(_)) => {
            Err(io::Error::other("Nearby event queue is unavailable"))
        }
    }
}
pub(crate) struct Reservation {
    busy: Arc<AtomicBool>,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.busy.store(false, Ordering::Release);
    }
}
/// mDNS announces only; DN consumes and filters discovery events itself. QUIC
/// must never resolve additional unfiltered addresses while connecting to a card.
#[derive(Debug)]
struct LocalAdvertisement(MdnsAddressLookup);
impl iroh::address_lookup::AddressLookup for LocalAdvertisement {
    fn publish(&self, data: &iroh::address_lookup::EndpointData) {
        iroh::address_lookup::AddressLookup::publish(&self.0, data);
    }
}
pub(crate) async fn endpoint(
    lan: &Lan,
    bind: Option<std::net::SocketAddr>,
) -> io::Result<Endpoint> {
    let filter_lan = lan.clone();
    let mut builder = Endpoint::builder(presets::Minimal)
        .alpns(vec![ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .clear_relay_transports()
        .clear_address_lookup()
        .portmapper_config(PortmapperConfig::Disabled)
        .net_report_config(NetReportConfig::minimal())
        .max_tls_tickets(0)
        .transport_config(
            iroh::endpoint::QuicTransportConfig::builder()
                .max_concurrent_bidi_streams(1u32.into())
                .max_concurrent_uni_streams(0u32.into())
                .keep_alive_interval(Duration::from_secs(5))
                .build(),
        )
        .addr_filter(iroh::address_lookup::AddrFilter::new(move |addresses| {
            std::borrow::Cow::Owned(
                addresses
                    .iter()
                    .filter(|a| matches!(a, iroh::TransportAddr::Ip(ip) if filter_lan.allows(ip)))
                    .cloned()
                    .collect(),
            )
        }));
    if let Some(address) = bind {
        builder = builder
            .clear_ip_transports()
            .bind_addr(address)
            .map_err(wire::error)?;
    } else {
        let binds = lan.binds();
        if binds.is_empty() {
            return Err(wire::error("No connected LAN interface is available"));
        }
        builder = builder.clear_ip_transports();
        for (address, prefix) in binds {
            builder = builder
                .bind_addr_with_opts(
                    address,
                    iroh::endpoint::BindOpts::default()
                        .set_prefix_len(prefix)
                        .set_is_default_route(false)
                        .set_is_required(false),
                )
                .map_err(wire::error)?;
        }
    }
    builder.bind().await.map_err(wire::error)
}
async fn run(
    mut commands: channel::Receiver<Command>,
    updates: Updates,
    busy: Arc<AtomicBool>,
) -> io::Result<()> {
    let lan = Lan::capture();
    let endpoint = endpoint(&lan, None).await?;
    let name = format!("DN {}", &endpoint.id().to_string()[..8]);
    let receiving = Arc::new(AtomicBool::new(false));
    let generation = Arc::new(AtomicU64::new(0));
    let trusted = Arc::new(Mutex::new(BTreeSet::new()));
    let mut discovered: Option<
        std::pin::Pin<Box<dyn futures_util::Stream<Item = DiscoveryEvent> + Send>>,
    > = None;
    let mut mdns = None;
    let mut clock = tokio::time::interval(Duration::from_secs(1));
    let mut attempts = Vec::<Instant>::new();
    let mut attempt_cancel: Option<Arc<AtomicBool>> = None;
    let mut last_details = String::new();
    let mut discovery_started = None;
    let mut discovery_found = false;
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                None => break,
                Some(Command::Stop) => {
                    if let Some(cancel) = attempt_cancel.take() { cancel.store(true,Ordering::Release); }
                    receiving.store(false, Ordering::Release);
                    endpoint.address_lookup().map_err(wire::error)?.clear();
                    discovered = None; mdns = None; discovery_started=None;
                }
                Some(Command::Discovery { generation: epoch, receive }) => {
                    generation.store(epoch, Ordering::Release);
                    last_details.clear();
                    discovery_started=(!receive).then(Instant::now);discovery_found=false;
                    receiving.store(receive, Ordering::Release);
                    endpoint.address_lookup().map_err(wire::error)?.clear();
                    discovered = None; mdns = None;
                    let lookup = MdnsAddressLookup::builder().service_name("dn-transfer-v1").advertise(receive).build(endpoint.id());
                    match lookup {
                        Ok(lookup) => {
                            discovered = Some(Box::pin(lookup.subscribe().await));
                            endpoint.address_lookup().map_err(wire::error)?.add(LocalAdvertisement(lookup.clone()));
                            mdns = Some(lookup);
                            let _ = emit(&updates, epoch, Event::Info(if receive {"Receiving is enabled while this window is open."} else {"Looking for nearby DN receivers…"}.into()));
                        }
                        Err(e) => { let _ = emit(&updates, epoch, Event::Info(format!("LAN discovery unavailable; use manual connection details: {e}"))); }
                    }
                }
                Some(Command::Connect { generation: epoch, address, selection }) => {
                    discovery_started=None;
                    if busy.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err() { let _ = emit(&updates, epoch, Event::Failure("Another peer session is active.".into())); continue; }
                    let reservation = Reservation { busy: busy.clone() };
                    let cancel = Arc::new(AtomicBool::new(false)); attempt_cancel = Some(cancel.clone());
                    let ep = endpoint.clone(); let ui = updates.clone(); let networks = lan.clone(); let own_name = name.clone(); let trust = trusted.clone();
                    tokio::spawn(async move {
                        let result = async {
                            let addr = lan::parse(&address, &networks)?;
                            let connection = tokio::time::timeout(Duration::from_secs(10), ep.connect(addr, ALPN)).await.map_err(|e|wire::error(format!("Could not connect; check Receive, the LAN address and UDP firewall access: {e}")))?.map_err(|e|wire::error(format!("Could not connect; check Receive, the LAN address and UDP firewall access: {e}")))?;
                            pair(connection, ep.id(), own_name, Some(selection), reservation, networks, epoch, ui.clone(), trust, cancel).await
                        }.await;
                        if let Err(e) = result { let _ = emit(&ui, epoch, Event::Failure(e.to_string())); }
                    });
                }
            },
            incoming = endpoint.accept() => if let Some(incoming) = incoming {
                let epoch = generation.load(Ordering::Acquire);
                attempts.retain(|t| t.elapsed() < Duration::from_secs(60));
                if !receiving.load(Ordering::Acquire) || attempts.len() >= 8 || !matches!(incoming.remote_addr(), IncomingAddr::Ip(a) if lan.allows(&a)) {
                    incoming.refuse(); continue;
                }
                attempts.push(Instant::now());
                if busy.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err() {
                    tokio::spawn(async move {
                        let _ = tokio::time::timeout(Duration::from_secs(5), async move {
                            if let Ok(connection)=incoming.await {
                                if let Ok((mut send,_recv))=connection.accept_bi().await {
                                    let _=wire::send(&mut send,&Message::Busy).await;
                                    let _=send.finish(); let _=send.stopped().await;
                                }
                                connection.close(2u32.into(),b"Busy");
                            }
                        }).await;
                    });
                    continue;
                }
                let reservation = Reservation { busy: busy.clone() };
                let cancel = Arc::new(AtomicBool::new(false)); attempt_cancel = Some(cancel.clone());
                let ui = updates.clone(); let networks = lan.clone(); let own_name = name.clone(); let id = endpoint.id(); let trust = trusted.clone();
                tokio::spawn(async move {
                    let result = async {
                        let connection = tokio::time::timeout(Duration::from_secs(10), incoming).await.map_err(wire::error)?.map_err(wire::error)?;
                        pair(connection, id, own_name, None, reservation, networks, epoch, ui.clone(), trust, cancel).await
                    }.await;
                    if let Err(e) = result { let _ = emit(&ui, epoch, Event::Failure(e.to_string())); }
                });
            },
            event = async { match &mut discovered { Some(stream) => stream.next().await, None => std::future::pending().await } } => if let Some(event) = event {
                let epoch = generation.load(Ordering::Acquire);
                match event {
                    DiscoveryEvent::Discovered { endpoint_info, .. } => {
                        if endpoint_info.endpoint_id != endpoint.id() {
                            if let Ok(address) = lan.discovered(endpoint_info.into_endpoint_addr()) {
                                discovery_found=true;
                                let peer_name = format!("DN {}", &address.id.to_string()[..8]);
                                let _ = emit(&updates, epoch, Event::Found(Peer { name: peer_name, address }));
                            }
                        }
                    }
                    DiscoveryEvent::Expired { endpoint_id } => { let _ = emit(&updates, epoch, Event::Expired(endpoint_id)); }
                    _ => {}
                }
            },
            _ = clock.tick() => {
                lan.refresh();
                if !discovery_found && discovery_started.is_some_and(|started|started.elapsed()>=Duration::from_secs(5)) {
                    discovery_started=None;
                    let _=emit(&updates,generation.load(Ordering::Acquire),Event::Info("No receivers found; check Receive or use manual details if multicast is blocked.".into()));
                }
                if mdns.is_some() || receiving.load(Ordering::Acquire) {
                if let Ok(address) = lan.filter(endpoint.addr()) {
                    // Minimal endpoints need no internet reachability report. Publish
                    // the eligible bound addresses explicitly for offline discovery.
                    if let Some(lookup)=&mdns {
                        iroh::address_lookup::AddressLookup::publish(lookup,&iroh::address_lookup::EndpointData::from_iter(address.addrs.iter().cloned()));
                    }
                    let details = lan::card(&address);
                    if details != last_details { last_details = details.clone(); let _ = emit(&updates, generation.load(Ordering::Acquire), Event::Details(details)); }
                }
                }
            },
        }
    }
    endpoint.close().await;
    Ok(())
}
#[allow(clippy::too_many_arguments)]
async fn pair(
    connection: iroh::endpoint::Connection,
    own: EndpointId,
    name: String,
    selection: Option<Selection>,
    reservation: Reservation,
    lan: Lan,
    generation: u64,
    updates: Updates,
    trusted: Arc<Mutex<BTreeSet<EndpointId>>>,
    cancel: Arc<AtomicBool>,
) -> io::Result<()> {
    lan.connection(&connection)?;
    let pairing_started = Instant::now();
    let pairing_left = || Duration::from_secs(120).saturating_sub(pairing_started.elapsed());
    let outgoing = selection.is_some();
    let (mut send, mut recv) = if outgoing {
        connection.open_bi().await.map_err(wire::error)?
    } else {
        tokio::time::timeout(Duration::from_secs(10), connection.accept_bi())
            .await
            .map_err(wire::error)?
            .map_err(wire::error)?
    };
    let remote = connection.remote_id();
    let already_trusted = trusted.lock().map_err(wire::error)?.contains(&remote);
    wire::bounded(
        wire::send(
            &mut send,
            &Message::Hello {
                version: 1,
                sender: outgoing,
                name,
                trusted: already_trusted,
            },
        ),
        &cancel,
        Duration::from_secs(10),
    )
    .await?;
    let (peer_name, remote_trusted) =
        match wire::bounded(wire::receive(&mut recv), &cancel, Duration::from_secs(10)).await? {
            Message::Hello {
                version: 1,
                sender,
                name,
                trusted,
            } if sender != outgoing && name.len() <= 128 && !name.chars().any(char::is_control) => {
                (name, trusted)
            }
            Message::Busy => return Err(wire::error("Nearby receiver is busy.")),
            _ => return Err(wire::error("Incompatible DN peer protocol")),
        };
    if !(already_trusted && remote_trusted) {
        let code = wire::code(
            &connection,
            if outgoing { own } else { remote },
            if outgoing { remote } else { own },
        )?;
        let (answer, response) = oneshot::channel();
        emit(
            &updates,
            generation,
            Event::Pair {
                code,
                peer: peer_name.clone(),
                answer,
            },
        )?;
        let yes = tokio::time::timeout(pairing_left(), response)
            .await
            .map_err(wire::error)?
            .map_err(wire::error)?;
        if !yes {
            connection.close(1u32.into(), b"Pairing declined");
            return Err(wire::error("Pairing was declined"));
        }
    }
    wire::bounded(
        wire::send(&mut send, &Message::Confirmed),
        &cancel,
        pairing_left(),
    )
    .await?;
    if !matches!(
        wire::bounded(wire::receive(&mut recv), &cancel, pairing_left()).await?,
        Message::Confirmed
    ) {
        return Err(wire::error("Peer did not confirm pairing"));
    }
    trusted.lock().map_err(wire::error)?.insert(remote);
    let (manifest, selection) = if let Some(selection) = selection {
        let prepare_cancel = cancel.clone();
        let selection = tokio::task::spawn_blocking(move || selection.prepare(&prepare_cancel))
            .await
            .map_err(wire::error)??;
        wire::bounded(
            wire::send(&mut send, &Message::Offer(selection.manifest.clone())),
            &cancel,
            Duration::from_secs(30),
        )
        .await?;
        (selection.manifest.clone(), Some(selection))
    } else {
        let manifest: Manifest =
            match wire::bounded(wire::receive(&mut recv), &cancel, Duration::from_secs(120)).await?
            {
                Message::Offer(m) => m,
                _ => return Err(wire::error("Missing peer file offer")),
            };
        manifest.validate()?;
        (manifest, None)
    };
    let session = Arc::new(Session::new(
        connection,
        send,
        recv,
        manifest,
        selection,
        reservation,
        lan,
        peer_name,
    ));
    emit(&updates, generation, Event::Ready(session))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{fs, path::PathBuf};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-peer-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    async fn next(receiver: &mpsc::Receiver<Update>) -> io::Result<Event> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match receiver.try_recv() {
                    Ok(update) => return Ok(update.event),
                    Err(mpsc::TryRecvError::Empty) => {
                        tokio::time::sleep(Duration::from_millis(1)).await
                    }
                    Err(e) => return Err(wire::error(e)),
                }
            }
        })
        .await
        .map_err(wire::error)?
    }
    pub(crate) async fn sessions(
        selection: Selection,
    ) -> io::Result<(Endpoint, Endpoint, Arc<Session>, Arc<Session>)> {
        sessions_approval(selection, true).await
    }
    async fn sessions_approval(
        selection: Selection,
        approval: bool,
    ) -> io::Result<(Endpoint, Endpoint, Arc<Session>, Arc<Session>)> {
        sessions_trust(selection, approval, 0).await
    }
    async fn sessions_trust(
        selection: Selection,
        approval: bool,
        trust: u8,
    ) -> io::Result<(Endpoint, Endpoint, Arc<Session>, Arc<Session>)> {
        let lan = Lan::loopback();
        let a = endpoint(&lan, Some("127.0.0.1:0".parse().unwrap())).await?;
        let b = endpoint(&lan, Some("127.0.0.1:0".parse().unwrap())).await?;
        assert!(a.address_lookup().map_err(wire::error)?.is_empty());
        assert!(a
            .addr()
            .addrs
            .iter()
            .all(|a| matches!(a, iroh::TransportAddr::Ip(_))));
        let address = b.addr();
        let incoming = async {
            b.accept()
                .await
                .ok_or_else(|| wire::error("Endpoint stopped"))?
                .await
                .map_err(wire::error)
        };
        let (ac, bc) = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(a.connect(address, ALPN), incoming)
        })
        .await
        .map_err(wire::error)?;
        let ac = ac.map_err(wire::error)?;
        let bc = bc?;
        // Production sessions can wait for a human or a paused queue. Tests must
        // not hang indefinitely if that protocol regresses. Closing both peers
        // also releases the blocking transfer workers before runtime shutdown.
        let deadline_a = ac.clone();
        let deadline_b = bc.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            deadline_a.close(3u32.into(), b"Test session deadline");
            deadline_b.close(3u32.into(), b"Test session deadline");
        });
        let (asend, ar) = Updates::new(8);
        let (bsend, br) = Updates::new(8);
        let aid = a.id();
        let bid = b.id();
        let pa = tokio::spawn(pair(
            ac,
            aid,
            "DN sender".into(),
            Some(selection),
            Reservation {
                busy: Arc::new(AtomicBool::new(true)),
            },
            lan.clone(),
            1,
            asend,
            Arc::new(Mutex::new(if trust > 0 {
                BTreeSet::from([bid])
            } else {
                BTreeSet::new()
            })),
            Arc::new(AtomicBool::new(false)),
        ));
        let pb = tokio::spawn(pair(
            bc,
            bid,
            "DN receiver".into(),
            None,
            Reservation {
                busy: Arc::new(AtomicBool::new(true)),
            },
            lan,
            1,
            bsend,
            Arc::new(Mutex::new(if trust > 1 {
                BTreeSet::from([aid])
            } else {
                BTreeSet::new()
            })),
            Arc::new(AtomicBool::new(false)),
        ));
        if trust < 2 {
            let Event::Pair {
                code: ca,
                answer: aa,
                ..
            } = next(&ar).await?
            else {
                return Err(wire::error("Missing sender pairing"));
            };
            let Event::Pair {
                code: cb,
                answer: ab,
                ..
            } = next(&br).await?
            else {
                return Err(wire::error("Missing receiver pairing"));
            };
            assert_eq!(ca, cb);
            assert_eq!(ca.replace(' ', "").len(), 8);
            aa.send(approval)
                .map_err(|_| wire::error("Pairing stopped"))?;
            ab.send(approval)
                .map_err(|_| wire::error("Pairing stopped"))?;
        }
        let ra = pa.await.map_err(wire::error)?;
        let rb = pb.await.map_err(wire::error)?;
        if !approval {
            assert!(ra.is_err() && rb.is_err());
            assert!(
                ar.try_recv().is_err() && br.try_recv().is_err(),
                "An unconfirmed peer must never receive a manifest"
            );
            a.close().await;
            b.close().await;
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Pairing declined",
            ));
        }
        ra?;
        rb?;
        let Event::Ready(a_session) = next(&ar).await? else {
            return Err(wire::error("Missing sender session"));
        };
        let Event::Ready(b_session) = next(&br).await? else {
            return Err(wire::error("Missing receiver offer"));
        };
        Ok((a, b, a_session, b_session))
    }
    #[tokio::test]
    async fn isolated_encrypted_pairing_streams_large_files_folders_and_empty_directories(
    ) -> io::Result<()> {
        let root = Temp::new()?;
        let source = root.0.join("Ёж世界");
        let destination = root.0.join("destination");
        fs::create_dir(&source)?;
        fs::create_dir(source.join("empty"))?;
        fs::create_dir(&destination)?;
        let bytes = vec![73; 3 * 1024 * 1024 + 17];
        fs::write(source.join("large.bin"), &bytes)?;
        let selection = Selection::capture(vec![source.clone()])?;
        let (a, b, sender, receiver) = sessions(selection).await?;
        let target = destination.clone();
        let receive = tokio::task::spawn_blocking(move || {
            receiver.run(
                Some(target),
                crate::operations::CopyOptions {
                    verify_writes: true,
                    ..Default::default()
                },
                &AtomicBool::new(false),
                |_| {},
                &mut |_| Err(wire::error("Unexpected conflict")),
            )
        });
        let send = tokio::task::spawn_blocking(move || {
            sender.run(
                None,
                Default::default(),
                &AtomicBool::new(false),
                |_| {},
                &mut |_| Err(wire::error("Unexpected conflict")),
            )
        });
        let received = receive.await.map_err(wire::error)??;
        let sent = send.await.map_err(wire::error)??;
        assert_eq!(received.bytes, bytes.len() as u64);
        assert_eq!(sent.bytes, received.bytes);
        assert_eq!(sent.files, received.files);
        assert_eq!(fs::read(destination.join("Ёж世界/large.bin"))?, bytes);
        assert!(destination.join("Ёж世界/empty").is_dir());
        assert_eq!(fs::read(source.join("large.bin"))?, bytes);
        a.close().await;
        b.close().await;
        Ok(())
    }
    #[tokio::test]
    async fn rejected_verification_never_exchanges_a_file_offer() -> io::Result<()> {
        let root = Temp::new()?;
        let path = root.0.join("private-file");
        fs::write(&path, b"not offered")?;
        assert!(sessions_approval(Selection::capture(vec![path])?, false)
            .await
            .is_err());
        Ok(())
    }
    #[tokio::test]
    async fn ask_merges_folders_replace_all_is_scoped_and_skip_preserves_originals(
    ) -> io::Result<()> {
        let root = Temp::new()?;
        let source = root.0.join("docs");
        let destination = root.0.join("destination");
        fs::create_dir(&source)?;
        fs::create_dir_all(destination.join("docs"))?;
        for name in ["a", "b"] {
            fs::write(source.join(name), b"new")?;
            fs::write(destination.join("docs").join(name), b"old")?;
        }
        fs::write(destination.join("docs/keep"), b"unrelated")?;
        for policy in [
            crate::operations::ExistingDestination::Ask,
            crate::operations::ExistingDestination::Skip,
        ] {
            let (a, b, sender, receiver) =
                sessions(Selection::capture(vec![source.clone()])?).await?;
            let target = destination.clone();
            let received = tokio::task::spawn_blocking(move || {
                let mut prompts = 0;
                let result = receiver.run(
                    Some(target),
                    crate::operations::CopyOptions {
                        existing: policy,
                        verify_writes: true,
                        ..Default::default()
                    },
                    &AtomicBool::new(false),
                    |_| {},
                    &mut |conflict| {
                        prompts += 1;
                        assert!(conflict.source_label.is_some());
                        Ok(crate::operations::conflict::Decision::ReplaceAll)
                    },
                );
                (result, prompts)
            });
            let sending = tokio::task::spawn_blocking(move || {
                sender.run(
                    None,
                    Default::default(),
                    &AtomicBool::new(false),
                    |_| {},
                    &mut |_| Err(wire::error("Unexpected sender conflict")),
                )
            });
            let (result, prompts) = received.await.map_err(wire::error)?;
            let result = result?;
            let sent = sending.await.map_err(wire::error)??;
            assert_eq!(sent.files, result.files);
            assert_eq!(
                prompts,
                usize::from(policy == crate::operations::ExistingDestination::Ask)
            );
            if policy == crate::operations::ExistingDestination::Skip {
                assert_eq!(result.files, 0);
                assert_eq!(result.skipped, 3);
            }
            assert_eq!(fs::read(destination.join("docs/keep"))?, b"unrelated");
            for name in ["a", "b"] {
                assert_eq!(fs::read(destination.join("docs").join(name))?, b"new");
                assert_eq!(fs::read(source.join(name))?, b"new");
            }
            a.close().await;
            b.close().await;
        }
        Ok(())
    }
    #[tokio::test]
    async fn cancellation_removes_owned_output_and_keeps_sources() -> io::Result<()> {
        let root = Temp::new()?;
        let source = root.0.join("file");
        let target = root.0.join("target");
        fs::write(&source, b"preserve")?;
        fs::create_dir(&target)?;
        let (a, b, sender, receiver) = sessions(Selection::capture(vec![source.clone()])?).await?;
        let destination = target.clone();
        let received = tokio::task::spawn_blocking(move || {
            receiver.run(
                Some(destination),
                Default::default(),
                &AtomicBool::new(true),
                |_| {},
                &mut |_| Err(wire::error("Unexpected conflict")),
            )
        });
        let sending = tokio::task::spawn_blocking(move || {
            sender.run(
                None,
                Default::default(),
                &AtomicBool::new(false),
                |_| {},
                &mut |_| Err(wire::error("Unexpected conflict")),
            )
        });
        assert_eq!(
            received.await.map_err(wire::error)?.unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        assert!(sending.await.map_err(wire::error)?.is_err());
        assert_eq!(fs::read_dir(&target)?.count(), 0);
        assert_eq!(fs::read(&source)?, b"preserve");
        a.close().await;
        b.close().await;
        Ok(())
    }
    #[tokio::test]
    async fn verification_is_skipped_only_for_bilateral_session_trust() -> io::Result<()> {
        let root = Temp::new()?;
        let path = root.0.join("file");
        fs::write(&path, b"trust")?;
        for trust in [1, 2, 0] {
            let (a, b, sender, receiver) =
                sessions_trust(Selection::capture(vec![path.clone()])?, true, trust).await?;
            assert!(sender.sending());
            assert!(!receiver.sending());
            // A Ready session is still an offer: no receive job or output exists.
            assert_eq!(fs::read_dir(&root.0)?.count(), 1);
            sender.abort();
            receiver.abort();
            a.close().await;
            b.close().await;
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires an eligible LAN interface and multicast permission"]
    fn local_mdns_discovers_only_an_explicit_receiver() -> io::Result<()> {
        let receiver = Service::start()?;
        let sender = Service::start()?;
        receiver.command(Command::Discovery {
            generation: 1,
            receive: true,
        })?;
        sender.command(Command::Discovery {
            generation: 1,
            receive: false,
        })?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut receiver_id = None;
        let mut seen = BTreeSet::new();
        while Instant::now() < deadline {
            while let Ok(update) = receiver.events.try_recv() {
                match update.event {
                    Event::Details(card) => {
                        receiver_id = Some(lan::parse(&card, &Lan::capture())?.id)
                    }
                    Event::Failure(error) => return Err(wire::error(error)),
                    _ => {}
                }
            }
            while let Ok(update) = sender.events.try_recv() {
                match update.event {
                    Event::Found(peer) => {
                        seen.insert(peer.address.id);
                    }
                    Event::Failure(error) => return Err(wire::error(error)),
                    _ => {}
                }
            }
            if receiver_id.is_some_and(|id| seen.contains(&id)) {
                receiver.command(Command::Stop)?;
                sender.command(Command::Stop)?;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(wire::error(
            "No receiver discovered on this host's multicast network",
        ))
    }
    #[test]
    fn bounded_event_overflow_rejects_pairing_and_retains_a_failure_notice() {
        let (updates, events) = Updates::new(1);
        emit(&updates, 4, Event::Info("first".into())).unwrap();
        let (answer, mut response) = oneshot::channel();
        assert!(emit(
            &updates,
            4,
            Event::Pair {
                code: "1234 5678".into(),
                peer: "DN peer".into(),
                answer
            }
        )
        .is_err());
        assert!(matches!(
            response.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        assert!(matches!(events.try_recv().unwrap().event, Event::Info(_)));
        let failure = updates.failure.lock().unwrap().take().unwrap();
        assert_eq!(failure.generation, 4);
        assert!(matches!(failure.event, Event::Failure(_)));
    }
}
