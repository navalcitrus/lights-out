//! Helpers shared by the integration tests and the unit tests.

/// Tiny LCG so we don't pull in a `rand` dependency.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xDEADBEEFCAFEBABE)
    }

    /// Uniform-ish value in `[0, m)`.
    pub fn below(&mut self, m: u8) -> u8 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) % m as u64) as u8
    }

    pub fn vec(&mut self, len: usize, m: u8) -> Vec<u8> {
        (0..len).map(|_| self.below(m)).collect()
    }
}
