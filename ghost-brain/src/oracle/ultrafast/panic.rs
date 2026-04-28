#![allow(arithmetic_side_effects)]
#![allow(clippy::arithmetic_side_effects)]

use std::collections::VecDeque;

use ghost_core::EventTimeMetadata;
use rustc_hash::FxHashMap;
use solana_sdk::pubkey::Pubkey;

use crate::config::PanicConfig;

/// P.A.N.I.C. is active only in the 0-7s window from the first observed trade.
const PANIC_ACTIVE_WINDOW_MS: u64 = 7_000;

/// Time-stamped trade used for PANIC heuristics.
#[derive(Debug, Clone, Copy)]
pub struct PanicTx {
    pub slot: Option<u64>,
    pub arrival_ts_ms: u64,
    /// Additive event/ingest provenance carried for downstream clock selection.
    pub event_time: EventTimeMetadata,
    pub impulse_weight: f64,
    pub requested_sol_amount: f64,
    pub executed_sol_amount: f64,
    pub priority_fee_micro_lamports: u64,
    pub success: bool,
    pub signer: Pubkey,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PanicOutput {
    pub score: f64,
    pub confidence: f64,
    pub pressure: f64,
    pub friction: f64,
    pub fee_spike: f64,
    pub impulse_score: f64,
    pub entropy_score: f64,
    pub tx_count: usize,
    pub unique_signers: usize,
    pub is_high_pressure: bool,
    pub is_bot_spam: bool,
    pub mute_sobp: bool,
    pub mute_scr: bool,
    pub tcr_value: Option<f64>,
    pub tcr_confidence: Option<f64>,
    pub tcr_directional_bias: Option<f64>,
    pub tcr_variance: Option<f64>,
}

/// PANIC state for CIR-scaled density/pressure detection.
pub struct PanicState {
    history: VecDeque<PanicTx>,
    signer_counts: FxHashMap<Pubkey, u32>, // zastąpić unique_signers_estimate (HyperLogLog / bitmap / cap) jeśli planowany jest agresywny Geysyser feed +100 tx/s
    first_seen_ts_ms: Option<u64>,
    last_seen_ts_ms: Option<u64>,
    failed_tx_density: f64,
    success_tx_density: f64,
    latent_volume_sol: f64,
    realized_volume_sol: f64,
    fee_weighted_sum: f64,
    fee_weighted_count: f64,
    last_fee_avg: f64,
    last_fee_spike: f64,
}

impl PanicTx {
    /// Observation wall-clock used by downstream consumers that operate on
    /// runtime-observed windows rather than strict chain event time.
    #[inline]
    pub fn observation_ts_ms(&self) -> Option<u64> {
        self.event_time
            .ingress_wall_ts_ms
            .or(self.event_time.chain_event_ts_ms)
    }
}

#[allow(arithmetic_side_effects)]
#[allow(clippy::arithmetic_side_effects)]
impl PanicState {
    pub fn new() -> Self {
        Self {
            history: VecDeque::new(),
            signer_counts: FxHashMap::default(),
            first_seen_ts_ms: None,
            last_seen_ts_ms: None,
            failed_tx_density: 0.0,
            success_tx_density: 0.0,
            latent_volume_sol: 0.0,
            realized_volume_sol: 0.0,
            fee_weighted_sum: 0.0,
            fee_weighted_count: 0.0,
            last_fee_avg: 0.0,
            last_fee_spike: 0.0,
        }
    }

    #[allow(arithmetic_side_effects)]
    #[allow(clippy::arithmetic_side_effects)]
    pub fn update(&mut self, tx: PanicTx) -> bool {
        if self.first_seen_ts_ms.is_none() {
            self.first_seen_ts_ms = Some(tx.arrival_ts_ms);
        }
        let first_seen = self.first_seen_ts_ms.unwrap_or(tx.arrival_ts_ms);
        let age_ms = tx.arrival_ts_ms.saturating_sub(first_seen);
        if age_ms > PANIC_ACTIVE_WINDOW_MS {
            return false;
        }

        let impulse_weight = tx.impulse_weight.max(0.0);
        let entry = PanicTx {
            impulse_weight,
            ..tx
        };

        self.last_seen_ts_ms = Some(tx.arrival_ts_ms);
        self.history.push_back(entry);
        let signer_count = self.signer_counts.entry(entry.signer).or_insert(0);
        *signer_count = signer_count.saturating_add(1);

        if entry.success {
            self.success_tx_density += impulse_weight;
        } else {
            self.failed_tx_density += impulse_weight;
        }

        self.latent_volume_sol += entry.requested_sol_amount * impulse_weight;
        self.realized_volume_sol += entry.executed_sol_amount * impulse_weight;

        let fee_weighted = (entry.priority_fee_micro_lamports as f64) * impulse_weight;
        self.fee_weighted_sum += fee_weighted;
        self.fee_weighted_count += impulse_weight;

        self.prune(tx.arrival_ts_ms);
        self.update_fee_spike();
        true
    }

    #[allow(arithmetic_side_effects)]
    #[allow(clippy::arithmetic_side_effects)]
    fn prune(&mut self, now_ms: u64) {
        while let Some(front) = self.history.front() {
            if now_ms.saturating_sub(front.arrival_ts_ms) <= PANIC_ACTIVE_WINDOW_MS {
                break;
            }

            if let Some(entry) = self.history.pop_front() {
                let impulse_weight = entry.impulse_weight;
                if let Some(count) = self.signer_counts.get_mut(&entry.signer) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        self.signer_counts.remove(&entry.signer);
                    }
                }
                if entry.success {
                    self.success_tx_density = (self.success_tx_density - impulse_weight).max(0.0);
                } else {
                    self.failed_tx_density = (self.failed_tx_density - impulse_weight).max(0.0);
                }

                self.latent_volume_sol =
                    (self.latent_volume_sol - entry.requested_sol_amount * impulse_weight).max(0.0);
                self.realized_volume_sol = (self.realized_volume_sol
                    - entry.executed_sol_amount * impulse_weight)
                    .max(0.0);

                let fee_weighted = (entry.priority_fee_micro_lamports as f64) * impulse_weight;
                self.fee_weighted_sum = (self.fee_weighted_sum - fee_weighted).max(0.0);
                self.fee_weighted_count = (self.fee_weighted_count - impulse_weight).max(0.0);
            }
        }
    }

    #[allow(arithmetic_side_effects)]
    #[allow(clippy::arithmetic_side_effects)]
    fn update_fee_spike(&mut self) {
        let current_avg = if self.fee_weighted_count > 0.0 {
            self.fee_weighted_sum / self.fee_weighted_count
        } else {
            0.0
        };

        self.last_fee_spike = if self.last_fee_avg > 0.0 {
            ((current_avg - self.last_fee_avg) / self.last_fee_avg).max(0.0)
        } else {
            0.0
        };

        self.last_fee_avg = current_avg;
    }

    #[allow(arithmetic_side_effects)]
    #[allow(clippy::arithmetic_side_effects)]
    pub fn calculate_score(&self, config: &PanicConfig) -> PanicOutput {
        let Some(now_ms) = self.last_seen_ts_ms else {
            return PanicOutput::default();
        };

        let Some(first_ts) = self.first_seen_ts_ms else {
            return PanicOutput::default();
        };

        if now_ms.saturating_sub(first_ts) > PANIC_ACTIVE_WINDOW_MS {
            return PanicOutput::default();
        }

        let total_density = self.failed_tx_density + self.success_tx_density;
        if total_density == 0.0 {
            return PanicOutput::default();
        }

        let tx_count = self.history.len();
        if tx_count == 0 {
            return PanicOutput::default();
        }

        let impulse_score = tx_count as f64 / config.impulse_threshold_txps.max(1) as f64;
        let unique_signers = self.signer_counts.len();
        let mut entropy_score = unique_signers as f64 / tx_count as f64; // zastąpić unique_signers_estimate (HyperLogLog / bitmap / cap) jeśli planowany jest agresywny Geysyser feed +100 tx/s
        if unique_signers < config.min_unique_signers {
            entropy_score = 0.0;
        }

        let epsilon = 0.001;
        let pressure = self.latent_volume_sol / self.realized_volume_sol.max(epsilon);
        let pressure_score = (pressure.min(3.0) / 3.0).clamp(0.0, 1.0);
        let friction = (self.failed_tx_density / total_density).clamp(0.0, 1.0);
        let fee_spike = self.last_fee_spike.min(1.0);

        let raw_score = (pressure_score * 0.5) + (friction * 0.3) + (fee_spike * 0.2);
        let score = (raw_score * entropy_score).min(1.0);

        let density_factor =
            (total_density / config.min_pressure_for_confidence.max(0.001)).min(1.0);
        let entropy_factor = entropy_score.clamp(0.0, 1.0);
        let impulse_factor = impulse_score.min(1.0);
        let mut confidence = (density_factor * entropy_factor * impulse_factor).min(1.0);
        if entropy_score == 0.0 {
            confidence = confidence.min(config.zero_entropy_confidence_cap);
        }
        if unique_signers < config.min_unique_signers {
            confidence = confidence.min(config.low_signer_confidence_cap);
        }

        let is_high_pressure = pressure >= config.demand_spring_threshold
            && friction >= config.high_friction_threshold
            && impulse_score >= 1.0
            && entropy_score >= config.entropy_threshold;
        let is_bot_spam = pressure >= config.demand_spring_threshold
            && friction >= config.high_friction_threshold
            && impulse_score >= 1.0
            && entropy_score < config.entropy_threshold;

        let mute_sobp = entropy_score >= config.entropy_threshold
            && score > config.mute_sobp_score_threshold
            && confidence > config.mute_sobp_confidence_threshold;
        let mute_scr = entropy_score >= config.entropy_threshold
            && !is_bot_spam
            && score > config.mute_scr_score_threshold
            && friction > config.mute_scr_friction_threshold;

        PanicOutput {
            score,
            confidence,
            pressure,
            friction,
            fee_spike,
            impulse_score,
            entropy_score,
            tx_count,
            unique_signers,
            is_high_pressure,
            is_bot_spam,
            mute_sobp,
            mute_scr,
            tcr_value: None,
            tcr_confidence: None,
            tcr_directional_bias: None,
            tcr_variance: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx(ts: u64, signer: Pubkey, success: bool, requested: f64, executed: f64) -> PanicTx {
        PanicTx {
            slot: Some(1),
            arrival_ts_ms: ts,
            event_time: EventTimeMetadata::default(),
            impulse_weight: 1.0,
            requested_sol_amount: requested,
            executed_sol_amount: executed,
            priority_fee_micro_lamports: 1_000,
            success,
            signer,
        }
    }

    #[test]
    fn observation_ts_prefers_ingress_wall_over_arrival() {
        let tx = PanicTx {
            slot: Some(1),
            arrival_ts_ms: 9_000,
            event_time: EventTimeMetadata::new(Some(1_000), Some(2_000), Some(9_000)),
            impulse_weight: 1.0,
            requested_sol_amount: 1.0,
            executed_sol_amount: 1.0,
            priority_fee_micro_lamports: 1_000,
            success: true,
            signer: Pubkey::new_unique(),
        };

        assert_eq!(tx.observation_ts_ms(), Some(2_000));
    }

    #[test]
    fn observation_ts_falls_back_to_chain_when_ingress_missing() {
        let tx = PanicTx {
            slot: Some(1),
            arrival_ts_ms: 9_000,
            event_time: EventTimeMetadata::new(Some(1_000), None, Some(9_000)),
            impulse_weight: 1.0,
            requested_sol_amount: 1.0,
            executed_sol_amount: 1.0,
            priority_fee_micro_lamports: 1_000,
            success: true,
            signer: Pubkey::new_unique(),
        };

        assert_eq!(tx.observation_ts_ms(), Some(1_000));
    }

    #[test]
    fn test_bot_spam_does_not_boost() {
        let config = PanicConfig::default();
        let mut state = PanicState::new();
        let signer = Pubkey::new_unique();

        for i in 0..config.impulse_threshold_txps {
            let _ = state.update(make_tx(1000 + i as u64, signer, false, 1.0, 0.0));
        }

        let out = state.calculate_score(&config);
        assert!(out.is_bot_spam);
        assert!(!out.is_high_pressure);
        assert_eq!(out.entropy_score, 0.0);
        assert!(out.confidence <= config.zero_entropy_confidence_cap);
        assert!(!out.mute_scr);
    }

    #[test]
    fn test_crowd_burst_boosts() {
        let config = PanicConfig::default();
        let mut state = PanicState::new();

        let failed_count = (config.impulse_threshold_txps * 3) / 5; // 60% failed to exceed friction threshold
        for i in 0..config.impulse_threshold_txps {
            let success = i >= failed_count;
            let executed = if success { 1.0 } else { 0.0 };
            let _ = state.update(make_tx(
                1000 + i as u64,
                Pubkey::new_unique(),
                success,
                2.0,
                executed,
            ));
        }

        let out = state.calculate_score(&config);
        assert!(out.is_high_pressure);
        assert!(!out.is_bot_spam);
        assert!(out.entropy_score >= config.entropy_threshold);
        assert!(out.confidence > 0.7);
    }

    #[test]
    fn test_mixed_case_not_bot_spam() {
        let config = PanicConfig::default();
        let mut state = PanicState::new();
        let bot = Pubkey::new_unique();

        for i in 0..5u64 {
            let _ = state.update(make_tx(1000 + i, bot, false, 1.0, 0.0));
        }
        for i in 0..15u64 {
            let _ = state.update(make_tx(2000 + i, Pubkey::new_unique(), true, 2.0, 1.0));
        }

        let out = state.calculate_score(&config);
        assert!(!out.is_bot_spam);
        assert!(out.entropy_score >= config.entropy_threshold);
    }
}
