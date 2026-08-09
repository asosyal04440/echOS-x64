//! Platform-independent pseudo-random state machines.
//!
//! Hardware entropy, timestamps, CPU identity, and per-CPU storage belong to
//! the kernel adapter. Keeping this crate pure makes the algorithm testable on
//! every host without importing privileged or topology-dependent operations.

/// Non-zero seed used when a caller supplies XorShift32's absorbing zero state.
pub const DEFAULT_NONZERO_SEED: u32 = 0x9E37_79B9;

/// Maps XorShift32's absorbing zero state to a documented non-zero state.
#[inline]
pub const fn normalize_seed(seed: u32) -> u32 {
    if seed == 0 {
        DEFAULT_NONZERO_SEED
    } else {
        seed
    }
}

/// Advances a XorShift32 state by one step.
///
/// The caller must keep the state non-zero. Use [`normalize_seed`] when the
/// seed comes from an external source.
#[inline]
pub const fn xorshift32_step(mut state: u32) -> u32 {
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    state
}

/// Small, deterministic PRNG state for non-cryptographic kernel decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XorShift32 {
    state: u32,
}

impl XorShift32 {
    #[inline]
    pub const fn new(seed: u32) -> Self {
        Self {
            state: normalize_seed(seed),
        }
    }

    #[inline]
    pub const fn state(&self) -> u32 {
        self.state
    }

    #[inline]
    pub fn reseed(&mut self, seed: u32) {
        self.state = normalize_seed(seed);
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        self.state = xorshift32_step(self.state);
        self.state
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let low = self.next_u32() as u64;
        let high = self.next_u32() as u64;
        (high << 32) | low
    }

    pub fn fill_bytes(&mut self, destination: &mut [u8]) {
        let mut offset = 0;
        while offset < destination.len() {
            let word = self.next_u32().to_le_bytes();
            let count = core::cmp::min(word.len(), destination.len() - offset);
            destination[offset..offset + count].copy_from_slice(&word[..count]);
            offset += count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector_from_seed_one() {
        let mut rng = XorShift32::new(1);
        assert_eq!(rng.next_u32(), 0x0004_2021);
        assert_eq!(rng.next_u32(), 0x0408_0601);
        assert_eq!(rng.next_u32(), 0x9DCC_A8C5);
    }

    #[test]
    fn zero_seed_is_normalized_and_never_absorbs() {
        let mut rng = XorShift32::new(0);
        assert_eq!(rng.state(), DEFAULT_NONZERO_SEED);
        for _ in 0..1024 {
            assert_ne!(rng.next_u32(), 0);
        }
    }

    #[test]
    fn fill_bytes_matches_little_endian_word_stream() {
        let mut words = XorShift32::new(0x1234_5678);
        let first = words.next_u32().to_le_bytes();
        let second = words.next_u32().to_le_bytes();

        let mut bytes = XorShift32::new(0x1234_5678);
        let mut output = [0u8; 7];
        bytes.fill_bytes(&mut output);

        assert_eq!(&output[..4], &first);
        assert_eq!(&output[4..], &second[..3]);
    }

    #[test]
    fn reseed_restores_the_same_stream() {
        let mut rng = XorShift32::new(0xCAFE_BABE);
        let first = rng.next_u64();
        rng.reseed(0xCAFE_BABE);
        assert_eq!(rng.next_u64(), first);
    }
}
