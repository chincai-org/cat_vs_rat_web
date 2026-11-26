/// Small seedable PRNG (xorshift64*), so tests are reproducible and wasm needs no extra crates.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // splitmix64 to spread the seed bits
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Rng {
            state: if z == 0 { 0x9E37_79B9_7F4A_7C15 } else { z },
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    /// Uniform in 0..n. `n` must be greater than 0.
    pub fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0, "below() needs a non-empty range");
        ((self.next_u32() as u64 * n as u64) >> 32) as usize
    }

    /// Uniform integer in lo..=hi.
    pub fn range_inclusive(&mut self, lo: u32, hi: u32) -> u32 {
        debug_assert!(lo <= hi, "range_inclusive() needs lo <= hi");
        lo + self.below((hi - lo + 1) as usize) as u32
    }

    pub fn coin(&mut self) -> bool {
        self.next_u32() & 0x8000_0000 != 0
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..20 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn below_and_range_stay_in_bounds() {
        let mut r = Rng::new(7);
        for _ in 0..1000 {
            assert!(r.below(5) < 5);
            let v = r.range_inclusive(3, 7);
            assert!((3..=7).contains(&v));
        }
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut r = Rng::new(1);
        let mut v: Vec<u32> = (0..10).collect();
        r.shuffle(&mut v);
        v.sort();
        assert_eq!(v, (0..10).collect::<Vec<u32>>());
    }
}
