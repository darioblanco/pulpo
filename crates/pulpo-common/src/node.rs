use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NodeInfo {
    pub name: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub cpus: usize,
    pub memory_mb: u64,
    pub gpu: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_info_serialize() {
        let info = NodeInfo {
            name: "test-node".into(),
            hostname: "localhost".into(),
            os: "macos".into(),
            arch: "aarch64".into(),
            cpus: 8,
            memory_mb: 16384,
            gpu: Some("RTX 5090".into()),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"name\":\"test-node\""));
        assert!(json.contains("\"gpu\":\"RTX 5090\""));
    }

    #[test]
    fn test_node_info_deserialize() {
        let json = r#"{"name":"n","hostname":"h","os":"linux","arch":"x86_64","cpus":4,"memory_mb":8192,"gpu":null}"#;
        let info: NodeInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.name, "n");
        assert_eq!(info.cpus, 4);
        assert!(info.gpu.is_none());
    }

    #[test]
    fn test_node_info_roundtrip() {
        let info = NodeInfo {
            name: "roundtrip".into(),
            hostname: "host".into(),
            os: "wsl2".into(),
            arch: "x86_64".into(),
            cpus: 16,
            memory_mb: 32768,
            gpu: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: NodeInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, info.name);
        assert_eq!(deserialized.memory_mb, info.memory_mb);
    }

    #[test]
    fn test_node_info_debug() {
        let info = NodeInfo {
            name: "debug".into(),
            hostname: "h".into(),
            os: "macos".into(),
            arch: "arm64".into(),
            cpus: 1,
            memory_mb: 0,
            gpu: None,
        };
        let debug = format!("{info:?}");
        assert!(debug.contains("debug"));
    }

    #[test]
    fn test_node_info_clone() {
        let info = NodeInfo {
            name: "clone".into(),
            hostname: "h".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            cpus: 2,
            memory_mb: 1024,
            gpu: Some("GPU".into()),
        };
        #[allow(clippy::redundant_clone)]
        let cloned = info.clone();
        assert_eq!(cloned.name, "clone");
        assert_eq!(cloned.gpu, Some("GPU".into()));
    }
}
