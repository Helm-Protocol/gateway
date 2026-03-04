// gateway/src/krishna_l2.rs
// ═══════════════════════════════════════════════════════════════
// AGENT SOVEREIGN PROTOCOL — L2 GUARDIAN CORE
// ═══════════════════════════════════════════════════════════════
// This module implements the 8D E8 Lattice quantization to map
// high-dimensional agent context into a discrete, high-density 
// knowledge lattice for autonomous Gap (QKV-G) calculation.

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatticeNode {
    pub id: String,
    pub vector: [f32; 8],
    pub metadata: String,
}

pub struct KrishnaL2 {
    pub nodes: Vec<LatticeNode>,
}

impl KrishnaL2 {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// E8 Lattice Quantization Algorithm
    /// Finds the closest lattice point in the 8D E8 lattice.
    /// E8 is defined as the union of D8 and (D8 + 1/2 * 1).
    pub fn quantize_e8(v: [f32; 8]) -> [f32; 8] {
        let f_v = Self::closest_d8(v);
        
        let mut v_shifted = [0.0f32; 8];
        for i in 0..8 { v_shifted[i] = v[i] - 0.5; }
        let g_v = Self::closest_d8(v_shifted);
        
        let mut g_v_final = [0.0f32; 8];
        for i in 0..8 { g_v_final[i] = g_v[i] + 0.5; }

        let dist_f = Self::euclidean_dist(v, f_v);
        let dist_g = Self::euclidean_dist(v, g_v_final);

        if dist_f < dist_g { f_v } else { g_v_final }
    }

    /// Closest point in D8 lattice (sum of coordinates is even)
    fn closest_d8(v: [f32; 8]) -> [f32; 8] {
        let mut rounded = [0.0f32; 8];
        let mut sum = 0;
        let mut diffs = [0.0f32; 8];

        for i in 0..8 {
            rounded[i] = v[i].round();
            sum += rounded[i] as i32;
            diffs[i] = v[i] - rounded[i];
        }

        if sum % 2 != 0 {
            // Find component with largest rounding error to flip
            let mut best_idx = 0;
            let mut max_diff = -1.0;
            for i in 0..8 {
                let d = diffs[i].abs();
                if d > max_diff {
                    max_diff = d;
                    best_idx = i;
                }
            }
            if diffs[best_idx] > 0.0 {
                rounded[best_idx] += 1.0;
            } else {
                rounded[best_idx] -= 1.0;
            }
        }
        rounded
    }

    fn euclidean_dist(a: [f32; 8], b: [f32; 8]) -> f32 {
        let mut sum = 0.0;
        for i in 0..8 {
            sum += (a[i] - b[i]).powi(2);
        }
        sum.sqrt()
    }

    /// Calculate G-Metric (Gap Score)
    pub fn calculate_g_score(&self, q: [f32; 8]) -> f32 {
        if self.nodes.is_empty() { return 1.0; }
        
        let mut min_dist = f32::MAX;
        for node in &self.nodes {
            let d = Self::euclidean_dist(q, node.vector);
            if d < min_dist { min_dist = d; }
        }

        // Apply tanh for normalization (0.0 to 1.0)
        (min_dist * 0.5).tanh()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_e8_quantization() {
        let v = [0.1, 0.2, 0.7, 0.4, 0.3, 0.9, 0.1, 0.2];
        let k = KrishnaL2::quantize_e8(v);
        // Sum of E8 points (after *2 if shifted) must follow specific parity
        let sum: f32 = k.iter().sum();
        println!("Quantized K: {:?}, Sum: {}", k, sum);
    }
}
