//! Perturbation Tester for MAR.
//!
//! Performs Monte Carlo simulations to test the stability of K(t)
//! under random perturbations of top holder balances.

use crate::mar::execution_cost::calculate_k;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use solana_sdk::pubkey::Pubkey;

/// Performs perturbation tests on top holders to check robustness of K(t).
pub struct PerturbationTester {
    rng: StdRng,
    iterations: usize,
}

impl PerturbationTester {
    /// Creates a new tester.
    ///
    /// # Arguments
    /// * `iterations` - Number of Monte Carlo iterations (e.g., 20-50).
    /// * `seed` - Optional seed for deterministic behavior (e.g. for tests).
    pub fn new(iterations: usize, seed: Option<u64>) -> Self {
        let rng = match seed {
            Some(s) => StdRng::seed_from_u64(s),
            None => StdRng::from_entropy(),
        };
        Self { rng, iterations }
    }

    /// Runs the perturbation test.
    ///
    /// 1. Samples 2 distinct holders weighted by balance.
    /// 2. Reduces their balance by 30%.
    /// 3. Recalculates K'.
    /// 4. Returns the median of K' values over `iterations`.
    ///
    /// Returns `None` if there are fewer than 2 holders with positive balance.
    pub fn run_test(
        &mut self,
        holders: &[(Pubkey, u64)],
        total_supply: u64,
        x_effective: f64,
    ) -> Option<f64> {
        let positive_balance_count = holders.iter().filter(|(_, b)| *b > 0).count();
        if positive_balance_count < 2 {
            return None;
        }

        let total_balance: u64 = holders.iter().map(|(_, b)| *b).sum();
        if total_balance == 0 {
            return None;
        }

        let target_amount = (total_supply as f64 * x_effective) as u64;
        let mut k_primes = Vec::with_capacity(self.iterations);

        for _ in 0..self.iterations {
            // 1. Pick first holder
            let idx1 = self.weighted_sample(holders, total_balance, None);

            // 2. Pick second holder (excluding first)
            let bal1 = holders[idx1].1;
            let idx2 = self.weighted_sample(holders, total_balance - bal1, Some(idx1));

            // 3. Clone and perturb
            let mut balances: Vec<u64> = holders.iter().map(|(_, b)| *b).collect();

            // Reduce by 30% -> multiply by 0.7
            balances[idx1] = (balances[idx1] as f64 * 0.7) as u64;
            balances[idx2] = (balances[idx2] as f64 * 0.7) as u64;

            // 4. Sort descending
            balances.sort_unstable_by(|a, b| b.cmp(a));

            // 5. Calculate K'
            let k_prime = calculate_k(&balances, target_amount);
            k_primes.push(k_prime);
        }

        // Median
        k_primes.sort_unstable();
        let len = k_primes.len();
        if len == 0 {
            return None;
        }

        if len % 2 == 1 {
            Some(k_primes[len / 2] as f64)
        } else {
            let mid1 = k_primes[len / 2 - 1];
            let mid2 = k_primes[len / 2];
            Some((mid1 + mid2) as f64 / 2.0)
        }
    }

    /// Weighted sample from holders.
    ///
    /// If `exclude_idx` is provided, skips that index and assumes `total_weight` excludes it too.
    fn weighted_sample(
        &mut self,
        holders: &[(Pubkey, u64)],
        total_weight: u64,
        exclude_idx: Option<usize>,
    ) -> usize {
        if total_weight == 0 {
            // Fallback to first non-excluded with positive balance
            for (i, (_, b)) in holders.iter().enumerate() {
                if Some(i) != exclude_idx && *b > 0 {
                    return i;
                }
            }
            // Fallback to any non-excluded (even 0 balance if forced)
            for i in 0..holders.len() {
                if Some(i) != exclude_idx {
                    return i;
                }
            }
            return 0;
        }

        let r = self.rng.gen_range(0..total_weight);
        let mut accumulated = 0;

        for (i, (_, balance)) in holders.iter().enumerate() {
            if Some(i) == exclude_idx {
                continue;
            }

            accumulated += balance;
            if accumulated > r {
                return i;
            }
        }

        // Fallback for rounding errors
        for (i, _) in holders.iter().enumerate().rev() {
            if Some(i) != exclude_idx {
                return i;
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perturbation_deterministic() {
        let mut tester = PerturbationTester::new(20, Some(12345));

        let holders = vec![
            (Pubkey::new_unique(), 1000),
            (Pubkey::new_unique(), 500),
            (Pubkey::new_unique(), 500),
        ];
        let total = 2000;
        let x = 0.5; // Target 1000.

        let result1 = tester.run_test(&holders, total, x).unwrap();

        let mut tester2 = PerturbationTester::new(20, Some(12345));
        let result2 = tester2.run_test(&holders, total, x).unwrap();

        assert_eq!(result1, result2);
    }

    #[test]
    fn test_perturbation_insufficient_holders() {
        let mut tester = PerturbationTester::new(20, Some(1));
        let holders = vec![(Pubkey::new_unique(), 1000)];
        let result = tester.run_test(&holders, 1000, 0.5);
        assert_eq!(result, None);
    }

    #[test]
    fn test_perturbation_impact() {
        // Setup a case where reducing top holder increases K.
        // Total 100. Target 60.
        // Holders: 70, 10, 10, 10.
        // K = 1 (70 >= 60).

        // Perturb: Reduce 70 by 30% -> 49.
        // Sorted: 49, 10, 10, 10.
        // Target 60.
        // 49 < 60.
        // 49+10 = 59 < 60.
        // 49+10+10 = 69 >= 60.
        // K' = 3.

        // Since 70 is dominant, it will be picked almost always.
        // So median should be close to 3 (or at least > 1).

        let mut tester = PerturbationTester::new(50, Some(42));
        let holders = vec![
            (Pubkey::new_unique(), 70),
            (Pubkey::new_unique(), 10),
            (Pubkey::new_unique(), 10),
            (Pubkey::new_unique(), 10),
        ];

        let result = tester.run_test(&holders, 100, 0.60).unwrap();
        assert!(result > 1.0, "K should increase from 1");
    }
}
