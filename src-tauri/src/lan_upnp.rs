//! Opt-in router port forwarding (UPnP IGD) so people outside the home
//! network can open LAN mode at the router's public address.
//!
//! Off by default: it puts the sign-in page on the internet. The mapping
//! points the same external port at this PC's LAN address, uses a one-hour
//! lease renewed every half hour (so a crash leaves nothing open for long),
//! and is removed when the setting is turned off or the app exits. Routers
//! that only accept permanent leases get one, still removed on exit.
//!
//! Desktop builds only: hosted/server builds run in containers or on remote
//! hosts where the router is not theirs to configure.

use serde::Serialize;
use std::net::Ipv4Addr;
use std::sync::Mutex;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpnpState {
    Off,
    /// Looking for the router or asking it for the mapping.
    Pending,
    Mapped,
    Failed,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpnpError {
    /// The setting is on but the server still listens on this PC only (LAN
    /// access was switched on without a restart).
    NotListening,
    /// No router answered UPnP discovery (UPnP off on the router, or none).
    NoRouter,
    /// The router refused or failed the mapping request.
    Refused,
    /// Another device already holds this external port on the router.
    PortInUse,
    /// The router's own "public" address is private or carrier-grade NAT, so
    /// a mapping on it would not reach the internet.
    DoubleNat,
    /// Not available in this build.
    Unsupported,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct UpnpStatus {
    pub state: UpnpState,
    /// `http(s)://<public ip>:<port>` once mapped, when the router reports
    /// its public address.
    pub external_url: Option<String>,
    pub error: Option<UpnpError>,
}

impl UpnpStatus {
    const fn off() -> Self {
        Self {
            state: UpnpState::Off,
            external_url: None,
            error: None,
        }
    }

    #[cfg_attr(not(feature = "desktop"), allow(dead_code))]
    const fn pending() -> Self {
        Self {
            state: UpnpState::Pending,
            external_url: None,
            error: None,
        }
    }

    const fn failed(error: UpnpError) -> Self {
        Self {
            state: UpnpState::Failed,
            external_url: None,
            error: Some(error),
        }
    }
}

struct Inner {
    status: UpnpStatus,
    /// `(port, https)` the running task maps, so repeat calls are no-ops.
    running: Option<(u16, bool)>,
    /// Bumped on every change, so a cancelled task can't overwrite the
    /// status of the one that replaced it.
    generation: u64,
    /// Dropping this tells the task to remove its mapping and exit.
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

static STATE: Mutex<Inner> = Mutex::new(Inner {
    status: UpnpStatus::off(),
    running: None,
    generation: 0,
    cancel: None,
    task: None,
});

fn lock() -> std::sync::MutexGuard<'static, Inner> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn status() -> UpnpStatus {
    lock().status.clone()
}

/// Start, keep or stop the mapping to match the setting and the running web
/// server. Call after the server binds and whenever the setting changes.
/// Must run inside a Tokio runtime.
pub fn sync(enabled: bool) {
    let (endpoint, _) = crate::lan_discovery::snapshot();
    let want = endpoint
        .filter(|ep| enabled && ep.lan)
        .map(|ep| (ep.port, ep.https));
    let mut st = lock();
    if want.is_some() && want == st.running {
        return;
    }
    st.generation += 1;
    // The old task removes its mapping on its own once cancelled.
    st.cancel = None;
    st.task = None;
    st.running = want;
    st.status = match want {
        None if enabled && endpoint.is_some() => UpnpStatus::failed(UpnpError::NotListening),
        None => UpnpStatus::off(),
        Some((port, https)) => start(&mut st, port, https),
    };
}

#[cfg(feature = "desktop")]
fn start(st: &mut Inner, port: u16, https: bool) -> UpnpStatus {
    let (tx, rx) = tokio::sync::oneshot::channel();
    st.cancel = Some(tx);
    st.task = Some(tokio::spawn(desktop::run(port, https, st.generation, rx)));
    UpnpStatus::pending()
}

#[cfg(not(feature = "desktop"))]
fn start(_st: &mut Inner, _port: u16, _https: bool) -> UpnpStatus {
    UpnpStatus::failed(UpnpError::Unsupported)
}

/// Remove the mapping before the app exits. Waits a few seconds at most.
pub async fn shutdown() {
    let task = {
        let mut st = lock();
        st.generation += 1;
        st.cancel = None;
        st.running = None;
        st.status = UpnpStatus::off();
        st.task.take()
    };
    if let Some(task) = task {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), task).await;
    }
}

#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
fn set_status(generation: u64, status: UpnpStatus) {
    let mut st = lock();
    if st.generation == generation {
        st.status = status;
    }
}

/// A router WAN address that the internet can't reach directly: private,
/// carrier-grade NAT (100.64.0.0/10), loopback, link-local or unspecified.
#[cfg_attr(not(any(feature = "desktop", test)), allow(dead_code))]
fn is_unroutable(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_private()
        || (a == 100 && (64..128).contains(&b))
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
}

#[cfg(feature = "desktop")]
mod desktop {
    use super::{is_unroutable, set_status, UpnpError, UpnpState, UpnpStatus};
    use igd_next::aio::tokio::search_gateway;
    use igd_next::aio::Gateway;
    use igd_next::{AddPortError, PortMappingProtocol, SearchOptions};
    use std::net::{IpAddr, SocketAddr, UdpSocket};
    use std::time::Duration;

    /// Lease in seconds; renewed at half-time.
    const LEASE_SECS: u32 = 3600;
    const DESCRIPTION: &str = "MooshieUI";

    pub(super) async fn run(
        port: u16,
        https: bool,
        generation: u64,
        mut cancel: tokio::sync::oneshot::Receiver<()>,
    ) {
        let mut options = SearchOptions::default();
        options.timeout = Some(Duration::from_secs(5));
        let gateway = tokio::select! {
            found = search_gateway(options) => found,
            _ = &mut cancel => return,
        };
        let gateway = match gateway {
            Ok(g) => g,
            Err(e) => {
                log::info!("UPnP: no router found: {e}");
                set_status(generation, UpnpStatus::failed(UpnpError::NoRouter));
                return;
            }
        };
        let external_ip = gateway.get_external_ip().await.ok();
        if let Some(IpAddr::V4(ip)) = external_ip {
            if is_unroutable(ip) {
                log::info!("UPnP: router's WAN address {ip} is not public; not mapping");
                set_status(generation, UpnpStatus::failed(UpnpError::DoubleNat));
                return;
            }
        }
        let Some(local_ip) = local_ip_toward(gateway.addr) else {
            set_status(generation, UpnpStatus::failed(UpnpError::NoRouter));
            return;
        };
        let local = SocketAddr::new(local_ip, port);
        let external_url = external_ip.map(|ip| {
            let scheme = if https { "https" } else { "http" };
            format!("{scheme}://{ip}:{port}")
        });

        let mut lease = LEASE_SECS;
        loop {
            if let Err(e) = add_mapping(&gateway, port, local, &mut lease).await {
                log::warn!("UPnP: mapping port {port} failed: {e}");
                set_status(generation, UpnpStatus::failed(classify(&e)));
                return;
            }
            set_status(
                generation,
                UpnpStatus {
                    state: UpnpState::Mapped,
                    external_url: external_url.clone(),
                    error: None,
                },
            );
            let renew = async {
                if lease == 0 {
                    std::future::pending::<()>().await;
                }
                tokio::time::sleep(Duration::from_secs(u64::from(lease / 2))).await;
            };
            tokio::select! {
                _ = renew => continue,
                _ = &mut cancel => break,
            }
        }
        match gateway.remove_port(PortMappingProtocol::TCP, port).await {
            Ok(()) => log::info!("UPnP: removed mapping for port {port}"),
            Err(e) => log::warn!("UPnP: could not remove mapping for port {port}: {e}"),
        }
    }

    async fn add_mapping(
        gateway: &Gateway<igd_next::aio::tokio::Tokio>,
        port: u16,
        local: SocketAddr,
        lease: &mut u32,
    ) -> Result<(), AddPortError> {
        let tcp = PortMappingProtocol::TCP;
        match gateway
            .add_port(tcp, port, local, *lease, DESCRIPTION)
            .await
        {
            Err(AddPortError::OnlyPermanentLeasesSupported) if *lease != 0 => {
                *lease = 0;
                gateway.add_port(tcp, port, local, 0, DESCRIPTION).await
            }
            other => other,
        }
    }

    fn classify(e: &AddPortError) -> UpnpError {
        match e {
            AddPortError::PortInUse | AddPortError::SamePortValuesRequired => UpnpError::PortInUse,
            _ => UpnpError::Refused,
        }
    }

    /// This PC's address on the router's subnet. Connecting a UDP socket
    /// sends nothing; it only picks the route.
    fn local_ip_toward(gateway: SocketAddr) -> Option<IpAddr> {
        let socket = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
        socket.connect(gateway).ok()?;
        let ip = socket.local_addr().ok()?.ip();
        (!ip.is_unspecified()).then_some(ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_cgnat_wan_addresses_are_unroutable() {
        for ip in [
            "192.168.1.1",
            "10.0.0.1",
            "172.16.5.4",
            "100.64.0.1",
            "100.127.255.254",
        ] {
            assert!(is_unroutable(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["203.0.113.7", "100.63.255.255", "100.128.0.1", "8.8.8.8"] {
            assert!(!is_unroutable(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn status_serializes_for_the_settings_page() {
        let json = serde_json::to_value(UpnpStatus::failed(UpnpError::DoubleNat)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "state": "failed", "external_url": null, "error": "double_nat" })
        );
    }
}
