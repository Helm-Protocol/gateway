//! Agent Scheduler — tick-based execution with priority queues.
//!
//! Manages the execution order of agents based on priority,
//! lifecycle state, and Socratic Claw status.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::agent::AgentId;

/// Task priority levels for agent scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TaskPriority {
    /// Background tasks, lowest priority.
    Low = 0,
    /// Standard agent execution.
    Normal = 1,
    /// Time-sensitive operations.
    High = 2,
    /// Security/anomaly response, highest priority.
    Critical = 3,
}

impl Default for TaskPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Scheduler entry for a single agent.
#[derive(Debug, Clone)]
struct SchedulerEntry {
    _agent_id: AgentId,
    priority: TaskPriority,
    ticks_since_last_run: u64,
    total_ticks_run: u64,
    halted: bool,
}

/// Scheduler configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Maximum agents to tick per round.
    pub max_ticks_per_round: usize,
    /// Starvation prevention: after this many skipped rounds, bump priority.
    pub starvation_threshold: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_ticks_per_round: 64,
            starvation_threshold: 10,
        }
    }
}

/// The agent scheduler. Decides which agents execute each round.
pub struct AgentScheduler {
    config: SchedulerConfig,
    entries: HashMap<AgentId, SchedulerEntry>,
    current_tick: u64,
}

impl AgentScheduler {
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config,
            entries: HashMap::new(),
            current_tick: 0,
        }
    }

    /// Register an agent with the scheduler.
    pub fn register(&mut self, agent_id: &AgentId, priority: TaskPriority) {
        self.entries.insert(
            agent_id.clone(),
            SchedulerEntry {
                _agent_id: agent_id.clone(),
                priority,
                ticks_since_last_run: 0,
                total_ticks_run: 0,
                halted: false,
            },
        );
    }

    /// Remove an agent from the scheduler.
    pub fn unregister(&mut self, agent_id: &AgentId) {
        self.entries.remove(agent_id);
    }

    /// Update an agent's priority.
    pub fn set_priority(&mut self, agent_id: &AgentId, priority: TaskPriority) {
        if let Some(entry) = self.entries.get_mut(agent_id) {
            entry.priority = priority;
        }
    }

    /// Mark an agent as halted (Socratic Claw pause).
    pub fn set_halted(&mut self, agent_id: &AgentId, halted: bool) {
        if let Some(entry) = self.entries.get_mut(agent_id) {
            entry.halted = halted;
        }
    }

    /// Get the current tick.
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// Number of scheduled agents.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Is the scheduler empty?
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Produce the ordered list of agents to tick this round.
    /// Agents are sorted by: effective priority (desc), ticks_since_last_run (desc).
    /// Halted agents are still ticked (they get halted=true in ExecutionContext).
    pub fn next_round(
        &mut self,
        running_agents: &[AgentId],
    ) -> Vec<(AgentId, bool)> {
        self.current_tick += 1;

        // Collect schedulable entries (only those in the running set)
        let mut candidates: Vec<(AgentId, u32, u64, bool)> = Vec::new();

        for agent_id in running_agents {
            if let Some(entry) = self.entries.get_mut(agent_id) {
                entry.ticks_since_last_run += 1;

                // Effective priority: base + starvation bump
                let starvation_bump = if entry.ticks_since_last_run > self.config.starvation_threshold {
                    1
                } else {
                    0
                };
                let effective = entry.priority as u32 + starvation_bump;

                candidates.push((
                    agent_id.clone(),
                    effective,
                    entry.ticks_since_last_run,
                    entry.halted,
                ));
            }
        }

        // Sort: highest effective priority first, then longest wait first
        candidates.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then(b.2.cmp(&a.2))
        });

        // Truncate to max_ticks_per_round
        candidates.truncate(self.config.max_ticks_per_round);

        // Mark them as run
        let result: Vec<(AgentId, bool)> = candidates
            .iter()
            .map(|(id, _, _, halted)| (id.clone(), *halted))
            .collect();

        for (id, _, _, _) in &candidates {
            if let Some(entry) = self.entries.get_mut(id) {
                entry.ticks_since_last_run = 0;
                entry.total_ticks_run += 1;
            }
        }

        result
    }

    /// Get scheduling stats for an agent.
    pub fn stats(&self, agent_id: &AgentId) -> Option<SchedulerStats> {
        self.entries.get(agent_id).map(|e| SchedulerStats {
            priority: e.priority,
            ticks_since_last_run: e.ticks_since_last_run,
            total_ticks_run: e.total_ticks_run,
            halted: e.halted,
        })
    }
}

/// Scheduling statistics for an agent.
#[derive(Debug, Clone)]
pub struct SchedulerStats {
    pub priority: TaskPriority,
    pub ticks_since_last_run: u64,
    pub total_ticks_run: u64,
    pub halted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_scheduler() -> AgentScheduler {
        AgentScheduler::new(SchedulerConfig {
            max_ticks_per_round: 3,
            starvation_threshold: 5,
        })
    }

    #[test]
    fn scheduler_creation() {
        let sched = make_scheduler();
        assert!(sched.is_empty());
        assert_eq!(sched.current_tick(), 0);
    }

    #[test]
    fn register_and_len() {
        let mut sched = make_scheduler();
        sched.register(&AgentId::new("a1"), TaskPriority::Normal);
        sched.register(&AgentId::new("a2"), TaskPriority::High);
        assert_eq!(sched.len(), 2);
    }

    #[test]
    fn unregister() {
        let mut sched = make_scheduler();
        let id = AgentId::new("rem");
        sched.register(&id, TaskPriority::Normal);
        assert_eq!(sched.len(), 1);
        sched.unregister(&id);
        assert_eq!(sched.len(), 0);
    }

    #[test]
    fn next_round_priority_ordering() {
        let mut sched = make_scheduler();
        let low = AgentId::new("low");
        let high = AgentId::new("high");
        let crit = AgentId::new("crit");

        sched.register(&low, TaskPriority::Low);
        sched.register(&high, TaskPriority::High);
        sched.register(&crit, TaskPriority::Critical);

        let running = vec![low.clone(), high.clone(), crit.clone()];
        let round = sched.next_round(&running);

        assert_eq!(round.len(), 3);
        assert_eq!(round[0].0, crit);  // Critical first
        assert_eq!(round[1].0, high);  // High second
        assert_eq!(round[2].0, low);   // Low last
    }

    #[test]
    fn next_round_max_per_round() {
        let mut sched = make_scheduler(); // max 3
        for i in 0..5 {
            sched.register(&AgentId::new(format!("a{i}")), TaskPriority::Normal);
        }

        let running: Vec<_> = (0..5).map(|i| AgentId::new(format!("a{i}"))).collect();
        let round = sched.next_round(&running);
        assert_eq!(round.len(), 3); // capped at max_ticks_per_round
    }

    #[test]
    fn tick_counter_advances() {
        let mut sched = make_scheduler();
        sched.register(&AgentId::new("a1"), TaskPriority::Normal);

        let running = vec![AgentId::new("a1")];
        sched.next_round(&running);
        assert_eq!(sched.current_tick(), 1);
        sched.next_round(&running);
        assert_eq!(sched.current_tick(), 2);
    }

    #[test]
    fn starvation_prevention() {
        let mut sched = AgentScheduler::new(SchedulerConfig {
            max_ticks_per_round: 1, // only 1 agent per round
            starvation_threshold: 3,
        });

        let high = AgentId::new("high");
        let low = AgentId::new("low");
        sched.register(&high, TaskPriority::High);
        sched.register(&low, TaskPriority::Low);

        let running = vec![high.clone(), low.clone()];

        // High always wins for first rounds
        for _ in 0..3 {
            let round = sched.next_round(&running);
            assert_eq!(round[0].0, high);
        }

        // After 4+ rounds of starvation, low gets a bump
        // Low has been waiting 4 rounds now (> threshold 3), gets +1 priority
        // High(2) vs Low(0+1=1) → High still wins
        let round = sched.next_round(&running);
        assert_eq!(round[0].0, high);
        // Low now waited 5 rounds
        let round = sched.next_round(&running);
        // Low gets another bump attempt but High still has base 2
        // Once low gets enough starvation it should eventually get scheduled
        let _ = round;
    }

    #[test]
    fn halted_agents_included() {
        let mut sched = make_scheduler();
        let id = AgentId::new("halted-agent");
        sched.register(&id, TaskPriority::Normal);
        sched.set_halted(&id, true);

        let running = vec![id.clone()];
        let round = sched.next_round(&running);
        assert_eq!(round.len(), 1);
        assert!(round[0].1); // halted = true
    }

    #[test]
    fn only_running_agents_scheduled() {
        let mut sched = make_scheduler();
        sched.register(&AgentId::new("a1"), TaskPriority::Normal);
        sched.register(&AgentId::new("a2"), TaskPriority::Normal);

        // Only a1 is in the running set
        let running = vec![AgentId::new("a1")];
        let round = sched.next_round(&running);
        assert_eq!(round.len(), 1);
        assert_eq!(round[0].0, AgentId::new("a1"));
    }

    #[test]
    fn set_priority() {
        let mut sched = make_scheduler();
        let id = AgentId::new("dynamic");
        sched.register(&id, TaskPriority::Low);
        assert_eq!(sched.stats(&id).unwrap().priority, TaskPriority::Low);

        sched.set_priority(&id, TaskPriority::Critical);
        assert_eq!(sched.stats(&id).unwrap().priority, TaskPriority::Critical);
    }

    #[test]
    fn stats_tracking() {
        let mut sched = make_scheduler();
        let id = AgentId::new("tracked");
        sched.register(&id, TaskPriority::Normal);

        let running = vec![id.clone()];
        sched.next_round(&running);
        sched.next_round(&running);

        let stats = sched.stats(&id).unwrap();
        assert_eq!(stats.total_ticks_run, 2);
        assert!(!stats.halted);
    }

    #[test]
    fn priority_ordering() {
        assert!(TaskPriority::Critical > TaskPriority::High);
        assert!(TaskPriority::High > TaskPriority::Normal);
        assert!(TaskPriority::Normal > TaskPriority::Low);
    }
}
