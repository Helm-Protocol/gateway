use anyhow::Result;
use helm_net::protocol::HelmMessage;

/// Context passed to plugins on each event.
pub struct PluginContext {
    pub node_name: String,
}

/// Trait that all Helm plugins must implement.
#[async_trait::async_trait]
pub trait Plugin: Send + Sync {
    /// Unique name of this plugin.
    fn name(&self) -> &str;

    /// Called once when the node starts.
    async fn on_start(&mut self, _ctx: &PluginContext) -> Result<()> {
        Ok(())
    }

    /// Called for each incoming network message.
    async fn on_message(
        &mut self,
        _ctx: &PluginContext,
        _msg: &HelmMessage,
    ) -> Result<()> {
        Ok(())
    }

    /// Called periodically (e.g., every heartbeat tick).
    async fn on_tick(&mut self, _ctx: &PluginContext) -> Result<()> {
        Ok(())
    }

    /// Called when the node shuts down.
    async fn on_shutdown(&mut self, _ctx: &PluginContext) -> Result<()> {
        Ok(())
    }
}
