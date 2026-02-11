//! Behavior Engine — pattern analysis and trust integration.
//!
//! Monitors agent behavior over time, builds behavior profiles,
//! and integrates with the QKV-G attention engine for anomaly detection.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::agent::AgentId;

/// A snapshot of an agent's behavior at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorSample {
    /// Tick when this sample was taken.
    pub tick: u64,
    /// Behavior vector (same dimension as QKV-G model).
    pub vector: Vec<f32>,
    /// G-metric at the time of sampling.
    pub g_metric: f32,
}

/// Aggregated behavior profile for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorProfile {
    /// Agent ID.
    pub agent_id: AgentId,
    /// Rolling average behavior vector.
    pub mean_vector: Vec<f32>,
    /// Number of samples collected.
    pub sample_count: u64,
    /// Average G-metric across all samples.
    pub avg_g_metric: f32,
    /// Number of anomalies detected.
    pub anomaly_count: u32,
    /// Current trust score (0.0..1.0).
    pub trust_score: f32,
    /// Recent behavior samples (bounded window).
    pub recent_samples: Vec<BehaviorSample>,
}

impl BehaviorProfile {
    pub fn new(agent_id: AgentId, vector_dim: usize) -> Self {
        Self {
            agent_id,
            mean_vector: vec![0.0; vector_dim],
            sample_count: 0,
            avg_g_metric: 0.0,
            anomaly_count: 0,
            trust_score: 0.5, // neutral starting trust
            recent_samples: Vec::new(),
        }
    }
}

/// Configuration for the Behavior Engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorEngineConfig {
    /// Dimension of behavior vectors.
    pub vector_dim: usize,
    /// Maximum recent samples to keep per agent.
    pub max_recent_samples: usize,
    /// Anomaly threshold: if behavior deviates more than this from mean, flag it.
    pub anomaly_deviation_threshold: f32,
    /// Trust decay factor per anomaly (trust -= factor * severity).
    pub trust_decay_factor: f32,
    /// Trust recovery per clean tick.
    pub trust_recovery_rate: f32,
}

impl Default for BehaviorEngineConfig {
    fn default() -> Self {
        Self {
            vector_dim: 64,
            max_recent_samples: 32,
            anomaly_deviation_threshold: 2.0,
            trust_decay_factor: 0.1,
            trust_recovery_rate: 0.005,
        }
    }
}

/// Central behavior monitoring engine.
#[derive(Debug)]
pub struct BehaviorEngine {
    config: BehaviorEngineConfig,
    profiles: HashMap<AgentId, BehaviorProfile>,
    total_samples: u64,
    total_anomalies: u64,
}

impl BehaviorEngine {
    pub fn new(config: BehaviorEngineConfig) -> Self {
        Self {
            config,
            profiles: HashMap::new(),
            total_samples: 0,
            total_anomalies: 0,
        }
    }

    /// Register an agent for behavior monitoring.
    pub fn register(&mut self, agent_id: &AgentId) {
        if !self.profiles.contains_key(agent_id) {
            self.profiles.insert(
                agent_id.clone(),
                BehaviorProfile::new(agent_id.clone(), self.config.vector_dim),
            );
        }
    }

    /// Record a behavior sample for an agent.
    /// Returns true if the behavior is anomalous.
    pub fn record(
        &mut self,
        agent_id: &AgentId,
        vector: &[f32],
        g_metric: f32,
        tick: u64,
    ) -> bool {
        let profile = self.profiles
            .entry(agent_id.clone())
            .or_insert_with(|| BehaviorProfile::new(agent_id.clone(), self.config.vector_dim));

        let sample = BehaviorSample {
            tick,
            vector: vector.to_vec(),
            g_metric,
        };

        // Check for anomaly before updating mean
        let is_anomaly = if profile.sample_count > 0 {
            let deviation = Self::cosine_distance(&profile.mean_vector, vector);
            deviation > self.config.anomaly_deviation_threshold
        } else {
            false
        };

        // Update running mean: new_mean = (old_mean * n + sample) / (n + 1)
        let n = profile.sample_count as f32;
        for (i, val) in vector.iter().enumerate() {
            if i < profile.mean_vector.len() {
                profile.mean_vector[i] = (profile.mean_vector[i] * n + val) / (n + 1.0);
            }
        }

        // Update average G-metric
        profile.avg_g_metric = (profile.avg_g_metric * n + g_metric) / (n + 1.0);
        profile.sample_count += 1;
        self.total_samples += 1;

        // Add to recent samples (bounded)
        profile.recent_samples.push(sample);
        if profile.recent_samples.len() > self.config.max_recent_samples {
            profile.recent_samples.remove(0);
        }

        // Update trust
        if is_anomaly {
            profile.anomaly_count += 1;
            self.total_anomalies += 1;
            let decay = self.config.trust_decay_factor * g_metric;
            profile.trust_score = (profile.trust_score - decay).max(0.0);
        } else {
            profile.trust_score =
                (profile.trust_score + self.config.trust_recovery_rate).min(1.0);
        }

        is_anomaly
    }

    /// Get a behavior profile.
    pub fn profile(&self, agent_id: &AgentId) -> Option<&BehaviorProfile> {
        self.profiles.get(agent_id)
    }

    /// Get the trust score for an agent.
    pub fn trust_score(&self, agent_id: &AgentId) -> Option<f32> {
        self.profiles.get(agent_id).map(|p| p.trust_score)
    }

    /// Number of monitored agents.
    pub fn agent_count(&self) -> usize {
        self.profiles.len()
    }

    /// Total behavior samples recorded.
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// Total anomalies detected.
    pub fn total_anomalies(&self) -> u64 {
        self.total_anomalies
    }

    /// Compute cosine distance between two vectors.
    /// Returns 0.0 for identical directions, 2.0 for opposite.
    fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a < 1e-8 || norm_b < 1e-8 {
            return 2.0; // treat zero vectors as maximally different
        }

        let cosine_sim = dot / (norm_a * norm_b);
        1.0 - cosine_sim // distance: 0 = same, 2 = opposite
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_engine() -> BehaviorEngine {
        BehaviorEngine::new(BehaviorEngineConfig {
            vector_dim: 4,
            max_recent_samples: 5,
            anomaly_deviation_threshold: 1.5,
            trust_decay_factor: 0.1,
            trust_recovery_rate: 0.01,
        })
    }

    #[test]
    fn engine_creation() {
        let engine = make_engine();
        assert_eq!(engine.agent_count(), 0);
        assert_eq!(engine.total_samples(), 0);
    }

    #[test]
    fn register_agent() {
        let mut engine = make_engine();
        let id = AgentId::new("agent-1");
        engine.register(&id);
        assert_eq!(engine.agent_count(), 1);
        let profile = engine.profile(&id).unwrap();
        assert_eq!(profile.trust_score, 0.5);
        assert_eq!(profile.sample_count, 0);
    }

    #[test]
    fn record_first_sample() {
        let mut engine = make_engine();
        let id = AgentId::new("a1");
        let vector = vec![1.0, 0.0, 0.0, 0.0];
        let is_anomaly = engine.record(&id, &vector, 0.1, 1);
        assert!(!is_anomaly); // first sample is never anomalous
        assert_eq!(engine.total_samples(), 1);
    }

    #[test]
    fn record_consistent_behavior() {
        let mut engine = make_engine();
        let id = AgentId::new("consistent");
        let vector = vec![1.0, 0.5, 0.3, 0.2];

        for tick in 0..10 {
            let is_anomaly = engine.record(&id, &vector, 0.1, tick);
            assert!(!is_anomaly);
        }

        let profile = engine.profile(&id).unwrap();
        assert_eq!(profile.sample_count, 10);
        assert_eq!(profile.anomaly_count, 0);
        // Trust should have increased from 0.5
        assert!(profile.trust_score > 0.5);
    }

    #[test]
    fn detect_anomalous_behavior() {
        let mut engine = make_engine();
        let id = AgentId::new("anomalous");

        // Establish normal behavior
        let normal = vec![1.0, 0.0, 0.0, 0.0];
        for tick in 0..5 {
            engine.record(&id, &normal, 0.1, tick);
        }

        // Wildly different behavior (opposite direction)
        let abnormal = vec![-1.0, 0.0, 0.0, 0.0];
        let is_anomaly = engine.record(&id, &abnormal, 0.8, 5);
        assert!(is_anomaly);

        let profile = engine.profile(&id).unwrap();
        assert_eq!(profile.anomaly_count, 1);
        // Trust should have decreased
        assert!(profile.trust_score < 0.5);
    }

    #[test]
    fn trust_score_bounds() {
        let mut engine = make_engine();
        let id = AgentId::new("bounded");
        engine.register(&id);

        // Record many good samples → trust shouldn't exceed 1.0
        let vector = vec![1.0, 0.5, 0.3, 0.2];
        for tick in 0..200 {
            engine.record(&id, &vector, 0.1, tick);
        }
        assert!(engine.trust_score(&id).unwrap() <= 1.0);
    }

    #[test]
    fn recent_samples_bounded() {
        let mut engine = make_engine();
        let id = AgentId::new("bounded-history");

        for tick in 0..20 {
            engine.record(&id, &vec![tick as f32; 4], 0.1, tick);
        }

        let profile = engine.profile(&id).unwrap();
        assert_eq!(profile.recent_samples.len(), 5); // max_recent_samples = 5
        assert_eq!(profile.recent_samples.last().unwrap().tick, 19);
    }

    #[test]
    fn cosine_distance_same() {
        let a = vec![1.0, 2.0, 3.0];
        let dist = BehaviorEngine::cosine_distance(&a, &a);
        assert!(dist.abs() < 1e-5);
    }

    #[test]
    fn cosine_distance_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        let dist = BehaviorEngine::cosine_distance(&a, &b);
        assert!((dist - 2.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_distance_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        let dist = BehaviorEngine::cosine_distance(&a, &b);
        assert_eq!(dist, 2.0); // treated as maximally different
    }

    #[test]
    fn multiple_agents() {
        let mut engine = make_engine();
        let a1 = AgentId::new("a1");
        let a2 = AgentId::new("a2");

        engine.record(&a1, &[1.0, 0.0, 0.0, 0.0], 0.1, 0);
        engine.record(&a2, &[0.0, 1.0, 0.0, 0.0], 0.2, 0);

        assert_eq!(engine.agent_count(), 2);
        assert_eq!(engine.total_samples(), 2);
    }
}
