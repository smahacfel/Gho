#[cfg(test)]
use crate::components::gatekeeper::GatekeeperVerdict;
use crate::components::gatekeeper::{GatekeeperBuffer, GatekeeperIngressOutcome};
use crate::events::PoolTransaction;
use crate::tx_intelligence::{
    compute_sybil_resistance, CrossPoolVelocityConfig, CrossPoolVelocityIndex, FundingSourceConfig,
    FundingSourceIndex, TxIntelligenceConfig, TxIntelligenceEngine,
    DEFAULT_SESSION_TX_RING_CAPACITY,
};
use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::account_state_core::types::{AccountStateFeatures, AccountStateUpdate, StatePhase};
use ghost_core::checkpoint::FeatureMaterializer;
use ghost_core::checkpoint::{
    AlphaFingerprintFeatures, CheckpointEngine, CheckpointProducer, CurveReadinessFeatures,
    MaterializedFeatureSet, ObservationFeatureBuilder, SessionCheckpoint,
};
use ghost_core::session::types::{
    SessionDiagnostics, SessionId, SessionMetadata, SessionStatus, VerdictOutcome,
};
use ghost_core::shadow_ledger::TxKey;
use ghost_core::tx_intelligence::types::{RiskFlag, TxIntelFeatures};
use ghost_core::{CurveFreshnessState, LAMPORTS_PER_SOL};
use parking_lot::RwLock;
use seer::early_fingerprint::EarlyFingerprintMetrics;
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub type SharedSession = Arc<RwLock<PoolObservationSession>>;

pub struct PoolObservationSession {
    pub session_id: SessionId,
    pub pool_amm_id: Pubkey,
    pub base_mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub dev_wallet: Option<Pubkey>,
    pub candidate_snapshot: EnhancedCandidate,
    pub created_at_wall_ms: u64,
    pub created_at_instant: Instant,
    pub deadline_wall_ms: u64,
    pub status: SessionStatus,
    pub tx_buffer: VecDeque<Arc<PoolTransaction>>,
    pub tx_keys_seen: HashSet<TxKey>,
    pub highest_seen_ts_ms: u64,
    pub account_state_core: Arc<AccountStateReducer>,
    pub account_features: AccountStateFeatures,
    pub gatekeeper_buffer: GatekeeperBuffer,
    pub tx_intelligence: TxIntelligenceEngine,
    pub tx_intel_features: TxIntelFeatures,
    pub cross_pool_velocity_index: Arc<CrossPoolVelocityIndex>,
    pub cross_pool_velocity_config: CrossPoolVelocityConfig,
    pub funding_source_index: Arc<FundingSourceIndex>,
    pub funding_source_config: FundingSourceConfig,
    pub checkpoint_engine: CheckpointEngine,
    pub feature_builder: ObservationFeatureBuilder,
    pub checkpoints: Vec<SessionCheckpoint>,
    pub diagnostics: SessionDiagnostics,
    pub active_risk_flags: Vec<RiskFlag>,
    pub verdict: Option<VerdictOutcome>,
}

impl PoolObservationSession {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        session_id: SessionId,
        pool_amm_id: Pubkey,
        base_mint: Pubkey,
        bonding_curve: Pubkey,
        dev_wallet: Option<Pubkey>,
        candidate_snapshot: EnhancedCandidate,
        created_at_wall_ms: u64,
        deadline_wall_ms: u64,
        gatekeeper_config: &GatekeeperV2Config,
        tx_intelligence_config: TxIntelligenceConfig,
    ) -> Self {
        Self::new_with_account_state_core(
            session_id,
            pool_amm_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            Arc::new(AccountStateReducer::new()),
            created_at_wall_ms,
            deadline_wall_ms,
            gatekeeper_config,
            tx_intelligence_config,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new_with_account_state_core(
        session_id: SessionId,
        pool_amm_id: Pubkey,
        base_mint: Pubkey,
        bonding_curve: Pubkey,
        dev_wallet: Option<Pubkey>,
        candidate_snapshot: EnhancedCandidate,
        account_state_core: Arc<AccountStateReducer>,
        created_at_wall_ms: u64,
        deadline_wall_ms: u64,
        gatekeeper_config: &GatekeeperV2Config,
        tx_intelligence_config: TxIntelligenceConfig,
    ) -> Self {
        let mut gatekeeper_buffer = GatekeeperBuffer::new(pool_amm_id, gatekeeper_config);
        gatekeeper_buffer.set_registered_wall_t0(created_at_wall_ms);
        gatekeeper_buffer.set_deadline_wall_ts_ms(deadline_wall_ms);
        let (curve_t0, curve_t0_source) = if candidate_snapshot.timestamp > 0 {
            (candidate_snapshot.timestamp, "candidate_event")
        } else {
            (created_at_wall_ms, "registered_wall")
        };
        gatekeeper_buffer.set_curve_t0_with_source(curve_t0, curve_t0_source);
        let tx_intelligence =
            TxIntelligenceEngine::new(tx_intelligence_config, &candidate_snapshot, dev_wallet);

        let mut session = Self {
            session_id,
            pool_amm_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            created_at_wall_ms,
            created_at_instant: Instant::now(),
            deadline_wall_ms,
            status: SessionStatus::Created,
            tx_buffer: VecDeque::with_capacity(DEFAULT_SESSION_TX_RING_CAPACITY),
            tx_keys_seen: HashSet::new(),
            highest_seen_ts_ms: 0,
            account_state_core,
            account_features: AccountStateFeatures::default(),
            gatekeeper_buffer,
            tx_intel_features: tx_intelligence.compute_features(),
            tx_intelligence,
            cross_pool_velocity_index: Arc::new(CrossPoolVelocityIndex::new()),
            cross_pool_velocity_config: CrossPoolVelocityConfig::from_gatekeeper_config(
                gatekeeper_config,
            ),
            funding_source_index: Arc::new(FundingSourceIndex::new()),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(gatekeeper_config),
            checkpoint_engine: CheckpointEngine::default(),
            feature_builder: ObservationFeatureBuilder,
            checkpoints: Vec::new(),
            diagnostics: SessionDiagnostics::default(),
            active_risk_flags: Vec::new(),
            verdict: None,
        };
        session.refresh_from_gatekeeper();
        session.sync_from_account_state_core_on_open();
        session
    }

    /// Test-only helper retained for in-crate suites that still assert
    /// legacy inline-verdict parity.
    ///
    /// Production/runtime code and external integration tests must use
    /// `ingest_transaction(...)` together with feature evaluation.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn legacy_test_verdict_from_transaction(
        &mut self,
        tx: Arc<PoolTransaction>,
    ) -> GatekeeperVerdict {
        self.tx_intelligence.on_transaction(tx.as_ref());
        self.refresh_tx_intelligence_snapshot();

        let prior_unique = self.gatekeeper_buffer.unique_tx_key_count();
        let verdict = self
            .gatekeeper_buffer
            .legacy_test_verdict_from_transaction(tx.clone());
        let accepted_unique = self.gatekeeper_buffer.unique_tx_key_count() > prior_unique;

        if accepted_unique {
            let pool_id = self.pool_amm_id.to_string();
            self.cross_pool_velocity_index.observe_transaction(
                pool_id.as_str(),
                tx.as_ref(),
                &self.cross_pool_velocity_config,
            );
            if let Some(tx_key) = GatekeeperBuffer::tx_key_for(tx.as_ref()) {
                self.tx_keys_seen.insert(tx_key);
            }
            if self.tx_buffer.len() == DEFAULT_SESSION_TX_RING_CAPACITY {
                self.tx_buffer.pop_front();
            }
            self.tx_buffer.push_back(tx);
            self.diagnostics.total_tx_seen = self.diagnostics.total_tx_seen.saturating_add(1);
            if matches!(self.status, SessionStatus::Created) {
                self.status = SessionStatus::Accumulating;
            }
        }

        self.refresh_from_gatekeeper();

        verdict
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn on_transaction(&mut self, tx: Arc<PoolTransaction>) -> GatekeeperVerdict {
        self.legacy_test_verdict_from_transaction(tx)
    }

    /// Production ingest path for PR6 trigger cutover.
    pub fn ingest_transaction(&mut self, tx: Arc<PoolTransaction>) -> GatekeeperIngressOutcome {
        self.tx_intelligence.on_transaction(tx.as_ref());
        self.refresh_tx_intelligence_snapshot();

        let prior_unique = self.gatekeeper_buffer.unique_tx_key_count();
        let outcome = self
            .gatekeeper_buffer
            .ingest_transaction_tracking_only(tx.clone());
        let accepted_unique = self.gatekeeper_buffer.unique_tx_key_count() > prior_unique;

        if accepted_unique {
            let pool_id = self.pool_amm_id.to_string();
            self.cross_pool_velocity_index.observe_transaction(
                pool_id.as_str(),
                tx.as_ref(),
                &self.cross_pool_velocity_config,
            );
            if let Some(tx_key) = GatekeeperBuffer::tx_key_for(tx.as_ref()) {
                self.tx_keys_seen.insert(tx_key);
            }
            if self.tx_buffer.len() == DEFAULT_SESSION_TX_RING_CAPACITY {
                self.tx_buffer.pop_front();
            }
            self.tx_buffer.push_back(tx);
            self.diagnostics.total_tx_seen = self.diagnostics.total_tx_seen.saturating_add(1);
            if matches!(self.status, SessionStatus::Created) {
                self.status = SessionStatus::Accumulating;
            }
        }

        self.refresh_from_gatekeeper();

        outcome
    }

    pub fn on_account_update(&mut self, update: &AccountStateUpdate) {
        let _ = self.account_state_core.apply_account_update(update.clone());
        self.on_account_state_core_updated();
    }

    pub fn on_account_state_core_updated(&mut self) {
        if let Some(features) = self.account_state_core.get_features(&self.base_mint) {
            tracing::info!(
                pool = %self.pool_amm_id,
                base_mint = %self.base_mint,
                update_count = features.update_count,
                state_phase = ?features.state_phase,
                curve_finality = %features.curve_finality.as_str(),
                bonding_progress = features.bonding_progress,
                market_cap_sol = features.market_cap_sol,
                "DIAG_SESSION_ACCOUNT_REFRESH"
            );
            if self.account_features.update_count == 0 && features.update_count > 0 {
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64;
                let latency_ms = now_ms.saturating_sub(self.created_at_wall_ms);
                ::metrics::histogram!("canonical_first_update_latency_ms", latency_ms as f64);
            }
            self.account_features = features;
        } else {
            tracing::warn!(
                pool = %self.pool_amm_id,
                base_mint = %self.base_mint,
                "DIAG_SESSION_ACCOUNT_REFRESH_MISSING"
            );
        }
        self.diagnostics.total_account_updates =
            self.diagnostics.total_account_updates.saturating_add(1);
        if matches!(self.status, SessionStatus::Created) {
            self.status = SessionStatus::Accumulating;
        }
    }

    pub fn sync_from_account_state_core_on_open(&mut self) {
        let Some(features) = self.account_state_core.get_features(&self.base_mint) else {
            return;
        };

        if self.account_features.update_count == 0 && features.update_count > 0 {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            let latency_ms = now_ms.saturating_sub(self.created_at_wall_ms);
            ::metrics::histogram!("canonical_first_update_latency_ms", latency_ms as f64);
        }

        self.account_features = features;
        if matches!(self.status, SessionStatus::Created) && self.account_features.update_count > 0 {
            self.status = SessionStatus::Accumulating;
        }
    }

    pub fn try_checkpoint(&mut self, now_ms: u64) {
        let account_features = self.current_account_features();
        if self.tx_intel_features.tx_count == 0 && account_features.update_count == 0 {
            return;
        }

        let trigger = self.checkpoint_engine.evaluate_trigger(
            now_ms,
            self.checkpoints.last(),
            &self.tx_intel_features,
            &self.active_risk_flags,
            self.gatekeeper_buffer.latest_price_impact_pct(),
        );
        if trigger.is_none() {
            return;
        }

        let checkpoint = self.checkpoint_engine.create_checkpoint(
            &account_features,
            &self.tx_intel_features,
            &self.active_risk_flags,
        );
        self.checkpoints.push(checkpoint);
        self.diagnostics.checkpoint_count = self.checkpoints.len() as u32;
    }

    fn current_curve_readiness(&self) -> CurveReadinessFeatures {
        let curve_dynamics = self.gatekeeper_buffer.current_curve_dynamics();
        if self.account_state_core.is_canonical(&self.base_mint) {
            if let Some(features) = self.account_state_core.get_features(&self.base_mint) {
                let update_count = u32::try_from(features.update_count).unwrap_or(u32::MAX);
                return CurveReadinessFeatures {
                    is_ready: true,
                    freshness: if features.curve_finality.is_finalized() {
                        CurveFreshnessState::Committed
                    } else {
                        CurveFreshnessState::Fresh
                    },
                    finality: features.curve_finality.normalized(true),
                    curve_data_known: true,
                    price_sample_count: curve_dynamics.price_data_points.max(update_count as usize)
                        as u32,
                    t0_event_ts_ms: self.gatekeeper_buffer.curve_t0_event_ts_ms(),
                    wait_elapsed_ms: self.gatekeeper_buffer.curve_wait_elapsed_ms(),
                };
            }
        }

        CurveReadinessFeatures {
            is_ready: self.gatekeeper_buffer.curve_ready(),
            freshness: self.gatekeeper_buffer.curve_quality(),
            finality: self.gatekeeper_buffer.curve_finality_state(),
            curve_data_known: curve_dynamics.curve_data_known,
            price_sample_count: curve_dynamics.price_data_points as u32,
            t0_event_ts_ms: self.gatekeeper_buffer.curve_t0_event_ts_ms(),
            wait_elapsed_ms: self.gatekeeper_buffer.curve_wait_elapsed_ms(),
        }
    }

    #[must_use]
    pub fn materialize_features(&self) -> MaterializedFeatureSet {
        let account_features = self.current_account_features();
        let mut materialized = self.feature_builder.materialize(
            account_features.clone(),
            self.tx_intel_features.clone(),
            &self.checkpoints,
            self.active_risk_flags.clone(),
            self.session_metadata(),
        );

        let curve_dynamics = self.gatekeeper_buffer.current_curve_dynamics();
        materialized
            .checkpoint_features
            .single_tx_max_price_impact_pct = materialized
            .checkpoint_features
            .single_tx_max_price_impact_pct
            .max(curve_dynamics.max_single_tx_price_impact_pct);
        materialized.checkpoint_features.max_single_sell_impact_pct = materialized
            .checkpoint_features
            .max_single_sell_impact_pct
            .max(curve_dynamics.max_single_sell_impact_pct);
        materialized.checkpoint_features.trajectory_assessment =
            self.gatekeeper_buffer.current_materialized_trajectory();
        materialized.tx_segment_sequence =
            self.gatekeeper_buffer.current_segment_sequence_from_config();
        materialized.curve_readiness = self.current_curve_readiness();

        if materialized
            .checkpoint_features
            .price_change_from_first_checkpoint_pct
            .abs()
            <= f64::EPSILON
            && curve_dynamics.price_data_points >= 2
        {
            materialized
                .checkpoint_features
                .price_change_from_first_checkpoint_pct =
                (curve_dynamics.price_change_ratio - 1.0) * 100.0;
        }

        if materialized.account_features.update_count == 0 {
            let fallback_bonding_progress = self
                .candidate_snapshot
                .bonding_curve_progress
                .or_else(|| {
                    self.candidate_snapshot
                        .shadow_bonding_progress
                        .map(|progress| progress as f64 / 100.0)
                })
                .unwrap_or_else(|| {
                    if curve_dynamics.curve_data_known {
                        curve_dynamics.bonding_progress_pct / 100.0
                    } else {
                        0.0
                    }
                });
            materialized.account_features.bonding_progress = fallback_bonding_progress;
            materialized.checkpoint_features.bonding_progress = fallback_bonding_progress;
        }

        if let Some(fingerprint) = self.fingerprint_metrics() {
            materialized.alpha_fingerprint = AlphaFingerprintFeatures {
                avg_inner_ix_count_50tx: fingerprint.avg_inner_ix_count_50tx,
                sell_buy_ratio: fingerprint.sell_buy_ratio,
                compute_unit_cluster_dominance: fingerprint.compute_unit_cluster_dominance,
                static_fee_profile_ratio: fingerprint.static_fee_profile_ratio,
                jito_tip_intensity: fingerprint.jito_tip_intensity,
                early_slot_volume_dominance_buy: fingerprint.early_slot_volume_dominance_buy,
                early_top3_buy_volume_pct_3s: fingerprint.early_top3_buy_volume_pct_3s,
                fixed_size_buy_ratio: fingerprint.fixed_size_buy_ratio,
                flipper_presence_ratio: fingerprint.flipper_presence_ratio,
            };
        }

        let sybil_dev_wallet = self.dev_wallet.map(|value| value.to_string()).or_else(|| {
            self.tx_buffer
                .iter()
                .find(|tx| tx.is_buy && tx.success && tx.is_dev_buy)
                .map(|tx| tx.signer.clone())
        });
        let sybil = compute_sybil_resistance(
            self.tx_buffer.iter().map(AsRef::as_ref),
            sybil_dev_wallet.as_deref(),
        );
        materialized.sybil_resistance.fee_topology_diversity_index =
            sybil.fee_topology_diversity_index;
        materialized
            .sybil_resistance
            .dev_buyer_infrastructure_affinity = sybil.dev_buyer_infrastructure_affinity;
        materialized.sybil_resistance.spend_fraction_divergence = sybil.spend_fraction_divergence;
        materialized.sybil_resistance.demand_elasticity_score = sybil.demand_elasticity_score;
        materialized.sybil_resistance.degraded_reasons = sybil.degraded_reasons;
        materialized.sybil_resistance.buy_sample_count = sybil.buy_sample_count;
        materialized.sybil_resistance.signer_sample_count = sybil.signer_sample_count;

        let cpv_anchor_ts_ms = self.highest_seen_ts_ms.max(
            self.tx_buffer
                .iter()
                .filter(|tx| tx.is_buy && tx.success)
                .map(|tx| {
                    tx.event_time
                        .compat_event_ts_ms(Some(tx.timestamp_ms))
                        .unwrap_or(tx.timestamp_ms)
                })
                .max()
                .unwrap_or_default(),
        );
        let pool_id = self.pool_amm_id.to_string();
        let cpv = self.cross_pool_velocity_index.compute_for_transactions(
            pool_id.as_str(),
            self.tx_buffer.iter().map(AsRef::as_ref),
            Some(cpv_anchor_ts_ms),
            &self.cross_pool_velocity_config,
        );
        materialized.sybil_resistance.signer_cross_pool_velocity = cpv.signer_cross_pool_velocity;
        for reason in cpv.degraded_reasons {
            if !materialized
                .sybil_resistance
                .degraded_reasons
                .iter()
                .any(|existing| existing == &reason)
            {
                materialized.sybil_resistance.degraded_reasons.push(reason);
            }
        }

        let fsc = self.funding_source_index.compute_for_transactions(
            self.tx_buffer.iter().map(AsRef::as_ref),
            &self.funding_source_config,
        );
        materialized.sybil_resistance.funding_source_concentration =
            fsc.funding_source_concentration;
        materialized.sybil_resistance.funding_source_diagnostics = Some(fsc.diagnostics.clone());
        for reason in fsc.degraded_reasons {
            if !materialized
                .sybil_resistance
                .degraded_reasons
                .iter()
                .any(|existing| existing == &reason)
            {
                materialized.sybil_resistance.degraded_reasons.push(reason);
            }
        }

        materialized
    }

    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        self.created_at_instant
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }

    #[must_use]
    pub fn canonical_update_count(&self) -> u64 {
        if self.account_features.update_count > 0 {
            return self.account_features.update_count;
        }

        self.account_state_core
            .get_features(&self.base_mint)
            .map(|features| features.update_count)
            .unwrap_or(0)
    }

    #[must_use]
    pub fn is_expired(&self, now_wall_ms: u64) -> bool {
        now_wall_ms >= self.deadline_wall_ms
    }

    #[must_use]
    pub const fn get_status(&self) -> &SessionStatus {
        &self.status
    }

    pub fn begin_evaluation(&mut self) {
        if !matches!(
            self.status,
            SessionStatus::Decided(_) | SessionStatus::Closed
        ) {
            self.status = SessionStatus::Evaluating;
        }
    }

    pub fn resume_accumulation(&mut self) {
        if !matches!(
            self.status,
            SessionStatus::Decided(_) | SessionStatus::Closed
        ) {
            self.status = SessionStatus::Accumulating;
        }
    }

    pub fn apply_verdict(&mut self, verdict: VerdictOutcome) {
        self.verdict = Some(verdict.clone());
        self.status = SessionStatus::Decided(verdict);
    }

    pub fn close(&mut self) {
        self.status = SessionStatus::Closed;
    }

    pub fn record_reject_reason(&mut self, reason: impl Into<String>) {
        self.diagnostics.reject_reasons.push(reason.into());
    }

    pub fn update_tx_intelligence_dev_wallet(&mut self, dev_wallet: Option<Pubkey>) {
        self.dev_wallet = dev_wallet;
        self.tx_intelligence.set_dev_wallet(dev_wallet);
        self.refresh_tx_intelligence_snapshot();
    }

    pub fn update_tx_intelligence_fingerprint_anchor(
        &mut self,
        slot: Option<u64>,
        timestamp_ms: Option<u64>,
        dev_wallet: Option<Pubkey>,
    ) {
        if let Some(dev_wallet) = dev_wallet {
            self.dev_wallet = Some(dev_wallet);
        }
        self.tx_intelligence
            .update_fingerprint_anchor(slot, timestamp_ms, self.dev_wallet);
        self.refresh_tx_intelligence_snapshot();
    }

    pub fn set_checkpoint_interval_ms(&mut self, interval_ms: u64) {
        self.checkpoint_engine.config.interval_ms = interval_ms;
    }

    pub fn set_cross_pool_velocity_index(&mut self, index: Arc<CrossPoolVelocityIndex>) {
        self.cross_pool_velocity_index = index;
    }

    pub fn set_funding_source_index(&mut self, index: Arc<FundingSourceIndex>) {
        self.funding_source_index = index;
    }

    #[must_use]
    pub fn fingerprint_metrics(&self) -> Option<EarlyFingerprintMetrics> {
        self.tx_intelligence.fingerprint_metrics()
    }

    /// Sync derived observation data from the embedded legacy gatekeeper buffer.
    ///
    /// Ownership rule for PR 3: `PoolObservationSession` remains the source of
    /// truth for `created_at_wall_ms` and `deadline_wall_ms`. The embedded
    /// `GatekeeperBuffer` may mirror those values for legacy logic, but must not
    /// overwrite the session-owned timestamps during refresh.
    pub fn refresh_from_gatekeeper(&mut self) {
        self.highest_seen_ts_ms = self.gatekeeper_buffer.highest_seen_ts_ms();
        self.diagnostics.first_tx_ts_ms = self.gatekeeper_buffer.first_tx_ts_ms();
        self.diagnostics.last_tx_ts_ms =
            (self.highest_seen_ts_ms > 0).then_some(self.highest_seen_ts_ms);
    }

    #[must_use]
    pub const fn gatekeeper_buffer(&self) -> &GatekeeperBuffer {
        &self.gatekeeper_buffer
    }

    pub fn gatekeeper_buffer_mut(&mut self) -> &mut GatekeeperBuffer {
        &mut self.gatekeeper_buffer
    }

    fn current_account_features(&self) -> AccountStateFeatures {
        if let Some(features) = self.account_state_core.get_features(&self.base_mint) {
            if features.update_count > 0 {
                return features;
            }
        }

        if self.account_features.update_count > 0 {
            return self.account_features.clone();
        }

        let curve_dynamics = self.gatekeeper_buffer.current_curve_dynamics();
        let fallback_price_sol = (curve_dynamics.price_data_points > 0
            && curve_dynamics.current_price.is_finite()
            && curve_dynamics.current_price > 0.0)
            .then_some(curve_dynamics.current_price)
            .or_else(|| {
                self.candidate_snapshot
                    .expected_price
                    .filter(|value| value.is_finite() && *value > 0.0)
            })
            .unwrap_or_default();
        let fallback_market_cap_sol = (curve_dynamics.price_data_points > 0
            && curve_dynamics.current_market_cap_sol.is_finite()
            && curve_dynamics.current_market_cap_sol > 0.0)
            .then_some(curve_dynamics.current_market_cap_sol)
            .or_else(|| {
                self.candidate_snapshot
                    .shadow_market_cap
                    .map(|market_cap| market_cap as f64 / LAMPORTS_PER_SOL)
                    .filter(|value| value.is_finite() && *value > 0.0)
            })
            .unwrap_or_default();
        let fallback_bonding_progress = self
            .candidate_snapshot
            .bonding_curve_progress
            .or_else(|| {
                self.candidate_snapshot
                    .shadow_bonding_progress
                    .map(|progress| progress as f64 / 100.0)
            })
            .or_else(|| {
                (curve_dynamics.curve_data_known
                    && curve_dynamics.bonding_progress_pct.is_finite()
                    && curve_dynamics.bonding_progress_pct > 0.0)
                    .then_some((curve_dynamics.bonding_progress_pct / 100.0).clamp(0.0, 1.0))
            })
            .unwrap_or_default();
        let fallback_price_change_since_t0_pct = if curve_dynamics.price_data_points >= 2
            && curve_dynamics.price_change_ratio.is_finite()
            && curve_dynamics.price_change_ratio > 0.0
        {
            (curve_dynamics.price_change_ratio - 1.0) * 100.0
        } else {
            0.0
        };

        AccountStateFeatures {
            current_reserves: (
                self.candidate_snapshot
                    .virtual_sol_reserves
                    .unwrap_or_default(),
                0,
            ),
            price_sol: fallback_price_sol,
            market_cap_sol: fallback_market_cap_sol,
            bonding_progress: fallback_bonding_progress,
            price_change_since_t0_pct: fallback_price_change_since_t0_pct,
            reserve_velocity_sol_per_sec: 0.0,
            is_bootstrap: true,
            curve_finality: self.gatekeeper_buffer.curve_finality_state(),
            state_phase: StatePhase::Bootstrap,
            update_count: 0,
        }
    }

    fn session_metadata(&self) -> SessionMetadata {
        // Observation duration must use a single time domain.
        // `diagnostics.last_tx_ts_ms` is event-time sourced from GatekeeperBuffer,
        // while `created_at_wall_ms` is wall-clock session open time. Mixing them
        // produces bogus zero-length or overlong windows depending on clock skew
        // and tx timestamp provenance. Reuse the buffer's canonical wall-clock
        // observation duration instead.
        let observation_duration_ms = self.gatekeeper_buffer.observation_duration_ms();
        SessionMetadata {
            session_id: self.session_id,
            pool_amm_id: self.pool_amm_id,
            base_mint: self.base_mint,
            observation_duration_ms,
            is_dev_known: self.dev_wallet.is_some(),
        }
    }

    fn refresh_tx_intelligence_snapshot(&mut self) {
        let (features, risk_flags) = self.tx_intelligence.snapshot();
        self.tx_intel_features = features;
        self.active_risk_flags = risk_flags;
    }
}
