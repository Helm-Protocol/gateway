use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level node configuration, loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelmConfig {
    pub node: NodeConfig,
    #[serde(default)]
    pub network: NetworkConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Human-readable node name (not PII — use pseudonyms).
    #[serde(default = "default_node_name")]
    pub name: String,
    /// Port to listen on. 0 = random.
    #[serde(default)]
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Bootstrap peer addresses to connect to on startup.
    #[serde(default)]
    pub bootstrap_peers: Vec<String>,
    /// Enable mDNS local discovery.
    #[serde(default = "default_true")]
    pub mdns_enabled: bool,
    /// Enable Kademlia DHT.
    #[serde(default = "default_true")]
    pub kademlia_enabled: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bootstrap_peers: Vec::new(),
            mdns_enabled: true,
            kademlia_enabled: true,
        }
    }
}

impl HelmConfig {
    /// Load configuration from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: HelmConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load from file if it exists, otherwise return defaults.
    pub fn load_or_default(path: &Path) -> Self {
        match Self::from_file(path) {
            Ok(config) => config,
            Err(_) => Self::default(),
        }
    }
}

impl Default for HelmConfig {
    fn default() -> Self {
        Self {
            node: NodeConfig {
                name: default_node_name(),
                port: 0,
            },
            network: NetworkConfig::default(),
        }
    }
}

fn default_node_name() -> String {
    "helm-node".to_string()
}

fn default_true() -> bool {
    true
}
