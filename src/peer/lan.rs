//! Only directly connected interfaces; no public DNS, STUN or gateway probes.
use iroh::{EndpointAddr, TransportAddr};
use std::{io, net::SocketAddr};
#[derive(Clone)]
pub struct Lan {
    interfaces: std::sync::Arc<std::sync::RwLock<Vec<netdev::Interface>>>,
    loopback: bool,
}
impl Lan {
    pub fn capture() -> Self {
        Self {
            interfaces: std::sync::Arc::new(std::sync::RwLock::new(Self::interfaces())),
            loopback: false,
        }
    }
    #[cfg(test)]
    pub fn loopback() -> Self {
        Self {
            interfaces: Default::default(),
            loopback: true,
        }
    }
    fn interfaces() -> Vec<netdev::Interface> {
        netdev::get_interfaces()
            .into_iter()
            .filter(|i| i.is_up() && !i.is_loopback())
            .collect()
    }
    pub fn refresh(&self) {
        if !self.loopback {
            let current = Self::interfaces();
            if let Ok(mut interfaces) = self.interfaces.write() {
                *interfaces = current;
            }
        }
    }
    pub fn allows(&self, address: &SocketAddr) -> bool {
        let ip = address.ip();
        if ip.is_unspecified() || ip.is_multicast() || address.port() == 0 {
            return false;
        }
        if ip.is_loopback() {
            return self.loopback;
        }
        let Ok(interfaces) = self.interfaces.read() else {
            return false;
        };
        interfaces.iter().any(|i| match address {
            SocketAddr::V4(a) => i
                .ipv4
                .iter()
                .any(|net| net.prefix_len() != 0 && net.contains(a.ip())),
            SocketAddr::V6(a) => {
                i.ipv6
                    .iter()
                    .any(|net| net.prefix_len() != 0 && net.contains(a.ip()))
                    && (!a.ip().is_unicast_link_local() || a.scope_id() == i.index)
            }
        })
    }
    /// Bound sockets have connected-prefix routes only, never a default route.
    pub fn binds(&self) -> Vec<(SocketAddr, u8)> {
        let Ok(interfaces) = self.interfaces.read() else {
            return vec![];
        };
        let mut binds = std::collections::BTreeSet::new();
        for i in interfaces.iter() {
            for net in &i.ipv4 {
                if net.prefix_len() > 0
                    && !net.addr().is_unspecified()
                    && !net.addr().is_multicast()
                {
                    binds.insert((SocketAddr::from((net.addr(), 0)), net.prefix_len()));
                }
            }
            for net in &i.ipv6 {
                if net.prefix_len() > 0
                    && !net.addr().is_unspecified()
                    && !net.addr().is_multicast()
                {
                    let scope = if net.addr().is_unicast_link_local() {
                        i.index
                    } else {
                        0
                    };
                    binds.insert((
                        SocketAddr::V6(std::net::SocketAddrV6::new(net.addr(), 0, 0, scope)),
                        net.prefix_len(),
                    ));
                }
            }
        }
        binds.into_iter().take(64).collect()
    }
    /// DNS AAAA records cannot carry a local interface index. Try only eligible
    /// connected scopes; TLS still authenticates the selected endpoint identity.
    pub fn discovered(&self, mut address: EndpointAddr) -> io::Result<EndpointAddr> {
        let unscoped = address
            .ip_addrs()
            .filter_map(|a| match a {
                SocketAddr::V6(a) if a.ip().is_unicast_link_local() && a.scope_id() == 0 => {
                    Some(*a)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Ok(interfaces) = self.interfaces.read() {
            for a in unscoped {
                for i in interfaces.iter().take(64) {
                    if i.ipv6
                        .iter()
                        .any(|n| n.prefix_len() > 0 && n.contains(a.ip()))
                    {
                        let mut scoped = a;
                        scoped.set_scope_id(i.index);
                        address = address.with_ip_addr(SocketAddr::V6(scoped));
                    }
                }
            }
        }
        self.filter(address)
    }
    pub fn filter(&self, mut addr: EndpointAddr) -> io::Result<EndpointAddr> {
        addr.addrs
            .retain(|a| matches!(a, TransportAddr::Ip(ip) if self.allows(ip)));
        addr.addrs = addr.addrs.into_iter().take(16).collect();
        if addr.addrs.is_empty() {
            return Err(io::Error::other(
                "Peer has no reachable address on a connected LAN",
            ));
        }
        Ok(addr)
    }
    pub fn connection(&self, connection: &iroh::endpoint::Connection) -> io::Result<()> {
        if connection
            .paths()
            .iter()
            .any(|p| !matches!(p.remote_addr(), TransportAddr::Ip(a) if self.allows(a)))
        {
            return Err(io::Error::other("Peer connection left the permitted LAN"));
        }
        Ok(())
    }
}
pub fn card(addr: &EndpointAddr) -> String {
    format!(
        "dn-peer:1:{}@{}",
        addr.id,
        addr.ip_addrs()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}
pub fn parse(value: &str, lan: &Lan) -> io::Result<EndpointAddr> {
    if value.len() > 4096 {
        return Err(io::Error::other("Peer connection details exceed limits"));
    }
    let rest = value
        .strip_prefix("dn-peer:1:")
        .ok_or_else(|| io::Error::other("Use DN peer connection details (dn-peer:1:...)"))?;
    let (id, addresses) = rest
        .split_once('@')
        .ok_or_else(|| io::Error::other("Missing peer identity or address"))?;
    let id = id.parse().map_err(io::Error::other)?;
    let mut addr = EndpointAddr::new(id);
    for a in addresses.split(',').take(17) {
        let ip: SocketAddr = a.parse().map_err(io::Error::other)?;
        if !lan.allows(&ip) {
            return Err(io::Error::other(
                "Manual peer address must be on a connected LAN",
            ));
        }
        addr = addr.with_ip_addr(ip);
    }
    if addr.addrs.len() > 16 {
        return Err(io::Error::other("Too many peer addresses"));
    }
    lan.filter(addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_details_are_literal_bounded_and_lan_only() {
        let id = iroh::SecretKey::from_bytes(&[7; 32]).public();
        let lan = Lan::loopback();
        let address = EndpointAddr::new(id).with_ip_addr("127.0.0.1:12345".parse().unwrap());
        assert_eq!(parse(&card(&address), &lan).unwrap(), address);
        for text in [
            format!("dn-peer:1:{id}@8.8.8.8:12345"),
            format!("dn-peer:1:{id}@0.0.0.0:12345"),
            format!("dn-peer:1:{id}@localhost:12345"),
            format!("dn-peer:2:{id}@127.0.0.1:12345"),
        ] {
            assert!(parse(&text, &lan).is_err());
        }
    }
    #[test]
    fn connected_prefixes_ipv6_scopes_and_replaced_interfaces_are_enforced() {
        let mut interface = netdev::Interface::dummy();
        interface.index = 42;
        interface.ipv4 = vec!["192.168.2.4/24".parse().unwrap()];
        interface.ipv6 = vec![
            "fe80::4/64".parse().unwrap(),
            "2001:db8:1::4/64".parse().unwrap(),
        ];
        let lan = Lan {
            interfaces: std::sync::Arc::new(std::sync::RwLock::new(vec![interface])),
            loopback: false,
        };
        assert!(lan.allows(&"192.168.2.9:1234".parse().unwrap()));
        assert!(!lan.allows(&"192.168.3.9:1234".parse().unwrap()));
        assert!(lan.allows(&"[fe80::5%42]:1234".parse().unwrap()));
        assert!(!lan.allows(&"[fe80::5%43]:1234".parse().unwrap()));
        assert!(!lan.allows(&"[fe80::5]:1234".parse().unwrap()));
        assert!(lan.allows(&"[2001:db8:1::5]:1234".parse().unwrap()));
        assert!(!lan.allows(&"[2001:db8:2::5]:1234".parse().unwrap()));
        let id = iroh::SecretKey::from_bytes(&[9; 32]).public();
        let unscoped = EndpointAddr::new(id).with_ip_addr("[fe80::5]:1234".parse().unwrap());
        assert!(lan.filter(unscoped.clone()).is_err());
        let scoped = lan.discovered(unscoped).unwrap();
        assert!(scoped
            .ip_addrs()
            .any(|a| matches!(a,SocketAddr::V6(a) if a.scope_id()==42)));
        assert!(lan
            .binds()
            .iter()
            .any(|(a, prefix)| matches!(a,SocketAddr::V6(a) if a.scope_id()==42) && *prefix == 64));
        let shared = lan.clone();
        lan.interfaces.write().unwrap().clear();
        assert!(!shared.allows(&"192.168.2.9:1234".parse().unwrap()));
    }
}
