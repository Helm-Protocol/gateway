//! Agent lifecycle state machine.
//!
//! Governs the lifespan of agents from creation to termination.
//! Enforces valid state transitions to prevent invalid agent states.
//!
//! ```text
//! Created → Initializing → Ready → Running → Suspended → Running (resume)
//!                                     ↓           ↓
//!                                Terminating  Terminating
//!                                     ↓           ↓
//!                                Terminated   Terminated
//! ```

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Lifecycle states for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleState {
    /// Agent has been instantiated but not initialized.
    Created,
    /// Agent is running init() logic.
    Initializing,
    /// Agent is initialized and ready to run.
    Ready,
    /// Agent is actively executing ticks.
    Running,
    /// Agent is paused (voluntary or Socratic halt).
    Suspended,
    /// Agent is shutting down.
    Terminating,
    /// Agent is permanently stopped.
    Terminated,
}

impl std::fmt::Display for LifecycleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "Created"),
            Self::Initializing => write!(f, "Initializing"),
            Self::Ready => write!(f, "Ready"),
            Self::Running => write!(f, "Running"),
            Self::Suspended => write!(f, "Suspended"),
            Self::Terminating => write!(f, "Terminating"),
            Self::Terminated => write!(f, "Terminated"),
        }
    }
}

#[derive(Debug, Error)]
pub enum LifecycleError {
    #[error("Invalid transition: {from} → {to}")]
    InvalidTransition {
        from: LifecycleState,
        to: LifecycleState,
    },
    #[error("Agent is terminated and cannot transition")]
    AlreadyTerminated,
}

/// A recorded state transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub from: LifecycleState,
    pub to: LifecycleState,
    pub tick: u64,
}

/// The lifecycle state machine for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lifecycle {
    state: LifecycleState,
    created_tick: u64,
    transitions: Vec<Transition>,
    suspend_count: u32,
}

impl Lifecycle {
    /// Create a new lifecycle in the Created state.
    pub fn new() -> Self {
        Self {
            state: LifecycleState::Created,
            created_tick: 0,
            transitions: Vec::new(),
            suspend_count: 0,
        }
    }

    /// Create a lifecycle with a specific creation tick.
    pub fn with_tick(tick: u64) -> Self {
        Self {
            state: LifecycleState::Created,
            created_tick: tick,
            transitions: Vec::new(),
            suspend_count: 0,
        }
    }

    /// Current state.
    pub fn state(&self) -> LifecycleState {
        self.state
    }

    /// Tick when this lifecycle was created.
    pub fn created_tick(&self) -> u64 {
        self.created_tick
    }

    /// Number of times this agent has been suspended.
    pub fn suspend_count(&self) -> u32 {
        self.suspend_count
    }

    /// Full transition history.
    pub fn transitions(&self) -> &[Transition] {
        &self.transitions
    }

    /// Is the agent in a runnable state?
    pub fn is_active(&self) -> bool {
        matches!(self.state, LifecycleState::Running)
    }

    /// Can the agent accept new messages?
    pub fn can_receive(&self) -> bool {
        matches!(
            self.state,
            LifecycleState::Ready | LifecycleState::Running | LifecycleState::Suspended
        )
    }

    /// Is the agent terminated?
    pub fn is_terminated(&self) -> bool {
        matches!(self.state, LifecycleState::Terminated)
    }

    /// Attempt a state transition. Returns error if the transition is invalid.
    pub fn transition_to(&mut self, to: LifecycleState, tick: u64) -> Result<(), LifecycleError> {
        if self.state == LifecycleState::Terminated {
            return Err(LifecycleError::AlreadyTerminated);
        }

        if !Self::is_valid_transition(self.state, to) {
            return Err(LifecycleError::InvalidTransition {
                from: self.state,
                to,
            });
        }

        if to == LifecycleState::Suspended {
            self.suspend_count += 1;
        }

        let from = self.state;
        self.state = to;
        self.transitions.push(Transition { from, to, tick });

        Ok(())
    }

    /// Check if a transition is allowed by the state machine rules.
    pub fn is_valid_transition(from: LifecycleState, to: LifecycleState) -> bool {
        matches!(
            (from, to),
            (LifecycleState::Created, LifecycleState::Initializing)
                | (LifecycleState::Initializing, LifecycleState::Ready)
                | (LifecycleState::Ready, LifecycleState::Running)
                | (LifecycleState::Running, LifecycleState::Suspended)
                | (LifecycleState::Running, LifecycleState::Terminating)
                | (LifecycleState::Suspended, LifecycleState::Running)
                | (LifecycleState::Suspended, LifecycleState::Terminating)
                | (LifecycleState::Terminating, LifecycleState::Terminated)
                // Fast-track: Ready → Terminating (abort before first tick)
                | (LifecycleState::Ready, LifecycleState::Terminating)
        )
    }
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_lifecycle_starts_created() {
        let lc = Lifecycle::new();
        assert_eq!(lc.state(), LifecycleState::Created);
        assert!(!lc.is_active());
        assert!(!lc.is_terminated());
        assert!(!lc.can_receive());
    }

    #[test]
    fn lifecycle_with_tick() {
        let lc = Lifecycle::with_tick(42);
        assert_eq!(lc.created_tick(), 42);
        assert_eq!(lc.state(), LifecycleState::Created);
    }

    #[test]
    fn full_lifecycle_happy_path() {
        let mut lc = Lifecycle::new();

        lc.transition_to(LifecycleState::Initializing, 1).unwrap();
        assert_eq!(lc.state(), LifecycleState::Initializing);

        lc.transition_to(LifecycleState::Ready, 2).unwrap();
        assert_eq!(lc.state(), LifecycleState::Ready);
        assert!(lc.can_receive());

        lc.transition_to(LifecycleState::Running, 3).unwrap();
        assert!(lc.is_active());
        assert!(lc.can_receive());

        lc.transition_to(LifecycleState::Terminating, 100).unwrap();
        lc.transition_to(LifecycleState::Terminated, 101).unwrap();
        assert!(lc.is_terminated());
        assert!(!lc.is_active());
    }

    #[test]
    fn suspend_and_resume() {
        let mut lc = Lifecycle::new();
        lc.transition_to(LifecycleState::Initializing, 0).unwrap();
        lc.transition_to(LifecycleState::Ready, 1).unwrap();
        lc.transition_to(LifecycleState::Running, 2).unwrap();

        // Suspend
        lc.transition_to(LifecycleState::Suspended, 5).unwrap();
        assert_eq!(lc.state(), LifecycleState::Suspended);
        assert_eq!(lc.suspend_count(), 1);
        assert!(lc.can_receive());
        assert!(!lc.is_active());

        // Resume
        lc.transition_to(LifecycleState::Running, 10).unwrap();
        assert!(lc.is_active());

        // Suspend again
        lc.transition_to(LifecycleState::Suspended, 15).unwrap();
        assert_eq!(lc.suspend_count(), 2);
    }

    #[test]
    fn terminate_from_suspended() {
        let mut lc = Lifecycle::new();
        lc.transition_to(LifecycleState::Initializing, 0).unwrap();
        lc.transition_to(LifecycleState::Ready, 1).unwrap();
        lc.transition_to(LifecycleState::Running, 2).unwrap();
        lc.transition_to(LifecycleState::Suspended, 3).unwrap();
        lc.transition_to(LifecycleState::Terminating, 4).unwrap();
        lc.transition_to(LifecycleState::Terminated, 5).unwrap();
        assert!(lc.is_terminated());
    }

    #[test]
    fn invalid_transition_created_to_running() {
        let mut lc = Lifecycle::new();
        let err = lc.transition_to(LifecycleState::Running, 0).unwrap_err();
        assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
    }

    #[test]
    fn invalid_transition_ready_to_suspended() {
        let mut lc = Lifecycle::new();
        lc.transition_to(LifecycleState::Initializing, 0).unwrap();
        lc.transition_to(LifecycleState::Ready, 1).unwrap();
        let err = lc.transition_to(LifecycleState::Suspended, 2).unwrap_err();
        assert!(matches!(err, LifecycleError::InvalidTransition { .. }));
    }

    #[test]
    fn terminated_cannot_transition() {
        let mut lc = Lifecycle::new();
        lc.transition_to(LifecycleState::Initializing, 0).unwrap();
        lc.transition_to(LifecycleState::Ready, 1).unwrap();
        lc.transition_to(LifecycleState::Terminating, 2).unwrap();
        lc.transition_to(LifecycleState::Terminated, 3).unwrap();

        let err = lc.transition_to(LifecycleState::Running, 4).unwrap_err();
        assert!(matches!(err, LifecycleError::AlreadyTerminated));
    }

    #[test]
    fn transition_history_recorded() {
        let mut lc = Lifecycle::new();
        lc.transition_to(LifecycleState::Initializing, 0).unwrap();
        lc.transition_to(LifecycleState::Ready, 1).unwrap();
        lc.transition_to(LifecycleState::Running, 2).unwrap();

        let history = lc.transitions();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].from, LifecycleState::Created);
        assert_eq!(history[0].to, LifecycleState::Initializing);
        assert_eq!(history[2].from, LifecycleState::Ready);
        assert_eq!(history[2].to, LifecycleState::Running);
        assert_eq!(history[2].tick, 2);
    }

    #[test]
    fn fast_track_ready_to_terminating() {
        let mut lc = Lifecycle::new();
        lc.transition_to(LifecycleState::Initializing, 0).unwrap();
        lc.transition_to(LifecycleState::Ready, 1).unwrap();
        // Abort before ever running
        lc.transition_to(LifecycleState::Terminating, 2).unwrap();
        lc.transition_to(LifecycleState::Terminated, 3).unwrap();
        assert!(lc.is_terminated());
    }

    #[test]
    fn lifecycle_state_display() {
        assert_eq!(LifecycleState::Created.to_string(), "Created");
        assert_eq!(LifecycleState::Running.to_string(), "Running");
        assert_eq!(LifecycleState::Terminated.to_string(), "Terminated");
    }

    #[test]
    fn valid_transitions_checked() {
        assert!(Lifecycle::is_valid_transition(
            LifecycleState::Created,
            LifecycleState::Initializing
        ));
        assert!(!Lifecycle::is_valid_transition(
            LifecycleState::Created,
            LifecycleState::Running
        ));
        assert!(Lifecycle::is_valid_transition(
            LifecycleState::Suspended,
            LifecycleState::Running
        ));
        assert!(!Lifecycle::is_valid_transition(
            LifecycleState::Terminated,
            LifecycleState::Created
        ));
    }

    #[test]
    fn lifecycle_serialization() {
        let mut lc = Lifecycle::new();
        lc.transition_to(LifecycleState::Initializing, 0).unwrap();
        lc.transition_to(LifecycleState::Ready, 1).unwrap();
        let json = serde_json::to_string(&lc).unwrap();
        let decoded: Lifecycle = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.state(), LifecycleState::Ready);
        assert_eq!(decoded.transitions().len(), 2);
    }
}
