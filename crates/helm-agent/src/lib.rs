//! Helm Agent — Autonomous Agent Framework for the Helm Protocol.
//!
//! Provides the core abstractions for creating, managing, and scheduling
//! autonomous agents within the Helm network. Each agent is a sovereign
//! entity with its own lifecycle, capabilities, and behavior profile.
//!
//! # Architecture
//!
//! - **Agent trait**: Core interface every agent implements
//! - **Lifecycle**: State machine governing agent lifespan
//! - **Registry**: Concurrent agent management and discovery
//! - **Capability**: Declarative capability system
//! - **Socratic Claw**: Gap-Aware Decision interceptor (QKV-G integration)
//! - **MLA Gap Repository**: Compressed ignorance storage via latent projections
//! - **Mailbox**: Async message passing between agents
//! - **Behavior Engine**: Pattern analysis and trust integration
//! - **Scheduler**: Tick-based agent execution with priority queues

pub mod agent;
pub mod capability;
pub mod lifecycle;
pub mod registry;
pub mod socratic;
pub mod message;
pub mod behavior;
pub mod scheduler;
pub mod plugin;
pub mod womb;
pub mod mining;

// Re-exports
pub use agent::{Agent, AgentId, AgentType, AgentConfig, ExecutionContext, AgentAction};
pub use capability::Capability;
pub use lifecycle::{Lifecycle, LifecycleState, LifecycleError};
pub use registry::AgentRegistry;
pub use socratic::claw::{SocraticClaw, SocraticDecision};
pub use socratic::gap_repo::{GapRepository, GapEntry};
pub use message::{AgentMessage, MessageKind, Mailbox};
pub use behavior::{BehaviorEngine, BehaviorProfile};
pub use scheduler::{AgentScheduler, SchedulerConfig, TaskPriority};
pub use plugin::{AgentPlugin, AgentPluginConfig};
pub use womb::{AgentWomb, AgentDna, BirthCertificate, WombConfig};
pub use mining::{MiningPool, MiningCategory, MiningReward, Contribution, AgentContributions};
