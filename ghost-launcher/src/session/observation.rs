#[cfg(test)]
use crate::components::gatekeeper::GatekeeperVerdict;
use crate::components::gatekeeper::{
    GatekeeperBuffer, GatekeeperIngressOutcome, PUMP_TOKEN_TOTAL_SUPPLY,
};
use crate::events::PoolTransaction;
use crate::tx_intelligence::{
    compute_sybil_resistance_with_ftdi, compute_velocity_profile, CrossPoolVelocityConfig,
    CrossPoolVelocityIndex, FundingSourceConfig, FundingSourceIndex,
    FundingSourceProducerConfigSnapshotV1, TxIntelligenceConfig, TxIntelligenceEngine,
    TxTimingProducerSnapshotV1,
};
use ghost_brain::config::GatekeeperV2Config;
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::account_state_core::types::{AccountStateFeatures, AccountStateUpdate, StatePhase};
use ghost_core::checkpoint::FeatureMaterializer;
use ghost_core::checkpoint::{
    AlphaFingerprintFeatures, CheckpointEngine, CheckpointProducer, CurveReadinessFeatures,
    DecisionTimeSeriesFeatures, DecisionTimeSeriesPriceSource, DecisionTimeSeriesRetentionPolicy,
    DecisionTimeSeriesRetentionStatus, DecisionTimeSeriesSourceCounts, EvidenceDegradedReason,
    EvidenceStatus, EvidenceUnavailableReason, FeatureEvidenceStatus,
    ManipulationContradictionFeatures, MaterializedEvidenceStatus, MaterializedFeatureSet,
    MetricEvidenceQuality, ObservationFeatureBuilder, OrganicBroadeningFeatures,
    PreEntryPathSummaryV1, SessionCheckpoint, SessionRegimeSnapshotV1, TemporalAnchorReachedBy,
    TemporalAnchorSnapshot, TemporalDeltaFeatures, TemporalMetricEvidenceContext,
    TemporalMetricSource, TxSegmentSequence,
};
use ghost_core::metric_contracts::{
    MetricContractDecisionSourceCutoffV1, MetricContractFoundationConfigV1,
    MetricContractProfileErrorV1, MetricContractProjectionErrorV1,
    MetricDecisionProjectionValidatedStaticContextV1, ResolvedMetricContractEffectiveConfigV1,
};
use ghost_core::session::types::{
    SessionDiagnostics, SessionId, SessionMetadata, SessionStatus, VerdictOutcome,
};
use ghost_core::shadow_ledger::TxKey;
use ghost_core::tx_intelligence::types::{RiskFlag, TxIntelFeatures};
use ghost_core::{CurveFreshnessState, LAMPORTS_PER_SOL};
use parking_lot::{Mutex, RwLock};
use seer::early_fingerprint::EarlyFingerprintMetrics;
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub type SharedSession = Arc<RwLock<PoolObservationSession>>;
pub(crate) const RCE_RECENT_WINDOW_MS_V1: u64 = 10_000;

#[derive(Debug, Error)]
pub enum MetricContractMaterializationErrorV1 {
    #[error("metric-contract materialization context is unavailable: {0}")]
    MissingContext(&'static str),
    #[error(transparent)]
    Profile(#[from] MetricContractProfileErrorV1),
    #[error(transparent)]
    Projection(#[from] MetricContractProjectionErrorV1),
    #[error(transparent)]
    Producer(#[from] crate::metric_contracts::Pr2bProducerErrorV1),
}

#[derive(Debug, Clone, Copy)]
struct TemporalCarryForwardRuntimeConfig {
    enabled: bool,
    max_staleness_ms: u64,
    event_counters_enabled: bool,
    state_metrics_enabled: bool,
    ratio_metrics_enabled: bool,
}

impl TemporalCarryForwardRuntimeConfig {
    fn from_gatekeeper_config(config: &GatekeeperV2Config) -> Self {
        Self {
            enabled: config.temporal_carry_forward_enabled,
            max_staleness_ms: config.temporal_carry_forward_max_staleness_ms.max(1),
            event_counters_enabled: config.temporal_carry_forward_event_counters_enabled,
            state_metrics_enabled: config.temporal_carry_forward_state_metrics_enabled,
            ratio_metrics_enabled: config.temporal_carry_forward_ratio_metrics_enabled,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct TemporalAnchorRawValues {
    tx_count: Option<u64>,
    buy_count: Option<u64>,
    unique_signers: Option<u64>,
    net_quote_sol: Option<f64>,
    total_volume_sol: Option<f64>,
    market_cap_sol: Option<f64>,
    price_pct: Option<f64>,
    burst_ratio: Option<f64>,
    jito_tip_intensity: Option<f64>,
    signer_cross_pool_velocity: Option<f64>,
    flipper_presence_ratio: Option<f64>,
}

#[derive(Debug, Clone, Default)]
struct RceWindowStats {
    same_ms_tx_ratio: Option<f64>,
    same_ms_extra_count: u64,
    tx_count: u64,
    burst_ratio: Option<f64>,
    unique_ratio: Option<f64>,
    top3_signer_volume_ratio: Option<f64>,
    buy_sell_ratio: Option<f64>,
    buy_count: u64,
    sell_count: u64,
}

#[derive(Debug, Clone, Copy)]
struct DecisionSeriesAccountPriceObservation {
    ts_ms: u64,
    price_sol: f64,
}

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
    metric_contract_static_context: Option<Arc<MetricDecisionProjectionValidatedStaticContextV1>>,
    metric_contract_funding_source_producer_config:
        Option<Arc<FundingSourceProducerConfigSnapshotV1>>,
    /// Last immutable one-producer snapshot produced for the exact MFS
    /// materialization. It is consumed once by the terminal PR2C logging
    /// boundary and is never serialized as part of MaterializedFeatureSet.
    pr2c_last_complete_metric_contract_snapshot:
        Mutex<Option<crate::metric_contracts::Pr2bTimedCompleteMetricContractSnapshotV1>>,
    temporal_carry_forward_config: TemporalCarryForwardRuntimeConfig,
    decision_time_series_tx_capacity: usize,
    decision_time_series_retention_policy: DecisionTimeSeriesRetentionPolicy,
    decision_series_account_price_observations: Vec<DecisionSeriesAccountPriceObservation>,
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
        let default_funding_source_config =
            FundingSourceConfig::from_gatekeeper_config(gatekeeper_config);
        let local_metric_contract_context =
            crate::metric_contracts::resolve_metric_contract_effective_config_v1(
                ghost_core::metric_contracts::MetricContractFoundationConfigV1::default(),
                gatekeeper_config,
                &tx_intelligence_config,
                &tx_intelligence_config.fingerprint,
                &default_funding_source_config,
                None,
            )
            .ok()
            .and_then(|effective_config| {
                let profile = MetricContractFoundationConfigV1 {
                    metric_contract_rollout_mode: effective_config.payload.rollout_mode,
                    metric_contract_profile: effective_config.payload.profile_id,
                }
                .resolve_profile()
                .ok()?;
                let static_context = MetricDecisionProjectionValidatedStaticContextV1::try_new(
                    effective_config.payload.rollout_mode,
                    profile,
                    effective_config,
                )
                .ok()?;
                default_funding_source_config
                    .metric_contract_producer_config_snapshot(None)
                    .ok()
                    .map(|producer_config| (Arc::new(static_context), Arc::new(producer_config)))
            });
        let tx_intelligence =
            TxIntelligenceEngine::new(tx_intelligence_config, &candidate_snapshot, dev_wallet);
        let decision_time_series_tx_capacity =
            gatekeeper_config.decision_time_series_tx_capacity.max(1);

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
            tx_buffer: VecDeque::with_capacity(decision_time_series_tx_capacity),
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
            funding_source_config: default_funding_source_config,
            metric_contract_static_context: local_metric_contract_context
                .as_ref()
                .map(|(static_context, _)| Arc::clone(static_context)),
            metric_contract_funding_source_producer_config: local_metric_contract_context
                .map(|(_, producer_config)| producer_config),
            pr2c_last_complete_metric_contract_snapshot: Mutex::new(None),
            temporal_carry_forward_config:
                TemporalCarryForwardRuntimeConfig::from_gatekeeper_config(gatekeeper_config),
            decision_time_series_tx_capacity,
            decision_time_series_retention_policy: gatekeeper_config
                .decision_time_series_retention_policy,
            decision_series_account_price_observations: Vec::new(),
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

    fn retain_decision_series_tx(&mut self, tx: Arc<PoolTransaction>) {
        while self.tx_buffer.len() >= self.decision_time_series_tx_capacity {
            self.tx_buffer.pop_front();
        }
        self.tx_buffer.push_back(tx);
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

        let prior_total_tx_count = self.gatekeeper_buffer.total_tx_count();
        let verdict = self
            .gatekeeper_buffer
            .legacy_test_verdict_from_transaction(tx.clone());
        let accepted_unique = self.gatekeeper_buffer.total_tx_count() > prior_total_tx_count;

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
            self.retain_decision_series_tx(tx);
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

        let prior_total_tx_count = self.gatekeeper_buffer.total_tx_count();
        let outcome = self
            .gatekeeper_buffer
            .ingest_transaction_tracking_only(tx.clone());
        let accepted_unique = self.gatekeeper_buffer.total_tx_count() > prior_total_tx_count;

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
            self.retain_decision_series_tx(tx);
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
        self.on_account_state_core_updated_at(Some(update.receive_ts_ms));
    }

    pub fn on_account_state_core_updated(&mut self) {
        self.on_account_state_core_updated_at(None);
    }

    pub fn on_account_state_core_updated_from_update(&mut self, update: &AccountStateUpdate) {
        self.on_account_state_core_updated_at(Some(update.receive_ts_ms));
    }

    fn on_account_state_core_updated_at(&mut self, account_update_ts_ms: Option<u64>) {
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
            self.record_decision_series_account_price_observation(account_update_ts_ms);
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

    fn record_decision_series_account_price_observation(&mut self, ts_ms: Option<u64>) {
        let Some(ts_ms) = ts_ms else {
            return;
        };
        let price_sol = self.account_features.price_sol;
        if !(price_sol.is_finite() && price_sol > 0.0) {
            return;
        }
        self.decision_series_account_price_observations
            .push(DecisionSeriesAccountPriceObservation { ts_ms, price_sol });
        self.decision_series_account_price_observations
            .sort_by_key(|observation| observation.ts_ms);
        const MAX_DECISION_SERIES_ACCOUNT_PRICE_OBSERVATIONS: usize = 512;
        let excess = self
            .decision_series_account_price_observations
            .len()
            .saturating_sub(MAX_DECISION_SERIES_ACCOUNT_PRICE_OBSERVATIONS);
        if excess > 0 {
            self.decision_series_account_price_observations
                .drain(0..excess);
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

    fn materialize_v3_organic_broadening(
        &self,
        materialized: &MaterializedFeatureSet,
    ) -> OrganicBroadeningFeatures {
        let mut features = OrganicBroadeningFeatures {
            total_tx_count: materialized.tx_intel_features.tx_count,
            total_unique_signers: materialized.tx_intel_features.unique_signers,
            ..OrganicBroadeningFeatures::default()
        };

        let Some(sequence) = materialized.tx_segment_sequence.as_ref() else {
            return features;
        };

        let segment_signers = self.materialize_v3_segment_unique_signers(sequence);
        let t0_unique = segment_signers.map_or(0, |counts| counts.0);
        let t1_unique = segment_signers.map_or(0, |counts| counts.1);
        let t2_unique = segment_signers.map_or(0, |counts| counts.2);
        let t2_new_signers = segment_signers.map_or(0, |counts| counts.3);
        let buy_ratios = [
            sequence.t0_segment.buy_ratio,
            sequence.t1_segment.buy_ratio,
            sequence.t2_segment.buy_ratio,
        ];
        let hhis = [
            sequence.t0_segment.hhi,
            sequence.t1_segment.hhi,
            sequence.t2_segment.hhi,
        ];

        features.sequence_available = true;
        features.t0_tx_count = sequence.t0_segment.tx_count;
        features.t1_tx_count = sequence.t1_segment.tx_count;
        features.t2_tx_count = sequence.t2_segment.tx_count;
        features.t0_unique_signers = t0_unique;
        features.t1_unique_signers = t1_unique;
        features.t2_unique_signers = t2_unique;
        features.t1_vs_t0_unique_signer_delta = t1_unique as i64 - t0_unique as i64;
        features.t2_vs_t1_unique_signer_delta = t2_unique as i64 - t1_unique as i64;
        features.signer_growth_t2_t0 = t2_unique as i64 - t0_unique as i64;
        features.tx_count_growth_ratio =
            growth_ratio(sequence.t2_segment.tx_count, sequence.t0_segment.tx_count);
        features.unique_signer_growth_ratio = growth_ratio(t2_unique, t0_unique);
        features.tx_count_growth_vs_signer_growth =
            features.tx_count_growth_ratio - features.unique_signer_growth_ratio;
        features.buy_ratio_mean = buy_ratios.iter().sum::<f64>() / buy_ratios.len() as f64;
        features.buy_ratio_min = buy_ratios.iter().copied().fold(f64::INFINITY, f64::min);
        features.buy_ratio_max = buy_ratios.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        features.max_segment_hhi = hhis.iter().copied().fold(0.0_f64, f64::max);
        features.min_segment_hhi = hhis.iter().copied().fold(f64::INFINITY, f64::min);
        features.hhi_delta_t2_t0 = sequence.t2_segment.hhi - sequence.t0_segment.hhi;
        features.new_signer_ratio_t2 = if t2_unique == 0 {
            0.0
        } else {
            t2_new_signers as f64 / t2_unique as f64
        };

        if !features.buy_ratio_min.is_finite() {
            features.buy_ratio_min = 0.0;
        }
        if !features.buy_ratio_max.is_finite() {
            features.buy_ratio_max = 0.0;
        }
        if !features.min_segment_hhi.is_finite() {
            features.min_segment_hhi = 0.0;
        }
        features.broadening_score = v3_broadening_score(&features);
        features.status = if !sequence.min_tx_per_segment_satisfied {
            features
                .degraded_reasons
                .push(EvidenceDegradedReason::SegmentSequencePartial);
            EvidenceStatus::InsufficientSample
        } else if segment_signers.is_none() {
            features
                .degraded_reasons
                .push(EvidenceDegradedReason::SegmentSignerCoveragePartial);
            EvidenceStatus::Degraded
        } else {
            EvidenceStatus::Clean
        };

        features
    }

    fn materialize_v3_segment_unique_signers(
        &self,
        sequence: &TxSegmentSequence,
    ) -> Option<(u64, u64, u64, u64)> {
        if self.tx_buffer.is_empty() || sequence.total_duration_ms == 0 {
            return None;
        }

        let first_ts = self.tx_buffer.iter().map(|tx| tx.timestamp_ms).min()?;
        let segment_duration = sequence.total_duration_ms as f64 / 3.0;
        let t0_end = first_ts.saturating_add(segment_duration as u64);
        let t1_end = first_ts.saturating_add((2.0 * segment_duration) as u64);
        let mut t0 = HashSet::new();
        let mut t1 = HashSet::new();
        let mut t2 = HashSet::new();

        for tx in &self.tx_buffer {
            if tx.timestamp_ms <= t0_end {
                t0.insert(tx.signer.clone());
            } else if tx.timestamp_ms <= t1_end {
                t1.insert(tx.signer.clone());
            } else {
                t2.insert(tx.signer.clone());
            }
        }

        let t2_new = t2
            .iter()
            .filter(|signer| !t0.contains(*signer) && !t1.contains(*signer))
            .count() as u64;

        Some((t0.len() as u64, t1.len() as u64, t2.len() as u64, t2_new))
    }

    fn materialize_v3_manipulation_contradictions(
        &self,
        materialized: &MaterializedFeatureSet,
    ) -> ManipulationContradictionFeatures {
        let tx = &materialized.tx_intel_features;
        let sybil = &materialized.sybil_resistance;
        let organic = &materialized.organic_broadening;
        let alpha = &materialized.alpha_fingerprint;
        let top3_signer_volume_ratio = tx.effective_top3_signer_volume_ratio();

        let momentum_without_broadening = organic.sequence_available
            && organic.tx_count_growth_ratio > 0.0
            && organic.unique_signer_growth_ratio <= 0.0;
        let volume_spike_without_new_signers = materialized
            .tx_segment_sequence
            .as_ref()
            .is_some_and(|sequence| {
                sequence.t2_segment.total_volume_sol
                    > sequence.t0_segment.total_volume_sol.max(0.000_001) * 1.5
                    && organic.new_signer_ratio_t2 <= 0.10
            });
        let high_buy_pressure_with_high_top3 =
            tx.buy_ratio >= 0.80 && top3_signer_volume_ratio >= 0.50;
        let fixed_size_or_ramping_pattern = alpha
            .fixed_size_buy_ratio
            .is_some_and(|ratio| ratio >= 0.50)
            || materialized
                .tx_segment_sequence
                .as_ref()
                .is_some_and(|sequence| {
                    sequence.t2_segment.same_size_streak >= 3
                        || (sequence.t2_segment.tx_count > sequence.t1_segment.tx_count
                            && sequence.t1_segment.tx_count > sequence.t0_segment.tx_count)
                });
        let timing_bundle_concentration =
            tx.same_ms_tx_ratio >= 0.20 || tx.bundle_suspicion_ratio >= 0.20;
        let early_top3_concentration = alpha
            .early_top3_buy_volume_pct_3s
            .is_some_and(|pct| pct >= 0.50);

        let mut reasons = Vec::new();
        for (flag, reason) in [
            (momentum_without_broadening, "momentum_without_broadening"),
            (
                volume_spike_without_new_signers,
                "volume_spike_without_new_signers",
            ),
            (
                high_buy_pressure_with_high_top3,
                "high_buy_pressure_with_high_top3",
            ),
            (
                fixed_size_or_ramping_pattern,
                "fixed_size_or_ramping_pattern",
            ),
            (timing_bundle_concentration, "timing_bundle_concentration"),
            (early_top3_concentration, "early_top3_concentration"),
        ] {
            if flag {
                reasons.push(reason.to_string());
            }
        }
        let contradiction_score = reasons.len() as f64 / 6.0;
        let status = if tx.tx_count == 0 {
            EvidenceStatus::Unavailable
        } else if organic.status != EvidenceStatus::Clean
            || !sybil.degraded_reasons.is_empty()
            || alpha.fixed_size_buy_ratio.is_none()
            || alpha.early_top3_buy_volume_pct_3s.is_none()
        {
            EvidenceStatus::Degraded
        } else {
            EvidenceStatus::Clean
        };

        ManipulationContradictionFeatures {
            same_ms_tx_ratio: tx.same_ms_tx_ratio,
            bundle_suspicion_ratio: tx.bundle_suspicion_ratio,
            top3_volume_pct: top3_signer_volume_ratio,
            hhi: tx.hhi,
            max_tx_per_signer: tx.max_tx_per_signer,
            dev_volume_ratio: tx.dev_volume_ratio,
            dev_has_sold: tx.dev_has_sold,
            fee_topology_diversity_index: sybil.fee_topology_diversity_index,
            spend_fraction_divergence: sybil.spend_fraction_divergence,
            signer_cross_pool_velocity: sybil.signer_cross_pool_velocity,
            funding_source_concentration: sybil.funding_source_concentration,
            sybil_evidence_degraded: !sybil.degraded_reasons.is_empty(),
            momentum_without_broadening,
            volume_spike_without_new_signers,
            high_buy_pressure_with_high_top3,
            fixed_size_or_ramping_pattern,
            timing_bundle_concentration,
            early_top3_concentration,
            contradiction_score,
            status,
            reasons,
            ..ManipulationContradictionFeatures::default()
        }
    }

    fn materialize_v3_evidence_status(
        &self,
        materialized: &MaterializedFeatureSet,
    ) -> MaterializedEvidenceStatus {
        let identity = if materialized.session_metadata.is_dev_known
            || materialized.tx_intel_features.dev_wallet_known
            || materialized.tx_intel_features.dev_tx_count > 0
        {
            evidence_clean()
        } else {
            evidence_fallback(vec![EvidenceDegradedReason::IdentityEvidenceFallback])
        };
        let account_state = if materialized.account_features.update_count > 0 {
            evidence_clean()
        } else {
            evidence_fallback(vec![EvidenceDegradedReason::AccountStateFallback])
        };
        let tx_intel = if materialized.tx_intel_features.tx_count > 0 {
            evidence_clean()
        } else {
            evidence_unavailable(vec![EvidenceUnavailableReason::TxIntelMissing])
        };
        let tx_segments = match materialized.tx_segment_sequence.as_ref() {
            None => evidence_unavailable(vec![EvidenceUnavailableReason::SegmentSequenceMissing]),
            Some(sequence) if !sequence.min_tx_per_segment_satisfied => {
                evidence_insufficient_sample(vec![EvidenceDegradedReason::SegmentSequencePartial])
            }
            Some(_) if self.tx_buffer.is_empty() => {
                evidence_degraded(vec![EvidenceDegradedReason::SegmentSignerCoveragePartial])
            }
            Some(_) => evidence_clean(),
        };
        let trajectory = match materialized.checkpoint_features.trajectory_checkpoint_count {
            0 => evidence_unavailable(vec![EvidenceUnavailableReason::TrajectoryMissing]),
            1 => {
                evidence_insufficient_sample(vec![EvidenceDegradedReason::TrajectoryEvidenceSparse])
            }
            _ => evidence_clean(),
        };
        let checkpoints = trajectory.clone();
        let pdd_sequence = match materialized.tx_segment_sequence.as_ref() {
            None => evidence_unavailable(vec![EvidenceUnavailableReason::PddSequenceMissing]),
            Some(sequence) if !sequence.min_tx_per_segment_satisfied => {
                evidence_insufficient_sample(vec![EvidenceDegradedReason::PddSequencePartial])
            }
            Some(_) => evidence_clean(),
        };
        let curve = if materialized.curve_readiness.curve_data_known {
            evidence_clean()
        } else if materialized.curve_readiness.price_sample_count > 0 {
            evidence_degraded(vec![EvidenceDegradedReason::CurveEvidencePartial])
        } else {
            evidence_unavailable(vec![EvidenceUnavailableReason::CurveDataMissing])
        };

        let sybil_metric_available = materialized
            .sybil_resistance
            .fee_topology_diversity_index
            .is_some()
            || materialized
                .sybil_resistance
                .dev_buyer_infrastructure_affinity
                .is_some()
            || materialized
                .sybil_resistance
                .spend_fraction_divergence
                .is_some()
            || materialized
                .sybil_resistance
                .demand_elasticity_score
                .is_some()
            || materialized
                .sybil_resistance
                .signer_cross_pool_velocity
                .is_some()
            || materialized
                .sybil_resistance
                .cpv_other_pool_activity
                .is_some()
            || materialized
                .sybil_resistance
                .funding_source_concentration
                .is_some();
        let sybil_available_metrics = [
            materialized
                .sybil_resistance
                .fee_topology_diversity_index
                .is_some(),
            materialized
                .sybil_resistance
                .dev_buyer_infrastructure_affinity
                .is_some(),
            materialized
                .sybil_resistance
                .spend_fraction_divergence
                .is_some(),
            materialized
                .sybil_resistance
                .demand_elasticity_score
                .is_some(),
            materialized
                .sybil_resistance
                .signer_cross_pool_velocity
                .is_some(),
            materialized
                .sybil_resistance
                .cpv_other_pool_activity
                .is_some(),
            materialized
                .sybil_resistance
                .funding_source_concentration
                .is_some(),
        ];
        let sybil_available_count = sybil_available_metrics
            .iter()
            .filter(|available| **available)
            .count();
        let sybil = if !materialized.sybil_resistance.degraded_reasons.is_empty() {
            evidence_degraded(vec![EvidenceDegradedReason::SybilEvidencePartial])
        } else if sybil_available_count > 0 && sybil_available_count < sybil_available_metrics.len()
        {
            evidence_degraded(vec![EvidenceDegradedReason::SybilEvidencePartial])
        } else if sybil_metric_available {
            evidence_clean()
        } else {
            evidence_unavailable(vec![EvidenceUnavailableReason::SybilMetricsMissing])
        };
        let cpv = match materialized.sybil_resistance.cpv_evidence.quality {
            MetricEvidenceQuality::Clean => evidence_clean(),
            MetricEvidenceQuality::DegradedLowSample => {
                evidence_degraded(vec![EvidenceDegradedReason::CpvEvidencePartial])
            }
            MetricEvidenceQuality::InsufficientSample => {
                evidence_insufficient_sample(vec![EvidenceDegradedReason::CpvEvidencePartial])
            }
            MetricEvidenceQuality::Stale => {
                evidence_degraded(vec![EvidenceDegradedReason::EvidenceStale])
            }
            MetricEvidenceQuality::NotAllowed => {
                evidence_unavailable(vec![EvidenceUnavailableReason::CpvMetricsMissing])
            }
            MetricEvidenceQuality::UnavailableSource
            | MetricEvidenceQuality::Unavailable
            | MetricEvidenceQuality::NotConfigured => {
                if materialized
                    .sybil_resistance
                    .signer_cross_pool_velocity
                    .is_some()
                {
                    evidence_degraded(vec![EvidenceDegradedReason::CpvEvidencePartial])
                } else if materialized
                    .sybil_resistance
                    .degraded_reasons
                    .iter()
                    .any(|reason| reason.starts_with("CPV_"))
                {
                    evidence_unavailable(vec![EvidenceUnavailableReason::CpvMetricsMissing])
                } else {
                    evidence_unavailable(vec![EvidenceUnavailableReason::CpvMetricsMissing])
                }
            }
            MetricEvidenceQuality::CarriedForward => {
                evidence_degraded(vec![EvidenceDegradedReason::CpvEvidencePartial])
            }
        };
        let fsc = if materialized
            .sybil_resistance
            .funding_source_concentration
            .is_some()
        {
            evidence_clean()
        } else if materialized
            .sybil_resistance
            .funding_source_diagnostics
            .is_some()
            || materialized
                .sybil_resistance
                .degraded_reasons
                .iter()
                .any(|reason| reason.starts_with("FSC_"))
        {
            evidence_degraded(vec![EvidenceDegradedReason::FscEvidencePartial])
        } else {
            evidence_unavailable(vec![EvidenceUnavailableReason::FscMetricsMissing])
        };
        let fsc_legacy = fsc.clone();
        let fsc_v2 = match materialized.sybil_resistance.funding_source_v2.as_ref() {
            Some(evidence)
                if evidence.status
                    == ghost_core::tx_intelligence::types::FscEvidenceStatus::Clean =>
            {
                evidence_clean()
            }
            Some(evidence)
                if evidence.status
                    == ghost_core::tx_intelligence::types::FscEvidenceStatus::Degraded =>
            {
                evidence_degraded(vec![EvidenceDegradedReason::FscEvidencePartial])
            }
            Some(_) => evidence_unavailable(vec![EvidenceUnavailableReason::FscMetricsMissing]),
            None => evidence_unavailable(vec![EvidenceUnavailableReason::FscMetricsMissing]),
        };

        let alpha_available_count = [
            materialized
                .alpha_fingerprint
                .avg_inner_ix_count_50tx
                .is_some(),
            materialized.alpha_fingerprint.sell_buy_ratio.is_some(),
            materialized
                .alpha_fingerprint
                .compute_unit_cluster_dominance
                .is_some(),
            materialized
                .alpha_fingerprint
                .static_fee_profile_ratio
                .is_some(),
            materialized.alpha_fingerprint.jito_tip_intensity.is_some(),
            materialized
                .alpha_fingerprint
                .early_slot_volume_dominance_buy
                .is_some(),
            materialized
                .alpha_fingerprint
                .early_top3_buy_volume_pct_3s
                .is_some(),
            materialized
                .alpha_fingerprint
                .fixed_size_buy_ratio
                .is_some(),
            materialized
                .alpha_fingerprint
                .flipper_presence_ratio
                .is_some(),
        ]
        .into_iter()
        .filter(|available| *available)
        .count();
        let alpha = if alpha_available_count == 9 {
            evidence_clean()
        } else if alpha_available_count > 0 {
            evidence_degraded(vec![EvidenceDegradedReason::AlphaEvidencePartial])
        } else {
            evidence_unavailable(vec![EvidenceUnavailableReason::AlphaFingerprintMissing])
        };

        let manipulation = if materialized.tx_intel_features.tx_count > 0 {
            evidence_clean()
        } else {
            evidence_unavailable(vec![EvidenceUnavailableReason::TxIntelMissing])
        };
        let organic_broadening = match materialized.organic_broadening.status {
            EvidenceStatus::Clean => evidence_clean(),
            EvidenceStatus::InsufficientSample => evidence_insufficient_sample(
                materialized.organic_broadening.degraded_reasons.clone(),
            ),
            EvidenceStatus::Unavailable => {
                evidence_unavailable(vec![EvidenceUnavailableReason::OrganicBroadeningMissing])
            }
            _ => evidence_degraded(materialized.organic_broadening.degraded_reasons.clone()),
        };
        let manipulation_contradiction = match materialized.manipulation_contradictions.status {
            EvidenceStatus::Clean => evidence_clean(),
            EvidenceStatus::Unavailable => evidence_unavailable(vec![
                EvidenceUnavailableReason::ManipulationContradictionMissing,
            ]),
            _ => evidence_degraded(vec![
                EvidenceDegradedReason::ManipulationContradictionPartial,
            ]),
        };

        MaterializedEvidenceStatus {
            identity,
            account_state,
            tx_intel,
            tx_segments,
            checkpoints,
            trajectory,
            pdd_sequence,
            curve,
            sybil,
            cpv,
            fsc,
            fsc_legacy,
            fsc_v2,
            alpha,
            manipulation,
            organic_broadening,
            manipulation_contradiction,
            execution: FeatureEvidenceStatus {
                status: EvidenceStatus::ShadowOnly,
                degraded_reasons: Vec::new(),
                unavailable_reasons: vec![EvidenceUnavailableReason::ExecutionNotRun],
            },
        }
    }

    #[must_use]
    fn materialize_v3_temporal_deltas(&self) -> TemporalDeltaFeatures {
        const ANCHORS_MS: [u64; 3] = [1_000, 2_000, 3_000];

        let sorted_txs = self.temporal_sorted_transactions();
        if sorted_txs.is_empty() {
            return TemporalDeltaFeatures {
                anchor_1s: Self::empty_temporal_anchor(ANCHORS_MS[0]),
                anchor_2s: Self::empty_temporal_anchor(ANCHORS_MS[1]),
                anchor_3s: Self::empty_temporal_anchor(ANCHORS_MS[2]),
                ..TemporalDeltaFeatures::default()
            };
        }

        let first_event_ts_ms = sorted_txs[0].2;
        let observed_end_event_elapsed_ms = sorted_txs
            .last()
            .map(|(_, _, ts)| ts.saturating_sub(first_event_ts_ms))
            .unwrap_or_default();
        let observation_elapsed_ms = self.temporal_observation_elapsed_ms();
        let configured_window_ms = self
            .deadline_wall_ms
            .saturating_sub(self.created_at_wall_ms);
        let first_price = sorted_txs
            .iter()
            .find_map(|(_, tx, _)| Self::temporal_price_value(tx));

        let mut previous_event_anchor: Option<TemporalAnchorSnapshot> = None;
        let mut previous_state_anchor: Option<TemporalAnchorSnapshot> = None;
        let mut previous_ratio_anchor: Option<TemporalAnchorSnapshot> = None;
        let mut anchors = Vec::with_capacity(ANCHORS_MS.len());

        for anchor_ms in ANCHORS_MS {
            let reached_by = Self::temporal_anchor_reached_by(
                anchor_ms,
                observed_end_event_elapsed_ms,
                observation_elapsed_ms,
                configured_window_ms,
            );
            let cutoff_ts_ms = first_event_ts_ms.saturating_add(anchor_ms);
            let raw_values =
                self.temporal_anchor_raw_values(&sorted_txs, cutoff_ts_ms, anchor_ms, first_price);
            let anchor = self.build_temporal_anchor(
                anchor_ms,
                reached_by,
                observation_elapsed_ms,
                raw_values,
                previous_event_anchor.as_ref(),
                previous_state_anchor.as_ref(),
                previous_ratio_anchor.as_ref(),
            );

            if anchor.tx_count.is_some() {
                previous_event_anchor = Some(anchor.clone());
            }
            if anchor.market_cap_sol.is_some() || anchor.price_pct.is_some() {
                previous_state_anchor = Some(anchor.clone());
            }
            if anchor.burst_ratio.is_some()
                || anchor.jito_tip_intensity.is_some()
                || anchor.signer_cross_pool_velocity.is_some()
                || anchor.flipper_presence_ratio.is_some()
            {
                previous_ratio_anchor = Some(anchor.clone());
            }
            anchors.push(anchor);
        }

        let mut features = TemporalDeltaFeatures {
            anchor_1s: anchors[0].clone(),
            anchor_2s: anchors[1].clone(),
            anchor_3s: anchors[2].clone(),
            ..TemporalDeltaFeatures::default()
        };
        self.populate_temporal_delta_pairs(&mut features);
        features.status = Self::temporal_delta_status(&features);
        features
    }

    fn empty_temporal_anchor(anchor_ms: u64) -> TemporalAnchorSnapshot {
        let unavailable = Self::temporal_context(
            MetricEvidenceQuality::Unavailable,
            TemporalMetricSource::Unavailable,
            None,
            None,
            Some("anchor_not_reached"),
        );
        TemporalAnchorSnapshot {
            anchor_ms,
            reached: false,
            reached_by: TemporalAnchorReachedBy::NotReached,
            status: MetricEvidenceQuality::Unavailable,
            event_counters_evidence: unavailable.clone(),
            state_metrics_evidence: unavailable.clone(),
            ratio_metrics_evidence: unavailable,
            ..TemporalAnchorSnapshot::default()
        }
    }

    fn temporal_observation_elapsed_ms(&self) -> u64 {
        let configured_window_ms = self
            .deadline_wall_ms
            .saturating_sub(self.created_at_wall_ms);
        let elapsed_ms = self.elapsed_ms();
        if configured_window_ms > 0 {
            elapsed_ms.min(configured_window_ms)
        } else {
            elapsed_ms
        }
    }

    fn temporal_anchor_reached_by(
        anchor_ms: u64,
        observed_end_event_elapsed_ms: u64,
        observation_elapsed_ms: u64,
        configured_window_ms: u64,
    ) -> TemporalAnchorReachedBy {
        if observed_end_event_elapsed_ms >= anchor_ms {
            TemporalAnchorReachedBy::Event
        } else if observation_elapsed_ms >= anchor_ms {
            if configured_window_ms > 0 && observation_elapsed_ms >= configured_window_ms {
                TemporalAnchorReachedBy::Deadline
            } else {
                TemporalAnchorReachedBy::ObservationElapsed
            }
        } else {
            TemporalAnchorReachedBy::NotReached
        }
    }

    fn temporal_sorted_transactions(&self) -> Vec<(usize, &PoolTransaction, u64)> {
        let mut txs: Vec<(usize, &PoolTransaction, u64)> = self
            .tx_buffer
            .iter()
            .enumerate()
            .map(|(idx, tx)| (idx, tx.as_ref(), Self::temporal_tx_event_ts_ms(tx.as_ref())))
            .collect();
        txs.sort_by_key(|(idx, tx, ts)| {
            (
                *ts,
                tx.slot.unwrap_or_default(),
                tx.tx_index.unwrap_or(u32::MAX),
                tx.event_ordinal.unwrap_or(u32::MAX),
                *idx,
            )
        });
        txs
    }

    fn temporal_tx_event_ts_ms(tx: &PoolTransaction) -> u64 {
        tx.event_time
            .compat_event_ts_ms(Some(tx.timestamp_ms))
            .unwrap_or(tx.timestamp_ms)
    }

    fn temporal_market_cap_value(tx: &PoolTransaction) -> Option<f64> {
        tx.market_cap_sol
            .filter(|value| value.is_finite() && *value > 0.0)
    }

    fn temporal_price_value(tx: &PoolTransaction) -> Option<f64> {
        if let Some(price) = tx
            .price_quote
            .filter(|value| value.is_finite() && *value > 0.0)
        {
            return Some(price);
        }
        match (
            tx.reserve_quote.or(tx.v_sol_in_bonding_curve),
            tx.reserve_base.or(tx.v_tokens_in_bonding_curve),
        ) {
            (Some(quote), Some(base)) if quote.is_finite() && base.is_finite() && base > 0.0 => {
                Some(quote / base)
            }
            _ => None,
        }
    }

    fn decision_series_tx_price(
        tx: &PoolTransaction,
    ) -> (Option<f64>, DecisionTimeSeriesPriceSource) {
        match (
            tx.reserve_quote.or(tx.v_sol_in_bonding_curve),
            tx.reserve_base.or(tx.v_tokens_in_bonding_curve),
        ) {
            (Some(quote), Some(base)) if quote.is_finite() && base.is_finite() && base > 0.0 => {
                return (Some(quote / base), DecisionTimeSeriesPriceSource::Reserve);
            }
            _ => {}
        }

        if let Some(price) = tx
            .price_quote
            .filter(|value| value.is_finite() && *value > 0.0)
        {
            return (Some(price), DecisionTimeSeriesPriceSource::Quote);
        }

        if let Some(market_cap_sol) = tx
            .market_cap_sol
            .filter(|value| value.is_finite() && *value > 0.0)
        {
            return (
                Some(market_cap_sol / PUMP_TOKEN_TOTAL_SUPPLY),
                DecisionTimeSeriesPriceSource::MarketCap,
            );
        }

        (None, DecisionTimeSeriesPriceSource::Missing)
    }

    fn decision_series_account_price_at_or_before(&self, ts_ms: u64) -> Option<f64> {
        self.decision_series_account_price_observations
            .iter()
            .rev()
            .find(|observation| observation.ts_ms <= ts_ms)
            .map(|observation| observation.price_sol)
            .filter(|value| value.is_finite() && *value > 0.0)
    }

    fn decision_series_price_for_tx(
        &self,
        tx: &PoolTransaction,
        ts_ms: u64,
    ) -> (Option<f64>, DecisionTimeSeriesPriceSource) {
        let (price, source) = Self::decision_series_tx_price(tx);
        if price.is_some() {
            return (price, source);
        }
        if let Some(account_state_price) = self.decision_series_account_price_at_or_before(ts_ms) {
            return (
                Some(account_state_price),
                DecisionTimeSeriesPriceSource::AccountState,
            );
        }
        (None, DecisionTimeSeriesPriceSource::Missing)
    }

    #[must_use]
    fn materialize_decision_time_series(&self) -> DecisionTimeSeriesFeatures {
        let sorted_txs = self.temporal_sorted_transactions();
        if sorted_txs.is_empty() {
            return DecisionTimeSeriesFeatures::default();
        }

        let first_event_ts_ms = sorted_txs[0].2;
        let retained_sample_count = sorted_txs.len() as u64;
        let total_tx_count = self.tx_intel_features.tx_count.max(retained_sample_count);
        let dropped_oldest_count = total_tx_count.saturating_sub(retained_sample_count);
        let mut features = DecisionTimeSeriesFeatures {
            status: EvidenceStatus::Clean,
            retention_status: if dropped_oldest_count > 0 {
                DecisionTimeSeriesRetentionStatus::Truncated
            } else {
                DecisionTimeSeriesRetentionStatus::Clean
            },
            retention_policy: self.decision_time_series_retention_policy,
            retention_capacity: self.decision_time_series_tx_capacity as u64,
            retained_sample_count,
            total_tx_count,
            dropped_oldest_count,
            sample_count: retained_sample_count,
            ..DecisionTimeSeriesFeatures::default()
        };
        let mut source_counts = DecisionTimeSeriesSourceCounts::default();

        for (_, tx, ts_ms) in sorted_txs {
            let offset_ms = ts_ms.saturating_sub(first_event_ts_ms).min(i64::MAX as u64) as i64;
            features.ts_offsets_ms.push(offset_ms);
            features.sol_amounts.push(if tx.volume_sol.is_finite() {
                tx.volume_sol
            } else {
                0.0
            });

            let (price, source) = self.decision_series_price_for_tx(tx, ts_ms);
            if price.is_some() {
                features.finite_price_count = features.finite_price_count.saturating_add(1);
            } else {
                features.missing_price_count = features.missing_price_count.saturating_add(1);
            }
            source_counts.increment(source);
            features.prices.push(price);
            features.price_sources.push(source);
        }

        features.interval_ms = features
            .ts_offsets_ms
            .windows(2)
            .map(|pair| pair[1].saturating_sub(pair[0]).max(0) as f64)
            .collect();
        features.d_price = features
            .prices
            .windows(2)
            .map(|pair| match (pair[0], pair[1]) {
                (Some(previous), Some(current)) => Some(current - previous),
                _ => None,
            })
            .collect();
        features.price_coverage_ratio = (features.sample_count > 0)
            .then_some(features.finite_price_count as f64 / features.sample_count as f64);
        features.source_counts = source_counts;
        if features.dropped_oldest_count > 0 {
            features.status = EvidenceStatus::Degraded;
            features
                .degraded_reasons
                .push(EvidenceDegradedReason::DecisionTimeSeriesTruncated);
        }
        if features.missing_price_count > 0 {
            features.status = EvidenceStatus::Degraded;
            features
                .degraded_reasons
                .push(EvidenceDegradedReason::DecisionTimeSeriesPricePartial);
        }

        features
    }

    fn rce_decision_path_points(series: &DecisionTimeSeriesFeatures) -> Vec<(u64, f64)> {
        series
            .ts_offsets_ms
            .iter()
            .zip(series.prices.iter())
            .filter_map(|(offset_ms, price)| {
                let offset_ms = (*offset_ms >= 0).then_some(*offset_ms as u64)?;
                let price = (*price).filter(|value| value.is_finite() && *value > 0.0)?;
                Some((offset_ms, price))
            })
            .collect()
    }

    fn rce_ret_bps(first_price: f64, current_price: f64) -> Option<f64> {
        (first_price.is_finite() && first_price > 0.0 && current_price.is_finite())
            .then_some(((current_price - first_price) / first_price) * 10_000.0)
    }

    fn rce_path_stats(
        points: &[(u64, f64)],
        horizon_ms: u64,
    ) -> (Option<f64>, Option<f64>, Option<f64>) {
        let first_price = points.first().map(|(_, price)| *price);
        let Some(first_price) = first_price else {
            return (None, None, None);
        };
        let returns: Vec<f64> = points
            .iter()
            .take_while(|(offset_ms, _)| *offset_ms <= horizon_ms)
            .filter_map(|(_, price)| Self::rce_ret_bps(first_price, *price))
            .collect();
        if returns.is_empty() {
            return (None, None, None);
        }
        let ret = returns.last().copied();
        let mfe = returns.iter().copied().reduce(f64::max);
        let mae = returns.iter().copied().reduce(f64::min);
        (ret, mfe, mae)
    }

    fn rce_return_points(points: &[(u64, f64)]) -> Vec<(u64, f64)> {
        let Some((_, first_price)) = points.first().copied() else {
            return Vec::new();
        };
        points
            .iter()
            .filter_map(|(offset_ms, price)| {
                Self::rce_ret_bps(first_price, *price).map(|ret_bps| (*offset_ms, ret_bps))
            })
            .collect()
    }

    fn rce_dwell_ms(return_points: &[(u64, f64)], threshold_bps: f64) -> u64 {
        return_points
            .windows(2)
            .filter_map(|pair| {
                let (start_ms, start_ret) = pair[0];
                let (end_ms, _) = pair[1];
                (start_ret >= threshold_bps).then_some(end_ms.saturating_sub(start_ms))
            })
            .sum()
    }

    fn rce_pullback_reclaim(
        return_points: &[(u64, f64)],
    ) -> (Option<f64>, Option<f64>, Option<f64>) {
        let Some((_, first_ret)) = return_points.first().copied() else {
            return (None, None, None);
        };
        let mut peak_ret = first_ret;
        let mut max_pullback = 0.0;
        let mut trough_at_max_pullback = first_ret;
        for (_, ret_bps) in return_points {
            if *ret_bps > peak_ret {
                peak_ret = *ret_bps;
            }
            let pullback = peak_ret - *ret_bps;
            if pullback > max_pullback {
                max_pullback = pullback;
                trough_at_max_pullback = *ret_bps;
            }
        }
        let current_ret = return_points
            .last()
            .map(|(_, ret)| *ret)
            .unwrap_or(first_ret);
        let reclaim = (current_ret - trough_at_max_pullback).max(0.0);
        let reclaim_fraction = (max_pullback > f64::EPSILON).then_some(reclaim / max_pullback);
        (
            Some(max_pullback),
            Some(reclaim),
            reclaim_fraction.map(|value| value.clamp(0.0, 1.0)),
        )
    }

    fn rce_higher_low_count(return_points: &[(u64, f64)]) -> u64 {
        let lows: Vec<f64> = return_points
            .windows(3)
            .filter_map(|triple| {
                let prev = triple[0].1;
                let current = triple[1].1;
                let next = triple[2].1;
                (current <= prev && current <= next).then_some(current)
            })
            .collect();
        lows.windows(2).filter(|pair| pair[1] > pair[0]).count() as u64
    }

    fn materialize_rce_pre_entry_path_summary(
        &self,
        series: &DecisionTimeSeriesFeatures,
    ) -> PreEntryPathSummaryV1 {
        let points = Self::rce_decision_path_points(series);
        if points.is_empty() {
            return PreEntryPathSummaryV1::default();
        }
        let (ret_5s, _, _) = Self::rce_path_stats(&points, 5_000);
        let (ret_10s, mfe_10s, mae_10s) = Self::rce_path_stats(&points, 10_000);
        let (ret_20s, mfe_20s, mae_20s) = Self::rce_path_stats(&points, 20_000);
        let (ret_30s, mfe_30s, mae_30s) = Self::rce_path_stats(&points, 30_000);
        let (ret_45s, mfe_45s, mae_45s) = Self::rce_path_stats(&points, 45_000);
        let return_points = Self::rce_return_points(&points);
        let (pullback_depth_bps, reclaim_bps, reclaim_fraction) =
            Self::rce_pullback_reclaim(&return_points);

        PreEntryPathSummaryV1 {
            pre_entry_ret_5s: ret_5s,
            pre_entry_ret_10s: ret_10s,
            pre_entry_ret_20s: ret_20s,
            pre_entry_ret_30s: ret_30s,
            pre_entry_ret_45s: ret_45s,
            pre_entry_mfe_10s: mfe_10s,
            pre_entry_mfe_20s: mfe_20s,
            pre_entry_mfe_30s: mfe_30s,
            pre_entry_mfe_45s: mfe_45s,
            pre_entry_mae_10s: mae_10s,
            pre_entry_mae_20s: mae_20s,
            pre_entry_mae_30s: mae_30s,
            pre_entry_mae_45s: mae_45s,
            pullback_depth_bps,
            reclaim_bps,
            reclaim_fraction,
            higher_low_count: Some(Self::rce_higher_low_count(&return_points)),
            above_0bps_dwell_ms: Some(Self::rce_dwell_ms(&return_points, 0.0)),
            above_300bps_dwell_ms: Some(Self::rce_dwell_ms(&return_points, 300.0)),
            above_600bps_dwell_ms: Some(Self::rce_dwell_ms(&return_points, 600.0)),
        }
    }

    fn rce_window_stats(
        sorted_txs: &[(usize, &PoolTransaction, u64)],
        start_ts_ms: u64,
        end_ts_ms: u64,
    ) -> RceWindowStats {
        if start_ts_ms > end_ts_ms {
            return RceWindowStats::default();
        }
        let mut timestamps = Vec::new();
        let mut timestamp_counts: HashMap<u64, u64> = HashMap::new();
        let mut signer_volume: HashMap<&str, f64> = HashMap::new();
        let mut unique_signers = HashSet::new();
        let mut tx_count = 0u64;
        let mut buy_count = 0u64;
        let mut sell_count = 0u64;
        let mut total_volume_sol = 0.0;

        for (_, tx, ts_ms) in sorted_txs {
            if *ts_ms < start_ts_ms || *ts_ms > end_ts_ms || !tx.success {
                continue;
            }
            tx_count = tx_count.saturating_add(1);
            timestamps.push(*ts_ms);
            *timestamp_counts.entry(*ts_ms).or_insert(0) += 1;
            if tx.is_buy {
                buy_count = buy_count.saturating_add(1);
            } else {
                sell_count = sell_count.saturating_add(1);
            }
            if !tx.signer.is_empty() {
                unique_signers.insert(tx.signer.as_str());
                if tx.volume_sol.is_finite() {
                    *signer_volume.entry(tx.signer.as_str()).or_insert(0.0) += tx.volume_sol.abs();
                }
            }
            if tx.volume_sol.is_finite() {
                total_volume_sol += tx.volume_sol.abs();
            }
        }

        if tx_count == 0 {
            return RceWindowStats::default();
        }
        let same_ms_extra_count: u64 = timestamp_counts
            .values()
            .map(|count| count.saturating_sub(1))
            .sum();
        let burst_ratio = Some(
            compute_velocity_profile(&timestamps, end_ts_ms.saturating_sub(start_ts_ms).max(1))
                .burst_ratio,
        );
        let mut volumes: Vec<f64> = signer_volume
            .values()
            .copied()
            .filter(|value| value.is_finite() && *value > 0.0)
            .collect();
        volumes.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let top3_sum: f64 = volumes.iter().take(3).sum();
        RceWindowStats {
            same_ms_tx_ratio: Some(same_ms_extra_count as f64 / tx_count as f64),
            same_ms_extra_count,
            tx_count,
            burst_ratio,
            unique_ratio: Some(unique_signers.len() as f64 / tx_count as f64),
            top3_signer_volume_ratio: (total_volume_sol > f64::EPSILON)
                .then_some(top3_sum / total_volume_sol),
            buy_sell_ratio: Some(if sell_count > 0 {
                buy_count as f64 / sell_count as f64
            } else {
                buy_count as f64
            }),
            buy_count,
            sell_count,
        }
    }

    fn subtract_optional(current: Option<f64>, previous: Option<f64>) -> Option<f64> {
        match (current, previous) {
            (Some(current), Some(previous)) => Some(current - previous),
            _ => None,
        }
    }

    fn decay_optional(previous: Option<f64>, current: Option<f64>) -> Option<f64> {
        match (previous, current) {
            (Some(previous), Some(current)) => Some(previous - current),
            _ => None,
        }
    }

    fn materialize_rce_session_regime_snapshot(&self) -> SessionRegimeSnapshotV1 {
        let sorted_txs = self.temporal_sorted_transactions();
        let (Some((_, _, first_ts_ms)), Some((_, _, last_ts_ms))) =
            (sorted_txs.first(), sorted_txs.last())
        else {
            return SessionRegimeSnapshotV1 {
                template_reason_code: Some("rce_a0_not_evaluated_logging_only".to_string()),
                ..SessionRegimeSnapshotV1::default()
            };
        };
        let recent_start = last_ts_ms
            .saturating_sub(RCE_RECENT_WINDOW_MS_V1)
            .max(*first_ts_ms);
        let previous_start = recent_start
            .saturating_sub(RCE_RECENT_WINDOW_MS_V1)
            .max(*first_ts_ms);
        let previous = Self::rce_window_stats(&sorted_txs, previous_start, recent_start);
        let recent = Self::rce_window_stats(&sorted_txs, recent_start, *last_ts_ms);

        SessionRegimeSnapshotV1 {
            same_ms_tx_ratio_recent: recent.same_ms_tx_ratio,
            same_ms_tx_ratio_decay: Self::decay_optional(
                previous.same_ms_tx_ratio,
                recent.same_ms_tx_ratio,
            ),
            burst_ratio_recent: recent.burst_ratio,
            burst_ratio_decay: Self::decay_optional(previous.burst_ratio, recent.burst_ratio),
            unique_ratio_recent: recent.unique_ratio,
            unique_ratio_drift: Self::subtract_optional(recent.unique_ratio, previous.unique_ratio),
            top3_signer_volume_ratio_recent: recent.top3_signer_volume_ratio,
            top3_signer_volume_ratio_drift: Self::subtract_optional(
                recent.top3_signer_volume_ratio,
                previous.top3_signer_volume_ratio,
            ),
            buy_sell_ratio_recent: recent.buy_sell_ratio,
            session_pool_rate_5m: None,
            session_pool_rate_10m: None,
            session_followthrough_rate_10m_optional: None,
            template_reason_code: Some("rce_a0_not_evaluated_logging_only".to_string()),
            veto_reason_code: None,
        }
    }

    #[must_use]
    pub fn metric_contract_recent_exact_timing_snapshot(&self) -> TxTimingProducerSnapshotV1 {
        let sorted_txs = self.temporal_sorted_transactions();
        let (Some((_, _, first_ts_ms)), Some((_, _, last_ts_ms))) =
            (sorted_txs.first(), sorted_txs.last())
        else {
            return TxTimingProducerSnapshotV1 {
                numerator: 0,
                denominator: 0,
                ratio: None,
                canonical_dedupe_applied: true,
                dust_filter_sol: None,
                window_ms: Some(RCE_RECENT_WINDOW_MS_V1),
                fallback_timestamp_count: 0,
                fallback_ordering_count: 0,
                source_complete: self.diagnostics.total_tx_seen == 0,
                source_state_capacity: Some(
                    u64::try_from(self.decision_time_series_tx_capacity).unwrap_or(u64::MAX),
                ),
            };
        };
        let recent_start = last_ts_ms
            .saturating_sub(RCE_RECENT_WINDOW_MS_V1)
            .max(*first_ts_ms);
        let recent = Self::rce_window_stats(&sorted_txs, recent_start, *last_ts_ms);
        let recent_successful = self.tx_buffer.iter().filter(|tx| {
            let event_ts_ms = Self::temporal_tx_event_ts_ms(tx.as_ref());
            tx.success && event_ts_ms >= recent_start && event_ts_ms <= *last_ts_ms
        });
        let fallback_timestamp_count = u64::try_from(
            recent_successful
                .clone()
                .filter(|tx| tx.compat_event_ts_ms().is_none())
                .count(),
        )
        .unwrap_or(u64::MAX);
        let fallback_ordering_count = u64::try_from(
            recent_successful
                .filter(|tx| !crate::tx_intelligence::tx_has_stable_timing_order_identity(tx))
                .count(),
        )
        .unwrap_or(u64::MAX);
        let source_complete = u64::try_from(self.tx_buffer.len())
            .is_ok_and(|retained| retained == self.diagnostics.total_tx_seen);
        TxTimingProducerSnapshotV1 {
            numerator: recent.same_ms_extra_count,
            denominator: recent.tx_count,
            ratio: recent.same_ms_tx_ratio,
            canonical_dedupe_applied: true,
            dust_filter_sol: None,
            window_ms: Some(RCE_RECENT_WINDOW_MS_V1),
            fallback_timestamp_count,
            fallback_ordering_count,
            source_complete,
            source_state_capacity: Some(
                u64::try_from(self.decision_time_series_tx_capacity).unwrap_or(u64::MAX),
            ),
        }
    }

    pub fn metric_contract_recent_buy_sell_snapshot(
        &self,
    ) -> Result<
        crate::metric_contracts::RecentBuySellProducerSnapshotV1,
        MetricContractMaterializationErrorV1,
    > {
        let sorted_txs = self.temporal_sorted_transactions();
        let source_complete = u64::try_from(self.tx_buffer.len())
            .is_ok_and(|retained| retained == self.diagnostics.total_tx_seen);
        let (Some((_, _, first_ts_ms)), Some((_, _, last_ts_ms))) =
            (sorted_txs.first(), sorted_txs.last())
        else {
            return Ok(crate::metric_contracts::RecentBuySellProducerSnapshotV1 {
                window_ms: RCE_RECENT_WINDOW_MS_V1,
                buy_count: 0,
                sell_count: 0,
                transaction_count: 0,
                failed_transaction_count: 0,
                source_complete,
            });
        };
        let recent_start = last_ts_ms
            .saturating_sub(RCE_RECENT_WINDOW_MS_V1)
            .max(*first_ts_ms);
        let recent = Self::rce_window_stats(&sorted_txs, recent_start, *last_ts_ms);
        let failed_transaction_count = u64::try_from(
            sorted_txs
                .iter()
                .filter(|(_, tx, ts_ms)| {
                    !tx.success && *ts_ms >= recent_start && *ts_ms <= *last_ts_ms
                })
                .count(),
        )
        .map_err(|_| {
            crate::metric_contracts::Pr2bProducerErrorV1::CountOverflow(
                "recent failed transactions",
            )
        })?;
        Ok(crate::metric_contracts::RecentBuySellProducerSnapshotV1 {
            window_ms: RCE_RECENT_WINDOW_MS_V1,
            buy_count: recent.buy_count,
            sell_count: recent.sell_count,
            transaction_count: recent.tx_count,
            failed_transaction_count,
            source_complete,
        })
    }

    fn temporal_anchor_raw_values<'a>(
        &self,
        sorted_txs: &[(usize, &'a PoolTransaction, u64)],
        cutoff_ts_ms: u64,
        anchor_ms: u64,
        first_price: Option<f64>,
    ) -> TemporalAnchorRawValues {
        let mut tx_count = 0u64;
        let mut buy_count = 0u64;
        let mut unique_signers = HashSet::new();
        let mut net_quote_sol = 0.0;
        let mut total_volume_sol = 0.0;
        let mut timestamps = Vec::new();
        let mut market_cap_sol = None;
        let mut price_value = None;
        let mut jito_known_count = 0u64;
        let mut jito_tip_count = 0u64;
        let mut anchor_txs = Vec::new();

        for (_, tx, ts_ms) in sorted_txs {
            if *ts_ms > cutoff_ts_ms {
                break;
            }
            anchor_txs.push(*tx);
            tx_count = tx_count.saturating_add(1);
            timestamps.push(*ts_ms);
            if tx.is_buy {
                buy_count = buy_count.saturating_add(1);
            }
            if !tx.signer.is_empty() {
                unique_signers.insert(tx.signer.as_str());
            }
            if tx.volume_sol.is_finite() {
                let signed = if tx.is_buy {
                    tx.volume_sol
                } else {
                    -tx.volume_sol
                };
                net_quote_sol += signed;
                total_volume_sol += tx.volume_sol.abs();
            }
            if let Some(value) = Self::temporal_market_cap_value(tx) {
                market_cap_sol = Some(value);
            }
            if let Some(value) = Self::temporal_price_value(tx) {
                price_value = Some(value);
            }
            if let Some(jito_tip_detected) = tx.jito_tip_detected {
                jito_known_count = jito_known_count.saturating_add(1);
                if jito_tip_detected {
                    jito_tip_count = jito_tip_count.saturating_add(1);
                }
            }
        }

        if tx_count == 0 {
            return TemporalAnchorRawValues::default();
        }

        let burst_ratio = Some(compute_velocity_profile(&timestamps, anchor_ms.max(1)).burst_ratio);
        let price_pct = match (first_price, price_value) {
            (Some(first), Some(current)) if first.is_finite() && first > 0.0 => {
                Some(((current - first) / first) * 100.0)
            }
            _ => None,
        };

        TemporalAnchorRawValues {
            tx_count: Some(tx_count),
            buy_count: Some(buy_count),
            unique_signers: Some(unique_signers.len() as u64),
            net_quote_sol: Some(net_quote_sol),
            total_volume_sol: Some(total_volume_sol),
            market_cap_sol,
            price_pct,
            burst_ratio,
            jito_tip_intensity: (jito_known_count > 0)
                .then_some(jito_tip_count as f64 / jito_known_count as f64),
            signer_cross_pool_velocity: self.temporal_anchor_cpv(&anchor_txs, cutoff_ts_ms),
            flipper_presence_ratio: Self::temporal_flipper_presence_ratio(&anchor_txs),
        }
    }

    fn temporal_anchor_cpv(
        &self,
        anchor_txs: &[&PoolTransaction],
        anchor_ts_ms: u64,
    ) -> Option<f64> {
        let pool_id = self.pool_amm_id.to_string();
        let cpv = self.cross_pool_velocity_index.compute_for_transactions(
            pool_id.as_str(),
            anchor_txs.iter().copied(),
            Some(anchor_ts_ms),
            &self.cross_pool_velocity_config,
        );
        (cpv.status == MetricEvidenceQuality::Clean)
            .then_some(cpv.signer_cross_pool_velocity)
            .flatten()
    }

    fn temporal_flipper_presence_ratio(anchor_txs: &[&PoolTransaction]) -> Option<f64> {
        let mut buyers = HashSet::new();
        let mut sellers = HashSet::new();

        for tx in anchor_txs {
            if !tx.success {
                continue;
            }
            if tx.owner_token_deltas.is_empty() {
                if tx.signer.is_empty() {
                    continue;
                }
                if tx.is_buy {
                    buyers.insert(tx.signer.clone());
                } else {
                    sellers.insert(tx.signer.clone());
                }
                continue;
            }

            for delta in &tx.owner_token_deltas {
                if delta.owner.is_empty() {
                    continue;
                }
                if delta.delta_raw > 0 {
                    buyers.insert(delta.owner.clone());
                } else if delta.delta_raw < 0 {
                    sellers.insert(delta.owner.clone());
                }
            }
        }

        if buyers.is_empty() {
            return None;
        }

        let flipper_count = buyers.intersection(&sellers).count();
        Some(flipper_count as f64 / buyers.len() as f64)
    }

    fn build_temporal_anchor(
        &self,
        anchor_ms: u64,
        reached_by: TemporalAnchorReachedBy,
        observation_elapsed_ms: u64,
        raw_values: TemporalAnchorRawValues,
        previous_event_anchor: Option<&TemporalAnchorSnapshot>,
        previous_state_anchor: Option<&TemporalAnchorSnapshot>,
        previous_ratio_anchor: Option<&TemporalAnchorSnapshot>,
    ) -> TemporalAnchorSnapshot {
        let reached = reached_by != TemporalAnchorReachedBy::NotReached;
        let mut anchor = TemporalAnchorSnapshot {
            anchor_ms,
            reached,
            reached_by,
            anchor_observation_elapsed_ms: reached.then_some(observation_elapsed_ms),
            ..TemporalAnchorSnapshot::default()
        };

        if !reached {
            let unavailable = Self::temporal_context(
                MetricEvidenceQuality::Unavailable,
                TemporalMetricSource::Unavailable,
                None,
                None,
                Some("anchor_not_reached"),
            );
            anchor.status = MetricEvidenceQuality::Unavailable;
            anchor.event_counters_evidence = unavailable.clone();
            anchor.state_metrics_evidence = unavailable.clone();
            anchor.ratio_metrics_evidence = unavailable;
            return anchor;
        }

        if reached_by == TemporalAnchorReachedBy::Event {
            anchor.tx_count = raw_values.tx_count;
            anchor.buy_count = raw_values.buy_count;
            anchor.unique_signers = raw_values.unique_signers;
            anchor.net_quote_sol = raw_values.net_quote_sol;
            anchor.total_volume_sol = raw_values.total_volume_sol;
            anchor.event_counters_evidence = Self::temporal_context(
                MetricEvidenceQuality::Clean,
                TemporalMetricSource::Observed,
                None,
                None,
                None,
            );
        } else {
            self.apply_temporal_event_counter_carry(anchor_ms, previous_event_anchor, &mut anchor);
        }

        if reached_by == TemporalAnchorReachedBy::Event {
            anchor.market_cap_sol = raw_values.market_cap_sol;
            anchor.price_pct = raw_values.price_pct;
            anchor.state_metrics_evidence =
                if anchor.market_cap_sol.is_some() || anchor.price_pct.is_some() {
                    Self::temporal_context(
                        MetricEvidenceQuality::Clean,
                        TemporalMetricSource::Observed,
                        None,
                        None,
                        None,
                    )
                } else {
                    Self::temporal_context(
                        MetricEvidenceQuality::Unavailable,
                        TemporalMetricSource::Unavailable,
                        None,
                        None,
                        Some("state_value_unavailable"),
                    )
                };
        } else {
            self.apply_temporal_state_carry(anchor_ms, previous_state_anchor, &mut anchor);
        }

        if reached_by == TemporalAnchorReachedBy::Event {
            anchor.burst_ratio = raw_values.burst_ratio;
            anchor.jito_tip_intensity = raw_values.jito_tip_intensity;
            anchor.signer_cross_pool_velocity = raw_values.signer_cross_pool_velocity;
            anchor.flipper_presence_ratio = raw_values.flipper_presence_ratio;
            anchor.ratio_metrics_evidence = if anchor.burst_ratio.is_some()
                || anchor.jito_tip_intensity.is_some()
                || anchor.signer_cross_pool_velocity.is_some()
                || anchor.flipper_presence_ratio.is_some()
            {
                Self::temporal_context(
                    MetricEvidenceQuality::Clean,
                    TemporalMetricSource::Observed,
                    None,
                    None,
                    None,
                )
            } else {
                Self::temporal_context(
                    MetricEvidenceQuality::Unavailable,
                    TemporalMetricSource::Unavailable,
                    None,
                    None,
                    Some("ratio_value_unavailable"),
                )
            };
        } else {
            self.apply_temporal_ratio_carry(anchor_ms, previous_ratio_anchor, &mut anchor);
        }

        anchor.status = Self::temporal_anchor_status(&anchor);
        anchor
    }

    fn apply_temporal_event_counter_carry(
        &self,
        anchor_ms: u64,
        previous_anchor: Option<&TemporalAnchorSnapshot>,
        anchor: &mut TemporalAnchorSnapshot,
    ) {
        if !self.temporal_carry_forward_config.enabled
            || !self.temporal_carry_forward_config.event_counters_enabled
        {
            anchor.event_counters_evidence = Self::temporal_context(
                MetricEvidenceQuality::NotAllowed,
                TemporalMetricSource::NotAllowed,
                None,
                None,
                Some("event_counter_carry_forward_not_allowed"),
            );
            return;
        }

        let Some(previous) = previous_anchor else {
            anchor.event_counters_evidence = Self::temporal_context(
                MetricEvidenceQuality::Unavailable,
                TemporalMetricSource::Unavailable,
                None,
                None,
                Some("event_counter_prior_anchor_unavailable"),
            );
            return;
        };
        let staleness_ms = anchor_ms.saturating_sub(previous.anchor_ms);
        if staleness_ms > self.temporal_carry_forward_config.max_staleness_ms {
            anchor.event_counters_evidence = Self::temporal_context(
                MetricEvidenceQuality::Stale,
                TemporalMetricSource::Stale,
                Some(previous.anchor_ms),
                Some(staleness_ms),
                Some("stale"),
            );
            return;
        }

        anchor.tx_count = previous.tx_count;
        anchor.buy_count = previous.buy_count;
        anchor.unique_signers = previous.unique_signers;
        anchor.net_quote_sol = previous.net_quote_sol;
        anchor.total_volume_sol = previous.total_volume_sol;
        anchor.event_counters_evidence = Self::temporal_context(
            MetricEvidenceQuality::CarriedForward,
            TemporalMetricSource::CarriedForwardNoEvent,
            Some(previous.anchor_ms),
            Some(staleness_ms),
            None,
        );
    }

    fn apply_temporal_state_carry(
        &self,
        anchor_ms: u64,
        previous_anchor: Option<&TemporalAnchorSnapshot>,
        anchor: &mut TemporalAnchorSnapshot,
    ) {
        if !self.temporal_carry_forward_config.enabled
            || !self.temporal_carry_forward_config.state_metrics_enabled
        {
            anchor.state_metrics_evidence = Self::temporal_context(
                MetricEvidenceQuality::NotAllowed,
                TemporalMetricSource::NotAllowed,
                None,
                None,
                Some("state_carry_forward_not_allowed"),
            );
            return;
        }
        let Some(previous) = previous_anchor else {
            anchor.state_metrics_evidence = Self::temporal_context(
                MetricEvidenceQuality::Unavailable,
                TemporalMetricSource::Unavailable,
                None,
                None,
                Some("state_prior_anchor_unavailable"),
            );
            return;
        };
        let staleness_ms = anchor_ms.saturating_sub(previous.anchor_ms);
        if staleness_ms > self.temporal_carry_forward_config.max_staleness_ms {
            anchor.state_metrics_evidence = Self::temporal_context(
                MetricEvidenceQuality::Stale,
                TemporalMetricSource::Stale,
                Some(previous.anchor_ms),
                Some(staleness_ms),
                Some("stale"),
            );
            return;
        }
        anchor.market_cap_sol = previous.market_cap_sol;
        anchor.price_pct = previous.price_pct;
        anchor.state_metrics_evidence = Self::temporal_context(
            MetricEvidenceQuality::CarriedForward,
            TemporalMetricSource::CarriedForwardNoEvent,
            Some(previous.anchor_ms),
            Some(staleness_ms),
            None,
        );
    }

    fn apply_temporal_ratio_carry(
        &self,
        anchor_ms: u64,
        previous_anchor: Option<&TemporalAnchorSnapshot>,
        anchor: &mut TemporalAnchorSnapshot,
    ) {
        if !self.temporal_carry_forward_config.enabled
            || !self.temporal_carry_forward_config.ratio_metrics_enabled
        {
            anchor.ratio_metrics_evidence = Self::temporal_context(
                MetricEvidenceQuality::NotAllowed,
                TemporalMetricSource::NotAllowed,
                None,
                None,
                Some("ratio_carry_forward_not_allowed"),
            );
            return;
        }
        let Some(previous) = previous_anchor else {
            anchor.ratio_metrics_evidence = Self::temporal_context(
                MetricEvidenceQuality::Unavailable,
                TemporalMetricSource::Unavailable,
                None,
                None,
                Some("ratio_prior_anchor_unavailable"),
            );
            return;
        };
        let staleness_ms = anchor_ms.saturating_sub(previous.anchor_ms);
        if staleness_ms > self.temporal_carry_forward_config.max_staleness_ms {
            anchor.ratio_metrics_evidence = Self::temporal_context(
                MetricEvidenceQuality::Stale,
                TemporalMetricSource::Stale,
                Some(previous.anchor_ms),
                Some(staleness_ms),
                Some("stale"),
            );
            return;
        }
        anchor.burst_ratio = previous.burst_ratio;
        anchor.jito_tip_intensity = previous.jito_tip_intensity;
        anchor.signer_cross_pool_velocity = previous.signer_cross_pool_velocity;
        anchor.flipper_presence_ratio = previous.flipper_presence_ratio;
        anchor.ratio_metrics_evidence = Self::temporal_context(
            MetricEvidenceQuality::CarriedForward,
            TemporalMetricSource::CarriedForwardNoEvent,
            Some(previous.anchor_ms),
            Some(staleness_ms),
            None,
        );
    }

    fn temporal_anchor_status(anchor: &TemporalAnchorSnapshot) -> MetricEvidenceQuality {
        let qualities = [
            anchor.event_counters_evidence.quality,
            anchor.state_metrics_evidence.quality,
            anchor.ratio_metrics_evidence.quality,
        ];
        if qualities.contains(&MetricEvidenceQuality::CarriedForward) {
            MetricEvidenceQuality::CarriedForward
        } else if qualities.contains(&MetricEvidenceQuality::Clean) {
            MetricEvidenceQuality::Clean
        } else if qualities.contains(&MetricEvidenceQuality::Stale) {
            MetricEvidenceQuality::Stale
        } else if qualities.contains(&MetricEvidenceQuality::NotAllowed) {
            MetricEvidenceQuality::NotAllowed
        } else {
            MetricEvidenceQuality::Unavailable
        }
    }

    fn temporal_context(
        quality: MetricEvidenceQuality,
        source: TemporalMetricSource,
        carried_from_anchor_ms: Option<u64>,
        staleness_ms: Option<u64>,
        reason: Option<&str>,
    ) -> TemporalMetricEvidenceContext {
        TemporalMetricEvidenceContext {
            quality,
            source,
            carried_from_anchor_ms,
            staleness_ms,
            reason: reason.map(str::to_string),
        }
    }

    fn populate_temporal_delta_pairs(&self, features: &mut TemporalDeltaFeatures) {
        let a1 = features.anchor_1s.clone();
        let a2 = features.anchor_2s.clone();
        let a3 = features.anchor_3s.clone();

        Self::set_temporal_i64_delta(
            features,
            "delta_buy_count_1s_to_2s",
            a1.buy_count,
            a2.buy_count,
            &a1.event_counters_evidence,
            &a2.event_counters_evidence,
            1_000,
            |features, value| features.delta_buy_count_1s_to_2s = value,
        );
        Self::set_temporal_i64_delta(
            features,
            "delta_buy_count_1s_to_3s",
            a1.buy_count,
            a3.buy_count,
            &a1.event_counters_evidence,
            &a3.event_counters_evidence,
            2_000,
            |features, value| features.delta_buy_count_1s_to_3s = value,
        );
        Self::set_temporal_i64_delta(
            features,
            "delta_buy_count_2s_to_3s",
            a2.buy_count,
            a3.buy_count,
            &a2.event_counters_evidence,
            &a3.event_counters_evidence,
            1_000,
            |features, value| features.delta_buy_count_2s_to_3s = value,
        );
        Self::set_temporal_i64_delta(
            features,
            "delta_unique_signers_1s_to_2s",
            a1.unique_signers,
            a2.unique_signers,
            &a1.event_counters_evidence,
            &a2.event_counters_evidence,
            1_000,
            |features, value| features.delta_unique_signers_1s_to_2s = value,
        );
        Self::set_temporal_i64_delta(
            features,
            "delta_unique_signers_1s_to_3s",
            a1.unique_signers,
            a3.unique_signers,
            &a1.event_counters_evidence,
            &a3.event_counters_evidence,
            2_000,
            |features, value| features.delta_unique_signers_1s_to_3s = value,
        );
        Self::set_temporal_i64_delta(
            features,
            "delta_unique_signers_2s_to_3s",
            a2.unique_signers,
            a3.unique_signers,
            &a2.event_counters_evidence,
            &a3.event_counters_evidence,
            1_000,
            |features, value| features.delta_unique_signers_2s_to_3s = value,
        );
        Self::set_temporal_i64_delta(
            features,
            "delta_tx_count_1s_to_2s",
            a1.tx_count,
            a2.tx_count,
            &a1.event_counters_evidence,
            &a2.event_counters_evidence,
            1_000,
            |features, value| features.delta_tx_count_1s_to_2s = value,
        );
        Self::set_temporal_i64_delta(
            features,
            "delta_tx_count_1s_to_3s",
            a1.tx_count,
            a3.tx_count,
            &a1.event_counters_evidence,
            &a3.event_counters_evidence,
            2_000,
            |features, value| features.delta_tx_count_1s_to_3s = value,
        );
        Self::set_temporal_i64_delta(
            features,
            "delta_tx_count_2s_to_3s",
            a2.tx_count,
            a3.tx_count,
            &a2.event_counters_evidence,
            &a3.event_counters_evidence,
            1_000,
            |features, value| features.delta_tx_count_2s_to_3s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_net_quote_sol_1s_to_2s",
            a1.net_quote_sol,
            a2.net_quote_sol,
            &a1.event_counters_evidence,
            &a2.event_counters_evidence,
            1_000,
            |features, value| features.delta_net_quote_sol_1s_to_2s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_net_quote_sol_1s_to_3s",
            a1.net_quote_sol,
            a3.net_quote_sol,
            &a1.event_counters_evidence,
            &a3.event_counters_evidence,
            2_000,
            |features, value| features.delta_net_quote_sol_1s_to_3s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_net_quote_sol_2s_to_3s",
            a2.net_quote_sol,
            a3.net_quote_sol,
            &a2.event_counters_evidence,
            &a3.event_counters_evidence,
            1_000,
            |features, value| features.delta_net_quote_sol_2s_to_3s = value,
        );

        Self::set_temporal_f64_delta(
            features,
            "delta_mcap_1s_to_2s",
            a1.market_cap_sol,
            a2.market_cap_sol,
            &a1.state_metrics_evidence,
            &a2.state_metrics_evidence,
            1_000,
            |features, value| features.delta_mcap_1s_to_2s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_mcap_1s_to_3s",
            a1.market_cap_sol,
            a3.market_cap_sol,
            &a1.state_metrics_evidence,
            &a3.state_metrics_evidence,
            2_000,
            |features, value| features.delta_mcap_1s_to_3s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_mcap_2s_to_3s",
            a2.market_cap_sol,
            a3.market_cap_sol,
            &a2.state_metrics_evidence,
            &a3.state_metrics_evidence,
            1_000,
            |features, value| features.delta_mcap_2s_to_3s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_price_pct_1s_to_2s",
            a1.price_pct,
            a2.price_pct,
            &a1.state_metrics_evidence,
            &a2.state_metrics_evidence,
            1_000,
            |features, value| features.delta_price_pct_1s_to_2s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_price_pct_1s_to_3s",
            a1.price_pct,
            a3.price_pct,
            &a1.state_metrics_evidence,
            &a3.state_metrics_evidence,
            2_000,
            |features, value| features.delta_price_pct_1s_to_3s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_price_pct_2s_to_3s",
            a2.price_pct,
            a3.price_pct,
            &a2.state_metrics_evidence,
            &a3.state_metrics_evidence,
            1_000,
            |features, value| features.delta_price_pct_2s_to_3s = value,
        );

        Self::set_temporal_f64_delta(
            features,
            "delta_burstratio_1s_to_2s",
            a1.burst_ratio,
            a2.burst_ratio,
            &a1.ratio_metrics_evidence,
            &a2.ratio_metrics_evidence,
            1_000,
            |features, value| features.delta_burstratio_1s_to_2s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_burstratio_1s_to_3s",
            a1.burst_ratio,
            a3.burst_ratio,
            &a1.ratio_metrics_evidence,
            &a3.ratio_metrics_evidence,
            2_000,
            |features, value| features.delta_burstratio_1s_to_3s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_burstratio_2s_to_3s",
            a2.burst_ratio,
            a3.burst_ratio,
            &a2.ratio_metrics_evidence,
            &a3.ratio_metrics_evidence,
            1_000,
            |features, value| features.delta_burstratio_2s_to_3s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_jito_tip_intensity_1s_to_2s",
            a1.jito_tip_intensity,
            a2.jito_tip_intensity,
            &a1.ratio_metrics_evidence,
            &a2.ratio_metrics_evidence,
            1_000,
            |features, value| features.delta_jito_tip_intensity_1s_to_2s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_jito_tip_intensity_1s_to_3s",
            a1.jito_tip_intensity,
            a3.jito_tip_intensity,
            &a1.ratio_metrics_evidence,
            &a3.ratio_metrics_evidence,
            2_000,
            |features, value| features.delta_jito_tip_intensity_1s_to_3s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_signer_cross_pool_velocity_1s_to_2s",
            a1.signer_cross_pool_velocity,
            a2.signer_cross_pool_velocity,
            &a1.ratio_metrics_evidence,
            &a2.ratio_metrics_evidence,
            1_000,
            |features, value| features.delta_signer_cross_pool_velocity_1s_to_2s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_signer_cross_pool_velocity_1s_to_3s",
            a1.signer_cross_pool_velocity,
            a3.signer_cross_pool_velocity,
            &a1.ratio_metrics_evidence,
            &a3.ratio_metrics_evidence,
            2_000,
            |features, value| features.delta_signer_cross_pool_velocity_1s_to_3s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_flipper_presence_ratio_1s_to_2s",
            a1.flipper_presence_ratio,
            a2.flipper_presence_ratio,
            &a1.ratio_metrics_evidence,
            &a2.ratio_metrics_evidence,
            1_000,
            |features, value| features.delta_flipper_presence_ratio_1s_to_2s = value,
        );
        Self::set_temporal_f64_delta(
            features,
            "delta_flipper_presence_ratio_1s_to_3s",
            a1.flipper_presence_ratio,
            a3.flipper_presence_ratio,
            &a1.ratio_metrics_evidence,
            &a3.ratio_metrics_evidence,
            2_000,
            |features, value| features.delta_flipper_presence_ratio_1s_to_3s = value,
        );

        Self::set_temporal_rate_from_f64_delta(
            features,
            "rate_mcap_sol_per_s_1s_to_2s",
            "delta_mcap_1s_to_2s",
            features.delta_mcap_1s_to_2s,
            1_000,
            |features, value| features.rate_mcap_sol_per_s_1s_to_2s = value,
        );
        Self::set_temporal_rate_from_f64_delta(
            features,
            "rate_mcap_sol_per_s_1s_to_3s",
            "delta_mcap_1s_to_3s",
            features.delta_mcap_1s_to_3s,
            2_000,
            |features, value| features.rate_mcap_sol_per_s_1s_to_3s = value,
        );
        Self::set_temporal_rate_from_f64_delta(
            features,
            "rate_mcap_sol_per_s_2s_to_3s",
            "delta_mcap_2s_to_3s",
            features.delta_mcap_2s_to_3s,
            1_000,
            |features, value| features.rate_mcap_sol_per_s_2s_to_3s = value,
        );
        Self::set_temporal_rate_from_i64_delta(
            features,
            "rate_buy_count_per_s_1s_to_2s",
            "delta_buy_count_1s_to_2s",
            features.delta_buy_count_1s_to_2s,
            1_000,
            |features, value| features.rate_buy_count_per_s_1s_to_2s = value,
        );
        Self::set_temporal_rate_from_i64_delta(
            features,
            "rate_buy_count_per_s_1s_to_3s",
            "delta_buy_count_1s_to_3s",
            features.delta_buy_count_1s_to_3s,
            2_000,
            |features, value| features.rate_buy_count_per_s_1s_to_3s = value,
        );
        Self::set_temporal_rate_from_i64_delta(
            features,
            "rate_unique_signers_per_s_1s_to_2s",
            "delta_unique_signers_1s_to_2s",
            features.delta_unique_signers_1s_to_2s,
            1_000,
            |features, value| features.rate_unique_signers_per_s_1s_to_2s = value,
        );
        Self::set_temporal_rate_from_i64_delta(
            features,
            "rate_unique_signers_per_s_1s_to_3s",
            "delta_unique_signers_1s_to_3s",
            features.delta_unique_signers_1s_to_3s,
            2_000,
            |features, value| features.rate_unique_signers_per_s_1s_to_3s = value,
        );
        Self::set_temporal_rate_from_f64_delta(
            features,
            "rate_net_quote_sol_per_s_1s_to_2s",
            "delta_net_quote_sol_1s_to_2s",
            features.delta_net_quote_sol_1s_to_2s,
            1_000,
            |features, value| features.rate_net_quote_sol_per_s_1s_to_2s = value,
        );
        Self::set_temporal_rate_from_f64_delta(
            features,
            "rate_net_quote_sol_per_s_1s_to_3s",
            "delta_net_quote_sol_1s_to_3s",
            features.delta_net_quote_sol_1s_to_3s,
            2_000,
            |features, value| features.rate_net_quote_sol_per_s_1s_to_3s = value,
        );
    }

    fn set_temporal_i64_delta(
        features: &mut TemporalDeltaFeatures,
        field: &'static str,
        from: Option<u64>,
        to: Option<u64>,
        from_evidence: &TemporalMetricEvidenceContext,
        to_evidence: &TemporalMetricEvidenceContext,
        _span_ms: u64,
        assign: impl FnOnce(&mut TemporalDeltaFeatures, Option<i64>),
    ) {
        let (value, evidence) = match (from, to) {
            (Some(from), Some(to)) => (
                Some(to as i64 - from as i64),
                Self::temporal_delta_evidence(from_evidence, to_evidence),
            ),
            _ => (
                None,
                Self::temporal_missing_delta_evidence(from_evidence, to_evidence),
            ),
        };
        assign(features, value);
        features.delta_evidence.insert(field.to_string(), evidence);
    }

    fn set_temporal_f64_delta(
        features: &mut TemporalDeltaFeatures,
        field: &'static str,
        from: Option<f64>,
        to: Option<f64>,
        from_evidence: &TemporalMetricEvidenceContext,
        to_evidence: &TemporalMetricEvidenceContext,
        _span_ms: u64,
        assign: impl FnOnce(&mut TemporalDeltaFeatures, Option<f64>),
    ) {
        let (value, evidence) = match (from, to) {
            (Some(from), Some(to)) if from.is_finite() && to.is_finite() => (
                Some(to - from),
                Self::temporal_delta_evidence(from_evidence, to_evidence),
            ),
            _ => (
                None,
                Self::temporal_missing_delta_evidence(from_evidence, to_evidence),
            ),
        };
        assign(features, value);
        features.delta_evidence.insert(field.to_string(), evidence);
    }

    fn set_temporal_rate_from_f64_delta(
        features: &mut TemporalDeltaFeatures,
        rate_field: &'static str,
        delta_field: &'static str,
        delta: Option<f64>,
        span_ms: u64,
        assign: impl FnOnce(&mut TemporalDeltaFeatures, Option<f64>),
    ) {
        let rate = delta.map(|value| value / (span_ms as f64 / 1_000.0));
        assign(features, rate);
        if let Some(evidence) = features.delta_evidence.get(delta_field).cloned() {
            features
                .delta_evidence
                .insert(rate_field.to_string(), evidence);
        }
    }

    fn set_temporal_rate_from_i64_delta(
        features: &mut TemporalDeltaFeatures,
        rate_field: &'static str,
        delta_field: &'static str,
        delta: Option<i64>,
        span_ms: u64,
        assign: impl FnOnce(&mut TemporalDeltaFeatures, Option<f64>),
    ) {
        let rate = delta.map(|value| value as f64 / (span_ms as f64 / 1_000.0));
        assign(features, rate);
        if let Some(evidence) = features.delta_evidence.get(delta_field).cloned() {
            features
                .delta_evidence
                .insert(rate_field.to_string(), evidence);
        }
    }

    fn temporal_delta_evidence(
        from_evidence: &TemporalMetricEvidenceContext,
        to_evidence: &TemporalMetricEvidenceContext,
    ) -> TemporalMetricEvidenceContext {
        if to_evidence.source == TemporalMetricSource::CarriedForwardNoEvent {
            return Self::temporal_context(
                MetricEvidenceQuality::CarriedForward,
                TemporalMetricSource::CarriedForwardNoEvent,
                to_evidence.carried_from_anchor_ms,
                to_evidence.staleness_ms,
                None,
            );
        }
        if from_evidence.quality == MetricEvidenceQuality::CarriedForward
            || to_evidence.quality == MetricEvidenceQuality::CarriedForward
        {
            return Self::temporal_context(
                MetricEvidenceQuality::CarriedForward,
                TemporalMetricSource::PartialCarriedForward,
                to_evidence
                    .carried_from_anchor_ms
                    .or(from_evidence.carried_from_anchor_ms),
                to_evidence.staleness_ms.or(from_evidence.staleness_ms),
                None,
            );
        }
        Self::temporal_context(
            MetricEvidenceQuality::Clean,
            TemporalMetricSource::Observed,
            None,
            None,
            None,
        )
    }

    fn temporal_missing_delta_evidence(
        from_evidence: &TemporalMetricEvidenceContext,
        to_evidence: &TemporalMetricEvidenceContext,
    ) -> TemporalMetricEvidenceContext {
        if matches!(to_evidence.quality, MetricEvidenceQuality::Stale) {
            return to_evidence.clone();
        }
        if matches!(to_evidence.quality, MetricEvidenceQuality::NotAllowed) {
            return to_evidence.clone();
        }
        if matches!(from_evidence.quality, MetricEvidenceQuality::Stale) {
            return from_evidence.clone();
        }
        if matches!(from_evidence.quality, MetricEvidenceQuality::NotAllowed) {
            return from_evidence.clone();
        }
        Self::temporal_context(
            MetricEvidenceQuality::Unavailable,
            TemporalMetricSource::Unavailable,
            None,
            None,
            Some("anchor_value_unavailable"),
        )
    }

    fn temporal_delta_status(features: &TemporalDeltaFeatures) -> EvidenceStatus {
        if features.delta_evidence.values().any(|evidence| {
            evidence.quality == MetricEvidenceQuality::CarriedForward
                || evidence.source == TemporalMetricSource::CarriedForwardNoEvent
                || evidence.source == TemporalMetricSource::PartialCarriedForward
        }) {
            EvidenceStatus::Degraded
        } else if features
            .delta_evidence
            .values()
            .any(|evidence| evidence.quality == MetricEvidenceQuality::Clean)
        {
            EvidenceStatus::Clean
        } else if features.anchor_1s.reached
            || features.anchor_2s.reached
            || features.anchor_3s.reached
        {
            EvidenceStatus::InsufficientSample
        } else {
            EvidenceStatus::Unavailable
        }
    }

    pub fn try_materialize_features(
        &self,
    ) -> Result<MaterializedFeatureSet, MetricContractMaterializationErrorV1> {
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
        materialized.tx_segment_sequence = self
            .gatekeeper_buffer
            .current_segment_sequence_from_config();
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

        let fingerprint_metrics = self.fingerprint_metrics();
        if let Some(fingerprint) = fingerprint_metrics.as_ref() {
            materialized.alpha_fingerprint = AlphaFingerprintFeatures {
                avg_inner_ix_count_50tx: fingerprint.avg_inner_ix_count_50tx,
                avg_cpi_depth_50tx: fingerprint.avg_cpi_depth_50tx,
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
        let sybil_computation = compute_sybil_resistance_with_ftdi(
            self.tx_buffer.iter().map(AsRef::as_ref),
            sybil_dev_wallet.as_deref(),
        );
        let sybil = &sybil_computation.features;
        materialized.sybil_resistance.fee_topology_diversity_index =
            sybil.fee_topology_diversity_index;
        materialized
            .sybil_resistance
            .dev_buyer_infrastructure_affinity = sybil.dev_buyer_infrastructure_affinity;
        materialized.sybil_resistance.spend_fraction_divergence = sybil.spend_fraction_divergence;
        materialized.sybil_resistance.demand_elasticity_score = sybil.demand_elasticity_score;
        materialized.sybil_resistance.degraded_reasons = sybil.degraded_reasons.clone();
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
        let cpv_can_emit_value = match cpv.status {
            MetricEvidenceQuality::Clean => true,
            MetricEvidenceQuality::DegradedLowSample => {
                self.cross_pool_velocity_config.emit_degraded_low_sample
            }
            _ => false,
        };
        materialized.sybil_resistance.signer_cross_pool_velocity = if cpv_can_emit_value {
            cpv.signer_cross_pool_velocity
        } else {
            None
        };
        materialized.sybil_resistance.cpv_other_pool_activity = if cpv_can_emit_value {
            cpv.cpv_other_pool_activity
        } else {
            None
        };
        materialized.sybil_resistance.cpv_evidence = cpv.evidence_context();
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
        materialized.sybil_resistance.funding_source_v2 = Some(fsc.funding_source_v2.clone());
        for reason in &fsc.degraded_reasons {
            if !materialized
                .sybil_resistance
                .degraded_reasons
                .iter()
                .any(|existing| existing == reason)
            {
                materialized
                    .sybil_resistance
                    .degraded_reasons
                    .push(reason.clone());
            }
        }

        materialized.organic_broadening = self.materialize_v3_organic_broadening(&materialized);
        let manipulation_legacy = self.materialize_v3_manipulation_contradictions(&materialized);
        let manipulation_frozen = crate::metric_contracts::freeze_manipulation_producer_snapshot_v2(
            &materialized,
            manipulation_legacy,
        );
        materialized.manipulation_contradictions = manipulation_frozen.legacy.clone();
        materialized.temporal_deltas = self.materialize_v3_temporal_deltas();
        materialized.decision_time_series = self.materialize_decision_time_series();
        materialized.pre_entry_path_summary_v1 =
            self.materialize_rce_pre_entry_path_summary(&materialized.decision_time_series);
        materialized.session_regime_snapshot_v1 = self.materialize_rce_session_regime_snapshot();
        materialized.evidence_status = self.materialize_v3_evidence_status(&materialized);

        let static_context = self.metric_contract_static_context.as_deref().ok_or(
            MetricContractMaterializationErrorV1::MissingContext(
                "validated static metric-contract context",
            ),
        )?;
        let funding_source_producer_config = self
            .metric_contract_funding_source_producer_config
            .as_deref()
            .ok_or(MetricContractMaterializationErrorV1::MissingContext(
                "FSC producer config",
            ))?;
        let source_cutoff = MetricContractDecisionSourceCutoffV1::try_new(
            self.highest_seen_ts_ms
                .max(self.candidate_snapshot.timestamp)
                .max(self.created_at_wall_ms),
            self.tx_buffer
                .iter()
                .filter_map(|tx| tx.slot)
                .max()
                .or(self.candidate_snapshot.slot),
        )?;
        let tx_intelligence = self
            .tx_intelligence
            .metric_contract_snapshot(&materialized.tx_intel_features);
        let gatekeeper_dev_primary = self
            .gatekeeper_buffer
            .metric_contract_dev_primary_compatibility_snapshot();
        let recent_exact_timing = self.metric_contract_recent_exact_timing_snapshot();
        let recent_buy_sell = self.metric_contract_recent_buy_sell_snapshot()?;
        let decision_timestamp_ms = source_cutoff.decision_timestamp_ms.get();
        let decision_slot = match &source_cutoff.decision_slot {
            ghost_core::metric_contracts::CanonicalNullableV1::Value(value) => Some(value.get()),
            ghost_core::metric_contracts::CanonicalNullableV1::Null => None,
        };
        let flip_v2 = self
            .tx_intelligence
            .flip_v2_snapshot(decision_timestamp_ms, decision_slot);
        let reserve_velocity = self
            .account_state_core
            .metric_contract_reserve_velocity_snapshot(&self.base_mint);
        let legacy_flip_ratio = fingerprint_metrics
            .as_ref()
            .and_then(|fingerprint| fingerprint.flipper_presence_ratio);
        let complete =
            crate::metric_contracts::build_pr2b_timed_complete_metric_contract_snapshot_with_validated_static_context_v1(
                crate::metric_contracts::Pr2bFrozenProducerInputsV1 {
                    pr2a: crate::metric_contracts::Pr2aFrozenProducerInputsV1 {
                        ftdi: &sybil_computation.ftdi,
                        tx_intelligence: &tx_intelligence,
                        gatekeeper_dev_primary: &gatekeeper_dev_primary,
                        recent_exact_timing: &recent_exact_timing,
                        fsc: &fsc,
                        funding_source_config: &self.funding_source_config,
                        funding_source_producer_config,
                    },
                    legacy_flip_ratio,
                    flip_v2: &flip_v2,
                    manipulation: &manipulation_frozen,
                    reserve_velocity: &reserve_velocity,
                    recent_buy_sell: &recent_buy_sell,
                },
                static_context,
                source_cutoff,
            )?;
        materialized.metric_contract_decision_projection_v1 =
            Some(complete.snapshot().compact_projection.clone());
        *self.pr2c_last_complete_metric_contract_snapshot.lock() = Some(complete);

        Ok(materialized)
    }

    /// Consume the exact full-evidence/projection pair created by the most
    /// recent successful materialization. No producer or live-state read is
    /// performed here.
    pub fn take_pr2c_complete_metric_contract_snapshot(
        &self,
    ) -> Option<crate::metric_contracts::Pr2bTimedCompleteMetricContractSnapshotV1> {
        self.pr2c_last_complete_metric_contract_snapshot
            .lock()
            .take()
    }

    #[must_use]
    pub fn metric_contract_effective_config_for_replay(
        &self,
    ) -> Option<ResolvedMetricContractEffectiveConfigV1> {
        self.metric_contract_static_context
            .as_deref()
            .map(|context| context.effective_config().clone())
    }

    /// Stable pool-creation transaction identity supplied by ingest. This is
    /// independent of the decision join key and is therefore suitable for
    /// cross-run underlying-event collision checks when present.
    #[must_use]
    pub fn metric_contract_stable_event_signature(&self) -> Option<String> {
        (!self.candidate_snapshot.signature.trim().is_empty())
            .then(|| self.candidate_snapshot.signature.clone())
    }

    /// Compatibility facade for existing callers. Active terminal runtime
    /// paths use `try_materialize_features` and propagate its typed error.
    #[must_use]
    pub fn materialize_features(&self) -> MaterializedFeatureSet {
        match self.try_materialize_features() {
            Ok(features) => features,
            Err(error) => panic!("metric-contract materialization failed closed: {error}"),
        }
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

    pub fn update_tx_intelligence_dev_identity(
        &mut self,
        dev_wallet: Option<Pubkey>,
        create_signature: Option<&str>,
    ) {
        self.dev_wallet = dev_wallet;
        self.tx_intelligence
            .set_dev_identity(dev_wallet, create_signature);
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

    pub fn set_funding_source_config(&mut self, config: FundingSourceConfig) {
        self.funding_source_config = config;
    }

    pub fn set_metric_contract_context(
        &mut self,
        effective_config: Arc<ResolvedMetricContractEffectiveConfigV1>,
        funding_source_producer_config: Arc<FundingSourceProducerConfigSnapshotV1>,
    ) -> Result<(), MetricContractMaterializationErrorV1> {
        let profile = MetricContractFoundationConfigV1 {
            metric_contract_rollout_mode: effective_config.payload.rollout_mode,
            metric_contract_profile: effective_config.payload.profile_id,
        }
        .resolve_profile()?;
        let static_context = MetricDecisionProjectionValidatedStaticContextV1::try_new(
            effective_config.payload.rollout_mode,
            profile,
            effective_config.as_ref().clone(),
        )?;
        self.metric_contract_static_context = Some(Arc::new(static_context));
        self.metric_contract_funding_source_producer_config = Some(funding_source_producer_config);
        Ok(())
    }

    pub fn mark_metric_contract_stream_gap(&mut self) {
        self.tx_intelligence.mark_flip_v2_reconnect_gap();
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

fn evidence_clean() -> FeatureEvidenceStatus {
    FeatureEvidenceStatus {
        status: EvidenceStatus::Clean,
        degraded_reasons: Vec::new(),
        unavailable_reasons: Vec::new(),
    }
}

fn evidence_degraded(reasons: Vec<EvidenceDegradedReason>) -> FeatureEvidenceStatus {
    FeatureEvidenceStatus {
        status: EvidenceStatus::Degraded,
        degraded_reasons: reasons,
        unavailable_reasons: Vec::new(),
    }
}

fn evidence_insufficient_sample(reasons: Vec<EvidenceDegradedReason>) -> FeatureEvidenceStatus {
    FeatureEvidenceStatus {
        status: EvidenceStatus::InsufficientSample,
        degraded_reasons: reasons,
        unavailable_reasons: Vec::new(),
    }
}

fn evidence_fallback(reasons: Vec<EvidenceDegradedReason>) -> FeatureEvidenceStatus {
    FeatureEvidenceStatus {
        status: EvidenceStatus::Fallback,
        degraded_reasons: reasons,
        unavailable_reasons: Vec::new(),
    }
}

fn evidence_unavailable(reasons: Vec<EvidenceUnavailableReason>) -> FeatureEvidenceStatus {
    FeatureEvidenceStatus {
        status: EvidenceStatus::Unavailable,
        degraded_reasons: Vec::new(),
        unavailable_reasons: reasons,
    }
}

fn growth_ratio(later: u64, earlier: u64) -> f64 {
    if earlier == 0 {
        return 0.0;
    }
    (later as f64 - earlier as f64) / earlier as f64
}

fn v3_broadening_score(features: &OrganicBroadeningFeatures) -> f64 {
    if !features.sequence_available {
        return 0.0;
    }

    let signer_growth = features.unique_signer_growth_ratio.max(0.0).min(1.0);
    let tx_growth = features.tx_count_growth_ratio.max(0.0).min(1.0);
    let new_signers = features.new_signer_ratio_t2.clamp(0.0, 1.0);
    let hhi_score = (1.0 - features.max_segment_hhi).clamp(0.0, 1.0);

    (0.30 * signer_growth + 0.25 * tx_growth + 0.25 * new_signers + 0.20 * hhi_score)
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod metric_contract_recent_window_tests {
    use super::PoolObservationSession;

    #[test]
    fn reversed_recent_window_remains_empty() {
        let stats = PoolObservationSession::rce_window_stats(&[], 2, 1);
        assert_eq!(stats.tx_count, 0);
        assert_eq!(stats.same_ms_extra_count, 0);
        assert_eq!(stats.same_ms_tx_ratio, None);
    }
}
