//! BVA (Behavioral Vacuum Analysis)
//!
//! Bootstrapped Value Assessment for the first 0-7 seconds of a token’s life.
//! Uses on-chain behavioral metadata only (no price/reserves/microstructure).
//! Designed as an early behavioral filter, not a final decision engine.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::config::BvaConfig;
use crate::oracle::snapshot_engine::TransactionRecord;
use crate::oracle::ultrafast::cir::CirEmittedTx;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BvaClassification {
    Organic,
    Steered,
    Chaotic,
    Dormant,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BvaMetrics {
    pub tds: f64,
    pub dc: f64,
    pub se: f64,
    pub cer: f64,
    pub erp: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BvaOutput {
    /// Final score (0.0-1.0)
    pub score: f64,
    /// Confidence in the score (0.0-1.0)
    pub confidence: f64,
    /// Behavior classification
    pub classification: BvaClassification,
    /// Raw metric values
    pub metrics: BvaMetrics,
}

#[derive(Debug, Clone)]
pub struct BvaState {
    pub birth_slot: Option<u64>,
    pub birth_ts_ms: u64,
    pub tx_count_total: usize,
    pub unique_signers: HashSet<Pubkey>,
    pub signer_counts: HashMap<Pubkey, usize>,
    pub inter_tx_deltas_ms: Vec<u64>,
    pub direction_sequence: Vec<bool>,
    pub early_reaction_count: usize,
    pub cir_echo_count: usize,
    pub cir_emitted_total: usize,
    pub cir_emitted_weighted_total: f64,
    pub cir_echo_weighted_sum: f64,
    pub last_update_slot: Option<u64>,
    pub last_update_ts_ms: u64,
    last_tx_ts_ms: Option<u64>,
    processed_keys: HashSet<u64>,
    sobp_ratio: Option<f64>,
    sobp_fallback_used: bool,
    congestion_flag: bool,
}

impl BvaState {
    pub fn new(birth_slot: Option<u64>, birth_ts_ms: u64) -> Self {
        Self {
            birth_slot,
            birth_ts_ms,
            tx_count_total: 0,
            unique_signers: HashSet::new(),
            signer_counts: HashMap::new(),
            inter_tx_deltas_ms: Vec::with_capacity(32),
            direction_sequence: Vec::with_capacity(32),
            early_reaction_count: 0,
            cir_echo_count: 0,
            cir_emitted_total: 0,
            cir_emitted_weighted_total: 0.0,
            cir_echo_weighted_sum: 0.0,
            last_update_slot: birth_slot,
            last_update_ts_ms: birth_ts_ms,
            last_tx_ts_ms: None,
            processed_keys: HashSet::with_capacity(64),
            sobp_ratio: None,
            sobp_fallback_used: false,
            congestion_flag: false,
        }
    }

    pub fn update_congestion_flag(&mut self, flag: bool) {
        self.congestion_flag = flag;
    }

    pub fn update_sobp(&mut self, ratio: Option<f64>, fallback_used: bool) {
        self.sobp_ratio = ratio;
        self.sobp_fallback_used = fallback_used;
    }

    pub fn process_tx(&mut self, tx: &TransactionRecord, config: &BvaConfig) {
        let key = tx_key_hash(tx);
        if !self.processed_keys.insert(key) {
            return;
        }

        if let Some(last_ts) = self.last_tx_ts_ms {
            let delta = tx.timestamp_ms.saturating_sub(last_ts);
            self.inter_tx_deltas_ms.push(delta.max(1));
        }
        let early_window_ms = config
            .early_reaction_slots
            .saturating_mul(config.slot_duration_ms);
        if tx.timestamp_ms.saturating_sub(self.birth_ts_ms) <= early_window_ms {
            self.early_reaction_count += 1;
        }

        self.tx_count_total = self.tx_count_total.saturating_add(1);
        self.unique_signers.insert(tx.signer);
        *self.signer_counts.entry(tx.signer).or_insert(0) += 1;
        self.direction_sequence.push(tx.is_buy);

        self.last_tx_ts_ms = Some(tx.timestamp_ms);
        self.last_update_slot = tx.slot;
        self.last_update_ts_ms = tx.timestamp_ms;
    }

    pub fn register_cir_emitted(&mut self, emitted: &CirEmittedTx, config: &BvaConfig) {
        self.cir_emitted_total = self.cir_emitted_total.saturating_add(1);
        let window_ms = config.primary_window_secs.saturating_mul(1000).max(1);
        let max_delta_ms = ((1.0 - config.cir_min_weight.clamp(0.0, 1.0)) * window_ms as f64)
            .round()
            .max(0.0) as u64;
        let delta_ms = emitted
            .timestamp_ms
            .saturating_sub(self.birth_ts_ms)
            .min(max_delta_ms.max(1));
        let weight = 1.0 - (delta_ms as f64 / window_ms as f64);
        self.cir_emitted_weighted_total += weight;
        if emitted.sustained_reactions > 0.0 {
            self.cir_echo_count = self.cir_echo_count.saturating_add(1);
            self.cir_echo_weighted_sum += weight;
        }
    }
}

#[derive(Debug, Clone)]
pub struct BvaAnalyzer {
    config: BvaConfig,
}

impl BvaAnalyzer {
    pub fn new(config: BvaConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &BvaConfig {
        &self.config
    }

    pub fn analyze(&self, state: &BvaState, now_ts_ms: u64) -> BvaOutput {
        let tds = self.temporal_density_score(state);
        let dc = self.directional_coherence(state);
        let se = self.signer_entropy(state);
        let cer = self.cir_echo_ratio(state);
        let erp = self.early_reaction_pressure(state);

        let mut score = (self.config.weight_tds * tds)
            + (self.config.weight_dc * dc)
            + (self.config.weight_se * se)
            + (self.config.weight_cer * cer)
            + (self.config.weight_erp * erp);

        score = score.clamp(0.0, 1.0);

        let confidence = self.confidence(state, now_ts_ms);

        let classification = self.classify(state, tds, dc, se, erp, confidence);

        BvaOutput {
            score,
            confidence,
            classification,
            metrics: BvaMetrics {
                tds,
                dc,
                se,
                cer,
                erp,
            },
        }
    }

    fn temporal_density_score(&self, state: &BvaState) -> f64 {
        let baseline_ms = self.config.slot_duration_ms.max(1) as f64;
        let median_inter =
            median_u64(&state.inter_tx_deltas_ms).unwrap_or(self.config.slot_duration_ms) as f64;
        let normalized = (median_inter / baseline_ms).clamp(0.0, 1.0);
        (1.0 - normalized).clamp(0.0, 1.0)
    }

    fn directional_coherence(&self, state: &BvaState) -> f64 {
        let total = state.direction_sequence.len() as f64;
        if total == 0.0 {
            return 0.0;
        }
        let buys = state.direction_sequence.iter().filter(|v| **v).count() as f64;
        let sells = total - buys;
        let raw_dc = ((buys - sells).abs() / total).clamp(0.0, 1.0);
        let sequence_coherence = if state.direction_sequence.len() > 1 {
            let flips = state
                .direction_sequence
                .windows(2)
                .filter(|pair| pair[0] != pair[1])
                .count();
            let alternation = flips as f64 / (state.direction_sequence.len() - 1) as f64;
            (1.0 - alternation).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let seq_adjusted = (raw_dc * sequence_coherence).clamp(0.0, 1.0);
        if let Some(sobp_ratio) = state.sobp_ratio {
            let sobp_dc = ((sobp_ratio - 0.5).abs() * 2.0).clamp(0.0, 1.0);
            let blend = self.config.sobp_dc_blend.clamp(0.0, 1.0);
            (seq_adjusted * (1.0 - blend) + sobp_dc * blend).clamp(0.0, 1.0)
        } else {
            seq_adjusted
        }
    }

    fn signer_entropy(&self, state: &BvaState) -> f64 {
        let total = state.tx_count_total as f64;
        if total == 0.0 {
            return 0.0;
        }
        let counts = &state.signer_counts;
        let unique = counts.len();
        if unique <= 1 {
            return 0.0;
        }
        let entropy = shannon_entropy(&counts, total);
        (entropy / (unique as f64).ln()).clamp(0.0, 1.0)
    }

    fn cir_echo_ratio(&self, state: &BvaState) -> f64 {
        if state.cir_emitted_weighted_total <= f64::EPSILON {
            return 0.0;
        }
        (state.cir_echo_weighted_sum / state.cir_emitted_weighted_total).clamp(0.0, 1.0)
    }

    fn early_reaction_pressure(&self, state: &BvaState) -> f64 {
        if state.tx_count_total == 0 {
            return 0.0;
        }
        (state.early_reaction_count as f64 / state.tx_count_total as f64).clamp(0.0, 1.0)
    }

    fn confidence(&self, state: &BvaState, now_ts_ms: u64) -> f64 {
        let tx_component =
            (state.tx_count_total as f64 / self.config.confidence_tx_divisor as f64).min(1.0);
        let signer_component = (state.unique_signers.len() as f64
            / self.config.confidence_signer_divisor as f64)
            .min(1.0);
        let alive_ms = now_ts_ms.saturating_sub(state.birth_ts_ms) as f64;
        let time_component = (alive_ms / self.config.confidence_time_divisor_ms as f64).min(1.0);

        let mut confidence = tx_component.min(signer_component).min(time_component);
        if state.sobp_fallback_used {
            confidence *= 1.0 - self.config.sobp_fallback_confidence_penalty;
        }
        if state.congestion_flag {
            confidence *= 1.0 - self.config.congestion_confidence_penalty;
        }
        confidence.clamp(0.0, 1.0)
    }

    fn classify(
        &self,
        state: &BvaState,
        tds: f64,
        dc: f64,
        se: f64,
        erp: f64,
        confidence: f64,
    ) -> BvaClassification {
        let min_tx = self.config.confidence_tx_divisor.max(1) as usize;
        let min_signers = self.config.confidence_signer_divisor.max(1) as usize;
        if state.tx_count_total < min_tx
            || state.unique_signers.len() < min_signers
            || confidence < self.config.classification_confidence_floor
        {
            return BvaClassification::Dormant;
        }
        if tds >= self.config.classification_organic_tds_min
            && se >= self.config.classification_organic_se_min
            && dc >= self.config.classification_organic_dc_min
        {
            return BvaClassification::Organic;
        }
        if dc >= self.config.classification_steered_dc_min
            && erp >= self.config.classification_steered_erp_min
            && se <= (1.0 - self.config.classification_organic_se_min).clamp(0.0, 1.0)
        {
            return BvaClassification::Steered;
        }
        BvaClassification::Chaotic
    }
}

fn tx_key_hash(tx: &TransactionRecord) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if !tx.signature.is_empty() {
        tx.signature.hash(&mut hasher);
    } else {
        tx.timestamp_ms.hash(&mut hasher);
        tx.signer.hash(&mut hasher);
        (tx.sol_amount.to_bits()).hash(&mut hasher);
        tx.is_buy.hash(&mut hasher);
    }
    hasher.finish()
}

fn shannon_entropy(counts: &HashMap<Pubkey, usize>, total: f64) -> f64 {
    counts
        .values()
        .map(|v| {
            let p = *v as f64 / total.max(1.0);
            if p > 0.0 {
                -p * p.ln()
            } else {
                0.0
            }
        })
        .sum()
}

fn median_u64(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    Some(sorted[mid])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::snapshot_engine::EventTsSource;

    fn tx(slot: u64, ts: u64, signer: Pubkey, buy: bool) -> TransactionRecord {
        TransactionRecord {
            slot: Some(slot),
            signature: format!("sig-{}-{}", slot, ts),
            signer,
            sol_amount: 0.5,
            is_buy: buy,
            is_dev_buy: false,
            timestamp_ms: ts,
            event_time: ghost_core::EventTimeMetadata::default(),
            event_ts_source: EventTsSource::Event,
            seq_no: ts,
            raw_bytes: None,
            raw_bytes_missing_reason: seer::types::RawBytesMissingReason::Unknown,
            price_quote: None,
        }
    }

    #[test]
    fn test_bva_output_bounds() {
        let cfg = BvaConfig::default();
        let analyzer = BvaAnalyzer::new(cfg);
        let mut state = BvaState::new(Some(100), 1_000);
        let signer = Pubkey::new_unique();
        state.process_tx(&tx(100, 1_000, signer, true), analyzer.config());
        state.process_tx(&tx(101, 1_200, signer, false), analyzer.config());

        let output = analyzer.analyze(&state, 2_000);
        assert!(output.score >= 0.0 && output.score <= 1.0);
        assert!(output.confidence >= 0.0 && output.confidence <= 1.0);
    }

    #[test]
    fn test_tds_prefers_dense_activity() {
        let cfg = BvaConfig::default();
        let analyzer = BvaAnalyzer::new(cfg);

        let mut dense = BvaState::new(Some(100), 1_000);
        let s1 = Pubkey::new_unique();
        dense.process_tx(&tx(100, 1_000, s1, true), analyzer.config());
        dense.process_tx(&tx(101, 1_100, s1, true), analyzer.config());

        let mut sparse = BvaState::new(Some(100), 1_000);
        let s2 = Pubkey::new_unique();
        sparse.process_tx(&tx(100, 1_000, s2, true), analyzer.config());
        sparse.process_tx(&tx(101, 1_800, s2, true), analyzer.config());

        let dense_out = analyzer.analyze(&dense, 2_000);
        let sparse_out = analyzer.analyze(&sparse, 2_000);

        assert!(dense_out.metrics.tds > sparse_out.metrics.tds);
    }

    #[test]
    fn test_entropy_increases_score() {
        let cfg = BvaConfig::default();
        let analyzer = BvaAnalyzer::new(cfg);

        let mut low_entropy = BvaState::new(Some(100), 1_000);
        let signer = Pubkey::new_unique();
        low_entropy.process_tx(&tx(100, 1_000, signer, true), analyzer.config());
        low_entropy.process_tx(&tx(101, 1_150, signer, true), analyzer.config());
        low_entropy.process_tx(&tx(102, 1_300, signer, true), analyzer.config());

        let mut high_entropy = BvaState::new(Some(100), 1_000);
        for (idx, ts) in [1_000_u64, 1_150, 1_300].iter().enumerate() {
            high_entropy.process_tx(
                &tx(100 + idx as u64, *ts, Pubkey::new_unique(), true),
                analyzer.config(),
            );
        }

        let low_out = analyzer.analyze(&low_entropy, 2_000);
        let high_out = analyzer.analyze(&high_entropy, 2_000);

        assert!(high_out.score > low_out.score);
    }

    #[test]
    fn test_direction_sequence_affects_dc() {
        let cfg = BvaConfig::default();
        let analyzer = BvaAnalyzer::new(cfg);

        let mut clustered = BvaState::new(Some(100), 1_000);
        let signer = Pubkey::new_unique();
        clustered.process_tx(&tx(100, 1_000, signer, true), analyzer.config());
        clustered.process_tx(&tx(101, 1_050, signer, true), analyzer.config());
        clustered.process_tx(&tx(102, 1_100, signer, true), analyzer.config());
        clustered.process_tx(&tx(103, 1_150, signer, false), analyzer.config());

        let mut alternating = BvaState::new(Some(100), 1_000);
        let signer2 = Pubkey::new_unique();
        alternating.process_tx(&tx(100, 1_000, signer2, true), analyzer.config());
        alternating.process_tx(&tx(101, 1_050, signer2, false), analyzer.config());
        alternating.process_tx(&tx(102, 1_100, signer2, true), analyzer.config());
        alternating.process_tx(&tx(103, 1_150, signer2, true), analyzer.config());

        let clustered_out = analyzer.analyze(&clustered, 2_000);
        let alternating_out = analyzer.analyze(&alternating, 2_000);

        assert!(clustered_out.metrics.dc > alternating_out.metrics.dc);
    }

    #[test]
    fn test_cir_echo_time_weighting() {
        let cfg = BvaConfig::default();
        let analyzer = BvaAnalyzer::new(cfg.clone());

        let mut early_echo = BvaState::new(Some(100), 1_000);
        early_echo.register_cir_emitted(
            &CirEmittedTx {
                tx_key: 1,
                slot: Some(100),
                signer: Pubkey::new_unique(),
                is_buy: true,
                amount_sol: 1.0,
                cir_effective: 0.7,
                timestamp_ms: 1_000,
                responder_count: 1,
                immediate_reactions: 1.0,
                sustained_reactions: 1.0,
            },
            &cfg,
        );
        early_echo.register_cir_emitted(
            &CirEmittedTx {
                tx_key: 2,
                slot: Some(110),
                signer: Pubkey::new_unique(),
                is_buy: true,
                amount_sol: 1.0,
                cir_effective: 0.7,
                timestamp_ms: 1_400,
                responder_count: 1,
                immediate_reactions: 1.0,
                sustained_reactions: 0.0,
            },
            &cfg,
        );

        let mut late_echo = BvaState::new(Some(100), 1_000);
        late_echo.register_cir_emitted(
            &CirEmittedTx {
                tx_key: 3,
                slot: Some(100),
                signer: Pubkey::new_unique(),
                is_buy: true,
                amount_sol: 1.0,
                cir_effective: 0.7,
                timestamp_ms: 1_000,
                responder_count: 1,
                immediate_reactions: 1.0,
                sustained_reactions: 0.0,
            },
            &cfg,
        );
        late_echo.register_cir_emitted(
            &CirEmittedTx {
                tx_key: 4,
                slot: Some(110),
                signer: Pubkey::new_unique(),
                is_buy: true,
                amount_sol: 1.0,
                cir_effective: 0.7,
                timestamp_ms: 1_400,
                responder_count: 1,
                immediate_reactions: 1.0,
                sustained_reactions: 1.0,
            },
            &cfg,
        );

        let early_out = analyzer.analyze(&early_echo, 2_000);
        let late_out = analyzer.analyze(&late_echo, 2_000);

        assert!(early_out.metrics.cer > late_out.metrics.cer);
    }

    #[test]
    fn test_classification_guards_dormant() {
        let cfg = BvaConfig::default();
        let analyzer = BvaAnalyzer::new(cfg);
        let mut state = BvaState::new(Some(100), 1_000);
        let signer = Pubkey::new_unique();
        state.process_tx(&tx(100, 1_000, signer, true), analyzer.config());

        let output = analyzer.analyze(&state, 1_200);
        assert_eq!(output.classification, BvaClassification::Dormant);
    }

    #[test]
    fn test_confidence_penalties_apply() {
        let cfg = BvaConfig::default();
        let analyzer = BvaAnalyzer::new(cfg);
        let mut state = BvaState::new(Some(100), 1_000);
        let signer = Pubkey::new_unique();
        state.process_tx(&tx(100, 1_000, signer, true), analyzer.config());
        state.process_tx(
            &tx(101, 1_200, Pubkey::new_unique(), true),
            analyzer.config(),
        );
        state.process_tx(
            &tx(102, 1_400, Pubkey::new_unique(), true),
            analyzer.config(),
        );

        let base = analyzer.analyze(&state, 2_000).confidence;

        state.update_sobp(None, true);
        state.update_congestion_flag(true);

        let penalized = analyzer.analyze(&state, 2_000).confidence;
        assert!(penalized < base);
    }
}
