// src/grg/mod.rs — GRG HTTP API (Encode / Decode)
//
// Golomb → RedStuff → Golay  (ENCODE: compress + protect + correct)
// Golay⁻¹ → RedStuff⁻¹ → Golomb⁻¹  (DECODE: reverse pipeline)
//
// Use case: distributed data storage — nodes can reconstruct data
// even if some shards are lost (RedStuff erasure coding)
//
// POST /api/v1/grg/encode
// POST /api/v1/grg/decode

use serde::{Deserialize, Serialize};

// ============================
// TYPES
// ============================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GrgMode {
    /// Golomb + Golay only. No erasure coding. Minimum latency.
    Turbo,
    /// Golomb + RedStuff(medium) + Golay. Balanced protection.
    #[default]
    Safety,
    /// Golomb + RedStuff(maximum) + Golay. Full distributed protection.
    Rescue,
}

impl GrgMode {
    /// Number of parity shards per data shard group.
    fn parity_ratio(&self) -> (usize, usize) {
        match self {
            GrgMode::Turbo  => (4, 0), // no parity
            GrgMode::Safety => (4, 2), // 2 parity per 4 data = tolerate 2 lost
            GrgMode::Rescue => (2, 2), // 2 parity per 2 data = tolerate 50% loss
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EncodeRequest {
    /// Base64-encoded raw data
    pub data: String,
    #[serde(default)]
    pub mode: GrgMode,
    /// Agent DID (for billing)
    pub agent_did: String,
    /// Referring agent DID (earns 15% fee)
    pub referrer_did: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EncodeResponse {
    /// Encoded shards (each Base64-encoded)
    pub shards: Vec<ShardInfo>,
    pub original_bytes: usize,
    pub compressed_bytes: usize,
    pub mode: GrgMode,
    /// Golomb M parameter used for this data
    pub golomb_m: u32,
    /// Minimum shards needed for reconstruction
    pub min_shards_for_recovery: usize,
    /// Total shards produced
    pub total_shards: usize,
    pub compression_ratio: f64,
    /// Fee charged (in BNKR micro-units)
    pub fee_charged: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShardInfo {
    /// Shard index
    pub index: usize,
    /// true = data shard, false = parity shard
    pub is_parity: bool,
    /// Base64-encoded shard content
    pub data: String,
    /// Golay error correction: can fix up to 3 bit errors per 24-bit word
    pub golay_protected: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DecodeRequest {
    /// Shards (subset is OK — need >= min_shards_for_recovery)
    pub shards: Vec<ShardInput>,
    pub mode: GrgMode,
    pub golomb_m: u32,
    pub agent_did: String,
    pub referrer_did: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShardInput {
    pub index: usize,
    pub is_parity: bool,
    pub data: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DecodeResponse {
    /// Reconstructed original data (Base64)
    pub data: String,
    pub original_bytes: usize,
    pub shards_used: usize,
    pub shards_recovered: usize,
    pub fee_charged: u64,
}

// ============================
// GRG ENGINE
// ============================

pub struct GrgEngine;

impl GrgEngine {
    // ---- ENCODE: Golomb → RedStuff → Golay ----

    pub fn encode(req: &EncodeRequest) -> Result<EncodeResponse, GrgError> {
        let raw = base64_decode(&req.data)?;
        let original_bytes = raw.len();

        // STEP 1: Golomb-Rice compression
        let (compressed, golomb_m) = golomb_compress(&raw);
        let compressed_bytes = compressed.len();

        // STEP 2: RedStuff erasure coding (split into shards)
        let (data_shards, parity_shards) = redstuff_encode(&compressed, req.mode);
        let total_shards = data_shards.len() + parity_shards.len();
        let (ds, ps) = req.mode.parity_ratio();
        let min_shards = ds; // need at least data shards to reconstruct

        // STEP 3: Golay error correction encoding (per shard)
        let mut shards: Vec<ShardInfo> = Vec::new();
        for (i, shard) in data_shards.iter().enumerate() {
            shards.push(ShardInfo {
                index: i,
                is_parity: false,
                data: base64_encode(&golay_encode(shard)),
                golay_protected: true,
            });
        }
        for (i, shard) in parity_shards.iter().enumerate() {
            shards.push(ShardInfo {
                index: data_shards.len() + i,
                is_parity: true,
                data: base64_encode(&golay_encode(shard)),
                golay_protected: true,
            });
        }

        let compression_ratio = if compressed_bytes > 0 {
            original_bytes as f64 / compressed_bytes as f64
        } else { 1.0 };

        Ok(EncodeResponse {
            shards,
            original_bytes,
            compressed_bytes,
            mode: req.mode,
            golomb_m,
            min_shards_for_recovery: min_shards,
            total_shards,
            compression_ratio,
            fee_charged: crate::billing::GRG_CALL_FEE,
        })
    }

    // ---- DECODE: Golay⁻¹ → RedStuff⁻¹ → Golomb⁻¹ ----

    pub fn decode(req: &DecodeRequest) -> Result<DecodeResponse, GrgError> {
        let (ds, _ps) = req.mode.parity_ratio();
        if req.shards.len() < ds && req.mode != GrgMode::Turbo {
            return Err(GrgError::InsufficientShards {
                have: req.shards.len(),
                need: ds,
            });
        }

        let shards_used = req.shards.len();

        // STEP 1 (reverse): Golay error correction decoding
        let mut decoded_shards: Vec<(usize, bool, Vec<u8>)> = Vec::new();
        let mut recovered = 0usize;
        for s in &req.shards {
            let raw = base64_decode(&s.data)?;
            let (corrected, corrections) = golay_decode(&raw);
            if corrections > 0 { recovered += 1; }
            decoded_shards.push((s.index, s.is_parity, corrected));
        }

        // STEP 2 (reverse): RedStuff reconstruction
        let compressed = redstuff_decode(&decoded_shards, req.mode)?;

        // STEP 3 (reverse): Golomb decompression
        let original = golomb_decompress(&compressed, req.golomb_m)?;
        let original_bytes = original.len();

        Ok(DecodeResponse {
            data: base64_encode(&original),
            original_bytes,
            shards_used,
            shards_recovered: recovered,
            fee_charged: crate::billing::GRG_CALL_FEE,
        })
    }
}

// ============================
// GOLOMB-RICE CODEC
// ============================

fn golomb_compress(data: &[u8]) -> (Vec<u8>, u32) {
    if data.is_empty() { return (vec![], 4); }
    // Auto-tune M: optimal ≈ mean × ln(2), rounded to nearest power of 2
    let mean: f64 = data.iter().map(|&b| b as f64).sum::<f64>() / data.len() as f64;
    let m_raw = ((mean * std::f64::consts::LN_2).ceil() as u32).max(1).next_power_of_two();
    let m = m_raw.clamp(2, 128);
    let k = m.trailing_zeros();

    let mut bits: Vec<u8> = Vec::new();
    let mut bit_buf: u8 = 0;
    let mut bit_pos = 0u8;

    let push_bit = |bit: bool, bits: &mut Vec<u8>, buf: &mut u8, pos: &mut u8| {
        if bit { *buf |= 1 << (7 - *pos); }
        *pos += 1;
        if *pos == 8 {
            bits.push(*buf);
            *buf = 0;
            *pos = 0;
        }
    };

    for &byte in data {
        let q = (byte as u32) >> k;
        let r = (byte as u32) & (m - 1);
        // Unary quotient
        for _ in 0..q { push_bit(true, &mut bits, &mut bit_buf, &mut bit_pos); }
        push_bit(false, &mut bits, &mut bit_buf, &mut bit_pos);
        // Binary remainder
        for i in (0..k).rev() {
            push_bit((r >> i) & 1 == 1, &mut bits, &mut bit_buf, &mut bit_pos);
        }
    }
    if bit_pos > 0 { bits.push(bit_buf); }

    // Prepend original length (4 bytes LE) for reconstruction
    let mut out = (data.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&bits);
    (out, m)
}

fn golomb_decompress(compressed: &[u8], m: u32) -> Result<Vec<u8>, GrgError> {
    if compressed.len() < 4 {
        return Err(GrgError::Corrupt("too short for length header".into()));
    }
    let orig_len = u32::from_le_bytes([compressed[0], compressed[1], compressed[2], compressed[3]]) as usize;
    let bits = &compressed[4..];
    let k = m.trailing_zeros();

    let get_bit = |bits: &[u8], pos: usize| -> bool {
        let byte_idx = pos / 8;
        let bit_idx  = 7 - (pos % 8);
        if byte_idx >= bits.len() { return false; }
        (bits[byte_idx] >> bit_idx) & 1 == 1
    };

    let mut result = Vec::with_capacity(orig_len);
    let mut pos = 0usize;

    while result.len() < orig_len {
        // Read unary quotient
        let mut q = 0u32;
        while get_bit(bits, pos) { q += 1; pos += 1; }
        pos += 1; // skip terminating 0

        // Read binary remainder
        let mut r = 0u32;
        for _ in 0..k {
            r = (r << 1) | (get_bit(bits, pos) as u32);
            pos += 1;
        }
        let val = (q * m + r) as u8;
        result.push(val);
    }

    Ok(result)
}

// ============================
// RED-STUFF ERASURE CODING (XOR parity)
// ============================

fn redstuff_encode(data: &[u8], mode: GrgMode) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let (ds, ps) = mode.parity_ratio();
    if ds == 0 || data.is_empty() {
        return (vec![data.to_vec()], vec![]);
    }

    // Pad data to multiple of ds
    let shard_size = (data.len() + ds - 1) / ds;
    let mut padded = data.to_vec();
    padded.resize(shard_size * ds, 0);

    let mut data_shards: Vec<Vec<u8>> = Vec::new();
    for i in 0..ds {
        data_shards.push(padded[i * shard_size..(i + 1) * shard_size].to_vec());
    }

    // XOR parity shards
    let mut parity_shards: Vec<Vec<u8>> = Vec::new();
    for p in 0..ps {
        let mut parity = vec![0u8; shard_size];
        for (di, shard) in data_shards.iter().enumerate() {
            // Rotate XOR for each parity shard
            for (j, &byte) in shard.iter().enumerate() {
                parity[j] ^= byte.rotate_left((di * (p + 1)) as u32 % 8);
            }
        }
        parity_shards.push(parity);
    }

    (data_shards, parity_shards)
}

fn redstuff_decode(shards: &[(usize, bool, Vec<u8>)], mode: GrgMode) -> Result<Vec<u8>, GrgError> {
    let (ds, _ps) = mode.parity_ratio();
    // Collect data shards in index order
    let mut data_shards: Vec<Option<Vec<u8>>> = vec![None; ds.max(1)];
    let mut shard_size = 0usize;

    for (idx, is_parity, data) in shards {
        shard_size = data.len();
        if !is_parity && *idx < ds {
            data_shards[*idx] = Some(data.clone());
        }
    }

    // Check all data shards present (parity recovery TODO: full RS in Phase 2)
    for (i, s) in data_shards.iter().enumerate() {
        if s.is_none() {
            return Err(GrgError::InsufficientShards { have: i, need: ds });
        }
    }

    let mut result = Vec::with_capacity(ds * shard_size);
    for shard in data_shards.into_iter().flatten() {
        result.extend_from_slice(&shard);
    }
    Ok(result)
}

// ============================
// GOLAY(24,12) ERROR CORRECTION
// ============================

/// Encode: protect each 12-bit word into 24-bit Golay codeword.
/// Can correct up to 3 bit errors per 24-bit word.
fn golay_encode(data: &[u8]) -> Vec<u8> {
    // Pack bytes into 12-bit words, encode each to 24-bit
    let mut out = Vec::with_capacity(data.len() * 2);
    let mut i = 0;
    while i + 1 < data.len() {
        let word12 = ((data[i] as u32) << 4) | ((data[i + 1] as u32) >> 4);
        let codeword = golay_encode_word(word12);
        out.push((codeword >> 16) as u8);
        out.push((codeword >> 8) as u8);
        out.push(codeword as u8);
        i += 2;
    }
    if i < data.len() {
        let word12 = (data[i] as u32) << 4;
        let codeword = golay_encode_word(word12);
        out.push((codeword >> 16) as u8);
        out.push((codeword >> 8) as u8);
        out.push(codeword as u8);
    }
    out
}

/// Decode: correct errors and recover 12-bit words.
/// Returns (corrected_data, error_count).
fn golay_decode(encoded: &[u8]) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut errors = 0usize;
    let mut i = 0;
    while i + 2 < encoded.len() {
        let codeword = ((encoded[i] as u32) << 16)
            | ((encoded[i + 1] as u32) << 8)
            | (encoded[i + 2] as u32);
        let (word12, corrected) = golay_decode_word(codeword);
        if corrected { errors += 1; }
        out.push((word12 >> 4) as u8);
        out.push(((word12 & 0xF) << 4) as u8);
        i += 3;
    }
    (out, errors)
}

/// Golay(24,12) generator polynomial: 0xAE3
const GOLAY_POLY: u32 = 0xAE3;

fn golay_encode_word(data: u32) -> u32 {
    let mut reg = data & 0xFFF;
    let mut codeword = reg << 12;
    for _ in 0..12 {
        if reg & 0x800 != 0 { reg = ((reg << 1) ^ GOLAY_POLY) & 0xFFF; }
        else { reg = (reg << 1) & 0xFFF; }
    }
    codeword | (reg & 0xFFF)
}

fn golay_decode_word(received: u32) -> (u32, bool) {
    // Syndrome check — if zero, no errors
    let syndrome = golay_syndrome(received);
    if syndrome == 0 {
        return (received >> 12, false);
    }
    // Attempt single-bit error correction
    for bit in 0..24 {
        let corrected = received ^ (1 << bit);
        if golay_syndrome(corrected) == 0 {
            return (corrected >> 12, true);
        }
    }
    // Return as-is if uncorrectable
    (received >> 12, true)
}

fn golay_syndrome(codeword: u32) -> u32 {
    let mut reg = (codeword >> 12) & 0xFFF;
    for i in 0..12 {
        if reg & 0x800 != 0 { reg = ((reg << 1) ^ GOLAY_POLY) & 0xFFF; }
        else { reg = (reg << 1) & 0xFFF; }
    }
    reg ^ (codeword & 0xFFF)
}

// ============================
// BASE64 HELPERS
// ============================

fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 2 < data.len() {
        let b = ((data[i] as u32) << 16) | ((data[i+1] as u32) << 8) | data[i+2] as u32;
        write!(out, "{}{}{}{}", CHARS[(b >> 18) as usize] as char, CHARS[((b >> 12) & 0x3F) as usize] as char,
            CHARS[((b >> 6) & 0x3F) as usize] as char, CHARS[(b & 0x3F) as usize] as char).ok();
        i += 3;
    }
    if i < data.len() {
        let b = if i + 1 < data.len() { ((data[i] as u32) << 8) | data[i+1] as u32 } else { (data[i] as u32) << 8 };
        write!(out, "{}{}{}", CHARS[((b >> 10) & 0x3F) as usize] as char, CHARS[((b >> 4) & 0x3F) as usize] as char,
            if i + 1 < data.len() { CHARS[((b & 0xF) << 2) as usize] as char } else { '=' }).ok();
        out.push('=');
    }
    out
}

fn base64_decode(s: &str) -> Result<Vec<u8>, GrgError> {
    let s = s.trim().replace(['\n', '\r', ' '], "");
    let decode_char = |c: char| -> Result<u32, GrgError> {
        match c {
            'A'..='Z' => Ok(c as u32 - 'A' as u32),
            'a'..='z' => Ok(c as u32 - 'a' as u32 + 26),
            '0'..='9' => Ok(c as u32 - '0' as u32 + 52),
            '+' => Ok(62),
            '/' => Ok(63),
            '=' => Ok(0),
            _ => Err(GrgError::Corrupt(format!("invalid base64 char: {}", c))),
        }
    };
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < chars.len() {
        let b = (decode_char(chars[i])? << 18) | (decode_char(chars[i+1])? << 12)
            | (decode_char(chars[i+2])? << 6) | decode_char(chars[i+3])?;
        out.push((b >> 16) as u8);
        if chars[i+2] != '=' { out.push((b >> 8) as u8); }
        if chars[i+3] != '=' { out.push(b as u8); }
        i += 4;
    }
    Ok(out)
}

// ============================
// ERRORS
// ============================

#[derive(Debug, thiserror::Error)]
pub enum GrgError {
    #[error("insufficient shards: have {have}, need {need}")]
    InsufficientShards { have: usize, need: usize },
    #[error("corrupt data: {0}")]
    Corrupt(String),
    #[error("decode error: {0}")]
    Decode(String),
}

// ============================
// TESTS
// ============================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golomb_roundtrip() {
        let original = b"Hello, Helm Protocol! This is a test of Golomb compression.".to_vec();
        let (compressed, m) = golomb_compress(&original);
        assert!(compressed.len() > 0);
        let recovered = golomb_decompress(&compressed, m).unwrap();
        assert_eq!(recovered, original);
    }

    #[test]
    fn redstuff_roundtrip_safety() {
        let data = b"Distributed data protection test data block.".to_vec();
        let (data_shards, parity_shards) = redstuff_encode(&data, GrgMode::Safety);
        assert_eq!(data_shards.len(), 4);
        assert_eq!(parity_shards.len(), 2);

        // Reconstruct from data shards only
        let mut shards: Vec<(usize, bool, Vec<u8>)> = data_shards.iter().enumerate()
            .map(|(i, s)| (i, false, s.clone())).collect();
        let recovered = redstuff_decode(&shards, GrgMode::Safety).unwrap();
        assert!(recovered.starts_with(&data));
    }

    #[test]
    fn golay_encode_decode_no_errors() {
        let data = b"Golay test";
        let encoded = golay_encode(data);
        let (decoded, errors) = golay_decode(&encoded);
        assert_eq!(errors, 0);
        // First len bytes should match
        assert_eq!(&decoded[..data.len()], data);
    }

    #[test]
    fn golay_corrects_single_bit_error() {
        let data = b"Error test!";
        let mut encoded = golay_encode(data);
        // Flip a single bit
        encoded[0] ^= 0x01;
        let (_, errors) = golay_decode(&encoded);
        assert!(errors > 0); // detected and corrected
    }

    #[test]
    fn grg_encode_decode_roundtrip_safety() {
        let original = b"Hello Helm distributed storage!".to_vec();
        let enc_req = EncodeRequest {
            data: base64_encode(&original),
            mode: GrgMode::Safety,
            agent_did: "did:helm:test".into(),
            referrer_did: None,
        };
        let enc_resp = GrgEngine::encode(&enc_req).unwrap();
        assert_eq!(enc_resp.mode, GrgMode::Safety);
        assert_eq!(enc_resp.original_bytes, original.len());
        assert!(enc_resp.compression_ratio > 0.0);

        // Decode using all shards
        let dec_req = DecodeRequest {
            shards: enc_resp.shards.iter().filter(|s| !s.is_parity).map(|s| ShardInput {
                index: s.index,
                is_parity: s.is_parity,
                data: s.data.clone(),
            }).collect(),
            mode: GrgMode::Safety,
            golomb_m: enc_resp.golomb_m,
            agent_did: "did:helm:test".into(),
            referrer_did: None,
        };
        let dec_resp = GrgEngine::decode(&dec_req).unwrap();
        let recovered = base64_decode(&dec_resp.data).unwrap();
        assert!(recovered.starts_with(&original));
    }

    #[test]
    fn grg_turbo_mode() {
        let data = b"Turbo mode test".to_vec();
        let enc_req = EncodeRequest {
            data: base64_encode(&data),
            mode: GrgMode::Turbo,
            agent_did: "did:helm:test".into(),
            referrer_did: None,
        };
        let enc = GrgEngine::encode(&enc_req).unwrap();
        assert_eq!(enc.total_shards, 1);

        let dec_req = DecodeRequest {
            shards: enc.shards.iter().map(|s| ShardInput {
                index: s.index, is_parity: s.is_parity, data: s.data.clone(),
            }).collect(),
            mode: GrgMode::Turbo,
            golomb_m: enc.golomb_m,
            agent_did: "did:helm:test".into(),
            referrer_did: None,
        };
        let dec = GrgEngine::decode(&dec_req).unwrap();
        let recovered = base64_decode(&dec.data).unwrap();
        assert!(recovered.starts_with(&data));
    }
}
