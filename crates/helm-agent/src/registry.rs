//! Agent Registry — concurrent agent management and discovery.
//!
//! Manages all active agents on a node, providing registration,
//! discovery by capability, lifecycle management, and message routing.

use std::collections::HashMap;
use anyhow::{Result, anyhow};

use crate::agent::{Agent, AgentId, AgentType, AgentAction, ExecutionContext};
use crate::capability::Capability;
use crate::lifecycle::LifecycleState;
use crate::message::{AgentMessage, Mailbox};

/// Entry in the registry for a single agent.
pub struct AgentEntry {
    /// The agent instance.
    agent: Box<dyn Agent>,
    /// Agent's mailbox.
    mailbox: Mailbox,
    /// Total ticks executed.
    tick_count: u64,
    /// Total actions produced.
    action_count: u64,
}

impl AgentEntry {
    fn new(agent: Box<dyn Agent>, mailbox_capacity: usize) -> Self {
        Self {
            agent,
            mailbox: Mailbox::new(mailbox_capacity),
            tick_count: 0,
            action_count: 0,
        }
    }
}

/// Configuration for the agent registry.
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Maximum number of agents allowed.
    pub max_agents: usize,
    /// Default mailbox capacity per agent.
    pub default_mailbox_capacity: usize,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            max_agents: 1024,
            default_mailbox_capacity: 256,
        }
    }
}

/// Central registry managing all agents on this node.
pub struct AgentRegistry {
    agents: HashMap<AgentId, AgentEntry>,
    config: RegistryConfig,
    next_msg_id: u64,
}

impl AgentRegistry {
    pub fn new(config: RegistryConfig) -> Self {
        Self {
            agents: HashMap::new(),
            config,
            next_msg_id: 1,
        }
    }

    /// Register a new agent. Fails if the registry is full or ID is duplicate.
    pub fn register(&mut self, agent: Box<dyn Agent>) -> Result<()> {
        let id = agent.id().clone();

        if self.agents.len() >= self.config.max_agents {
            return Err(anyhow!("Registry full: max {} agents", self.config.max_agents));
        }

        if self.agents.contains_key(&id) {
            return Err(anyhow!("Agent already registered: {}", id));
        }

        let entry = AgentEntry::new(agent, self.config.default_mailbox_capacity);
        self.agents.insert(id.clone(), entry);

        tracing::info!(agent = %id, "Agent registered");
        Ok(())
    }

    /// Unregister an agent by ID. Returns the agent if found.
    pub fn unregister(&mut self, id: &AgentId) -> Option<Box<dyn Agent>> {
        self.agents.remove(id).map(|entry| {
            tracing::info!(agent = %id, "Agent unregistered");
            entry.agent
        })
    }

    /// Get an immutable reference to an agent.
    pub fn get(&self, id: &AgentId) -> Option<&dyn Agent> {
        self.agents.get(id).map(|e| e.agent.as_ref())
    }

    /// Get a mutable reference to an agent.
    pub fn get_mut(&mut self, id: &AgentId) -> Option<&mut Box<dyn Agent>> {
        self.agents.get_mut(id).map(|e| &mut e.agent)
    }

    /// Check if an agent is registered.
    pub fn contains(&self, id: &AgentId) -> bool {
        self.agents.contains_key(id)
    }

    /// Number of registered agents.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Is the registry empty?
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// List all registered agent IDs.
    pub fn agent_ids(&self) -> Vec<AgentId> {
        self.agents.keys().cloned().collect()
    }

    /// Find agents by capability.
    pub fn find_by_capability(&self, cap: &Capability) -> Vec<AgentId> {
        self.agents
            .iter()
            .filter(|(_, entry)| entry.agent.capabilities().contains(cap))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Find agents by type.
    pub fn find_by_type(&self, agent_type: AgentType) -> Vec<AgentId> {
        self.agents
            .iter()
            .filter(|(_, entry)| entry.agent.agent_type() == agent_type)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Find agents in a specific lifecycle state.
    pub fn find_by_state(&self, state: LifecycleState) -> Vec<AgentId> {
        self.agents
            .iter()
            .filter(|(_, entry)| entry.agent.lifecycle().state() == state)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Deliver a message to an agent's mailbox. Returns false if mailbox is full.
    pub fn deliver(&mut self, msg: AgentMessage) -> bool {
        if let Some(entry) = self.agents.get_mut(&msg.to) {
            entry.mailbox.push(msg)
        } else {
            false
        }
    }

    /// Send a message from one agent to another.
    pub fn send_message(
        &mut self,
        from: &AgentId,
        to: &AgentId,
        kind: crate::message::MessageKind,
        timestamp: u64,
    ) -> Result<()> {
        if !self.agents.contains_key(from) {
            return Err(anyhow!("Sender not found: {}", from));
        }
        if !self.agents.contains_key(to) {
            return Err(anyhow!("Recipient not found: {}", to));
        }

        let msg_id = self.next_msg_id;
        self.next_msg_id += 1;

        let msg = AgentMessage::new(msg_id, from.clone(), to.clone(), kind, timestamp);

        if !self.deliver(msg) {
            return Err(anyhow!("Mailbox full for agent: {}", to));
        }
        Ok(())
    }

    /// Execute a single tick for an agent. Delivers pending messages first,
    /// then calls execute(), collecting the resulting action.
    pub fn tick_agent(
        &mut self,
        id: &AgentId,
        node_name: &str,
        tick: u64,
        g_metric: f32,
        halted: bool,
    ) -> Result<AgentAction> {
        let entry = self.agents.get_mut(id)
            .ok_or_else(|| anyhow!("Agent not found: {}", id))?;

        // Deliver pending messages to the agent
        let pending = entry.mailbox.len();
        let messages: Vec<AgentMessage> = entry.mailbox.drain_all();
        for msg in &messages {
            entry.agent.receive(msg)?;
        }

        let ctx = ExecutionContext {
            tick,
            node_name: node_name.to_string(),
            g_metric,
            halted,
            pending_messages: pending,
        };

        let action = entry.agent.execute(&ctx)?;
        entry.tick_count += 1;
        entry.action_count += 1;

        Ok(action)
    }

    /// Initialize an agent (Created → Initializing → Ready).
    pub fn init_agent(&mut self, id: &AgentId, tick: u64) -> Result<()> {
        let entry = self.agents.get_mut(id)
            .ok_or_else(|| anyhow!("Agent not found: {}", id))?;

        entry.agent.lifecycle_mut().transition_to(LifecycleState::Initializing, tick)?;
        entry.agent.init()?;
        entry.agent.lifecycle_mut().transition_to(LifecycleState::Ready, tick)?;

        tracing::debug!(agent = %id, "Agent initialized");
        Ok(())
    }

    /// Start an agent (Ready → Running).
    pub fn start_agent(&mut self, id: &AgentId, tick: u64) -> Result<()> {
        let entry = self.agents.get_mut(id)
            .ok_or_else(|| anyhow!("Agent not found: {}", id))?;

        entry.agent.lifecycle_mut().transition_to(LifecycleState::Running, tick)?;
        tracing::debug!(agent = %id, "Agent started");
        Ok(())
    }

    /// Suspend an agent (Running → Suspended).
    pub fn suspend_agent(&mut self, id: &AgentId, tick: u64) -> Result<()> {
        let entry = self.agents.get_mut(id)
            .ok_or_else(|| anyhow!("Agent not found: {}", id))?;

        entry.agent.lifecycle_mut().transition_to(LifecycleState::Suspended, tick)?;
        tracing::debug!(agent = %id, "Agent suspended");
        Ok(())
    }

    /// Resume a suspended agent (Suspended → Running).
    pub fn resume_agent(&mut self, id: &AgentId, tick: u64) -> Result<()> {
        let entry = self.agents.get_mut(id)
            .ok_or_else(|| anyhow!("Agent not found: {}", id))?;

        entry.agent.lifecycle_mut().transition_to(LifecycleState::Running, tick)?;
        tracing::debug!(agent = %id, "Agent resumed");
        Ok(())
    }

    /// Terminate an agent (→ Terminating → Terminated).
    pub fn terminate_agent(&mut self, id: &AgentId, tick: u64) -> Result<()> {
        let entry = self.agents.get_mut(id)
            .ok_or_else(|| anyhow!("Agent not found: {}", id))?;

        entry.agent.lifecycle_mut().transition_to(LifecycleState::Terminating, tick)?;
        entry.agent.shutdown()?;
        entry.agent.lifecycle_mut().transition_to(LifecycleState::Terminated, tick)?;

        tracing::info!(agent = %id, "Agent terminated");
        Ok(())
    }

    /// Get agent statistics.
    pub fn agent_stats(&self, id: &AgentId) -> Option<AgentStats> {
        self.agents.get(id).map(|e| AgentStats {
            id: id.clone(),
            agent_type: e.agent.agent_type(),
            state: e.agent.lifecycle().state(),
            tick_count: e.tick_count,
            action_count: e.action_count,
            mailbox_len: e.mailbox.len(),
            mailbox_total_received: e.mailbox.total_received(),
            mailbox_total_dropped: e.mailbox.total_dropped(),
        })
    }

    /// Get all running agent IDs.
    pub fn running_agents(&self) -> Vec<AgentId> {
        self.find_by_state(LifecycleState::Running)
    }
}

/// Statistics snapshot for an agent.
#[derive(Debug, Clone)]
pub struct AgentStats {
    pub id: AgentId,
    pub agent_type: AgentType,
    pub state: LifecycleState,
    pub tick_count: u64,
    pub action_count: u64,
    pub mailbox_len: usize,
    pub mailbox_total_received: u64,
    pub mailbox_total_dropped: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, AgentType, ExecutionContext, AgentAction};
    use crate::capability::Capability;
    use crate::lifecycle::Lifecycle;
    use crate::message::{AgentMessage, MessageKind};

    /// Simple test agent for registry tests.
    struct SimpleAgent {
        id: AgentId,
        agent_type: AgentType,
        capabilities: Vec<Capability>,
        lifecycle: Lifecycle,
        received_count: usize,
    }

    impl SimpleAgent {
        fn new(id: &str) -> Self {
            Self {
                id: AgentId::new(id),
                agent_type: AgentType::Ai,
                capabilities: Vec::new(),
                lifecycle: Lifecycle::new(),
                received_count: 0,
            }
        }

        fn with_type(mut self, t: AgentType) -> Self {
            self.agent_type = t;
            self
        }

        fn with_caps(mut self, caps: Vec<Capability>) -> Self {
            self.capabilities = caps;
            self
        }
    }

    impl Agent for SimpleAgent {
        fn id(&self) -> &AgentId { &self.id }
        fn agent_type(&self) -> AgentType { self.agent_type }
        fn capabilities(&self) -> &[Capability] { &self.capabilities }
        fn lifecycle(&self) -> &Lifecycle { &self.lifecycle }
        fn lifecycle_mut(&mut self) -> &mut Lifecycle { &mut self.lifecycle }

        fn execute(&mut self, _ctx: &ExecutionContext) -> Result<AgentAction> {
            Ok(AgentAction::Noop)
        }

        fn receive(&mut self, _msg: &AgentMessage) -> Result<()> {
            self.received_count += 1;
            Ok(())
        }
    }

    fn default_registry() -> AgentRegistry {
        AgentRegistry::new(RegistryConfig::default())
    }

    #[test]
    fn register_and_contains() {
        let mut reg = default_registry();
        let agent = SimpleAgent::new("agent-1");
        reg.register(Box::new(agent)).unwrap();
        assert!(reg.contains(&AgentId::new("agent-1")));
        assert!(!reg.contains(&AgentId::new("agent-2")));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn register_duplicate_fails() {
        let mut reg = default_registry();
        reg.register(Box::new(SimpleAgent::new("dup"))).unwrap();
        let result = reg.register(Box::new(SimpleAgent::new("dup")));
        assert!(result.is_err());
    }

    #[test]
    fn register_max_agents() {
        let mut reg = AgentRegistry::new(RegistryConfig {
            max_agents: 2,
            default_mailbox_capacity: 10,
        });
        reg.register(Box::new(SimpleAgent::new("a1"))).unwrap();
        reg.register(Box::new(SimpleAgent::new("a2"))).unwrap();
        let result = reg.register(Box::new(SimpleAgent::new("a3")));
        assert!(result.is_err());
    }

    #[test]
    fn unregister_agent() {
        let mut reg = default_registry();
        reg.register(Box::new(SimpleAgent::new("rem"))).unwrap();
        assert_eq!(reg.len(), 1);

        let removed = reg.unregister(&AgentId::new("rem"));
        assert!(removed.is_some());
        assert_eq!(reg.len(), 0);
        assert!(reg.is_empty());
    }

    #[test]
    fn find_by_capability() {
        let mut reg = default_registry();
        reg.register(Box::new(
            SimpleAgent::new("compute-1").with_caps(vec![Capability::Compute]),
        )).unwrap();
        reg.register(Box::new(
            SimpleAgent::new("storage-1").with_caps(vec![Capability::Storage]),
        )).unwrap();
        reg.register(Box::new(
            SimpleAgent::new("both").with_caps(vec![Capability::Compute, Capability::Storage]),
        )).unwrap();

        let compute_agents = reg.find_by_capability(&Capability::Compute);
        assert_eq!(compute_agents.len(), 2);

        let storage_agents = reg.find_by_capability(&Capability::Storage);
        assert_eq!(storage_agents.len(), 2);

        let gov_agents = reg.find_by_capability(&Capability::Governance);
        assert!(gov_agents.is_empty());
    }

    #[test]
    fn find_by_type() {
        let mut reg = default_registry();
        reg.register(Box::new(SimpleAgent::new("ai-1"))).unwrap();
        reg.register(Box::new(
            SimpleAgent::new("human-1").with_type(AgentType::Human),
        )).unwrap();

        let ai = reg.find_by_type(AgentType::Ai);
        assert_eq!(ai.len(), 1);

        let human = reg.find_by_type(AgentType::Human);
        assert_eq!(human.len(), 1);
    }

    #[test]
    fn agent_lifecycle_management() {
        let mut reg = default_registry();
        reg.register(Box::new(SimpleAgent::new("lc-agent"))).unwrap();

        let id = AgentId::new("lc-agent");
        reg.init_agent(&id, 0).unwrap();
        assert_eq!(
            reg.get(&id).unwrap().lifecycle().state(),
            LifecycleState::Ready,
        );

        reg.start_agent(&id, 1).unwrap();
        assert_eq!(
            reg.get(&id).unwrap().lifecycle().state(),
            LifecycleState::Running,
        );

        reg.suspend_agent(&id, 5).unwrap();
        assert_eq!(
            reg.get(&id).unwrap().lifecycle().state(),
            LifecycleState::Suspended,
        );

        reg.resume_agent(&id, 10).unwrap();
        assert_eq!(
            reg.get(&id).unwrap().lifecycle().state(),
            LifecycleState::Running,
        );

        reg.terminate_agent(&id, 100).unwrap();
        assert_eq!(
            reg.get(&id).unwrap().lifecycle().state(),
            LifecycleState::Terminated,
        );
    }

    #[test]
    fn send_and_tick() {
        let mut reg = default_registry();
        reg.register(Box::new(SimpleAgent::new("sender"))).unwrap();
        reg.register(Box::new(SimpleAgent::new("receiver"))).unwrap();

        let sender_id = AgentId::new("sender");
        let recv_id = AgentId::new("receiver");

        // Init and start receiver
        reg.init_agent(&recv_id, 0).unwrap();
        reg.start_agent(&recv_id, 1).unwrap();

        // Send messages
        reg.send_message(
            &sender_id,
            &recv_id,
            MessageKind::Text { content: "hello".to_string() },
            2,
        ).unwrap();

        reg.send_message(
            &sender_id,
            &recv_id,
            MessageKind::Text { content: "world".to_string() },
            3,
        ).unwrap();

        // Tick the receiver — it should process both messages
        let action = reg.tick_agent(&recv_id, "test-node", 4, 0.0, false).unwrap();
        assert!(matches!(action, AgentAction::Noop));
    }

    #[test]
    fn send_to_nonexistent_fails() {
        let mut reg = default_registry();
        reg.register(Box::new(SimpleAgent::new("sender"))).unwrap();

        let result = reg.send_message(
            &AgentId::new("sender"),
            &AgentId::new("ghost"),
            MessageKind::Ping,
            0,
        );
        assert!(result.is_err());
    }

    #[test]
    fn agent_stats() {
        let mut reg = default_registry();
        reg.register(Box::new(SimpleAgent::new("stats-agent"))).unwrap();

        let id = AgentId::new("stats-agent");
        reg.init_agent(&id, 0).unwrap();
        reg.start_agent(&id, 1).unwrap();

        // Tick a few times
        reg.tick_agent(&id, "node", 2, 0.1, false).unwrap();
        reg.tick_agent(&id, "node", 3, 0.2, false).unwrap();

        let stats = reg.agent_stats(&id).unwrap();
        assert_eq!(stats.tick_count, 2);
        assert_eq!(stats.state, LifecycleState::Running);
        assert_eq!(stats.agent_type, AgentType::Ai);
    }

    #[test]
    fn running_agents_filter() {
        let mut reg = default_registry();
        reg.register(Box::new(SimpleAgent::new("a1"))).unwrap();
        reg.register(Box::new(SimpleAgent::new("a2"))).unwrap();

        let a1 = AgentId::new("a1");
        let a2 = AgentId::new("a2");

        reg.init_agent(&a1, 0).unwrap();
        reg.start_agent(&a1, 1).unwrap();
        reg.init_agent(&a2, 0).unwrap();
        // a2 is Ready but not Running

        let running = reg.running_agents();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0], a1);
    }

    #[test]
    fn agent_ids_listing() {
        let mut reg = default_registry();
        reg.register(Box::new(SimpleAgent::new("x"))).unwrap();
        reg.register(Box::new(SimpleAgent::new("y"))).unwrap();

        let ids = reg.agent_ids();
        assert_eq!(ids.len(), 2);
    }
}
