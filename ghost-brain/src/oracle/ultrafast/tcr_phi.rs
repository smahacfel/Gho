#![allow(arithmetic_side_effects)]
#![allow(clippy::arithmetic_side_effects)]
//! TCR-Φ (Temporal Causality Resonance – Phi Edition)
//!
//! Detects **intentional steering** by measuring how tightly reactions align
//! in time and direction with a registered causal impact.
//!
//! Core outputs are gated and emitted only when confidence exceeds the
//! minimum threshold to avoid noise propagation.

use std::collections::VecDeque;

use metrics::{gauge, histogram, increment_counter};
use solana_sdk::pubkey::Pubkey;
use tracing::warn;

use super::ecto::{EctoFlags, EctoSignal};
pub use crate::config::TcrPhiConfig;
use crate::oracle::ultrafast::cir::BuySell;

// =============================================================================
// Configuration
// =============================================================================

impl TcrPhiConfig {
    fn sanitize(mut self) -> (Self, bool) {
        let mut violated = false;

        if self.min_samples < 2 {
            self.min_samples = 2;
            violated = true;
        }

        if self.confidence_samples < self.min_samples {
            self.confidence_samples = self.min_samples;
            violated = true;
        }

        let max_tempo = (self.window_size_slots.saturating_mul(5)).max(1) as usize;
        if self.tempo_window == 0 {
            self.tempo_window = 1;
            violated = true;
        }
        if self.tempo_window > max_tempo {
            self.tempo_window = max_tempo;
            violated = true;
        }

        if !self.default_slot_ms.is_finite() || self.default_slot_ms <= 0.0 {
            self.default_slot_ms = 400.0;
            violated = true;
        }

        self.min_confidence_emit = self.min_confidence_emit.clamp(0.0, 1.0);
        self.synergy_timing_threshold = self.synergy_timing_threshold.clamp(0.0, 1.0);
        self.synergy_bias_threshold = self.synergy_bias_threshold.clamp(0.0, 1.0);
        if !self.synergy_boost.is_finite() || self.synergy_boost <= 0.0 {
            self.synergy_boost = 1.0;
            violated = true;
        }

        if !(0.2..=0.5).contains(&self.ecto_k_phi) {
            self.ecto_k_phi = self.ecto_k_phi.clamp(0.2, 0.5);
            violated = true;
        }

        if !self.ecto_early_event_weight.is_finite() || self.ecto_early_event_weight < 1.0 {
            self.ecto_early_event_weight = 1.0;
            violated = true;
        }

        if self.ecto_early_window_slots == 0 {
            self.ecto_early_window_slots = 1;
            violated = true;
        }

        (self, violated)
    }
}

// =============================================================================
// Types
// =============================================================================

/// Impact event registered as a potential causal driver.
#[derive(Debug, Clone, Copy)]
pub struct TcrImpact {
    pub id: u64,
    pub slot: Option<u64>,
    pub timestamp_ms: Option<u64>,
    pub signer: Pubkey,
    pub side: BuySell,
}

/// Reaction event that may respond to an impact.
#[derive(Debug, Clone, Copy)]
pub struct TcrReaction {
    pub slot: Option<u64>,
    pub timestamp_ms: Option<u64>,
    pub signer: Pubkey,
    pub side: BuySell,
}

/// Emitted TCR-Φ score when confidence is sufficient.
#[derive(Debug, Clone, Copy)]
pub struct TcrScore {
    pub impact_id: u64,
    pub tcr_value: f64,
    pub timing_score: f64,
    pub directional_bias: f64,
    pub confidence: f64,
    pub variance_phi: f64,
    pub mean_phi: f64,
    pub sample_count: usize,
    pub buy_count: usize,
    pub sell_count: usize,
    pub last_update_slot: Option<u64>,
}

/// Causal continuity breakpoints (fed to PANIC later via runtime).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalBreak {
    GenesisCorruption,
}

#[derive(Debug, Clone)]
struct ImpactState {
    id: u64,
    slot: Option<u64>,
    timestamp_ms: Option<u64>,
    signer: Pubkey,
    is_buy: bool,
    stats: RunningStats,
    buy_count: usize,
    sell_count: usize,
    last_update_slot: Option<u64>,
}

impl ImpactState {
    fn new(impact: TcrImpact) -> Self {
        Self {
            id: impact.id,
            slot: impact.slot,
            timestamp_ms: impact.timestamp_ms,
            signer: impact.signer,
            is_buy: impact.side == BuySell::Buy,
            stats: RunningStats::default(),
            buy_count: 0,
            sell_count: 0,
            last_update_slot: impact.slot,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RunningStats {
    count: usize,
    mean: f64,
    m2: f64,
}

impl RunningStats {
    fn push(&mut self, value: f64) {
        let count = self.count + 1;
        let delta = value - self.mean;
        let mean = self.mean + delta / count as f64;
        let delta2 = value - mean;
        let m2 = self.m2 + delta * delta2;

        self.count = count;
        self.mean = mean;
        self.m2 = m2;
    }

    fn variance(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.m2 / self.count as f64
        }
    }
}

#[derive(Debug, Clone)]
struct SlotTempo {
    samples: VecDeque<f64>,
    max_samples: usize,
    last_ts: Option<u64>,
    default_slot_ms: f64,
}

impl SlotTempo {
    fn new(max_samples: usize, default_slot_ms: f64) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples.max(1)),
            max_samples: max_samples.max(1),
            last_ts: None,
            default_slot_ms,
        }
    }

    fn observe(&mut self, timestamp_ms: Option<u64>) {
        if let (Some(last_ts), Some(ts)) = (self.last_ts, timestamp_ms) {
            if ts > last_ts {
                let delta_ms = ts - last_ts;
                if delta_ms > 0 {
                    let delta_ms_f = delta_ms as f64;
                    if delta_ms_f.is_finite() && delta_ms_f > 0.0 {
                        self.samples.push_back(delta_ms_f);
                        while self.samples.len() > self.max_samples {
                            self.samples.pop_front();
                        }
                    }
                }
            }
        }

        if timestamp_ms.is_some() {
            self.last_ts = timestamp_ms;
        }
    }

    fn median_ms(&self) -> f64 {
        if self.samples.is_empty() {
            return self.default_slot_ms;
        }

        let mut buf: Vec<f64> = self.samples.iter().copied().collect();
        buf.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mid = buf.len() / 2;
        if buf.len() % 2 == 0 {
            ((buf[mid - 1] + buf[mid]) * 0.5).max(1.0)
        } else {
            buf[mid].max(1.0)
        }
    }
}

// =============================================================================
// Core
// =============================================================================

/// Stateful TCR-Φ engine.
pub struct TcrPhiCore {
    config: TcrPhiConfig,
    impacts: VecDeque<ImpactState>,
    tempo: SlotTempo,
    contract_violated: bool,
    ecto_phi_modifier: f64,
    early_event_weight: f64,
    last_causal_break: Option<CausalBreak>,
    last_ecto_flags: EctoFlags,
}

impl TcrPhiCore {
    pub fn new(config: TcrPhiConfig) -> Self {
        let (config, violated) = config.sanitize();
        if violated {
            warn!(
                "TCR-Φ config contract violated; sanitized to safe values (min_samples>=2, tempo_window<=5x window_size)"
            );
            increment_counter!("tcr_phi_config_contract_violations_total");
        }
        let tempo = SlotTempo::new(config.tempo_window, config.default_slot_ms);
        Self {
            config,
            impacts: VecDeque::new(),
            tempo,
            contract_violated: violated,
            ecto_phi_modifier: 1.0,
            early_event_weight: 1.0,
            last_causal_break: None,
            last_ecto_flags: EctoFlags::empty(),
        }
    }

    pub fn apply_ecto_signal(&mut self, signal: &EctoSignal) {
        if !self.config.ecto_enabled {
            return;
        }

        let modifier = 1.0 + (signal.bias * signal.confidence * self.config.ecto_k_phi);
        if modifier.is_finite() && modifier > 0.0 {
            self.ecto_phi_modifier = modifier;
        }

        self.last_ecto_flags = signal.flags;
        self.early_event_weight = 1.0;

        if signal.flags.contains(EctoFlags::DEV_SOLD) {
            self.mark_causal_break(CausalBreak::GenesisCorruption);
        }

        if signal.flags.contains(EctoFlags::SNIPER_WALL) {
            self.increase_early_event_weight();
        }
    }

    pub fn mark_causal_break(&mut self, causal_break: CausalBreak) {
        self.last_causal_break = Some(causal_break);
    }

    pub fn last_causal_break(&self) -> Option<CausalBreak> {
        self.last_causal_break
    }

    pub fn phi_modifier(&self) -> f64 {
        self.ecto_phi_modifier
    }

    pub fn ecto_flags(&self) -> EctoFlags {
        self.last_ecto_flags
    }

    pub fn increase_early_event_weight(&mut self) {
        self.early_event_weight = self.config.ecto_early_event_weight.max(1.0);
    }

    pub fn register_impact(&mut self, impact: TcrImpact) {
        self.tempo.observe(impact.timestamp_ms);
        if let Some(ts_ms) = impact.timestamp_ms {
            self.evict_stale(ts_ms);
        }

        if self.impacts.iter().any(|i| i.id == impact.id) {
            return;
        }

        if self.impacts.len() >= self.config.max_impacts {
            self.impacts.pop_front();
        }

        self.impacts.push_back(ImpactState::new(impact));
    }

    /// Process a reaction against all relevant impacts.
    pub fn process_reaction(&mut self, reaction: TcrReaction) -> Vec<TcrScore> {
        self.tempo.observe(reaction.timestamp_ms);
        if let Some(ts_ms) = reaction.timestamp_ms {
            self.evict_stale(ts_ms);
        }

        let config = self.config.clone();
        let tempo_ms = self.tempo.median_ms();
        let phi_modifier = self.ecto_phi_modifier;
        let early_window_ms = (self.config.ecto_early_window_slots as f64
            * self.config.default_slot_ms)
            .round()
            .max(1.0);
        let early_weight = self.early_event_weight;
        let window_ms = (self.config.window_size_slots as f64 * self.config.default_slot_ms)
            .round()
            .max(1.0);

        let mut emitted = Vec::new();
        for impact in self.impacts.iter_mut() {
            if reaction.signer == impact.signer {
                continue;
            }
            let (impact_ts, reaction_ts) = match (impact.timestamp_ms, reaction.timestamp_ms) {
                (Some(impact_ts), Some(reaction_ts)) => (impact_ts, reaction_ts),
                _ => continue,
            };
            if reaction_ts <= impact_ts {
                continue;
            }
            let delta_ms = (reaction_ts - impact_ts) as f64;
            if delta_ms > window_ms {
                continue;
            }
            let mut phi = (delta_ms / tempo_ms).max(0.0);
            if phi_modifier > 0.0 {
                phi /= phi_modifier;
            }
            if delta_ms <= early_window_ms {
                phi *= early_weight;
            }
            if !phi.is_finite() {
                continue;
            }

            if phi.is_finite() {
                impact.stats.push(phi);
                impact.last_update_slot = reaction.slot;
                if reaction.side == BuySell::Buy {
                    impact.buy_count += 1;
                } else {
                    impact.sell_count += 1;
                }

                let score = Self::score_for_impact_with_config(&config, impact);
                if score.confidence >= self.config.min_confidence_emit {
                    emitted.push(score);
                }
            }
        }

        emitted
    }

    /// Compute a score snapshot for a single impact.
    pub fn score_for_impact_id(&self, impact_id: u64) -> Option<TcrScore> {
        self.impacts
            .iter()
            .find(|impact| impact.id == impact_id)
            .map(|impact| Self::score_for_impact_with_config(&self.config, impact))
    }

    /// Whether the config contract had to be sanitized.
    pub fn contract_violated(&self) -> bool {
        self.contract_violated
    }

    fn score_for_impact_with_config(config: &TcrPhiConfig, impact: &ImpactState) -> TcrScore {
        let n = impact.stats.count;
        let variance = impact.stats.variance();
        let mut timing_score = if variance.is_finite() {
            1.0 / (1.0 + variance)
        } else {
            0.0
        };

        let total = impact.buy_count + impact.sell_count;
        let directional_bias = if total > 0 {
            let aligned = if impact.is_buy {
                impact.buy_count
            } else {
                impact.sell_count
            };
            aligned as f64 / total as f64
        } else {
            0.0
        };

        let mut confidence = if config.confidence_samples > 0 {
            (n as f64 / config.confidence_samples as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        if n < config.min_samples {
            timing_score *= confidence.sqrt();
            increment_counter!("tcr_phi_small_n_dampening_total");
        }

        let mut tcr_value = timing_score * directional_bias * confidence;

        if timing_score > config.synergy_timing_threshold
            && directional_bias > config.synergy_bias_threshold
        {
            tcr_value = (tcr_value * config.synergy_boost).min(1.0);
        }

        if !tcr_value.is_finite() {
            tcr_value = 0.0;
            confidence = 0.0;
            timing_score = 0.0;
        }

        histogram!("tcr_phi_value_histogram", tcr_value);
        gauge!("tcr_phi_confidence", confidence);

        TcrScore {
            impact_id: impact.id,
            tcr_value: tcr_value.clamp(0.0, 1.0),
            timing_score: timing_score.clamp(0.0, 1.0),
            directional_bias: directional_bias.clamp(0.0, 1.0),
            confidence,
            variance_phi: variance.max(0.0),
            mean_phi: impact.stats.mean,
            sample_count: n,
            buy_count: impact.buy_count,
            sell_count: impact.sell_count,
            last_update_slot: impact.last_update_slot,
        }
    }

    fn evict_stale(&mut self, current_ts_ms: u64) {
        let window_ms = (self.config.window_size_slots as f64 * self.config.default_slot_ms)
            .round()
            .max(1.0) as u64;
        while let Some(front) = self.impacts.front() {
            let evict = match front.timestamp_ms {
                Some(ts_ms) => current_ts_ms.saturating_sub(ts_ms) > window_ms,
                None => false,
            };
            if evict {
                self.impacts.pop_front();
                continue;
            }
            break;
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PanicConfig;
    use crate::oracle::ultrafast::ecto::{EctoFlags, EctoSignal};
    use crate::oracle::ultrafast::panic::{PanicState, PanicTx};

    fn pk(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    #[test]
    fn small_n_dampens_timing() {
        let mut config = TcrPhiConfig::default();
        config.min_samples = 4;
        config.confidence_samples = 4;
        config.min_confidence_emit = 0.0;

        let mut core = TcrPhiCore::new(config);
        core.register_impact(TcrImpact {
            id: 1,
            slot: Some(10),
            timestamp_ms: Some(10_000),
            signer: pk(1),
            side: BuySell::Buy,
        });

        let scores = core.process_reaction(TcrReaction {
            slot: Some(11),
            timestamp_ms: Some(10_400),
            signer: pk(2),
            side: BuySell::Buy,
        });

        assert!(!scores.is_empty());
        let score = scores[0];
        assert!(score.timing_score < 1.0);
        assert!(score.tcr_value < 0.5);
    }

    #[test]
    fn synergy_applies_only_on_strong_timing_and_bias() {
        let mut config = TcrPhiConfig::default();
        config.min_samples = 2;
        config.confidence_samples = 2;
        config.min_confidence_emit = 0.0;

        let mut core = TcrPhiCore::new(config);
        core.register_impact(TcrImpact {
            id: 2,
            slot: Some(20),
            timestamp_ms: Some(20_000),
            signer: pk(3),
            side: BuySell::Buy,
        });

        for i in 0..3u64 {
            let _ = core.process_reaction(TcrReaction {
                slot: Some(21 + i),
                timestamp_ms: Some(20_400 + i * 400),
                signer: pk((10 + i) as u8),
                side: BuySell::Buy,
            });
        }

        let score = core.score_for_impact_id(2).unwrap();
        assert!(score.timing_score > 0.75);
        assert!(score.directional_bias > 0.8);
        assert!(score.tcr_value > 0.5);
    }

    #[test]
    fn emit_requires_min_confidence() {
        let mut config = TcrPhiConfig::default();
        config.min_samples = 2;
        config.confidence_samples = 10;
        config.min_confidence_emit = 0.5;

        let mut core = TcrPhiCore::new(config);
        core.register_impact(TcrImpact {
            id: 3,
            slot: Some(30),
            timestamp_ms: Some(30_000),
            signer: pk(4),
            side: BuySell::Sell,
        });

        let scores = core.process_reaction(TcrReaction {
            slot: Some(31),
            timestamp_ms: Some(30_400),
            signer: pk(5),
            side: BuySell::Sell,
        });

        assert!(scores.is_empty());
        let score = core.score_for_impact_id(3).unwrap();
        assert!(score.confidence < 0.5);
    }

    #[test]
    fn ecto_disabled_keeps_tcr_and_panic_identical() {
        let mut config = TcrPhiConfig::default();
        config.ecto_enabled = false;
        config.min_samples = 2;
        config.confidence_samples = 2;
        config.min_confidence_emit = 0.0;

        let mut baseline = TcrPhiCore::new(config.clone());
        let mut ecto_core = TcrPhiCore::new(config);

        let ecto_signal = EctoSignal {
            bias: -1.0,
            score: 0.0,
            confidence: 1.0,
            flags: EctoFlags::DEV_SOLD,
            window_ms: 2_000,
            verdict: crate::oracle::ultrafast::EctoVerdict::Rug,
        };
        ecto_core.apply_ecto_signal(&ecto_signal);

        let impact = TcrImpact {
            id: 9,
            slot: Some(42),
            timestamp_ms: Some(42_000),
            signer: pk(9),
            side: BuySell::Buy,
        };
        baseline.register_impact(impact);
        ecto_core.register_impact(impact);

        let reaction = TcrReaction {
            slot: Some(43),
            timestamp_ms: Some(42_400),
            signer: pk(10),
            side: BuySell::Buy,
        };
        let base_scores = baseline.process_reaction(reaction);
        let ecto_scores = ecto_core.process_reaction(reaction);

        assert_eq!(base_scores.len(), ecto_scores.len());
        let base = base_scores[0];
        let ecto = ecto_scores[0];
        let eps = 1e-12;
        assert!((base.tcr_value - ecto.tcr_value).abs() < eps);
        assert!((base.confidence - ecto.confidence).abs() < eps);
        assert!((base.timing_score - ecto.timing_score).abs() < eps);
        assert!((base.directional_bias - ecto.directional_bias).abs() < eps);
        assert!(ecto_core.last_causal_break().is_none());
        assert!((ecto_core.phi_modifier() - 1.0).abs() < eps);

        let panic_config = PanicConfig::default();
        let mut panic_a = PanicState::new();
        let mut panic_b = PanicState::new();
        let tx = PanicTx {
            slot: Some(42),
            arrival_ts_ms: 42_500,
            event_time: ghost_core::EventTimeMetadata::default(),
            impulse_weight: 1.0,
            requested_sol_amount: 1.2,
            executed_sol_amount: 1.1,
            priority_fee_micro_lamports: 1_000,
            success: true,
            signer: pk(11),
        };
        panic_a.update(tx);
        panic_b.update(tx);

        let mut out_a = panic_a.calculate_score(&panic_config);
        let mut out_b = panic_b.calculate_score(&panic_config);

        out_a.tcr_value = Some(base.tcr_value);
        out_a.tcr_confidence = Some(base.confidence);
        out_a.tcr_directional_bias = Some(base.directional_bias);
        out_a.tcr_variance = Some(base.variance_phi);

        out_b.tcr_value = Some(ecto.tcr_value);
        out_b.tcr_confidence = Some(ecto.confidence);
        out_b.tcr_directional_bias = Some(ecto.directional_bias);
        out_b.tcr_variance = Some(ecto.variance_phi);

        assert!((out_a.score - out_b.score).abs() < eps);
        assert!((out_a.confidence - out_b.confidence).abs() < eps);
        assert_eq!(out_a.tcr_value, out_b.tcr_value);
        assert_eq!(out_a.tcr_confidence, out_b.tcr_confidence);
        assert_eq!(out_a.tcr_directional_bias, out_b.tcr_directional_bias);
        assert_eq!(out_a.tcr_variance, out_b.tcr_variance);
    }
}
