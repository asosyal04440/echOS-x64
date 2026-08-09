//! DEFLATE decompression per RFC 1951
//! Supports: stored blocks, fixed Huffman, dynamic Huffman

use alloc::vec;
use alloc::vec::Vec;

pub fn decompress_deflate(input: &[u8], max_output: usize) -> Result<Vec<u8>, &'static str> {
    let mut reader = BitReader::new(input);
    let mut output = Vec::with_capacity(max_output);
    let mut is_final = false;

    while !is_final {
        if reader.bits_remaining() == 0 {
            break;
        }
        is_final = reader.read_bit()?;
        let btype = reader.read_bits(2)?;

        match btype {
            0 => decompress_stored_block(&mut reader, &mut output, max_output)?,
            1 => decompress_huffman_block(&mut reader, &mut output, max_output, true)?,
            2 => decompress_huffman_block(&mut reader, &mut output, max_output, false)?,
            3 => return Err("deflate: reserved block type"),
            _ => unreachable!(),
        }
    }

    Ok(output)
}

// --- Bit reader ---

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, byte_pos: 0, bit_pos: 0 }
    }

    fn read_bit(&mut self) -> Result<bool, &'static str> {
        if self.byte_pos >= self.data.len() {
            return Err("deflate: unexpected end of input");
        }
        let bit = (self.data[self.byte_pos] >> self.bit_pos) & 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Ok(bit != 0)
    }

    fn read_bits(&mut self, n: u8) -> Result<u16, &'static str> {
        let mut result = 0u16;
        for i in 0..n {
            if self.read_bit()? {
                result |= 1u16 << i;
            }
        }
        Ok(result)
    }

    fn read_bits_msb(&mut self, n: u8) -> Result<u16, &'static str> {
        let mut result = 0u16;
        for _ in 0..n {
            result = (result << 1) | if self.read_bit()? { 1 } else { 0 };
        }
        Ok(result)
    }

    fn align_byte(&mut self) {
        self.bit_pos = 0;
        self.byte_pos += 1;
    }

    fn bits_remaining(&self) -> usize {
        self.data.len().saturating_sub(self.byte_pos) * 8
    }
}

// --- Stored block (BTYPE=00) ---

fn decompress_stored_block(
    reader: &mut BitReader<'_>,
    output: &mut Vec<u8>,
    max_output: usize,
) -> Result<(), &'static str> {
    reader.align_byte();
    if reader.byte_pos + 4 > reader.data.len() {
        return Err("deflate: truncated stored block");
    }
    let len = u16::from_le_bytes([reader.data[reader.byte_pos], reader.data[reader.byte_pos + 1]]) as usize;
    let nlen = u16::from_le_bytes([reader.data[reader.byte_pos + 2], reader.data[reader.byte_pos + 3]]) as usize;
    reader.byte_pos += 4;

    if len != (!nlen & 0xFFFF) {
        return Err("deflate: stored block length mismatch");
    }
    if reader.byte_pos + len > reader.data.len() {
        return Err("deflate: stored block exceeds input");
    }
    if output.len() + len > max_output {
        return Err("deflate: output exceeds max");
    }

    output.extend_from_slice(&reader.data[reader.byte_pos..reader.byte_pos + len]);
    reader.byte_pos += len;
    Ok(())
}

// --- Huffman block (BTYPE=01 or 10) ---

const FIXED_LIT_BITS: [u8; 288] = {
    let mut arr = [0u8; 288];
    let mut i = 0;
    while i < 144 { arr[i] = 8; i += 1; }
    while i < 256 { arr[i] = 9; i += 1; }
    while i < 280 { arr[i] = 7; i += 1; }
    while i < 288 { arr[i] = 8; i += 1; }
    arr
};

const FIXED_DIST_BITS: [u8; 32] = {
    let mut arr = [0u8; 32];
    let mut i = 0;
    while i < 32 { arr[i] = 5; i += 1; }
    arr
};

// Length extra bits table (RFC 1951 section 3.2.5)
const LENGTH_EXTRA: [(u8, u16); 29] = [
    (0, 3), (0, 4), (0, 5), (0, 6), (0, 7), (0, 8), (0, 9), (0, 10),
    (1, 11), (1, 13), (2, 15), (2, 19), (3, 23), (3, 27), (4, 31), (4, 35),
    (5, 43), (5, 51), (6, 59), (6, 67), (7, 83), (7, 99), (8, 115), (8, 131),
    (9, 163), (9, 195), (10, 227), (10, 258), (0, 258), // 285 = 258, no extra
];

// Distance extra bits table
const DIST_EXTRA: [(u8, u16); 30] = [
    (0, 1), (0, 2), (0, 3), (0, 4), (1, 5), (1, 7), (2, 9), (2, 13),
    (3, 17), (3, 25), (4, 33), (4, 49), (5, 65), (5, 97), (6, 129), (6, 193),
    (7, 257), (7, 385), (8, 513), (8, 769), (9, 1025), (9, 1537), (10, 2049), (10, 3073),
    (11, 4097), (11, 6145), (12, 8193), (12, 12289), (13, 16385), (13, 24577),
];

fn decompress_huffman_block(
    reader: &mut BitReader<'_>,
    output: &mut Vec<u8>,
    max_output: usize,
    fixed: bool,
) -> Result<(), &'static str> {
    let (lit_tree, dist_tree) = if fixed {
        let lit_codes = build_codes_from_lengths(&FIXED_LIT_BITS);
        let dist_codes = build_codes_from_lengths(&FIXED_DIST_BITS);
        (lit_codes, dist_codes)
    } else {
        build_dynamic_trees(reader)?
    };

    loop {
        let sym = read_huffman_symbol(reader, &lit_tree)?;
        if sym < 256 {
            if output.len() >= max_output {
                return Err("deflate: output exceeds max");
            }
            output.push(sym as u8);
        } else if sym == 256 {
            break; // end of block
        } else {
            let length_code = sym - 257;
            if length_code > 28 {
                return Err("deflate: invalid length code");
            }
            let (extra_bits, base_len) = LENGTH_EXTRA[length_code as usize];
            let extra = reader.read_bits(extra_bits)? as u16;
            let length = base_len + extra;

            let dist_code = read_huffman_symbol(reader, &dist_tree)?;
            if dist_code > 29 {
                return Err("deflate: invalid distance code");
            }
            let (dist_extra_bits, dist_base) = DIST_EXTRA[dist_code as usize];
            let dist_extra = reader.read_bits(dist_extra_bits)? as usize;
            let distance = dist_base as usize + dist_extra;

            if distance == 0 || distance > output.len() {
                return Err("deflate: invalid distance");
            }
            if output.len() + length as usize > max_output {
                return Err("deflate: output exceeds max");
            }

            // LZ77 copy (handles overlapping copies correctly)
            let start = output.len() - distance;
            for _ in 0..length {
                let byte = output[start + (output.len() - (output.len() - distance)) % distance];
                // Simpler: copy byte by byte from the sliding window
                let src = output.len() - distance;
                let b = output[src];
                output.push(b);
            }
        }
    }

    Ok(())
}

// --- Huffman tree structures ---

#[derive(Clone)]
struct HuffmanCode {
    code: u16,
    len: u8,
    symbol: u16,
}

struct HuffmanTree {
    codes: Vec<HuffmanCode>,
    max_len: u8,
}

fn build_codes_from_lengths(lengths: &[u8]) -> Vec<HuffmanCode> {
    let max_sym = lengths.len();
    let mut bl_count = [0u16; 16];
    for &len in lengths.iter() {
        if len > 0 {
            bl_count[len as usize] += 1;
        }
    }

    let mut next_code = [0u16; 16];
    let mut code = 0u16;
    for bits in 1..16 {
        code = (code + bl_count[bits - 1]) << 1;
        next_code[bits] = code;
    }

    let mut codes = Vec::with_capacity(max_sym);
    for (i, &len) in lengths.iter().enumerate() {
        if len > 0 {
            let c = next_code[len as usize];
            next_code[len as usize] += 1;
            codes.push(HuffmanCode { code: c, len, symbol: i as u16 });
        }
    }
    codes
}

fn build_dynamic_trees(
    reader: &mut BitReader<'_>,
) -> Result<(Vec<HuffmanCode>, Vec<HuffmanCode>), &'static str> {
    let hlit = reader.read_bits(5)? as usize + 257;
    let hdist = reader.read_bits(5)? as usize + 1;
    let hclen = reader.read_bits(4)? as usize + 4;

    // Code length alphabet order
    const CL_ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];

    let mut cl_lengths = [0u8; 19];
    for i in 0..hclen {
        cl_lengths[CL_ORDER[i]] = reader.read_bits(3)? as u8;
    }

    let cl_codes = build_codes_from_lengths(&cl_lengths);

    // Read code lengths for literal/length and distance alphabets
    let total = hlit + hdist;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        let sym = read_huffman_symbol(reader, &cl_codes)?;
        if sym <= 15 {
            lengths.push(sym as u8);
        } else if sym == 16 {
            let prev = *lengths.last().ok_or("deflate: code 16 without prev")?;
            let count = reader.read_bits(2)? as usize + 3;
            for _ in 0..count {
                lengths.push(prev);
                if lengths.len() >= total { break; }
            }
        } else if sym == 17 {
            let count = reader.read_bits(3)? as usize + 3;
            for _ in 0..count {
                lengths.push(0);
                if lengths.len() >= total { break; }
            }
        } else if sym == 18 {
            let count = reader.read_bits(7)? as usize + 11;
            for _ in 0..count {
                lengths.push(0);
                if lengths.len() >= total { break; }
            }
        } else {
            return Err("deflate: invalid code length symbol");
        }
    }

    let lit_lengths = &lengths[..hlit];
    let dist_lengths = &lengths[hlit..];

    let lit_codes = build_codes_from_lengths(lit_lengths);
    let dist_codes = build_codes_from_lengths(dist_lengths);

    Ok((lit_codes, dist_codes))
}

fn read_huffman_symbol(reader: &mut BitReader<'_>, codes: &[HuffmanCode]) -> Result<u16, &'static str> {
    let mut bits = 0u16;
    let mut bit_count = 0u8;

    // Read up to max_len bits, checking for match at each step
    let max_len = codes.iter().map(|c| c.len).max().unwrap_or(16);
    for _ in 0..max_len {
        if reader.read_bit()? {
            bits |= 1u16 << bit_count;
        }
        bit_count += 1;

        // Check if any code matches (MSB-first comparison)
        // Codes are stored as read LSB-first, so we need to reverse
        let reversed = reverse_bits(bits, bit_count);
        if let Some(code) = codes.iter().find(|c| c.len == bit_count && c.code == reversed) {
            return Ok(code.symbol);
        }
    }

    Err("deflate: invalid huffman code")
}

fn reverse_bits(val: u16, n: u8) -> u16 {
    let mut result = 0u16;
    for i in 0..n {
        if (val & (1 << i)) != 0 {
            result |= 1 << (n - 1 - i);
        }
    }
    result
}
