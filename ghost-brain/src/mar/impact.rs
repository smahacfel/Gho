//! Impact Simulator for MAR.
//!
//! Calculates price impact based on AMM reserves to select appropriate X_effective.

use crate::mar::types::MarPoolReserves;

/// Calculates the price drop (fractional) caused by selling a specific amount of tokens.
///
/// Assumes a Constant Product Market Maker (x * y = k).
/// The price drop is defined as `(P_before - P_after) / P_before`.
///
/// Formula: `drop = 1.0 - (reserve_token / (reserve_token + tokens_sold))^2`
///
/// # Arguments
///
/// * `reserves` - Current pool reserves (must use virtual reserves for Pump.fun).
/// * `tokens_sold` - Amount of tokens to sell.
///
/// # Returns
///
/// Fractional price drop (0.0 to 1.0). Returns 0.0 if inputs are invalid.
pub fn calculate_price_drop(reserves: &MarPoolReserves, tokens_sold: u128) -> f64 {
    if reserves.reserve_token == 0 {
        return 0.0;
    }
    if tokens_sold == 0 {
        return 0.0;
    }

    let x = reserves.reserve_token;
    // Check for overflow in addition
    let x_prime = match x.checked_add(tokens_sold) {
        Some(val) => val,
        None => return 0.0, // Overflow, though unlikely with u128
    };

    // Cast to f64 for division. u128 fits in f64 but with precision loss for very large numbers.
    // However, we are calculating a ratio, so it should be fine.
    // Or we can do math in f64 directly.
    // For x ~ 10^15, f64 (53 bits significand ~ 15-17 decimal digits) is precise enough.

    let ratio = x as f64 / x_prime as f64;

    // Price drop = 1 - ratio^2
    1.0 - (ratio * ratio)
}

/// Selects the smallest X_effective (fraction of supply) that causes a price drop >= threshold.
///
/// # Arguments
///
/// * `reserves` - Current pool reserves.
/// * `total_supply` - Total supply of the token.
/// * `candidates` - List of candidate fractions to test (e.g., [0.10, 0.15, 0.20]).
/// * `impact_threshold` - Minimum price drop required (e.g., 0.60 for 60%).
///
/// # Returns
///
/// The chosen candidate fraction, or None if no candidate meets the threshold.
pub fn choose_x_effective(
    reserves: &MarPoolReserves,
    total_supply: u128,
    candidates: &[f64],
    impact_threshold: f64,
) -> Option<f64> {
    // Candidates should ideally be sorted, but we will iterate as provided.
    // If the caller wants the "smallest", they should provide a sorted list or we sort it?
    // The plan says "wybierz najmniejsze x", implies we should check in increasing order.
    // We assume candidates are passed in increasing order or we iterate through them and find min.
    // But typically candidates like [10%, 15%, 20%] are already sorted.
    // We will find the *first* one that satisfies, assuming sorted input.
    // If not sorted, we might return a larger one if it comes first.
    // To be safe, let's find the minimum satisfying candidate.

    let mut best_candidate: Option<f64> = None;

    for &candidate in candidates {
        if candidate <= 0.0 || candidate > 1.0 {
            continue;
        }

        let tokens_sold = (total_supply as f64 * candidate) as u128;
        let drop = calculate_price_drop(reserves, tokens_sold);

        if drop >= impact_threshold {
            match best_candidate {
                None => best_candidate = Some(candidate),
                Some(current_best) => {
                    if candidate < current_best {
                        best_candidate = Some(candidate);
                    }
                }
            }
        }
    }

    best_candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_price_drop_basic() {
        let reserves = MarPoolReserves {
            reserve_token: 1_000_000,
            reserve_sol: 100, // Doesn't matter for this calc
        };

        // Sell 1M tokens (double the supply in pool)
        // x = 1M
        // x' = 2M
        // ratio = 0.5
        // drop = 1 - 0.25 = 0.75
        let drop = calculate_price_drop(&reserves, 1_000_000);
        assert!((drop - 0.75).abs() < 1e-9);
    }

    #[test]
    fn test_calculate_price_drop_small() {
        let reserves = MarPoolReserves {
            reserve_token: 1_000_000,
            reserve_sol: 100,
        };

        // Sell 0
        let drop = calculate_price_drop(&reserves, 0);
        assert_eq!(drop, 0.0);
    }

    #[test]
    fn test_calculate_price_drop_monotonicity() {
        let reserves = MarPoolReserves {
            reserve_token: 1_000_000,
            reserve_sol: 100,
        };

        let drop1 = calculate_price_drop(&reserves, 100_000);
        let drop2 = calculate_price_drop(&reserves, 200_000);

        assert!(drop2 > drop1);
    }

    #[test]
    fn test_choose_x_effective_selection() {
        let reserves = MarPoolReserves {
            reserve_token: 1_000_000_000, // 1B in pool
            reserve_sol: 30_000_000_000,
        };
        let total_supply = 1_000_000_000; // 100% in pool (virtual)

        let candidates = vec![0.10, 0.20, 0.30, 0.40, 0.50];
        // Threshold 60% drop (0.60)

        // Let's calc drops manually approx:
        // 10%: x=1, x'=1.1 => ratio=0.909 => drop=1-0.826=0.17
        // 20%: x=1, x'=1.2 => ratio=0.833 => drop=1-0.694=0.30
        // 30%: x=1, x'=1.3 => ratio=0.769 => drop=1-0.591=0.40
        // 40%: x=1, x'=1.4 => ratio=0.714 => drop=1-0.510=0.49
        // 50%: x=1, x'=1.5 => ratio=0.666 => drop=1-0.444=0.55
        // Wait, for 60% drop, we need:
        // 1 - (1/(1+c))^2 >= 0.6
        // 0.4 >= (1/(1+c))^2
        // sqrt(0.4) >= 1/(1+c)
        // 0.632 >= 1/(1+c)
        // 1+c >= 1/0.632 = 1.58
        // c >= 0.58

        // So with these candidates, none should pass for 0.60.
        let result = choose_x_effective(&reserves, total_supply, &candidates, 0.60);
        assert_eq!(result, None);

        // Try threshold 0.40
        // 30% gives ~0.408 drop.
        let result = choose_x_effective(&reserves, total_supply, &candidates, 0.40);
        assert_eq!(result, Some(0.30));
    }

    #[test]
    fn test_choose_x_effective_min() {
        let reserves = MarPoolReserves {
            reserve_token: 1_000_000_000,
            reserve_sol: 30_000_000_000,
        };
        let total_supply = 1_000_000_000;

        // Unsorted candidates
        let candidates = vec![0.50, 0.30, 0.40];
        // Threshold 0.40.
        // 0.30 gives > 0.40 drop.
        // 0.40 gives > 0.40 drop.
        // 0.50 gives > 0.40 drop.
        // Should return 0.30.

        let result = choose_x_effective(&reserves, total_supply, &candidates, 0.40);
        assert_eq!(result, Some(0.30));
    }
}
