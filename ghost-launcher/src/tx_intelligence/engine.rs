use crate::events::PoolTransaction;
use crate::tx_intelligence::config::TxIntelligenceConfig;
use crate::tx_intelligence::{
    compute_dev_behavior, compute_signer_diversity, compute_velocity_profile,
    compute_volume_sanity, SignerStats,
};
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::shadow_ledger::TxKey;
use ghost_core::tx_intelligence::types::{
    BurstWindow, RiskFlag, RiskSeverity, TxIntelFeatures, TxIntelligenceState,
};
use ghost_core::{EventTruthKind, SlotQuality};
use seer::early_fingerprint::{
    EarlyFingerprintMetrics, FingerprintAggregator, FingerprintTxEvent, TokenDelta,
};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
const PUMPFUN_TOKEN_DECIMALS: u8 = 6;
const GENESIS_TOKEN_RESERVES_RAW: u128 = 1_073_000_000_000_000;
const BUNDLE_CLUSTER_THRESHOLD_MS: u64 = 50;

#[derive(Debug, Clone, Default)]
struct SignerBehaviorStats {
    tx_count: usize,
    buy_count: usize,
    sell_count: usize,
    total_volume_sol: f64,
    buy_volume_sol: f64,
    sell_volume_sol: f64,
    first_buy_volume_sol: Option<f64>,
    first_buy_tokens: Option<f64>,
}

impl SignerBehaviorStats {
    fn to_gatekeeper_stats(&self) -> SignerStats {
        SignerStats {
            tx_count: self.tx_count,
            buy_count: self.buy_count,
            sell_count: self.sell_count,
            total_volume_sol: self.total_volume_sol,
        }
    }
}

#[derive(Debug)]
pub struct TxIntelligenceEngine {
    pub state: TxIntelligenceState,
    pub config: TxIntelligenceConfig,
    signer_stats: HashMap<String, SignerBehaviorStats>,
    tx_timestamps_sorted: Vec<u64>,
    tx_volumes: Vec<f64>,
    total_volume_sol: f64,
    current_consecutive_buys: usize,
    max_consecutive_buys: usize,
    dev_wallet: Option<String>,
    first_signer: Option<String>,
    dev_buy_total_sol: f64,
    dev_buy_volume_total_sol: f64,
    dev_sell_total_sol: f64,
    dev_initial_buy_tokens: Option<f64>,
    tx_keys_seen: HashSet<TxKey>,
    tx_keys_fifo: VecDeque<TxKey>,
    fingerprint_agg: Option<FingerprintAggregator>,
    fingerprint_slot: Option<u64>,
    fingerprint_t0_ms: u64,
}

impl TxIntelligenceEngine {
    #[must_use]
    pub fn new(
        config: TxIntelligenceConfig,
        candidate_snapshot: &EnhancedCandidate,
        dev_wallet: Option<Pubkey>,
    ) -> Self {
        let mut engine = Self {
            state: TxIntelligenceState::default(),
            config,
            signer_stats: HashMap::new(),
            tx_timestamps_sorted: Vec::new(),
            tx_volumes: Vec::new(),
            total_volume_sol: 0.0,
            current_consecutive_buys: 0,
            max_consecutive_buys: 0,
            dev_wallet: dev_wallet.map(|value| value.to_string()),
            first_signer: None,
            dev_buy_total_sol: 0.0,
            dev_buy_volume_total_sol: 0.0,
            dev_sell_total_sol: 0.0,
            dev_initial_buy_tokens: None,
            tx_keys_seen: HashSet::new(),
            tx_keys_fifo: VecDeque::new(),
            fingerprint_agg: None,
            fingerprint_slot: candidate_snapshot.slot,
            fingerprint_t0_ms: candidate_snapshot.timestamp,
        };
        engine.rebuild_fingerprint_aggregator();
        engine
    }

    #[must_use]
    pub const fn state(&self) -> &TxIntelligenceState {
        &self.state
    }

    #[must_use]
    pub const fn total_tx_count(&self) -> u64 {
        self.state.total_tx
    }

    #[must_use]
    pub fn unique_signer_count(&self) -> usize {
        self.state.unique_signers.len()
    }

    #[must_use]
    pub const fn dev_has_sold(&self) -> bool {
        self.state.dev_has_sold
    }

    pub fn set_dev_wallet(&mut self, dev_wallet: Option<Pubkey>) {
        self.dev_wallet = dev_wallet.map(|value| value.to_string());
        self.refresh_dev_metrics_from_signer_stats();
        self.rebuild_fingerprint_aggregator();
    }

    pub fn update_fingerprint_anchor(
        &mut self,
        slot: Option<u64>,
        timestamp_ms: Option<u64>,
        dev_wallet: Option<Pubkey>,
    ) {
        self.fingerprint_slot = slot.or(self.fingerprint_slot);
        if let Some(timestamp_ms) = timestamp_ms {
            self.fingerprint_t0_ms = timestamp_ms;
        }
        if let Some(dev_wallet) = dev_wallet {
            self.dev_wallet = Some(dev_wallet.to_string());
            self.refresh_dev_metrics_from_signer_stats();
        }
        self.rebuild_fingerprint_aggregator();
    }

    pub fn on_transaction(&mut self, tx: &PoolTransaction) {
        self.ingest_fingerprint(tx);

        let tx_key = tx_key_for(tx);
        if let Some(ref tx_key) = tx_key {
            if self.tx_keys_seen.contains(tx_key) {
                return;
            }
        }

        if tx.volume_sol < self.config.min_sol_threshold {
            self.state.dust_tx_count = self.state.dust_tx_count.saturating_add(1);
            return;
        }

        if let Some(tx_key) = tx_key {
            self.track_tx_key(tx_key);
        }

        let event_ts_ms = tx_epoch_like_event_ts_ms(tx);
        if self.first_signer.is_none() {
            self.first_signer = Some(tx.signer.clone());
        }

        self.state.total_tx = self.state.total_tx.saturating_add(1);
        if !tx.success {
            self.state.failed_tx_count = self.state.failed_tx_count.saturating_add(1);
        }
        if tx.is_buy {
            self.state.total_buys = self.state.total_buys.saturating_add(1);
            self.state.buy_volume_sol += tx.volume_sol;
        } else {
            self.state.total_sells = self.state.total_sells.saturating_add(1);
            self.state.sell_volume_sol += tx.volume_sol;
        }
        self.total_volume_sol += tx.volume_sol;
        self.tx_volumes.push(tx.volume_sol);

        let ts_insert_pos = self
            .tx_timestamps_sorted
            .partition_point(|timestamp| *timestamp <= event_ts_ms);
        self.tx_timestamps_sorted.insert(ts_insert_pos, event_ts_ms);
        self.recompute_timing_state();

        let signer_key = Pubkey::try_from(tx.signer.as_str()).ok();
        if let Some(signer_key) = signer_key {
            self.state.unique_signers.insert(signer_key);
            *self
                .state
                .signer_volume_map
                .entry(signer_key)
                .or_insert(0.0) += tx.volume_sol;
        }

        let signer_stats = self.signer_stats.entry(tx.signer.clone()).or_default();
        signer_stats.tx_count += 1;
        signer_stats.total_volume_sol += tx.volume_sol;
        if tx.is_buy {
            signer_stats.buy_count += 1;
            signer_stats.buy_volume_sol += tx.volume_sol;
            if signer_stats.first_buy_volume_sol.is_none() {
                signer_stats.first_buy_volume_sol = Some(tx.volume_sol);
            }
            if signer_stats.first_buy_tokens.is_none() {
                signer_stats.first_buy_tokens = tx.token_amount_units.map(|value| value as f64);
            }
        } else {
            signer_stats.sell_count += 1;
            signer_stats.sell_volume_sol += tx.volume_sol;
        }

        if tx.is_buy {
            self.current_consecutive_buys += 1;
            self.max_consecutive_buys =
                self.max_consecutive_buys.max(self.current_consecutive_buys);
        } else {
            self.current_consecutive_buys = 0;
        }

        if tx.is_dev_buy {
            self.state.dev_buy_lamports = self
                .state
                .dev_buy_lamports
                .saturating_add(tx.dev_buy_lamports);
            if self.dev_wallet.is_none() {
                self.dev_wallet = Some(tx.signer.clone());
            }
        }

        self.refresh_dev_metrics_from_signer_stats();
    }

    #[must_use]
    pub fn compute_features(&self) -> TxIntelFeatures {
        let total_tx = self.state.total_tx as usize;
        let gatekeeper_signer_stats: HashMap<String, SignerStats> = self
            .signer_stats
            .iter()
            .map(|(signer, stats)| (signer.clone(), stats.to_gatekeeper_stats()))
            .collect();

        let velocity = compute_velocity_profile(
            &self.tx_timestamps_sorted,
            self.config.observation_window_ms,
        );
        let diversity = compute_signer_diversity(
            &gatekeeper_signer_stats,
            total_tx,
            self.total_volume_sol,
            &self.tx_timestamps_sorted,
        );
        let volume = compute_volume_sanity(
            &self.tx_volumes,
            self.state.total_buys as usize,
            self.state.total_sells as usize,
            self.total_volume_sol,
            self.state.buy_volume_sol,
            self.max_consecutive_buys,
        );
        let dev = compute_dev_behavior(
            &self.dev_wallet,
            &self.first_signer,
            self.dev_buy_total_sol,
            self.dev_buy_volume_total_sol,
            self.dev_sell_total_sol,
            self.state.dev_tx_count as usize,
            self.state.dev_has_sold,
            self.dev_initial_buy_tokens,
            total_tx,
            self.total_volume_sol,
        );

        let tx_count = self.state.total_tx;
        let unique_signers = self.state.unique_signers.len() as u64;
        let total_tx_f64 = tx_count.max(1) as f64;
        let total_volume = self.total_volume_sol;
        let dust_denominator = tx_count.saturating_add(self.state.dust_tx_count).max(1) as f64;

        TxIntelFeatures {
            tx_count,
            buy_count: self.state.total_buys,
            sell_count: self.state.total_sells,
            unique_signers,
            buy_ratio: if tx_count > 0 {
                self.state.total_buys as f64 / tx_count as f64
            } else {
                0.0
            },
            sol_buy_ratio: volume.sol_buy_ratio,
            avg_tx_sol: volume.avg_tx_sol,
            volume_cv: volume.volume_cv,
            hhi: diversity.hhi,
            volume_gini: diversity.volume_gini,
            unique_signer_ratio: unique_signers as f64 / total_tx_f64,
            avg_tx_per_signer: if unique_signers > 0 {
                tx_count as f64 / unique_signers as f64
            } else {
                0.0
            },
            same_ms_tx_ratio: self.state.same_ms_tx_count as f64 / total_tx_f64,
            bundle_suspicion_ratio: self.state.bundle_suspicion_count as f64 / total_tx_f64,
            top3_volume_pct: if total_volume > 0.0 {
                diversity.top3_volume_pct
            } else {
                0.0
            },
            dev_buy_sol: dev.dev_buy_total_sol,
            dev_volume_ratio: dev.dev_volume_ratio,
            dev_tx_ratio: dev.dev_tx_ratio,
            dev_has_sold: dev.dev_has_sold,
            interval_cv: velocity.interval_cv,
            timing_entropy: velocity.timing_entropy,
            avg_interval_ms: velocity.avg_interval_ms,
            burst_ratio: velocity.burst_ratio,
            dust_ratio: self.state.dust_tx_count as f64 / dust_denominator,
            max_tx_per_signer: diversity.max_tx_per_signer as u64,
            total_volume_sol: volume.total_volume_sol,
            min_tx_sol: volume.min_tx_sol,
            max_tx_sol: volume.max_tx_sol,
            max_consecutive_buys: volume.max_consecutive_buys as u64,
            dev_wallet_known: dev.dev_wallet_known,
            dev_initial_buy_tokens: dev.dev_initial_buy_tokens,
            dev_tx_count: dev.dev_tx_count as u64,
            dev_is_first_buyer: dev.dev_is_first_buyer,
            dust_tx_count: self.state.dust_tx_count,
            failed_tx_count: self.state.failed_tx_count,
        }
    }

    #[must_use]
    pub fn get_risk_flags(&self) -> Vec<RiskFlag> {
        let features = self.compute_features();
        self.risk_flags_for_features(&features)
    }

    #[must_use]
    pub fn snapshot(&self) -> (TxIntelFeatures, Vec<RiskFlag>) {
        let features = self.compute_features();
        let flags = self.risk_flags_for_features(&features);
        (features, flags)
    }

    #[must_use]
    pub fn fingerprint_metrics(&self) -> Option<EarlyFingerprintMetrics> {
        self.fingerprint_agg
            .as_ref()
            .map(FingerprintAggregator::finalize)
    }

    fn risk_flags_for_features(&self, features: &TxIntelFeatures) -> Vec<RiskFlag> {
        let mut flags = Vec::new();
        let detected_at_ms = self
            .tx_timestamps_sorted
            .last()
            .copied()
            .unwrap_or_default();
        let dev_known = self.dev_wallet.is_some();
        let max_tx_per_signer = self
            .signer_stats
            .values()
            .map(|stats| stats.tx_count)
            .max()
            .unwrap_or_default();

        if self.config.reject_on_dev_sell && features.dev_has_sold {
            flags.push(risk_flag(
                "dev_has_sold",
                RiskSeverity::Hard,
                detected_at_ms,
                "Developer wallet sold during the observation window".to_string(),
            ));
        }

        if features.tx_count >= 2 && features.interval_cv < 0.08 && features.avg_interval_ms < 30.0
        {
            flags.push(risk_flag(
                "extreme_bot_timing",
                RiskSeverity::Hard,
                detected_at_ms,
                format!(
                    "interval_cv={:.3} avg_interval_ms={:.1}",
                    features.interval_cv, features.avg_interval_ms
                ),
            ));
        }

        if features.hhi > 0.5 {
            flags.push(risk_flag(
                "extreme_signer_concentration",
                RiskSeverity::Hard,
                detected_at_ms,
                format!("hhi={:.3}", features.hhi),
            ));
        }

        if features.interval_cv < self.config.min_interval_cv {
            flags.push(risk_flag(
                "low_interval_cv",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "interval_cv={:.3} < {:.3}",
                    features.interval_cv, self.config.min_interval_cv
                ),
            ));
        }

        if features.interval_cv > self.config.max_interval_cv {
            flags.push(risk_flag(
                "high_interval_cv",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "interval_cv={:.3} > {:.3}",
                    features.interval_cv, self.config.max_interval_cv
                ),
            ));
        }

        if features.timing_entropy < self.config.min_timing_entropy {
            flags.push(risk_flag(
                "low_timing_entropy",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "timing_entropy={:.3} < {:.3}",
                    features.timing_entropy, self.config.min_timing_entropy
                ),
            ));
        }

        if features.timing_entropy > self.config.max_timing_entropy {
            flags.push(risk_flag(
                "high_timing_entropy",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "timing_entropy={:.3} > {:.3}",
                    features.timing_entropy, self.config.max_timing_entropy
                ),
            ));
        }

        if features.avg_interval_ms < self.config.min_avg_interval_ms
            || features.avg_interval_ms > self.config.max_avg_interval_ms
        {
            flags.push(risk_flag(
                "avg_interval_out_of_range",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "avg_interval_ms={:.1} not in [{:.1}, {:.1}]",
                    features.avg_interval_ms,
                    self.config.min_avg_interval_ms,
                    self.config.max_avg_interval_ms
                ),
            ));
        }

        if features.burst_ratio > self.config.max_burst_ratio {
            flags.push(risk_flag(
                "high_burst_ratio",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "burst_ratio={:.3} > {:.3}",
                    features.burst_ratio, self.config.max_burst_ratio
                ),
            ));
        }

        if features.bundle_suspicion_ratio > self.config.max_same_ms_tx_ratio {
            flags.push(risk_flag(
                "bundle_suspicion",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!(
                    "bundle_ratio={:.3} > {:.3}",
                    features.bundle_suspicion_ratio, self.config.max_same_ms_tx_ratio
                ),
            ));
        }

        if features.unique_signer_ratio < self.config.min_unique_ratio
            || features.unique_signer_ratio > self.config.max_unique_ratio
        {
            flags.push(risk_flag(
                "unique_ratio_out_of_range",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "unique_signer_ratio={:.3} not in [{:.3}, {:.3}]",
                    features.unique_signer_ratio,
                    self.config.min_unique_ratio,
                    self.config.max_unique_ratio
                ),
            ));
        }

        if features.hhi > self.config.max_hhi {
            flags.push(risk_flag(
                "high_hhi",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!("hhi={:.3} > {:.3}", features.hhi, self.config.max_hhi),
            ));
        }

        if max_tx_per_signer > self.config.max_tx_per_signer {
            flags.push(risk_flag(
                "high_tx_per_signer",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "max_tx_per_signer={} > {}",
                    max_tx_per_signer, self.config.max_tx_per_signer
                ),
            ));
        }

        if features.volume_gini > self.config.max_volume_gini {
            flags.push(risk_flag(
                "high_volume_gini",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "volume_gini={:.3} > {:.3}",
                    features.volume_gini, self.config.max_volume_gini
                ),
            ));
        }

        if features.top3_volume_pct > self.config.max_top3_volume_pct {
            flags.push(risk_flag(
                "top3_volume_dominance",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!(
                    "top3_volume_pct={:.3} > {:.3}",
                    features.top3_volume_pct, self.config.max_top3_volume_pct
                ),
            ));
        }

        if self.state.dust_tx_count < self.config.min_dust_filtered_count {
            flags.push(risk_flag(
                "low_dust_count",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "dust_tx_count={} < {}",
                    self.state.dust_tx_count, self.config.min_dust_filtered_count
                ),
            ));
        }

        if dev_known && features.dev_buy_sol < self.config.min_dev_buy_sol {
            flags.push(risk_flag(
                "dev_buy_too_small",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "dev_buy_sol={:.3} < {:.3}",
                    features.dev_buy_sol, self.config.min_dev_buy_sol
                ),
            ));
        }

        if features.dev_buy_sol > self.config.max_dev_buy_sol {
            flags.push(risk_flag(
                "dev_buy_too_large",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!(
                    "dev_buy_sol={:.3} > {:.3}",
                    features.dev_buy_sol, self.config.max_dev_buy_sol
                ),
            ));
        }

        if features.dev_tx_ratio > self.config.max_dev_tx_ratio {
            flags.push(risk_flag(
                "high_dev_tx_ratio",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!(
                    "dev_tx_ratio={:.3} > {:.3}",
                    features.dev_tx_ratio, self.config.max_dev_tx_ratio
                ),
            ));
        }

        if dev_known && features.dev_tx_ratio < self.config.min_dev_tx_ratio {
            flags.push(risk_flag(
                "low_dev_tx_ratio",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "dev_tx_ratio={:.3} < {:.3}",
                    features.dev_tx_ratio, self.config.min_dev_tx_ratio
                ),
            ));
        }

        if features.dev_volume_ratio > self.config.max_dev_volume_ratio {
            flags.push(risk_flag(
                "high_dev_volume_ratio",
                RiskSeverity::Soft(2),
                detected_at_ms,
                format!(
                    "dev_volume_ratio={:.3} > {:.3}",
                    features.dev_volume_ratio, self.config.max_dev_volume_ratio
                ),
            ));
        }

        if dev_known
            && self.config.min_dev_volume_ratio > 0.0
            && features.dev_volume_ratio < self.config.min_dev_volume_ratio
        {
            flags.push(risk_flag(
                "low_dev_volume_ratio",
                RiskSeverity::Soft(1),
                detected_at_ms,
                format!(
                    "dev_volume_ratio={:.3} < {:.3}",
                    features.dev_volume_ratio, self.config.min_dev_volume_ratio
                ),
            ));
        }

        flags
    }

    fn ingest_fingerprint(&mut self, tx: &PoolTransaction) {
        let Some(ref mut fingerprint_agg) = self.fingerprint_agg else {
            return;
        };
        let Some(event) = pool_tx_to_fingerprint_event(tx) else {
            return;
        };
        if fingerprint_agg.in_window(&event) {
            fingerprint_agg.ingest(&event);
        }
    }

    fn refresh_dev_metrics_from_signer_stats(&mut self) {
        self.dev_buy_total_sol = 0.0;
        self.dev_buy_volume_total_sol = 0.0;
        self.dev_sell_total_sol = 0.0;
        self.dev_initial_buy_tokens = None;
        self.state.dev_tx_count = 0;
        self.state.dev_has_sold = false;

        let Some(dev_wallet) = self.dev_wallet.as_ref() else {
            return;
        };
        let Some(stats) = self.signer_stats.get(dev_wallet) else {
            return;
        };

        self.dev_buy_total_sol = stats.first_buy_volume_sol.unwrap_or(0.0);
        self.dev_buy_volume_total_sol = stats.buy_volume_sol;
        self.dev_sell_total_sol = stats.sell_volume_sol;
        self.dev_initial_buy_tokens = stats.first_buy_tokens;
        self.state.dev_tx_count = stats.tx_count as u64;
        self.state.dev_has_sold = stats.sell_count > 0;
    }

    fn rebuild_fingerprint_aggregator(&mut self) {
        self.fingerprint_agg = Some(FingerprintAggregator::new(
            self.config.fingerprint.clone(),
            self.fingerprint_slot.unwrap_or(u64::MAX),
            self.fingerprint_slot.is_some(),
            self.fingerprint_t0_ms,
            self.fingerprint_slot.map(|_| GENESIS_TOKEN_RESERVES_RAW),
            PUMPFUN_TOKEN_DECIMALS,
            self.dev_wallet.clone(),
        ));
    }

    fn recompute_timing_state(&mut self) {
        self.state.tx_intervals_ms = self
            .tx_timestamps_sorted
            .windows(2)
            .map(|window| window[1].saturating_sub(window[0]))
            .filter(|interval| *interval > 0)
            .collect();

        self.state.same_ms_tx_count = self
            .tx_timestamps_sorted
            .windows(2)
            .filter(|window| window[1].saturating_sub(window[0]) == 0)
            .count() as u64;

        self.state.bundle_suspicion_count = self
            .tx_timestamps_sorted
            .windows(2)
            .filter(|window| window[1].saturating_sub(window[0]) < BUNDLE_CLUSTER_THRESHOLD_MS)
            .count() as u64;

        self.state.burst_windows.clear();
        if let Some(first_ts_ms) = self.tx_timestamps_sorted.first().copied() {
            let burst_end_ms = first_ts_ms.saturating_add(self.config.burst_window_ms.max(1));
            let tx_count = self
                .tx_timestamps_sorted
                .iter()
                .take_while(|timestamp| **timestamp <= burst_end_ms)
                .count() as u64;
            if tx_count > 0 {
                self.state.burst_windows.push(BurstWindow {
                    start_ts_ms: first_ts_ms,
                    end_ts_ms: burst_end_ms,
                    tx_count,
                });
            }
        }
    }

    fn track_tx_key(&mut self, tx_key: TxKey) {
        self.tx_keys_seen.insert(tx_key.clone());
        self.tx_keys_fifo.push_back(tx_key);
        while self.tx_keys_fifo.len() > self.config.tx_key_capacity {
            if let Some(oldest) = self.tx_keys_fifo.pop_front() {
                self.tx_keys_seen.remove(&oldest);
            }
        }
    }
}

fn risk_flag(
    flag_id: &'static str,
    severity: RiskSeverity,
    detected_at_ms: u64,
    detail: String,
) -> RiskFlag {
    RiskFlag {
        flag_id: Cow::Borrowed(flag_id),
        severity,
        detected_at_ms,
        detail,
    }
}

fn tx_epoch_like_event_ts_ms(tx: &PoolTransaction) -> u64 {
    if let Some(explicit_event_ts_ms) = tx.effective_event_ts_ms() {
        explicit_event_ts_ms
    } else {
        wallclock_epoch_ms()
    }
}

fn tx_ordering_ts_ms(tx: &PoolTransaction) -> u64 {
    if let Some(explicit_event_ts_ms) = tx.compat_event_ts_ms() {
        explicit_event_ts_ms
    } else if tx.arrival_ts_ms > 0 {
        tx.arrival_ts_ms
    } else {
        wallclock_epoch_ms()
    }
}

fn wallclock_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn tx_key_for(tx: &PoolTransaction) -> Option<TxKey> {
    let event_ts_ms = tx_ordering_ts_ms(tx);
    if event_ts_ms == 0 {
        return None;
    }
    let signature = if tx.signature.is_empty() {
        None
    } else {
        Signature::from_str(&tx.signature).ok()
    };
    let has_ordering_info = signature.is_some() || tx.event_ordinal.is_some();
    let fallback_counter = if has_ordering_info {
        0
    } else {
        fallback_counter_for_tx(tx, event_ts_ms)
    };
    TxKey::new(
        event_ts_ms,
        tx.slot,
        tx.event_ordinal,
        signature,
        fallback_counter,
    )
    .ok()
}

fn fallback_counter_for_tx(tx: &PoolTransaction, event_ts_ms: u64) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    event_ts_ms.hash(&mut hasher);
    tx.signer.hash(&mut hasher);
    tx.is_buy.hash(&mut hasher);
    tx.volume_sol.to_bits().hash(&mut hasher);
    tx.event_ordinal.hash(&mut hasher);
    if let Some(price) = tx.price_quote {
        price.to_bits().hash(&mut hasher);
    }
    if let Some(lamports) = tx.sol_amount_lamports {
        lamports.hash(&mut hasher);
    }
    hasher.finish()
}

fn pool_tx_to_fingerprint_event(tx: &PoolTransaction) -> Option<FingerprintTxEvent> {
    if matches!(tx.semantic.event_truth_kind, EventTruthKind::Synthetic) {
        return None;
    }

    if !matches!(tx.semantic.slot_quality, SlotQuality::Present) {
        return None;
    }

    let slot = tx.slot?;
    let signer = tx.signer.clone();

    let mut token_deltas = Vec::new();
    if let Some(token_units) = tx.token_amount_units {
        let delta_raw = if tx.is_buy {
            token_units as i128
        } else {
            -(token_units as i128)
        };
        token_deltas.push(TokenDelta {
            owner: signer.clone(),
            delta_raw,
            decimals: PUMPFUN_TOKEN_DECIMALS,
        });
    }

    let mut sol_pre_balances = HashMap::new();
    if let Some(pre_balance_lamports) = tx.signer_pre_balance_lamports {
        sol_pre_balances.insert(signer.clone(), pre_balance_lamports);
    }

    Some(FingerprintTxEvent {
        slot,
        tx_index: 0,
        signature: tx.signature.clone(),
        timestamp_ms: tx_epoch_like_event_ts_ms(tx),
        is_buy: tx.is_buy,
        sol_amount_sol: tx
            .sol_amount_lamports
            .map(|lamports| lamports as f64 / LAMPORTS_PER_SOL)
            .or(Some(tx.volume_sol)),
        resolved_owner_deltas: tx.owner_token_deltas.clone(),
        token_deltas,
        sol_pre_balances,
        cu_price_micro_lamports: tx.cu_price_micro_lamports,
        compute_unit_limit: tx.compute_unit_limit,
        compute_units_consumed: tx.compute_units_consumed,
        inner_ix_count: tx.inner_ix_count,
        cpi_depth: tx.cpi_depth,
        ata_create_count: tx.ata_create_count,
        jito_tip_detected: tx.jito_tip_detected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx() -> PoolTransaction {
        PoolTransaction {
            semantic: ghost_core::EventSemanticEnvelope {
                slot_quality: SlotQuality::Present,
                ..ghost_core::EventSemanticEnvelope::default()
            },
            pool_amm_id: Pubkey::new_unique().to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 0,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 77,
            signer: Pubkey::new_unique().to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: Some(1_000_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: crate::events::RawBytesMissingReason::Unknown,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: seer::types::ToolchainFingerprintInput::default(),
            curve_data_known: false,
            curve_finality: ghost_core::CurveFinality::Speculative,
        }
    }

    #[test]
    fn tx_epoch_like_event_ts_ms_prefers_explicit_event_time_over_arrival() {
        let mut tx = make_tx();
        tx.event_time.ingress_wall_ts_ms = Some(5_000);
        tx.arrival_ts_ms = 12;

        assert_eq!(tx_epoch_like_event_ts_ms(&tx), 5_000);
    }

    #[test]
    fn tx_epoch_like_event_ts_ms_does_not_use_monotonic_arrival_as_epoch() {
        let tx = make_tx();
        let before = wallclock_epoch_ms();
        let actual = tx_epoch_like_event_ts_ms(&tx);
        let after = wallclock_epoch_ms();

        assert!(
            actual >= before && actual <= after,
            "expected wallclock fallback, got {actual} outside [{before}, {after}]"
        );
    }

    #[test]
    fn tx_epoch_like_event_ts_ms_ignores_legacy_only_timestamp() {
        let mut tx = make_tx();
        tx.timestamp_ms = 5_000;

        let before = wallclock_epoch_ms();
        let actual = tx_epoch_like_event_ts_ms(&tx);
        let after = wallclock_epoch_ms();

        assert_ne!(actual, 5_000);
        assert!(
            actual >= before && actual <= after,
            "expected wallclock fallback, got {actual} outside [{before}, {after}]"
        );
    }

    #[test]
    fn tx_ordering_ts_ms_uses_arrival_for_internal_tie_breaks() {
        let tx = make_tx();

        assert_eq!(tx_ordering_ts_ms(&tx), 77);
    }

    #[test]
    fn fingerprint_event_uses_normalized_event_time() {
        let mut tx = make_tx();
        tx.event_time.ingress_wall_ts_ms = Some(9_000);
        tx.arrival_ts_ms = 15;

        let event = pool_tx_to_fingerprint_event(&tx).expect("fingerprint event");

        assert_eq!(event.timestamp_ms, 9_000);
    }

    #[test]
    fn fingerprint_anchor_preserves_existing_t0_when_timestamp_missing() {
        let mut candidate = EnhancedCandidate::default();
        candidate.timestamp = 1_000;
        let mut engine =
            TxIntelligenceEngine::new(TxIntelligenceConfig::default(), &candidate, None);

        engine.update_fingerprint_anchor(Some(7), None, None);

        assert_eq!(engine.fingerprint_t0_ms, 1_000);
        assert_eq!(engine.fingerprint_slot, Some(7));
    }
}
