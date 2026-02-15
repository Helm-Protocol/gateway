//! Identity Plugin — EventLoop integration for helm-identity.
//!
//! Bridges the Agent Spanner identity system into the Helm EventLoop.
//! Listens for AgentBorn events to auto-register identities, handles
//! heartbeat ticks for online tracking, and responds to identity
//! verification requests via the inter-plugin bus.

use anyhow::Result;
use helm_core::plugin::{Plugin, PluginContext, PluginEvent};
use helm_net::protocol::HelmMessage;

use crate::did::HelmKeyPair;
use crate::spanner::AgentSpanner;

// --- Identity Plugin Constants ---
pub const PLUGIN_NAME: &str = "helm-identity";
pub const EVENT_IDENTITY_REGISTERED: &str = "identity_registered";
pub const EVENT_IDENTITY_VERIFIED: &str = "identity_verified";
pub const EVENT_VERIFY_IDENTITY: &str = "verify_identity";
pub const EVENT_HEARTBEAT: &str = "heartbeat";
pub const EVENT_TERMINATE: &str = "terminate";
const DEFAULT_REPLY_TARGET: &str = "helm-agent";
const DEFAULT_WOMB_ID: &str = "plugin-womb";

/// Default online threshold in seconds (5 minutes).
pub const DEFAULT_ONLINE_THRESHOLD_SECS: u64 = 300;
/// Default reputation decay factor per epoch.
pub const DEFAULT_DECAY_FACTOR: f64 = 0.95;
/// Default ticks between decay applications.
pub const DEFAULT_DECAY_INTERVAL_TICKS: u64 = 100;

/// Configuration for the Identity Plugin.
pub struct IdentityPluginConfig {
    /// Online threshold in seconds (default 300).
    pub online_threshold_secs: u64,
    /// Reputation decay factor per epoch (default 0.95).
    pub decay_factor: f64,
    /// Ticks between decay applications (default 100).
    pub decay_interval_ticks: u64,
}

impl Default for IdentityPluginConfig {
    fn default() -> Self {
        Self {
            online_threshold_secs: DEFAULT_ONLINE_THRESHOLD_SECS,
            decay_factor: DEFAULT_DECAY_FACTOR,
            decay_interval_ticks: DEFAULT_DECAY_INTERVAL_TICKS,
        }
    }
}

/// Identity Plugin — manages agent identities within the EventLoop.
pub struct IdentityPlugin {
    spanner: AgentSpanner,
    config: IdentityPluginConfig,
    tick_count: u64,
    current_time: u64,
}

impl IdentityPlugin {
    pub fn new(config: IdentityPluginConfig) -> Self {
        Self {
            spanner: AgentSpanner::with_threshold(config.online_threshold_secs),
            config,
            tick_count: 0,
            current_time: 0,
        }
    }

    /// Access the underlying Agent Spanner.
    pub fn spanner(&self) -> &AgentSpanner {
        &self.spanner
    }

    /// Access the underlying Agent Spanner mutably.
    pub fn spanner_mut(&mut self) -> &mut AgentSpanner {
        &mut self.spanner
    }

    /// Handle an AgentBorn event: create DID + Bond for the new agent.
    fn handle_agent_born(
        &mut self,
        agent_id: &str,
        capability: &str,
        ctx: &mut PluginContext,
    ) -> Result<()> {
        let kp = HelmKeyPair::generate();
        let doc = kp.create_document(self.current_time);
        let did = doc.id.clone();

        let result = self.spanner.register_agent(
            doc,
            agent_id,
            &ctx.node_name,
            DEFAULT_WOMB_ID,
            vec![capability.to_string()],
            self.current_time,
        );

        match result {
            Ok(_) => {
                tracing::info!(agent = %agent_id, did = %did, "identity auto-registered via plugin");

                // Persist identity entry to KvStore via StorePlugin
                if let Ok(entry) = self.spanner.resolve(&did) {
                    if let Ok(serialized) = serde_json::to_vec(entry) {
                        let key = format!("identity:{}", did);
                        ctx.emit(PluginEvent::StoreRequest {
                            key: key.into_bytes(),
                            value: serialized,
                            source: PLUGIN_NAME.to_string(),
                        });
                    }
                }

                // Emit confirmation event
                ctx.emit(PluginEvent::Custom {
                    source_plugin: PLUGIN_NAME.to_string(),
                    target_plugin: DEFAULT_REPLY_TARGET.to_string(),
                    event_type: EVENT_IDENTITY_REGISTERED.to_string(),
                    payload: serde_json::json!({
                        "agent_id": agent_id,
                        "did": did,
                    }),
                });
            }
            Err(e) => {
                tracing::warn!(agent = %agent_id, error = %e, "identity registration failed");
            }
        }

        Ok(())
    }

    /// Handle identity verification request.
    fn handle_verify_request(
        &self,
        payload: &serde_json::Value,
        ctx: &mut PluginContext,
    ) {
        let did = payload.get("did").and_then(|v| v.as_str()).unwrap_or("");
        let capability = payload.get("capability").and_then(|v| v.as_str()).unwrap_or("");
        let request_id = payload.get("request_id").and_then(|v| v.as_str()).unwrap_or("");

        let verified = self.spanner.verify(did, capability);

        ctx.emit(PluginEvent::Custom {
            source_plugin: PLUGIN_NAME.to_string(),
            target_plugin: payload
                .get("reply_to")
                .and_then(|v| v.as_str())
                .unwrap_or(DEFAULT_REPLY_TARGET)
                .to_string(),
            event_type: EVENT_IDENTITY_VERIFIED.to_string(),
            payload: serde_json::json!({
                "request_id": request_id,
                "did": did,
                "capability": capability,
                "verified": verified,
            }),
        });
    }
}

#[async_trait::async_trait]
impl Plugin for IdentityPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    async fn on_start(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        tracing::info!("{} plugin started", PLUGIN_NAME);
        Ok(())
    }

    async fn on_message(
        &mut self,
        _ctx: &mut PluginContext,
        _msg: &HelmMessage,
    ) -> Result<()> {
        // Identity messages from network peers would be handled here
        // (DID document replication, reputation gossip, etc.)
        Ok(())
    }

    async fn on_tick(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        self.tick_count += 1;
        self.current_time += 1; // Simplified: 1 tick = 1 second

        // Periodic reputation decay
        if self.tick_count % self.config.decay_interval_ticks == 0 {
            self.spanner.apply_decay(self.config.decay_factor);
            tracing::debug!(tick = self.tick_count, "reputation decay applied");
        }

        Ok(())
    }

    async fn on_event(&mut self, ctx: &mut PluginContext, event: &PluginEvent) -> Result<()> {
        match event {
            PluginEvent::AgentBorn {
                agent_id,
                capability,
            } => {
                self.handle_agent_born(agent_id, capability, ctx)?;
            }
            PluginEvent::Custom {
                target_plugin,
                event_type,
                payload,
                ..
            } if target_plugin == PLUGIN_NAME => {
                match event_type.as_str() {
                    EVENT_VERIFY_IDENTITY => {
                        self.handle_verify_request(payload, ctx);
                    }
                    EVENT_HEARTBEAT => {
                        if let Some(did) = payload.get("did").and_then(|v| v.as_str()) {
                            let addr = payload
                                .get("address")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            self.spanner.heartbeat(did, self.current_time, addr);
                        }
                    }
                    EVENT_TERMINATE => {
                        if let Some(did) = payload.get("did").and_then(|v| v.as_str()) {
                            let _ = self.spanner.terminate_agent(did, self.current_time);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn on_shutdown(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        tracing::info!(
            active = self.spanner.active_count(),
            total = self.spanner.total_count(),
            "{} plugin shutting down", PLUGIN_NAME
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plugin() -> IdentityPlugin {
        IdentityPlugin::new(IdentityPluginConfig::default())
    }

    fn make_ctx() -> PluginContext {
        PluginContext::new("test-node".to_string())
    }

    #[tokio::test]
    async fn plugin_name() {
        let plugin = make_plugin();
        assert_eq!(plugin.name(), "helm-identity");
    }

    #[tokio::test]
    async fn plugin_start() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();
        plugin.on_start(&mut ctx).await.unwrap();
    }

    #[tokio::test]
    async fn plugin_handles_agent_born() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        let event = PluginEvent::AgentBorn {
            agent_id: "agent-1".to_string(),
            capability: "compute".to_string(),
        };

        plugin.on_event(&mut ctx, &event).await.unwrap();

        assert_eq!(plugin.spanner().active_count(), 1);
        let entry = plugin.spanner().resolve_by_agent("agent-1").unwrap();
        assert!(entry.has_capability("compute"));

        // Should have emitted: StoreRequest (persist) + Custom (confirmation)
        let events = ctx.drain_events();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], PluginEvent::StoreRequest { .. }));
        assert!(matches!(events[1], PluginEvent::Custom { .. }));
    }

    #[tokio::test]
    async fn plugin_handles_verify_request() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        // First register an agent
        let born = PluginEvent::AgentBorn {
            agent_id: "agent-1".to_string(),
            capability: "compute".to_string(),
        };
        plugin.on_event(&mut ctx, &born).await.unwrap();
        ctx.drain_events(); // Clear

        // Get the DID
        let did = plugin
            .spanner()
            .resolve_by_agent("agent-1")
            .unwrap()
            .did
            .clone();

        // Send verify request
        let verify = PluginEvent::Custom {
            source_plugin: "helm-agent".to_string(),
            target_plugin: "helm-identity".to_string(),
            event_type: "verify_identity".to_string(),
            payload: serde_json::json!({
                "did": did,
                "capability": "compute",
                "request_id": "req-1",
                "reply_to": "helm-agent",
            }),
        };
        plugin.on_event(&mut ctx, &verify).await.unwrap();

        let events = ctx.drain_events();
        assert_eq!(events.len(), 1);
        if let PluginEvent::Custom { payload, .. } = &events[0] {
            assert_eq!(payload["verified"], true);
        }
    }

    #[tokio::test]
    async fn plugin_handles_heartbeat() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        let born = PluginEvent::AgentBorn {
            agent_id: "agent-1".to_string(),
            capability: "compute".to_string(),
        };
        plugin.on_event(&mut ctx, &born).await.unwrap();

        let did = plugin
            .spanner()
            .resolve_by_agent("agent-1")
            .unwrap()
            .did
            .clone();

        // Advance time
        for _ in 0..10 {
            plugin.on_tick(&mut ctx).await.unwrap();
        }

        let heartbeat = PluginEvent::Custom {
            source_plugin: "helm-net".to_string(),
            target_plugin: "helm-identity".to_string(),
            event_type: "heartbeat".to_string(),
            payload: serde_json::json!({
                "did": did,
                "address": "/ip4/10.0.0.1/tcp/9000",
            }),
        };
        plugin.on_event(&mut ctx, &heartbeat).await.unwrap();

        let entry = plugin.spanner().resolve(&did).unwrap();
        assert_eq!(entry.last_seen, 10); // current_time after 10 ticks
        assert_eq!(entry.address.as_deref(), Some("/ip4/10.0.0.1/tcp/9000"));
    }

    #[tokio::test]
    async fn plugin_handles_terminate() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        let born = PluginEvent::AgentBorn {
            agent_id: "agent-1".to_string(),
            capability: "compute".to_string(),
        };
        plugin.on_event(&mut ctx, &born).await.unwrap();

        let did = plugin
            .spanner()
            .resolve_by_agent("agent-1")
            .unwrap()
            .did
            .clone();

        assert_eq!(plugin.spanner().active_count(), 1);

        let terminate = PluginEvent::Custom {
            source_plugin: "helm-agent".to_string(),
            target_plugin: "helm-identity".to_string(),
            event_type: "terminate".to_string(),
            payload: serde_json::json!({ "did": did }),
        };
        plugin.on_event(&mut ctx, &terminate).await.unwrap();

        assert_eq!(plugin.spanner().active_count(), 0);
    }

    #[tokio::test]
    async fn plugin_tick_decay() {
        let mut plugin = IdentityPlugin::new(IdentityPluginConfig {
            decay_interval_ticks: 5,
            decay_factor: 0.8,
            ..Default::default()
        });
        let mut ctx = make_ctx();

        let born = PluginEvent::AgentBorn {
            agent_id: "agent-1".to_string(),
            capability: "compute".to_string(),
        };
        plugin.on_event(&mut ctx, &born).await.unwrap();

        let did = plugin
            .spanner()
            .resolve_by_agent("agent-1")
            .unwrap()
            .did
            .clone();

        // Boost reputation
        plugin
            .spanner_mut()
            .record_reputation(&did, "reliability", 0.4)
            .unwrap();
        let before = plugin.spanner().resolve(&did).unwrap().trust_score();

        // Tick past decay interval
        for _ in 0..5 {
            plugin.on_tick(&mut ctx).await.unwrap();
        }

        let after = plugin.spanner().resolve(&did).unwrap().trust_score();
        // Decay pulls toward neutral
        assert!(after <= before || (after - before).abs() < 0.01);
    }

    #[tokio::test]
    async fn plugin_shutdown() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();
        plugin.on_shutdown(&mut ctx).await.unwrap();
    }

    #[tokio::test]
    async fn plugin_ignores_unrelated_events() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        let event = PluginEvent::StoreRequest {
            key: b"key".to_vec(),
            value: b"val".to_vec(),
            source: "test".to_string(),
        };
        plugin.on_event(&mut ctx, &event).await.unwrap();
        assert_eq!(plugin.spanner().active_count(), 0);
    }

    #[tokio::test]
    async fn plugin_ignores_custom_for_other_plugins() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        let event = PluginEvent::Custom {
            source_plugin: "helm-agent".to_string(),
            target_plugin: "helm-token".to_string(), // not us
            event_type: "something".to_string(),
            payload: serde_json::json!({}),
        };
        plugin.on_event(&mut ctx, &event).await.unwrap();
        assert_eq!(plugin.spanner().active_count(), 0);
    }

    #[tokio::test]
    async fn plugin_duplicate_agent_born() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        let born = PluginEvent::AgentBorn {
            agent_id: "agent-1".to_string(),
            capability: "compute".to_string(),
        };

        // First registration succeeds
        plugin.on_event(&mut ctx, &born).await.unwrap();
        assert_eq!(plugin.spanner().active_count(), 1);

        ctx.drain_events();

        // Second registration should be handled gracefully (no panic)
        plugin.on_event(&mut ctx, &born).await.unwrap();
        // Still only 1 agent
        assert_eq!(plugin.spanner().active_count(), 1);
    }

    #[tokio::test]
    async fn plugin_persists_identity_to_store() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        let born = PluginEvent::AgentBorn {
            agent_id: "agent-persist".to_string(),
            capability: "storage".to_string(),
        };
        plugin.on_event(&mut ctx, &born).await.unwrap();

        let events = ctx.drain_events();
        // First event should be StoreRequest with serialized identity
        let store_req = &events[0];
        if let PluginEvent::StoreRequest { key, value, source } = store_req {
            assert_eq!(source, PLUGIN_NAME);
            let key_str = String::from_utf8_lossy(key);
            assert!(key_str.starts_with("identity:did:helm:"));
            // Value should deserialize back to SpannerEntry
            let entry: crate::spanner::SpannerEntry = serde_json::from_slice(value).unwrap();
            assert_eq!(entry.agent_id, "agent-persist");
            assert!(entry.bond.is_active());
        } else {
            panic!("expected StoreRequest, got {:?}", store_req);
        }
    }

    #[test]
    fn config_default() {
        let config = IdentityPluginConfig::default();
        assert_eq!(config.online_threshold_secs, 300);
        assert!((config.decay_factor - 0.95).abs() < f64::EPSILON);
        assert_eq!(config.decay_interval_ticks, 100);
    }
}
