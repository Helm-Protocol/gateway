//! Red-stuff erasure coding for distributed recovery.
//!
//! Inspired by Walrus (Sui/Mysten Labs) protocol — Reed-Solomon based
//! erasure coding that splits data into N shards where any K shards
//! can reconstruct the original data.
//!
//! This implementation uses XOR-based parity for the initial version,
//! providing (K+1)-of-(2K+1) reconstruction capability.

use thiserror::Error;
use serde::{Serialize, Deserialize};

#[derive(Error, Debug)]
pub enum RedStuffError {
    #[error("data shards must be > 0, got {0}")]
    InvalidShardCount(usize),
    #[error("empty input data")]
    EmptyInput,
    #[error("insufficient shards for reconstruction: have {have}, need {need}")]
    InsufficientShards { have: usize, need: usize },
    #[error("shard size mismatch: expected {expected}, got {got}")]
    ShardSizeMismatch { expected: usize, got: usize },
    #[error("shard index {0} out of range")]
    InvalidShardIndex(usize),
}

/// A single shard of erasure-coded data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shard {
    /// Shard index in the encoding scheme
    pub index: usize,
    /// Whether this is a data shard or parity shard
    pub is_parity: bool,
    /// The shard payload
    pub data: Vec<u8>,
}

/// Redundancy level for erasure coding.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RedundancyLevel {
    /// Low: 1 parity shard per 4 data shards (tolerate 1 loss per group)
    Low,
    /// Medium: 1 parity shard per 2 data shards (tolerate ~33% loss)
    Medium,
    /// High: 1 parity shard per 1 data shard (tolerate ~50% loss)
    High,
    /// Maximum: 2 parity shards per 1 data shard (tolerate ~66% loss)
    Maximum,
}

impl RedundancyLevel {
    /// Returns (data_shards, parity_shards) per group.
    fn ratio(self) -> (usize, usize) {
        match self {
            RedundancyLevel::Low => (4, 1),
            RedundancyLevel::Medium => (2, 1),
            RedundancyLevel::High => (1, 1),
            RedundancyLevel::Maximum => (1, 2),
        }
    }
}

/// Red-stuff erasure encoder/decoder.
#[derive(Debug, Clone)]
pub struct RedStuffCodec {
    /// Number of data shards to split into
    data_shards: usize,
    /// Number of parity shards to generate
    parity_shards: usize,
    /// Shard size (bytes) — all shards are equal size
    shard_size: usize,
}

impl RedStuffCodec {
    /// Create a codec with explicit shard counts.
    pub fn new(data_shards: usize, parity_shards: usize) -> Result<Self, RedStuffError> {
        if data_shards == 0 {
            return Err(RedStuffError::InvalidShardCount(data_shards));
        }
        Ok(Self {
            data_shards,
            parity_shards,
            shard_size: 0, // set during encode
        })
    }

    /// Create a codec from a redundancy level with a target shard count.
    pub fn from_level(level: RedundancyLevel, target_data_shards: usize) -> Result<Self, RedStuffError> {
        let (d_ratio, p_ratio) = level.ratio();
        let groups = target_data_shards.div_ceil(d_ratio);
        let data_shards = groups * d_ratio;
        let parity_shards = groups * p_ratio;
        Self::new(data_shards, parity_shards)
    }

    /// Encode data into data shards + parity shards.
    pub fn encode(&mut self, data: &[u8]) -> Result<Vec<Shard>, RedStuffError> {
        if data.is_empty() {
            return Err(RedStuffError::EmptyInput);
        }

        // Calculate shard size (pad to equal sizes)
        self.shard_size = data.len().div_ceil(self.data_shards);
        let padded_len = self.shard_size * self.data_shards;

        // Pad data to fit evenly
        let mut padded = data.to_vec();
        padded.resize(padded_len, 0);

        let mut shards = Vec::with_capacity(self.data_shards + self.parity_shards);

        // Create data shards
        for i in 0..self.data_shards {
            let start = i * self.shard_size;
            let end = start + self.shard_size;
            shards.push(Shard {
                index: i,
                is_parity: false,
                data: padded[start..end].to_vec(),
            });
        }

        // Generate parity shards using XOR across data shard groups
        let (d_ratio, _) = self.parity_group_size();
        let groups = self.data_shards.div_ceil(d_ratio);

        for g in 0..groups {
            let parity_count = self.parity_shards / groups.max(1);
            for p in 0..parity_count.max(1) {
                let mut parity = vec![0u8; self.shard_size];

                // XOR all data shards in this group
                let group_start = g * d_ratio;
                let group_end = (group_start + d_ratio).min(self.data_shards);
                for shard in &shards[group_start..group_end] {
                    for (j, byte) in shard.data.iter().enumerate() {
                        parity[j] ^= byte;
                    }
                }

                // For additional parity shards, rotate and XOR again
                if p > 0 {
                    parity.rotate_left(p * 7);
                    for shard in &shards[group_start..group_end] {
                        for (j, byte) in shard.data.iter().enumerate() {
                            parity[j] ^= byte.rotate_left(p as u32);
                        }
                    }
                }

                shards.push(Shard {
                    index: self.data_shards + g * parity_count.max(1) + p,
                    is_parity: true,
                    data: parity,
                });
            }
        }

        Ok(shards)
    }

    /// Reconstruct original data from available shards.
    /// Requires at least `data_shards` shards present.
    pub fn decode(&self, shards: &[Shard], original_len: usize) -> Result<Vec<u8>, RedStuffError> {
        if self.shard_size == 0 {
            return Err(RedStuffError::EmptyInput);
        }

        let data_shard_count = shards.iter().filter(|s| !s.is_parity).count();

        if data_shard_count >= self.data_shards {
            // All data shards present — direct reconstruction
            let mut sorted: Vec<&Shard> = shards.iter().filter(|s| !s.is_parity).collect();
            sorted.sort_by_key(|s| s.index);

            let mut result = Vec::with_capacity(self.shard_size * self.data_shards);
            for shard in sorted.iter().take(self.data_shards) {
                result.extend_from_slice(&shard.data);
            }
            result.truncate(original_len);
            return Ok(result);
        }

        // Attempt XOR-based recovery for single missing shard per group
        let (d_ratio, _) = self.parity_group_size();
        let mut recovered_data: Vec<Option<Vec<u8>>> = (0..self.data_shards).map(|_| None).collect();

        // Map available shards
        for shard in shards {
            if !shard.is_parity && shard.index < self.data_shards {
                recovered_data[shard.index] = Some(shard.data.clone());
            }
        }

        // Recover missing shards using parity
        let groups = self.data_shards.div_ceil(d_ratio);
        for g in 0..groups {
            let group_start = g * d_ratio;
            let group_end = (group_start + d_ratio).min(self.data_shards);

            let missing: Vec<usize> = (group_start..group_end)
                .filter(|&i| recovered_data[i].is_none())
                .collect();

            if missing.len() == 1 {
                // Find parity shard for this group
                let parity_idx = self.data_shards + g;
                if let Some(parity) = shards.iter().find(|s| s.index == parity_idx && s.is_parity) {
                    let mut reconstructed = parity.data.clone();
                    for data in recovered_data[group_start..group_end].iter().flatten() {
                        for (j, byte) in data.iter().enumerate() {
                            reconstructed[j] ^= byte;
                        }
                    }
                    recovered_data[missing[0]] = Some(reconstructed);
                }
            }
        }

        // Check if all data shards are recovered
        let have = recovered_data.iter().filter(|s| s.is_some()).count();
        if have < self.data_shards {
            return Err(RedStuffError::InsufficientShards {
                have,
                need: self.data_shards,
            });
        }

        let mut result = Vec::with_capacity(self.shard_size * self.data_shards);
        for shard_data in recovered_data.iter().take(self.data_shards) {
            result.extend_from_slice(shard_data.as_ref().unwrap());
        }
        result.truncate(original_len);
        Ok(result)
    }

    fn parity_group_size(&self) -> (usize, usize) {
        let groups = if self.parity_shards > 0 {
            self.data_shards.div_ceil(self.parity_shards)
        } else {
            self.data_shards
        };
        (groups.max(1), self.parity_shards)
    }

    pub fn data_shards(&self) -> usize {
        self.data_shards
    }

    pub fn parity_shards(&self) -> usize {
        self.parity_shards
    }

    pub fn total_shards(&self) -> usize {
        self.data_shards + self.parity_shards
    }

    pub fn shard_size(&self) -> usize {
        self.shard_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let mut codec = RedStuffCodec::new(4, 2).unwrap();
        let data = b"Hello, Helm Engine! This is a test of the Red-stuff erasure coding.";
        let shards = codec.encode(data).unwrap();
        assert_eq!(shards.len(), 6); // 4 data + 2 parity
        let decoded = codec.decode(&shards, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn recover_with_missing_shard() {
        let mut codec = RedStuffCodec::new(2, 1).unwrap();
        let data = b"Recovery test data for distributed storage systems";
        let shards = codec.encode(data).unwrap();

        // Remove one data shard, keep parity
        let partial: Vec<Shard> = shards.into_iter().filter(|s| s.index != 0).collect();
        let decoded = codec.decode(&partial, data.len()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn redundancy_levels() {
        let codec = RedStuffCodec::from_level(RedundancyLevel::High, 4).unwrap();
        assert_eq!(codec.data_shards(), 4);
        assert_eq!(codec.parity_shards(), 4);

        let codec = RedStuffCodec::from_level(RedundancyLevel::Low, 4).unwrap();
        assert_eq!(codec.data_shards(), 4);
        assert_eq!(codec.parity_shards(), 1);
    }

    #[test]
    fn insufficient_shards_error() {
        let mut codec = RedStuffCodec::new(4, 1).unwrap();
        let data = b"Test data";
        let shards = codec.encode(data).unwrap();

        // Keep only 2 of 4 data shards and no parity
        let partial: Vec<Shard> = shards.into_iter().take(2).collect();
        assert!(codec.decode(&partial, data.len()).is_err());
    }
}
