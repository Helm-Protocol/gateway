//! AgentPlugin — integrates the agent framework with helm-core's Plugin system.
//!
//! This plugin manages all agents on the node: initializing them,
//! running the scheduler each tick, processing actions, and
//! routing messages through the registry.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use helm_core::{Plugin, PluginContext};
use helm_net::protocol::HelmMessage;

use crate::agent::{AgentId, AgentAction};
use crate::behavior::{BehaviorEngine, BehaviorEngineConfig};
use crate::message::{AgentMessage, MessageKind};
use crate::registry::{AgentRegistry, RegistryConfig};
use crate::scheduler::{AgentScheduler, SchedulerConfig, TaskPriority};
use crate::socratic::claw::SocraticClaw;

/// Configuration for the AgentPlugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPluginConfig {
    /// Maximum agents.
    pub max_agents: usize,
    /// Default mailbox capacity.
    pub mailbox_capacity: usize,
    /// Max agents ticked per round.
    pub max_ticks_per_round: usize,
    /// Behavior vector dimension (should match QKV-G model dim).
    pub vector_dim: usize,
    /// Latent dimension for Socratic Claw gap compression.
    pub latent_dim: usize,
    /// G-metric threshold for Socratic Claw.
    pub g_threshold: f32,
}

impl Default for AgentPluginConfig {
    fn default() -> Self {
        Self {
            max_agents: 1024,
            mailbox_capacity: 256,
            max_ticks_per_round: 64,
            vector_dim: 64,
            latent_dim: 8,
            g_threshold: 0.4,
        }
    }
}

/// The AgentPlugin — runs inside the helm-core EventLoop.
pub struct AgentPlugin {
    config: AgentPluginConfig,
    registry: AgentRegistry,
    scheduler: AgentScheduler,
    behavior_engine: BehaviorEngine,
    socratic_claw: SocraticClaw,
    tick_count: u64,
    actions_processed: u64,
}

impl AgentPlugin {
    pub fn new(config: AgentPluginConfig) -> Self {
        let registry = AgentRegistry::new(RegistryConfig {
            max_agents: config.max_agents,
            default_mailbox_capacity: config.mailbox_capacity,
        });

        let scheduler = AgentScheduler::new(SchedulerConfig {
            max_ticks_per_round: config.max_ticks_per_round,
            ..Default::default()
        });

        let behavior_engine = BehaviorEngine::new(BehaviorEngineConfig {
            vector_dim: config.vector_dim,
            ..Default::default()
        });

        let socratic_claw = SocraticClaw::new(config.vector_dim, config.latent_dim)
            .with_threshold(config.g_threshold);

        Self {
            config,
            registry,
            scheduler,
            behavior_engine,
            socratic_claw,
            tick_count: 0,
            actions_processed: 0,
        }
    }

    /// Access the agent registry.
    pub fn registry(&self) -> &AgentRegistry {
        &self.registry
    }

    /// Mutable access to the agent registry.
    pub fn registry_mut(&mut self) -> &mut AgentRegistry {
        &mut self.registry
    }

    /// Access the behavior engine.
    pub fn behavior_engine(&self) -> &BehaviorEngine {
        &self.behavior_engine
    }

    /// Access the Socratic Claw.
    pub fn socratic_claw(&self) -> &SocraticClaw {
        &self.socratic_claw
    }

    /// Register and initialize a new agent.
    pub fn add_agent(
        &mut self,
        agent: Box<dyn crate::agent::Agent>,
        priority: TaskPriority,
    ) -> Result<AgentId> {
        let id = agent.id().clone();
        self.registry.register(agent)?;
        self.scheduler.register(&id, priority);
        self.behavior_engine.register(&id);

        // Initialize the agent through lifecycle
        self.registry.init_agent(&id, self.tick_count)?;
        self.registry.start_agent(&id, self.tick_count)?;

        tracing::info!(agent = %id, "Agent added and started");
        Ok(id)
    }

    /// Remove an agent.
    pub fn remove_agent(&mut self, id: &AgentId) -> Result<()> {
        self.registry.terminate_agent(id, self.tick_count)?;
        self.scheduler.unregister(id);
        Ok(())
    }

    /// Process a single agent action.
    fn process_action(&mut self, from: &AgentId, action: AgentAction) {
        match action {
            AgentAction::Noop => {}
            AgentAction::Send { target, payload } => {
                let msg_kind = MessageKind::Text { content: payload };
                let _ = self.registry.send_message(from, &target, msg_kind, self.tick_count);
            }
            AgentAction::Broadcast { payload } => {
                let all_ids = self.registry.agent_ids();
                for target in all_ids {
                    if &target != from {
                        let msg_kind = MessageKind::Text { content: payload.clone() };
                        let _ = self.registry.send_message(from, &target, msg_kind, self.tick_count);
                    }
                }
            }
            AgentAction::Suspend { reason } => {
                tracing::info!(agent = %from, reason = %reason, "Agent self-suspending");
                let _ = self.registry.suspend_agent(from, self.tick_count);
            }
            AgentAction::Terminate { reason } => {
                tracing::info!(agent = %from, reason = %reason, "Agent self-terminating");
                let _ = self.registry.terminate_agent(from, self.tick_count);
                self.scheduler.unregister(from);
            }
            AgentAction::SocraticAnswer { gap_id, answer } => {
                self.socratic_claw.submit_answer(gap_id, &answer);
            }
            AgentAction::Batch(actions) => {
                for a in actions {
                    self.process_action(from, a);
                }
            }
            AgentAction::Store { .. } | AgentAction::RequestCapability { .. } => {
                // These need cross-plugin coordination (store plugin, etc.)
                // Logged for now, will be routed in Phase 4 integration
                tracing::debug!(agent = %from, "Action requires cross-plugin routing");
            }
        }
        self.actions_processed += 1;
    }

    /// Total actions processed.
    pub fn actions_processed(&self) -> u64 {
        self.actions_processed
    }

    /// Total ticks executed.
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }
}

#[async_trait]
impl Plugin for AgentPlugin {
    fn name(&self) -> &str {
        "helm-agent"
    }

    async fn on_start(&mut self, ctx: &PluginContext) -> Result<()> {
        tracing::info!(
            node = %ctx.node_name,
            max_agents = self.config.max_agents,
            g_threshold = self.config.g_threshold,
            "AgentPlugin started"
        );
        Ok(())
    }

    async fn on_message(&mut self, _ctx: &PluginContext, msg: &HelmMessage) -> Result<()> {
        // Route incoming network messages to appropriate agents
        if let Some(target) = msg.payload.get("agent_target") {
            if let Some(target_str) = target.as_str() {
                let target_id = AgentId::new(target_str);
                if self.registry.contains(&target_id) {
                    let payload = msg.payload.get("data")
                        .and_then(|d| d.as_str())
                        .map(|s| s.as_bytes().to_vec())
                        .unwrap_or_default();

                    let source = msg.payload.get("source")
                        .and_then(|s| s.as_str())
                        .unwrap_or("network");

                    let agent_msg = AgentMessage::data(
                        self.tick_count,
                        AgentId::new(source),
                        target_id,
                        payload,
                        self.tick_count,
                    );
                    self.registry.deliver(agent_msg);
                }
            }
        }
        Ok(())
    }

    async fn on_tick(&mut self, ctx: &PluginContext) -> Result<()> {
        self.tick_count += 1;

        // Get all running agents
        let running = self.registry.running_agents();

        // Schedule this round
        let schedule = self.scheduler.next_round(&running);

        // Execute each scheduled agent
        let mut actions_to_process: Vec<(AgentId, AgentAction)> = Vec::new();

        for (agent_id, halted) in &schedule {
            // Get behavior vector for Socratic Claw evaluation
            let g_metric = if let Some(agent) = self.registry.get(agent_id) {
                if let Some(bv) = agent.behavior_vector() {
                    let decision = self.socratic_claw.intercept(0.0, &bv, agent_id.as_str());
                    match decision {
                        crate::socratic::claw::SocraticDecision::Halt { g_metric, .. } => {
                            self.scheduler.set_halted(agent_id, true);
                            g_metric
                        }
                        crate::socratic::claw::SocraticDecision::Proceed { g_metric } => {
                            self.scheduler.set_halted(agent_id, false);
                            g_metric
                        }
                    }
                } else {
                    0.0
                }
            } else {
                0.0
            };

            // Tick the agent
            match self.registry.tick_agent(
                agent_id,
                &ctx.node_name,
                self.tick_count,
                g_metric,
                *halted,
            ) {
                Ok(action) => {
                    // Record behavior
                    if let Some(agent) = self.registry.get(agent_id) {
                        if let Some(bv) = agent.behavior_vector() {
                            self.behavior_engine.record(
                                agent_id, &bv, g_metric, self.tick_count,
                            );
                        }
                    }
                    actions_to_process.push((agent_id.clone(), action));
                }
                Err(e) => {
                    tracing::warn!(agent = %agent_id, error = %e, "Agent tick failed");
                }
            }
        }

        // Process actions after all ticks (avoids borrow conflicts)
        for (agent_id, action) in actions_to_process {
            self.process_action(&agent_id, action);
        }

        Ok(())
    }

    async fn on_shutdown(&mut self, _ctx: &PluginContext) -> Result<()> {
        let agents = self.registry.agent_ids();
        for id in &agents {
            if let Some(agent) = self.registry.get(id) {
                if !agent.lifecycle().is_terminated() {
                    let _ = self.registry.terminate_agent(id, self.tick_count);
                }
            }
        }

        let (total_gaps, unresolved, avg_severity) = self.socratic_claw.meta_cognition();
        tracing::info!(
            agents = agents.len(),
            ticks = self.tick_count,
            actions = self.actions_processed,
            gaps_total = total_gaps,
            gaps_unresolved = unresolved,
            gap_avg_severity = format!("{:.3}", avg_severity),
            "AgentPlugin shutdown"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, AgentType, ExecutionContext, AgentAction};
    use crate::capability::Capability;
    use crate::lifecycle::Lifecycle;

    struct PluginTestAgent {
        id: AgentId,
        lifecycle: Lifecycle,
        execute_count: u32,
    }

    impl PluginTestAgent {
        fn new(id: &str) -> Self {
            Self {
                id: AgentId::new(id),
                lifecycle: Lifecycle::new(),
                execute_count: 0,
            }
        }
    }

    impl Agent for PluginTestAgent {
        fn id(&self) -> &AgentId { &self.id }
        fn agent_type(&self) -> AgentType { AgentType::Ai }
        fn capabilities(&self) -> &[Capability] { &[] }
        fn lifecycle(&self) -> &Lifecycle { &self.lifecycle }
        fn lifecycle_mut(&mut self) -> &mut Lifecycle { &mut self.lifecycle }

        fn execute(&mut self, _ctx: &ExecutionContext) -> Result<AgentAction> {
            self.execute_count += 1;
            Ok(AgentAction::Noop)
        }

        fn receive(&mut self, _msg: &AgentMessage) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn plugin_creation() {
        let plugin = AgentPlugin::new(AgentPluginConfig::default());
        assert_eq!(plugin.name(), "helm-agent");
        assert_eq!(plugin.tick_count(), 0);
        assert_eq!(plugin.actions_processed(), 0);
    }

    #[test]
    fn add_and_remove_agent() {
        let mut plugin = AgentPlugin::new(AgentPluginConfig::default());
        let agent = PluginTestAgent::new("test-1");

        let id = plugin.add_agent(Box::new(agent), TaskPriority::Normal).unwrap();
        assert_eq!(id.as_str(), "test-1");
        assert_eq!(plugin.registry().len(), 1);

        plugin.remove_agent(&id).unwrap();
        // Agent is terminated but still in registry
        assert!(plugin.registry().get(&id).unwrap().lifecycle().is_terminated());
    }

    #[test]
    fn plugin_config_defaults() {
        let cfg = AgentPluginConfig::default();
        assert_eq!(cfg.max_agents, 1024);
        assert_eq!(cfg.mailbox_capacity, 256);
        assert_eq!(cfg.vector_dim, 64);
        assert_eq!(cfg.latent_dim, 8);
        assert_eq!(cfg.g_threshold, 0.4);
    }

    #[tokio::test]
    async fn plugin_on_start() {
        let mut plugin = AgentPlugin::new(AgentPluginConfig::default());
        let ctx = PluginContext {
            node_name: "test-node".to_string(),
        };
        plugin.on_start(&ctx).await.unwrap();
    }

    #[tokio::test]
    async fn plugin_on_tick_with_agents() {
        let mut plugin = AgentPlugin::new(AgentPluginConfig::default());
        let ctx = PluginContext {
            node_name: "test-node".to_string(),
        };

        plugin.add_agent(Box::new(PluginTestAgent::new("a1")), TaskPriority::Normal).unwrap();
        plugin.add_agent(Box::new(PluginTestAgent::new("a2")), TaskPriority::High).unwrap();

        // Run a tick
        plugin.on_tick(&ctx).await.unwrap();
        assert_eq!(plugin.tick_count(), 1);
    }

    #[tokio::test]
    async fn plugin_on_shutdown() {
        let mut plugin = AgentPlugin::new(AgentPluginConfig::default());
        let ctx = PluginContext {
            node_name: "test-node".to_string(),
        };

        plugin.add_agent(Box::new(PluginTestAgent::new("a1")), TaskPriority::Normal).unwrap();
        plugin.on_shutdown(&ctx).await.unwrap();

        // All agents should be terminated
        let id = AgentId::new("a1");
        assert!(plugin.registry().get(&id).unwrap().lifecycle().is_terminated());
    }

    #[test]
    fn process_send_action() {
        let mut plugin = AgentPlugin::new(AgentPluginConfig::default());
        plugin.add_agent(Box::new(PluginTestAgent::new("sender")), TaskPriority::Normal).unwrap();
        plugin.add_agent(Box::new(PluginTestAgent::new("receiver")), TaskPriority::Normal).unwrap();

        let from = AgentId::new("sender");
        plugin.process_action(&from, AgentAction::Send {
            target: AgentId::new("receiver"),
            payload: "hello".to_string(),
        });
        assert_eq!(plugin.actions_processed(), 1);
    }

    #[test]
    fn process_batch_action() {
        let mut plugin = AgentPlugin::new(AgentPluginConfig::default());
        plugin.add_agent(Box::new(PluginTestAgent::new("batcher")), TaskPriority::Normal).unwrap();

        let from = AgentId::new("batcher");
        plugin.process_action(&from, AgentAction::Batch(vec![
            AgentAction::Noop,
            AgentAction::Noop,
        ]));
        // Batch counts as 1 + 2 inner actions
        assert_eq!(plugin.actions_processed(), 3);
    }

    #[test]
    fn socratic_claw_accessible() {
        let plugin = AgentPlugin::new(AgentPluginConfig {
            g_threshold: 0.6,
            ..Default::default()
        });
        assert_eq!(plugin.socratic_claw().threshold(), 0.6);
    }
}
