use anyhow::Result;
use tracing::{info, warn};

use helm_net::transport::{HelmTransport, TransportEvent};
use helm_net::protocol::HelmProtocol;

use crate::config::HelmConfig;
use crate::plugin::{Plugin, PluginContext};

/// Main event loop: drives the transport and dispatches events to plugins.
pub struct EventLoop {
    transport: HelmTransport,
    config: HelmConfig,
    plugins: Vec<Box<dyn Plugin>>,
}

impl EventLoop {
    pub fn new(config: HelmConfig, plugins: Vec<Box<dyn Plugin>>) -> Result<Self> {
        let transport = HelmTransport::new()?;
        Ok(Self {
            transport,
            config,
            plugins,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", self.config.node.port).parse()?;
        self.transport.listen_on(listen_addr)?;

        info!("Local PeerID: {}", self.transport.local_peer_id());

        let ctx = PluginContext {
            node_name: self.config.node.name.clone(),
        };

        // Notify plugins of startup
        for plugin in &mut self.plugins {
            if let Err(e) = plugin.on_start(&ctx).await {
                warn!("Plugin '{}' failed on_start: {e}", plugin.name());
            }
        }

        // Announce presence
        let announce = HelmProtocol::announce(vec!["chat".into(), "task".into()]);
        if let Err(e) = self.transport.publish(&announce) {
            // May fail if no peers yet — that's fine on first start.
            tracing::debug!("Initial announce skipped (no peers yet): {e}");
        }

        info!("Node started. Listening for peers...");

        loop {
            let event = self.transport.next_event().await;

            match event {
                TransportEvent::Message { source, message } => {
                    info!("Message from {source}: {:?}", message.kind);
                    for plugin in &mut self.plugins {
                        if let Err(e) = plugin.on_message(&ctx, &message).await {
                            warn!(
                                "Plugin '{}' failed on_message: {e}",
                                plugin.name()
                            );
                        }
                    }
                }
                TransportEvent::PeersDiscovered(peers) => {
                    info!("Discovered {} new peer(s)", peers.len());
                }
                TransportEvent::Connected(peer_id) => {
                    info!("Peer connected: {peer_id}");
                }
                TransportEvent::Disconnected(peer_id) => {
                    warn!("Peer disconnected: {peer_id}");
                }
                TransportEvent::Listening(addr) => {
                    info!("Now listening on {addr}");
                }
            }
        }
    }
}
