//! Golomb-Rice coding for source compression.
//!
//! Optimal for geometric distributions (sensor data, sparse deltas).
//! Parameter M controls the trade-off between quotient (unary) and
//! remainder (binary) lengths.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum GolombError {
    #[error("parameter M must be a power of 2, got {0}")]
    InvalidParameter(u32),
    #[error("empty input data")]
    EmptyInput,
    #[error("corrupted bitstream during decode")]
    CorruptedStream,
}

/// Golomb-Rice encoder/decoder with configurable parameter M.
#[derive(Debug, Clone)]
pub struct GolombCodec {
    /// M parameter (must be power of 2 for Rice coding)
    m: u32,
    /// log2(M) — number of bits for remainder
    k: u32,
}

impl GolombCodec {
    /// Create a new Golomb-Rice codec.
    /// `m` must be a power of 2.
    pub fn new(m: u32) -> Result<Self, GolombError> {
        if m == 0 || (m & (m - 1)) != 0 {
            return Err(GolombError::InvalidParameter(m));
        }
        let k = m.trailing_zeros();
        Ok(Self { m, k })
    }

    /// Auto-select optimal M based on data distribution.
    pub fn auto_tune(data: &[u8]) -> Self {
        if data.is_empty() {
            return Self { m: 4, k: 2 };
        }
        let mean: f64 = data.iter().map(|&b| b as f64).sum::<f64>() / data.len() as f64;
        // Optimal M ≈ ceil(mean * ln(2)) rounded to nearest power of 2
        let optimal = ((mean * std::f64::consts::LN_2).ceil() as u32).max(1).next_power_of_two();
        let m = optimal.clamp(2, 256);
        let k = m.trailing_zeros();
        Self { m, k }
    }

    /// Encode a byte slice using Golomb-Rice coding.
    /// Returns a packed bitstream as Vec<u8>.
    pub fn encode(&self, data: &[u8]) -> Result<Vec<u8>, GolombError> {
        if data.is_empty() {
            return Err(GolombError::EmptyInput);
        }

        let mut bits = BitWriter::new();

        // Header: original length (4 bytes) + M parameter (1 byte)
        let len = data.len() as u32;
        for i in (0..32).rev() {
            bits.write_bit((len >> i) & 1 == 1);
        }
        bits.write_byte(self.k as u8);

        for &byte in data {
            let value = byte as u32;
            let q = value / self.m;
            let r = value % self.m;

            // Quotient: q zeros followed by a 1
            for _ in 0..q {
                bits.write_bit(false);
            }
            bits.write_bit(true);

            // Remainder: k bits in binary
            for i in (0..self.k).rev() {
                bits.write_bit((r >> i) & 1 == 1);
            }
        }

        Ok(bits.finish())
    }

    /// Decode a Golomb-Rice encoded bitstream back to bytes.
    pub fn decode(encoded: &[u8]) -> Result<Vec<u8>, GolombError> {
        if encoded.len() < 5 {
            return Err(GolombError::CorruptedStream);
        }

        let mut reader = BitReader::new(encoded);

        // Read header
        let mut len = 0u32;
        for _ in 0..32 {
            len = (len << 1) | reader.read_bit().ok_or(GolombError::CorruptedStream)? as u32;
        }
        let k = reader.read_byte().ok_or(GolombError::CorruptedStream)? as u32;
        let m = 1u32 << k;

        let mut output = Vec::with_capacity(len as usize);

        for _ in 0..len {
            // Read quotient (unary)
            let mut q = 0u32;
            loop {
                match reader.read_bit() {
                    Some(true) => break,
                    Some(false) => q += 1,
                    None => return Err(GolombError::CorruptedStream),
                }
            }

            // Read remainder (k bits)
            let mut r = 0u32;
            for _ in 0..k {
                let bit = reader.read_bit().ok_or(GolombError::CorruptedStream)?;
                r = (r << 1) | bit as u32;
            }

            let value = q * m + r;
            if value > 255 {
                return Err(GolombError::CorruptedStream);
            }
            output.push(value as u8);
        }

        Ok(output)
    }

    pub fn parameter(&self) -> u32 {
        self.m
    }
}

/// Bit-level writer for building packed bitstreams.
struct BitWriter {
    buffer: Vec<u8>,
    current: u8,
    bit_pos: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            buffer: Vec::new(),
            current: 0,
            bit_pos: 0,
        }
    }

    fn write_bit(&mut self, bit: bool) {
        if bit {
            self.current |= 1 << (7 - self.bit_pos);
        }
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.buffer.push(self.current);
            self.current = 0;
            self.bit_pos = 0;
        }
    }

    fn write_byte(&mut self, byte: u8) {
        for i in (0..8).rev() {
            self.write_bit((byte >> i) & 1 == 1);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bit_pos > 0 {
            self.buffer.push(self.current);
        }
        self.buffer
    }
}

/// Bit-level reader for parsing packed bitstreams.
struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn read_bit(&mut self) -> Option<bool> {
        if self.byte_pos >= self.data.len() {
            return None;
        }
        let bit = (self.data[self.byte_pos] >> (7 - self.bit_pos)) & 1 == 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Some(bit)
    }

    fn read_byte(&mut self) -> Option<u8> {
        let mut byte = 0u8;
        for _ in 0..8 {
            let bit = self.read_bit()?;
            byte = (byte << 1) | bit as u8;
        }
        Some(byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_basic() {
        let codec = GolombCodec::new(4).unwrap();
        let data = vec![0, 1, 2, 3, 4, 5, 10, 20, 100, 255];
        let encoded = codec.encode(&data).unwrap();
        let decoded = GolombCodec::decode(&encoded).unwrap();
        assert_eq!(data, decoded);
    }

    #[test]
    fn roundtrip_zeros() {
        let codec = GolombCodec::new(2).unwrap();
        let data = vec![0; 100];
        let encoded = codec.encode(&data).unwrap();
        let decoded = GolombCodec::decode(&encoded).unwrap();
        assert_eq!(data, decoded);
    }

    #[test]
    fn auto_tune_compresses() {
        let data: Vec<u8> = (0..200).map(|i| (i % 8) as u8).collect();
        let codec = GolombCodec::auto_tune(&data);
        let encoded = codec.encode(&data).unwrap();
        // Compression should reduce size for low-entropy data
        assert!(encoded.len() < data.len());
    }

    #[test]
    fn invalid_parameter() {
        assert!(GolombCodec::new(0).is_err());
        assert!(GolombCodec::new(3).is_err());
        assert!(GolombCodec::new(6).is_err());
        assert!(GolombCodec::new(4).is_ok());
        assert!(GolombCodec::new(16).is_ok());
    }

    #[test]
    fn empty_input_error() {
        let codec = GolombCodec::new(4).unwrap();
        assert!(codec.encode(&[]).is_err());
    }
}
