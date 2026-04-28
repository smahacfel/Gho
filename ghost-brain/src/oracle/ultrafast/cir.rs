//! CIR (Causal Impact Ratio) Module
//!
//! Event-time based causal impact scoring designed for environments without raw bytes.
//! Replaces MPCF by evaluating whether a transaction causes downstream market reactions
//! from independent actors within strict time windows (milliseconds).
//!
//! Core principle: a transaction has value only if it triggers reactions.
//!
//! This module is incremental (streaming): each new tx updates the impact of
//! prior txs in a bounded time window, and emits txs once they cross the CIR threshold.
//!
//! # Event-Time Logic (FAZA 6)
//! All reaction windows are now based on `timestamp_ms`, not Solana slots.
//! This ensures consistent behavior regardless of slot presence.

use std::collections::{HashMap, HashSet, VecDeque};

use metrics::{gauge, histogram, increment_counter};
use solana_sdk::pubkey::Pubkey;

// =============================================================================
// Configuration
// =============================================================================

/// Runtime configuration for CIR.
///
/// # Event-Time Based (FAZA 6)
/// All time windows are now in milliseconds, not slots.
/// This ensures consistent behavior regardless of slot presence.
#[derive(Debug, Clone, Copy)]
pub struct CirConfig {
    /// Immediate reaction window (milliseconds)
    /// Default: 800ms (~2 slots × 400ms)
    pub tau1_ms: u64,
    /// Sustained reaction window (milliseconds)
    /// Default: 2400ms (~6 slots × 400ms)
    pub tau2_ms: u64,
    /// Minimum temporal spread for "sustained" classification (ms)
    /// Reactions must span at least this duration to be considered sustained.
    /// Default: 5000ms (5 seconds)
    pub min_sustained_spread_ms: u64,
    /// Minimum responder amount ratio vs base tx
    pub epsilon_amount: f64,
    /// Resonance penalty threshold for stddev(amount_sol)
    pub epsilon_resonance: f64,
    /// Emission threshold for CIR_effective
    pub theta: f64,
    /// Fresh wallet window (milliseconds)
    /// Default: 800ms (~2 slots × 400ms)
    pub freshness_window_ms: u64,
    /// Fresh wallet reaction weight
    pub freshness_weight: f64,
    /// Amount resonance penalty multiplier
    pub resonance_penalty: f64,
    /// Max seen tx keys kept for dedup
    pub max_seen_keys: usize,
}

impl Default for CirConfig {
    fn default() -> Self {
        Self {
            tau1_ms: 800,                  // ~2 slots × 400ms
            tau2_ms: 2400,                 // ~6 slots × 400ms
            min_sustained_spread_ms: 5000, // 5 seconds
            epsilon_amount: 0.1,
            epsilon_resonance: 0.05,
            theta: 0.25,
            freshness_window_ms: 800, // ~2 slots × 400ms
            freshness_weight: 0.2,
            resonance_penalty: 0.6,
            max_seen_keys: 512,
        }
    }
}

// =============================================================================
// Types
// =============================================================================

/// Buy/Sell side for CIR processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuySell {
    Buy,
    Sell,
}

/// Minimal transaction event for CIR processing.
#[derive(Debug, Clone, Copy)]
pub struct CirEvent {
    pub slot: Option<u64>,
    pub timestamp_ms: u64,
    pub signer: Pubkey,
    pub side: BuySell,
    pub amount_sol: f64,
}

/// CIR-emitted transaction ready for downstream modules (SOBP).
#[derive(Debug, Clone, Copy)]
pub struct CirEmittedTx {
    pub tx_key: u64,
    pub slot: Option<u64>,
    pub signer: Pubkey,
    pub is_buy: bool,
    pub amount_sol: f64,
    pub cir_effective: f64,
    pub timestamp_ms: u64,
    pub responder_count: usize,
    pub immediate_reactions: f64,
    pub sustained_reactions: f64,
}

/// CIR context for downstream modules that need impulse scaling.
#[derive(Debug, Clone, Copy)]
pub struct CirContext {
    pub impulse_weight: f64,
}

/// Telemetry snapshot (aggregated per pool).
#[derive(Debug, Clone, Copy)]
pub struct CirTelemetry {
    pub avg_cir: f64,
    pub avg_ic: f64,
    pub avg_sc: f64,
    pub avg_ad: f64,
    pub count: usize,
    pub ic_only_count: usize,
}

#[derive(Debug, Clone)]
struct PendingImpact {
    tx_key: u64,
    slot: Option<u64>,
    /// Base transaction timestamp (event-time)
    timestamp_ms: u64,
    signer: Pubkey,
    is_buy: bool,
    amount_sol: f64,
    immediate_reactions: f64,
    sustained_reactions: f64,
    responders: Vec<Pubkey>,
    responder_amounts: Vec<f64>,
    /// Earliest responder timestamp (for spread calculation)
    first_reaction_ts_ms: u64,
    /// Latest responder timestamp (for spread calculation)
    last_reaction_ts_ms: u64,
    emitted: bool,
    last_emitted_ts_ms: Option<u64>,
}

impl PendingImpact {
    fn new(tx_key: u64, event: &CirEvent) -> Self {
        Self {
            tx_key,
            slot: event.slot,
            timestamp_ms: event.timestamp_ms,
            signer: event.signer,
            is_buy: event.side == BuySell::Buy,
            amount_sol: event.amount_sol,
            immediate_reactions: 0.0,
            sustained_reactions: 0.0,
            responders: Vec::with_capacity(8),
            responder_amounts: Vec::with_capacity(8),
            first_reaction_ts_ms: 0,
            last_reaction_ts_ms: 0,
            emitted: false,
            last_emitted_ts_ms: None,
        }
    }

    fn add_responder(&mut self, responder: Pubkey, amount_sol: f64, timestamp_ms: u64) {
        if !self.responders.iter().any(|pk| *pk == responder) {
            self.responders.push(responder);
            self.responder_amounts.push(amount_sol);

            // Track reaction timestamps for spread calculation
            if self.first_reaction_ts_ms == 0 {
                self.first_reaction_ts_ms = timestamp_ms;
            } else {
                self.first_reaction_ts_ms = self.first_reaction_ts_ms.min(timestamp_ms);
            }
            self.last_reaction_ts_ms = self.last_reaction_ts_ms.max(timestamp_ms);
        }
    }

    /// Calculate the temporal spread of reactions (ms)
    fn reaction_spread_ms(&self) -> u64 {
        if self.first_reaction_ts_ms == 0 {
            return 0;
        }
        self.last_reaction_ts_ms
            .saturating_sub(self.first_reaction_ts_ms)
    }
}

// =============================================================================
// CIR Core
// =============================================================================

/// Incremental CIR computation engine.
pub struct CirCore {
    config: CirConfig,
    pending: VecDeque<PendingImpact>,
    signer_first_seen: HashMap<Pubkey, u64>,
    seen_keys: HashSet<u64>,
    seen_order: VecDeque<u64>,
}

impl CirCore {
    pub fn new(config: CirConfig) -> Self {
        Self {
            config,
            pending: VecDeque::new(),
            signer_first_seen: HashMap::new(),
            seen_keys: HashSet::new(),
            seen_order: VecDeque::new(),
        }
    }

    /// Process a new event and return newly eligible transactions (CIR >= theta).
    ///
    /// # Event-Time Logic (FAZA 6)
    /// All reaction windows are now based on `timestamp_ms`, not slots.
    pub fn process_event(&mut self, event: CirEvent, tx_key: u64) -> Vec<CirEmittedTx> {
        let config = self.config;
        if self.seen_keys.contains(&tx_key) {
            return Vec::new();
        }

        self.track_seen(tx_key);
        self.track_first_seen(event.signer, event.timestamp_ms);

        // Update pending impacts with this event as responder
        for impact in self.pending.iter_mut() {
            // Decay reactions for already-emitted impacts based on time delta
            if impact.emitted {
                if let Some(last_ts) = impact.last_emitted_ts_ms {
                    let delta_ms = event.timestamp_ms.saturating_sub(last_ts);
                    if delta_ms > 0 {
                        // Time-based decay: ~15% per 400ms for immediate, ~10% per 400ms for sustained
                        let decay_factor = delta_ms as f64 / 400.0; // normalize to ~slot time
                        let decay_immediate = 0.85_f64.powf(decay_factor);
                        let decay_sustained = 0.9_f64.powf(decay_factor);
                        impact.immediate_reactions *= decay_immediate;
                        impact.sustained_reactions *= decay_sustained;
                        impact.last_emitted_ts_ms = Some(event.timestamp_ms);
                    }
                }
            }

            // Skip if event is not after the impact (using timestamp)
            if event.timestamp_ms <= impact.timestamp_ms {
                continue;
            }

            // Calculate time delta in milliseconds
            let delta_ms = event.timestamp_ms.saturating_sub(impact.timestamp_ms);

            // Skip if outside sustained reaction window
            if delta_ms > config.tau2_ms {
                continue;
            }

            // Skip same signer (self-reactions don't count)
            if event.signer == impact.signer {
                continue;
            }

            // Skip if amount too small relative to base tx
            if event.amount_sol < impact.amount_sol * config.epsilon_amount {
                continue;
            }

            // Fresh wallet weight calculation (event-time based)
            let weight = if Self::is_fresh_responder_with_config(
                &self.signer_first_seen,
                event.signer,
                event.timestamp_ms,
                config,
            ) {
                increment_counter!("cir_penalty_wallet_fresh_total");
                config.freshness_weight
            } else {
                1.0
            };

            // Classify as immediate or sustained based on time window
            if delta_ms <= config.tau1_ms {
                impact.immediate_reactions += weight;
            } else {
                impact.sustained_reactions += weight;
            }

            // Track responder with timestamp for spread calculation
            impact.add_responder(event.signer, event.amount_sol, event.timestamp_ms);
        }

        // Emit impacts that cross threshold
        let mut emitted = Vec::new();
        for impact in self.pending.iter_mut() {
            if impact.emitted {
                continue;
            }
            let cir_effective = Self::cir_effective_with_config(impact, config);
            if cir_effective >= config.theta {
                impact.emitted = true;
                impact.last_emitted_ts_ms = Some(event.timestamp_ms);
                emitted.push(CirEmittedTx {
                    tx_key: impact.tx_key,
                    slot: impact.slot,
                    signer: impact.signer,
                    is_buy: impact.is_buy,
                    amount_sol: impact.amount_sol,
                    cir_effective,
                    timestamp_ms: impact.timestamp_ms,
                    responder_count: impact.responders.len(),
                    immediate_reactions: impact.immediate_reactions,
                    sustained_reactions: impact.sustained_reactions,
                });
            }
        }

        // Evict stale impacts
        self.evict_stale(event.timestamp_ms);

        // Insert current event
        self.pending.push_back(PendingImpact::new(tx_key, &event));

        emitted
    }

    /// Compute telemetry snapshot for current pending impacts.
    pub fn telemetry_snapshot(&self) -> Option<CirTelemetry> {
        if self.pending.is_empty() {
            return None;
        }

        let mut sum_cir = 0.0;
        let mut sum_ic = 0.0;
        let mut sum_sc = 0.0;
        let mut sum_ad = 0.0;
        let mut count = 0usize;
        let mut ic_only_count = 0usize;

        for impact in self.pending.iter() {
            let (ic, sc, ad, cir) = self.cir_components(impact);
            if ic + sc <= 0.0 {
                continue;
            }
            if ic > 0.0 && sc == 0.0 {
                ic_only_count = ic_only_count.saturating_add(1);
            }
            sum_cir += cir;
            sum_ic += ic;
            sum_sc += sc;
            sum_ad += ad;
            count += 1;
        }

        if count == 0 {
            return None;
        }

        let avg_cir = sum_cir / count as f64;
        let avg_ic = sum_ic / count as f64;
        let avg_sc = sum_sc / count as f64;
        let avg_ad = sum_ad / count as f64;

        Some(CirTelemetry {
            avg_cir,
            avg_ic,
            avg_sc,
            avg_ad,
            count,
            ic_only_count,
        })
    }

    /// Current global CIR score (max effective value in window).
    pub fn global_score(&self) -> Option<f64> {
        self.pending
            .iter()
            .map(|p| self.cir_effective(p))
            .max_by(|a, b| a.partial_cmp(b).unwrap())
    }

    /// Check if reactions for an impact are "sustained" (spread over time).
    ///
    /// # Returns
    /// `true` if reaction spread >= `min_sustained_spread_ms` (default 5s)
    /// `false` if all reactions are burst (same timestamp or small spread)
    #[inline]
    pub fn is_sustained(&self, impact: &PendingImpact) -> bool {
        impact.reaction_spread_ms() >= self.config.min_sustained_spread_ms
    }

    fn track_first_seen(&mut self, signer: Pubkey, timestamp_ms: u64) {
        self.signer_first_seen.entry(signer).or_insert(timestamp_ms);
    }

    fn is_fresh_responder(&self, signer: Pubkey, current_ts_ms: u64) -> bool {
        if let Some(first_ts) = self.signer_first_seen.get(&signer) {
            let delta_ms = current_ts_ms.saturating_sub(*first_ts);
            delta_ms <= self.config.freshness_window_ms
        } else {
            false
        }
    }

    fn track_seen(&mut self, key: u64) {
        self.seen_keys.insert(key);
        self.seen_order.push_back(key);
        while self.seen_order.len() > self.config.max_seen_keys {
            if let Some(old) = self.seen_order.pop_front() {
                self.seen_keys.remove(&old);
            }
        }
    }

    /// Evict impacts that are outside the sustained reaction window (event-time based).
    fn evict_stale(&mut self, current_ts_ms: u64) {
        while let Some(front) = self.pending.front() {
            if current_ts_ms.saturating_sub(front.timestamp_ms) > self.config.tau2_ms {
                self.pending.pop_front();
            } else {
                break;
            }
        }
    }

    fn cir_components(&self, impact: &PendingImpact) -> (f64, f64, f64, f64) {
        let ic = impact.immediate_reactions;
        let sc = impact.sustained_reactions;
        if ic + sc <= 0.0 {
            return (ic, sc, 0.0, 0.0);
        }

        let mut ad = impact.responders.len() as f64
            / (1.0 + impact.immediate_reactions.max(impact.sustained_reactions));
        if self.is_resonant(impact) {
            ad *= self.config.resonance_penalty;
        }

        let cir_raw = ic * sc.powf(0.7) * ad.powf(1.2);
        let cir_effective = self.normalize_cir(cir_raw);
        (ic, sc, ad, cir_effective)
    }

    fn cir_effective(&self, impact: &PendingImpact) -> f64 {
        let (_, _, _, cir) = self.cir_components(impact);
        cir
    }

    fn cir_effective_with_config(impact: &PendingImpact, config: CirConfig) -> f64 {
        let ic = impact.immediate_reactions;
        let sc = impact.sustained_reactions;
        if ic + sc <= 0.0 {
            return 0.0;
        }

        let mut ad = impact.responders.len() as f64
            / (1.0 + impact.immediate_reactions.max(impact.sustained_reactions));
        if Self::is_resonant_with_config(impact, config) {
            ad *= config.resonance_penalty;
        }

        let cir_raw = ic * sc.powf(0.7) * ad.powf(1.2);
        let normalized = if cir_raw <= 0.0 {
            0.0
        } else {
            cir_raw / (cir_raw + 1.0)
        };
        normalized.min(1.0)
    }

    fn normalize_cir(&self, raw: f64) -> f64 {
        if raw <= 0.0 {
            return 0.0;
        }
        let normalized = raw / (raw + 1.0);
        normalized.min(1.0)
    }

    fn is_resonant(&self, impact: &PendingImpact) -> bool {
        let n = impact.responder_amounts.len();
        if n < 2 {
            return false;
        }
        let mean = impact.responder_amounts.iter().sum::<f64>() / n as f64;
        let var = impact
            .responder_amounts
            .iter()
            .map(|v| {
                let diff = v - mean;
                diff * diff
            })
            .sum::<f64>()
            / n as f64;
        let stddev = var.sqrt();

        if stddev < self.config.epsilon_resonance {
            increment_counter!("cir_penalty_resonance_total");
            true
        } else {
            false
        }
    }

    fn is_resonant_with_config(impact: &PendingImpact, config: CirConfig) -> bool {
        let n = impact.responder_amounts.len();
        if n < 2 {
            return false;
        }
        let mean = impact.responder_amounts.iter().sum::<f64>() / n as f64;
        let var = impact
            .responder_amounts
            .iter()
            .map(|v| {
                let diff = v - mean;
                diff * diff
            })
            .sum::<f64>()
            / n as f64;
        let stddev = var.sqrt();

        if stddev < config.epsilon_resonance {
            increment_counter!("cir_penalty_resonance_total");
            true
        } else {
            false
        }
    }

    fn is_fresh_responder_with_config(
        signer_first_seen: &HashMap<Pubkey, u64>,
        signer: Pubkey,
        current_ts_ms: u64,
        config: CirConfig,
    ) -> bool {
        if let Some(first_ts) = signer_first_seen.get(&signer) {
            let delta_ms = current_ts_ms.saturating_sub(*first_ts);
            delta_ms <= config.freshness_window_ms
        } else {
            false
        }
    }

    /// Update metrics for telemetry snapshot.
    pub fn record_metrics(&self) {
        if let Some(snapshot) = self.telemetry_snapshot() {
            histogram!("cir_effective_histogram", snapshot.avg_cir);
            gauge!("cir_ic_avg", snapshot.avg_ic as f64);
            gauge!("cir_sc_avg", snapshot.avg_sc as f64);
            gauge!("cir_ad_avg", snapshot.avg_ad as f64);
        }
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Build a stable tx key for CIR deduplication.
pub fn cir_tx_key(
    signature: &str,
    slot: Option<u64>,
    timestamp_ms: u64,
    signer: &Pubkey,
    amount_sol: f64,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let normalized_slot = slot.and_then(|s| if s > 0 { Some(s) } else { None });
    signature.hash(&mut hasher);
    normalized_slot.hash(&mut hasher);
    timestamp_ms.hash(&mut hasher);
    signer.hash(&mut hasher);
    amount_sol.to_bits().hash(&mut hasher);
    hasher.finish()
}

// =============================================================================
// Tests (minimal CIR scenarios)
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn pk(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    #[test]
    fn scenario_a_dev_wash_cir_zero() {
        let config = CirConfig::default();
        let mut core = CirCore::new(config);
        let signer = pk(1);

        // Single signer wash trading - should have near-zero CIR
        for i in 0..20u64 {
            let event = CirEvent {
                slot: Some(10 + i),
                timestamp_ms: 1000 + i * 100, // 100ms apart
                signer,
                side: BuySell::Buy,
                amount_sol: 0.5 + (i as f64 * 0.01),
            };
            let key = cir_tx_key(
                "",
                event.slot,
                event.timestamp_ms,
                &event.signer,
                event.amount_sol,
            );
            core.process_event(event, key);
        }

        let global = core.global_score().unwrap_or(0.0);
        assert!(
            global < 0.1,
            "CIR should be near zero for single signer wash"
        );
    }

    #[test]
    fn scenario_b_sybil_resonance_penalized() {
        let mut config = CirConfig::default();
        config.epsilon_resonance = 0.0001;
        let mut core = CirCore::new(config);

        // Base tx
        let base = CirEvent {
            slot: Some(100),
            timestamp_ms: 1000,
            signer: pk(1),
            side: BuySell::Buy,
            amount_sol: 1.0,
        };
        let key = cir_tx_key(
            "base",
            base.slot,
            base.timestamp_ms,
            &base.signer,
            base.amount_sol,
        );
        core.process_event(base, key);

        // 10 responders with identical amounts in tight timing (burst reactions)
        for i in 0..10u8 {
            let event = CirEvent {
                slot: Some(101),
                timestamp_ms: 1100 + i as u64 * 10, // 10ms apart - within tau1_ms
                signer: pk(10 + i),
                side: BuySell::Buy,
                amount_sol: 0.2,
            };
            let key = cir_tx_key(
                "",
                event.slot,
                event.timestamp_ms,
                &event.signer,
                event.amount_sol,
            );
            core.process_event(event, key);
        }

        let global = core.global_score().unwrap_or(0.0);
        assert!(
            global < 0.6,
            "Sybil resonance should reduce CIR below threshold"
        );
    }

    #[test]
    fn scenario_c_organic_reactions_high_cir() {
        let config = CirConfig::default();
        let mut core = CirCore::new(config);

        let base = CirEvent {
            slot: Some(200),
            timestamp_ms: 1000,
            signer: pk(1),
            side: BuySell::Buy,
            amount_sol: 1.0,
        };
        let key = cir_tx_key(
            "base",
            base.slot,
            base.timestamp_ms,
            &base.signer,
            base.amount_sol,
        );
        core.process_event(base, key);

        // Organic reactions spread over time (both immediate and sustained)
        for i in 0..15u8 {
            let event = CirEvent {
                slot: Some(201 + (i as u64 % 6)),
                // Mix of immediate (< 800ms) and sustained (800-2400ms) reactions
                timestamp_ms: 1000 + 100 + (i as u64 * 150), // 150ms apart
                signer: pk(20 + i),
                side: BuySell::Buy,
                amount_sol: 0.2 + (i as f64 * 0.01),
            };
            let key = cir_tx_key(
                "",
                event.slot,
                event.timestamp_ms,
                &event.signer,
                event.amount_sol,
            );
            core.process_event(event, key);
        }

        let global = core.global_score().unwrap_or(0.0);
        assert!(
            global > 0.5,
            "Organic reactions should yield high CIR, got {}",
            global
        );
    }

    // =========================================================================
    // Event-Time Specific Tests (FAZA 6)
    // =========================================================================

    #[test]
    fn test_sustained_reactions_spread() {
        // Reactions spread over >= 5s should be classified as sustained
        // Need tau2_ms to be large enough to capture reactions at 6000ms
        let mut config = CirConfig::default();
        config.tau2_ms = 10_000; // 10s window to capture all reactions
        let mut core = CirCore::new(config);

        let base = CirEvent {
            slot: Some(100),
            timestamp_ms: 0,
            signer: pk(1),
            side: BuySell::Buy,
            amount_sol: 1.0,
        };
        let key = cir_tx_key(
            "base",
            base.slot,
            base.timestamp_ms,
            &base.signer,
            base.amount_sol,
        );
        core.process_event(base, key);

        // Reactions at different times to create spread
        // All within tau2_ms (10s), but with spread >= 5s
        let timestamps = [500, 1000, 2000, 6000]; // spread = 5500ms >= 5000ms
        for (i, ts) in timestamps.iter().enumerate() {
            let event = CirEvent {
                slot: Some(101 + i as u64),
                timestamp_ms: *ts,
                signer: pk(10 + i as u8),
                side: BuySell::Buy,
                amount_sol: 0.2,
            };
            let key = cir_tx_key(
                "",
                event.slot,
                event.timestamp_ms,
                &event.signer,
                event.amount_sol,
            );
            core.process_event(event, key);
        }

        // Check that the base impact has sustained spread
        let impact = &core.pending[0];
        let spread = impact.reaction_spread_ms();
        assert!(spread >= 5000, "Spread should be >= 5000ms, got {}", spread);
        assert!(
            core.is_sustained(impact),
            "Reactions with spread >= 5s should be sustained"
        );
    }

    #[test]
    fn tx_key_treats_none_and_zero_slot_identically() {
        let signer = pk(7);
        let key_none = cir_tx_key("sig", None, 1234, &signer, 0.42);
        let key_zero = cir_tx_key("sig", Some(0), 1234, &signer, 0.42);
        assert_eq!(
            key_none, key_zero,
            "slot=0 must normalize to None in tx keying"
        );
    }

    #[test]
    fn test_burst_reactions_same_timestamp() {
        // All reactions in same timestamp = burst (spread = 0)
        let config = CirConfig::default();
        let mut core = CirCore::new(config);

        let base = CirEvent {
            slot: Some(100),
            timestamp_ms: 0,
            signer: pk(1),
            side: BuySell::Buy,
            amount_sol: 1.0,
        };
        let key = cir_tx_key(
            "base",
            base.slot,
            base.timestamp_ms,
            &base.signer,
            base.amount_sol,
        );
        core.process_event(base, key);

        // All reactions at the exact same timestamp
        for i in 0..5u8 {
            let event = CirEvent {
                slot: Some(101),
                timestamp_ms: 500, // same timestamp for all
                signer: pk(10 + i),
                side: BuySell::Buy,
                amount_sol: 0.2,
            };
            let key = cir_tx_key(
                "",
                event.slot,
                event.timestamp_ms,
                &event.signer,
                event.amount_sol,
            );
            core.process_event(event, key);
        }

        // Check that the base impact has zero spread (burst)
        let impact = &core.pending[0];
        let spread = impact.reaction_spread_ms();
        assert_eq!(
            spread, 0,
            "Burst reactions should have spread = 0, got {}",
            spread
        );
        assert!(
            !core.is_sustained(impact),
            "Burst reactions should not be sustained"
        );
    }

    #[test]
    fn test_time_based_eviction() {
        // Impacts should be evicted after tau2_ms (2400ms default)
        let config = CirConfig::default();
        let mut core = CirCore::new(config);

        let base = CirEvent {
            slot: Some(100),
            timestamp_ms: 0,
            signer: pk(1),
            side: BuySell::Buy,
            amount_sol: 1.0,
        };
        let key = cir_tx_key(
            "base",
            base.slot,
            base.timestamp_ms,
            &base.signer,
            base.amount_sol,
        );
        core.process_event(base, key);
        assert_eq!(core.pending.len(), 1);

        // Event at tau2_ms + 1 should trigger eviction of base
        let late_event = CirEvent {
            slot: Some(200),
            timestamp_ms: 2500, // > tau2_ms (2400ms)
            signer: pk(99),
            side: BuySell::Buy,
            amount_sol: 0.1,
        };
        let key = cir_tx_key(
            "late",
            late_event.slot,
            late_event.timestamp_ms,
            &late_event.signer,
            late_event.amount_sol,
        );
        core.process_event(late_event, key);

        // Base should be evicted, only late_event remains
        assert_eq!(core.pending.len(), 1);
        assert_eq!(core.pending[0].timestamp_ms, 2500);
    }
}
