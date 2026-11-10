//! Read-only local metadata; no external connectivity probes or DNS queries.
use crate::utilities::information::Fields;
use std::path::Path;
const INTERFACES: usize = 64;
const ADDRESSES: usize = 16;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceKey {
    pub index: u32,
    pub name: String,
}
#[derive(Clone, Debug)]
pub struct Adapter {
    pub key: InterfaceKey,
    pub label: String,
    pub fields: Fields,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub adapters: Vec<Adapter>,
    pub notices: Fields,
}
impl Snapshot {
    pub fn details(&self, selected: usize) -> Fields {
        let mut fields = self.notices.clone();
        if let Some(adapter) = self.adapters.get(selected) {
            fields.extend(adapter.fields.clone());
        }
        fields
    }
}
pub fn collect() -> Fields {
    snapshot().details(0)
}
pub fn snapshot() -> Snapshot {
    let mut fields = vec![(
        "Internet reachability".into(),
        "Not probed; interface state is not Internet connectivity".into(),
    )];
    let mut interfaces = netdev::get_interfaces();
    interfaces.sort_by(|a, b| {
        b.default
            .cmp(&a.default)
            .then_with(|| a.index.cmp(&b.index))
    });
    if interfaces.is_empty() {
        fields.push((
            "Network interfaces".into(),
            "Unavailable or no interfaces reported".into(),
        ));
    }
    if interfaces.len() > INTERFACES {
        fields.push((
            "Network interfaces".into(),
            format!("{} / {}", INTERFACES, interfaces.len()),
        ));
    }
    let notices = fields;
    let mut adapters = Vec::new();
    for interface in interfaces.into_iter().take(INTERFACES) {
        let mut fields = Vec::new();
        let key = InterfaceKey {
            index: interface.index,
            name: interface.name.clone(),
        };
        let label = interface
            .friendly_name
            .as_ref()
            .filter(|name| *name != &interface.name)
            .map_or_else(
                || interface.name.clone(),
                |name| format!("{name} ({})", interface.name),
            );
        fields.push((
            "Interface".into(),
            format!(
                "{}{}",
                interface.name,
                if interface.default {
                    " (default route)"
                } else {
                    ""
                }
            ),
        ));
        fields.push(("Interface type".into(), format!("{:?}", interface.if_type)));
        fields.push((
            "Address protocols".into(),
            match (!interface.ipv4.is_empty(), !interface.ipv6.is_empty()) {
                (true, true) => "IPv4 / IPv6",
                (true, false) => "IPv4",
                (false, true) => "IPv6",
                _ => "No assigned addresses",
            }
            .into(),
        ));
        fields.push((
            "Link state".into(),
            if interface.is_oper_up() {
                "Up"
            } else {
                "Down or unknown"
            }
            .into(),
        ));
        fields.push((
            "MAC address".into(),
            interface
                .mac_addr
                .map_or_else(|| "Unavailable".into(), |v| v.to_string()),
        ));
        fields.push((
            "IPv4".into(),
            interface
                .ipv4
                .iter()
                .take(ADDRESSES)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        ));
        fields.push((
            "IPv6".into(),
            interface
                .ipv6
                .iter()
                .zip(interface.ipv6_scope_ids.iter().chain(std::iter::repeat(&0)))
                .take(ADDRESSES)
                .map(|(ip, scope)| {
                    if *scope == 0 {
                        ip.to_string()
                    } else {
                        format!("{ip} (scope {scope})")
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
        ));
        fields.push((
            "Default gateway".into(),
            interface.gateway.map_or_else(
                || "Unavailable".into(),
                |g| {
                    g.ipv4
                        .iter()
                        .map(ToString::to_string)
                        .chain(g.ipv6.iter().map(ToString::to_string))
                        .collect::<Vec<_>>()
                        .join(", ")
                },
            ),
        ));
        fields.push((
            "DNS servers".into(),
            interface
                .dns_servers
                .iter()
                .take(ADDRESSES)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        ));
        fields.push((
            "MTU".into(),
            interface
                .mtu
                .map_or_else(|| "Unavailable".into(), |n| n.to_string()),
        ));
        adapters.push(Adapter { key, label, fields });
    }
    Snapshot { adapters, notices }
}
pub fn paths(config: Option<&Path>) -> Fields {
    let mut fields = Vec::new();
    for (label, value) in [
        (
            "Home directory",
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(std::path::PathBuf::from),
        ),
        ("Executable path", std::env::current_exe().ok()),
        ("Temporary directory", Some(std::env::temp_dir())),
        ("Configuration file", config.map(Path::to_path_buf)),
        (
            "Recovery directory",
            config.map(|p| p.with_file_name("editor-recovery")),
        ),
        (
            "Temp Drive directory",
            config.map(|p| p.with_file_name("temp-drive")),
        ),
    ] {
        fields.push((
            label.into(),
            value.map_or_else(
                || "Unavailable".into(),
                |p| p.to_string_lossy().into_owned(),
            ),
        ));
    }
    fields
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_are_contextual_and_metadata_is_bounded_without_connectivity_claims() {
        let path = std::env::temp_dir().join("DN 世界/config.conf");
        let snapshot = snapshot();
        assert!(snapshot.adapters.len() <= INTERFACES);
        assert!(snapshot
            .adapters
            .iter()
            .all(|adapter| adapter.fields.len() == 10));
        let mut fields = snapshot.details(0);
        assert!(fields.iter().filter(|(key, _)| key == "Interface").count() <= 1);
        fields.extend(paths(Some(&path)));
        assert!(fields.len() <= 1 + 1 + 10 + 6);
        assert!(fields
            .iter()
            .any(|(k, v)| k == "Configuration file" && v == &path.to_string_lossy()));
        assert!(fields
            .iter()
            .any(|(k, v)| k == "Internet reachability" && v.starts_with("Not probed")));
    }
}
