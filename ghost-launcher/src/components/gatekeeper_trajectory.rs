//! Gatekeeper V2.5 Trajectory Aware Scoring (TAS)
//!
//! Scoring functions and types for trajectory-aware confidence modulation.
//! The `materialize_trajectory()` method lives on `GatekeeperBuffer` because
//! it needs access to private buffer fields (`buffered_txs`, `first_tx_ts`, etc.).

use ghost_brain::config::gatekeeper_v25_config::TrajectoryAwareScoringConfig;
use ghost_core::checkpoint::MaterializedTrajectoryAssessment;
use std::collections::HashMap;

use crate::events::PoolTransaction;

/// Per-segment aggregated metrics.
#[derive(Debug, Clone)]
pub struct TrajectorySegment {
    pub tx_count: usize,
    pub buy_ratio: f64,
    pub avg_interval_ms: f64,
    pub total_volume_sol: f64,
    pub hhi: f64,
}

/// Full trajectory assessment from 3-segment analysis.
#[derive(Debug, Clone)]
pub struct TrajectoryAssessment {
    /// Overall TAS score: weighted sum of 5 dimensions, [0.0, 1.0]
    pub overall_tas_score: f64,
    /// Momentum dimension score
    pub momentum_score: f64,
    /// HHI trajectory score (declining = good)
    pub hhi_score: f64,
    /// Volume consistency score
    pub volume_score: f64,
    /// Interval trajectory score (shortening = good)
    pub interval_score: f64,
    /// Buy ratio stability score
    pub buy_ratio_score: f64,
    /// Number of valid segments (3 if all have sufficient TX)
    pub segment_count: usize,
    /// TX counts per segment
    pub t0_tx_count: usize,
    pub t1_tx_count: usize,
    pub t2_tx_count: usize,
}

impl TrajectoryAssessment {
    #[must_use]
    pub fn to_materialized(&self) -> MaterializedTrajectoryAssessment {
        MaterializedTrajectoryAssessment {
            overall_tas_score: self.overall_tas_score,
            momentum_score: self.momentum_score,
            hhi_score: self.hhi_score,
            volume_score: self.volume_score,
            interval_score: self.interval_score,
            buy_ratio_score: self.buy_ratio_score,
            segment_count: self.segment_count,
            t0_tx_count: self.t0_tx_count,
            t1_tx_count: self.t1_tx_count,
            t2_tx_count: self.t2_tx_count,
        }
    }

    #[must_use]
    pub fn from_materialized(materialized: &MaterializedTrajectoryAssessment) -> Self {
        Self {
            overall_tas_score: materialized.overall_tas_score,
            momentum_score: materialized.momentum_score,
            hhi_score: materialized.hhi_score,
            volume_score: materialized.volume_score,
            interval_score: materialized.interval_score,
            buy_ratio_score: materialized.buy_ratio_score,
            segment_count: materialized.segment_count,
            t0_tx_count: materialized.t0_tx_count,
            t1_tx_count: materialized.t1_tx_count,
            t2_tx_count: materialized.t2_tx_count,
        }
    }
}

/// Build a segment from a list of transactions.
pub fn build_segment(txs: &[&PoolTransaction]) -> TrajectorySegment {
    let tx_count = txs.len();
    let buy_count = txs.iter().filter(|tx| tx.is_buy).count();
    let buy_ratio = if tx_count > 0 {
        buy_count as f64 / tx_count as f64
    } else {
        0.0
    };

    let total_volume_sol: f64 = txs.iter().map(|tx| tx.volume_sol).sum();

    let avg_interval_ms = if tx_count >= 2 {
        let mut ts_sorted: Vec<u64> = txs.iter().map(|tx| tx.timestamp_ms).collect();
        ts_sorted.sort_unstable();
        let total_gap: u64 = ts_sorted
            .windows(2)
            .map(|w| w[1].saturating_sub(w[0]))
            .sum();
        total_gap as f64 / (tx_count - 1) as f64
    } else {
        0.0
    };

    let mut signer_counts: HashMap<&str, usize> = HashMap::with_capacity(tx_count);
    for tx in txs {
        *signer_counts.entry(tx.signer.as_str()).or_insert(0) += 1;
    }
    let hhi = if tx_count > 0 {
        let n = tx_count as f64;
        signer_counts
            .values()
            .map(|&c| {
                let p = c as f64 / n;
                p * p
            })
            .sum()
    } else {
        1.0
    };

    TrajectorySegment {
        tx_count,
        buy_ratio,
        avg_interval_ms,
        total_volume_sol,
        hhi,
    }
}

/// Score trajectory from 3 pre-built segments using configurable weights.
pub fn score_trajectory(
    seg0: &TrajectorySegment,
    seg1: &TrajectorySegment,
    seg2: &TrajectorySegment,
    config: &TrajectoryAwareScoringConfig,
) -> TrajectoryAssessment {
    // 1. Momentum: T2/T0 tx count ratio
    let momentum_ratio = seg2.tx_count as f64 / seg0.tx_count.max(1) as f64;
    let momentum_score = if momentum_ratio > config.momentum_accel_min_ratio {
        1.0
    } else if momentum_ratio < config.momentum_decel_max_ratio {
        0.0
    } else {
        0.5
    };

    // 2. HHI trajectory
    let hhi_ratio = if seg0.hhi > 0.0 {
        seg2.hhi / seg0.hhi
    } else {
        1.0
    };
    let hhi_score = if hhi_ratio < config.hhi_decline_min_ratio {
        1.0
    } else {
        (1.0 - hhi_ratio).clamp(0.0, 1.0)
    };

    // 3. Volume consistency
    let vols = [
        seg0.total_volume_sol,
        seg1.total_volume_sol,
        seg2.total_volume_sol,
    ];
    let vol_mean = (vols[0] + vols[1] + vols[2]) / 3.0;
    let vol_var = vols.iter().map(|v| (v - vol_mean).powi(2)).sum::<f64>() / 3.0;
    let vol_cv = if vol_mean > 0.0 {
        vol_var.sqrt() / vol_mean
    } else {
        1.0
    };
    let volume_score = 1.0 - (vol_cv / config.volume_cv_max).clamp(0.0, 1.0);

    // 4. Interval trajectory
    let interval_ratio = if seg0.avg_interval_ms > 0.0 {
        seg2.avg_interval_ms / seg0.avg_interval_ms
    } else {
        1.0
    };
    let interval_score = if interval_ratio < config.interval_shortening_min_ratio {
        1.0
    } else {
        (1.0 - interval_ratio).clamp(0.0, 1.0)
    };

    // 5. Buy ratio stability
    let buy_ratio_score = (seg2.buy_ratio.min(config.buy_ratio_stability_min)
        / config.buy_ratio_stability_min.max(f64::EPSILON))
    .clamp(0.0, 1.0);

    let overall_tas_score = (config.momentum_trajectory_weight * momentum_score
        + config.hhi_trajectory_weight * hhi_score
        + config.volume_trajectory_weight * volume_score
        + config.interval_trajectory_weight * interval_score
        + config.buy_ratio_trajectory_weight * buy_ratio_score)
        .clamp(0.0, 1.0);

    TrajectoryAssessment {
        overall_tas_score,
        momentum_score,
        hhi_score,
        volume_score,
        interval_score,
        buy_ratio_score,
        segment_count: 3,
        t0_tx_count: seg0.tx_count,
        t1_tx_count: seg1.tx_count,
        t2_tx_count: seg2.tx_count,
    }
}

/// Evaluate trajectory and return the overall TAS score in [0.0, 1.0].
pub fn evaluate_trajectory(trajectory: &TrajectoryAssessment) -> f64 {
    trajectory.overall_tas_score
}

/// Map TAS score [0.0, 1.0] to confidence modulator [min, max].
///
/// Default: 0.75 at score=0, 1.00 at score=0.5, 1.25 at score=1.0.
pub fn compute_tas_modulator(tas_score: f64, config: &TrajectoryAwareScoringConfig) -> f64 {
    config.tas_confidence_modulator_min
        + tas_score * (config.tas_confidence_modulator_max - config.tas_confidence_modulator_min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_brain::config::GatekeeperV2Config;

    fn tas_test_config() -> TrajectoryAwareScoringConfig {
        let cfg = GatekeeperV2Config::default();
        let mut tas = cfg.tas;
        tas.enabled = true;
        tas.tas_min_tx_per_segment = 2;
        tas.tas_min_total_duration_ms = 1000;
        tas
    }

    #[test]
    fn test_compute_tas_modulator_range() {
        let config = tas_test_config();
        assert!(
            (compute_tas_modulator(0.0, &config) - config.tas_confidence_modulator_min).abs()
                < f64::EPSILON
        );
        assert!(
            (compute_tas_modulator(1.0, &config) - config.tas_confidence_modulator_max).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn test_build_segment_empty() {
        let seg = build_segment(&[]);
        assert_eq!(seg.tx_count, 0);
        assert!((seg.buy_ratio - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_score_trajectory_all_ones() {
        let config = tas_test_config();
        let seg = TrajectorySegment {
            tx_count: 10,
            buy_ratio: 0.6,
            avg_interval_ms: 200.0,
            total_volume_sol: 5.0,
            hhi: 0.3,
        };
        let result = score_trajectory(&seg, &seg, &seg, &config);
        // All segments identical → momentum should be ~1.0 ratio → 0.5 (neutral, neither accel nor decel)
        // HHI identical → hhi_score high (declining is good, but stable is still scored)
        assert!(result.overall_tas_score > 0.4 && result.overall_tas_score < 0.9);
        assert_eq!(result.segment_count, 3);
    }

    #[test]
    fn test_score_trajectory_accelerating() {
        let config = tas_test_config();
        let seg0 = TrajectorySegment {
            tx_count: 5,
            buy_ratio: 0.5,
            avg_interval_ms: 400.0,
            total_volume_sol: 2.0,
            hhi: 0.5,
        };
        let seg1 = TrajectorySegment {
            tx_count: 8,
            buy_ratio: 0.6,
            avg_interval_ms: 300.0,
            total_volume_sol: 3.0,
            hhi: 0.4,
        };
        let seg2 = TrajectorySegment {
            tx_count: 12,
            buy_ratio: 0.7,
            avg_interval_ms: 200.0,
            total_volume_sol: 5.0,
            hhi: 0.3,
        };
        let result = score_trajectory(&seg0, &seg1, &seg2, &config);
        // Accelerating momentum (12/5 = 2.4 > 1.15) → momentum 1.0
        // Declining HHI (0.3/0.5 = 0.6 < 0.85) → hhi 1.0
        // Shortening intervals (200/400 = 0.5 < 0.8) → interval 1.0
        assert!(result.overall_tas_score > 0.7);
        assert!((result.momentum_score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_score_trajectory_decelerating() {
        let config = tas_test_config();
        let seg0 = TrajectorySegment {
            tx_count: 12,
            buy_ratio: 0.7,
            avg_interval_ms: 200.0,
            total_volume_sol: 5.0,
            hhi: 0.3,
        };
        let seg1 = TrajectorySegment {
            tx_count: 8,
            buy_ratio: 0.6,
            avg_interval_ms: 300.0,
            total_volume_sol: 3.0,
            hhi: 0.4,
        };
        let seg2 = TrajectorySegment {
            tx_count: 5,
            buy_ratio: 0.5,
            avg_interval_ms: 400.0,
            total_volume_sol: 2.0,
            hhi: 0.5,
        };
        let result = score_trajectory(&seg0, &seg1, &seg2, &config);
        // Decelerating momentum (5/12=0.42 < 0.85) → momentum 0.0
        // Degrading HHI (0.5/0.3=1.67 > 1.0) → hhi near 0
        assert!(result.overall_tas_score < 0.4);
        assert!((result.momentum_score - 0.0).abs() < f64::EPSILON);
    }
}
