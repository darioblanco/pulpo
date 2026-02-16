use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub name: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub cpus: usize,
    pub memory_mb: u64,
    pub gpu: Option<String>,
}
