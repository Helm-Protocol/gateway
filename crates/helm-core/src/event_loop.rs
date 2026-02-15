use anyhow::Result;
use tracing::{info, warn, debug};
use std::time::Duration;
use tokio::sync::watch;

use helm_net::transport::{HelmTransport, TransportEvent};
use helm_net::protocol::HelmProtocol;

use crate::config::HelmConfig;
use crate::plugin::{Plugin, PluginContext, PluginEvent};

/// Default tick interval in milliseconds.
const DEFAULT_TICK_INTERVAL_MS: u64 = 100;

/// Maximum inter-plugin event routing rounds per tick (prevents infinite loops).
const MAX_EVENT_ROUNDS: usize = 8;

/// Handle to signal the EventLoop to shut down gracefully.
#[derive(Clone)]
pub struct ShutdownHandle {
    tx: watch::Sender<bool>,
}

impl ShutdownHandle {
    /// Signal the EventLoop to shut down.
    pub fn shutdown(&self) {
        let _ = self.tx.send(true);
    }
}

/// Main event loop: drives the transport and dispatches events to plugins.
pub struct EventLoop {
    transport: HelmTransport,
    config: HelmConfig,
    plugins: Vec<Box<dyn Plugin>>,
    tick_interval_ms: u64,
    shutdown_rx: watch::Receiver<bool>,
}

impl EventLoop {
    pub fn new(config: HelmConfig, plugins: Vec<Box<dyn Plugin>>) -> Result<(Self, ShutdownHandle)> {
        let transport = HelmTransport::new()?;
        let (tx, rx) = watch::channel(false);
        Ok((
            Self {
                transport,
                config,
                plugins,
                tick_interval_ms: DEFAULT_TICK_INTERVAL_MS,
                shutdown_rx: rx,
            },
            ShutdownHandle { tx },
        ))
    }

    /// Set tick interval.
    pub fn with_tick_interval(mut self, ms: u64) -> Self {
        self.tick_interval_ms = ms;
        self
    }

    pub async fn run(&mut self) -> Result<()> {
        let listen_addr = format!(
            "/ip4/{}/tcp/{}",
            self.config.node.listen_addr, self.config.node.port
        )
        .parse()?;
        self.transport.listen_on(listen_addr)?;

        info!("Local PeerID: {}", self.transport.local_peer_id());

        let mut ctx = PluginContext::new(self.config.node.name.clone());

        // Notify plugins of startup
        for plugin in &mut self.plugins {
            if let Err(e) = plugin.on_start(&mut ctx).await {
                warn!("Plugin '{}' failed on_start: {e}", plugin.name());
            }
        }
        // Route any events emitted during startup
        self.route_events(&mut ctx).await;

        // Announce presence
        let announce = HelmProtocol::announce(vec!["chat".into(), "task".into()]);
        if let Err(e) = self.transport.publish(&announce) {
            tracing::debug!("Initial announce skipped (no peers yet): {e}");
        }

        info!(
            plugins = self.plugins.len(),
            tick_ms = self.tick_interval_ms,
            "Node started. Listening for peers..."
        );

        let tick_duration = Duration::from_millis(self.tick_interval_ms);
        let mut tick_interval = tokio::time::interval(tick_duration);

        loop {
            tokio::select! {
                event = self.transport.next_event() => {
                    self.handle_transport_event(event, &mut ctx).await;
                    self.route_events(&mut ctx).await;
                }
                _ = tick_interval.tick() => {
                    self.handle_tick(&mut ctx).await;
                    self.route_events(&mut ctx).await;
                }
                _ = self.shutdown_rx.changed() => {
                    if *self.shutdown_rx.borrow() {
                        info!("Shutdown signal received, draining events...");
                        self.route_events(&mut ctx).await;
                        break;
                    }
                }
            }
        }

        // Graceful shutdown: notify all plugins
        self.shutdown_plugins(&mut ctx).await?;
        info!("EventLoop terminated gracefully");
        Ok(())
    }

    /// Handle a transport event.
    async fn handle_transport_event(
        &mut self,
        event: TransportEvent,
        ctx: &mut PluginContext,
    ) {
        match event {
            TransportEvent::Message { source, message } => {
                info!("Message from {source}: {:?}", message.kind);
                for plugin in &mut self.plugins {
                    if let Err(e) = plugin.on_message(ctx, &message).await {
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

    /// Handle a periodic tick.
    async fn handle_tick(&mut self, ctx: &mut PluginContext) {
        for plugin in &mut self.plugins {
            if let Err(e) = plugin.on_tick(ctx).await {
                warn!("Plugin '{}' failed on_tick: {e}", plugin.name());
            }
        }
    }

    /// Route inter-plugin events: drain outbox, deliver to all plugins, repeat.
    /// Limited to MAX_EVENT_ROUNDS to prevent infinite cascading.
    async fn route_events(&mut self, ctx: &mut PluginContext) {
        for round in 0..MAX_EVENT_ROUNDS {
            let events = ctx.drain_events();
            if events.is_empty() {
                break;
            }

            debug!(
                round = round,
                count = events.len(),
                "Routing inter-plugin events"
            );

            // Handle NetworkBroadcast directly (transport publish)
            for event in &events {
                if let PluginEvent::NetworkBroadcast { message } = event {
                    if let Err(e) = self.transport.publish(message) {
                        warn!("NetworkBroadcast failed: {e}");
                    }
                }
            }

            // Deliver each event to all plugins
            for event in &events {
                for plugin in &mut self.plugins {
                    if let Err(e) = plugin.on_event(ctx, event).await {
                        warn!(
                            "Plugin '{}' failed on_event: {e}",
                            plugin.name()
                        );
                    }
                }
            }
        }
    }

    /// Graceful shutdown: notify all plugins.
    async fn shutdown_plugins(&mut self, ctx: &mut PluginContext) -> Result<()> {
        for plugin in &mut self.plugins {
            if let Err(e) = plugin.on_shutdown(ctx).await {
                warn!("Plugin '{}' failed on_shutdown: {e}", plugin.name());
            }
        }
        info!("All plugins shut down");
        Ok(())
    }
}
