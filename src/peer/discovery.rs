//! Prepared, bounded discovery state. All socket/interface work stays on the peer runtime.
use super::{
    lan::{self, Lan},
    service::{self, Peer},
    wire,
};
use futures_util::Stream;
use iroh::{Endpoint, EndpointId, SecretKey};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use std::{
    collections::BTreeMap,
    io,
    net::SocketAddr,
    pin::Pin,
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Off,
    Starting,
    Advertising,
    Searching,
    NoAnnouncements,
    Filtered,
    BindingFailed,
    DiscoveryFailed,
    Pending,
}
impl Phase {
    pub fn caption(self) -> &'static str {
        match self {
            Self::Off => "Discovery is off",
            Self::Starting => "Starting LAN discovery",
            Self::Advertising => "Advertising on LAN",
            Self::Searching => "Searching for receivers",
            Self::NoAnnouncements => "No announcements received; still searching",
            Self::Filtered => "Receivers seen; no eligible LAN address",
            Self::BindingFailed => "LAN binding unavailable",
            Self::DiscoveryFailed => "Discovery unavailable; use manual details",
            Self::Pending => "Network refresh pending",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub request: u64,
    pub revision: u64,
    pub phase: Phase,
    pub receiving: bool,
    pub ready: bool,
    pub details: String,
    pub peers: Vec<Peer>,
    pub seen: usize,
    pub filtered: usize,
    pub cause: String,
    pub local_addresses: Vec<String>,
}
#[derive(Default)]
struct Catalog {
    records: BTreeMap<EndpointId, Option<Peer>>,
}
impl Catalog {
    fn event(&mut self, event: DiscoveryEvent, own: EndpointId, lan: &Lan) {
        match event {
            DiscoveryEvent::Discovered { endpoint_info, .. }
                if endpoint_info.endpoint_id != own =>
            {
                let id = endpoint_info.endpoint_id;
                if self.records.len() >= 128 && !self.records.contains_key(&id) {
                    return;
                }
                let name = super::identity::advertised(endpoint_info.user_data(), id);
                let peer = lan
                    .discovered(endpoint_info.into_endpoint_addr())
                    .ok()
                    .map(|address| Peer { name, address });
                self.records.insert(id, peer);
            }
            DiscoveryEvent::Expired { endpoint_id } => {
                self.records.remove(&endpoint_id);
            }
            _ => {}
        }
    }
    fn prepare(&self, view: &mut Snapshot, elapsed: bool, advertising: bool) {
        view.seen = self.records.len();
        view.peers = self.records.values().filter_map(Clone::clone).collect();
        view.filtered = view.seen - view.peers.len();
        view.phase = if view.seen > 0 && view.peers.is_empty() {
            Phase::Filtered
        } else if advertising {
            Phase::Advertising
        } else if view.seen == 0 && elapsed {
            Phase::NoAnnouncements
        } else {
            Phase::Searching
        };
    }
}
pub type Events = Pin<Box<dyn Stream<Item = DiscoveryEvent> + Send>>;
pub struct Network {
    pub lan: Lan,
    pub endpoint: Option<Endpoint>,
    pub events: Option<Events>,
    pub view: Snapshot,
    key: SecretKey,
    name: String,
    lookup: Option<MdnsAddressLookup>,
    active: Option<bool>,
    pending: bool,
    bound: Vec<(SocketAddr, u8)>,
    observed: Vec<(SocketAddr, u8)>,
    catalog: Catalog,
    started: Option<Instant>,
    #[cfg(test)]
    test_bind: Option<SocketAddr>,
    #[cfg(test)]
    suppress_discovery: bool,
}
impl Network {
    pub fn new(key: SecretKey, name: String) -> Self {
        Self::with_identity(Lan::capture(), key, name)
    }
    #[cfg(test)]
    fn with_lan(lan: Lan) -> Self {
        let key = SecretKey::generate();
        let name = super::identity::anonymous(key.public());
        Self::with_identity(lan, key, name)
    }
    fn with_identity(lan: Lan, key: SecretKey, name: String) -> Self {
        Self {
            observed: lan.binds(),
            lan,
            endpoint: None,
            events: None,
            view: Default::default(),
            key,
            name,
            lookup: None,
            active: None,
            pending: false,
            bound: vec![],
            catalog: Default::default(),
            started: None,
            #[cfg(test)]
            test_bind: None,
            #[cfg(test)]
            suppress_discovery: false,
        }
    }
    pub fn rename(&mut self, name: String) {
        self.name = name;
        self.publish();
    }
    pub fn id(&self) -> EndpointId {
        self.key.public()
    }
    fn clear_lookup(&mut self) {
        if let Some(endpoint) = &self.endpoint {
            if let Ok(services) = endpoint.address_lookup() {
                services.clear();
            }
        }
        self.events = None;
        self.lookup = None;
    }
    pub fn request(&mut self, request: u64, receive: bool, busy: bool) {
        self.clear_lookup();
        self.active = Some(receive);
        self.pending = true;
        self.view.request = request;
        self.view.revision = 0;
        self.view.receiving = false;
        self.view.cause.clear();
        self.view.phase = if busy {
            Phase::Pending
        } else {
            Phase::Starting
        };
    }
    pub fn stop(&mut self, request: u64) {
        self.view.request = request;
        self.view.revision = 0;
        self.clear_lookup();
        self.active = None;
        self.pending = false;
        self.started = None;
        self.view.phase = Phase::Off;
        self.view.receiving = false;
        self.catalog = Default::default();
        self.view.peers.clear();
        self.view.seen = 0;
        self.view.filtered = 0;
    }
    pub async fn tick(&mut self, busy: bool) {
        if self.active.is_some() || busy {
            self.lan.refresh();
            let current = self.lan.binds();
            if current != self.observed {
                self.observed = current;
                if self.active.is_some() {
                    self.pending = true;
                    self.clear_lookup();
                }
            }
        }
        if self.pending {
            if busy {
                self.view.phase = Phase::Pending;
            } else {
                self.apply().await;
            }
        }
        self.publish();
    }
    pub async fn connect_ready(&mut self) -> io::Result<Endpoint> {
        self.lan.refresh();
        if self.endpoint.is_none() || self.bound != self.lan.binds() || self.pending {
            if self.active.is_none() {
                self.active = Some(false);
            }
            self.pending = true;
            self.apply().await;
        }
        if !self.view.ready {
            return Err(wire::error(if self.view.cause.is_empty() {
                "No connected LAN interface is available"
            } else {
                &self.view.cause
            }));
        }
        self.endpoint
            .clone()
            .ok_or_else(|| wire::error("No connected LAN interface is available"))
    }
    async fn apply(&mut self) {
        self.clear_lookup();
        self.pending = false;
        self.view.revision = self.view.revision.wrapping_add(1);
        self.view.cause.clear();
        self.catalog = Default::default();
        self.view.peers.clear();
        self.view.seen = 0;
        self.view.filtered = 0;
        self.started = Some(Instant::now());
        let current = self.lan.binds();
        self.observed = current.clone();
        if self.endpoint.is_none() || self.bound != current {
            #[cfg(test)]
            let bind = self.test_bind;
            #[cfg(not(test))]
            let bind = None;
            match service::endpoint_with_key(&self.lan, bind, Some(self.key.clone())).await {
                Ok(endpoint) => {
                    if let Some(old) = self.endpoint.replace(endpoint) {
                        old.close().await;
                    }
                    self.bound = current;
                }
                Err(error) => {
                    if let Some(old) = self.endpoint.take() {
                        old.close().await;
                    }
                    self.bound.clear();
                    self.view.phase = Phase::BindingFailed;
                    self.view.ready = false;
                    self.view.receiving = false;
                    self.view.details.clear();
                    self.view.local_addresses.clear();
                    self.view.cause = error.to_string();
                    return;
                }
            }
        }
        let receive = self.active.unwrap_or(false);
        self.view.receiving = receive;
        #[cfg(test)]
        if self.suppress_discovery {
            self.events = Some(Box::pin(futures_util::stream::pending()));
            self.catalog.prepare(&mut self.view, false, receive);
            return;
        }
        let lookup = MdnsAddressLookup::builder()
            .service_name("dn-transfer-v1")
            .advertise(receive)
            .build(self.id());
        match lookup {
            Ok(lookup) => {
                self.events = Some(Box::pin(lookup.subscribe().await));
                if let Some(endpoint) = &self.endpoint {
                    if let Ok(services) = endpoint.address_lookup() {
                        services.add(service::LocalAdvertisement(lookup.clone()));
                    }
                }
                self.lookup = Some(lookup);
                self.catalog.prepare(&mut self.view, false, receive);
            }
            Err(error) => {
                self.view.phase = Phase::DiscoveryFailed;
                self.view.cause = error.to_string();
            }
        }
    }
    pub fn event(&mut self, event: Option<DiscoveryEvent>) {
        if let Some(event) = event {
            self.catalog.event(event, self.id(), &self.lan);
        } else {
            self.clear_lookup();
            self.view.phase = Phase::DiscoveryFailed;
            self.view.cause = "Discovery stream ended. Refresh to retry.".into();
        }
        self.publish();
    }
    fn publish(&mut self) {
        if let Some(address) = self
            .endpoint
            .as_ref()
            .and_then(|endpoint| self.lan.filter(endpoint.addr()).ok())
        {
            self.view.ready = true;
            self.view.details = lan::card(&address);
            self.view.local_addresses = address
                .ip_addrs()
                .map(|a| format!("{} · {a}", self.lan.describe(a)))
                .collect();
            if let Some(lookup) = &self.lookup {
                iroh::address_lookup::AddressLookup::publish(
                    lookup,
                    &iroh::address_lookup::EndpointData::from_iter(address.addrs.iter().cloned())
                        .with_user_data(
                            format!("dn-name:1:{}", self.name)
                                .parse()
                                .expect("validated bounded nickname"),
                        ),
                );
            }
            if self.events.is_some() {
                self.catalog.prepare(
                    &mut self.view,
                    self.started
                        .is_some_and(|t| t.elapsed() >= Duration::from_secs(5)),
                    self.active == Some(true),
                );
            }
        } else {
            self.view.ready = false;
            self.view.details.clear();
            self.view.local_addresses.clear();
        }
        if self.pending {
            self.view.phase = Phase::Pending;
        }
    }
    pub async fn close(&mut self) {
        self.stop(self.view.request);
        if let Some(endpoint) = self.endpoint.take() {
            endpoint.close().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn network() -> Network {
        let mut network = Network::with_lan(Lan::loopback());
        network.test_bind = Some("127.0.0.1:0".parse().unwrap());
        network.suppress_discovery = true;
        network
    }
    fn event(id: EndpointId, address: &str) -> DiscoveryEvent {
        DiscoveryEvent::Discovered {
            endpoint_info: iroh::address_lookup::EndpointInfo::from_parts(
                id,
                iroh::address_lookup::EndpointData::from_iter([iroh::TransportAddr::Ip(
                    address.parse().unwrap(),
                )]),
            ),
            last_updated: None,
        }
    }
    fn changed_interface(lan: &Lan) {
        let mut interface = netdev::Interface::dummy();
        interface.index = 71;
        interface.ipv4 = vec!["127.0.0.1/8".parse().unwrap()];
        lan.set_interfaces(vec![interface]);
    }
    #[test]
    fn announcements_distinguish_absence_filtering_and_usable_peers_with_bounded_cache() {
        let lan = Lan::loopback();
        let own = SecretKey::from_bytes(&[3; 32]).public();
        let peer = SecretKey::from_bytes(&[4; 32]).public();
        let mut catalog = Catalog::default();
        let mut view = Snapshot::default();
        catalog.prepare(&mut view, true, false);
        assert_eq!(view.phase, Phase::NoAnnouncements);
        catalog.event(event(peer, "10.0.2.15:1234"), own, &lan);
        catalog.prepare(&mut view, false, false);
        assert_eq!(view.phase, Phase::Filtered);
        assert_eq!((view.seen, view.filtered), (1, 1));
        catalog.event(event(peer, "127.0.0.1:1234"), own, &lan);
        catalog.prepare(&mut view, false, false);
        assert_eq!((view.seen, view.filtered, view.peers.len()), (1, 0, 1));
        assert_eq!(view.peers[0].address.id, peer);
        catalog.event(
            DiscoveryEvent::Discovered {
                endpoint_info: iroh::address_lookup::EndpointInfo::from_parts(
                    peer,
                    iroh::address_lookup::EndpointData::from_iter([iroh::TransportAddr::Ip(
                        "127.0.0.1:1234".parse().unwrap(),
                    )])
                    .with_user_data("dn-name:1:Лаптоп".parse().unwrap()),
                ),
                last_updated: None,
            },
            own,
            &lan,
        );
        catalog.prepare(&mut view, false, false);
        assert_eq!(view.peers[0].name, "Лаптоп");
        assert_eq!(
            view.peers[0].address.id, peer,
            "A nickname must not replace the authenticated identity"
        );
        catalog.event(DiscoveryEvent::Expired { endpoint_id: peer }, own, &lan);
        catalog.prepare(&mut view, true, false);
        assert_eq!(view.phase, Phase::NoAnnouncements);
        for n in 0..200u16 {
            let mut bytes = [0; 32];
            bytes[..2].copy_from_slice(&n.to_le_bytes());
            let id = SecretKey::from_bytes(&bytes).public();
            catalog.event(event(id, "10.0.2.15:1234"), own, &lan);
        }
        assert_eq!(catalog.records.len(), 128);
    }
    #[tokio::test]
    async fn refresh_and_interface_changes_defer_while_busy_preserve_identity_and_live_stream(
    ) -> io::Result<()> {
        let mut network = network();
        let id = network.id();
        network.request(1, true, false);
        network.tick(false).await;
        assert!(network.view.ready && network.view.receiving);
        let initial = network.view.details.clone();
        let endpoint = network.endpoint.as_ref().unwrap().clone();
        let remote = service::endpoint(&network.lan, Some("127.0.0.1:0".parse().unwrap())).await?;
        let (client, server) = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(
                remote.connect(endpoint.addr(), super::super::wire::ALPN),
                async { endpoint.accept().await.unwrap().await }
            )
        })
        .await
        .map_err(wire::error)?;
        let client = client.map_err(wire::error)?;
        let server = server.map_err(wire::error)?;
        let deadline_client = client.clone();
        let deadline_server = server.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            deadline_client.close(3u32.into(), b"Test stream deadline");
            deadline_server.close(3u32.into(), b"Test stream deadline");
        });
        let (mut send, _recv) = client.open_bi().await.map_err(wire::error)?;
        send.write_all(b"before").await.map_err(wire::error)?;
        let (_send, mut recv) = server.accept_bi().await.map_err(wire::error)?;
        let mut bytes = [0; 6];
        recv.read_exact(&mut bytes).await.map_err(wire::error)?;
        assert_eq!(&bytes, b"before");
        changed_interface(&network.lan);
        network.tick(true).await;
        assert_eq!(network.view.phase, Phase::Pending);
        assert_eq!(network.view.details, initial);
        network.request(2, true, true);
        network.tick(true).await;
        assert_eq!(network.view.phase, Phase::Pending);
        assert_eq!(network.id(), id);
        send.write_all(b"after!").await.map_err(wire::error)?;
        recv.read_exact(&mut bytes).await.map_err(wire::error)?;
        assert_eq!(&bytes, b"after!");
        client.close(0u32.into(), b"done");
        server.close(0u32.into(), b"done");
        network.tick(false).await;
        assert!(network.view.ready);
        assert_eq!(network.id(), id);
        assert_eq!(network.view.request, 2);
        assert_ne!(network.view.details, initial);
        let current = network.view.details.clone();
        network.request(3, true, false);
        network.tick(false).await;
        assert_eq!(network.view.details, current);
        assert_eq!(network.id(), id);
        network.stop(4);
        assert!(!network.view.receiving);
        assert!(network.events.is_none());
        assert_eq!(network.id(), id);
        network.request(5, false, false);
        network.tick(false).await;
        assert_eq!(network.id(), id);
        assert_eq!(network.view.phase, Phase::Searching);
        network.event(None);
        assert_eq!(network.view.phase, Phase::DiscoveryFailed);
        assert!(network.view.ready);
        assert!(network.events.is_none());
        remote.close().await;
        network.close().await;
        Ok(())
    }
    #[tokio::test]
    async fn unavailable_binding_can_be_refreshed_without_losing_session_identity() {
        let mut network = network();
        let id = network.id();
        network.test_bind = None;
        network.request(1, true, false);
        network.tick(false).await;
        assert_eq!(network.view.phase, Phase::BindingFailed);
        assert!(!network.view.ready);
        assert!(!network.view.cause.is_empty());
        network.test_bind = Some("127.0.0.1:0".parse().unwrap());
        network.request(2, true, false);
        network.tick(false).await;
        assert!(network.view.ready);
        assert_eq!(network.id(), id);
        assert!(network.view.cause.is_empty());
        network.close().await;
    }
}
