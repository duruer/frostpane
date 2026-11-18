//! Seedable randomness. Every simulation takes `&mut impl Rng` (or owns one),
//! so tests are reproducible.

/// Source of 64-bit random values plus the helpers the specs use
/// (`Rand`, `Unit`, `Coin`, `Whole`, `Inclusive`, `Below`, `In`).
pub trait Rng {
    fn next_u64(&mut self) -> u64;

    /// Uniform in `[0, 1)` with 53 bits.
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[lo, hi)`.
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.unit() * (hi - lo)
    }

    /// 50/50.
    fn coin(&mut self) -> bool {
        self.unit() < 0.5
    }

    /// Integer in `[0, n)`; 0 when `n == 0`.
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { ((self.unit() * n as f64) as u64).min(n - 1) }
    }

    /// Integer in `[lo, hi)`; `lo` when the range is empty.
    fn whole(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo { lo } else { lo + self.below((hi - lo) as u64) as i64 }
    }

    /// Integer in `[lo, hi]`.
    fn inclusive(&mut self, lo: i64, hi: i64) -> i64 {
        self.whole(lo, hi.saturating_add(1))
    }

    /// Uniform element of a slice.
    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T>
    where
        Self: Sized,
    {
        if items.is_empty() { None } else { items.get(self.below(items.len() as u64) as usize) }
    }
}

/// 64-bit LCG (the tree generator from trees 2 `TreeRandom`, bit-exact):
/// `state = seed | 1`, `state = state * 6364136223846793005 + 1442695040888963407`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lcg64 {
    state: u64,
}

impl Lcg64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }
}

impl Rng for Lcg64 {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.state
    }
}

/// Seed derivation helper (SplitMix64 finaliser), for deriving per-object seeds.
pub fn mix_seed(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Vec<u64>);
    impl Rng for Fixed {
        fn next_u64(&mut self) -> u64 {
            self.0.remove(0)
        }
    }

    #[test]
    fn lcg_is_deterministic() {
        let mut a = Lcg64::new(42);
        let mut b = Lcg64::new(43); // 42|1 == 43
        assert_eq!(a.next_u64(), b.next_u64());
        let u = a.unit();
        assert!((0.0..1.0).contains(&u));
        assert_eq!(Lcg64::new(1).next_u64(), 6364136223846793005u64.wrapping_add(1442695040888963407));
    }

    #[test]
    fn helpers() {
        let mut r = Lcg64::new(7);
        for _ in 0..1000 {
            let x = r.range(2.0, 3.0);
            assert!((2.0..3.0).contains(&x));
            assert!(r.below(5) < 5);
            let w = r.whole(-2, 3);
            assert!((-2..3).contains(&w));
            let i = r.inclusive(1, 3);
            assert!((1..=3).contains(&i));
        }
        assert_eq!(r.below(0), 0);
        assert_eq!(r.whole(5, 5), 5);
        assert!(r.pick::<u8>(&[]).is_none());
        assert!(r.pick(&[1, 2, 3]).is_some());
        let mut f = Fixed(vec![0, u64::MAX]);
        assert!(f.coin());
        assert!(!f.coin());
        // the top value maps to n-1, never n
        let mut f = Fixed(vec![u64::MAX]);
        assert_eq!(f.below(3), 2);
        assert_ne!(mix_seed(1), mix_seed(2));
    }
}
