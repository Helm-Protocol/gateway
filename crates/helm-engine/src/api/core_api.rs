//! Hidden Core API — the autonomous agent's control interface.
//!
//! The distributed core brain uses this API to:
//! - Send information and emergency alerts to individual agents
//! - Detect anomalous behavior patterns across the network
//! - Issue questions/requests to agents (both human and AI)
//! - Enforce self-security protocols
//!
//! This is the foundation of the autonomous agent system:
//! a self-aware security system that monitors and protects the network.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error};

use crate::qkvg::attention::{HelmAttentionEngine, AnomalyAlert};
use crate::qkvg::cache_block::Vector;

/// Message types the core brain can send to agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoreMessage {
    /// Informational broadcast
    Info {
        topic: String,
        content: String,
    },
    /// Emergency alert requiring immediate attention
    EmergencyAlert {
        severity: AlertSeverity,
        threat: String,
        recommended_action: String,
    },
    /// Question directed at a specific agent
    Question {
        question: String,
        context: String,
        requires_response: bool,
    },
    /// Request for an agent to perform an action
    ActionRequest {
        action: String,
        parameters: HashMap<String, String>,
        priority: Priority,
    },
    /// Security policy update
    PolicyUpdate {
        policy_id: String,
        rules: Vec<String>,
    },
    /// Network topology change notification
    TopologyChange {
        event: String,
        affected_peers: Vec<String>,
    },
}

/// Alert severity levels.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum AlertSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Action priority levels.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Priority {
    Background,
    Normal,
    Urgent,
    Immediate,
}

/// An agent registered with the core brain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Agent identifier
    pub agent_id: String,
    /// Agent type (human, ai, hybrid)
    pub agent_type: AgentType,
    /// Trust score (0.0 = untrusted, 1.0 = fully trusted)
    pub trust_score: f32,
    /// Behavior pattern vector (for anomaly detection)
    pub behavior_vector: Option<Vector>,
    /// Pending messages for this agent
    pub inbox: Vec<CoreMessage>,
    /// Number of anomalies detected
    pub anomaly_count: u32,
}

/// Type of agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentType {
    Human,
    Ai,
    Hybrid,
}

/// The Hidden Core API.
///
/// Powers the autonomous agent that monitors and secures the network.
/// This is the "brain" of the distributed system — the first autonomous
/// agent with self-awareness about network security.
pub struct CoreApi {
    /// Registered agents
    agents: HashMap<String, AgentInfo>,
    /// QKV-G engine for behavior pattern analysis
    engine: HelmAttentionEngine,
    /// Reference behavior table (normal patterns)
    reference_table: usize,
    /// Broadcast history
    broadcast_log: Vec<(u64, CoreMessage)>,
    /// Global threat level (0.0 = safe, 1.0 = under attack)
    threat_level: f32,
}

impl CoreApi {
    /// Create a new Core API with the given attention engine capacity.
    pub fn new(pool_capacity: usize) -> Self {
        let mut engine = HelmAttentionEngine::new(pool_capacity);
        let reference_table = engine.create_sequence(0); // Sequence 0 = reference behaviors

        Self {
            agents: HashMap::new(),
            engine,
            reference_table,
            broadcast_log: Vec::new(),
            threat_level: 0.0,
        }
    }

    /// Register an agent with the core brain.
    pub fn register_agent(&mut self, agent_id: &str, agent_type: AgentType) {
        info!("Core API: registering agent '{}' ({:?})", agent_id, agent_type);
        self.agents.insert(agent_id.to_string(), AgentInfo {
            agent_id: agent_id.to_string(),
            agent_type,
            trust_score: 0.5, // Start neutral
            behavior_vector: None,
            inbox: Vec::new(),
            anomaly_count: 0,
        });
    }

    /// Store a reference behavior pattern (what "normal" looks like).
    pub fn store_reference_behavior(
        &mut self,
        key: Vector,
        value: Vector,
        token_pos: usize,
    ) -> Result<(), anyhow::Error> {
        self.engine.store_kv(self.reference_table, token_pos, key, value)
    }

    /// Analyze an agent's behavior pattern for anomalies.
    pub fn analyze_behavior(
        &mut self,
        agent_id: &str,
        behavior: &Vector,
    ) -> Option<AnomalyAlert> {
        let alert = self.engine.detect_anomaly(agent_id, behavior, self.reference_table);

        if let Some(ref alert) = alert {
            if let Some(agent) = self.agents.get_mut(agent_id) {
                agent.behavior_vector = Some(behavior.clone());
                agent.anomaly_count += 1;

                // Decrease trust based on anomaly severity
                agent.trust_score = (agent.trust_score - alert.severity * 0.1).max(0.0);

                // Auto-respond based on severity
                if alert.severity >= 0.5 {
                    warn!("Core API: anomaly from '{}' (G={:.3})", agent_id, alert.g_metric);
                    let severity = if alert.severity >= 0.8 {
                        AlertSeverity::Critical
                    } else {
                        AlertSeverity::High
                    };

                    agent.inbox.push(CoreMessage::EmergencyAlert {
                        severity,
                        threat: alert.description.clone(),
                        recommended_action: "Review behavior and re-authenticate".to_string(),
                    });
                }
            }

            // Update global threat level
            self.update_threat_level();
        }

        alert
    }

    /// Send a message to a specific agent.
    pub fn send_to_agent(&mut self, agent_id: &str, message: CoreMessage) -> bool {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            info!("Core API → {}: {:?}", agent_id, std::mem::discriminant(&message));
            agent.inbox.push(message);
            true
        } else {
            warn!("Core API: agent '{}' not found", agent_id);
            false
        }
    }

    /// Send a question to an agent (the core brain asking for information).
    pub fn ask_agent(&mut self, agent_id: &str, question: &str, context: &str) -> bool {
        self.send_to_agent(agent_id, CoreMessage::Question {
            question: question.to_string(),
            context: context.to_string(),
            requires_response: true,
        })
    }

    /// Broadcast a message to all registered agents.
    pub fn broadcast(&mut self, message: CoreMessage, timestamp_ms: u64) {
        info!("Core API: broadcasting to {} agents", self.agents.len());
        self.broadcast_log.push((timestamp_ms, message.clone()));

        let agent_ids: Vec<String> = self.agents.keys().cloned().collect();
        for agent_id in agent_ids {
            if let Some(agent) = self.agents.get_mut(&agent_id) {
                agent.inbox.push(message.clone());
            }
        }
    }

    /// Broadcast an emergency alert.
    pub fn emergency_broadcast(
        &mut self,
        severity: AlertSeverity,
        threat: &str,
        action: &str,
        timestamp_ms: u64,
    ) {
        error!("Core API: EMERGENCY {:?} — {}", severity, threat);
        self.broadcast(CoreMessage::EmergencyAlert {
            severity,
            threat: threat.to_string(),
            recommended_action: action.to_string(),
        }, timestamp_ms);
    }

    /// Drain an agent's inbox (agent reads its messages).
    pub fn drain_inbox(&mut self, agent_id: &str) -> Vec<CoreMessage> {
        self.agents.get_mut(agent_id)
            .map(|a| std::mem::take(&mut a.inbox))
            .unwrap_or_default()
    }

    /// Get an agent's trust score.
    pub fn trust_score(&self, agent_id: &str) -> Option<f32> {
        self.agents.get(agent_id).map(|a| a.trust_score)
    }

    /// Manually adjust an agent's trust score.
    pub fn set_trust_score(&mut self, agent_id: &str, score: f32) {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.trust_score = score.clamp(0.0, 1.0);
        }
    }

    /// Get current global threat level.
    pub fn threat_level(&self) -> f32 {
        self.threat_level
    }

    /// Number of registered agents.
    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }

    /// Get agent info.
    pub fn agent_info(&self, agent_id: &str) -> Option<&AgentInfo> {
        self.agents.get(agent_id)
    }

    /// Get anomaly log from the attention engine.
    pub fn anomaly_log(&self) -> &[AnomalyAlert] {
        self.engine.anomaly_log()
    }

    /// Recalculate global threat level based on recent anomalies.
    fn update_threat_level(&mut self) {
        let total_anomalies: u32 = self.agents.values().map(|a| a.anomaly_count).sum();
        let agent_count = self.agents.len().max(1) as f32;
        let avg_anomalies = total_anomalies as f32 / agent_count;

        // Threat level rises with average anomaly count
        self.threat_level = (avg_anomalies / 10.0).min(1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_query_agents() {
        let mut core = CoreApi::new(16);
        core.register_agent("alice", AgentType::Human);
        core.register_agent("bot-1", AgentType::Ai);

        assert_eq!(core.agent_count(), 2);
        assert_eq!(core.trust_score("alice"), Some(0.5));
        assert_eq!(core.agent_info("bot-1").unwrap().agent_type, AgentType::Ai);
    }

    #[test]
    fn send_and_drain_messages() {
        let mut core = CoreApi::new(16);
        core.register_agent("agent-1", AgentType::Ai);

        core.send_to_agent("agent-1", CoreMessage::Info {
            topic: "test".to_string(),
            content: "hello".to_string(),
        });

        core.ask_agent("agent-1", "What is your status?", "routine check");

        let messages = core.drain_inbox("agent-1");
        assert_eq!(messages.len(), 2);

        // Inbox should be empty after drain
        assert!(core.drain_inbox("agent-1").is_empty());
    }

    #[test]
    fn broadcast_reaches_all() {
        let mut core = CoreApi::new(16);
        core.register_agent("a1", AgentType::Human);
        core.register_agent("a2", AgentType::Ai);
        core.register_agent("a3", AgentType::Hybrid);

        core.broadcast(CoreMessage::Info {
            topic: "update".to_string(),
            content: "system maintenance".to_string(),
        }, 1000);

        assert_eq!(core.drain_inbox("a1").len(), 1);
        assert_eq!(core.drain_inbox("a2").len(), 1);
        assert_eq!(core.drain_inbox("a3").len(), 1);
    }

    #[test]
    fn emergency_alert() {
        let mut core = CoreApi::new(16);
        core.register_agent("node-1", AgentType::Ai);

        core.emergency_broadcast(
            AlertSeverity::Critical,
            "Eclipse attack detected",
            "Disconnect from suspicious peers",
            5000,
        );

        let messages = core.drain_inbox("node-1");
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            CoreMessage::EmergencyAlert { severity, .. } => {
                assert_eq!(*severity, AlertSeverity::Critical);
            }
            _ => panic!("expected EmergencyAlert"),
        }
    }

    #[test]
    fn trust_score_adjustment() {
        let mut core = CoreApi::new(16);
        core.register_agent("agent-x", AgentType::Ai);

        core.set_trust_score("agent-x", 0.9);
        assert_eq!(core.trust_score("agent-x"), Some(0.9));

        core.set_trust_score("agent-x", 1.5); // Clamped to 1.0
        assert_eq!(core.trust_score("agent-x"), Some(1.0));
    }

    #[test]
    fn threat_level_starts_zero() {
        let core = CoreApi::new(16);
        assert_eq!(core.threat_level(), 0.0);
    }
}
