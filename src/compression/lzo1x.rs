//! LZO1X decompression per LZO specification
//! Used by btrfs for compression

use alloc::vec;
use alloc::vec::Vec;

pub fn decompress_lzo1x(input: &[u8], max_output: usize) -> Result<Vec<u8>, &'static str> {
    let mut output = Vec::with_capacity(max_output);
    let mut ip = 0usize; // input position
    let mut first = true;

    if input.is_empty() {
        return Err("lzo1x: empty input");
    }

    // Skip optional LZO header (4 bytes: 0x89 0x4C 0x5A 0x4F = ".LZO")
    if input.len() >= 4 && input[0] == 0x89 && input[1] == 0x4C && input[2] == 0x5A && input[3] == 0x4F {
        ip = 4;
    }

    // First literal run
    if ip >= input.len() {
        return Err("lzo1x: truncated input");
    }
    let mut t = input[ip];
    ip += 1;

    if t > 17 {
        let mut lit_len = (t - 17) as usize;
        if lit_len < 4 {
            // This shouldn't happen in valid LZO1X, but handle gracefully
        }
        if ip + lit_len > input.len() {
            return Err("lzo1x: literal overrun");
        }
        if output.len() + lit_len > max_output {
            return Err("lzo1x: output exceeds max");
        }
        output.extend_from_slice(&input[ip..ip + lit_len]);
        ip += lit_len;
        if ip >= input.len() {
            return Ok(output);
        }
        t = input[ip];
        ip += 1;
    }

    loop {
        if ip >= input.len() {
            return Err("lzo1x: unexpected end of input");
        }
        t = input[ip];
        ip += 1;

        if t >= 16 {
            // Match
            let mut m_pos = output.len().wrapping_sub(1);
            let mut m_len: usize;

            if t < 64 {
                // Short match: 2-3 byte offset
                let offset = ((t >> 2) & 7) as usize + 1;
                m_len = (t & 3) as usize + 2;
                m_pos = m_pos.wrapping_sub(offset);
            } else if t < 128 {
                // 2-byte offset match
                if ip >= input.len() {
                    return Err("lzo1x: truncated match");
                }
                let b = input[ip] as usize;
                ip += 1;
                let offset = ((t as usize & 7) << 8) | b;
                m_len = (t as usize >> 5) + 2;
                m_pos = m_pos.wrapping_sub(offset + 1);
            } else {
                // Variable length match (t >= 128)
                if ip >= input.len() {
                    return Err("lzo1x: truncated match");
                }
                let b = input[ip];
                ip += 1;

                if b >= 64 {
                    let offset = ((t as usize & 3) << 8) | b as usize;
                    m_len = ((t as usize >> 2) & 7) + 2;
                    m_pos = m_pos.wrapping_sub(offset + 1);
                } else {
                    // Long match
                    if ip >= input.len() {
                        return Err("lzo1x: truncated long match");
                    }
                    let b2 = input[ip];
                    ip += 1;
                    let offset = ((t as usize & 3) << 8 | b as usize) << 8 | b2 as usize;
                    m_len = 9;
                    m_pos = m_pos.wrapping_sub(offset + 1);
                }
            }

            if m_pos >= output.len() {
                return Err("lzo1x: match position out of range");
            }

            // Additional length bytes
            loop {
                if ip >= input.len() {
                    break;
                }
                let b = input[ip];
                ip += 1;
                m_len += b as usize;
                if b != 255 {
                    break;
                }
            }

            if output.len() + m_len > max_output {
                return Err("lzo1x: output exceeds max");
            }

            // Copy match (handles overlap)
            let src = m_pos;
            for _ in 0..m_len {
                let byte = output[src + (output.len() - m_pos)];
                output.push(byte);
            }
        } else if t <= 15 {
            // Literal run of t+1 bytes
            if t == 0 {
                // Count additional literal bytes
                let mut extra = 0usize;
                loop {
                    if ip >= input.len() {
                        return Err("lzo1x: truncated literal");
                    }
                    let b = input[ip];
                    ip += 1;
                    extra += b as usize;
                    if b != 255 {
                        break;
                    }
                }
                let lit_len = 1 + extra;
                if ip + lit_len > input.len() {
                    return Err("lzo1x: literal overrun");
                }
                if output.len() + lit_len > max_output {
                    return Err("lzo1x: output exceeds max");
                }
                output.extend_from_slice(&input[ip..ip + lit_len]);
                ip += lit_len;
            } else {
                let lit_len = t as usize + 1;
                if ip + lit_len > input.len() {
                    return Err("lzo1x: literal overrun");
                }
                if output.len() + lit_len > max_output {
                    return Err("lzo1x: output exceeds max");
                }
                output.extend_from_slice(&input[ip..ip + lit_len]);
                ip += lit_len;
            }
        } else {
            // t == 17: end of stream marker (in some variants)
            if t == 17 {
                if first {
                    // Just a zero-length literal
                    first = false;
                    continue;
                }
            }
        }
        first = false;
    }
}
