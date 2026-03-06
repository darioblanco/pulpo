pub mod mdns;
pub mod seed;
pub mod tailscale;

/// mDNS service type for Pulpo daemon discovery.
pub const SERVICE_TYPE: &str = "_pulpo._tcp.local.";

/// Information needed to register a pulpo service on the network.
#[derive(Debug, Clone)]
pub struct ServiceRegistration {
    pub node_name: String,
    pub port: u16,
}

impl ServiceRegistration {
    /// Build the TXT record properties for mDNS advertisement.
    pub fn txt_properties(&self) -> Vec<(&str, String)> {
        vec![
            ("version", env!("CARGO_PKG_VERSION").to_owned()),
            ("name", self.node_name.clone()),
        ]
    }

    /// The full mDNS hostname (e.g. `"my-node.local."`).
    pub fn hostname(&self) -> String {
        format!("{}.local.", self.node_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_type() {
        assert_eq!(SERVICE_TYPE, "_pulpo._tcp.local.");
    }

    #[test]
    fn test_service_registration_debug() {
        let reg = ServiceRegistration {
            node_name: "mac-mini".into(),
            port: 7433,
        };
        let debug = format!("{reg:?}");
        assert!(debug.contains("mac-mini"));
        assert!(debug.contains("7433"));
    }

    #[test]
    fn test_service_registration_clone() {
        let reg = ServiceRegistration {
            node_name: "mac-mini".into(),
            port: 7433,
        };
        #[allow(clippy::redundant_clone)]
        let cloned = reg.clone();
        assert_eq!(cloned.node_name, "mac-mini");
        assert_eq!(cloned.port, 7433);
    }

    #[test]
    fn test_txt_properties() {
        let reg = ServiceRegistration {
            node_name: "mac-mini".into(),
            port: 7433,
        };
        let props = reg.txt_properties();
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].0, "version");
        assert_eq!(props[0].1, env!("CARGO_PKG_VERSION"));
        assert_eq!(props[1].0, "name");
        assert_eq!(props[1].1, "mac-mini");
    }

    #[test]
    fn test_hostname() {
        let reg = ServiceRegistration {
            node_name: "mac-mini".into(),
            port: 7433,
        };
        assert_eq!(reg.hostname(), "mac-mini.local.");
    }
}
