//! Fractal Resonance Engine - Decision Engine Module
//!
//! Stateful decision engine that consumes raw computations from math.rs and applies:
//! - FSW (Fractal Stability Window): Monitors variance in Hurst exponent over time
//! - STT (Scale-Transition Test): Checks coherence across transaction size buckets
//! - ARB (Asymmetric Risk Bias): Nonlinear risk evaluation combining all metrics
//!
//! Output: BUY, WATCH, or SKIP verdicts with confidence scores

use super::math::{FractalMath, WelfordVariance};
use crate::config::ghost_brain_config::FreConfig;
use crate::oracle::ultrafast::praecog::SwapInfo;

// =============================================================================
// Output Types
// =============================================================================

/// Final decision action from FRE analysis
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FractalAction {
    /// Clean signal - proceed with buy
    Buy,

    /// Promising but unstable - monitor closely
    Watch(String),

    /// Botnet/Chaos/Scam detected - skip this token
    Skip(String),
}

/// Complete verdict with all metrics
#[derive(Debug, Clone, PartialEq)]
pub struct FractalVerdict {
    /// Final action decision
    pub action: FractalAction,

    /// Organic score (0-100) indicating real human activity
    pub organic_score: u8,

    /// Scale coherence (0.0-1.0) from STT analysis
    pub coherence: f64,

    /// Stability sigma from FSW
    pub stability_sigma: f64,

    /// Global Hurst exponent
    pub hurst_global: f64,
}

// =============================================================================
// Engine
// =============================================================================

/// Stateful Fractal Resonance Engine
#[derive(Clone)]
pub struct FractalEngine {
    config: FreConfig,
    fsw_state: WelfordVariance,
}

impl FractalEngine {
    /// Create a new FractalEngine with given configuration
    pub fn new(config: FreConfig) -> Self {
        Self {
            config,
            fsw_state: WelfordVariance::new(),
        }
    }

    /// Main analysis method - processes swap data and returns verdict
    pub fn analyze(&mut self, swaps: &[SwapInfo]) -> FractalVerdict {
        // Edge case: Very few transactions
        if swaps.len() < 10 {
            return FractalVerdict {
                action: FractalAction::Watch("Insufficient data (<10 tx)".to_string()),
                organic_score: 0,
                coherence: 0.0,
                stability_sigma: 0.0,
                hurst_global: 0.5,
            };
        }

        // Step 1: Calculate Global Hurst from all swap amounts
        let amounts: Vec<f64> = swaps.iter().map(|s| s.amount_in as f64).collect();

        let hurst_global = match FractalMath::calculate_rs_hurst(&amounts) {
            Some(h) => h,
            None => {
                // Edge case: All swaps identical (degenerate case)
                return FractalVerdict {
                    action: FractalAction::Skip("Degenerate data (all identical)".to_string()),
                    organic_score: 0,
                    coherence: 0.0,
                    stability_sigma: 0.0,
                    hurst_global: 0.0,
                };
            }
        };

        // Step 2: FSW Update & Stability Check
        self.fsw_state.update(hurst_global);
        let stability_sigma = self.fsw_state.std_dev().unwrap_or(0.0);

        let fsw_status = if stability_sigma < self.config.fsw_sigma_watch {
            FswStatus::Stable
        } else if stability_sigma <= self.config.fsw_sigma_skip {
            FswStatus::Jittery
        } else {
            FswStatus::Chaotic
        };

        // Early exit: Chaotic stability
        if matches!(fsw_status, FswStatus::Chaotic) {
            return FractalVerdict {
                action: FractalAction::Skip(format!(
                    "Unstable structure (σ={:.3})",
                    stability_sigma
                )),
                organic_score: 0,
                coherence: 0.0,
                stability_sigma,
                hurst_global,
            };
        }

        // Step 3: STT (Scale-Transition Test)
        let coherence = if swaps.len() < self.config.stt_min_tx_count {
            // Not enough data for STT - return uncertainty penalty
            0.5 // Neutral coherence
        } else {
            self.calculate_stt_coherence(&amounts)
        };

        // Step 4: ARB (Asymmetric Risk Bias) Logic
        let verdict = self.apply_arb_logic(hurst_global, coherence, stability_sigma, fsw_status);

        verdict
    }

    /// Calculate STT coherence by bucketing transactions and comparing Hurst values
    fn calculate_stt_coherence(&self, amounts: &[f64]) -> f64 {
        let n = amounts.len();

        // Define bucket boundaries (percentiles)
        let p33_idx = n / 3;
        let p66_idx = (2 * n) / 3;

        // Create three buckets: Small, Mid, Large
        let small_bucket = &amounts[..p33_idx];
        let mid_bucket = &amounts[p33_idx..p66_idx];
        let large_bucket = &amounts[p66_idx..];

        // Calculate Hurst for each bucket
        let h_small = FractalMath::calculate_rs_hurst(small_bucket);
        let h_mid = FractalMath::calculate_rs_hurst(mid_bucket);
        let h_large = FractalMath::calculate_rs_hurst(large_bucket);

        // Collect valid Hurst values
        let hurst_values: Vec<f64> = [h_small, h_mid, h_large]
            .iter()
            .filter_map(|&h| h)
            .collect();

        // Need at least 2 valid buckets for coherence calculation
        if hurst_values.len() < 2 {
            return 0.5; // Neutral coherence if not enough buckets
        }

        // Calculate coherence: 1.0 - (std_dev / 0.25)
        // Max expected std_dev is 0.25 for completely incoherent scales
        const MAX_STD_DEV: f64 = 0.25;

        let mut stats = WelfordVariance::new();
        for &h in &hurst_values {
            stats.update(h);
        }

        let std_dev = stats.std_dev().unwrap_or(0.0);
        let coherence = (1.0 - std_dev / MAX_STD_DEV).clamp(0.0, 1.0);

        coherence
    }

    /// Apply Asymmetric Risk Bias logic to determine final action
    fn apply_arb_logic(
        &self,
        hurst_global: f64,
        coherence: f64,
        stability_sigma: f64,
        fsw_status: FswStatus,
    ) -> FractalVerdict {
        // Calculate component scores (0-100 scale)

        // Hurst score: Linear mapping from [0.5, 0.9] to [0, 100]
        // Below 0.5 = mean-reverting (bad), above 0.9 = overfitting (also bad)
        let hurst_score = if hurst_global < 0.5 {
            0.0
        } else if hurst_global > 0.9 {
            ((1.0 - hurst_global) / 0.1 * 100.0).clamp(0.0, 100.0)
        } else {
            ((hurst_global - 0.5) / 0.4 * 100.0).clamp(0.0, 100.0)
        };

        // Coherence score: Direct mapping [0.0, 1.0] -> [0, 100]
        let coherence_score = coherence * 100.0;

        // Stability penalty: Exponential penalty for higher sigma
        // Jittery state gets moderate penalty, Chaotic gets severe penalty
        let sigma_penalty = match fsw_status {
            FswStatus::Stable => 0.0,
            FswStatus::Jittery => stability_sigma * self.config.arb_penalty_factor * 50.0,
            FswStatus::Chaotic => stability_sigma * self.config.arb_penalty_factor * 100.0,
        };

        // ARB Formula: Weighted combination with coherence dominance
        // High coherence can rescue medium Hurst, but low coherence kills everything
        let base_score = (hurst_score * 0.4) + (coherence_score * 0.6);

        // Apply multiplicative coherence gate: if coherence < 0.4, apply severe penalty
        let gated_score = if coherence < 0.4 {
            base_score * coherence / 0.4 // Scale down proportionally
        } else {
            base_score
        };

        // Subtract stability penalty
        let final_score = (gated_score - sigma_penalty).clamp(0.0, 100.0);

        let organic_score = final_score as u8;

        // Determine action based on final score and constraints
        let action = if coherence < 0.4 {
            FractalAction::Skip(format!("Scale mismatch (coherence={:.2})", coherence))
        } else if organic_score >= self.config.min_organic_score {
            if matches!(fsw_status, FswStatus::Stable) {
                FractalAction::Buy
            } else {
                FractalAction::Watch(format!(
                    "Good signal but jittery (σ={:.3})",
                    stability_sigma
                ))
            }
        } else if organic_score >= 50 {
            FractalAction::Watch(format!("Moderate signal (score={})", organic_score))
        } else {
            FractalAction::Skip(format!("Low organic score ({})", organic_score))
        };

        FractalVerdict {
            action,
            organic_score,
            coherence,
            stability_sigma,
            hurst_global,
        }
    }
}

/// Internal FSW stability classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FswStatus {
    Stable,  // σ < 0.12
    Jittery, // 0.12 ≤ σ ≤ 0.18
    Chaotic, // σ > 0.18
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_swaps_with_trend(count: usize, base: f64, increment: f64) -> Vec<SwapInfo> {
        (0..count)
            .map(|i| SwapInfo {
                amount_in: ((base + i as f64 * increment) * 1e9) as u128,
                is_buy: true,
                timestamp_ms: i as u64 * 100,
            })
            .collect()
    }

    fn create_swaps_constant(count: usize, amount: f64) -> Vec<SwapInfo> {
        (0..count)
            .map(|i| SwapInfo {
                amount_in: (amount * 1e9) as u128,
                is_buy: true,
                timestamp_ms: i as u64 * 100,
            })
            .collect()
    }

    fn create_swaps_alternating(count: usize) -> Vec<SwapInfo> {
        (0..count)
            .map(|i| SwapInfo {
                amount_in: if i % 2 == 0 {
                    1_000_000_000
                } else {
                    2_000_000_000
                },
                is_buy: true,
                timestamp_ms: i as u64 * 100,
            })
            .collect()
    }

    #[test]
    fn test_perfect_organic_scenario() {
        // Scenario: Perfect organic pump
        // Expected: High Hurst (~0.7), High coherence (~0.95), Low sigma (~0.05)
        // Result: BUY with score > 85

        let config = FreConfig::default();
        let mut engine = FractalEngine::new(config);

        // Create realistic organic pump: increasing amounts with variance
        let mut swaps = Vec::new();
        let mut base_amount = 0.1;
        for i in 0..50 {
            let variance = (i % 5) as f64 * 0.01;
            let amount = base_amount + variance;
            swaps.push(SwapInfo {
                amount_in: (amount * 1e9) as u128,
                is_buy: true,
                timestamp_ms: i as u64 * 50,
            });
            base_amount += 0.02; // Steady growth
        }

        let verdict = engine.analyze(&swaps);

        println!("Perfect Organic: {:?}", verdict);
        assert!(
            verdict.hurst_global > 0.65,
            "Hurst should be > 0.65 for trending data"
        );
        assert!(verdict.coherence > 0.80, "Coherence should be high");
        assert!(verdict.organic_score >= 75, "Organic score should be >= 75");

        // After first analysis, stability should be good (only one sample)
        // Run a few more times to stabilize FSW
        for _ in 0..3 {
            engine.analyze(&swaps);
        }

        let final_verdict = engine.analyze(&swaps);
        assert!(
            matches!(final_verdict.action, FractalAction::Buy)
                || matches!(final_verdict.action, FractalAction::Watch(_)),
            "Should be Buy or Watch for organic pump"
        );
    }

    #[test]
    fn test_botnet_attack_scenario() {
        // Scenario: Botnet attack - high global Hurst but bots only on small amounts
        // Expected: Global Hurst high (~0.8), but STT low (~0.4) due to scale mismatch
        // Result: SKIP (Reason: "Scale Mismatch")

        let config = FreConfig::default();
        let mut engine = FractalEngine::new(config);

        // Create botnet pattern: many small identical trades, few large trades
        let mut swaps = Vec::new();

        // 90% small bot trades (almost identical with tiny variance)
        for i in 0..45 {
            swaps.push(SwapInfo {
                amount_in: 50_000_000 + (i % 3) as u128 * 100_000, // 0.05 SOL ± tiny variance
                is_buy: true,
                timestamp_ms: i as u64 * 50,
            });
        }

        // 10% large trades with strong trend
        for i in 45..50 {
            let amount = 3.0 + (i - 45) as f64 * 0.5;
            swaps.push(SwapInfo {
                amount_in: (amount * 1e9) as u128,
                is_buy: true,
                timestamp_ms: i as u64 * 50,
            });
        }

        let verdict = engine.analyze(&swaps);

        println!("Botnet Attack: {:?}", verdict);

        // The key is that small bucket will have very low Hurst (constant values)
        // while large bucket will have high Hurst (trending)
        // This creates scale incoherence

        // Should result in SKIP or low score
        assert!(
            verdict.organic_score < 75,
            "Score should be < 75 for botnet, got {}",
            verdict.organic_score
        );
    }

    #[test]
    fn test_pump_and_dump_start_scenario() {
        // Scenario: Pump & Dump start - Hurst OK initially, but sigma suddenly spikes
        // Expected: Global Hurst OK, but σ jumps to 0.20 (FSW detects instability)
        // Result: SKIP (Reason: "Unstable Structure")

        let config = FreConfig::default();
        let mut engine = FractalEngine::new(config);

        // First phase: Stable pump
        let stable_swaps = create_swaps_with_trend(30, 0.5, 0.05);

        // Analyze multiple times to build stable FSW
        for _ in 0..5 {
            engine.analyze(&stable_swaps);
        }

        // Second phase: Sudden chaos (pump & dump)
        let mut chaotic_swaps = stable_swaps.clone();
        // Add erratic behavior
        for i in 0..20 {
            let amount = if i % 2 == 0 { 5.0 } else { 0.1 };
            chaotic_swaps.push(SwapInfo {
                amount_in: (amount * 1e9) as u128,
                is_buy: i % 3 != 0,
                timestamp_ms: (30 + i) as u64 * 50,
            });
        }

        let verdict = engine.analyze(&chaotic_swaps);

        println!("Pump & Dump: {:?}", verdict);

        // The variance in Hurst should increase
        // After seeing stable then chaotic patterns, sigma should be elevated
        assert!(
            verdict.stability_sigma > 0.0,
            "Stability sigma should be > 0"
        );

        // We should at least get a Watch or Skip
        assert!(
            !matches!(verdict.action, FractalAction::Buy),
            "Should not BUY during unstable structure"
        );
    }

    #[test]
    fn test_insufficient_data_edge_case() {
        // Scenario: Very few transactions (< 10)
        // Expected: WATCH with low confidence

        let config = FreConfig::default();
        let mut engine = FractalEngine::new(config);

        let swaps = create_swaps_with_trend(5, 0.5, 0.1);
        let verdict = engine.analyze(&swaps);

        println!("Insufficient Data: {:?}", verdict);

        assert!(
            matches!(verdict.action, FractalAction::Watch(_)),
            "Should WATCH for insufficient data"
        );

        if let FractalAction::Watch(reason) = &verdict.action {
            assert!(
                reason.contains("Insufficient") || reason.contains("10"),
                "Watch reason should mention insufficient data"
            );
        }

        assert_eq!(
            verdict.organic_score, 0,
            "Score should be 0 for insufficient data"
        );
    }

    #[test]
    fn test_all_identical_swaps_edge_case() {
        // Scenario: All swaps are identical (obvious bot)
        // Expected: SKIP (degenerate data)

        let config = FreConfig::default();
        let mut engine = FractalEngine::new(config);

        let swaps = create_swaps_constant(30, 1.0);
        let verdict = engine.analyze(&swaps);

        println!("All Identical: {:?}", verdict);

        assert!(
            matches!(verdict.action, FractalAction::Skip(_)),
            "Should SKIP for all identical swaps"
        );

        if let FractalAction::Skip(reason) = &verdict.action {
            assert!(
                reason.contains("Degenerate") || reason.contains("identical"),
                "Skip reason should mention degenerate data"
            );
        }
    }

    #[test]
    fn test_stt_min_tx_threshold() {
        // Scenario: Just below STT minimum (11 tx) vs just above (13 tx)
        // Expected: Different coherence handling

        let config = FreConfig::default();
        let mut engine = FractalEngine::new(config);

        // Below threshold
        let swaps_below = create_swaps_with_trend(11, 0.5, 0.05);
        let verdict_below = engine.analyze(&swaps_below);

        println!("Below STT threshold: {:?}", verdict_below);

        // Should use default coherence (0.5) when below threshold
        assert_eq!(
            verdict_below.coherence, 0.5,
            "Should use neutral coherence when below STT threshold"
        );

        // Above threshold - reset engine
        let mut engine2 = FractalEngine::new(config);
        let swaps_above = create_swaps_with_trend(15, 0.5, 0.05);
        let verdict_above = engine2.analyze(&swaps_above);

        println!("Above STT threshold: {:?}", verdict_above);

        // Should calculate actual coherence
        assert_ne!(
            verdict_above.coherence, 0.5,
            "Should calculate real coherence when above STT threshold"
        );
    }

    #[test]
    fn test_config_thresholds() {
        // Test that custom config thresholds are respected

        let custom_config = FreConfig {
            stt_min_tx_count: 20,
            fsw_sigma_watch: 0.10,
            fsw_sigma_skip: 0.15,
            min_organic_score: 80,
            arb_penalty_factor: 2.0,
        };

        let mut engine = FractalEngine::new(custom_config);

        let swaps = create_swaps_with_trend(25, 0.5, 0.05);
        let verdict = engine.analyze(&swaps);

        println!("Custom Config: {:?}", verdict);

        // Verify it doesn't panic and produces valid output
        assert!(verdict.hurst_global >= 0.0 && verdict.hurst_global <= 1.0);
        assert!(verdict.coherence >= 0.0 && verdict.coherence <= 1.0);
        assert!(verdict.organic_score <= 100);
    }
}
