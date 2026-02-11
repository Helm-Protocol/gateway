use anyhow::Result;
use tracing::info;

use crate::config::HelmConfig;
use crate::event_loop::EventLoop;
use crate::plugin::Plugin;

/// Top-level runtime that owns the event loop and plugin registry.
pub struct Runtime {
    config: HelmConfig,
    plugins: Vec<Box<dyn Plugin>>,
}

impl Runtime {
    pub fn new(config: HelmConfig) -> Self {
        Self {
            config,
            plugins: Vec::new(),
        }
    }

    /// Register a plugin to be run inside the event loop.
    pub fn register_plugin(&mut self, plugin: Box<dyn Plugin>) {
        info!("Registered plugin: {}", plugin.name());
        self.plugins.push(plugin);
    }

    /// Start the node: initialize transport, run plugins, enter event loop.
    pub async fn run(self) -> Result<()> {
        info!(
            "Starting Helm node '{}' with {} plugin(s)",
            self.config.node.name,
            self.plugins.len()
        );

        let mut event_loop = EventLoop::new(self.config.clone(), self.plugins)?;
        event_loop.run().await
    }
}
