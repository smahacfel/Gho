//! Gatekeeper V2.5 — PDD sequence signal detection from `TxSegmentSequence`.
//!
//! Path B (feature-driven, materialized) does not have access to raw buffered
//! transactions. These functions compute spike, ramping, and flash crash
//! signals from the 3-segment trajectory snapshot (`TxSegmentSequence`) carried
//! in `MaterializedFeatureSet`.
//!
//! When the sequence is absent or `min_tx_per_segment_satisfied` is false,
//! callers receive `(false, None)` — honest unavailability, consistent with
//! SSOT contract N14 (no synthetic backfill).

use ghost_brain::config::gatekeeper_v25_config::PumpAndDumpDetectorConfig;
use ghost_core::checkpoint::TxSegmentSequence;

/// Detect a volume spike by comparing T2 volume rate vs (T0+T1)/2 rate.
///
/// Returns `(detected, reason_label)` where `reason_label` is a short
/// diagnostic string when detected.
pub fn detect_spike_from_segments(
    seq: &TxSegmentSequence,
    config: &PumpAndDumpDetectorConfig,
) -> (bool, Option<&'static str>) {
    if !config.spike_detection_enabled {
        return (false, None);
    }
    if !seq.min_tx_per_segment_satisfied {
        return (false, None);
    }

    // Volume rate: total_volume / tx_count → avg volume per TX
    let t0_rate = seq.t0_segment.total_volume_sol / seq.t0_segment.tx_count.max(1) as f64;
    let t1_rate = seq.t1_segment.total_volume_sol / seq.t1_segment.tx_count.max(1) as f64;
    let t2_rate = seq.t2_segment.total_volume_sol / seq.t2_segment.tx_count.max(1) as f64;

    let earlier_avg = (t0_rate + t1_rate) / 2.0;
    if earlier_avg <= f64::EPSILON {
        return (false, None);
    }

    let spike_ratio = t2_rate / earlier_avg;
    if spike_ratio > config.spike_ratio_threshold {
        (true, Some("spike_t2_vs_earlier"))
    } else {
        (false, None)
    }
}

/// Detect ramping: consecutive same-size buys in T1 or T2.
///
/// Uses the `same_size_streak` field from each segment snapshot, which
/// counts the longest sequence of buys within 15% size tolerance.
pub fn detect_ramping_from_segments(
    seq: &TxSegmentSequence,
    config: &PumpAndDumpDetectorConfig,
) -> (bool, Option<&'static str>) {
    if !config.ramping_detection_enabled {
        return (false, None);
    }
    if !seq.min_tx_per_segment_satisfied {
        return (false, None);
    }

    let min_streak = config.ramping_min_consecutive_buys as u32;

    if seq.t1_segment.same_size_streak >= min_streak {
        return (true, Some("ramping_t1_same_size_streak"));
    }
    if seq.t2_segment.same_size_streak >= min_streak {
        return (true, Some("ramping_t2_same_size_streak"));
    }

    (false, None)
}

/// Flash crash detection from segment sequence — honest unavailability.
///
/// Path A detects flash crash from actual price history (`price_history`).
/// Path B only has per-segment transaction sizes, not price impact data.
/// True flash crash detection requires sell price impact, which is not
/// available in `TxSegmentSequence`. Returns `(false, None)` — the
/// signal is marked unavailable, not guessed.
///
/// This preserves N14 (no synthetic backfill) and avoids false parity
/// with Path A's price-based flash crash detector.
pub fn detect_flash_crash_from_segments(
    _seq: &TxSegmentSequence,
    config: &PumpAndDumpDetectorConfig,
) -> (bool, Option<&'static str>) {
    if !config.flash_crash_protection_enabled {
        return (false, None);
    }
    // Flash crash detection requires sell price impact data not available
    // in the segment sequence. Path A handles this via price_history.
    // Path B honestly marks it unavailable (N14).
    (false, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::checkpoint::TrajectorySegmentSnapshot;

    fn test_seq(
        t0_vol: f64,
        t1_vol: f64,
        t2_vol: f64,
        t0_tx: u64,
        t1_tx: u64,
        t2_tx: u64,
        t1_streak: u32,
        t2_streak: u32,
        t2_impact: f64,
    ) -> TxSegmentSequence {
        TxSegmentSequence {
            t0_segment: TrajectorySegmentSnapshot {
                tx_count: t0_tx,
                total_volume_sol: t0_vol,
                max_single_tx_sol: 1.0,
                ..Default::default()
            },
            t1_segment: TrajectorySegmentSnapshot {
                tx_count: t1_tx,
                total_volume_sol: t1_vol,
                same_size_streak: t1_streak,
                max_single_tx_sol: 1.5,
                ..Default::default()
            },
            t2_segment: TrajectorySegmentSnapshot {
                tx_count: t2_tx,
                total_volume_sol: t2_vol,
                same_size_streak: t2_streak,
                max_single_tx_sol: t2_impact,
                ..Default::default()
            },
            total_duration_ms: 6000,
            min_tx_per_segment_satisfied: true,
        }
    }

    fn pdd_config() -> PumpAndDumpDetectorConfig {
        PumpAndDumpDetectorConfig {
            enabled: true,
            spike_detection_enabled: true,
            spike_ratio_threshold: 2.0,
            ramping_detection_enabled: true,
            ramping_min_consecutive_buys: 4,
            flash_crash_protection_enabled: true,
            flash_crash_max_price_impact_pct: 15.0,
            ..Default::default()
        }
    }

    #[test]
    fn test_spike_detected_when_t2_doubles_earlier() {
        let seq = test_seq(1.0, 1.0, 5.0, 5, 5, 5, 0, 0, 1.0);
        let (detected, reason) = detect_spike_from_segments(&seq, &pdd_config());
        assert!(detected);
        assert_eq!(reason, Some("spike_t2_vs_earlier"));
    }

    #[test]
    fn test_spike_not_detected_when_t2_similar() {
        let seq = test_seq(1.0, 1.0, 1.5, 5, 5, 5, 0, 0, 1.0);
        let (detected, _) = detect_spike_from_segments(&seq, &pdd_config());
        assert!(!detected);
    }

    #[test]
    fn test_ramping_detected_t1_streak() {
        let seq = test_seq(1.0, 1.0, 1.0, 5, 5, 5, 5, 0, 1.0);
        let (detected, reason) = detect_ramping_from_segments(&seq, &pdd_config());
        assert!(detected);
        assert_eq!(reason, Some("ramping_t1_same_size_streak"));
    }

    #[test]
    fn test_ramping_not_detected_short_streak() {
        let seq = test_seq(1.0, 1.0, 1.0, 5, 5, 5, 2, 2, 1.0);
        let (detected, _) = detect_ramping_from_segments(&seq, &pdd_config());
        assert!(!detected);
    }

    #[test]
    fn test_disabled_sequence_rules_return_false() {
        let mut cfg = pdd_config();
        cfg.spike_detection_enabled = false;
        let seq = test_seq(1.0, 1.0, 5.0, 5, 5, 5, 0, 0, 1.0);
        let (detected, _) = detect_spike_from_segments(&seq, &cfg);
        assert!(!detected);
    }
}
