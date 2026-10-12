//! How other devices on the network reach the embedded web server: the PC's
//! LAN addresses, and (desktop builds) an mDNS name so people can type
//! `mooshieui.local` instead of an IP.
//!
//! `.local` is the only suffix every OS resolves over multicast DNS with no
//! router or DNS server setup, so a name like `mooshie.ui` cannot work locally.
//! The IP addresses stay on show for devices whose resolver skips mDNS (some
//! Android builds and locked-down Windows machines).

use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::sync::Mutex;

/// mDNS host label, advertised as `mooshieui.local`.
pub const MDNS_HOST_LABEL: &str = "mooshieui";

/// What the running web server is bound to, recorded by `webserver::start_server`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerEndpoint {
    /// The port actually bound (may differ from `ui_server_port` after a fallback).
    pub port: u16,
    pub https: bool,
    /// Bound on all interfaces. False until the app restarts after LAN access
    /// is switched on, since the listener is bound once at startup.
    pub lan: bool,
}

struct LanState {
    endpoint: Option<ServerEndpoint>,
    /// The advertised mDNS host name without the trailing dot, e.g.
    /// `mooshieui.local`, or `mooshieui-2.local` after a name conflict.
    hostname: Option<String>,
    #[cfg(feature = "desktop")]
    daemon: Option<mdns_sd::ServiceDaemon>,
}

static STATE: Mutex<LanState> = Mutex::new(LanState {
    endpoint: None,
    hostname: None,
    #[cfg(feature = "desktop")]
    daemon: None,
});

fn lock_state() -> std::sync::MutexGuard<'static, LanState> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Record the bound endpoint and, when the server listens on the LAN in a
/// desktop build, advertise it over mDNS.
pub fn server_started(endpoint: ServerEndpoint) {
    lock_state().endpoint = Some(endpoint);
    // Hosted/server builds run in containers or on remote hosts where
    // multicast never reaches the people using them.
    #[cfg(feature = "desktop")]
    if endpoint.lan {
        advertise(endpoint);
    }
}

/// The recorded endpoint and the current mDNS host name, if any.
pub fn snapshot() -> (Option<ServerEndpoint>, Option<String>) {
    let state = lock_state();
    (state.endpoint, state.hostname.clone())
}

/// `http(s)://host:port` for display.
pub fn url_for(host: &str, endpoint: &ServerEndpoint) -> String {
    let scheme = if endpoint.https { "https" } else { "http" };
    format!("{scheme}://{host}:{}", endpoint.port)
}

/// LAN IPv4 addresses other devices can use, best guess first.
pub fn lan_ipv4_addrs() -> Vec<Ipv4Addr> {
    rank_lan_addrs(default_route_ipv4(), up_ipv4_interfaces())
}

/// The source address the OS would use for the default route. Connecting a
/// UDP socket sends nothing; it only resolves the route.
fn default_route_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// `(interface name, address)` for every up, non-loopback, non-tunnel IPv4 interface.
fn up_ipv4_interfaces() -> Vec<(String, Ipv4Addr)> {
    let Ok(ifaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    ifaces
        .into_iter()
        .filter(|i| {
            !i.is_loopback()
                && !i.is_p2p
                && !matches!(
                    i.oper_status,
                    if_addrs::IfOperStatus::Down | if_addrs::IfOperStatus::NotPresent
                )
        })
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(v4) => Some((i.name, v4.ip)),
            if_addrs::IfAddr::V6(_) => None,
        })
        .collect()
}

/// Adapters that carry a private address but never face the home network:
/// VM host-only networks, container bridges, overlay VPNs.
fn is_virtual_adapter(name: &str) -> bool {
    const MARKERS: [&str; 14] = [
        "vethernet",
        "virtualbox",
        "vboxnet",
        "vmware",
        "vmnet",
        "hyper-v",
        "wsl",
        "docker",
        "br-",
        "veth",
        "virbr",
        "zerotier",
        "tailscale",
        "utun",
    ];
    let name = name.to_ascii_lowercase();
    MARKERS.iter().any(|m| name.contains(m))
}

/// Order candidate addresses for display. The default-route address comes
/// first whatever its adapter is called (a Hyper-V external switch names the
/// real NIC "vEthernet (...)"), then other private addresses on physical
/// adapters. A public default-route address is shown only when nothing
/// private exists, matching a PC plugged straight into a modem.
fn rank_lan_addrs(
    primary: Option<Ipv4Addr>,
    ifaces: impl IntoIterator<Item = (String, Ipv4Addr)>,
) -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = Vec::new();
    if let Some(ip) = primary.filter(|ip| ip.is_private()) {
        out.push(ip);
    }
    for (name, ip) in ifaces {
        if ip.is_private() && !is_virtual_adapter(&name) && !out.contains(&ip) {
            out.push(ip);
        }
    }
    if out.is_empty() {
        out.extend(primary.filter(|ip| !ip.is_link_local()));
    }
    out
}

#[cfg(feature = "desktop")]
fn advertise(endpoint: ServerEndpoint) {
    use mdns_sd::{DaemonEvent, IfKind, ServiceDaemon, ServiceInfo};

    let mut state = lock_state();
    if let Some(old) = state.daemon.take() {
        let _ = old.shutdown();
    }
    state.hostname = None;

    // Every IPv4 address on the box; the daemon answers each interface with
    // only the addresses in that interface's subnet. IPv6 is left out because
    // the web server binds 0.0.0.0, so an AAAA answer would point at nothing.
    let addrs: Vec<IpAddr> = up_ipv4_interfaces()
        .into_iter()
        .map(|(_, ip)| IpAddr::V4(ip))
        .collect();
    if addrs.is_empty() {
        log::info!("mDNS: no IPv4 interfaces, not advertising");
        return;
    }

    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            log::warn!("mDNS: could not start responder: {e}");
            return;
        }
    };
    // Loopback answers would hand 127.0.0.1 to nobody useful.
    let _ = daemon.disable_interface(vec![IfKind::LoopbackV4, IfKind::LoopbackV6]);

    let host_fqdn = format!("{MDNS_HOST_LABEL}.local.");
    let service_type = if endpoint.https {
        "_https._tcp.local."
    } else {
        "_http._tcp.local."
    };
    let info = match ServiceInfo::new(
        service_type,
        "MooshieUI",
        &host_fqdn,
        addrs.as_slice(),
        endpoint.port,
        &[("path", "/")][..],
    ) {
        Ok(i) => i,
        Err(e) => {
            log::warn!("mDNS: invalid service info: {e}");
            let _ = daemon.shutdown();
            return;
        }
    };

    // Watch for a name conflict (another PC already answering for
    // mooshieui.local); the daemon renames ours to mooshieui-2.local.
    if let Ok(events) = daemon.monitor() {
        std::thread::Builder::new()
            .name("mdns-monitor".into())
            .spawn(move || {
                while let Ok(event) = events.recv() {
                    if let DaemonEvent::NameChange(change) = event {
                        if change.original != host_fqdn {
                            continue;
                        }
                        let renamed = change.new_name.trim_end_matches('.').to_string();
                        log::warn!(
                            "mDNS: {} is taken on this network, using {renamed}",
                            change.original.trim_end_matches('.')
                        );
                        lock_state().hostname = Some(renamed);
                    }
                }
            })
            .ok();
    }

    match daemon.register(info) {
        Ok(()) => {
            let host = format!("{MDNS_HOST_LABEL}.local");
            log::info!("mDNS: advertising {}", url_for(&host, &endpoint));
            state.hostname = Some(host);
            state.daemon = Some(daemon);
        }
        Err(e) => {
            log::warn!("mDNS: register failed: {e}");
            let _ = daemon.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    fn iface(name: &str, addr: &str) -> (String, Ipv4Addr) {
        (name.to_string(), ip(addr))
    }

    #[test]
    fn default_route_comes_first_and_is_not_repeated() {
        let ranked = rank_lan_addrs(
            Some(ip("192.168.1.20")),
            vec![
                iface("Wi-Fi", "10.0.0.5"),
                iface("Ethernet", "192.168.1.20"),
            ],
        );
        assert_eq!(ranked, vec![ip("192.168.1.20"), ip("10.0.0.5")]);
    }

    #[test]
    fn virtual_adapters_are_skipped() {
        let ranked = rank_lan_addrs(
            Some(ip("192.168.1.20")),
            vec![
                iface("vEthernet (WSL (Hyper-V firewall))", "172.28.80.1"),
                iface("VirtualBox Host-Only Network", "192.168.56.1"),
                iface("docker0", "172.17.0.1"),
                iface("Ethernet", "192.168.1.20"),
            ],
        );
        assert_eq!(ranked, vec![ip("192.168.1.20")]);
    }

    #[test]
    fn default_route_on_a_virtual_switch_is_kept() {
        // A Hyper-V external switch takes over the physical NIC's name.
        let ranked = rank_lan_addrs(
            Some(ip("192.168.0.8")),
            vec![iface("vEthernet (External)", "192.168.0.8")],
        );
        assert_eq!(ranked, vec![ip("192.168.0.8")]);
    }

    #[test]
    fn public_and_cgnat_addresses_are_not_listed_beside_private_ones() {
        let ranked = rank_lan_addrs(
            Some(ip("192.168.1.20")),
            vec![
                iface("Ethernet", "192.168.1.20"),
                iface("Ethernet 2", "100.101.102.103"),
                iface("Ethernet 3", "203.0.113.9"),
            ],
        );
        assert_eq!(ranked, vec![ip("192.168.1.20")]);
    }

    #[test]
    fn public_default_route_is_the_fallback() {
        let ranked = rank_lan_addrs(Some(ip("203.0.113.9")), Vec::new());
        assert_eq!(ranked, vec![ip("203.0.113.9")]);
        assert!(rank_lan_addrs(None, Vec::new()).is_empty());
        assert!(rank_lan_addrs(Some(ip("169.254.3.4")), Vec::new()).is_empty());
    }

    #[test]
    fn private_interfaces_are_listed_without_a_default_route() {
        let ranked = rank_lan_addrs(None, vec![iface("en0", "192.168.50.2")]);
        assert_eq!(ranked, vec![ip("192.168.50.2")]);
    }

    #[test]
    fn url_uses_scheme_and_port() {
        let ep = ServerEndpoint {
            port: 3200,
            https: false,
            lan: true,
        };
        assert_eq!(
            url_for("mooshieui.local", &ep),
            "http://mooshieui.local:3200"
        );
        let tls = ServerEndpoint { https: true, ..ep };
        assert_eq!(url_for("192.168.1.2", &tls), "https://192.168.1.2:3200");
    }
}
