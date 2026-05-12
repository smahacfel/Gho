use super::traits::FeatureMaterializer;
use super::types::{
    CheckpointDerivedFeatures, MaterializedFeatureSet, SessionCheckpoint, TrendDirection,
};
use crate::account_state_core::types::AccountStateFeatures;
use crate::session::types::SessionMetadata;
use crate::tx_intelligence::types::{RiskFlag, TxIntelFeatures};

const TREND_EPSILON: f64 = 0.01;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObservationFeatureBuilder;

impl ObservationFeatureBuilder {
    fn build_checkpoint_features(
        &self,
        account_features: &AccountStateFeatures,
        tx_intel_features: &TxIntelFeatures,
        checkpoints: &[SessionCheckpoint],
        risk_flags: &[RiskFlag],
    ) -> CheckpointDerivedFeatures {
        let price_trajectory: Vec<f64> = checkpoints
            .iter()
            .map(|checkpoint| checkpoint.account_state_snapshot.price_sol)
            .collect();
        let reserve_trajectory: Vec<(u64, u64)> = checkpoints
            .iter()
            .map(|checkpoint| checkpoint.account_state_snapshot.current_reserves)
            .collect();

        let buy_pressure_samples = append_current_sample(
            checkpoints
                .iter()
                .map(|checkpoint| checkpoint.tx_intel_snapshot.buy_ratio)
                .collect(),
            tx_intel_features.buy_ratio,
        );
        let signer_diversity_samples = append_current_sample(
            checkpoints
                .iter()
                .map(|checkpoint| checkpoint.tx_intel_snapshot.unique_signer_ratio)
                .collect(),
            tx_intel_features.unique_signer_ratio,
        );
        let risk_flag_count_samples = append_current_sample(
            checkpoints
                .iter()
                .map(|checkpoint| checkpoint.risk_flags.len() as f64)
                .collect(),
            risk_flags.len() as f64,
        );
        let price_impact_samples =
            append_current_sample(price_trajectory.clone(), account_features.price_sol);
        let reserve_samples = append_current_reserve_sample(
            reserve_trajectory.clone(),
            account_features.current_reserves,
        );

        CheckpointDerivedFeatures {
            price_trajectory,
            reserve_trajectory,
            buy_pressure_trend: derive_trend(&buy_pressure_samples, TREND_EPSILON),
            signer_diversity_trend: derive_trend(&signer_diversity_samples, TREND_EPSILON),
            risk_flag_count_trend: derive_trend(&risk_flag_count_samples, TREND_EPSILON),
            trajectory_checkpoint_count: checkpoints.len() as u32,
            price_change_from_first_checkpoint_pct: derive_price_change_from_first_checkpoint_pct(
                checkpoints,
                account_features.price_sol,
                account_features.price_change_since_t0_pct,
            ),
            single_tx_max_price_impact_pct: derive_max_price_impact_pct(&price_impact_samples),
            max_single_sell_impact_pct: derive_max_sell_impact_pct(
                &price_impact_samples,
                &reserve_samples,
            ),
            bonding_progress: account_features.bonding_progress,
            trajectory_assessment: None,
        }
    }
}

impl FeatureMaterializer for ObservationFeatureBuilder {
    fn materialize(
        &self,
        account_features: AccountStateFeatures,
        tx_intel_features: TxIntelFeatures,
        checkpoints: &[SessionCheckpoint],
        risk_flags: Vec<RiskFlag>,
        metadata: SessionMetadata,
    ) -> MaterializedFeatureSet {
        MaterializedFeatureSet {
            checkpoint_features: self.build_checkpoint_features(
                &account_features,
                &tx_intel_features,
                checkpoints,
                &risk_flags,
            ),
            account_features,
            tx_intel_features,
            risk_flags,
            session_metadata: metadata,
            curve_readiness: Default::default(),
            sybil_resistance: Default::default(),
            alpha_fingerprint: Default::default(),
            tx_segment_sequence: None,
        }
    }
}

fn append_current_sample(mut historical: Vec<f64>, current: f64) -> Vec<f64> {
    if historical
        .last()
        .map(|last| (last - current).abs() > f64::EPSILON)
        .unwrap_or(true)
    {
        historical.push(current);
    }
    historical
}

fn append_current_reserve_sample(
    mut historical: Vec<(u64, u64)>,
    current: (u64, u64),
) -> Vec<(u64, u64)> {
    if historical.last().copied() != Some(current) {
        historical.push(current);
    }
    historical
}

fn derive_price_change_from_first_checkpoint_pct(
    checkpoints: &[SessionCheckpoint],
    current_price: f64,
    fallback_pct: f64,
) -> f64 {
    let Some(first_checkpoint_price) = checkpoints
        .first()
        .map(|checkpoint| checkpoint.account_state_snapshot.price_sol)
    else {
        return fallback_pct;
    };

    if first_checkpoint_price > 0.0 {
        ((current_price - first_checkpoint_price) / first_checkpoint_price) * 100.0
    } else {
        fallback_pct
    }
}

fn derive_max_price_impact_pct(prices: &[f64]) -> f64 {
    prices
        .windows(2)
        .filter_map(|window| {
            let previous = window[0];
            let current = window[1];
            (previous > 0.0).then_some(((current - previous) / previous).abs() * 100.0)
        })
        .fold(0.0_f64, f64::max)
}

fn derive_max_sell_impact_pct(prices: &[f64], reserves: &[(u64, u64)]) -> f64 {
    if prices.len() < 2 || reserves.len() < 2 {
        return 0.0;
    }

    prices
        .windows(2)
        .zip(reserves.windows(2))
        .filter_map(|(price_window, reserve_window)| {
            let previous_price = price_window[0];
            let current_price = price_window[1];
            let (previous_sol, previous_token) = reserve_window[0];
            let (current_sol, current_token) = reserve_window[1];

            let looks_like_sell = current_sol < previous_sol && current_token > previous_token;
            if !looks_like_sell || previous_price <= 0.0 {
                return None;
            }

            Some(((current_price - previous_price) / previous_price).abs() * 100.0)
        })
        .fold(0.0_f64, f64::max)
}

fn derive_trend(samples: &[f64], epsilon: f64) -> TrendDirection {
    if samples.len() < 2 {
        return TrendDirection::Insufficient;
    }

    let first = samples[0];
    let last = samples[samples.len() - 1];
    let delta = last - first;
    if delta.abs() <= epsilon {
        TrendDirection::Stable
    } else if delta.is_sign_positive() {
        TrendDirection::Rising
    } else {
        TrendDirection::Falling
    }
}

#[cfg(test)]
mod tests {
    use super::{
        append_current_reserve_sample, append_current_sample, derive_max_price_impact_pct,
        derive_max_sell_impact_pct, derive_trend,
    };
    use crate::checkpoint::types::TrendDirection;

    #[test]
    fn append_current_sample_only_appends_when_changed() {
        assert_eq!(append_current_sample(vec![1.0], 1.0), vec![1.0]);
        assert_eq!(append_current_sample(vec![1.0], 1.5), vec![1.0, 1.5]);
    }

    #[test]
    fn append_current_reserve_sample_only_appends_when_changed() {
        assert_eq!(
            append_current_reserve_sample(vec![(100, 900)], (100, 900)),
            vec![(100, 900)]
        );
        assert_eq!(
            append_current_reserve_sample(vec![(100, 900)], (90, 910)),
            vec![(100, 900), (90, 910)]
        );
    }

    #[test]
    fn derive_max_price_impact_pct_uses_adjacent_points() {
        let impact = derive_max_price_impact_pct(&[1.0, 1.15, 0.92, 0.95]);
        assert!((impact - 20.0).abs() < 1e-9);
    }

    #[test]
    fn derive_max_sell_impact_pct_only_counts_sell_like_reserve_moves() {
        let impact = derive_max_sell_impact_pct(
            &[1.0, 1.2, 0.9, 0.8],
            &[(100, 900), (120, 880), (110, 890), (100, 900)],
        );
        assert!((impact - 25.0).abs() < 1e-9);
    }

    #[test]
    fn derive_trend_returns_insufficient_for_short_series() {
        assert_eq!(derive_trend(&[1.0], 0.01), TrendDirection::Insufficient);
    }
}
