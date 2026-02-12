//! Extended Golay (24,12) error-correcting code.
//!
//! The last line of defense in the GRG pipeline.
//! Corrects up to 3 bit errors per 24-bit codeword.
//! Uses the (23,12) generator polynomial extended with an overall parity bit.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum GolayError {
    #[error("uncorrectable error detected (>3 bit errors)")]
    Uncorrectable,
    #[error("empty input")]
    EmptyInput,
}

/// Generator polynomial for the (23,12) Golay code:
/// g(x) = x^11 + x^10 + x^6 + x^5 + x^4 + x^2 + 1
const GOLAY_GEN: u32 = (1 << 11) | (1 << 10) | (1 << 6) | (1 << 5) | (1 << 4) | (1 << 2) | 1;

/// Extended Golay (24,12) codec.
///
/// Properties:
/// - 12 data bits → 24 coded bits (rate 1/2)
/// - Minimum distance: 8
/// - Corrects up to 3 bit errors per codeword
#[derive(Debug, Clone)]
pub struct GolayCodec;

impl GolayCodec {
    /// Compute the 11-bit remainder of (data << 11) mod g(x) in GF(2).
    fn poly_mod(data: u16) -> u32 {
        let mut r = (data as u32) << 11;
        for i in (11..23).rev() {
            if (r >> i) & 1 == 1 {
                r ^= GOLAY_GEN << (i - 11);
            }
        }
        r & 0x7FF
    }

    /// Compute overall parity bit (XOR of all bits).
    fn parity_bit(v: u32) -> u32 {
        v.count_ones() & 1
    }

    /// Encode 12 data bits into a 24-bit extended Golay codeword.
    ///
    /// Layout: bit 23 = overall parity, bits 22..11 = data, bits 10..0 = check
    pub fn encode_word(data: u16) -> u32 {
        let data = (data & 0x0FFF) as u32;
        let check = Self::poly_mod(data as u16);
        let word23 = (data << 11) | check;
        let p = Self::parity_bit(word23);
        word23 | (p << 23)
    }

    /// Compute 11-bit syndrome of the 23-bit (non-parity) part.
    fn syndrome_23(word23: u32) -> u32 {
        let mut r = word23 & 0x7FFFFF;
        for i in (11..23).rev() {
            if (r >> i) & 1 == 1 {
                r ^= GOLAY_GEN << (i - 11);
            }
        }
        r & 0x7FF
    }

    /// Decode a 24-bit extended Golay codeword, correcting up to 3 bit errors.
    /// Returns the 12-bit data word.
    pub fn decode_word(received: u32) -> Result<u16, GolayError> {
        let word23 = received & 0x7FFFFF;
        let received_parity = (received >> 23) & 1;
        let computed_parity = Self::parity_bit(word23);
        let parity_error = received_parity ^ computed_parity;

        let s = Self::syndrome_23(word23);

        // No errors at all
        if s == 0 && parity_error == 0 {
            return Ok(((word23 >> 11) & 0x0FFF) as u16);
        }

        // Error only in the parity bit (bit 23)
        if s == 0 && parity_error == 1 {
            return Ok(((word23 >> 11) & 0x0FFF) as u16);
        }

        // Weight-based fast path for 1-bit errors in bits 0..22
        if s.count_ones() <= 3 && parity_error == 1 {
            // Error pattern matches syndrome directly (in check bits)
            let corrected = word23 ^ s;
            return Ok(((corrected >> 11) & 0x0FFF) as u16);
        }

        // Try single-bit errors in bits 0..22
        for i in 0..23u32 {
            let trial = word23 ^ (1 << i);
            if Self::syndrome_23(trial) == 0 {
                // Single bit error + possible parity error = valid
                return Ok(((trial >> 11) & 0x0FFF) as u16);
            }
        }

        // Try double-bit errors in bits 0..22
        for i in 0..23u32 {
            for j in (i + 1)..23 {
                let trial = word23 ^ (1 << i) ^ (1 << j);
                if Self::syndrome_23(trial) == 0 {
                    return Ok(((trial >> 11) & 0x0FFF) as u16);
                }
            }
        }

        // Try triple-bit errors (up to 3 errors total in 24 bits;
        // if parity_error == 1 then 3 errors in bits 0..22;
        // if parity_error == 0 then 2 errors in bits 0..22 + 1 in bit 23,
        // which is already handled above)
        if parity_error == 1 {
            for i in 0..23u32 {
                for j in (i + 1)..23 {
                    for k in (j + 1)..23 {
                        let trial = word23 ^ (1 << i) ^ (1 << j) ^ (1 << k);
                        if Self::syndrome_23(trial) == 0 {
                            return Ok(((trial >> 11) & 0x0FFF) as u16);
                        }
                    }
                }
            }
        }

        Err(GolayError::Uncorrectable)
    }

    /// Encode a byte slice by processing 12 bits at a time.
    pub fn encode(data: &[u8]) -> Result<Vec<u8>, GolayError> {
        if data.is_empty() {
            return Err(GolayError::EmptyInput);
        }

        let total_bits = data.len() * 8;
        let num_words = total_bits.div_ceil(12);

        let mut encoded = Vec::with_capacity(num_words * 3 + 4);
        encoded.extend_from_slice(&(data.len() as u32).to_be_bytes());

        let mut bit_pos = 0;
        for _ in 0..num_words {
            let mut word = 0u16;
            for b in 0..12 {
                let byte_idx = (bit_pos + b) / 8;
                let bit_idx = 7 - ((bit_pos + b) % 8);
                if byte_idx < data.len() && (data[byte_idx] >> bit_idx) & 1 == 1 {
                    word |= 1 << b;
                }
            }
            bit_pos += 12;

            let codeword = Self::encode_word(word);
            encoded.push((codeword >> 16) as u8);
            encoded.push((codeword >> 8) as u8);
            encoded.push(codeword as u8);
        }

        Ok(encoded)
    }

    /// Decode Golay-protected data back to original bytes.
    pub fn decode(encoded: &[u8]) -> Result<Vec<u8>, GolayError> {
        if encoded.len() < 4 {
            return Err(GolayError::EmptyInput);
        }

        let original_len = u32::from_be_bytes([
            encoded[0], encoded[1], encoded[2], encoded[3],
        ]) as usize;

        let codeword_bytes = &encoded[4..];
        if !codeword_bytes.len().is_multiple_of(3) {
            return Err(GolayError::Uncorrectable);
        }

        let num_words = codeword_bytes.len() / 3;
        let total_bits = original_len * 8;
        let mut output = vec![0u8; original_len];
        let mut bit_pos = 0;

        for i in 0..num_words {
            let codeword = ((codeword_bytes[i * 3] as u32) << 16)
                | ((codeword_bytes[i * 3 + 1] as u32) << 8)
                | (codeword_bytes[i * 3 + 2] as u32);

            let word = Self::decode_word(codeword)?;

            for b in 0..12 {
                if bit_pos + b >= total_bits {
                    break;
                }
                if (word >> b) & 1 == 1 {
                    let byte_idx = (bit_pos + b) / 8;
                    let bit_idx = 7 - ((bit_pos + b) % 8);
                    if byte_idx < original_len {
                        output[byte_idx] |= 1 << bit_idx;
                    }
                }
            }
            bit_pos += 12;
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_word_no_errors() {
        for data in 0..4096u16 {
            let codeword = GolayCodec::encode_word(data);
            let decoded = GolayCodec::decode_word(codeword).unwrap();
            assert_eq!(data, decoded, "failed for data={data}");
        }
    }

    #[test]
    fn correct_single_bit_error() {
        let data: u16 = 0b101010101010;
        let codeword = GolayCodec::encode_word(data);

        for bit in 0..24 {
            let corrupted = codeword ^ (1 << bit);
            let decoded = GolayCodec::decode_word(corrupted).unwrap();
            assert_eq!(data, decoded, "failed to correct bit {bit}");
        }
    }

    #[test]
    fn correct_double_bit_error() {
        let data: u16 = 0b110011001100;
        let codeword = GolayCodec::encode_word(data);

        let corrupted = codeword ^ (1 << 0) ^ (1 << 5);
        let decoded = GolayCodec::decode_word(corrupted).unwrap();
        assert_eq!(data, decoded);
    }

    #[test]
    fn correct_triple_bit_error() {
        let data: u16 = 0b000111000111;
        let codeword = GolayCodec::encode_word(data);

        let corrupted = codeword ^ (1 << 0) ^ (1 << 8) ^ (1 << 16);
        let decoded = GolayCodec::decode_word(corrupted).unwrap();
        assert_eq!(data, decoded);
    }

    #[test]
    fn byte_roundtrip() {
        let data = b"Helm Engine Golay protection layer";
        let encoded = GolayCodec::encode(data).unwrap();
        let decoded = GolayCodec::decode(&encoded).unwrap();
        assert_eq!(data.as_slice(), decoded.as_slice());
    }

    #[test]
    fn byte_roundtrip_with_corruption() {
        let data = b"Protect this data";
        let mut encoded = GolayCodec::encode(data).unwrap();

        if encoded.len() > 5 {
            encoded[5] ^= 0x04;
        }

        let decoded = GolayCodec::decode(&encoded).unwrap();
        assert_eq!(data.as_slice(), decoded.as_slice());
    }
}
