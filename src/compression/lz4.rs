//! # LZ4 Decompression — RFC-compliant LZ4 block decoder
//!
//! Implements LZ4 block format decompression per the LZ4 specification.
//! Used by EROFS (default compressor) and SquashFS (optional compressor).
//!
//! ## Block Format
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │ Token │ LitLen │ Literals │ Offset │ MatchLen   │
//! │ (1B)  │ (0-4B) │ (var)    │ (2B)   │ (0-4B)     │
//! └─────────────────────────────────────────────────┘
//! ```
//!
//! - Token high nibble: literal length (0-15, extended if 15)
//! - Token low nibble: match length (0-15, extended if 15)
//! - Minimum match length: 4 bytes
//! - Match offset: 2 bytes little-endian

use alloc::vec::Vec;

/// LZ4 decompression error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lz4Error {
    /// Input data is truncated
    TruncatedInput,
    /// Match offset exceeds output position
    InvalidOffset,
    /// Output buffer overflow
    OutputOverflow,
    /// Literal length exceeds output bounds
    LiteralOverflow,
    /// Invalid or corrupted data
    InvalidData,
}

/// Decompress an LZ4 block into a buffer of known `decompressed_size`.
///
/// Follows the LZ4 block format specification:
/// - Token byte encodes literal and match lengths in nibbles
/// - Lengths >= 15 are extended with additional bytes (0xFF continuation)
/// - Match offset is 2 bytes little-endian
/// - Minimum match length is 4
pub fn decompress_lz4(data: &[u8], decompressed_size: usize) -> Result<Vec<u8>, Lz4Error> {
    let mut output = Vec::with_capacity(decompressed_size);
    let mut pos = 0usize;

    while pos < data.len() {
        let token = data[pos];
        pos += 1;

        // Decode literal length from high nibble
        let mut lit_len = (token >> 4) as usize;
        if lit_len == 15 {
            loop {
                if pos >= data.len() {
                    return Err(Lz4Error::TruncatedInput);
                }
                let extra = data[pos] as usize;
                pos += 1;
                lit_len += extra;
                if extra != 255 {
                    break;
                }
            }
        }

        // Copy literals
        if lit_len > 0 {
            if pos + lit_len > data.len() {
                return Err(Lz4Error::TruncatedInput);
            }
            if output.len() + lit_len > decompressed_size {
                return Err(Lz4Error::LiteralOverflow);
            }
            output.extend_from_slice(&data[pos..pos + lit_len]);
            pos += lit_len;
        }

        // Check if we've reached the end (no match follows)
        if pos >= data.len() {
            break;
        }

        // Need at least 2 bytes for match offset
        if pos + 2 > data.len() {
            return Err(Lz4Error::TruncatedInput);
        }

        // Decode match offset (little-endian)
        let offset = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        if offset == 0 || offset > output.len() {
            return Err(Lz4Error::InvalidOffset);
        }

        // Decode match length from low nibble
        let mut match_len = (token & 0x0F) as usize + 4; // minimum match length is 4
        if match_len == 19 {
            // 15 + 4 = 19, meaning extended
            loop {
                if pos >= data.len() {
                    return Err(Lz4Error::TruncatedInput);
                }
                let extra = data[pos] as usize;
                pos += 1;
                match_len += extra;
                if extra != 255 {
                    break;
                }
            }
        }

        // Copy match (may overlap with output, so byte-by-byte copy)
        if output.len() + match_len > decompressed_size {
            return Err(Lz4Error::OutputOverflow);
        }
        let start = output.len() - offset;
        for i in 0..match_len {
            output.push(output[start + i]);
        }
    }

    if output.len() != decompressed_size {
        return Err(Lz4Error::InvalidData);
    }

    Ok(output)
}

/// Decompress an LZ4 block without requiring a known decompressed size.
///
/// Returns the decompressed data with capacity based on input size heuristic.
/// Useful when the output size is not known in advance (e.g., metadata blocks).
pub fn decompress_lz4_unbounded(data: &[u8]) -> Result<Vec<u8>, Lz4Error> {
    // Heuristic: output is typically 2-4x the compressed size for metadata
    let initial_cap = data.len().saturating_mul(4).max(8192);
    let mut output = Vec::with_capacity(initial_cap);
    let mut pos = 0usize;

    while pos < data.len() {
        let token = data[pos];
        pos += 1;

        let mut lit_len = (token >> 4) as usize;
        if lit_len == 15 {
            loop {
                if pos >= data.len() {
                    return Err(Lz4Error::TruncatedInput);
                }
                let extra = data[pos] as usize;
                pos += 1;
                lit_len += extra;
                if extra != 255 {
                    break;
                }
            }
        }

        if lit_len > 0 {
            if pos + lit_len > data.len() {
                return Err(Lz4Error::TruncatedInput);
            }
            output.extend_from_slice(&data[pos..pos + lit_len]);
            pos += lit_len;
        }

        if pos >= data.len() {
            break;
        }

        if pos + 2 > data.len() {
            return Err(Lz4Error::TruncatedInput);
        }

        let offset = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        if offset == 0 || offset > output.len() {
            return Err(Lz4Error::InvalidOffset);
        }

        let mut match_len = (token & 0x0F) as usize + 4;
        if match_len == 19 {
            loop {
                if pos >= data.len() {
                    return Err(Lz4Error::TruncatedInput);
                }
                let extra = data[pos] as usize;
                pos += 1;
                match_len += extra;
                if extra != 255 {
                    break;
                }
            }
        }

        let start = output.len() - offset;
        for i in 0..match_len {
            output.push(output[start + i]);
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decompress_empty() {
        // Empty input produces empty output only if decompressed_size is 0
        let result = decompress_lz4(&[], 0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_decompress_literals_only() {
        // Token 0x40 = 4 literals, no match
        // But we need to end without a match, so just literals
        // Actually LZ4 requires a match after literals unless it's the last sequence
        // Let's test with a simple case: "Hello" as literals
        // Token: 0x50 = 5 literals, then no match (end of data)
        let data = [0x50, b'H', b'e', b'l', b'l', b'o'];
        let result = decompress_lz4(&data, 5);
        assert!(result.is_ok());
        assert_eq!(&result.unwrap(), b"Hello");
    }

    #[test]
    fn test_decompress_with_match() {
        // "AAAA" = 1 literal 'A' + match of 3 bytes at offset 1
        // Token: 0x13 = 1 literal, match len 3+4=7... no that's wrong
        // Let's do "ABAB" = literals "AB" + match offset 2, len 2
        // But min match is 4, so we need at least 4 match bytes
        // "ABABABAB" = literals "AB" + match offset 2, len 6 (token match nibble = 2, so 2+4=6)
        // Token: high=2 (2 literals), low=2 (match len 2+4=6)
        let data: [u8; 5] = [0x22, b'A', b'B', 0x02, 0x00];
        // Wait, this gives 2 literals "AB" + match offset 2, len 6
        // But output only has 2 bytes, can't match 6 at offset 2
        // Let me do a simpler test: "AAAA" = 1 literal 'A' + match offset 1, len 3
        // Token: high=1, low=0-1 (match len 0+4=4)
        // That gives "A" + 4 bytes from offset 1 = "AAAAA" (5 bytes)
        // Token 0x10: 1 literal, match len 0+4=4
        let data: [u8; 4] = [0x10, b'A', 0x01, 0x00];
        // Hmm, that's only 4 bytes. Let me recount.
        // Actually: token(1) + literal(1) + offset(2) = 4 bytes, no extra match len bytes
        let data2: [u8; 4] = [0x10, b'A', 0x01, 0x00];
        let result = decompress_lz4(&data2, 5);
        assert!(result.is_ok());
        assert_eq!(&result.unwrap(), b"AAAAA");
    }
}
