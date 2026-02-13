use anyhow::Result;
use helm_net::protocol::HelmMessage;
use std::collections::VecDeque;

/// Internal event types for cross-plugin communication.
#[derive(Debug, Clone)]
pub enum PluginEvent {
    /// Store a key-value pair (from Agent → Store).
    StoreRequest {
        key: Vec<u8>,
        value: Vec<u8>,
        source: String,
    },
    /// Store retrieval result (from Store → Agent).
    StoreResponse {
        key: Vec<u8>,
        value: Option<Vec<u8>>,
        target: String,
    },
    /// Broadcast a message to the network (from Agent → Network).
    NetworkBroadcast {
        message: HelmMessage,
    },
    /// API revenue collected (from Engine → Token).
    ApiRevenue {
        caller: String,
        amount_units: u64,
        endpoint: String,
    },
    /// Token operation result.
    TokenResult {
        operation: String,
        success: bool,
        detail: String,
    },
    /// Agent lifecycle event (from Agent → others).
    AgentBorn {
        agent_id: String,
        capability: String,
    },
    /// Custom inter-plugin event.
    Custom {
        source_plugin: String,
        target_plugin: String,
        event_type: String,
        payload: serde_json::Value,
    },
}

/// Context passed to plugins on each event.
///
/// Includes the inter-plugin message bus for cross-plugin communication.
/// Plugins emit events into the `outbox`, and the EventLoop routes them
/// to the appropriate plugin(s) via their `on_event` handler.
pub struct PluginContext {
    pub node_name: String,
    /// Outbound event queue: plugins push events here during callbacks.
    outbox: VecDeque<PluginEvent>,
}

impl PluginContext {
    /// Create a new context for the given node.
    pub fn new(node_name: String) -> Self {
        Self {
            node_name,
            outbox: VecDeque::new(),
        }
    }

    /// Emit an event to the inter-plugin bus.
    pub fn emit(&mut self, event: PluginEvent) {
        self.outbox.push_back(event);
    }

    /// Drain all pending events from the outbox.
    pub fn drain_events(&mut self) -> Vec<PluginEvent> {
        self.outbox.drain(..).collect()
    }

    /// Number of pending outbound events.
    pub fn pending_events(&self) -> usize {
        self.outbox.len()
    }
}

/// Trait that all Helm plugins must implement.
#[async_trait::async_trait]
pub trait Plugin: Send + Sync {
    /// Unique name of this plugin.
    fn name(&self) -> &str;

    /// Called once when the node starts.
    async fn on_start(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        Ok(())
    }

    /// Called for each incoming network message.
    async fn on_message(
        &mut self,
        _ctx: &mut PluginContext,
        _msg: &HelmMessage,
    ) -> Result<()> {
        Ok(())
    }

    /// Called periodically (e.g., every heartbeat tick).
    async fn on_tick(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        Ok(())
    }

    /// Called when a cross-plugin event is routed to this plugin.
    async fn on_event(&mut self, _ctx: &mut PluginContext, _event: &PluginEvent) -> Result<()> {
        Ok(())
    }

    /// Called when the node shuts down.
    async fn on_shutdown(&mut self, _ctx: &mut PluginContext) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_context_creation() {
        let ctx = PluginContext::new("test-node".to_string());
        assert_eq!(ctx.node_name, "test-node");
        assert_eq!(ctx.pending_events(), 0);
    }

    #[test]
    fn plugin_context_emit_and_drain() {
        let mut ctx = PluginContext::new("test-node".to_string());

        ctx.emit(PluginEvent::StoreRequest {
            key: b"key1".to_vec(),
            value: b"val1".to_vec(),
            source: "agent-1".to_string(),
        });

        ctx.emit(PluginEvent::NetworkBroadcast {
            message: HelmMessage {
                version: 1,
                kind: helm_net::protocol::MessageKind::Chat,
                payload: serde_json::json!({"text": "hello"}),
                timestamp: 0,
            },
        });

        assert_eq!(ctx.pending_events(), 2);

        let events = ctx.drain_events();
        assert_eq!(events.len(), 2);
        assert_eq!(ctx.pending_events(), 0);
    }

    #[test]
    fn plugin_context_drain_empty() {
        let mut ctx = PluginContext::new("test-node".to_string());
        let events = ctx.drain_events();
        assert!(events.is_empty());
    }

    #[test]
    fn plugin_event_variants() {
        let revenue = PluginEvent::ApiRevenue {
            caller: "agent-1".to_string(),
            amount_units: 100,
            endpoint: "grg/encode".to_string(),
        };
        assert!(matches!(revenue, PluginEvent::ApiRevenue { .. }));

        let born = PluginEvent::AgentBorn {
            agent_id: "agent-2".to_string(),
            capability: "compute".to_string(),
        };
        assert!(matches!(born, PluginEvent::AgentBorn { .. }));

        let custom = PluginEvent::Custom {
            source_plugin: "helm-agent".to_string(),
            target_plugin: "helm-token".to_string(),
            event_type: "stake_request".to_string(),
            payload: serde_json::json!({"amount": 1000}),
        };
        assert!(matches!(custom, PluginEvent::Custom { .. }));
    }

    #[test]
    fn plugin_event_store_response() {
        let resp = PluginEvent::StoreResponse {
            key: b"key1".to_vec(),
            value: Some(b"val1".to_vec()),
            target: "agent-1".to_string(),
        };
        match resp {
            PluginEvent::StoreResponse { key, value, target } => {
                assert_eq!(key, b"key1");
                assert_eq!(value, Some(b"val1".to_vec()));
                assert_eq!(target, "agent-1");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn plugin_event_token_result() {
        let result = PluginEvent::TokenResult {
            operation: "transfer".to_string(),
            success: true,
            detail: "transferred 1000 HELM".to_string(),
        };
        assert!(matches!(result, PluginEvent::TokenResult { success: true, .. }));
    }
}
