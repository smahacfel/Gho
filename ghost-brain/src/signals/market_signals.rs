//! Market Signals
//!
//! Aggregated market signals used as input for QEDD and MCI computations.

use ghost_core::shadow_ledger::types::PriceState;
use ghost_core::shadow_ledger::{MarketSnapshot, ShadowLedger};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

const EPS: f64 = 1e-9;
const IMPACT_VOLUME_FRACTION: f64 = 0.001;
const MIN_SPREAD_PCT: f64 = 0.002;
const RESONANCE_HIGH_RISK_CV: f64 = 0.25;
const RESONANCE_MEDIUM_RISK_CV: f64 = 0.5;
const RESONANCE_HIGH_RISK: f64 = 0.8;
const RESONANCE_MEDIUM_RISK: f64 = 0.4;
const RESONANCE_LOW_RISK: f64 = 0.1;

/// Aggregated market signals for analysis
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MarketSignals {
    /// Volume-based signals
    pub volume: VolumeSignals,

    /// Price-based signals
    pub price: PriceSignals,

    /// Order book signals
    pub orderbook: OrderbookSignals,

    /// Time-based signals
    pub time: TimeSignals,

    /// SOBP (Slot-Over-Slot Buying Pressure) signals
    pub sobp: SobpSignals,

    /// Flow-based signals (QOFSV-related)
    pub flow: FlowSignals,

    /// Resonance signals (bot detection)
    pub resonance: ResonanceSignals,

    /// Deviation/anomaly signals (QMAN-related)
    pub deviation: DeviationSignals,

    /// Entropy signals (SSMI, MPCF)
    pub entropy: EntropySignals,
}

/// Volume-related signals
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VolumeSignals {
    /// Current volume
    pub current: f64,

    /// Moving average volume
    pub ma: f64,

    /// Volume standard deviation
    pub std_dev: f64,
}

/// Price-related signals
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PriceSignals {
    /// Current price
    pub current: f64,

    /// Price momentum
    pub momentum: f64,

    /// Price volatility
    pub volatility: f64,

    /// Whether price data is considered valid for momentum/stability calculations
    pub valid: bool,
}

/// Order book signals
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct OrderbookSignals {
    /// Bid-ask spread
    pub spread: f64,

    /// Order book depth
    pub depth: f64,

    /// Imbalance ratio
    pub imbalance: f64,
}

/// Time-based signals
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TimeSignals {
    /// Timestamp in milliseconds
    pub timestamp_ms: u64,

    /// Time since last trade
    pub time_since_last_trade_ms: u64,
}

/// SOBP (Slot-Over-Slot Buying Pressure) signals
///
/// SOBP measures the rate of change in buying pressure between consecutive Solana slots
/// to detect pump onset within the critical 0-2 second window after token launch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SobpSignals {
    /// Current SOBP value (ratio of current to previous slot buying pressure)
    pub current: f64,

    /// SOBP drop indicator: negative change in SOBP (0.0 = no drop, 1.0 = complete collapse)
    pub drop: f64,

    /// Historical SOBP moving average for comparison
    pub ma: f64,
}

/// Flow-based signals (QOFSV direction)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FlowSignals {
    /// Outflow intensity [0.0, 1.0] - higher means capital leaving
    pub outflow: f64,

    /// Directional alignment with QASS [-1.0, 1.0] - correlation with expected direction
    pub qass_alignment: f64,

    /// Net flow vector magnitude
    pub magnitude: f64,
}

/// Resonance signals (bot activity detection)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ResonanceSignals {
    /// Resonance risk score [0.0, 1.0] - higher means more bot-like periodic behavior
    pub risk: f64,

    /// Coefficient of variation of trade intervals
    pub cv: f64,

    /// Number of samples used in analysis
    pub sample_count: usize,
}

/// Deviation/anomaly signals (QMAN-related)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DeviationSignals {
    /// Deviation risk [0.0, 1.0] - market divergence from expected quantum state
    pub risk: f64,

    /// State coherence loss metric
    pub coherence_loss: f64,

    /// Anomaly magnitude
    pub anomaly_magnitude: f64,
}

/// Entropy signals (SSMI, MPCF)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EntropySignals {
    /// SSMI entropy [0.0, 1.0] - Sub-Slot Microentropy Index
    pub ssmi: f64,

    /// MPCF entropy [0.0, 1.0] - Micro-Payload Cognitive Fingerprint entropy
    pub mpcf: f64,

    /// Combined entropy measure
    pub combined: f64,
}

impl MarketSignals {
    /// Build MarketSignals directly from ShadowLedger cache without RPC latency.
    ///
    /// This method consumes in-memory snapshots only and performs O(n) math on
    /// a tiny (<=3) window to stay well under the 50µs budget in the sniper path.
    pub fn from_shadow_ledger(ledger: &ShadowLedger, mint: Pubkey) -> Self {
        let snapshots = match ledger.get_snapshots(&mint) {
            Some(snaps) if !snaps.is_empty() => snaps,
            _ => return Self::mock(),
        };
        let latest = match snapshots.last() {
            Some(last) => last.clone(),
            None => return Self::mock(),
        };

        // Price series stats (only snapshots with valid price)
        let valid_price_snaps: Vec<MarketSnapshot> = snapshots
            .iter()
            .cloned()
            .filter(|s| s.price_state == PriceState::Valid)
            .collect();
        let price_series: Vec<f64> = valid_price_snaps
            .iter()
            .map(|s| s.price_sol_per_token)
            .collect();
        let price_current = valid_price_snaps
            .last()
            .map(|s| s.price_sol_per_token)
            .unwrap_or(0.0);
        let price_valid = !price_series.is_empty();
        let (price_mean, price_std) = mean_std(&price_series);
        let price_volatility = if !price_valid {
            0.0
        } else if price_mean.abs() > EPS {
            (price_std / price_mean.abs()).clamp(0.0, 5.0)
        } else {
            price_std
        };

        // Momentum: relative change normalized by elapsed time (per second)
        let price_momentum = if valid_price_snaps.len() < 2 {
            0.0
        } else {
            valid_price_snaps
                .windows(2)
                .last()
                .map(|w| {
                    let delta_price = w[1].price_sol_per_token - w[0].price_sol_per_token;
                    let rel = if w[0].price_sol_per_token.abs() > EPS {
                        delta_price / w[0].price_sol_per_token
                    } else {
                        delta_price
                    };
                    let dt_ms = w[1].timestamp_ms.saturating_sub(w[0].timestamp_ms);
                    if dt_ms == 0 {
                        0.0
                    } else {
                        (rel * 1000.0) / dt_ms as f64
                    }
                })
                .unwrap_or(0.0)
        };

        // Volume deltas between snapshots
        let mut volume_deltas = Vec::with_capacity(snapshots.len().saturating_sub(1));
        for w in snapshots.windows(2) {
            volume_deltas.push(w[1].cum_volume_sol - w[0].cum_volume_sol);
        }
        let volume_current = volume_deltas.last().copied().unwrap_or(0.0);
        let (volume_mean, volume_std) = mean_std(&volume_deltas);
        let volume_ma = if volume_deltas.is_empty() {
            volume_current
        } else {
            volume_mean
        };

        // Orderbook approximations from cached geometry
        let simulated_order = (latest.reserve_base * IMPACT_VOLUME_FRACTION).max(1.0);
        // Approximate best bid/ask distance by projecting a small-order price impact.
        // d_price_d_slippage and d_price_d_volume are derivatives from ShadowLedger snapshots
        // expressed as delta-price per unit slippage and delta-price per token of size=1.
        let spread_from_slippage = latest.d_price_d_slippage.abs();
        let spread_from_volume = latest.d_price_d_volume.abs() * simulated_order;
        let derivative_spread = spread_from_slippage.max(spread_from_volume);
        let spread = if derivative_spread > EPS {
            derivative_spread
        } else {
            price_current * MIN_SPREAD_PCT
        };
        let depth = latest.reserve_quote;
        let imbalance = {
            let base_as_quote = latest.reserve_base * price_current;
            let denom = latest.reserve_quote + base_as_quote + EPS;
            ((latest.reserve_quote - base_as_quote).abs() / denom).clamp(0.0, 1.0)
        };

        // Time signals
        let time_since_last_trade_ms = snapshots
            .windows(2)
            .last()
            .map(|w| w[1].timestamp_ms.saturating_sub(w[0].timestamp_ms))
            .unwrap_or(0);

        // SOBP derived from bonding progress
        let sobp_values: Vec<f64> = snapshots
            .iter()
            .map(|s| s.bonding_progress_pct / 100.0)
            .collect();
        let sobp_current = sobp_values.last().copied().unwrap_or(0.0);
        let sobp_ma = if sobp_values.is_empty() {
            sobp_current
        } else {
            sobp_values.iter().sum::<f64>() / sobp_values.len() as f64
        };
        let sobp_drop = sobp_values
            .windows(2)
            .last()
            .map(|w| {
                if w[0] > EPS {
                    ((w[0] - w[1]).max(0.0) / w[0]).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);

        // Flow estimates driven by short-term momentum/volatility
        let qass_alignment = price_momentum.clamp(-1.0, 1.0);
        let outflow = (-price_momentum).max(0.0).min(1.0);
        let flow_magnitude = price_volatility.min(1.0);

        // Resonance from interval regularity (low CV => bot-like)
        let mut intervals = Vec::with_capacity(snapshots.len().saturating_sub(1));
        for w in snapshots.windows(2) {
            intervals.push(w[1].timestamp_ms.saturating_sub(w[0].timestamp_ms) as f64);
        }
        let (interval_mean, interval_std) = mean_std(&intervals);
        let interval_cv = if interval_mean > EPS {
            (interval_std / interval_mean).clamp(0.0, 5.0)
        } else {
            0.0
        };
        let resonance_risk = if intervals.is_empty() {
            0.0
        } else if interval_cv < RESONANCE_HIGH_RISK_CV {
            RESONANCE_HIGH_RISK
        } else if interval_cv < RESONANCE_MEDIUM_RISK_CV {
            RESONANCE_MEDIUM_RISK
        } else {
            RESONANCE_LOW_RISK
        };

        // Deviation/entropy heuristics from volatility and interval dispersion
        let deviation_risk = price_volatility.min(1.0);
        let coherence_loss = sobp_drop;
        let anomaly_magnitude = (price_volatility + interval_cv).min(1.0);

        // Entropy blends price volatility (SSMI analogue) and resonance/deviation signals (MPCF analogue)
        let ssmi_entropy = (price_volatility + interval_cv).min(1.0);
        let mpcf_entropy = (resonance_risk + deviation_risk).min(1.0);
        let entropy_combined = ((ssmi_entropy + mpcf_entropy) / 2.0).min(1.0);

        Self {
            volume: VolumeSignals {
                current: volume_current,
                ma: volume_ma,
                std_dev: volume_std,
            },
            price: PriceSignals {
                current: price_current,
                momentum: price_momentum,
                volatility: price_volatility,
                valid: price_valid,
            },
            orderbook: OrderbookSignals {
                spread,
                depth,
                imbalance,
            },
            time: TimeSignals {
                timestamp_ms: latest.timestamp_ms,
                time_since_last_trade_ms,
            },
            sobp: SobpSignals {
                current: sobp_current,
                drop: sobp_drop,
                ma: sobp_ma,
            },
            flow: FlowSignals {
                outflow,
                qass_alignment,
                magnitude: flow_magnitude,
            },
            resonance: ResonanceSignals {
                risk: resonance_risk,
                cv: interval_cv,
                sample_count: intervals.len(),
            },
            deviation: DeviationSignals {
                risk: deviation_risk,
                coherence_loss,
                anomaly_magnitude,
            },
            entropy: EntropySignals {
                ssmi: ssmi_entropy,
                mpcf: mpcf_entropy,
                combined: entropy_combined,
            },
        }
    }

    /// Create a mock MarketSignals for testing purposes
    pub fn mock() -> Self {
        Self {
            volume: VolumeSignals {
                current: 100000.0,
                ma: 95000.0,
                std_dev: 5000.0,
            },
            price: PriceSignals {
                current: 1.0,
                momentum: 0.05,
                volatility: 0.02,
                valid: true,
            },
            orderbook: OrderbookSignals {
                spread: 0.001,
                depth: 50000.0,
                imbalance: 0.55,
            },
            time: TimeSignals {
                timestamp_ms: 1000000,
                time_since_last_trade_ms: 100,
            },
            sobp: SobpSignals {
                current: 1.5,
                drop: 0.1,
                ma: 1.4,
            },
            flow: FlowSignals {
                outflow: 0.3,
                qass_alignment: 0.7,
                magnitude: 0.8,
            },
            resonance: ResonanceSignals {
                risk: 0.2,
                cv: 0.6,
                sample_count: 32,
            },
            deviation: DeviationSignals {
                risk: 0.15,
                coherence_loss: 0.1,
                anomaly_magnitude: 0.2,
            },
            entropy: EntropySignals {
                ssmi: 0.65,
                mpcf: 0.70,
                combined: 0.675,
            },
        }
    }

    /// Create signals for a hype scenario (strong pump)
    pub fn mock_hype() -> Self {
        Self {
            volume: VolumeSignals {
                current: 500000.0,
                ma: 100000.0,
                std_dev: 50000.0,
            },
            price: PriceSignals {
                current: 5.0,
                momentum: 0.95,
                volatility: 0.15,
                valid: true,
            },
            orderbook: OrderbookSignals {
                spread: 0.005,
                depth: 200000.0,
                imbalance: 0.85,
            },
            time: TimeSignals {
                timestamp_ms: 2000000,
                time_since_last_trade_ms: 50,
            },
            sobp: SobpSignals {
                current: 4.5,
                drop: 0.0,
                ma: 2.0,
            },
            flow: FlowSignals {
                outflow: 0.05,
                qass_alignment: 0.95,
                magnitude: 0.98,
            },
            resonance: ResonanceSignals {
                risk: 0.1,
                cv: 0.85,
                sample_count: 64,
            },
            deviation: DeviationSignals {
                risk: 0.05,
                coherence_loss: 0.02,
                anomaly_magnitude: 0.05,
            },
            entropy: EntropySignals {
                ssmi: 0.85,
                mpcf: 0.90,
                combined: 0.875,
            },
        }
    }

    /// Create signals for a rug scenario (collapse/dump)
    pub fn mock_rug() -> Self {
        Self {
            volume: VolumeSignals {
                current: 20000.0,
                ma: 100000.0,
                std_dev: 80000.0,
            },
            price: PriceSignals {
                current: 0.1,
                momentum: -0.90,
                volatility: 0.50,
                valid: true,
            },
            orderbook: OrderbookSignals {
                spread: 0.05,
                depth: 5000.0,
                imbalance: 0.15,
            },
            time: TimeSignals {
                timestamp_ms: 3000000,
                time_since_last_trade_ms: 5000,
            },
            sobp: SobpSignals {
                current: 0.2,
                drop: 0.95,
                ma: 1.5,
            },
            flow: FlowSignals {
                outflow: 0.95,
                qass_alignment: -0.85,
                magnitude: 0.92,
            },
            resonance: ResonanceSignals {
                risk: 0.85,
                cv: 0.15,
                sample_count: 16,
            },
            deviation: DeviationSignals {
                risk: 0.90,
                coherence_loss: 0.85,
                anomaly_magnitude: 0.95,
            },
            entropy: EntropySignals {
                ssmi: 0.20,
                mpcf: 0.15,
                combined: 0.175,
            },
        }
    }

    /// Create signals for a stable scenario (balanced market)
    pub fn mock_stable() -> Self {
        Self {
            volume: VolumeSignals {
                current: 100000.0,
                ma: 100000.0,
                std_dev: 10000.0,
            },
            price: PriceSignals {
                current: 1.0,
                momentum: 0.02,
                volatility: 0.05,
                valid: true,
            },
            orderbook: OrderbookSignals {
                spread: 0.002,
                depth: 100000.0,
                imbalance: 0.50,
            },
            time: TimeSignals {
                timestamp_ms: 4000000,
                time_since_last_trade_ms: 200,
            },
            sobp: SobpSignals {
                current: 1.0,
                drop: 0.0,
                ma: 1.0,
            },
            flow: FlowSignals {
                outflow: 0.50,
                qass_alignment: 0.0,
                magnitude: 0.30,
            },
            resonance: ResonanceSignals {
                risk: 0.50,
                cv: 0.50,
                sample_count: 48,
            },
            deviation: DeviationSignals {
                risk: 0.40,
                coherence_loss: 0.30,
                anomaly_magnitude: 0.25,
            },
            entropy: EntropySignals {
                ssmi: 0.50,
                mpcf: 0.50,
                combined: 0.50,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::shadow_ledger::{MarketSnapshot, ShadowLedger};
    use solana_sdk::pubkey::Pubkey;

    #[test]
    fn test_mock_signals() {
        let signals = MarketSignals::mock();
        assert_eq!(signals.volume.current, 100000.0);
        assert_eq!(signals.price.current, 1.0);
        assert!(signals.orderbook.spread > 0.0);
    }

    #[test]
    fn test_serialization() {
        let signals = MarketSignals::mock();
        let serialized = serde_json::to_string(&signals).unwrap();
        let deserialized: MarketSignals = serde_json::from_str(&serialized).unwrap();
        assert_eq!(signals.volume.current, deserialized.volume.current);
    }

    #[test]
    fn test_from_shadow_ledger_builds_real_values() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        let mut g0 = MarketSnapshot::new(1_000);
        g0.price_sol_per_token = 0.5;
        g0.price_state = PriceState::Valid;
        g0.cum_volume_sol = 0.0;
        g0.reserve_base = 10_000.0;
        g0.reserve_quote = 5_000.0;
        g0.bonding_progress_pct = 10.0;

        let mut g1 = MarketSnapshot::new(1_050);
        g1.price_sol_per_token = 0.55;
        g1.price_state = PriceState::Valid;
        g1.cum_volume_sol = 100.0;
        g1.reserve_base = 9_800.0;
        g1.reserve_quote = 5_200.0;
        g1.bonding_progress_pct = 15.0;

        let mut g2 = MarketSnapshot::new(1_100);
        g2.price_sol_per_token = 0.6;
        g2.price_state = PriceState::Valid;
        g2.cum_volume_sol = 250.0;
        g2.reserve_base = 9_500.0;
        g2.reserve_quote = 5_400.0;
        g2.bonding_progress_pct = 18.0;

        ledger.commit_history(mint, vec![g0, g1, g2], None);

        let signals = MarketSignals::from_shadow_ledger(&ledger, mint);
        assert!((signals.price.current - 0.6).abs() < 1e-6);
        assert!(signals.price.momentum > 0.0);
        assert!(signals.price.volatility > 0.0);
        assert_eq!(signals.time.time_since_last_trade_ms, 50);
        assert!(signals.orderbook.spread > 0.0);
        assert!((signals.volume.current - 150.0).abs() < 1e-6);
        assert!(signals.sobp.current > 0.0);
    }

    #[test]
    fn test_from_shadow_ledger_handles_invalid_price() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        let mut g0 = MarketSnapshot::new(1_000);
        g0.price_sol_per_token = f64::INFINITY;
        g0.price_state = PriceState::Invalid;
        g0.reserve_base = 0.0;
        g0.reserve_quote = 0.0;

        let mut g1 = MarketSnapshot::new(1_050);
        g1.price_sol_per_token = 0.0;
        g1.price_state = PriceState::Invalid;
        g1.reserve_base = 0.0;
        g1.reserve_quote = 0.0;

        ledger.commit_history(mint, vec![g0, g1], None);

        let signals = MarketSignals::from_shadow_ledger(&ledger, mint);
        assert!(!signals.price.valid);
        assert_eq!(signals.price.momentum, 0.0);
        assert_eq!(signals.price.volatility, 0.0);
    }
}

#[inline]
fn mean_std(values: &[f64]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    if values.len() == 1 {
        return (values[0], 0.0);
    }
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    let mut m2 = 0.0;
    for v in values {
        let diff = *v - mean;
        m2 += diff * diff;
    }
    // Sample variance with Bessel's correction to avoid underestimation for small n
    let variance = if count > 1.0 { m2 / (count - 1.0) } else { 0.0 };
    (mean, variance.sqrt())
}
