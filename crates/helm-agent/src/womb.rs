//! Agent Womb — QKV-G Socratic autonomous agent spawning.
//!
//! The Womb is the maternal core of the Helm Protocol, birthing sovereign
//! autonomous agents through Socratic questioning and QKV-G evaluation.
//!
//! Birth process:
//! 1. Seed: Initial purpose/intent vector
//! 2. Socratic Gestation: QKV-G evaluates knowledge gaps
//! 3. Capability Focus: Select from 11 capability types
//! 4. DNA Imprint: Personality vector + behavior baseline
//! 5. Birth: Fully autonomous agent with its own identity

use serde::{Deserialize, Serialize};

use crate::agent::{AgentId, AgentType, AgentConfig};
use crate::capability::Capability;
use crate::socratic::claw::SocraticClaw;

/// Agent DNA — the personality and behavioral seed of a born agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDna {
    /// Personality vector (normalized, dimension = model_dim).
    pub personality: Vec<f32>,
    /// Primary capability focus.
    pub primary_capability: Capability,
    /// Secondary capabilities.
    pub secondary_capabilities: Vec<Capability>,
    /// Socratic threshold — curiosity level (lower = more questioning).
    pub g_threshold: f32,
    /// Autonomy level (0.0 = fully guided, 1.0 = fully autonomous).
    pub autonomy: f32,
    /// Creative spark — variance in behavior (0.0 = deterministic, 1.0 = creative).
    pub creativity: f32,
}

/// Birth certificate — proof of agent creation through the Womb.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirthCertificate {
    pub agent_id: AgentId,
    pub agent_config: AgentConfig,
    pub dna: AgentDna,
    pub birth_epoch: u64,
    /// G-metric at birth (knowledge readiness).
    pub birth_g_metric: f32,
    /// The Womb that created this agent.
    pub womb_id: String,
}

/// Womb configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WombConfig {
    /// Model dimension for QKV-G processing.
    pub model_dim: usize,
    /// Latent dimension for gap compression.
    pub latent_dim: usize,
    /// Maximum agents that can be gestating simultaneously.
    pub max_gestating: usize,
    /// Minimum G-metric threshold for birth readiness.
    pub birth_readiness_threshold: f32,
    /// Default autonomy level for spawned agents.
    pub default_autonomy: f32,
}

impl Default for WombConfig {
    fn default() -> Self {
        Self {
            model_dim: 64,
            latent_dim: 8,
            max_gestating: 16,
            birth_readiness_threshold: 0.3,
            default_autonomy: 0.7,
        }
    }
}

/// Gestation state — an agent being formed in the Womb.
#[derive(Debug)]
struct Gestation {
    name: String,
    intent_vector: Vec<f32>,
    capability_focus: Capability,
    secondary: Vec<Capability>,
    questions_asked: u64,
    answers_received: u64,
    current_g: f32,
    autonomy: f32,
    creativity: f32,
}

/// The Agent Womb — maternal core that births autonomous agents.
///
/// Uses QKV-G Socratic evaluation to ensure each agent is born with
/// sufficient knowledge and a clear purpose. The Womb asks questions
/// until the agent's G-metric drops below the birth readiness threshold.
pub struct AgentWomb {
    config: WombConfig,
    /// Socratic Claw for evaluating gestating agents.
    claw: SocraticClaw,
    /// Agents currently in gestation.
    gestating: Vec<Gestation>,
    /// Total agents birthed.
    total_births: u64,
    /// Current epoch.
    current_epoch: u64,
    /// Unique womb identifier.
    womb_id: String,
}

impl AgentWomb {
    pub fn new(config: WombConfig) -> Self {
        let claw = SocraticClaw::new(config.model_dim, config.latent_dim)
            .with_threshold(config.birth_readiness_threshold);
        Self {
            claw,
            gestating: Vec::new(),
            total_births: 0,
            current_epoch: 0,
            womb_id: format!("womb-{:08x}", rand::random::<u32>()),
            config,
        }
    }

    pub fn womb_id(&self) -> &str {
        &self.womb_id
    }

    pub fn total_births(&self) -> u64 {
        self.total_births
    }

    pub fn gestating_count(&self) -> usize {
        self.gestating.len()
    }

    pub fn advance_epoch(&mut self) {
        self.current_epoch += 1;
    }

    /// Begin gestation — start creating a new agent with a purpose.
    pub fn begin_gestation(
        &mut self,
        name: &str,
        intent_vector: Vec<f32>,
        capability_focus: Capability,
    ) -> Result<usize, String> {
        if self.gestating.len() >= self.config.max_gestating {
            return Err(format!(
                "womb at capacity: {}/{}",
                self.gestating.len(),
                self.config.max_gestating
            ));
        }

        // Validate intent vector dimension
        let expected_dim = self.config.model_dim;
        if intent_vector.len() != expected_dim {
            return Err(format!(
                "intent vector dimension mismatch: got {}, expected {}",
                intent_vector.len(),
                expected_dim
            ));
        }

        let index = self.gestating.len();
        self.gestating.push(Gestation {
            name: name.to_string(),
            intent_vector,
            capability_focus,
            secondary: Vec::new(),
            questions_asked: 0,
            answers_received: 0,
            current_g: 1.0, // starts with maximum gap
            autonomy: self.config.default_autonomy,
            creativity: 0.5,
        });

        Ok(index)
    }

    /// Add secondary capabilities to a gestating agent.
    pub fn add_secondary_capability(
        &mut self,
        gestation_index: usize,
        capability: Capability,
    ) -> Result<(), String> {
        let g = self
            .gestating
            .get_mut(gestation_index)
            .ok_or_else(|| format!("no gestation at index {}", gestation_index))?;

        if !g.secondary.contains(&capability) && capability != g.capability_focus {
            g.secondary.push(capability);
        }
        Ok(())
    }

    /// Set autonomy level for a gestating agent.
    pub fn set_autonomy(
        &mut self,
        gestation_index: usize,
        autonomy: f32,
    ) -> Result<(), String> {
        let g = self
            .gestating
            .get_mut(gestation_index)
            .ok_or_else(|| format!("no gestation at index {}", gestation_index))?;
        g.autonomy = autonomy.clamp(0.0, 1.0);
        Ok(())
    }

    /// Set creativity level for a gestating agent.
    pub fn set_creativity(
        &mut self,
        gestation_index: usize,
        creativity: f32,
    ) -> Result<(), String> {
        let g = self
            .gestating
            .get_mut(gestation_index)
            .ok_or_else(|| format!("no gestation at index {}", gestation_index))?;
        g.creativity = creativity.clamp(0.0, 1.0);
        Ok(())
    }

    /// Feed a Socratic answer into a gestating agent, reducing its G-metric.
    /// Returns the new G-metric and whether the agent is ready for birth.
    pub fn feed_answer(
        &mut self,
        gestation_index: usize,
        answer_vector: &[f32],
    ) -> Result<(f32, bool), String> {
        let g = self
            .gestating
            .get_mut(gestation_index)
            .ok_or_else(|| format!("no gestation at index {}", gestation_index))?;

        g.answers_received += 1;

        // Simulate knowledge absorption: each answer reduces G-metric
        // The reduction depends on the quality (magnitude) of the answer
        let answer_magnitude: f32 = answer_vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        let intent_magnitude: f32 = g.intent_vector.iter().map(|x| x * x).sum::<f32>().sqrt();

        let relevance = if intent_magnitude > 0.0 && answer_magnitude > 0.0 {
            // Cosine similarity as relevance measure
            let dot: f32 = g
                .intent_vector
                .iter()
                .zip(answer_vector.iter())
                .map(|(a, b)| a * b)
                .sum();
            (dot / (intent_magnitude * answer_magnitude)).abs()
        } else {
            0.5
        };

        // G decreases proportionally to relevance
        let reduction = relevance * 0.15;
        g.current_g = (g.current_g - reduction).max(0.0);

        let ready = g.current_g < self.config.birth_readiness_threshold;
        Ok((g.current_g, ready))
    }

    /// Ask a Socratic question about a gestating agent's knowledge gaps.
    /// Returns a question hint vector derived from the QKV-G evaluation.
    pub fn ask_question(
        &mut self,
        gestation_index: usize,
    ) -> Result<Vec<f32>, String> {
        let g = self
            .gestating
            .get(gestation_index)
            .ok_or_else(|| format!("no gestation at index {}", gestation_index))?;

        let decision = self.claw.intercept(
            g.current_g,
            &g.intent_vector,
            &g.name,
        );

        // Increment question counter on the gestation
        let g = &mut self.gestating[gestation_index];
        g.questions_asked += 1;

        match decision {
            crate::socratic::claw::SocraticDecision::Halt {
                question_hint, ..
            } => Ok(question_hint),
            crate::socratic::claw::SocraticDecision::Proceed { .. } => {
                // Already ready — return the intent vector as the "question"
                Ok(g.intent_vector.clone())
            }
        }
    }

    /// Check if a gestating agent is ready for birth.
    pub fn is_ready(&self, gestation_index: usize) -> bool {
        self.gestating
            .get(gestation_index)
            .map(|g| g.current_g < self.config.birth_readiness_threshold)
            .unwrap_or(false)
    }

    /// Birth an agent — finalize gestation and produce a BirthCertificate.
    /// The agent must have its G-metric below the birth readiness threshold.
    pub fn birth(&mut self, gestation_index: usize) -> Result<BirthCertificate, String> {
        if gestation_index >= self.gestating.len() {
            return Err(format!("no gestation at index {}", gestation_index));
        }

        let g = &self.gestating[gestation_index];
        if g.current_g >= self.config.birth_readiness_threshold {
            return Err(format!(
                "agent not ready: G={:.3}, threshold={:.3}",
                g.current_g, self.config.birth_readiness_threshold
            ));
        }

        let g = self.gestating.remove(gestation_index);
        self.total_births += 1;

        let agent_id = AgentId::new(format!(
            "{}-{:04}",
            g.name,
            self.total_births
        ));

        let primary = g.capability_focus.clone();
        let mut capabilities = vec![primary.clone()];
        capabilities.extend(g.secondary.iter().cloned());

        let agent_config = AgentConfig {
            name: g.name.clone(),
            agent_type: AgentType::Autonomous,
            capabilities: capabilities.clone(),
            max_ticks: 0,
            g_threshold: Some(g.current_g.max(0.1)),
        };

        // Normalize personality vector
        let mag: f32 = g.intent_vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        let personality = if mag > 0.0 {
            g.intent_vector.iter().map(|x| x / mag).collect()
        } else {
            g.intent_vector.clone()
        };

        let dna = AgentDna {
            personality,
            primary_capability: primary,
            secondary_capabilities: g.secondary,
            g_threshold: g.current_g.max(0.1),
            autonomy: g.autonomy,
            creativity: g.creativity,
        };

        Ok(BirthCertificate {
            agent_id,
            agent_config,
            dna,
            birth_epoch: self.current_epoch,
            birth_g_metric: g.current_g,
            womb_id: self.womb_id.clone(),
        })
    }

    /// Quick birth — bypass gestation for pre-configured agents.
    /// Used for system agents and testing.
    pub fn quick_birth(
        &mut self,
        name: &str,
        capability: Capability,
        intent: Vec<f32>,
    ) -> Result<BirthCertificate, String> {
        let idx = self.begin_gestation(name, intent, capability)?;

        // Force G below threshold for immediate birth
        let g = &mut self.gestating[idx];
        g.current_g = 0.0;

        self.birth(idx)
    }

    /// Available capability types for agent focus selection.
    pub fn available_capabilities() -> Vec<Capability> {
        vec![
            Capability::Compute,
            Capability::Storage,
            Capability::Network,
            Capability::Governance,
            Capability::Security,
            Capability::Codec,
            Capability::Socratic,
            Capability::Spawning,
            Capability::Token,
            Capability::EdgeApi,
            Capability::Custom("user-defined".to_string()),
        ]
    }
}

impl std::fmt::Debug for AgentWomb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentWomb")
            .field("womb_id", &self.womb_id)
            .field("total_births", &self.total_births)
            .field("gestating", &self.gestating.len())
            .field("epoch", &self.current_epoch)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(dim: usize, val: f32) -> Vec<f32> {
        vec![val; dim]
    }

    #[test]
    fn womb_creation() {
        let womb = AgentWomb::new(WombConfig::default());
        assert_eq!(womb.total_births(), 0);
        assert_eq!(womb.gestating_count(), 0);
        assert!(womb.womb_id().starts_with("womb-"));
    }

    #[test]
    fn begin_gestation() {
        let mut womb = AgentWomb::new(WombConfig::default());
        let idx = womb
            .begin_gestation("explorer", intent(64, 0.5), Capability::Network)
            .unwrap();
        assert_eq!(idx, 0);
        assert_eq!(womb.gestating_count(), 1);
    }

    #[test]
    fn gestation_wrong_dimension() {
        let mut womb = AgentWomb::new(WombConfig::default());
        assert!(womb
            .begin_gestation("bad", vec![1.0; 10], Capability::Compute)
            .is_err());
    }

    #[test]
    fn gestation_capacity_limit() {
        let config = WombConfig {
            max_gestating: 2,
            ..Default::default()
        };
        let mut womb = AgentWomb::new(config);
        womb.begin_gestation("a1", intent(64, 0.5), Capability::Compute)
            .unwrap();
        womb.begin_gestation("a2", intent(64, 0.5), Capability::Storage)
            .unwrap();
        assert!(womb
            .begin_gestation("a3", intent(64, 0.5), Capability::Network)
            .is_err());
    }

    #[test]
    fn feed_answer_reduces_g() {
        let mut womb = AgentWomb::new(WombConfig::default());
        womb.begin_gestation("learner", intent(64, 0.5), Capability::Socratic)
            .unwrap();

        // G starts at 1.0
        assert!(!womb.is_ready(0));

        // Feed relevant answers to reduce G
        for _ in 0..10 {
            let (g, _ready) = womb.feed_answer(0, &intent(64, 0.5)).unwrap();
            if g < 0.3 {
                break;
            }
        }

        // After enough answers, should be ready
        assert!(womb.is_ready(0));
    }

    #[test]
    fn ask_question_returns_hint() {
        let mut womb = AgentWomb::new(WombConfig::default());
        womb.begin_gestation("curious", intent(64, 0.5), Capability::Governance)
            .unwrap();

        let hint = womb.ask_question(0).unwrap();
        assert_eq!(hint.len(), 64);
    }

    #[test]
    fn birth_after_gestation() {
        let mut womb = AgentWomb::new(WombConfig::default());
        womb.begin_gestation("newborn", intent(64, 0.5), Capability::Security)
            .unwrap();

        // Feed answers until ready
        for _ in 0..20 {
            let (_, ready) = womb.feed_answer(0, &intent(64, 0.5)).unwrap();
            if ready {
                break;
            }
        }

        let cert = womb.birth(0).unwrap();
        assert!(cert.agent_id.as_str().starts_with("newborn-"));
        assert_eq!(cert.agent_config.agent_type, AgentType::Autonomous);
        assert_eq!(cert.dna.primary_capability, Capability::Security);
        assert!(cert.birth_g_metric < 0.3);
        assert_eq!(womb.total_births(), 1);
        assert_eq!(womb.gestating_count(), 0);
    }

    #[test]
    fn birth_not_ready_fails() {
        let mut womb = AgentWomb::new(WombConfig::default());
        womb.begin_gestation("premature", intent(64, 0.5), Capability::Compute)
            .unwrap();

        // Try to birth without answering questions
        assert!(womb.birth(0).is_err());
    }

    #[test]
    fn quick_birth() {
        let mut womb = AgentWomb::new(WombConfig::default());
        let cert = womb
            .quick_birth("system-agent", Capability::Token, intent(64, 1.0))
            .unwrap();

        assert_eq!(cert.agent_config.agent_type, AgentType::Autonomous);
        assert_eq!(cert.dna.primary_capability, Capability::Token);
        assert_eq!(womb.total_births(), 1);
    }

    #[test]
    fn secondary_capabilities() {
        let mut womb = AgentWomb::new(WombConfig::default());
        womb.begin_gestation("multi", intent(64, 0.5), Capability::Compute)
            .unwrap();
        womb.add_secondary_capability(0, Capability::Storage).unwrap();
        womb.add_secondary_capability(0, Capability::Network).unwrap();
        // Duplicate ignored
        womb.add_secondary_capability(0, Capability::Storage).unwrap();
        // Primary ignored
        womb.add_secondary_capability(0, Capability::Compute).unwrap();

        // Quick birth to check
        let g = &mut womb.gestating[0];
        g.current_g = 0.0;
        let cert = womb.birth(0).unwrap();
        assert_eq!(cert.dna.secondary_capabilities.len(), 2);
    }

    #[test]
    fn set_autonomy_and_creativity() {
        let mut womb = AgentWomb::new(WombConfig::default());
        womb.begin_gestation("tuned", intent(64, 0.5), Capability::EdgeApi)
            .unwrap();
        womb.set_autonomy(0, 0.9).unwrap();
        womb.set_creativity(0, 0.8).unwrap();

        let g = &mut womb.gestating[0];
        g.current_g = 0.0;
        let cert = womb.birth(0).unwrap();
        assert!((cert.dna.autonomy - 0.9).abs() < 0.01);
        assert!((cert.dna.creativity - 0.8).abs() < 0.01);
    }

    #[test]
    fn available_capabilities_list() {
        let caps = AgentWomb::available_capabilities();
        assert_eq!(caps.len(), 11);
    }

    #[test]
    fn multiple_births() {
        let mut womb = AgentWomb::new(WombConfig::default());

        for i in 0..3 {
            let cert = womb
                .quick_birth(
                    &format!("agent-{}", i),
                    Capability::Compute,
                    intent(64, 0.5),
                )
                .unwrap();
            assert!(cert.agent_id.as_str().contains(&format!("agent-{}", i)));
        }

        assert_eq!(womb.total_births(), 3);
    }

    #[test]
    fn birth_certificate_dna_normalized() {
        let mut womb = AgentWomb::new(WombConfig::default());
        let cert = womb
            .quick_birth("normed", Capability::Codec, intent(64, 3.0))
            .unwrap();

        // Personality should be normalized
        let mag: f32 = cert.dna.personality.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((mag - 1.0).abs() < 0.01);
    }

    #[test]
    fn womb_debug_format() {
        let womb = AgentWomb::new(WombConfig::default());
        let debug = format!("{:?}", womb);
        assert!(debug.contains("AgentWomb"));
        assert!(debug.contains("womb-"));
    }
}
