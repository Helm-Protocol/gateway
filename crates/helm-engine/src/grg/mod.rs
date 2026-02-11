//! GRG Pipeline: Golomb → Red-stuff → Golay
//!
//! 3-layer hierarchical defense architecture:
//! - Layer 1 (Golomb): Source coding — data compression
//! - Layer 2 (Red-stuff): Erasure coding — distributed recovery
//! - Layer 3 (Golay): Channel coding — bit-level error correction
//!
//! Processing order: Compress → Distribute → Protect
//! Receive order (inverse): Correct → Reconstruct → Decompress

pub mod golomb;
pub mod redstuff;
pub mod golay;
pub mod pipeline;
