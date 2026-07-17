//! Pure sampled-trajectory projection for HET Position Manager V2 PR A.
//!
//! The projection consumes only the bounded canonical `SnapshotTimeline`
//! materialized by the monitoring engine. It does not read live state, await,
//! resolve quotes, or claim complete event-path coverage.

use ghost_core::shadow_ledger::MarketSnapshot;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TrajectoryQualityV1 {
    Usable,
    PartialHistory,
    InsufficientSamples,
    Stale,
    Invalid,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct TrajectoryFlagsV1(u32);

impl TrajectoryFlagsV1 {
    pub(super) const RETURN_1500MS_UNAVAILABLE: u32 = 1 << 0;
    pub(super) const RETURN_5S_UNAVAILABLE: u32 = 1 << 1;
    pub(super) const RETURN_15S_UNAVAILABLE: u32 = 1 << 2;
    pub(super) const COLLAPSED_CANONICAL_UPDATES: u32 = 1 << 3;
    pub(super) const SAME_SLOT_ONLY: u32 = 1 << 4;
    pub(super) const STALE_NEWEST_SAMPLE: u32 = 1 << 5;
    pub(super) const INVALID_SLOT_ORDERING: u32 = 1 << 6;
    pub(super) const INVALID_TIMESTAMP_ORDERING: u32 = 1 << 7;
    pub(super) const INVALID_PRICE: u32 = 1 << 8;

    fn insert(&mut self, flag: u32) {
        self.0 |= flag;
    }

    pub(super) fn contains(self, flag: u32) -> bool {
        self.0 & flag != 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TrajectoryFeaturesV1 {
    pub(super) return_1500ms_bps: Option<i32>,
    pub(super) return_5s_bps: Option<i32>,
    pub(super) return_15s_bps: Option<i32>,
    pub(super) peak_mark_price_sol: Option<f64>,
    pub(super) peak_sample_slot: Option<u64>,
    pub(super) peak_sample_timestamp_ms: Option<u64>,
    pub(super) drawdown_from_peak_bps: Option<i32>,
    pub(super) time_since_peak_ms: Option<u64>,
    pub(super) peak_giveback_velocity_bps_per_sec: Option<i32>,
    pub(super) newest_sample_slot: Option<u64>,
    pub(super) newest_sample_timestamp_ms: Option<u64>,
    pub(super) newest_sample_age_ms: Option<u64>,
    pub(super) distinct_slots_1500ms: u8,
    /// Delta of canonical state updates, despite the historical `tx_count`
    /// field name used by the source snapshot.
    pub(super) state_update_delta_since_previous_sample: u64,
    pub(super) quality: TrajectoryQualityV1,
    pub(super) flags: TrajectoryFlagsV1,
}

impl TrajectoryFeaturesV1 {
    fn empty(quality: TrajectoryQualityV1) -> Self {
        Self {
            return_1500ms_bps: None,
            return_5s_bps: None,
            return_15s_bps: None,
            peak_mark_price_sol: None,
            peak_sample_slot: None,
            peak_sample_timestamp_ms: None,
            drawdown_from_peak_bps: None,
            time_since_peak_ms: None,
            peak_giveback_velocity_bps_per_sec: None,
            newest_sample_slot: None,
            newest_sample_timestamp_ms: None,
            newest_sample_age_ms: None,
            distinct_slots_1500ms: 0,
            state_update_delta_since_previous_sample: 0,
            quality,
            flags: TrajectoryFlagsV1::default(),
        }
    }

    fn invalidate(mut self, flags: TrajectoryFlagsV1) -> Self {
        self.quality = TrajectoryQualityV1::Invalid;
        self.flags = flags;
        self
    }
}

fn valid_price(snapshot: &MarketSnapshot) -> Option<f64> {
    (snapshot.price_state.is_valid()
        && snapshot.price_sol_per_token.is_finite()
        && snapshot.price_sol_per_token > 0.0)
        .then_some(snapshot.price_sol_per_token)
}

fn bps_return(current: f64, reference: f64) -> Option<i32> {
    let bps = 10_000.0 * (current / reference - 1.0);
    bps.is_finite()
        .then_some(bps.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32)
}

fn sampled_return(
    snapshots: &[MarketSnapshot],
    newest: &MarketSnapshot,
    window_ms: u64,
    monitor_tick_ms: u64,
) -> Option<i32> {
    let target_ts = newest.timestamp_ms.saturating_sub(window_ms);
    let reference = snapshots
        .iter()
        .rev()
        .find(|sample| sample.timestamp_ms <= target_ts)?;
    let elapsed_ms = newest.timestamp_ms.checked_sub(reference.timestamp_ms)?;
    let tolerance_ms = monitor_tick_ms
        .saturating_mul(2)
        .max(window_ms.saturating_div(2));
    if elapsed_ms > window_ms.saturating_add(tolerance_ms) {
        return None;
    }
    bps_return(valid_price(newest)?, valid_price(reference)?)
}

/// Materialize the non-lookahead, sampled trajectory contract.
pub(super) fn materialize_trajectory_v1(
    snapshots: &[MarketSnapshot],
    now_ms: u64,
    monitor_tick_ms: u64,
    short_ms: u64,
    medium_ms: u64,
    long_ms: u64,
    max_newest_sample_age_ms: u64,
) -> TrajectoryFeaturesV1 {
    let Some(newest) = snapshots.last() else {
        return TrajectoryFeaturesV1::empty(TrajectoryQualityV1::Unavailable);
    };

    let mut result = TrajectoryFeaturesV1::empty(TrajectoryQualityV1::InsufficientSamples);
    result.newest_sample_slot = newest.slot;
    result.newest_sample_timestamp_ms = Some(newest.timestamp_ms);
    result.newest_sample_age_ms = now_ms.checked_sub(newest.timestamp_ms);
    let mut flags = TrajectoryFlagsV1::default();

    for (index, sample) in snapshots.iter().enumerate() {
        if valid_price(sample).is_none() {
            flags.insert(TrajectoryFlagsV1::INVALID_PRICE);
        }
        if index == 0 {
            continue;
        }
        let previous = &snapshots[index - 1];
        if sample.timestamp_ms <= previous.timestamp_ms || sample.timestamp_ms > now_ms {
            flags.insert(TrajectoryFlagsV1::INVALID_TIMESTAMP_ORDERING);
        }
        if let (Some(previous_slot), Some(current_slot)) = (previous.slot, sample.slot) {
            if current_slot < previous_slot {
                flags.insert(TrajectoryFlagsV1::INVALID_SLOT_ORDERING);
            }
        }
    }
    if newest.timestamp_ms > now_ms {
        flags.insert(TrajectoryFlagsV1::INVALID_TIMESTAMP_ORDERING);
    }
    if flags.contains(TrajectoryFlagsV1::INVALID_PRICE)
        || flags.contains(TrajectoryFlagsV1::INVALID_TIMESTAMP_ORDERING)
        || flags.contains(TrajectoryFlagsV1::INVALID_SLOT_ORDERING)
    {
        return result.invalidate(flags);
    }
    let Some(newest_price) = valid_price(newest) else {
        flags.insert(TrajectoryFlagsV1::INVALID_PRICE);
        return result.invalidate(flags);
    };

    if let Some(previous) = snapshots.get(snapshots.len().saturating_sub(2)) {
        result.state_update_delta_since_previous_sample =
            newest.tx_count.saturating_sub(previous.tx_count);
        if result.state_update_delta_since_previous_sample > 1 {
            flags.insert(TrajectoryFlagsV1::COLLAPSED_CANONICAL_UPDATES);
        }
    }

    let newest_age_ms = now_ms.saturating_sub(newest.timestamp_ms);
    if newest_age_ms > max_newest_sample_age_ms {
        flags.insert(TrajectoryFlagsV1::STALE_NEWEST_SAMPLE);
    }

    result.return_1500ms_bps = sampled_return(snapshots, newest, short_ms, monitor_tick_ms);
    result.return_5s_bps = sampled_return(snapshots, newest, medium_ms, monitor_tick_ms);
    result.return_15s_bps = sampled_return(snapshots, newest, long_ms, monitor_tick_ms);
    if result.return_1500ms_bps.is_none() {
        flags.insert(TrajectoryFlagsV1::RETURN_1500MS_UNAVAILABLE);
    }
    if result.return_5s_bps.is_none() {
        flags.insert(TrajectoryFlagsV1::RETURN_5S_UNAVAILABLE);
    }
    if result.return_15s_bps.is_none() {
        flags.insert(TrajectoryFlagsV1::RETURN_15S_UNAVAILABLE);
    }

    let cutoff_1500 = newest.timestamp_ms.saturating_sub(short_ms);
    let mut distinct_slots = 0_u8;
    let mut last_slot = None;
    for slot in snapshots
        .iter()
        .filter(|sample| sample.timestamp_ms >= cutoff_1500)
        .filter_map(|sample| sample.slot)
    {
        if last_slot != Some(slot) {
            distinct_slots = distinct_slots.saturating_add(1);
            last_slot = Some(slot);
        }
    }
    result.distinct_slots_1500ms = distinct_slots;
    if !snapshots.is_empty() && distinct_slots <= 1 {
        flags.insert(TrajectoryFlagsV1::SAME_SLOT_ONLY);
    }

    if let Some((peak, peak_price)) = snapshots
        .iter()
        .filter_map(|sample| valid_price(sample).map(|price| (sample, price)))
        .max_by(|(_, lhs), (_, rhs)| lhs.total_cmp(rhs))
    {
        result.peak_mark_price_sol = Some(peak_price);
        result.peak_sample_slot = peak.slot;
        result.peak_sample_timestamp_ms = Some(peak.timestamp_ms);
        let drawdown_bps = (10_000.0 * (1.0 - newest_price / peak_price))
            .max(0.0)
            .round()
            .clamp(0.0, i32::MAX as f64) as i32;
        let time_since_peak_ms = newest.timestamp_ms.saturating_sub(peak.timestamp_ms);
        result.drawdown_from_peak_bps = Some(drawdown_bps);
        result.time_since_peak_ms = Some(time_since_peak_ms);
        result.peak_giveback_velocity_bps_per_sec = Some(
            ((drawdown_bps as i64 * 1_000) / time_since_peak_ms.max(1) as i64)
                .clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        );
    }

    result.quality = if flags.contains(TrajectoryFlagsV1::STALE_NEWEST_SAMPLE) {
        TrajectoryQualityV1::Stale
    } else if snapshots.len() < 2 {
        TrajectoryQualityV1::InsufficientSamples
    } else if result.return_1500ms_bps.is_some()
        && result.return_5s_bps.is_some()
        && result.return_15s_bps.is_some()
    {
        TrajectoryQualityV1::Usable
    } else {
        TrajectoryQualityV1::PartialHistory
    };
    result.flags = flags;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::shadow_ledger::types::PriceState;

    fn sample(ts: u64, slot: u64, updates: u64, price: f64) -> MarketSnapshot {
        MarketSnapshot {
            timestamp_ms: ts,
            slot: Some(slot),
            tx_count: updates,
            price_sol_per_token: price,
            price_state: PriceState::Valid,
            ..MarketSnapshot::default()
        }
    }

    fn project(samples: &[MarketSnapshot], now_ms: u64) -> TrajectoryFeaturesV1 {
        materialize_trajectory_v1(samples, now_ms, 500, 1_500, 5_000, 15_000, 1_500)
    }

    #[test]
    fn exact_boundaries_are_non_lookahead_and_deterministic() {
        let samples = vec![
            sample(1_000, 1, 1, 1.0),
            sample(11_000, 2, 2, 1.5),
            sample(14_500, 3, 3, 1.8),
            sample(16_000, 4, 4, 2.0),
        ];
        let first = project(&samples, 16_000);
        let second = project(&samples, 16_000);

        assert_eq!(first.return_1500ms_bps, Some(1_111));
        assert_eq!(first.return_5s_bps, Some(3_333));
        assert_eq!(first.return_15s_bps, Some(10_000));
        assert_eq!(first.flags, second.flags);
        assert_eq!(first.return_15s_bps, second.return_15s_bps);
    }

    #[test]
    fn old_reference_is_rejected_and_same_slot_is_typed() {
        let samples = vec![sample(1_000, 7, 1, 1.0), sample(16_000, 7, 4, 2.0)];
        let result = materialize_trajectory_v1(&samples, 16_000, 100, 1_500, 5_000, 10_000, 1_500);

        assert_eq!(result.return_1500ms_bps, None);
        assert!(result.flags.contains(TrajectoryFlagsV1::SAME_SLOT_ONLY));
        assert!(result
            .flags
            .contains(TrajectoryFlagsV1::COLLAPSED_CANONICAL_UPDATES));
        assert_eq!(result.state_update_delta_since_previous_sample, 3);
    }

    #[test]
    fn ordering_and_invalid_price_fail_closed() {
        let reversed_slot = vec![sample(1_000, 2, 1, 1.0), sample(2_000, 1, 2, 0.9)];
        assert_eq!(
            project(&reversed_slot, 2_000).quality,
            TrajectoryQualityV1::Invalid
        );

        let reversed_time = vec![sample(2_000, 1, 1, 1.0), sample(1_000, 2, 2, 0.9)];
        assert_eq!(
            project(&reversed_time, 2_000).quality,
            TrajectoryQualityV1::Invalid
        );

        let invalid = vec![sample(1_000, 1, 1, 1.0), sample(2_000, 2, 2, f64::NAN)];
        assert_eq!(
            project(&invalid, 2_000).quality,
            TrajectoryQualityV1::Invalid
        );
    }

    #[test]
    fn peak_drawdown_time_and_velocity_reset_on_new_peak() {
        let giveback = project(&[sample(1_000, 1, 1, 1.0), sample(2_000, 2, 2, 0.8)], 2_000);
        assert_eq!(giveback.peak_mark_price_sol, Some(1.0));
        assert_eq!(giveback.drawdown_from_peak_bps, Some(2_000));
        assert_eq!(giveback.time_since_peak_ms, Some(1_000));
        assert_eq!(giveback.peak_giveback_velocity_bps_per_sec, Some(2_000));

        let new_peak = project(&[sample(1_000, 1, 1, 1.0), sample(2_000, 2, 2, 1.2)], 2_000);
        assert_eq!(new_peak.drawdown_from_peak_bps, Some(0));
        assert_eq!(new_peak.time_since_peak_ms, Some(0));
        assert_eq!(new_peak.peak_giveback_velocity_bps_per_sec, Some(0));
    }

    #[test]
    fn stale_newest_sample_is_not_usable() {
        let result = project(&[sample(1_000, 1, 1, 1.0)], 3_000);
        assert_eq!(result.quality, TrajectoryQualityV1::Stale);
        assert!(result
            .flags
            .contains(TrajectoryFlagsV1::STALE_NEWEST_SAMPLE));
    }
}
