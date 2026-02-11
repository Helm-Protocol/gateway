//! Socratic Claw — Gap-Aware Decision interceptor.
//!
//! Sits at the entry of the agent execution loop. Uses the QKV-G
//! G-metric to detect knowledge gaps and halt execution when the
//! agent lacks sufficient information to proceed safely.

use serde::{Deserialize, Serialize};
use crate::socratic::gap_repo::{GapRepository, GapEntry};

/// Default G-metric threshold. Execution halts when G >= this value.
pub const DEFAULT_G_THRESHOLD: f32 = 0.4;

/// Result of the Socratic Claw evaluation.
#[derive(Debug, Clone)]
pub enum SocraticDecision {
    /// G < threshold — knowledge sufficient, proceed with execution.
    Proceed {
        g_metric: f32,
    },
    /// G >= threshold — knowledge gap detected, halt execution.
    Halt {
        g_metric: f32,
        gap_id: u64,
        question_hint: Vec<f32>,
    },
}

/// Socratic Claw interceptor state.
#[derive(Debug, Serialize, Deserialize)]
pub struct SocraticClaw {
    /// G-metric threshold for halting.
    g_threshold: f32,
    /// Gap repository for storing compressed ignorance.
    gap_repo: GapRepository,
    /// Whether the Claw is currently in questioning mode.
    questioning_active: bool,
    /// ID of the current gap being questioned (if any).
    active_gap_id: Option<u64>,
    /// Number of total halts triggered.
    halt_count: u64,
    /// Number of gaps successfully resolved.
    resolved_count: u64,
    /// Next gap ID to assign.
    next_gap_id: u64,
}

impl SocraticClaw {
    /// Create a new Socratic Claw with the given model/latent dimensions.
    pub fn new(model_dim: usize, latent_dim: usize) -> Self {
        Self {
            g_threshold: DEFAULT_G_THRESHOLD,
            gap_repo: GapRepository::new(model_dim, latent_dim),
            questioning_active: false,
            active_gap_id: None,
            halt_count: 0,
            resolved_count: 0,
            next_gap_id: 1,
        }
    }

    /// Override the G-threshold.
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.g_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Current G-threshold.
    pub fn threshold(&self) -> f32 {
        self.g_threshold
    }

    /// Is the Claw currently in questioning mode?
    pub fn is_questioning(&self) -> bool {
        self.questioning_active
    }

    /// ID of the active gap, if any.
    pub fn active_gap_id(&self) -> Option<u64> {
        self.active_gap_id
    }

    /// Total number of halts triggered.
    pub fn halt_count(&self) -> u64 {
        self.halt_count
    }

    /// Number of gaps successfully resolved.
    pub fn resolved_count(&self) -> u64 {
        self.resolved_count
    }

    /// Access the gap repository.
    pub fn gap_repo(&self) -> &GapRepository {
        &self.gap_repo
    }

    /// Evaluate the current G-metric and decide whether to proceed or halt.
    ///
    /// # Arguments
    /// * `g_metric` — Current G-metric from QKV-G attention (0.0 = perfect, 1.0 = total gap)
    /// * `context_vector` — The query/context vector that produced this G-metric
    /// * `agent_id` — The agent being evaluated
    pub fn intercept(
        &mut self,
        g_metric: f32,
        context_vector: &[f32],
        agent_id: &str,
    ) -> SocraticDecision {
        if g_metric < self.g_threshold {
            // Knowledge sufficient — proceed
            if self.questioning_active {
                // Was questioning, gap resolved
                if let Some(gap_id) = self.active_gap_id.take() {
                    self.gap_repo.resolve(gap_id);
                    self.resolved_count += 1;
                }
                self.questioning_active = false;
            }
            return SocraticDecision::Proceed { g_metric };
        }

        // G >= threshold — knowledge gap detected
        self.halt_count += 1;
        self.questioning_active = true;

        // Compress the gap via MLA Down-Projection
        let latent = self.gap_repo.down_project(context_vector);

        // Store in gap repository
        let gap_id = self.next_gap_id;
        self.next_gap_id += 1;
        self.active_gap_id = Some(gap_id);

        self.gap_repo.store(GapEntry {
            gap_id,
            latent_vector: latent.clone(),
            agent_id: agent_id.to_string(),
            severity: g_metric,
            resolved: false,
            timestamp: 0, // caller should set this
        });

        // Generate question hint via Up-Projection
        let question_hint = self.gap_repo.up_project(&latent);

        SocraticDecision::Halt {
            g_metric,
            gap_id,
            question_hint,
        }
    }

    /// Submit an answer to fill a gap. Returns true if the gap was found and updated.
    pub fn submit_answer(&mut self, gap_id: u64, answer: &[f32]) -> bool {
        if self.gap_repo.resolve(gap_id) {
            self.resolved_count += 1;
            if self.active_gap_id == Some(gap_id) {
                self.questioning_active = false;
                self.active_gap_id = None;
            }
            // The answer vector would be stored in QKV-G cache by the caller:
            // K ← context, V ← answer
            let _ = answer; // consumed by the caller for KV storage
            true
        } else {
            false
        }
    }

    /// Get a summary of gap cluster themes (meta-cognition).
    /// Returns (cluster_count, unresolved_count, avg_severity).
    pub fn meta_cognition(&self) -> (usize, usize, f32) {
        let gaps = self.gap_repo.entries();
        let unresolved: Vec<_> = gaps.iter().filter(|g| !g.resolved).collect();
        let avg_severity = if unresolved.is_empty() {
            0.0
        } else {
            unresolved.iter().map(|g| g.severity).sum::<f32>() / unresolved.len() as f32
        };
        (gaps.len(), unresolved.len(), avg_severity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_vector(dim: usize, value: f32) -> Vec<f32> {
        vec![value; dim]
    }

    #[test]
    fn claw_creation() {
        let claw = SocraticClaw::new(64, 8);
        assert_eq!(claw.threshold(), DEFAULT_G_THRESHOLD);
        assert!(!claw.is_questioning());
        assert!(claw.active_gap_id().is_none());
        assert_eq!(claw.halt_count(), 0);
        assert_eq!(claw.resolved_count(), 0);
    }

    #[test]
    fn claw_custom_threshold() {
        let claw = SocraticClaw::new(64, 8).with_threshold(0.6);
        assert_eq!(claw.threshold(), 0.6);
    }

    #[test]
    fn claw_threshold_clamped() {
        let claw = SocraticClaw::new(64, 8).with_threshold(1.5);
        assert_eq!(claw.threshold(), 1.0);
        let claw2 = SocraticClaw::new(64, 8).with_threshold(-0.5);
        assert_eq!(claw2.threshold(), 0.0);
    }

    #[test]
    fn claw_proceed_when_g_low() {
        let mut claw = SocraticClaw::new(16, 4);
        let ctx = make_vector(16, 0.5);
        let decision = claw.intercept(0.1, &ctx, "agent-1");
        match decision {
            SocraticDecision::Proceed { g_metric } => {
                assert!(g_metric < 0.4);
            }
            _ => panic!("expected Proceed"),
        }
        assert!(!claw.is_questioning());
    }

    #[test]
    fn claw_halt_when_g_high() {
        let mut claw = SocraticClaw::new(16, 4);
        let ctx = make_vector(16, 0.8);
        let decision = claw.intercept(0.65, &ctx, "agent-1");
        match decision {
            SocraticDecision::Halt { g_metric, gap_id, question_hint } => {
                assert!(g_metric >= 0.4);
                assert_eq!(gap_id, 1);
                assert_eq!(question_hint.len(), 16); // up-projected back to model_dim
            }
            _ => panic!("expected Halt"),
        }
        assert!(claw.is_questioning());
        assert_eq!(claw.halt_count(), 1);
        assert_eq!(claw.active_gap_id(), Some(1));
    }

    #[test]
    fn claw_gap_resolved_on_low_g() {
        let mut claw = SocraticClaw::new(16, 4);
        let ctx = make_vector(16, 0.8);

        // First: trigger a halt
        claw.intercept(0.65, &ctx, "agent-1");
        assert!(claw.is_questioning());

        // Second: G drops below threshold → gap resolved
        let decision = claw.intercept(0.2, &ctx, "agent-1");
        assert!(matches!(decision, SocraticDecision::Proceed { .. }));
        assert!(!claw.is_questioning());
        assert_eq!(claw.resolved_count(), 1);
    }

    #[test]
    fn claw_submit_answer() {
        let mut claw = SocraticClaw::new(16, 4);
        let ctx = make_vector(16, 0.5);

        claw.intercept(0.7, &ctx, "agent-1");
        assert!(claw.is_questioning());

        let answer = vec![1.0; 16];
        assert!(claw.submit_answer(1, &answer));
        assert!(!claw.is_questioning());
        assert_eq!(claw.resolved_count(), 1);
    }

    #[test]
    fn claw_submit_invalid_gap() {
        let mut claw = SocraticClaw::new(16, 4);
        assert!(!claw.submit_answer(999, &[1.0; 16]));
    }

    #[test]
    fn claw_multiple_halts() {
        let mut claw = SocraticClaw::new(16, 4);
        let ctx = make_vector(16, 0.5);

        // Halt 1
        claw.intercept(0.5, &ctx, "agent-1");
        assert_eq!(claw.halt_count(), 1);

        // Resolve via low G
        claw.intercept(0.1, &ctx, "agent-1");

        // Halt 2
        claw.intercept(0.6, &ctx, "agent-1");
        assert_eq!(claw.halt_count(), 2);
        assert_eq!(claw.active_gap_id(), Some(2));
    }

    #[test]
    fn claw_meta_cognition() {
        let mut claw = SocraticClaw::new(16, 4);
        let ctx = make_vector(16, 0.5);

        // Two halts, one resolved
        claw.intercept(0.5, &ctx, "agent-1");
        claw.intercept(0.1, &ctx, "agent-1"); // resolves gap 1
        claw.intercept(0.7, &ctx, "agent-2"); // gap 2 unresolved

        let (total, unresolved, avg) = claw.meta_cognition();
        assert_eq!(total, 2);
        assert_eq!(unresolved, 1);
        assert!(avg > 0.5);
    }
}
