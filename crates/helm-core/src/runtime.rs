use anyhow::Result;
use tracing::info;

use crate::config::HelmConfig;
use crate::event_loop::{EventLoop, ShutdownHandle};
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
    /// Returns a ShutdownHandle that can be used to stop the node gracefully.
    pub async fn run(self) -> Result<ShutdownHandle> {
        info!(
            "Starting Helm node '{}' with {} plugin(s)",
            self.config.node.name,
            self.plugins.len()
        );

        let (mut event_loop, handle) = EventLoop::new(self.config.clone(), self.plugins)?;
        let run_handle = handle.clone();

        tokio::spawn(async move {
            if let Err(e) = event_loop.run().await {
                tracing::error!("EventLoop error: {e}");
            }
        });

        Ok(run_handle)
    }

    /// Start the node and block until shutdown.
    pub async fn run_blocking(self) -> Result<()> {
        info!(
            "Starting Helm node '{}' with {} plugin(s)",
            self.config.node.name,
            self.plugins.len()
        );

        let (mut event_loop, _handle) = EventLoop::new(self.config.clone(), self.plugins)?;
        event_loop.run().await
    }
}
