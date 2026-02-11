//! Core Agent trait and supporting types.

use serde::{Deserialize, Serialize};
use anyhow::Result;

use crate::capability::Capability;
use crate::lifecycle::Lifecycle;
use crate::message::AgentMessage;

/// Unique agent identifier within the Helm network.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

impl AgentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Classification of agent origin and autonomy level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentType {
    /// Human-controlled agent.
    Human,
    /// AI-driven agent.
    Ai,
    /// Human-AI hybrid agent.
    Hybrid,
    /// Fully autonomous (Helm Womb spawned).
    Autonomous,
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Human => write!(f, "Human"),
            Self::Ai => write!(f, "AI"),
            Self::Hybrid => write!(f, "Hybrid"),
            Self::Autonomous => write!(f, "Autonomous"),
        }
    }
}

/// Configuration for agent instantiation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Human-readable agent name.
    pub name: String,
    /// Agent classification.
    pub agent_type: AgentType,
    /// Declared capabilities.
    pub capabilities: Vec<Capability>,
    /// Maximum ticks before forced suspension (0 = unlimited).
    pub max_ticks: u64,
    /// Socratic Claw G-threshold override (None = use global default 0.4).
    pub g_threshold: Option<f32>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "unnamed-agent".to_string(),
            agent_type: AgentType::Ai,
            capabilities: Vec::new(),
            max_ticks: 0,
            g_threshold: None,
        }
    }
}

/// Context provided to agents during execution ticks.
#[derive(Debug)]
pub struct ExecutionContext {
    /// Current tick number.
    pub tick: u64,
    /// Node name this agent is running on.
    pub node_name: String,
    /// Current G-metric from the Socratic Claw (0.0 = full knowledge, 1.0 = total gap).
    pub g_metric: f32,
    /// Whether the Socratic Claw has halted execution pending gap resolution.
    pub halted: bool,
    /// Number of pending messages in the mailbox.
    pub pending_messages: usize,
}

/// Actions an agent can emit after execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentAction {
    /// No operation — agent is idle.
    Noop,
    /// Send a message to another agent.
    Send {
        target: AgentId,
        payload: String,
    },
    /// Broadcast a message to all agents.
    Broadcast {
        payload: String,
    },
    /// Store a key-value pair in the distributed store.
    Store {
        key: Vec<u8>,
        value: Vec<u8>,
    },
    /// Request a capability that the agent doesn't yet have.
    RequestCapability {
        capability: Capability,
        reason: String,
    },
    /// Voluntarily suspend execution.
    Suspend {
        reason: String,
    },
    /// Voluntarily terminate.
    Terminate {
        reason: String,
    },
    /// Submit a Socratic answer to fill a detected knowledge gap.
    SocraticAnswer {
        gap_id: u64,
        answer: Vec<f32>,
    },
    /// Batch of multiple actions.
    Batch(Vec<AgentAction>),
}

/// The core Agent trait. Every agent in the Helm network implements this.
pub trait Agent: Send + Sync {
    /// Returns the unique agent identifier.
    fn id(&self) -> &AgentId;

    /// Returns the agent type classification.
    fn agent_type(&self) -> AgentType;

    /// Returns the agent's declared capabilities.
    fn capabilities(&self) -> &[Capability];

    /// Returns the current lifecycle state.
    fn lifecycle(&self) -> &Lifecycle;

    /// Mutable access to lifecycle for state transitions.
    fn lifecycle_mut(&mut self) -> &mut Lifecycle;

    /// Called once when the agent is initialized (Created → Ready).
    fn init(&mut self) -> Result<()> {
        Ok(())
    }

    /// Execute a single tick. The core agent loop.
    /// Returns an action (or batch of actions) to be processed by the runtime.
    fn execute(&mut self, ctx: &ExecutionContext) -> Result<AgentAction>;

    /// Handle an incoming message from another agent or the system.
    fn receive(&mut self, msg: &AgentMessage) -> Result<()>;

    /// Called when the agent is being terminated (→ Terminated).
    fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }

    /// Return a behavior vector for trust analysis.
    /// Agents that support behavior profiling override this.
    fn behavior_vector(&self) -> Option<Vec<f32>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::Lifecycle;
    use crate::message::{AgentMessage, MessageKind};

    /// A minimal test agent for unit tests.
    struct TestAgent {
        id: AgentId,
        lifecycle: Lifecycle,
        received: Vec<String>,
    }

    impl TestAgent {
        fn new(id: &str) -> Self {
            Self {
                id: AgentId::new(id),
                lifecycle: Lifecycle::new(),
                received: Vec::new(),
            }
        }
    }

    impl Agent for TestAgent {
        fn id(&self) -> &AgentId { &self.id }
        fn agent_type(&self) -> AgentType { AgentType::Ai }
        fn capabilities(&self) -> &[Capability] { &[] }
        fn lifecycle(&self) -> &Lifecycle { &self.lifecycle }
        fn lifecycle_mut(&mut self) -> &mut Lifecycle { &mut self.lifecycle }

        fn execute(&mut self, ctx: &ExecutionContext) -> Result<AgentAction> {
            if ctx.halted {
                return Ok(AgentAction::Noop);
            }
            Ok(AgentAction::Noop)
        }

        fn receive(&mut self, msg: &AgentMessage) -> Result<()> {
            if let MessageKind::Data { payload } = &msg.kind {
                self.received.push(String::from_utf8_lossy(payload).to_string());
            }
            Ok(())
        }
    }

    #[test]
    fn agent_id_creation() {
        let id = AgentId::new("agent-001");
        assert_eq!(id.as_str(), "agent-001");
        assert_eq!(id.to_string(), "agent-001");
    }

    #[test]
    fn agent_id_equality() {
        let a = AgentId::new("alpha");
        let b = AgentId::new("alpha");
        let c = AgentId::new("beta");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn agent_type_display() {
        assert_eq!(AgentType::Human.to_string(), "Human");
        assert_eq!(AgentType::Ai.to_string(), "AI");
        assert_eq!(AgentType::Hybrid.to_string(), "Hybrid");
        assert_eq!(AgentType::Autonomous.to_string(), "Autonomous");
    }

    #[test]
    fn agent_config_defaults() {
        let cfg = AgentConfig::default();
        assert_eq!(cfg.name, "unnamed-agent");
        assert_eq!(cfg.agent_type, AgentType::Ai);
        assert!(cfg.capabilities.is_empty());
        assert_eq!(cfg.max_ticks, 0);
        assert!(cfg.g_threshold.is_none());
    }

    #[test]
    fn test_agent_trait_basics() {
        let agent = TestAgent::new("test-01");
        assert_eq!(agent.id().as_str(), "test-01");
        assert_eq!(agent.agent_type(), AgentType::Ai);
        assert!(agent.capabilities().is_empty());
        assert!(agent.behavior_vector().is_none());
    }

    #[test]
    fn test_agent_execute_noop() {
        let mut agent = TestAgent::new("idle");
        let ctx = ExecutionContext {
            tick: 0,
            node_name: "test-node".to_string(),
            g_metric: 0.0,
            halted: false,
            pending_messages: 0,
        };
        let action = agent.execute(&ctx).unwrap();
        assert!(matches!(action, AgentAction::Noop));
    }

    #[test]
    fn test_agent_execute_halted() {
        let mut agent = TestAgent::new("halted");
        let ctx = ExecutionContext {
            tick: 10,
            node_name: "test-node".to_string(),
            g_metric: 0.8,
            halted: true,
            pending_messages: 0,
        };
        let action = agent.execute(&ctx).unwrap();
        assert!(matches!(action, AgentAction::Noop));
    }

    #[test]
    fn test_agent_receive_message() {
        let mut agent = TestAgent::new("receiver");
        let msg = AgentMessage {
            id: 1,
            from: AgentId::new("sender"),
            to: AgentId::new("receiver"),
            kind: MessageKind::Data {
                payload: b"hello helm".to_vec(),
            },
            timestamp: 1000,
        };
        agent.receive(&msg).unwrap();
        assert_eq!(agent.received, vec!["hello helm"]);
    }

    #[test]
    fn agent_action_batch() {
        let batch = AgentAction::Batch(vec![
            AgentAction::Noop,
            AgentAction::Send {
                target: AgentId::new("peer"),
                payload: "data".to_string(),
            },
        ]);
        match batch {
            AgentAction::Batch(actions) => assert_eq!(actions.len(), 2),
            _ => panic!("expected batch"),
        }
    }

    #[test]
    fn agent_action_serialize() {
        let action = AgentAction::Store {
            key: b"key1".to_vec(),
            value: b"val1".to_vec(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("Store"));
        let decoded: AgentAction = serde_json::from_str(&json).unwrap();
        match decoded {
            AgentAction::Store { key, value } => {
                assert_eq!(key, b"key1");
                assert_eq!(value, b"val1");
            }
            _ => panic!("expected Store"),
        }
    }
}
