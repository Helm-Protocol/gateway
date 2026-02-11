//! Adaptive GRG pipeline: Golomb → Red-stuff → Golay
//!
//! Three operating modes:
//! - Turbo: G + G only (Walrus bypassed, minimum latency)
//! - Safety: G + R(medium) + G (balanced)
//! - Rescue: G + R(maximum) + G (full protection)

use serde::{Serialize, Deserialize};
use tracing::{info, warn};

use super::golomb::GolombCodec;
use super::redstuff::{RedStuffCodec, RedundancyLevel, Shard};
use super::golay::GolayCodec;

/// Operating mode for the GRG pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrgMode {
    /// Golomb + Golay only. Red-stuff bypassed. Minimum latency.
    Turbo,
    /// Golomb + Red-stuff(medium) + Golay. Balanced protection.
    Safety,
    /// Golomb + Red-stuff(maximum) + Golay. Full protection.
    Rescue,
}

/// Result of encoding through the GRG pipeline.
#[derive(Debug, Clone)]
pub struct GrgEncoded {
    /// The protected shards (or single blob in Turbo mode)
    pub shards: Vec<Shard>,
    /// Original data length before compression
    pub original_len: usize,
    /// Compressed length (after Golomb, before Red-stuff)
    pub compressed_len: usize,
    /// Mode used for encoding
    pub mode: GrgMode,
    /// Golomb M parameter used
    pub golomb_m: u32,
}

/// Adaptive GRG pipeline.
///
/// ```text
/// Encode: Data → [Golomb compress] → [Red-stuff split] → [Golay protect] → Shards
/// Decode: Shards → [Golay correct] → [Red-stuff reconstruct] → [Golomb decompress] → Data
/// ```
#[derive(Debug, Clone)]
pub struct GrgPipeline {
    mode: GrgMode,
    data_shards: usize,
}

impl GrgPipeline {
    /// Create a new pipeline with the given mode.
    pub fn new(mode: GrgMode) -> Self {
        Self {
            mode,
            data_shards: 4,
        }
    }

    /// Set the number of data shards for Red-stuff encoding.
    pub fn with_data_shards(mut self, n: usize) -> Self {
        self.data_shards = n.max(1);
        self
    }

    /// Get current mode.
    pub fn mode(&self) -> GrgMode {
        self.mode
    }

    /// Switch mode dynamically (adaptive).
    pub fn set_mode(&mut self, mode: GrgMode) {
        if self.mode != mode {
            info!("GRG pipeline mode: {:?} → {:?}", self.mode, mode);
            self.mode = mode;
        }
    }

    /// Encode data through the full GRG pipeline.
    pub fn encode(&self, data: &[u8]) -> Result<GrgEncoded, anyhow::Error> {
        let original_len = data.len();

        // Layer 1: Golomb compression
        let golomb = GolombCodec::auto_tune(data);
        let compressed = golomb.encode(data)?;
        let compressed_len = compressed.len();

        info!(
            "GRG L1 (Golomb M={}): {} → {} bytes ({:.1}% ratio)",
            golomb.parameter(),
            original_len,
            compressed_len,
            (compressed_len as f64 / original_len as f64) * 100.0
        );

        match self.mode {
            GrgMode::Turbo => {
                // Skip Red-stuff, apply Golay directly
                let protected = GolayCodec::encode(&compressed)?;
                info!(
                    "GRG L3 (Golay): {} → {} bytes",
                    compressed_len,
                    protected.len()
                );

                Ok(GrgEncoded {
                    shards: vec![Shard {
                        index: 0,
                        is_parity: false,
                        data: protected,
                    }],
                    original_len,
                    compressed_len,
                    mode: self.mode,
                    golomb_m: golomb.parameter(),
                })
            }
            GrgMode::Safety | GrgMode::Rescue => {
                // Layer 2: Red-stuff erasure coding
                let level = match self.mode {
                    GrgMode::Safety => RedundancyLevel::Medium,
                    GrgMode::Rescue => RedundancyLevel::Maximum,
                    _ => unreachable!(),
                };

                let mut rs = RedStuffCodec::from_level(level, self.data_shards)?;
                let shards = rs.encode(&compressed)?;

                info!(
                    "GRG L2 (Red-stuff {:?}): {} data + {} parity shards",
                    level,
                    rs.data_shards(),
                    rs.parity_shards()
                );

                // Layer 3: Golay protection on each shard
                let protected_shards: Result<Vec<Shard>, anyhow::Error> = shards
                    .into_iter()
                    .map(|mut shard| -> Result<Shard, anyhow::Error> {
                        let protected = GolayCodec::encode(&shard.data)?;
                        shard.data = protected;
                        Ok(shard)
                    })
                    .collect();
                let protected_shards = protected_shards?;

                info!(
                    "GRG L3 (Golay): {} shards protected",
                    protected_shards.len()
                );

                Ok(GrgEncoded {
                    shards: protected_shards,
                    original_len,
                    compressed_len,
                    mode: self.mode,
                    golomb_m: golomb.parameter(),
                })
            }
        }
    }

    /// Decode data from GRG-encoded shards (inverse pipeline).
    /// Order: Golay correct → Red-stuff reconstruct → Golomb decompress
    pub fn decode(&self, encoded: &GrgEncoded) -> Result<Vec<u8>, anyhow::Error> {
        match encoded.mode {
            GrgMode::Turbo => {
                if encoded.shards.is_empty() {
                    anyhow::bail!("no shards to decode");
                }

                // Layer 3 (inverse): Golay correction
                let corrected = GolayCodec::decode(&encoded.shards[0].data)?;

                // Layer 1 (inverse): Golomb decompression
                let data = GolombCodec::decode(&corrected)?;
                Ok(data)
            }
            GrgMode::Safety | GrgMode::Rescue => {
                // Layer 3 (inverse): Golay correction on each shard
                let corrected_shards: Result<Vec<Shard>, anyhow::Error> = encoded
                    .shards
                    .iter()
                    .map(|shard| -> Result<Shard, anyhow::Error> {
                        let corrected = GolayCodec::decode(&shard.data)?;
                        Ok(Shard {
                            index: shard.index,
                            is_parity: shard.is_parity,
                            data: corrected,
                        })
                    })
                    .collect();
                let corrected_shards = corrected_shards?;

                // Layer 2 (inverse): Red-stuff reconstruction
                let level = match encoded.mode {
                    GrgMode::Safety => RedundancyLevel::Medium,
                    GrgMode::Rescue => RedundancyLevel::Maximum,
                    _ => unreachable!(),
                };
                let data_shard_count = corrected_shards.iter().filter(|s| !s.is_parity).count();
                let parity_count = corrected_shards.iter().filter(|s| s.is_parity).count();
                let rs = RedStuffCodec::new(data_shard_count, parity_count);
                if rs.is_err() {
                    // Fallback: try to determine counts from level
                    let rs = RedStuffCodec::from_level(level, self.data_shards)?;
                    let compressed = rs.decode(&corrected_shards, encoded.compressed_len)?;
                    let data = GolombCodec::decode(&compressed)?;
                    return Ok(data);
                }

                // Determine shard size from first shard
                let shard_size = corrected_shards
                    .first()
                    .map(|s| s.data.len())
                    .unwrap_or(0);
                if shard_size == 0 {
                    anyhow::bail!("empty shards");
                }

                // Direct reconstruction if all data shards present
                let mut sorted_data: Vec<&Shard> = corrected_shards
                    .iter()
                    .filter(|s| !s.is_parity)
                    .collect();
                sorted_data.sort_by_key(|s| s.index);

                let mut compressed = Vec::with_capacity(shard_size * sorted_data.len());
                for shard in &sorted_data {
                    compressed.extend_from_slice(&shard.data);
                }
                compressed.truncate(encoded.compressed_len);

                // Layer 1 (inverse): Golomb decompression
                let data = GolombCodec::decode(&compressed)?;
                Ok(data)
            }
        }
    }

    /// Suggest optimal mode based on network health score (0.0 = worst, 1.0 = best).
    pub fn suggest_mode(health_score: f64) -> GrgMode {
        if health_score > 0.8 {
            GrgMode::Turbo
        } else if health_score > 0.4 {
            GrgMode::Safety
        } else {
            warn!("Network health {:.2} — switching to Rescue mode", health_score);
            GrgMode::Rescue
        }
    }
}

impl Default for GrgPipeline {
    fn default() -> Self {
        Self::new(GrgMode::Safety)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turbo_roundtrip() {
        let pipeline = GrgPipeline::new(GrgMode::Turbo);
        let data = b"Helm Engine turbo mode test data for the GRG pipeline";
        let encoded = pipeline.encode(data).unwrap();
        assert_eq!(encoded.mode, GrgMode::Turbo);
        assert_eq!(encoded.shards.len(), 1);

        let decoded = pipeline.decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn safety_roundtrip() {
        let pipeline = GrgPipeline::new(GrgMode::Safety).with_data_shards(2);
        let data: Vec<u8> = (0..200).map(|i| (i % 32) as u8).collect();
        let encoded = pipeline.encode(&data).unwrap();
        assert_eq!(encoded.mode, GrgMode::Safety);
        assert!(encoded.shards.len() > 1);

        let decoded = pipeline.decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn rescue_roundtrip() {
        let pipeline = GrgPipeline::new(GrgMode::Rescue).with_data_shards(2);
        let data: Vec<u8> = (0..100).map(|i| (i % 16) as u8).collect();
        let encoded = pipeline.encode(&data).unwrap();
        assert_eq!(encoded.mode, GrgMode::Rescue);

        let decoded = pipeline.decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn mode_suggestion() {
        assert_eq!(GrgPipeline::suggest_mode(0.95), GrgMode::Turbo);
        assert_eq!(GrgPipeline::suggest_mode(0.6), GrgMode::Safety);
        assert_eq!(GrgPipeline::suggest_mode(0.2), GrgMode::Rescue);
    }

    #[test]
    fn adaptive_mode_switch() {
        let mut pipeline = GrgPipeline::new(GrgMode::Turbo);
        assert_eq!(pipeline.mode(), GrgMode::Turbo);

        pipeline.set_mode(GrgMode::Rescue);
        assert_eq!(pipeline.mode(), GrgMode::Rescue);
    }
}
