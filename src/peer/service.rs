//! One lazy runtime owns the endpoint, discovery and short-lived pairing work.
use super::{
    discovery::{self, Network},
    lan::{self, Lan},
    manifest::{Manifest, Selection},
    session::Session,
    wire::{self, Message, ALPN, TEST_ALPN, TEXT_ALPN, TEXT_LIMIT},
};
use futures_util::StreamExt;
use iroh::{
    endpoint::{presets, IncomingAddr, NetReportConfig, PortmapperConfig, RelayMode},
    Endpoint, EndpointAddr, EndpointId,
};
use iroh_mdns_address_lookup::MdnsAddressLookup;
#[cfg(test)]
use std::sync::atomic::AtomicU64;
use std::{
    collections::BTreeSet,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc as channel, oneshot};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub name: String,
    pub address: EndpointAddr,
}
pub enum Event {
    TextOffer {
        peer: String,
        bytes: usize,
        answer: oneshot::Sender<bool>,
    },
    TextReceived {
        peer: String,
        text: String,
        stored: oneshot::Sender<bool>,
        reservation: Arc<Reservation>,
    },
    TextSent,
    Failure(String),
    Network(discovery::Snapshot),
    ConnectFailure {
        text: bool,
        testing: bool,
        timed_out: bool,
        cause: String,
    },
    Pair {
        code: String,
        peer: String,
        answer: oneshot::Sender<bool>,
        testing: bool,
    },
    Ready(Arc<Session>),
    Tested {
        peer: String,
    },
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
    Text {
        generation: u64,
        address: String,
        text: String,
    },
    Rename(String),
    Test {
        generation: u64,
        address: String,
    },
    Discovery {
        generation: u64,
        network: u64,
        receive: bool,
    },
    Connect {
        generation: u64,
        address: String,
        selection: Selection,
    },
    Stop {
        generation: u64,
        network: u64,
    },
}
pub struct Service {
    send: channel::Sender<Command>,
    pub events: mpsc::Receiver<Update>,
    failure: Arc<Mutex<Option<Update>>>,
    busy: Arc<AtomicBool>,
}
impl Service {
    #[cfg(test)]
    pub fn start() -> io::Result<Self> {
        let identity = super::identity::Identity::default();
        Self::start_named(identity.key, identity.name)
    }
    pub fn start_named(key: iroh::SecretKey, name: String) -> io::Result<Self> {
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
                    if let Err(e) = run(commands, updates.clone(), worker_busy, key, name).await {
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
pub(super) struct LocalAdvertisement(pub(super) MdnsAddressLookup);
impl iroh::address_lookup::AddressLookup for LocalAdvertisement {
    fn publish(&self, data: &iroh::address_lookup::EndpointData) {
        iroh::address_lookup::AddressLookup::publish(&self.0, data);
    }
}
#[cfg(test)]
pub(crate) async fn endpoint(
    lan: &Lan,
    bind: Option<std::net::SocketAddr>,
) -> io::Result<Endpoint> {
    endpoint_with_key(lan, bind, None).await
}
pub(super) async fn endpoint_with_key(
    lan: &Lan,
    bind: Option<std::net::SocketAddr>,
    key: Option<iroh::SecretKey>,
) -> io::Result<Endpoint> {
    let filter_lan = lan.clone();
    let mut builder = Endpoint::builder(presets::Minimal)
        .alpns(vec![ALPN.to_vec(), TEST_ALPN.to_vec(), TEXT_ALPN.to_vec()])
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
    if let Some(key) = key {
        builder = builder.secret_key(key);
    }
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
fn report_network(
    network: &Network,
    last: &mut Option<discovery::Snapshot>,
    updates: &Updates,
    generation: u64,
) {
    if last.as_ref() != Some(&network.view)
        && emit(updates, generation, Event::Network(network.view.clone())).is_ok()
    {
        *last = Some(network.view.clone());
    }
}
async fn run(
    mut commands: channel::Receiver<Command>,
    updates: Updates,
    busy: Arc<AtomicBool>,
    key: iroh::SecretKey,
    mut name: String,
) -> io::Result<()> {
    let mut network = Network::new(key, name.clone());
    let mut generation = 0;
    let trusted = Arc::new(Mutex::new(BTreeSet::new()));
    let mut clock = tokio::time::interval(Duration::from_secs(1));
    let mut attempts = Vec::<Instant>::new();
    let mut attempt_cancel: Option<Arc<AtomicBool>> = None;
    let mut reported = None;
    loop {
        tokio::select! {
            command=commands.recv()=>match command {
                None=>break,
                Some(Command::Rename(value))=>{name=value.clone();network.rename(value);},
                Some(Command::Stop{generation:owner,network:request})=>{
                    generation=owner;
                    if let Some(cancel)=attempt_cancel.take(){cancel.store(true,Ordering::Release);}
                    network.stop(request);
                }
                Some(Command::Discovery{generation:owner,network:request,receive})=>{
                    generation=owner;
                    network.request(request,receive,busy.load(Ordering::Acquire));
                    network.tick(busy.load(Ordering::Acquire)).await;
                }
                Some(command @ (Command::Connect{..}|Command::Test{..}|Command::Text{..}))=>{
                    let (owner,address,operation)=match command {
                        Command::Connect{generation,address,selection}=>(generation,address,Exchange::Transfer(Some(selection))),
                        Command::Test{generation,address}=>(generation,address,Exchange::Test(true)),
                        Command::Text{generation,address,text}=>(generation,address,Exchange::Text(Some(text))),
                        _=>unreachable!(),
                    };
                    let testing=matches!(operation,Exchange::Test(_));
                    let text_transfer=matches!(operation,Exchange::Text(_));
                    let protocol=operation.protocol();
                    if busy.load(Ordering::Acquire){let _=emit(&updates,owner,Event::Failure("Another peer session is active.".into()));continue;}
                    let ep=match network.connect_ready().await {
                        Ok(endpoint)=>endpoint,
                        Err(error)=>{let _=emit(&updates,owner,Event::ConnectFailure{text:text_transfer,testing,timed_out:false,cause:error.to_string()});report_network(&network,&mut reported,&updates,generation);continue;}
                    };
                    if busy.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err(){let _=emit(&updates,owner,Event::Failure("Another peer session is active.".into()));continue;}
                    let reservation=Reservation{busy:busy.clone()};
                    let cancel=Arc::new(AtomicBool::new(false));attempt_cancel=Some(cancel.clone());
                    let ui=updates.clone();let networks=network.lan.clone();let own_name=name.clone();let trust=trusted.clone();
                    tokio::spawn(async move {
                        let addr=match lan::parse(&address,&networks){Ok(addr)=>addr,Err(error)=>{let _=emit(&ui,owner,Event::ConnectFailure{text:text_transfer,testing,timed_out:false,cause:error.to_string()});return;}};
                        let connection=match tokio::time::timeout(Duration::from_secs(10),ep.connect(addr,protocol)).await {
                            Ok(Ok(connection))=>connection,
                            Ok(Err(error))=>{let cause=error.to_string();let _=emit(&ui,owner,Event::ConnectFailure{text:text_transfer,testing,timed_out:false,cause});return;}
                            Err(error)=>{let _=emit(&ui,owner,Event::ConnectFailure{text:text_transfer,testing,timed_out:true,cause:error.to_string()});return;}
                        };

                        if let Err(error)=exchange(connection,ep.id(),own_name,operation,reservation,networks,owner,ui.clone(),trust,cancel).await {
                            let _=emit(&ui,owner,Event::Failure(error.to_string()));
                        }
                    });
                }
            },
            incoming=async {match &network.endpoint{Some(endpoint)=>endpoint.accept().await,None=>std::future::pending().await}}=>if let Some(incoming)=incoming {
                attempts.retain(|t|t.elapsed()<Duration::from_secs(60));
                if !network.view.receiving || attempts.len()>=8 || !matches!(incoming.remote_addr(),IncomingAddr::Ip(a) if network.lan.allows(&a)) {incoming.refuse();continue;}
                attempts.push(Instant::now());
                if busy.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err() {
                    tokio::spawn(async move {let _=tokio::time::timeout(Duration::from_secs(5),async move {
                        if let Ok(connection)=incoming.await {
                            if let Ok((mut send,_recv))=connection.accept_bi().await{let _=wire::send(&mut send,&Message::Busy).await;let _=send.finish();let _=send.stopped().await;}
                            connection.close(2u32.into(),b"Busy");
                        }
                    }).await;});continue;
                }
                let reservation=Reservation{busy:busy.clone()};
                let cancel=Arc::new(AtomicBool::new(false));attempt_cancel=Some(cancel.clone());
                let ui=updates.clone();let networks=network.lan.clone();let own_name=name.clone();let id=network.id();let trust=trusted.clone();let owner=generation;
                tokio::spawn(async move {
                    let result=async {
                        let connection=tokio::time::timeout(Duration::from_secs(10),incoming).await.map_err(wire::error)?.map_err(wire::error)?;
                        let operation=if connection.alpn()==TEST_ALPN {Exchange::Test(false)}else if connection.alpn()==TEXT_ALPN {Exchange::Text(None)}else{Exchange::Transfer(None)};
                        exchange(connection,id,own_name,operation,reservation,networks,owner,ui.clone(),trust,cancel).await
                    }.await;
                    if let Err(error)=result{let _=emit(&ui,owner,Event::Failure(error.to_string()));}
                });
            },
            event=async {match &mut network.events {Some(events)=>events.next().await,None=>std::future::pending().await}}=>network.event(event),
            _=clock.tick()=>network.tick(busy.load(Ordering::Acquire)).await,
        }
        report_network(&network, &mut reported, &updates, generation);
    }
    network.close().await;
    Ok(())
}
enum Exchange {
    Text(Option<String>),
    Transfer(Option<Selection>),
    Test(bool),
}
impl Exchange {
    fn protocol(&self) -> &'static [u8] {
        match self {
            Self::Text(_) => TEXT_ALPN,
            Self::Test(_) => TEST_ALPN,
            Self::Transfer(_) => ALPN,
        }
    }
}

#[cfg(test)]
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
    exchange(
        connection,
        own,
        name,
        Exchange::Transfer(selection),
        reservation,
        lan,
        generation,
        updates,
        trusted,
        cancel,
    )
    .await
}
#[allow(clippy::too_many_arguments)]
async fn exchange(
    connection: iroh::endpoint::Connection,
    own: EndpointId,
    name: String,
    operation: Exchange,
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
    let protocol = operation.protocol();
    let (selection, outgoing, testing, text) = match operation {
        Exchange::Transfer(selection) => {
            let outgoing = selection.is_some();
            (selection, outgoing, false, None)
        }
        Exchange::Test(outgoing) => (None, outgoing, true, None),
        Exchange::Text(text) => (None, text.is_some(), false, Some(text)),
    };
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
            protocol,
        )?;
        let (answer, response) = oneshot::channel();
        emit(
            &updates,
            generation,
            Event::Pair {
                code,
                peer: peer_name.clone(),
                answer,
                testing,
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
    if let Some(text) = text {
        let result = exchange_text(
            &mut send,
            &mut recv,
            text,
            peer_name,
            reservation,
            generation,
            &updates,
            &cancel,
        )
        .await;
        connection.close(0u32.into(), b"Text exchange finished");
        return result;
    }
    if testing {
        async fn expect(
            recv: &mut iroh::endpoint::RecvStream,
            cancel: &AtomicBool,
            expected: fn(&Message) -> bool,
        ) -> io::Result<()> {
            let message =
                wire::bounded(wire::receive(recv), cancel, Duration::from_secs(10)).await?;
            if expected(&message) {
                Ok(())
            } else {
                Err(wire::error(
                    "Only connection-test messages are allowed; no file offer was accepted.",
                ))
            }
        }
        async fn send_test(
            send: &mut iroh::endpoint::SendStream,
            cancel: &AtomicBool,
            message: Message,
        ) -> io::Result<()> {
            wire::bounded(wire::send(send, &message), cancel, Duration::from_secs(10)).await
        }
        let result = async {
            if outgoing {
                send_test(&mut send, &cancel, Message::TestRequest).await?;
                expect(&mut recv, &cancel, |m| matches!(m, Message::TestResponse)).await?;
                send_test(&mut send, &cancel, Message::TestFinished).await?;
                expect(&mut recv, &cancel, |m| matches!(m, Message::TestReceipt)).await?;
                let _ = send_test(&mut send, &cancel, Message::Acknowledged).await;
            } else {
                expect(&mut recv, &cancel, |m| matches!(m, Message::TestRequest)).await?;
                send_test(&mut send, &cancel, Message::TestResponse).await?;
                expect(&mut recv, &cancel, |m| matches!(m, Message::TestFinished)).await?;
                send_test(&mut send, &cancel, Message::TestReceipt).await?;
                let _ = expect(&mut recv, &cancel, |m| matches!(m, Message::Acknowledged)).await;
            }
            if cancel.load(Ordering::Relaxed) {
                return Err(wire::error("Connection test cancelled"));
            }
            emit(&updates, generation, Event::Tested { peer: peer_name })
        }
        .await;
        connection.close(0u32.into(), b"Connection test finished");
        return result;
    }
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

#[allow(clippy::too_many_arguments)]
async fn exchange_text(
    send: &mut iroh::endpoint::SendStream,
    recv: &mut iroh::endpoint::RecvStream,
    text: Option<String>,
    peer: String,
    reservation: Reservation,
    generation: u64,
    updates: &Updates,
    cancel: &AtomicBool,
) -> io::Result<()> {
    let reservation = Arc::new(reservation);
    let short = Duration::from_secs(10);
    let human = Duration::from_secs(120);
    if let Some(text) = text {
        validate_text(&text)?;
        wire::bounded(
            wire::send(send, &Message::TextOffer { bytes: text.len() }),
            cancel,
            short,
        )
        .await?;
        match wire::bounded(
            wire::receive_limited(recv, TEXT_LIMIT * 6 + 128),
            cancel,
            human,
        )
        .await?
        {
            Message::TextAccepted => {}
            Message::TextDeclined => {
                let _ =
                    wire::bounded(wire::send(send, &Message::Acknowledged), cancel, short).await;
                return Err(wire::error("Text offer declined; no text was sent."));
            }
            _ => return Err(wire::error("Invalid text-offer response")),
        }
        // Once body publication starts a lost response cannot establish non-delivery.
        let delivery = async {
            wire::bounded(wire::send(send, &Message::TextBody(text)), cancel, short).await?;
            if !matches!(
                wire::bounded(
                    wire::receive_limited(recv, TEXT_LIMIT * 6 + 128),
                    cancel,
                    short
                )
                .await?,
                Message::TextReceipt
            ) {
                return Err(wire::error("Missing text receipt"));
            }
            Ok::<_, io::Error>(())
        }
        .await;
        if let Err(error) = delivery {
            return Err(wire::error(format!(
                "Text delivery unconfirmed; the other DN may have received it: {error}"
            )));
        }
        let _ = wire::bounded(wire::send(send, &Message::Acknowledged), cancel, short).await;
        emit(updates, generation, Event::TextSent)
    } else {
        let bytes = match wire::bounded(
            wire::receive_limited(recv, TEXT_LIMIT * 6 + 128),
            cancel,
            human,
        )
        .await?
        {
            Message::TextOffer { bytes } if bytes > 0 && bytes <= TEXT_LIMIT => bytes,
            _ => return Err(wire::error("Invalid text offer; limit is 64 KiB.")),
        };
        let (answer, response) = oneshot::channel();
        emit(
            updates,
            generation,
            Event::TextOffer {
                peer: peer.clone(),
                bytes,
                answer,
            },
        )?;
        let approved =
            wire::bounded(async { response.await.map_err(wire::error) }, cancel, human).await?;
        wire::bounded(
            wire::send(
                send,
                &if approved {
                    Message::TextAccepted
                } else {
                    Message::TextDeclined
                },
            ),
            cancel,
            short,
        )
        .await?;
        if !approved {
            let _ = wire::bounded(
                wire::receive_limited(recv, TEXT_LIMIT * 6 + 128),
                cancel,
                short,
            )
            .await;
            return Ok(());
        }
        let text = match wire::bounded(
            wire::receive_limited(recv, TEXT_LIMIT * 6 + 128),
            cancel,
            short,
        )
        .await?
        {
            Message::TextBody(text) if text.len() == bytes => text,
            _ => return Err(wire::error("Invalid text payload")),
        };
        validate_text(&text)?;
        let (stored, response) = oneshot::channel();
        emit(
            updates,
            generation,
            Event::TextReceived {
                peer,
                text,
                stored,
                reservation: reservation.clone(),
            },
        )?;
        if !wire::bounded(async { response.await.map_err(wire::error) }, cancel, short).await? {
            return Err(wire::error("Text preview is no longer available"));
        }
        wire::bounded(wire::send(send, &Message::TextReceipt), cancel, short).await?;
        let _ = wire::bounded(
            wire::receive_limited(recv, TEXT_LIMIT * 6 + 128),
            cancel,
            short,
        )
        .await;
        Ok(())
    }
}
fn validate_text(text: &str) -> io::Result<()> {
    if text.is_empty() || text.len() > TEXT_LIMIT {
        Err(wire::error("Text must contain 1–65536 UTF-8 bytes."))
    } else {
        Ok(())
    }
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

    #[tokio::test]
    async fn encrypted_text_requires_pairing_and_approval_then_retains_exact_payload(
    ) -> io::Result<()> {
        for approve in [true, false] {
            let lan = Lan::loopback();
            let a = endpoint(&lan, Some("127.0.0.1:0".parse().unwrap())).await?;
            let b = endpoint(&lan, Some("127.0.0.1:0".parse().unwrap())).await?;
            let incoming = async { b.accept().await.unwrap().await.map_err(wire::error) };
            let (ac, bc) = tokio::time::timeout(Duration::from_secs(10), async {
                tokio::join!(a.connect(b.addr(), TEXT_ALPN), incoming)
            })
            .await
            .map_err(wire::error)?;
            let ac = ac.map_err(wire::error)?;
            let bc = bc?;
            let (au, ar) = Updates::new(8);
            let (bu, br) = Updates::new(8);
            let ab = Arc::new(AtomicBool::new(true));
            let bb = Arc::new(AtomicBool::new(true));
            let text = "https://example.test/Ёжик?q=世界\r\nnotes\twith tabs\n".repeat(500);
            let pa = tokio::spawn(exchange(
                ac,
                a.id(),
                "sender".into(),
                Exchange::Text(Some(text.clone())),
                Reservation { busy: ab.clone() },
                lan.clone(),
                1,
                au,
                Arc::new(Mutex::new(BTreeSet::new())),
                Arc::new(AtomicBool::new(false)),
            ));
            let pb = tokio::spawn(exchange(
                bc,
                b.id(),
                "receiver".into(),
                Exchange::Text(None),
                Reservation { busy: bb.clone() },
                lan,
                1,
                bu,
                Arc::new(Mutex::new(BTreeSet::new())),
                Arc::new(AtomicBool::new(false)),
            ));
            let Event::Pair {
                code: ca,
                answer: aa,
                ..
            } = next(&ar).await?
            else {
                panic!("Pair first");
            };
            let Event::Pair {
                code: cb,
                answer: ba,
                ..
            } = next(&br).await?
            else {
                panic!("Pair first");
            };
            assert_eq!(ca, cb);
            aa.send(true).unwrap();
            ba.send(true).unwrap();
            let Event::TextOffer {
                bytes,
                answer,
                peer,
            } = next(&br).await?
            else {
                panic!("Offer before body");
            };
            assert_eq!(bytes, text.len());
            assert_eq!(peer, "sender");
            assert!(
                ar.try_recv().is_err() && br.try_recv().is_err(),
                "No text before approval"
            );
            answer.send(approve).unwrap();
            let mut held = None;
            if approve {
                let Event::TextReceived {
                    peer,
                    text: received,
                    stored,
                    reservation,
                } = next(&br).await?
                else {
                    panic!("Missing received text");
                };
                assert_eq!(received, text);
                assert_eq!(peer, "sender");
                assert!(
                    ar.try_recv().is_err(),
                    "Success must wait for live-host retention"
                );
                stored.send(true).unwrap();
                held = Some(reservation);
                assert!(matches!(next(&ar).await?, Event::TextSent));
            }
            let (ra, rb) =
                tokio::time::timeout(Duration::from_secs(15), async { tokio::join!(pa, pb) })
                    .await
                    .map_err(wire::error)?;
            if approve {
                ra.map_err(wire::error)??;
            } else {
                assert!(ra.map_err(wire::error)?.is_err());
            }
            rb.map_err(wire::error)??;
            assert!(!ab.load(Ordering::Acquire));
            assert_eq!(bb.load(Ordering::Acquire), approve);
            drop(held);
            assert!(!bb.load(Ordering::Acquire));
            assert!(
                ar.try_recv().is_err() && br.try_recv().is_err(),
                "No file sessions or manifests"
            );
            a.close().await;
            b.close().await;
        }
        Ok(())
    }
    async fn text_connections() -> io::Result<(
        Endpoint,
        Endpoint,
        iroh::endpoint::Connection,
        iroh::endpoint::Connection,
    )> {
        let lan = Lan::loopback();
        let a = endpoint(&lan, Some("127.0.0.1:0".parse().unwrap())).await?;
        let b = endpoint(&lan, Some("127.0.0.1:0".parse().unwrap())).await?;
        let incoming = async { b.accept().await.unwrap().await.map_err(wire::error) };
        let (ac, bc) = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(a.connect(b.addr(), TEXT_ALPN), incoming)
        })
        .await
        .map_err(wire::error)?;
        Ok((a, b, ac.map_err(wire::error)?, bc?))
    }
    #[tokio::test]
    async fn malformed_text_and_cancelled_approval_never_publish_a_preview() -> io::Result<()> {
        for case in 0..4 {
            let (a, b, ac, bc) = text_connections().await?;
            let (updates, events) = Updates::new(8);
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_cancel = cancel.clone();
            let receiver = tokio::spawn(async move {
                let (mut send, mut recv) = bc.accept_bi().await.map_err(wire::error)?;
                exchange_text(
                    &mut send,
                    &mut recv,
                    None,
                    "peer".into(),
                    Reservation {
                        busy: Arc::new(AtomicBool::new(true)),
                    },
                    1,
                    &updates,
                    &worker_cancel,
                )
                .await
            });
            let (mut send, mut recv) = ac.open_bi().await.map_err(wire::error)?;
            wire::send(
                &mut send,
                &Message::TextOffer {
                    bytes: if case == 0 { TEXT_LIMIT + 1 } else { 2 },
                },
            )
            .await?;
            if case != 0 {
                let Event::TextOffer { answer, .. } = next(&events).await? else {
                    panic!("Expected offer");
                };
                if case == 3 {
                    cancel.store(true, Ordering::Release);
                    drop(answer);
                } else {
                    answer.send(true).unwrap();
                    assert!(matches!(
                        wire::receive(&mut recv).await?,
                        Message::TextAccepted
                    ));
                    if case == 1 {
                        wire::send(&mut send, &Message::TextBody("x".into())).await?;
                    } else {
                        let mut data = b"{\"TextBody\":\"".to_vec();
                        data.push(0xff);
                        data.extend_from_slice(b"\"}");
                        send.write_all(&(data.len() as u32).to_be_bytes())
                            .await
                            .map_err(wire::error)?;
                        send.write_all(&data).await.map_err(wire::error)?;
                    }
                }
            }
            assert!(tokio::time::timeout(Duration::from_secs(10), receiver)
                .await
                .map_err(wire::error)?
                .map_err(wire::error)?
                .is_err());
            assert!(
                events.try_recv().is_err(),
                "Malformed/cancelled input must never reach a preview"
            );
            a.close().await;
            b.close().await;
        }
        Ok(())
    }
    #[tokio::test]
    async fn missing_text_receipt_reports_unconfirmed_delivery_not_success() -> io::Result<()> {
        let (a, b, ac, bc) = text_connections().await?;
        let (updates, events) = Updates::new(8);
        let outgoing = tokio::spawn(async move {
            let (mut send, mut recv) = ac.open_bi().await.map_err(wire::error)?;
            exchange_text(
                &mut send,
                &mut recv,
                Some("https://example.test".into()),
                "peer".into(),
                Reservation {
                    busy: Arc::new(AtomicBool::new(true)),
                },
                1,
                &updates,
                &AtomicBool::new(false),
            )
            .await
        });
        let (mut send, mut recv) = bc.accept_bi().await.map_err(wire::error)?;
        assert!(matches!(
            wire::receive(&mut recv).await?,
            Message::TextOffer { .. }
        ));
        wire::send(&mut send, &Message::TextAccepted).await?;
        assert!(matches!(
            wire::receive(&mut recv).await?,
            Message::TextBody(_)
        ));
        bc.close(0u32.into(), b"receipt lost");
        let error = tokio::time::timeout(Duration::from_secs(10), outgoing)
            .await
            .map_err(wire::error)?
            .map_err(wire::error)?
            .unwrap_err();
        assert!(error.to_string().contains("delivery unconfirmed"));
        assert!(events.try_recv().is_err());
        a.close().await;
        b.close().await;
        Ok(())
    }
    #[tokio::test]
    async fn legacy_file_only_endpoints_cannot_accept_text_protocol() -> io::Result<()> {
        let a = endpoint(&Lan::loopback(), Some("127.0.0.1:0".parse().unwrap())).await?;
        let b = Endpoint::builder(presets::Minimal)
            .alpns(vec![ALPN.to_vec()])
            .relay_mode(RelayMode::Disabled)
            .clear_relay_transports()
            .clear_address_lookup()
            .portmapper_config(PortmapperConfig::Disabled)
            .net_report_config(NetReportConfig::minimal())
            .clear_ip_transports()
            .bind_addr("127.0.0.1:0".parse::<std::net::SocketAddr>().unwrap())
            .map_err(wire::error)?
            .bind()
            .await
            .map_err(wire::error)?;
        let incoming = async {
            if let Some(incoming) = b.accept().await {
                assert!(incoming.await.is_err());
            }
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(a.connect(b.addr(), TEXT_ALPN), incoming)
        })
        .await
        .map_err(wire::error)?;
        assert!(result.is_err());
        a.close().await;
        b.close().await;
        Ok(())
    }
    #[test]
    fn text_limits_are_in_utf8_bytes_and_empty_messages_are_rejected() {
        assert!(validate_text("").is_err());
        assert!(validate_text(&"Ё".repeat(TEXT_LIMIT / 2)).is_ok());
        assert!(validate_text(&"x".repeat(TEXT_LIMIT + 1)).is_err());
    }
    #[tokio::test]
    async fn connection_tests_pair_without_a_manifest_or_transfer_and_can_be_declined(
    ) -> io::Result<()> {
        for approve in [true, false] {
            let lan = Lan::loopback();
            let a = endpoint(&lan, Some("127.0.0.1:0".parse().unwrap())).await?;
            let b = endpoint(&lan, Some("127.0.0.1:0".parse().unwrap())).await?;
            let incoming = async { b.accept().await.unwrap().await.map_err(wire::error) };
            let (ac, bc) = tokio::time::timeout(Duration::from_secs(10), async {
                tokio::join!(a.connect(b.addr(), TEST_ALPN), incoming)
            })
            .await
            .map_err(wire::error)?;
            let (au, ar) = Updates::new(8);
            let (bu, br) = Updates::new(8);
            let at = Arc::new(Mutex::new(BTreeSet::new()));
            let bt = Arc::new(Mutex::new(BTreeSet::new()));
            let ab = Arc::new(AtomicBool::new(true));
            let bb = Arc::new(AtomicBool::new(true));
            let pa = tokio::spawn(exchange(
                ac.map_err(wire::error)?,
                a.id(),
                "Sender Ё".into(),
                Exchange::Test(true),
                Reservation { busy: ab.clone() },
                lan.clone(),
                1,
                au,
                at.clone(),
                Arc::new(AtomicBool::new(false)),
            ));
            let pb = tokio::spawn(exchange(
                bc?,
                b.id(),
                "Receiver Ё".into(),
                Exchange::Test(false),
                Reservation { busy: bb.clone() },
                lan,
                1,
                bu,
                bt.clone(),
                Arc::new(AtomicBool::new(false)),
            ));
            let Event::Pair {
                code: ca,
                answer: aa,
                testing: true,
                ..
            } = next(&ar).await?
            else {
                panic!("Missing test pairing");
            };
            let Event::Pair {
                code: cb,
                answer: ba,
                testing: true,
                ..
            } = next(&br).await?
            else {
                panic!("Missing test pairing");
            };
            assert_eq!(ca, cb);
            aa.send(approve).unwrap();
            ba.send(approve).unwrap();
            let (ra, rb) =
                tokio::time::timeout(Duration::from_secs(15), async { tokio::join!(pa, pb) })
                    .await
                    .map_err(wire::error)?;
            if approve {
                ra.map_err(wire::error)??;
                rb.map_err(wire::error)??;
                assert!(matches!(next(&ar).await?,Event::Tested{peer} if peer=="Receiver Ё"));
                assert!(matches!(next(&br).await?,Event::Tested{peer} if peer=="Sender Ё"));
                assert!(at.lock().unwrap().contains(&b.id()));
                assert!(bt.lock().unwrap().contains(&a.id()));
            } else {
                assert!(ra.map_err(wire::error)?.is_err());
                assert!(rb.map_err(wire::error)?.is_err());
                assert!(at.lock().unwrap().is_empty() && bt.lock().unwrap().is_empty());
            }
            assert!(
                ar.try_recv().is_err() && br.try_recv().is_err(),
                "No Ready session or file offer may be produced"
            );
            assert!(!ab.load(Ordering::Acquire) && !bb.load(Ordering::Acquire));
            a.close().await;
            b.close().await;
        }
        Ok(())
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
            network: 1,
            receive: true,
        })?;
        sender.command(Command::Discovery {
            generation: 1,
            network: 1,
            receive: false,
        })?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut receiver_id = None;
        let mut seen = BTreeSet::new();
        while Instant::now() < deadline {
            while let Ok(update) = receiver.events.try_recv() {
                match update.event {
                    Event::Network(state) if !state.details.is_empty() => {
                        receiver_id = Some(lan::parse(&state.details, &Lan::capture())?.id);
                    }
                    Event::Failure(error) => return Err(wire::error(error)),
                    _ => {}
                }
            }
            while let Ok(update) = sender.events.try_recv() {
                match update.event {
                    Event::Network(state) => {
                        seen.extend(state.peers.iter().map(|p| p.address.id));
                    }
                    Event::Failure(error) => return Err(wire::error(error)),
                    _ => {}
                }
            }
            if receiver_id.is_some_and(|id| seen.contains(&id)) {
                receiver.command(Command::Stop {
                    generation: 1,
                    network: 2,
                })?;
                sender.command(Command::Stop {
                    generation: 1,
                    network: 2,
                })?;
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
        emit(&updates, 4, Event::Failure("first".into())).unwrap();
        let (answer, mut response) = oneshot::channel();
        assert!(emit(
            &updates,
            4,
            Event::Pair {
                code: "1234 5678".into(),
                peer: "DN peer".into(),
                answer,
                testing: false
            }
        )
        .is_err());
        assert!(matches!(
            response.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        assert!(matches!(
            events.try_recv().unwrap().event,
            Event::Failure(_)
        ));
        let failure = updates.failure.lock().unwrap().take().unwrap();
        assert_eq!(failure.generation, 4);
        assert!(matches!(failure.event, Event::Failure(_)));
    }
}
