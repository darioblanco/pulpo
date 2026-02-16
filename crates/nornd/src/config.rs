use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub node: NodeConfig,
}

#[derive(Debug, Deserialize)]
pub struct NodeConfig {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}

fn default_name() -> String {
    hostname::get().map_or_else(|_| "unknown".into(), |h| h.to_string_lossy().into_owned())
}

const fn default_port() -> u16 {
    7433
}

fn default_data_dir() -> String {
    dirs::home_dir().map_or_else(
        || "~/.norn".into(),
        |h| h.join(".norn").to_string_lossy().into_owned(),
    )
}

impl Config {
    pub fn data_dir(&self) -> String {
        shellexpand::tilde(&self.node.data_dir).into_owned()
    }
}

pub fn load(path: &str) -> Result<Config> {
    let expanded = shellexpand::tilde(path);
    let path = std::path::Path::new(expanded.as_ref());

    if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        toml::from_str(&content).context("Failed to parse config")
    } else {
        // Return defaults if no config file exists
        Ok(Config {
            node: NodeConfig {
                name: default_name(),
                port: default_port(),
                data_dir: default_data_dir(),
            },
        })
    }
}
