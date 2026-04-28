use crate::chaos::amm_math::AmmPool;
use crate::oracle::TransactionMetrics;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Instant;

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

// =============================================================================
// Bot Detection Constants
// =============================================================================

/// Minimum transaction size for whale detection (SOL)
/// Transactions >= this size are considered potential whale activity
const WHALE_MIN_SOL: f64 = 1.0;

/// Maximum transaction size for whale detection (SOL)
/// Transactions <= this size are in the typical whale accumulation range
const WHALE_MAX_SOL: f64 = 5.0;

/// Threshold for micro-transaction spam detection (SOL)
/// Transactions < this size are flagged as potential bot spam
const MICRO_TX_THRESHOLD_SOL: f64 = 0.01;

/// Minimum whale ratio to trigger whale accumulation detection
/// If > 50% of transactions are in whale range, treat as organic
const WHALE_RATIO_THRESHOLD: f64 = 0.5;

/// Minimum mean transaction size for whale accumulation (SOL)
/// Mean must be >= this for whale detection to apply
const WHALE_MEAN_THRESHOLD_SOL: f64 = 1.0;

/// Minimum micro-transaction ratio for bot spam detection
/// If > 50% of transactions are micro-txs, likely bot spam
const MICRO_RATIO_THRESHOLD: f64 = 0.5;

/// Maximum CV for micro-transaction bot detection
/// Low CV with high micro ratio indicates consistent bot spam
const MICRO_CV_THRESHOLD: f32 = 0.3;

/// Minimum normal human range ratio for CV penalty reduction
/// Transactions in 0.1-5 SOL range get reduced bot likeness
const NORMAL_HUMAN_RATIO_THRESHOLD: f64 = 0.7;

/// Normal human transaction range minimum (SOL)
const NORMAL_HUMAN_MIN_SOL: f64 = 0.1;

/// Normal human transaction range maximum (SOL)
const NORMAL_HUMAN_MAX_SOL: f64 = 5.0;

/// Bot likeness score for whale accumulation pattern
/// Low score indicates organic whale activity, not bots
const WHALE_BOT_LIKENESS_SCORE: f32 = 0.2;

/// Bot likeness score for micro-transaction spam
/// High score indicates automated bot spam activity
const MICRO_TX_BOT_LIKENESS_SCORE: f32 = 0.9;

/// CV-based bot likeness reduction factor for normal human range
/// Reduces bot score by 50% when transactions are in 0.1-5 SOL range
const NORMAL_RANGE_BOT_REDUCTION: f32 = 0.5;

// =============================================================================
// Data Structures
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MesaResult {
    pub execution_fingerprint: u64,
    pub bot_likeness: f32,
    pub wash_likeness: f32,
    pub organic_likeness: f32,
    pub entropy_score: f32,
    pub impact_efficiency: f32,
    pub tx_count: usize,
    pub analysis_time_us: u64,
}

#[derive(Debug, Default, Clone)]
pub struct MesaAnalyzer;

impl MesaAnalyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze_microstructure(
        &self,
        pool: &AmmPool,
        metrics: &[TransactionMetrics],
    ) -> MesaResult {
        let start = Instant::now();
        let mut virtual_pool = *pool;

        let mut buys = 0usize;
        let mut sells = 0usize;
        let mut total_volume = 0.0_f64;
        let mut net_volume = 0.0_f64;
        let mut impact_sum = 0.0_f32;
        let mut efficiency_weight = 0.0_f32;
        let mut volumes: Vec<f64> = Vec::new();
        let mut tx_total = 0usize;
        let mut drift_accum = 0.0_f64;

        let mut process_tx = |amount_sol: f64, is_buy: bool| {
            if amount_sol <= 0.0 {
                return;
            }
            tx_total += 1;
            volumes.push(amount_sol);
            total_volume += amount_sol;
            if is_buy {
                buys += 1;
                net_volume += amount_sol;
            } else {
                sells += 1;
                net_volume -= amount_sol;
            }

            let amount_in = (amount_sol * LAMPORTS_PER_SOL).round() as u128;
            if amount_in == 0 {
                return;
            }

            let k_before = virtual_pool.reserve_a as f64 * virtual_pool.reserve_b as f64;
            if let Ok(sim) = virtual_pool.simulate_swap(amount_in, is_buy) {
                if is_buy {
                    virtual_pool.reserve_a = sim.new_reserve_in;
                    virtual_pool.reserve_b = sim.new_reserve_out;
                } else {
                    virtual_pool.reserve_b = sim.new_reserve_in;
                    virtual_pool.reserve_a = sim.new_reserve_out;
                }

                let price_impact = sim.price_impact_bps as f32 / 10_000.0;
                impact_sum += price_impact;
                efficiency_weight += amount_sol as f32;

                let k_after = virtual_pool.reserve_a as f64 * virtual_pool.reserve_b as f64;
                if k_before > 0.0 {
                    drift_accum += (k_after - k_before) / k_before;
                }
            }
        };

        for m in metrics {
            if !m.volumes_sol.is_empty() && !m.is_buys.is_empty() {
                for (amt, is_buy) in m.iter_transactions() {
                    process_tx(amt, is_buy);
                }
            } else if m.tx_count > 0 {
                let total = m.tx_count;
                let inferred_buy = if m.buy_count + m.sell_count > 0 {
                    m.buy_count
                } else {
                    (total + 1) / 2
                };
                let inferred_sell = if m.buy_count + m.sell_count > 0 {
                    m.sell_count
                } else {
                    total - inferred_buy
                };
                let avg_amount = if total > 0 {
                    (m.total_volume_sol / total as f64).max(0.0)
                } else {
                    0.0
                };
                for _ in 0..inferred_buy {
                    process_tx(avg_amount, true);
                }
                for _ in 0..inferred_sell {
                    process_tx(avg_amount, false);
                }
            }
        }

        let entropy_score = compute_entropy(buys, sells);
        let bot_likeness = compute_bot_likeness(&volumes);
        let wash_likeness = compute_wash_likeness(total_volume, net_volume, entropy_score);
        let organic_likeness = compute_organic_score(bot_likeness, wash_likeness, entropy_score);
        let avg_drift = if tx_total > 0 {
            drift_accum / tx_total as f64
        } else {
            0.0
        };

        let impact_efficiency = if efficiency_weight > 0.0 {
            impact_sum / efficiency_weight
        } else {
            0.0
        };

        let fingerprint = build_fingerprint(
            buys,
            sells,
            total_volume,
            net_volume,
            entropy_score,
            bot_likeness,
            wash_likeness,
            organic_likeness,
            avg_drift,
        );

        MesaResult {
            execution_fingerprint: fingerprint,
            bot_likeness,
            wash_likeness,
            organic_likeness,
            entropy_score,
            impact_efficiency,
            tx_count: tx_total,
            analysis_time_us: start.elapsed().as_micros() as u64,
        }
    }
}

fn compute_entropy(buys: usize, sells: usize) -> f32 {
    let total = buys + sells;
    if total == 0 {
        return 0.0;
    }
    let p_buy = buys as f32 / total as f32;
    let p_sell = sells as f32 / total as f32;

    let h_buy = if p_buy > 0.0 {
        -p_buy * p_buy.log2()
    } else {
        0.0
    };
    let h_sell = if p_sell > 0.0 {
        -p_sell * p_sell.log2()
    } else {
        0.0
    };
    (h_buy + h_sell).clamp(0.0, 1.0)
}

fn compute_bot_likeness(volumes: &[f64]) -> f32 {
    if volumes.is_empty() {
        return 0.0;
    }
    let mean = volumes.iter().sum::<f64>() / volumes.len() as f64;
    if mean <= 0.0 {
        return 0.0;
    }

    // Whale detection: Large transactions in whale range are likely human whales, not bots
    // Count transactions in whale range
    let whale_count = volumes
        .iter()
        .filter(|&&v| v >= WHALE_MIN_SOL && v <= WHALE_MAX_SOL)
        .count();
    let whale_ratio = whale_count as f64 / volumes.len() as f64;

    // If majority of transactions are in whale range, treat as organic
    if whale_ratio > WHALE_RATIO_THRESHOLD && mean >= WHALE_MEAN_THRESHOLD_SOL {
        // Whale accumulation pattern - low bot likeness
        return WHALE_BOT_LIKENESS_SCORE;
    }

    // Calculate coefficient of variation for standard bot detection
    let variance = volumes
        .iter()
        .map(|v| {
            let diff = v - mean;
            diff * diff
        })
        .sum::<f64>()
        / volumes.len() as f64;
    let std_dev = variance.sqrt();
    let cv = (std_dev / mean).min(1.5);

    // Micro-transaction bot detection: Very small consistent transactions
    let micro_count = volumes
        .iter()
        .filter(|&&v| v < MICRO_TX_THRESHOLD_SOL)
        .count();
    let micro_ratio = micro_count as f64 / volumes.len() as f64;

    // If majority are micro-transactions with low CV, likely bot spam
    if micro_ratio > MICRO_RATIO_THRESHOLD && cv < MICRO_CV_THRESHOLD as f64 {
        return MICRO_TX_BOT_LIKENESS_SCORE;
    }

    // Standard CV-based bot detection (low CV = bot-like behavior)
    // But reduced for transactions in normal human range
    let normal_human_range_count = volumes
        .iter()
        .filter(|&&v| v >= NORMAL_HUMAN_MIN_SOL && v <= NORMAL_HUMAN_MAX_SOL)
        .count();
    let normal_ratio = normal_human_range_count as f64 / volumes.len() as f64;

    let base_bot_likeness = (1.0 - cv as f32).clamp(0.0, 1.0);

    // Reduce bot likeness for transactions in normal human range
    if normal_ratio > NORMAL_HUMAN_RATIO_THRESHOLD {
        (base_bot_likeness * NORMAL_RANGE_BOT_REDUCTION).clamp(0.0, 1.0)
    } else {
        base_bot_likeness
    }
}

fn compute_wash_likeness(total_volume: f64, net_volume: f64, entropy_score: f32) -> f32 {
    if total_volume <= 0.0 {
        return 0.0;
    }

    // Calculate volume imbalance (how much net buying vs total volume)
    let volume_imbalance = 1.0 - (net_volume.abs() / total_volume).min(1.0);

    // High volume_imbalance (close to 1.0) = balanced buys/sells = potential wash trading
    // Low volume_imbalance (close to 0.0) = strong directional flow = not wash trading

    // However, in early token phases, balanced volume could also indicate:
    // - Early accumulation with some profit-taking
    // - Multiple whales entering and exiting positions
    // - Natural price discovery

    // Wash trading typically has:
    // 1. Very balanced volume (imbalance > 0.8)
    // 2. Low entropy (same wallets buying and selling)
    // 3. Small, consistent transaction sizes

    // Use entropy as a strong filter:
    // - Low entropy (< 0.5) with balanced volume = likely wash trading
    // - High entropy (> 0.7) even with balanced volume = likely organic

    let base_wash = if volume_imbalance > 0.8 {
        // Very balanced volume - check entropy
        if entropy_score < 0.5 {
            // Low entropy + balanced = high wash likelihood
            0.9 * volume_imbalance as f32
        } else {
            // High entropy + balanced = likely multiple organic traders
            0.3 * volume_imbalance as f32
        }
    } else if volume_imbalance > 0.6 {
        // Moderately balanced
        if entropy_score < 0.5 {
            0.6 * volume_imbalance as f32
        } else {
            0.2 * volume_imbalance as f32
        }
    } else {
        // Strong directional flow - unlikely to be wash trading
        0.1 * volume_imbalance as f32
    };

    base_wash.clamp(0.0, 1.0)
}

fn compute_organic_score(bot_likeness: f32, wash_likeness: f32, entropy_score: f32) -> f32 {
    let organic_raw = (1.0 - bot_likeness + 1.0 - wash_likeness + entropy_score) / 3.0;
    organic_raw.clamp(0.0, 1.0)
}

fn build_fingerprint(
    buys: usize,
    sells: usize,
    total_volume: f64,
    net_volume: f64,
    entropy_score: f32,
    bot_likeness: f32,
    wash_likeness: f32,
    organic_likeness: f32,
    avg_drift: f64,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    buys.hash(&mut hasher);
    sells.hash(&mut hasher);
    (total_volume.to_bits()).hash(&mut hasher);
    (net_volume.to_bits()).hash(&mut hasher);
    (entropy_score.to_bits()).hash(&mut hasher);
    (bot_likeness.to_bits()).hash(&mut hasher);
    (wash_likeness.to_bits()).hash(&mut hasher);
    (organic_likeness.to_bits()).hash(&mut hasher);
    (avg_drift.to_bits()).hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chaos::amm_math::AmmPool;

    fn genesis_pool() -> AmmPool {
        AmmPool::new(30_000_000_000, 1_073_000_000_000_000, 100).unwrap()
    }

    fn build_metrics(volumes: Vec<f64>, is_buys: Vec<bool>) -> TransactionMetrics {
        let timestamps: Vec<u64> = (0..volumes.len()).map(|i| (i as u64) * 100).collect();
        let signers = vec!["wallet".to_string(); volumes.len()];
        TransactionMetrics::from_transactions(&timestamps, &signers, &volumes, &is_buys)
    }

    #[test]
    fn test_detect_wash_trading() {
        let volumes = vec![1.0; 12];
        let is_buys = (0..12).map(|i| i % 2 == 0).collect();
        let metrics = build_metrics(volumes, is_buys);

        let analyzer = MesaAnalyzer::new();
        let result = analyzer.analyze_microstructure(&genesis_pool(), &[metrics]);

        assert!(
            result.wash_likeness > 0.9,
            "Wash likeness should be high for alternating flow, got {}",
            result.wash_likeness
        );
        assert_eq!(result.tx_count, 12);
    }

    #[test]
    fn test_detect_sniper_attack() {
        let volumes = vec![0.25; 10];
        let is_buys = vec![true; 10];
        let metrics = build_metrics(volumes, is_buys);

        let analyzer = MesaAnalyzer::new();
        let result = analyzer.analyze_microstructure(&genesis_pool(), &[metrics]);

        assert!(
            result.bot_likeness > 0.9,
            "Bot likeness should be high for identical sequential buys, got {}",
            result.bot_likeness
        );
        assert!(result.wash_likeness < 0.4, "Wash likeness should stay low");
    }
}
