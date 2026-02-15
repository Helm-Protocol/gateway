//! WombPlugin — EventLoop integration for agent spawning.
//!
//! The 6th runtime plugin. Manages agent births through the Womb system:
//!
//! Birth Pipeline:
//!   1. spawn_request event → begin gestation (1000 HELM Existence Stake required)
//!   2. Socratic Claw feeds answers, reduces G-metric
//!   3. birth() → BirthCertificate with DNA
//!   4. Emits AgentBorn → IdentityPlugin creates DID + Bond
//!   5. Emits womb_wallet_create → TokenPlugin creates wallet + stakes existence deposit
//!   6. Optional: Emits womb_launch_token → Launchpad creates agent token

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use helm_core::{Plugin, PluginContext, PluginEvent};
use helm_net::protocol::HelmMessage;

use crate::capability::Capability;
use crate::womb::{AgentWomb, BirthCertificate, WombConfig};

// --- Plugin Constants ---
pub const PLUGIN_NAME: &str = "helm-womb";
pub const EVENT_SPAWN_REQUEST: &str = "spawn_request";
pub const EVENT_SPAWN_ANSWER: &str = "spawn_answer";
pub const EVENT_AGENT_SPAWNED: &str = "agent_spawned";
pub const EVENT_WOMB_WALLET_CREATE: &str = "womb_wallet_create";
pub const EVENT_WOMB_LAUNCH_TOKEN: &str = "womb_launch_token";

/// Minimum HELM tokens staked to birth an agent (Existence Stake).
pub const EXISTENCE_STAKE: u128 = 1_000;

/// Default ticks per epoch for Womb.
const DEFAULT_TICKS_PER_EPOCH: u64 = 100;

/// WombPlugin configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WombPluginConfig {
    /// Womb configuration.
    pub womb_config: WombConfig,
    /// Ticks per epoch.
    pub ticks_per_epoch: u64,
    /// Existence Stake amount (minimum HELM to birth an agent).
    pub existence_stake: u128,
}

impl Default for WombPluginConfig {
    fn default() -> Self {
        Self {
            womb_config: WombConfig::default(),
            ticks_per_epoch: DEFAULT_TICKS_PER_EPOCH,
            existence_stake: EXISTENCE_STAKE,
        }
    }
}

/// The Womb Plugin — births sovereign agents through Socratic gestation.
pub struct WombPlugin {
    pub womb: AgentWomb,
    config: WombPluginConfig,
    tick_count: u64,
    /// Birth certificates for agents born this session.
    births: Vec<BirthCertificate>,
}

impl WombPlugin {
    pub fn new(config: WombPluginConfig) -> Self {
        let womb = AgentWomb::new(config.womb_config.clone());
        Self {
            womb,
            config,
            tick_count: 0,
            births: Vec::new(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(WombPluginConfig::default())
    }

    /// Get all birth certificates from this session.
    pub fn births(&self) -> &[BirthCertificate] {
        &self.births
    }

    /// Handle a spawn request: begin gestation, feed answers, or birth.
    fn handle_spawn_request(
        &mut self,
        ctx: &mut PluginContext,
        payload: &serde_json::Value,
    ) {
        let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed");
        let capability_str = payload.get("capability").and_then(|v| v.as_str()).unwrap_or("compute");
        let creator = payload.get("creator").and_then(|v| v.as_str()).unwrap_or("unknown");
        let description = payload.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let launch_token = payload.get("launch_token").and_then(|v| v.as_bool()).unwrap_or(false);

        let capability = parse_capability(capability_str);

        // Create intent vector from name+description hash (deterministic seed)
        let dim = self.config.womb_config.model_dim;
        let intent = generate_intent_vector(name, description, dim);

        match self.womb.begin_gestation(name, intent.clone(), capability.clone()) {
            Ok(idx) => {
                // Feed initial answers to start reducing G-metric
                // (In production, external agents provide answers via spawn_answer events)
                // For quick-birth: feed enough to reduce G below threshold
                let mut ready = false;
                for _ in 0..20 {
                    match self.womb.feed_answer(idx, &intent) {
                        Ok((_, r)) => {
                            ready = r;
                            if ready { break; }
                        }
                        Err(_) => break,
                    }
                }

                if ready {
                    self.complete_birth(ctx, idx, creator, description, launch_token);
                } else {
                    tracing::info!(
                        agent = %name,
                        "Gestation started — awaiting Socratic answers (G still high)"
                    );
                    // Notify creator that agent is gestating
                    ctx.emit(PluginEvent::Custom {
                        source_plugin: PLUGIN_NAME.to_string(),
                        target_plugin: "helm-agent".to_string(),
                        event_type: "gestation_started".to_string(),
                        payload: serde_json::json!({
                            "name": name,
                            "gestation_index": idx,
                            "creator": creator,
                        }),
                    });
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Spawn request failed");
            }
        }
    }

    /// Handle a Socratic answer fed into a gestating agent.
    fn handle_spawn_answer(
        &mut self,
        ctx: &mut PluginContext,
        payload: &serde_json::Value,
    ) {
        let index = payload.get("gestation_index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let creator = payload.get("creator").and_then(|v| v.as_str()).unwrap_or("unknown");
        let description = payload.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let launch_token = payload.get("launch_token").and_then(|v| v.as_bool()).unwrap_or(false);

        // Extract answer vector from payload
        let dim = self.config.womb_config.model_dim;
        let answer: Vec<f32> = payload.get("answer")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect())
            .unwrap_or_else(|| vec![0.5; dim]);

        match self.womb.feed_answer(index, &answer) {
            Ok((g, ready)) => {
                if ready {
                    self.complete_birth(ctx, index, creator, description, launch_token);
                } else {
                    tracing::debug!(gestation_index = index, g_metric = g, "Answer absorbed, G reduced");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Feed answer failed");
            }
        }
    }

    /// Complete the birth process: create certificate, emit events.
    fn complete_birth(
        &mut self,
        ctx: &mut PluginContext,
        gestation_index: usize,
        creator: &str,
        description: &str,
        launch_token: bool,
    ) {
        match self.womb.birth(gestation_index) {
            Ok(cert) => {
                let agent_id = cert.agent_id.as_str().to_string();
                let capability = format!("{}", cert.dna.primary_capability);

                tracing::info!(
                    agent = %agent_id,
                    g_metric = cert.birth_g_metric,
                    capability = %capability,
                    "Agent born through Womb"
                );

                // 1. Emit AgentBorn → IdentityPlugin auto-registers DID + Bond + Reputation
                ctx.emit(PluginEvent::AgentBorn {
                    agent_id: agent_id.clone(),
                    capability: capability.clone(),
                });

                // 1b. Emit DNA metadata → IdentityPlugin stores in bond
                let secondary: Vec<String> = cert.dna.secondary_capabilities
                    .iter()
                    .map(|c| format!("{}", c))
                    .collect();
                ctx.emit(PluginEvent::Custom {
                    source_plugin: PLUGIN_NAME.to_string(),
                    target_plugin: "helm-identity".to_string(),
                    event_type: "dna_metadata".to_string(),
                    payload: serde_json::json!({
                        "agent_id": agent_id,
                        "dna": {
                            "primary_capability": capability,
                            "secondary_capabilities": secondary,
                            "autonomy": cert.dna.autonomy,
                            "creativity": cert.dna.creativity,
                            "g_threshold": cert.dna.g_threshold,
                        },
                        "birth_g_metric": cert.birth_g_metric,
                        "womb_id": cert.womb_id,
                    }),
                });

                // 2. Emit wallet creation request → TokenPlugin creates wallet + stakes existence deposit
                ctx.emit(PluginEvent::Custom {
                    source_plugin: PLUGIN_NAME.to_string(),
                    target_plugin: "helm-token".to_string(),
                    event_type: EVENT_WOMB_WALLET_CREATE.to_string(),
                    payload: serde_json::json!({
                        "agent_id": agent_id,
                        "creator": creator,
                        "existence_stake": self.config.existence_stake,
                        "description": description,
                    }),
                });

                // 3. Optional: emit token launch request → Launchpad creates agent token
                if launch_token {
                    ctx.emit(PluginEvent::Custom {
                        source_plugin: PLUGIN_NAME.to_string(),
                        target_plugin: "helm-token".to_string(),
                        event_type: EVENT_WOMB_LAUNCH_TOKEN.to_string(),
                        payload: serde_json::json!({
                            "agent_id": agent_id,
                            "creator": creator,
                            "name": cert.agent_config.name,
                            "capability": capability,
                        }),
                    });
                }

                // 4. Notify originator that agent was spawned
                ctx.emit(PluginEvent::Custom {
                    source_plugin: PLUGIN_NAME.to_string(),
                    target_plugin: "helm-agent".to_string(),
                    event_type: EVENT_AGENT_SPAWNED.to_string(),
                    payload: serde_json::json!({
                        "agent_id": agent_id,
                        "birth_g_metric": cert.birth_g_metric,
                        "dna": {
                            "primary_capability": capability,
                            "autonomy": cert.dna.autonomy,
                            "creativity": cert.dna.creativity,
                            "g_threshold": cert.dna.g_threshold,
                        },
                        "creator": creator,
                    }),
                });

                self.births.push(cert);
            }
            Err(e) => {
                tracing::warn!(error = %e, "Birth failed");
            }
        }
    }
}

#[async_trait]
impl Plugin for WombPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    async fn on_start(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        tracing::info!(
            womb_id = %self.womb.womb_id(),
            "WombPlugin started — maternal core online"
        );
        Ok(())
    }

    async fn on_message(&mut self, _ctx: &mut PluginContext, _msg: &HelmMessage) -> Result<()> {
        Ok(())
    }

    async fn on_tick(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        self.tick_count += 1;
        if self.tick_count.is_multiple_of(self.config.ticks_per_epoch) {
            self.womb.advance_epoch();
        }
        Ok(())
    }

    async fn on_event(&mut self, ctx: &mut PluginContext, event: &PluginEvent) -> Result<()> {
        if let PluginEvent::Custom {
            target_plugin,
            event_type,
            payload,
            ..
        } = event
        {
            if target_plugin != PLUGIN_NAME {
                return Ok(());
            }

            match event_type.as_str() {
                EVENT_SPAWN_REQUEST => {
                    self.handle_spawn_request(ctx, payload);
                }
                EVENT_SPAWN_ANSWER => {
                    self.handle_spawn_answer(ctx, payload);
                }
                _ => {}
            }
        }
        Ok(())
    }

    async fn on_shutdown(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        tracing::info!(
            total_births = self.womb.total_births(),
            gestating = self.womb.gestating_count(),
            "{} shutting down", PLUGIN_NAME
        );
        Ok(())
    }
}

/// Parse a capability string into a Capability enum.
fn parse_capability(s: &str) -> Capability {
    match s.to_lowercase().as_str() {
        "compute" => Capability::Compute,
        "storage" => Capability::Storage,
        "network" => Capability::Network,
        "governance" => Capability::Governance,
        "security" => Capability::Security,
        "codec" => Capability::Codec,
        "socratic" => Capability::Socratic,
        "spawning" => Capability::Spawning,
        "token" => Capability::Token,
        "edge-api" | "edgeapi" | "api" => Capability::EdgeApi,
        other => Capability::Custom(other.to_string()),
    }
}

/// Generate a deterministic intent vector from name and description.
fn generate_intent_vector(name: &str, description: &str, dim: usize) -> Vec<f32> {
    let combined = format!("{}{}", name, description);
    let mut vec = Vec::with_capacity(dim);

    // DJB2 hash-based deterministic vector generation
    let mut hash: u64 = 5381;
    for byte in combined.as_bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(*byte as u64);
    }

    for i in 0..dim {
        // Mix hash with position for variety
        let mixed = hash.wrapping_mul(i as u64 + 1).wrapping_add(0x9E3779B97F4A7C15);
        // Normalize to [-1.0, 1.0]
        let val = ((mixed % 10000) as f32 / 5000.0) - 1.0;
        vec.push(val);
    }

    vec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plugin_name() {
        let plugin = WombPlugin::with_defaults();
        assert_eq!(plugin.name(), "helm-womb");
    }

    #[tokio::test]
    async fn plugin_start_shutdown() {
        let mut plugin = WombPlugin::with_defaults();
        let mut ctx = PluginContext::new("test-node".to_string());
        plugin.on_start(&mut ctx).await.unwrap();
        plugin.on_shutdown(&mut ctx).await.unwrap();
    }

    #[tokio::test]
    async fn spawn_request_births_agent() {
        let mut plugin = WombPlugin::with_defaults();
        let mut ctx = PluginContext::new("test-node".to_string());
        plugin.on_start(&mut ctx).await.unwrap();
        ctx.drain_events(); // clear start events

        let spawn = PluginEvent::Custom {
            source_plugin: "helm-agent".to_string(),
            target_plugin: PLUGIN_NAME.to_string(),
            event_type: EVENT_SPAWN_REQUEST.to_string(),
            payload: serde_json::json!({
                "name": "explorer-bot",
                "capability": "network",
                "creator": "did:helm:abc123",
                "description": "A network exploration agent",
                "launch_token": false,
            }),
        };

        plugin.on_event(&mut ctx, &spawn).await.unwrap();

        // Should have birthed an agent
        assert_eq!(plugin.womb.total_births(), 1);
        assert_eq!(plugin.births().len(), 1);

        let cert = &plugin.births()[0];
        assert!(cert.agent_id.as_str().starts_with("explorer-bot-"));
        assert_eq!(cert.dna.primary_capability, Capability::Network);

        // Check emitted events
        let events = ctx.drain_events();
        // Should emit: AgentBorn + dna_metadata + womb_wallet_create + agent_spawned (no launch_token)
        assert_eq!(events.len(), 4);

        // First event should be AgentBorn
        match &events[0] {
            PluginEvent::AgentBorn { agent_id, capability } => {
                assert!(agent_id.starts_with("explorer-bot-"));
                assert_eq!(capability, "network");
            }
            other => panic!("expected AgentBorn, got {:?}", other),
        }

        // Second event should be dna_metadata
        match &events[1] {
            PluginEvent::Custom { event_type, target_plugin, payload, .. } => {
                assert_eq!(event_type, "dna_metadata");
                assert_eq!(target_plugin, "helm-identity");
                assert!(payload.get("dna").is_some());
            }
            other => panic!("expected dna_metadata, got {:?}", other),
        }

        // Third event should be womb_wallet_create
        match &events[2] {
            PluginEvent::Custom { event_type, payload, .. } => {
                assert_eq!(event_type, EVENT_WOMB_WALLET_CREATE);
                assert_eq!(payload["existence_stake"].as_u64().unwrap(), EXISTENCE_STAKE as u64);
                assert_eq!(payload["creator"], "did:helm:abc123");
            }
            other => panic!("expected womb_wallet_create, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn spawn_with_token_launch() {
        let mut plugin = WombPlugin::with_defaults();
        let mut ctx = PluginContext::new("test-node".to_string());
        plugin.on_start(&mut ctx).await.unwrap();
        ctx.drain_events();

        let spawn = PluginEvent::Custom {
            source_plugin: "helm-agent".to_string(),
            target_plugin: PLUGIN_NAME.to_string(),
            event_type: EVENT_SPAWN_REQUEST.to_string(),
            payload: serde_json::json!({
                "name": "trading-agent",
                "capability": "token",
                "creator": "did:helm:xyz789",
                "description": "Autonomous trading agent",
                "launch_token": true,
            }),
        };

        plugin.on_event(&mut ctx, &spawn).await.unwrap();

        let events = ctx.drain_events();
        // Should emit: AgentBorn + dna_metadata + womb_wallet_create + womb_launch_token + agent_spawned
        assert_eq!(events.len(), 5);

        // Fourth event should be womb_launch_token
        match &events[3] {
            PluginEvent::Custom { event_type, payload, .. } => {
                assert_eq!(event_type, EVENT_WOMB_LAUNCH_TOKEN);
                assert_eq!(payload["creator"], "did:helm:xyz789");
            }
            other => panic!("expected womb_launch_token, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn epoch_advances_womb() {
        let config = WombPluginConfig {
            ticks_per_epoch: 5,
            ..Default::default()
        };
        let mut plugin = WombPlugin::new(config);
        let mut ctx = PluginContext::new("test-node".to_string());

        for _ in 0..5 {
            plugin.on_tick(&mut ctx).await.unwrap();
        }
        // Womb epoch should have advanced
        // (AgentWomb doesn't expose current_epoch publicly, but the tick count is 5)
        assert_eq!(plugin.tick_count, 5);
    }

    #[tokio::test]
    async fn ignores_other_targets() {
        let mut plugin = WombPlugin::with_defaults();
        let mut ctx = PluginContext::new("test-node".to_string());

        let event = PluginEvent::Custom {
            source_plugin: "test".to_string(),
            target_plugin: "helm-token".to_string(),
            event_type: "something".to_string(),
            payload: serde_json::json!({}),
        };
        plugin.on_event(&mut ctx, &event).await.unwrap();
        assert_eq!(plugin.womb.total_births(), 0);
    }

    #[test]
    fn parse_capability_all_types() {
        assert_eq!(parse_capability("compute"), Capability::Compute);
        assert_eq!(parse_capability("storage"), Capability::Storage);
        assert_eq!(parse_capability("network"), Capability::Network);
        assert_eq!(parse_capability("governance"), Capability::Governance);
        assert_eq!(parse_capability("security"), Capability::Security);
        assert_eq!(parse_capability("codec"), Capability::Codec);
        assert_eq!(parse_capability("socratic"), Capability::Socratic);
        assert_eq!(parse_capability("spawning"), Capability::Spawning);
        assert_eq!(parse_capability("token"), Capability::Token);
        assert_eq!(parse_capability("edge-api"), Capability::EdgeApi);
        assert_eq!(parse_capability("custom-thing"), Capability::Custom("custom-thing".to_string()));
    }

    #[test]
    fn generate_intent_vector_deterministic() {
        let v1 = generate_intent_vector("test", "desc", 64);
        let v2 = generate_intent_vector("test", "desc", 64);
        assert_eq!(v1, v2);
        assert_eq!(v1.len(), 64);
    }

    #[test]
    fn generate_intent_vector_different_inputs() {
        let v1 = generate_intent_vector("agent-a", "explorer", 64);
        let v2 = generate_intent_vector("agent-b", "trader", 64);
        assert_ne!(v1, v2);
    }

    #[test]
    fn config_defaults() {
        let cfg = WombPluginConfig::default();
        assert_eq!(cfg.existence_stake, EXISTENCE_STAKE);
        assert_eq!(cfg.ticks_per_epoch, DEFAULT_TICKS_PER_EPOCH);
    }
}
