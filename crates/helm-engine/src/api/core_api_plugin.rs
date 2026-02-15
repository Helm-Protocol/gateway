//! CoreApiPlugin — EventLoop integration for the Mother Agent brain.
//!
//! The 7th runtime plugin. Monitors agent behavior, detects anomalies,
//! broadcasts emergency alerts, and maintains global threat level.
//!
//! Event Flow:
//!   - AgentBorn → registers agent with CoreApi
//!   - on_tick → periodic threat level recalculation
//!   - Custom "behavior_report" → analyzes agent behavior via QKV-G
//!   - Custom "core_query" → returns threat level / agent info

use anyhow::Result;
use async_trait::async_trait;

use helm_core::{Plugin, PluginContext, PluginEvent};
use helm_net::protocol::HelmMessage;

use super::core_api::{AgentType, CoreApi};

pub const PLUGIN_NAME: &str = "helm-core-api";
pub const EVENT_BEHAVIOR_REPORT: &str = "behavior_report";
pub const EVENT_CORE_QUERY: &str = "core_query";
pub const EVENT_THREAT_ALERT: &str = "threat_alert";

/// Default pool capacity for the QKV-G engine used by CoreApi.
const DEFAULT_POOL_CAPACITY: usize = 64;

/// Ticks between threat level recalculations.
const DEFAULT_THREAT_CHECK_INTERVAL: u64 = 50;

/// Configuration for the CoreApi Plugin.
pub struct CoreApiPluginConfig {
    /// Pool capacity for the attention engine.
    pub pool_capacity: usize,
    /// Ticks between threat level checks.
    pub threat_check_interval: u64,
}

impl Default for CoreApiPluginConfig {
    fn default() -> Self {
        Self {
            pool_capacity: DEFAULT_POOL_CAPACITY,
            threat_check_interval: DEFAULT_THREAT_CHECK_INTERVAL,
        }
    }
}

/// CoreApiPlugin — the Mother Agent brain as a runtime plugin.
pub struct CoreApiPlugin {
    core: CoreApi,
    config: CoreApiPluginConfig,
    tick_count: u64,
}

impl CoreApiPlugin {
    pub fn new(config: CoreApiPluginConfig) -> Self {
        let core = CoreApi::new(config.pool_capacity);
        Self {
            core,
            config,
            tick_count: 0,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(CoreApiPluginConfig::default())
    }

    /// Access the underlying CoreApi.
    pub fn core(&self) -> &CoreApi {
        &self.core
    }

    /// Access the underlying CoreApi mutably.
    pub fn core_mut(&mut self) -> &mut CoreApi {
        &mut self.core
    }

    /// Handle AgentBorn: register the new agent with the core brain.
    fn handle_agent_born(&mut self, agent_id: &str, _capability: &str) {
        self.core.register_agent(agent_id, AgentType::Ai);
        tracing::info!(agent = %agent_id, "CoreApi: agent registered with Mother Agent brain");
    }

    /// Handle behavior report: analyze via QKV-G engine.
    fn handle_behavior_report(
        &mut self,
        ctx: &mut PluginContext,
        payload: &serde_json::Value,
    ) {
        let agent_id = match payload.get("agent_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return,
        };

        let behavior: Vec<f32> = payload
            .get("behavior")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect())
            .unwrap_or_default();

        if behavior.is_empty() {
            return;
        }

        if let Some(alert) = self.core.analyze_behavior(agent_id, &behavior) {
            // Emit threat alert to all interested plugins
            ctx.emit(PluginEvent::Custom {
                source_plugin: PLUGIN_NAME.to_string(),
                target_plugin: "helm-agent".to_string(),
                event_type: EVENT_THREAT_ALERT.to_string(),
                payload: serde_json::json!({
                    "agent_id": agent_id,
                    "severity": alert.severity,
                    "g_metric": alert.g_metric,
                    "description": alert.description,
                    "threat_level": self.core.threat_level(),
                }),
            });
        }
    }

    /// Handle core query: return agent info or threat level.
    fn handle_core_query(
        &self,
        ctx: &mut PluginContext,
        payload: &serde_json::Value,
    ) {
        let query_type = payload.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let reply_to = payload
            .get("reply_to")
            .and_then(|v| v.as_str())
            .unwrap_or("helm-agent");

        let response = match query_type {
            "threat_level" => serde_json::json!({
                "threat_level": self.core.threat_level(),
                "agent_count": self.core.agent_count(),
            }),
            "agent_info" => {
                let agent_id = payload.get("agent_id").and_then(|v| v.as_str()).unwrap_or("");
                match self.core.agent_info(agent_id) {
                    Some(info) => serde_json::json!({
                        "agent_id": info.agent_id,
                        "trust_score": info.trust_score,
                        "anomaly_count": info.anomaly_count,
                    }),
                    None => serde_json::json!({"error": "agent not found"}),
                }
            }
            _ => serde_json::json!({"error": "unknown query type"}),
        };

        ctx.emit(PluginEvent::Custom {
            source_plugin: PLUGIN_NAME.to_string(),
            target_plugin: reply_to.to_string(),
            event_type: "core_query_response".to_string(),
            payload: response,
        });
    }
}

#[async_trait]
impl Plugin for CoreApiPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    async fn on_start(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        tracing::info!(
            pool_capacity = self.config.pool_capacity,
            "CoreApiPlugin started — Mother Agent brain online"
        );
        Ok(())
    }

    async fn on_message(&mut self, _ctx: &mut PluginContext, _msg: &HelmMessage) -> Result<()> {
        Ok(())
    }

    async fn on_tick(&mut self, ctx: &mut PluginContext) -> Result<()> {
        self.tick_count += 1;

        // Periodic threat level broadcast if elevated
        if self.tick_count.is_multiple_of(self.config.threat_check_interval) {
            let threat = self.core.threat_level();
            if threat > 0.3 {
                tracing::warn!(threat_level = threat, "Elevated network threat level");
                ctx.emit(PluginEvent::Custom {
                    source_plugin: PLUGIN_NAME.to_string(),
                    target_plugin: "helm-agent".to_string(),
                    event_type: EVENT_THREAT_ALERT.to_string(),
                    payload: serde_json::json!({
                        "threat_level": threat,
                        "agent_count": self.core.agent_count(),
                        "periodic": true,
                    }),
                });
            }
        }
        Ok(())
    }

    async fn on_event(&mut self, ctx: &mut PluginContext, event: &PluginEvent) -> Result<()> {
        match event {
            PluginEvent::AgentBorn { agent_id, capability } => {
                self.handle_agent_born(agent_id, capability);
            }
            PluginEvent::Custom {
                target_plugin,
                event_type,
                payload,
                ..
            } if target_plugin == PLUGIN_NAME => {
                match event_type.as_str() {
                    EVENT_BEHAVIOR_REPORT => {
                        self.handle_behavior_report(ctx, payload);
                    }
                    EVENT_CORE_QUERY => {
                        self.handle_core_query(ctx, payload);
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
            agents = self.core.agent_count(),
            threat_level = self.core.threat_level(),
            "{} shutting down — Mother Agent brain offline", PLUGIN_NAME
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plugin() -> CoreApiPlugin {
        CoreApiPlugin::with_defaults()
    }

    fn make_ctx() -> PluginContext {
        PluginContext::new("test-node".to_string())
    }

    #[tokio::test]
    async fn plugin_name() {
        let plugin = make_plugin();
        assert_eq!(plugin.name(), "helm-core-api");
    }

    #[tokio::test]
    async fn plugin_start_shutdown() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();
        plugin.on_start(&mut ctx).await.unwrap();
        plugin.on_shutdown(&mut ctx).await.unwrap();
    }

    #[tokio::test]
    async fn plugin_registers_agent_on_born() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        let born = PluginEvent::AgentBorn {
            agent_id: "agent-1".to_string(),
            capability: "compute".to_string(),
        };

        plugin.on_event(&mut ctx, &born).await.unwrap();
        assert_eq!(plugin.core().agent_count(), 1);
        assert_eq!(plugin.core().trust_score("agent-1"), Some(0.5));
    }

    #[tokio::test]
    async fn plugin_handles_core_query_threat() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        let query = PluginEvent::Custom {
            source_plugin: "helm-agent".to_string(),
            target_plugin: PLUGIN_NAME.to_string(),
            event_type: EVENT_CORE_QUERY.to_string(),
            payload: serde_json::json!({
                "query": "threat_level",
                "reply_to": "helm-agent",
            }),
        };

        plugin.on_event(&mut ctx, &query).await.unwrap();

        let events = ctx.drain_events();
        assert_eq!(events.len(), 1);
        if let PluginEvent::Custom { payload, event_type, .. } = &events[0] {
            assert_eq!(event_type, "core_query_response");
            assert_eq!(payload["threat_level"], 0.0);
        } else {
            panic!("expected Custom event");
        }
    }

    #[tokio::test]
    async fn plugin_handles_core_query_agent_info() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        // Register agent first
        let born = PluginEvent::AgentBorn {
            agent_id: "agent-x".to_string(),
            capability: "security".to_string(),
        };
        plugin.on_event(&mut ctx, &born).await.unwrap();
        ctx.drain_events();

        // Query agent info
        let query = PluginEvent::Custom {
            source_plugin: "test".to_string(),
            target_plugin: PLUGIN_NAME.to_string(),
            event_type: EVENT_CORE_QUERY.to_string(),
            payload: serde_json::json!({
                "query": "agent_info",
                "agent_id": "agent-x",
                "reply_to": "test",
            }),
        };

        plugin.on_event(&mut ctx, &query).await.unwrap();

        let events = ctx.drain_events();
        assert_eq!(events.len(), 1);
        if let PluginEvent::Custom { payload, .. } = &events[0] {
            assert_eq!(payload["agent_id"], "agent-x");
            assert_eq!(payload["trust_score"], 0.5);
            assert_eq!(payload["anomaly_count"], 0);
        }
    }

    #[tokio::test]
    async fn plugin_ignores_other_targets() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        let event = PluginEvent::Custom {
            source_plugin: "test".to_string(),
            target_plugin: "helm-token".to_string(),
            event_type: "something".to_string(),
            payload: serde_json::json!({}),
        };

        plugin.on_event(&mut ctx, &event).await.unwrap();
        assert_eq!(plugin.core().agent_count(), 0);
    }

    #[tokio::test]
    async fn plugin_multiple_agents() {
        let mut plugin = make_plugin();
        let mut ctx = make_ctx();

        for i in 0..5 {
            let born = PluginEvent::AgentBorn {
                agent_id: format!("agent-{}", i),
                capability: "compute".to_string(),
            };
            plugin.on_event(&mut ctx, &born).await.unwrap();
        }

        assert_eq!(plugin.core().agent_count(), 5);
    }

    #[tokio::test]
    async fn plugin_tick_no_alert_at_zero_threat() {
        let config = CoreApiPluginConfig {
            threat_check_interval: 5,
            ..Default::default()
        };
        let mut plugin = CoreApiPlugin::new(config);
        let mut ctx = make_ctx();

        // Run 5 ticks — should not emit any alert (threat is 0)
        for _ in 0..5 {
            plugin.on_tick(&mut ctx).await.unwrap();
        }

        let events = ctx.drain_events();
        assert!(events.is_empty());
    }
}
