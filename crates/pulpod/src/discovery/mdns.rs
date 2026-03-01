use std::net::IpAddr;

use anyhow::{Context, Result};
use mdns_sd::{ServiceDaemon, ServiceInfo};

use super::{SERVICE_TYPE, ServiceRegistration};

/// Build a `ServiceInfo` from the given registration parameters.
///
/// This is separated from `register()` for testability — the `ServiceInfo`
/// construction logic can be verified without touching the network.
pub fn build_service_info(reg: &ServiceRegistration) -> Result<ServiceInfo> {
    let txt = reg.txt_properties();
    let props: Vec<(&str, &str)> = txt.iter().map(|(k, v)| (*k, v.as_str())).collect();
    ServiceInfo::new(
        SERVICE_TYPE,
        &reg.node_name,
        &reg.hostname(),
        "",
        reg.port,
        props.as_slice(),
    )
    .context("Failed to build mDNS service info")
}

/// Extract the instance name from an mDNS fullname.
///
/// E.g. `"mac-mini._pulpo._tcp.local."` → `Some("mac-mini")`.
pub fn parse_instance_name(fullname: &str) -> Option<&str> {
    fullname.strip_suffix(&format!(".{SERVICE_TYPE}"))
}

/// Format a peer address from an IP and port.
pub fn format_peer_address(ip: &IpAddr, port: u16) -> String {
    match ip {
        IpAddr::V4(v4) => format!("{v4}:{port}"),
        IpAddr::V6(v6) => format!("[{v6}]:{port}"),
    }
}

/// Handle to a running mDNS registration. Dropping it unregisters the service.
///
/// Excluded from coverage builds because it requires real network I/O
/// (spawns an mDNS daemon thread). All testable logic is in [`build_service_info`].
#[cfg(not(coverage))]
pub struct MdnsRegistration {
    daemon: ServiceDaemon,
    fullname: String,
}

#[cfg(not(coverage))]
impl MdnsRegistration {
    /// Register the pulpo service on the local network via mDNS.
    pub fn register(reg: &ServiceRegistration) -> Result<Self> {
        let daemon = ServiceDaemon::new().context("Failed to create mDNS daemon")?;
        let service_info = build_service_info(reg)?;
        let fullname = service_info.get_fullname().to_owned();
        daemon
            .register(service_info)
            .context("Failed to register mDNS service")?;
        tracing::info!("mDNS: registered {} on port {}", reg.node_name, reg.port);
        Ok(Self { daemon, fullname })
    }

    /// Gracefully unregister the service and shut down the daemon.
    pub fn shutdown(self) -> Result<()> {
        if let Err(e) = self.daemon.unregister(&self.fullname) {
            tracing::error!("mDNS: failed to unregister service: {e}");
        }
        self.daemon
            .shutdown()
            .context("Failed to shut down mDNS daemon")?;
        tracing::info!("mDNS: shut down");
        Ok(())
    }
}

/// Background task that browses for pulpo services on the local network and
/// updates the `PeerRegistry` with discovered peers.
///
/// Excluded from coverage builds because it uses real mDNS I/O.
#[cfg(not(coverage))]
pub async fn run_mdns_browser(
    registry: crate::peers::PeerRegistry,
    own_name: String,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    use mdns_sd::ServiceEvent;

    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("mDNS browser: failed to create daemon: {e}");
            return;
        }
    };

    let receiver = match daemon.browse(SERVICE_TYPE) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("mDNS browser: failed to start browse: {e}");
            return;
        }
    };

    tracing::info!("mDNS browser: started browsing for {SERVICE_TYPE}");

    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    tracing::info!("mDNS browser: shutting down");
                    let _ = daemon.shutdown();
                    break;
                }
            }
            event = tokio::task::spawn_blocking({
                let receiver = receiver.clone();
                move || receiver.recv_timeout(std::time::Duration::from_secs(1))
            }) => {
                match event {
                    Ok(Ok(ServiceEvent::ServiceResolved(info))) => {
                        let fullname = info.get_fullname();
                        if let Some(name) = parse_instance_name(fullname) {
                            if name == own_name {
                                continue; // skip our own service
                            }
                            let port = info.get_port();
                            if let Some(ip) = info.get_addresses().iter().next() {
                                let address = format_peer_address(&ip.to_ip_addr(), port);
                                if registry.add_discovered_peer(name, &address).await {
                                    tracing::info!("mDNS: discovered peer {name} at {address}");
                                }
                            }
                        }
                    }
                    Ok(Ok(ServiceEvent::ServiceRemoved(_, fullname))) => {
                        if let Some(name) = parse_instance_name(&fullname)
                            && registry.remove_discovered_peer(name).await
                        {
                            tracing::info!("mDNS: peer {name} removed");
                        }
                    }
                    Ok(Ok(_) | Err(_)) => {} // other events + recv_timeout
                    Err(e) => {
                        tracing::error!("mDNS browser: task error: {e}");
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_service_info_valid() {
        let reg = ServiceRegistration {
            node_name: "mac-mini".into(),
            port: 7433,
        };
        let info = build_service_info(&reg).unwrap();
        assert!(info.get_fullname().contains("mac-mini"));
        assert!(info.get_fullname().contains(SERVICE_TYPE));
        assert_eq!(info.get_port(), 7433);
    }

    #[test]
    fn test_build_service_info_txt_records() {
        let reg = ServiceRegistration {
            node_name: "test-node".into(),
            port: 9000,
        };
        let info = build_service_info(&reg).unwrap();
        let props = info.get_properties();
        let version = props.get("version").map(mdns_sd::TxtProperty::val_str);
        let name = props.get("name").map(mdns_sd::TxtProperty::val_str);
        assert_eq!(version, Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(name, Some("test-node"));
    }

    #[test]
    fn test_build_service_info_different_ports() {
        for port in [0, 1, 7433, 65535] {
            let reg = ServiceRegistration {
                node_name: "node".into(),
                port,
            };
            let info = build_service_info(&reg).unwrap();
            assert_eq!(info.get_port(), port);
        }
    }

    #[test]
    fn test_build_service_info_hostname() {
        let reg = ServiceRegistration {
            node_name: "my-box".into(),
            port: 7433,
        };
        let info = build_service_info(&reg).unwrap();
        assert_eq!(info.get_hostname(), "my-box.local.");
    }

    #[test]
    fn test_parse_instance_name_valid() {
        assert_eq!(
            parse_instance_name("mac-mini._pulpo._tcp.local."),
            Some("mac-mini")
        );
    }

    #[test]
    fn test_parse_instance_name_with_dots() {
        assert_eq!(
            parse_instance_name("my.server._pulpo._tcp.local."),
            Some("my.server")
        );
    }

    #[test]
    fn test_parse_instance_name_wrong_type() {
        assert_eq!(parse_instance_name("mac-mini._http._tcp.local."), None);
    }

    #[test]
    fn test_parse_instance_name_empty() {
        assert_eq!(parse_instance_name(""), None);
    }

    #[test]
    fn test_parse_instance_name_just_suffix() {
        assert_eq!(parse_instance_name(&format!(".{SERVICE_TYPE}")), Some(""));
    }

    #[test]
    fn test_format_peer_address_ipv4() {
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        assert_eq!(format_peer_address(&ip, 7433), "192.168.1.100:7433");
    }

    #[test]
    fn test_format_peer_address_ipv6() {
        let ip: IpAddr = "::1".parse().unwrap();
        assert_eq!(format_peer_address(&ip, 7433), "[::1]:7433");
    }

    #[test]
    fn test_format_peer_address_ipv6_full() {
        let ip: IpAddr = "fe80::1".parse().unwrap();
        assert_eq!(format_peer_address(&ip, 9000), "[fe80::1]:9000");
    }

    #[cfg(not(coverage))]
    #[test]
    fn test_register_and_shutdown() {
        let reg = ServiceRegistration {
            node_name: "test-register".into(),
            port: 0,
        };
        let registration = MdnsRegistration::register(&reg).unwrap();
        assert!(!registration.fullname.is_empty());
        registration.shutdown().unwrap();
    }
}
