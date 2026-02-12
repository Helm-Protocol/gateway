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
    /// Listen address. Default: 127.0.0.1 (use 0.0.0.0 to expose externally).
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
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
        Self::from_file(path).unwrap_or_default()
    }
}

impl Default for HelmConfig {
    fn default() -> Self {
        Self {
            node: NodeConfig {
                name: default_node_name(),
                listen_addr: default_listen_addr(),
                port: 0,
            },
            network: NetworkConfig::default(),
        }
    }
}

fn default_node_name() -> String {
    "helm-node".to_string()
}

fn default_listen_addr() -> String {
    "127.0.0.1".to_string()
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn default_config() {
        let config = HelmConfig::default();
        assert_eq!(config.node.name, "helm-node");
        assert_eq!(config.node.port, 0);
        assert!(config.network.mdns_enabled);
        assert!(config.network.kademlia_enabled);
        assert!(config.network.bootstrap_peers.is_empty());
    }

    #[test]
    fn load_from_toml() {
        let toml_content = r#"
[node]
name = "test-node"
port = 9735

[network]
mdns_enabled = false
kademlia_enabled = true
bootstrap_peers = ["/ip4/1.2.3.4/tcp/9735"]
"#;
        let dir = std::env::temp_dir().join("helm_test_config");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("helm.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        let config = HelmConfig::from_file(&path).unwrap();
        assert_eq!(config.node.name, "test-node");
        assert_eq!(config.node.port, 9735);
        assert!(!config.network.mdns_enabled);
        assert!(config.network.kademlia_enabled);
        assert_eq!(config.network.bootstrap_peers, vec!["/ip4/1.2.3.4/tcp/9735"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_or_default_missing_file() {
        let config = HelmConfig::load_or_default(Path::new("/nonexistent/helm.toml"));
        assert_eq!(config.node.name, "helm-node");
    }

    #[test]
    fn minimal_toml_uses_defaults() {
        let toml_content = r#"
[node]
name = "minimal"
"#;
        let dir = std::env::temp_dir().join("helm_test_minimal");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("helm.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(toml_content.as_bytes()).unwrap();

        let config = HelmConfig::from_file(&path).unwrap();
        assert_eq!(config.node.name, "minimal");
        assert_eq!(config.node.port, 0);
        assert!(config.network.mdns_enabled);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn config_serialization_roundtrip() {
        let config = HelmConfig::default();
        let toml_str = toml::to_string(&config).unwrap();
        let decoded: HelmConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(decoded.node.name, config.node.name);
        assert_eq!(decoded.node.port, config.node.port);
    }
}
