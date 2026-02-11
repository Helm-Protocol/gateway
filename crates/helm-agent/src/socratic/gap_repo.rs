//! MLA Latent Gap Repository — compressed ignorance storage.
//!
//! Inspired by DeepSeek-V2/V3's Multi-Head Latent Attention (MLA),
//! this module compresses knowledge gap vectors from the full model
//! dimension (d_model) to a latent dimension (d_latent) for efficient
//! storage, and reconstructs them for question generation.
//!
//! # Architecture
//!
//! ```text
//! d_model (e.g. 64) → Down-Projection → d_latent (e.g. 8)  [8x compression]
//! d_latent (e.g. 8)  → Up-Projection   → d_model (e.g. 64) [reconstruction]
//! ```
//!
//! The Down-Projection matrix (d_latent × d_model) compresses the gap
//! vector into a latent representation. The Up-Projection matrix
//! (d_model × d_latent) reconstructs hints for question generation.
//!
//! Gap entries cluster naturally in latent space, enabling meta-cognition:
//! "I mainly lack knowledge about X" through simple distance analysis.

use serde::{Deserialize, Serialize};
use rand::Rng;

/// A stored knowledge gap in compressed latent form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapEntry {
    /// Unique gap identifier.
    pub gap_id: u64,
    /// Compressed latent vector (d_latent dimensions).
    pub latent_vector: Vec<f32>,
    /// Agent that triggered this gap.
    pub agent_id: String,
    /// G-metric severity at detection time (0.4..1.0).
    pub severity: f32,
    /// Whether this gap has been resolved.
    pub resolved: bool,
    /// Tick/timestamp when detected.
    pub timestamp: u64,
}

/// MLA-based Gap Repository with Down/Up projection matrices.
#[derive(Debug, Serialize, Deserialize)]
pub struct GapRepository {
    /// Full model dimension.
    model_dim: usize,
    /// Compressed latent dimension.
    latent_dim: usize,
    /// Down-Projection matrix: d_latent × d_model.
    /// Compresses gap vectors for storage.
    down_proj: Vec<Vec<f32>>,
    /// Up-Projection matrix: d_model × d_latent.
    /// Reconstructs hints for question generation.
    up_proj: Vec<Vec<f32>>,
    /// Stored gap entries.
    gaps: Vec<GapEntry>,
}

impl GapRepository {
    /// Create a new Gap Repository with random Xavier-initialized projections.
    pub fn new(model_dim: usize, latent_dim: usize) -> Self {
        assert!(latent_dim <= model_dim, "latent_dim must be <= model_dim");
        assert!(latent_dim > 0 && model_dim > 0);

        let mut rng = rand::thread_rng();
        let scale = (2.0 / (model_dim + latent_dim) as f32).sqrt();

        // Down-Projection: d_latent rows × d_model cols
        let down_proj: Vec<Vec<f32>> = (0..latent_dim)
            .map(|_| {
                (0..model_dim)
                    .map(|_| rng.gen_range(-scale..scale))
                    .collect()
            })
            .collect();

        // Up-Projection: d_model rows × d_latent cols
        let up_proj: Vec<Vec<f32>> = (0..model_dim)
            .map(|_| {
                (0..latent_dim)
                    .map(|_| rng.gen_range(-scale..scale))
                    .collect()
            })
            .collect();

        Self {
            model_dim,
            latent_dim,
            down_proj,
            up_proj,
            gaps: Vec::new(),
        }
    }

    /// Model dimension.
    pub fn model_dim(&self) -> usize {
        self.model_dim
    }

    /// Latent dimension.
    pub fn latent_dim(&self) -> usize {
        self.latent_dim
    }

    /// Compression ratio (model_dim / latent_dim).
    pub fn compression_ratio(&self) -> f32 {
        self.model_dim as f32 / self.latent_dim as f32
    }

    /// Down-project a full-dimension vector to latent space.
    /// output[i] = sum_j(down_proj[i][j] * input[j])
    pub fn down_project(&self, input: &[f32]) -> Vec<f32> {
        assert_eq!(input.len(), self.model_dim);

        self.down_proj
            .iter()
            .map(|row| {
                row.iter()
                    .zip(input.iter())
                    .map(|(w, x)| w * x)
                    .sum()
            })
            .collect()
    }

    /// Up-project a latent vector back to full model dimension.
    /// output[i] = sum_j(up_proj[i][j] * latent[j])
    pub fn up_project(&self, latent: &[f32]) -> Vec<f32> {
        assert_eq!(latent.len(), self.latent_dim);

        self.up_proj
            .iter()
            .map(|row| {
                row.iter()
                    .zip(latent.iter())
                    .map(|(w, x)| w * x)
                    .sum()
            })
            .collect()
    }

    /// Store a gap entry.
    pub fn store(&mut self, entry: GapEntry) {
        self.gaps.push(entry);
    }

    /// Mark a gap as resolved. Returns true if found and updated.
    pub fn resolve(&mut self, gap_id: u64) -> bool {
        if let Some(entry) = self.gaps.iter_mut().find(|g| g.gap_id == gap_id) {
            entry.resolved = true;
            true
        } else {
            false
        }
    }

    /// Get a gap entry by ID.
    pub fn get(&self, gap_id: u64) -> Option<&GapEntry> {
        self.gaps.iter().find(|g| g.gap_id == gap_id)
    }

    /// All gap entries.
    pub fn entries(&self) -> &[GapEntry] {
        &self.gaps
    }

    /// Number of stored gaps.
    pub fn len(&self) -> usize {
        self.gaps.len()
    }

    /// Is the repository empty?
    pub fn is_empty(&self) -> bool {
        self.gaps.is_empty()
    }

    /// Number of unresolved gaps.
    pub fn unresolved_count(&self) -> usize {
        self.gaps.iter().filter(|g| !g.resolved).count()
    }

    /// Compute the L2 distance between two latent vectors.
    pub fn latent_distance(a: &[f32], b: &[f32]) -> f32 {
        assert_eq!(a.len(), b.len());
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Find the nearest unresolved gap to a given latent vector.
    pub fn nearest_unresolved(&self, latent: &[f32]) -> Option<&GapEntry> {
        self.gaps
            .iter()
            .filter(|g| !g.resolved)
            .min_by(|a, b| {
                let da = Self::latent_distance(&a.latent_vector, latent);
                let db = Self::latent_distance(&b.latent_vector, latent);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Clear all resolved gaps (garbage collection).
    pub fn gc_resolved(&mut self) -> usize {
        let before = self.gaps.len();
        self.gaps.retain(|g| !g.resolved);
        before - self.gaps.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_repo_creation() {
        let repo = GapRepository::new(64, 8);
        assert_eq!(repo.model_dim(), 64);
        assert_eq!(repo.latent_dim(), 8);
        assert_eq!(repo.compression_ratio(), 8.0);
        assert!(repo.is_empty());
    }

    #[test]
    fn down_projection_dimensions() {
        let repo = GapRepository::new(32, 4);
        let input = vec![1.0; 32];
        let latent = repo.down_project(&input);
        assert_eq!(latent.len(), 4); // compressed to d_latent
    }

    #[test]
    fn up_projection_dimensions() {
        let repo = GapRepository::new(32, 4);
        let latent = vec![1.0; 4];
        let output = repo.up_project(&latent);
        assert_eq!(output.len(), 32); // reconstructed to d_model
    }

    #[test]
    fn roundtrip_preserves_dimensions() {
        let repo = GapRepository::new(16, 4);
        let input = vec![0.5; 16];
        let latent = repo.down_project(&input);
        assert_eq!(latent.len(), 4);
        let reconstructed = repo.up_project(&latent);
        assert_eq!(reconstructed.len(), 16);
        // Note: reconstruction is lossy due to compression
    }

    #[test]
    fn store_and_retrieve_gap() {
        let mut repo = GapRepository::new(16, 4);
        repo.store(GapEntry {
            gap_id: 1,
            latent_vector: vec![0.1, 0.2, 0.3, 0.4],
            agent_id: "agent-1".to_string(),
            severity: 0.65,
            resolved: false,
            timestamp: 100,
        });

        assert_eq!(repo.len(), 1);
        let entry = repo.get(1).unwrap();
        assert_eq!(entry.agent_id, "agent-1");
        assert!(!entry.resolved);
        assert_eq!(entry.severity, 0.65);
    }

    #[test]
    fn resolve_gap() {
        let mut repo = GapRepository::new(16, 4);
        repo.store(GapEntry {
            gap_id: 10,
            latent_vector: vec![0.0; 4],
            agent_id: "a".to_string(),
            severity: 0.5,
            resolved: false,
            timestamp: 0,
        });

        assert_eq!(repo.unresolved_count(), 1);
        assert!(repo.resolve(10));
        assert_eq!(repo.unresolved_count(), 0);
        assert!(repo.get(10).unwrap().resolved);
    }

    #[test]
    fn resolve_nonexistent_returns_false() {
        let mut repo = GapRepository::new(16, 4);
        assert!(!repo.resolve(999));
    }

    #[test]
    fn latent_distance_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let dist = GapRepository::latent_distance(&a, &a);
        assert!(dist.abs() < 1e-6);
    }

    #[test]
    fn latent_distance_different() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![3.0, 4.0, 0.0];
        let dist = GapRepository::latent_distance(&a, &b);
        assert!((dist - 5.0).abs() < 1e-6); // 3-4-5 triangle
    }

    #[test]
    fn nearest_unresolved() {
        let mut repo = GapRepository::new(16, 4);
        repo.store(GapEntry {
            gap_id: 1,
            latent_vector: vec![1.0, 0.0, 0.0, 0.0],
            agent_id: "a".to_string(),
            severity: 0.5,
            resolved: false,
            timestamp: 0,
        });
        repo.store(GapEntry {
            gap_id: 2,
            latent_vector: vec![0.0, 0.0, 0.0, 1.0],
            agent_id: "b".to_string(),
            severity: 0.7,
            resolved: false,
            timestamp: 0,
        });

        let query = vec![0.9, 0.0, 0.0, 0.0]; // closer to gap 1
        let nearest = repo.nearest_unresolved(&query).unwrap();
        assert_eq!(nearest.gap_id, 1);
    }

    #[test]
    fn nearest_skips_resolved() {
        let mut repo = GapRepository::new(16, 4);
        repo.store(GapEntry {
            gap_id: 1,
            latent_vector: vec![1.0, 0.0, 0.0, 0.0],
            agent_id: "a".to_string(),
            severity: 0.5,
            resolved: true, // resolved!
            timestamp: 0,
        });
        repo.store(GapEntry {
            gap_id: 2,
            latent_vector: vec![0.0, 0.0, 0.0, 1.0],
            agent_id: "b".to_string(),
            severity: 0.7,
            resolved: false,
            timestamp: 0,
        });

        let query = vec![1.0, 0.0, 0.0, 0.0]; // closer to gap 1 but it's resolved
        let nearest = repo.nearest_unresolved(&query).unwrap();
        assert_eq!(nearest.gap_id, 2); // falls back to the unresolved one
    }

    #[test]
    fn gc_resolved() {
        let mut repo = GapRepository::new(16, 4);
        repo.store(GapEntry {
            gap_id: 1, latent_vector: vec![0.0; 4],
            agent_id: "a".to_string(), severity: 0.5,
            resolved: true, timestamp: 0,
        });
        repo.store(GapEntry {
            gap_id: 2, latent_vector: vec![0.0; 4],
            agent_id: "b".to_string(), severity: 0.6,
            resolved: false, timestamp: 0,
        });
        repo.store(GapEntry {
            gap_id: 3, latent_vector: vec![0.0; 4],
            agent_id: "c".to_string(), severity: 0.7,
            resolved: true, timestamp: 0,
        });

        let removed = repo.gc_resolved();
        assert_eq!(removed, 2);
        assert_eq!(repo.len(), 1);
        assert_eq!(repo.get(2).unwrap().gap_id, 2);
    }

    #[test]
    fn gap_entry_serialization() {
        let entry = GapEntry {
            gap_id: 42,
            latent_vector: vec![0.1, 0.2, 0.3, 0.4],
            agent_id: "agent-x".to_string(),
            severity: 0.55,
            resolved: false,
            timestamp: 12345,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let decoded: GapEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.gap_id, 42);
        assert_eq!(decoded.latent_vector.len(), 4);
    }

    #[test]
    #[should_panic]
    fn down_project_wrong_dim_panics() {
        let repo = GapRepository::new(16, 4);
        repo.down_project(&[1.0; 8]); // wrong size
    }

    #[test]
    #[should_panic]
    fn up_project_wrong_dim_panics() {
        let repo = GapRepository::new(16, 4);
        repo.up_project(&[1.0; 8]); // wrong size
    }
}
