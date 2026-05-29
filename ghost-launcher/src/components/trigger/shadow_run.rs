use crate::components::trigger::component::PreparedBuyRequest;
use crate::components::trigger::component::TriggerDispatchFailureContext;
use crate::config::{ShadowRunCommitment, TriggerEntryMode, TriggerShadowRunConfig};
use crate::events::{
    build_execution_candidate_id, ExecutionJoinMetadata, ShadowBuySimulationEvent,
    ShadowSimulationAccountDiagnostics,
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use blake3::Hasher;
use metrics::{counter, histogram, increment_counter};
use seer::new_async_rpc_client;
use solana_account_decoder::{UiAccount, UiAccountEncoding};
use solana_client::rpc_config::{
    RpcSimulateTransactionAccountsConfig, RpcSimulateTransactionConfig,
};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::program_pack::Pack;
use spl_token_2022::state::Account as SplTokenAccount;
#[cfg(test)]
use std::collections::HashMap;
use std::path::Path;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tracing::warn;

/// Compatibility alias: config owns the payer-strategy contract.
pub use crate::config::TriggerShadowPayerStrategy as ShadowPayerStrategy;

/// Generate an idempotency key for a shadow dispatch.
/// Uses blake3(pool_id || join_key || rollout_profile) for dedup.
pub fn make_shadow_idempotency_key(pool_id: &str, join_key: &str, rollout_profile: &str) -> String {
    let mut hasher = Hasher::new();
    hasher.update(pool_id.as_bytes());
    hasher.update(b":");
    hasher.update(join_key.as_bytes());
    hasher.update(b":");
    hasher.update(rollout_profile.as_bytes());
    hasher.finalize().to_hex().to_string()
}

pub fn make_shadow_join_key(pool_id: &str, base_mint: &str, first_seen_ts_ms: u64) -> String {
    format!("{pool_id}:{base_mint}:{first_seen_ts_ms}")
}

pub fn derive_shadow_rollout_profile_from_path(path: &Path) -> String {
    let mut previous = None::<String>;
    for component in path.components() {
        let current = component.as_os_str().to_string_lossy();
        if previous.as_deref() == Some("rollout") && !current.is_empty() {
            return current.into_owned();
        }
        previous = Some(current.into_owned());
    }
    "unknown_rollout".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowDispatchStatus {
    Submitted,
    Failed,
    Abandoned,
    Closed,
}

impl ShadowDispatchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ShadowDispatchStatus::Submitted => "submitted",
            ShadowDispatchStatus::Failed => "failed",
            ShadowDispatchStatus::Abandoned => "abandoned",
            ShadowDispatchStatus::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ShadowDispatchLifecycleRecord {
    #[serde(default, flatten)]
    pub join_metadata: ExecutionJoinMetadata,
    #[serde(default, flatten)]
    pub account_diagnostics: ShadowSimulationAccountDiagnostics,
    pub record_type: String,
    pub dispatch_id: String,
    pub idempotency_key: String,
    pub dispatch_status: ShadowDispatchStatus,
    pub classification: String,
    pub simulation_outcome: String,
    pub candidate_id: String,
    pub pool_id: String,
    pub mint_id: String,
    pub join_key: String,
    pub rollout_profile: String,
    pub entry_mode: String,
    pub decision_ts_ms: u64,
    pub timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_detail_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub err: Option<String>,
}

impl ShadowDispatchLifecycleRecord {
    fn failure_classification(record: &ShadowBuySimulationRecord) -> String {
        record
            .error_detail_class
            .clone()
            .or_else(|| record.error_class.clone())
            .or_else(|| {
                record
                    .err
                    .as_deref()
                    .map(classify_shadow_error)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "unclassified".to_string())
    }

    fn from_shadow_buy_record_with_status(
        record: &ShadowBuySimulationRecord,
        join_key: impl Into<String>,
        rollout_profile: impl Into<String>,
        status: ShadowDispatchStatus,
    ) -> ShadowDispatchLifecycleRecord {
        let join_key = join_key.into();
        let rollout_profile = rollout_profile.into();
        let idempotency_key = record.idempotency_key.clone().unwrap_or_else(|| {
            make_shadow_idempotency_key(&record.pool_amm_id, &join_key, &rollout_profile)
        });
        ShadowDispatchLifecycleRecord {
            join_metadata: record.join_metadata.clone(),
            account_diagnostics: record.account_diagnostics.clone(),
            record_type: "shadow_dispatch".to_string(),
            dispatch_id: format!("shadow-dispatch:{idempotency_key}"),
            idempotency_key,
            dispatch_status: status,
            classification: match status {
                ShadowDispatchStatus::Submitted => "dispatch_submitted".to_string(),
                ShadowDispatchStatus::Closed => "simulation_completed".to_string(),
                ShadowDispatchStatus::Failed | ShadowDispatchStatus::Abandoned => {
                    Self::failure_classification(record)
                }
            },
            simulation_outcome: status.as_str().to_string(),
            candidate_id: record.candidate_id.clone(),
            pool_id: record.pool_amm_id.clone(),
            mint_id: record.base_mint.clone(),
            join_key,
            rollout_profile,
            entry_mode: record.entry_mode.clone(),
            decision_ts_ms: record.decision_ts_ms,
            timestamp_ms: record.sim_finished_ts_ms.max(record.decision_ts_ms),
            error_class: record.error_class.clone(),
            error_code: record.error_code.clone(),
            error_detail_class: record.error_detail_class.clone(),
            err: record.err.clone(),
        }
    }

    pub fn submitted_from_request(
        entry_mode: TriggerEntryMode,
        pool_amm_id: &str,
        base_mint: &str,
        request: &PreparedBuyRequest,
        join_key: impl Into<String>,
        rollout_profile: impl Into<String>,
    ) -> ShadowDispatchLifecycleRecord {
        let join_key = join_key.into();
        let rollout_profile = rollout_profile.into();
        let idempotency_key = make_shadow_idempotency_key(pool_amm_id, &join_key, &rollout_profile);
        ShadowDispatchLifecycleRecord {
            join_metadata: request.join_metadata.clone(),
            account_diagnostics: ShadowSimulationAccountDiagnostics::default(),
            record_type: "shadow_dispatch".to_string(),
            dispatch_id: format!("shadow-dispatch:{idempotency_key}"),
            idempotency_key,
            dispatch_status: ShadowDispatchStatus::Submitted,
            classification: "dispatch_submitted".to_string(),
            simulation_outcome: ShadowDispatchStatus::Submitted.as_str().to_string(),
            candidate_id: build_execution_candidate_id(
                base_mint,
                pool_amm_id,
                request.decision_ts_ms.to_string(),
            ),
            pool_id: pool_amm_id.to_string(),
            mint_id: base_mint.to_string(),
            join_key,
            rollout_profile,
            entry_mode: entry_mode.as_str().to_string(),
            decision_ts_ms: request.decision_ts_ms,
            timestamp_ms: current_time_ms().max(request.decision_ts_ms),
            error_class: None,
            error_code: None,
            error_detail_class: None,
            err: None,
        }
    }

    pub fn terminal_from_shadow_buy_record(
        record: &ShadowBuySimulationRecord,
        join_key: impl Into<String>,
        rollout_profile: impl Into<String>,
    ) -> ShadowDispatchLifecycleRecord {
        let status = if record.err.is_some() {
            ShadowDispatchStatus::Failed
        } else {
            ShadowDispatchStatus::Closed
        };
        Self::from_shadow_buy_record_with_status(record, join_key, rollout_profile, status)
    }

    pub fn failed_from_shadow_buy_record(
        record: &ShadowBuySimulationRecord,
        join_key: impl Into<String>,
        rollout_profile: impl Into<String>,
    ) -> ShadowDispatchLifecycleRecord {
        Self::from_shadow_buy_record_with_status(
            record,
            join_key,
            rollout_profile,
            ShadowDispatchStatus::Failed,
        )
    }

    pub fn abandoned_from_shadow_buy_record(
        record: &ShadowBuySimulationRecord,
        join_key: impl Into<String>,
        rollout_profile: impl Into<String>,
    ) -> ShadowDispatchLifecycleRecord {
        Self::from_shadow_buy_record_with_status(
            record,
            join_key,
            rollout_profile,
            ShadowDispatchStatus::Abandoned,
        )
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShadowBuySimulationReport {
    #[serde(default, flatten)]
    pub join_metadata: ExecutionJoinMetadata,
    pub mint: String,
    pub live_signature: Option<String>,
    pub payer_pubkey: String,
    pub payer_provenance: String,
    pub amount_lamports: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_token_amount_raw: Option<u64>,
    pub tip_lamports: u64,
    pub decision_ts_ms: u64,
    pub simulation_started_ts_ms: u64,
    pub simulation_finished_ts_ms: u64,
    pub latency_ms: u64,
    pub shadow_duration_ms: u64,
    pub rpc_slot: u64,
    pub retry_count: usize,
    pub used_sig_verify: bool,
    pub used_replace_recent_blockhash: bool,
    pub units_consumed: Option<u64>,
    pub logs: Vec<String>,
    pub return_data: Option<String>,
    pub err: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ShadowBuySimulationRecord {
    #[serde(default, flatten)]
    pub join_metadata: ExecutionJoinMetadata,
    #[serde(default, flatten)]
    pub account_diagnostics: ShadowSimulationAccountDiagnostics,
    pub candidate_id: String,
    pub pool_amm_id: String,
    pub base_mint: String,
    pub entry_mode: String,
    pub decision_ts_ms: u64,
    pub sim_started_ts_ms: u64,
    pub sim_finished_ts_ms: u64,
    pub decision_to_sim_start_ms: u64,
    pub shadow_duration_ms: u64,
    pub amount_lamports: u64,
    pub tip_lamports: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_token_amount_raw: Option<u64>,
    pub payer_provenance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payer_pubkey: Option<String>,
    pub err: Option<String>,
    pub error_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_detail_class: Option<String>,
    pub units_consumed: Option<u64>,
    pub rpc_slot: Option<u64>,
    pub retry_count: usize,
    pub live_signature: Option<String>,
    pub logs_digest: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs_excerpt: Vec<String>,
    /// P5: Idempotency key for deduplication in shadow lifecycle reconciliation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ShadowSimulationError {
    message: String,
    retry_count: usize,
}

impl ShadowSimulationError {
    fn new(message: impl Into<String>, retry_count: usize) -> Self {
        Self {
            message: message.into(),
            retry_count,
        }
    }

    pub fn retry_count(&self) -> usize {
        self.retry_count
    }
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ShadowPreparationError {
    message: String,
    retry_count: usize,
}

impl ShadowPreparationError {
    pub fn new(message: impl Into<String>, retry_count: usize) -> Self {
        Self {
            message: message.into(),
            retry_count,
        }
    }

    pub fn retry_count(&self) -> usize {
        self.retry_count
    }
}

impl ShadowBuySimulationRecord {
    /// P5: Attach an idempotency key for shadow lifecycle dedup.
    pub fn with_idempotency_key(mut self, key: String) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    pub fn with_lifecycle_identity(
        mut self,
        join_key: impl Into<String>,
        rollout_profile: impl Into<String>,
    ) -> Self {
        let join_key = join_key.into();
        let rollout_profile = rollout_profile.into();
        self.idempotency_key = Some(make_shadow_idempotency_key(
            &self.pool_amm_id,
            &join_key,
            &rollout_profile,
        ));
        self
    }

    pub fn from_event(
        entry_mode: TriggerEntryMode,
        event: &ShadowBuySimulationEvent,
    ) -> ShadowBuySimulationRecord {
        let diagnostics = shadow_error_diagnostics(event.err.as_deref());
        ShadowBuySimulationRecord {
            join_metadata: event.join_metadata.clone(),
            account_diagnostics: event.account_diagnostics.clone(),
            candidate_id: event.candidate_id.clone(),
            pool_amm_id: event.pool_amm_id.clone(),
            base_mint: event.base_mint.clone(),
            entry_mode: entry_mode.as_str().to_string(),
            decision_ts_ms: event.decision_ts_ms,
            sim_started_ts_ms: event.simulation_started_ts_ms,
            sim_finished_ts_ms: event.simulation_finished_ts_ms,
            decision_to_sim_start_ms: event
                .simulation_started_ts_ms
                .saturating_sub(event.decision_ts_ms),
            shadow_duration_ms: event.shadow_duration_ms,
            amount_lamports: event.amount_lamports,
            tip_lamports: event.tip_lamports,
            entry_token_amount_raw: event.entry_token_amount_raw,
            payer_provenance: event.payer_provenance.clone(),
            payer_pubkey: Some(event.payer_pubkey.clone()),
            err: event.err.clone(),
            error_class: event.error_class.clone().or(diagnostics.error_class),
            error_code: event.error_code.clone().or(diagnostics.error_code),
            error_detail_class: event
                .error_detail_class
                .clone()
                .or(diagnostics.error_detail_class),
            units_consumed: event.units_consumed,
            rpc_slot: Some(event.rpc_slot),
            retry_count: event.retry_count,
            live_signature: event.live_signature.clone(),
            logs_digest: logs_digest(&event.logs),
            logs_excerpt: summarize_logs(&event.logs),
            idempotency_key: None,
        }
    }

    pub fn from_failure(
        entry_mode: TriggerEntryMode,
        pool_amm_id: &str,
        base_mint: &str,
        request: &PreparedBuyRequest,
        live_signature: Option<String>,
        err: &anyhow::Error,
    ) -> ShadowBuySimulationRecord {
        let finished_ts_ms = current_time_ms();
        let err_string = err.to_string();
        let diagnostics = shadow_error_diagnostics(Some(&err_string));
        ShadowBuySimulationRecord {
            join_metadata: request.join_metadata.clone(),
            account_diagnostics: ShadowSimulationAccountDiagnostics::default(),
            candidate_id: build_execution_candidate_id(
                base_mint,
                pool_amm_id,
                request.decision_ts_ms.to_string(),
            ),
            pool_amm_id: pool_amm_id.to_string(),
            base_mint: base_mint.to_string(),
            entry_mode: entry_mode.as_str().to_string(),
            decision_ts_ms: request.decision_ts_ms,
            sim_started_ts_ms: request.decision_ts_ms,
            sim_finished_ts_ms: finished_ts_ms,
            decision_to_sim_start_ms: 0,
            shadow_duration_ms: finished_ts_ms.saturating_sub(request.decision_ts_ms),
            amount_lamports: request.amount_lamports,
            tip_lamports: request.tip_lamports,
            entry_token_amount_raw: request.entry_token_amount_raw,
            payer_provenance: request.payer_provenance.to_string(),
            payer_pubkey: Some(request.payer_pubkey.to_string()),
            err: Some(err_string.clone()),
            error_class: diagnostics.error_class,
            error_code: diagnostics.error_code,
            error_detail_class: diagnostics.error_detail_class,
            units_consumed: None,
            rpc_slot: None,
            retry_count: shadow_error_retry_count(err),
            live_signature,
            logs_digest: logs_digest(&[]),
            logs_excerpt: Vec::new(),
            idempotency_key: None,
        }
    }

    pub fn from_failure_context(
        entry_mode: TriggerEntryMode,
        pool_amm_id: &str,
        base_mint: &str,
        context: &TriggerDispatchFailureContext,
        live_signature: Option<String>,
        err: &anyhow::Error,
    ) -> ShadowBuySimulationRecord {
        let finished_ts_ms = current_time_ms();
        let err_string = err.to_string();
        let diagnostics = shadow_error_diagnostics(Some(&err_string));
        ShadowBuySimulationRecord {
            join_metadata: context.join_metadata.clone(),
            account_diagnostics: ShadowSimulationAccountDiagnostics {
                active_shadow_precheck_status: Some("not_run_no_prepared_request".to_string()),
                active_shadow_lifecycle_eligibility_status: Some(
                    "not_lifecycle_eligible".to_string(),
                ),
                ..Default::default()
            },
            candidate_id: build_execution_candidate_id(
                base_mint,
                pool_amm_id,
                context.decision_ts_ms.to_string(),
            ),
            pool_amm_id: pool_amm_id.to_string(),
            base_mint: base_mint.to_string(),
            entry_mode: entry_mode.as_str().to_string(),
            decision_ts_ms: context.decision_ts_ms,
            sim_started_ts_ms: context.decision_ts_ms,
            sim_finished_ts_ms: finished_ts_ms,
            decision_to_sim_start_ms: 0,
            shadow_duration_ms: finished_ts_ms.saturating_sub(context.decision_ts_ms),
            amount_lamports: context.amount_lamports,
            tip_lamports: context.tip_lamports,
            entry_token_amount_raw: None,
            payer_provenance: context.payer_provenance.to_string(),
            payer_pubkey: context.payer_pubkey.clone(),
            err: Some(err_string.clone()),
            error_class: diagnostics.error_class,
            error_code: diagnostics.error_code,
            error_detail_class: diagnostics.error_detail_class,
            units_consumed: None,
            rpc_slot: None,
            retry_count: shadow_error_retry_count(err),
            live_signature,
            logs_digest: logs_digest(&[]),
            logs_excerpt: Vec::new(),
            idempotency_key: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ShadowErrorDiagnostics {
    pub error_class: Option<String>,
    pub error_code: Option<String>,
    pub error_detail_class: Option<String>,
}

pub(crate) fn shadow_error_diagnostics(err: Option<&str>) -> ShadowErrorDiagnostics {
    ShadowErrorDiagnostics {
        error_class: err.map(classify_shadow_error).map(str::to_string),
        error_code: err.and_then(extract_shadow_error_code).map(str::to_string),
        error_detail_class: err
            .and_then(classify_shadow_error_detail)
            .map(str::to_string),
    }
}

pub fn shadow_failure_event_from_request(
    pool_amm_id: &str,
    base_mint: &str,
    request: &PreparedBuyRequest,
    live_signature: Option<String>,
    err: &anyhow::Error,
) -> ShadowBuySimulationEvent {
    let finished_ts_ms = current_time_ms();
    let err_string = err.to_string();
    let diagnostics = shadow_error_diagnostics(Some(&err_string));
    ShadowBuySimulationEvent {
        join_metadata: request.join_metadata.clone(),
        account_diagnostics: ShadowSimulationAccountDiagnostics::default(),
        candidate_id: build_execution_candidate_id(
            base_mint,
            pool_amm_id,
            request.decision_ts_ms.to_string(),
        ),
        pool_amm_id: pool_amm_id.to_string(),
        base_mint: base_mint.to_string(),
        mint: request.mint.to_string(),
        live_signature,
        payer_pubkey: request.payer_pubkey.to_string(),
        payer_provenance: request.payer_provenance.to_string(),
        amount_lamports: request.amount_lamports,
        entry_token_amount_raw: request.entry_token_amount_raw,
        tip_lamports: request.tip_lamports,
        decision_ts_ms: request.decision_ts_ms,
        simulation_started_ts_ms: request.decision_ts_ms,
        simulation_finished_ts_ms: finished_ts_ms,
        latency_ms: finished_ts_ms.saturating_sub(request.decision_ts_ms),
        shadow_duration_ms: finished_ts_ms.saturating_sub(request.decision_ts_ms),
        rpc_slot: 0,
        retry_count: shadow_error_retry_count(err),
        used_sig_verify: false,
        used_replace_recent_blockhash: false,
        units_consumed: None,
        logs: Vec::new(),
        return_data: None,
        err: Some(err_string.clone()),
        error_class: diagnostics.error_class,
        error_code: diagnostics.error_code,
        error_detail_class: diagnostics.error_detail_class,
    }
}

pub fn shadow_failure_event_from_context(
    pool_amm_id: &str,
    base_mint: &str,
    context: &TriggerDispatchFailureContext,
    mint: impl Into<String>,
    live_signature: Option<String>,
    err: &anyhow::Error,
) -> ShadowBuySimulationEvent {
    let finished_ts_ms = current_time_ms();
    let err_string = err.to_string();
    let diagnostics = shadow_error_diagnostics(Some(&err_string));
    ShadowBuySimulationEvent {
        join_metadata: context.join_metadata.clone(),
        account_diagnostics: ShadowSimulationAccountDiagnostics {
            active_shadow_precheck_status: Some("not_run_no_prepared_request".to_string()),
            active_shadow_lifecycle_eligibility_status: Some("not_lifecycle_eligible".to_string()),
            ..Default::default()
        },
        candidate_id: build_execution_candidate_id(
            base_mint,
            pool_amm_id,
            context.decision_ts_ms.to_string(),
        ),
        pool_amm_id: pool_amm_id.to_string(),
        base_mint: base_mint.to_string(),
        mint: mint.into(),
        live_signature,
        payer_pubkey: context
            .payer_pubkey
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        payer_provenance: context.payer_provenance.to_string(),
        amount_lamports: context.amount_lamports,
        entry_token_amount_raw: None,
        tip_lamports: context.tip_lamports,
        decision_ts_ms: context.decision_ts_ms,
        simulation_started_ts_ms: context.decision_ts_ms,
        simulation_finished_ts_ms: finished_ts_ms,
        latency_ms: finished_ts_ms.saturating_sub(context.decision_ts_ms),
        shadow_duration_ms: finished_ts_ms.saturating_sub(context.decision_ts_ms),
        rpc_slot: 0,
        retry_count: shadow_error_retry_count(err),
        used_sig_verify: false,
        used_replace_recent_blockhash: false,
        units_consumed: None,
        logs: Vec::new(),
        return_data: None,
        err: Some(err_string.clone()),
        error_class: diagnostics.error_class,
        error_code: diagnostics.error_code,
        error_detail_class: diagnostics.error_detail_class,
    }
}

pub(crate) fn shadow_buy_event_from_report(
    pool_amm_id: &str,
    base_mint: &str,
    report: ShadowBuySimulationReport,
) -> ShadowBuySimulationEvent {
    let ShadowBuySimulationReport {
        join_metadata,
        mint,
        live_signature,
        payer_pubkey,
        payer_provenance,
        amount_lamports,
        entry_token_amount_raw,
        tip_lamports,
        decision_ts_ms,
        simulation_started_ts_ms,
        simulation_finished_ts_ms,
        latency_ms,
        shadow_duration_ms,
        rpc_slot,
        retry_count,
        used_sig_verify,
        used_replace_recent_blockhash,
        units_consumed,
        logs,
        return_data,
        err,
    } = report;
    let trace_ref = live_signature
        .clone()
        .unwrap_or_else(|| decision_ts_ms.to_string());
    let diagnostics = shadow_error_diagnostics(err.as_deref());
    ShadowBuySimulationEvent {
        join_metadata,
        account_diagnostics: ShadowSimulationAccountDiagnostics::default(),
        candidate_id: build_execution_candidate_id(base_mint, pool_amm_id, trace_ref),
        pool_amm_id: pool_amm_id.to_string(),
        base_mint: base_mint.to_string(),
        mint,
        live_signature,
        payer_pubkey,
        payer_provenance,
        amount_lamports,
        entry_token_amount_raw,
        tip_lamports,
        decision_ts_ms,
        simulation_started_ts_ms,
        simulation_finished_ts_ms,
        latency_ms,
        shadow_duration_ms,
        rpc_slot,
        retry_count,
        used_sig_verify,
        used_replace_recent_blockhash,
        units_consumed,
        logs,
        return_data,
        err,
        error_class: diagnostics.error_class,
        error_code: diagnostics.error_code,
        error_detail_class: diagnostics.error_detail_class,
    }
}

#[derive(Debug, Clone)]
pub enum TriggerBuyOutcome {
    LiveConfirmed {
        signature: solana_sdk::signature::Signature,
        landed_slot: Option<u64>,
    },
    DryRunMock {
        signature: solana_sdk::signature::Signature,
    },
    ShadowSimulated {
        report: ShadowBuySimulationReport,
    },
}

#[async_trait]
pub trait ShadowSimulator: Send + Sync {
    async fn simulate_buy(
        &self,
        request: &PreparedBuyRequest,
        config: &TriggerShadowRunConfig,
    ) -> Result<ShadowBuySimulationReport>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShadowMetricsSnapshot {
    pub shadow_buy_total: u64,
    pub shadow_buy_success_total: u64,
    pub shadow_buy_failure_total: u64,
    pub shadow_buy_retry_total: u64,
    pub shadow_buy_queue_overflow_total: u64,
}

#[derive(Debug, Default)]
pub struct RpcShadowSimulator;

impl RpcShadowSimulator {
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn commitment(commitment: ShadowRunCommitment) -> CommitmentConfig {
        match commitment {
            ShadowRunCommitment::Processed => CommitmentConfig::processed(),
            ShadowRunCommitment::Confirmed => CommitmentConfig::confirmed(),
            ShadowRunCommitment::Finalized => CommitmentConfig::finalized(),
        }
    }

    pub(crate) fn is_retryable(message: &str) -> bool {
        let lower = message.to_lowercase();
        lower.contains("blockhash")
            || lower.contains("timed out")
            || lower.contains("timeout")
            || lower.contains("connection reset")
            || lower.contains("connection refused")
            || lower.contains("transport")
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn logs_digest(logs: &[String]) -> String {
    let mut hasher = Hasher::new();
    for log in logs {
        hasher.update(log.as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize().to_hex().to_string()
}

fn summarize_logs(logs: &[String]) -> Vec<String> {
    const LOGS_EXCERPT_HEAD: usize = 6;
    const LOGS_EXCERPT_TAIL: usize = 6;
    if logs.len() <= LOGS_EXCERPT_HEAD + LOGS_EXCERPT_TAIL {
        return logs.to_vec();
    }

    let omitted = logs
        .len()
        .saturating_sub(LOGS_EXCERPT_HEAD + LOGS_EXCERPT_TAIL);
    let mut excerpt = Vec::with_capacity(LOGS_EXCERPT_HEAD + LOGS_EXCERPT_TAIL + 1);
    excerpt.extend(logs.iter().take(LOGS_EXCERPT_HEAD).cloned());
    excerpt.push(format!("... {} log lines omitted ...", omitted));
    excerpt.extend(
        logs.iter()
            .skip(logs.len().saturating_sub(LOGS_EXCERPT_TAIL))
            .cloned(),
    );
    excerpt
}

fn extract_shadow_error_code(err: &str) -> Option<&'static str> {
    let lower = err.to_lowercase();
    if lower.contains("custom(2006)") || lower.contains("constraintseeds") {
        Some("2006")
    } else if lower.contains("custom(6000)") || lower.contains("notauthorized") {
        Some("6000")
    } else if lower.contains("custom(6062)") {
        Some("6062")
    } else if lower.contains("custom(6024)") || lower.contains("overflow") {
        Some("6024")
    } else {
        None
    }
}

fn classify_shadow_error_detail(err: &str) -> Option<&'static str> {
    let lower = err.to_lowercase();
    if lower.contains("custom(2006)") || lower.contains("constraintseeds") {
        Some("seed_mismatch_constraint_seeds")
    } else if lower.contains("custom(6000)") || lower.contains("notauthorized") {
        Some("protocol_not_authorized")
    } else if lower.contains("custom(6062)") {
        Some("buyback_fee_recipient_missing")
    } else if lower.contains("custom(6024)") || lower.contains("overflow") {
        Some("legacy_buy_amount_overflow")
    } else {
        None
    }
}

pub fn classify_shadow_error(err: &str) -> &'static str {
    let lower = err.to_lowercase();
    if RpcShadowSimulator::is_retryable(&lower)
        || lower.contains("rpc")
        || lower.contains("too many requests")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("transport")
    {
        "network_provider_problem"
    } else if lower.contains("blockhash")
        || lower.contains("last valid block height")
        || lower.contains("expired")
        || lower.contains("timed out")
        || lower.contains("timeout")
    {
        "timing_blockhash_problem"
    } else if lower.contains("invalidaccountforfee")
        || lower.contains("invalid account for fee")
        || lower.contains("fee payer")
        || lower.contains("owner")
        || lower.contains("authority")
        || lower.contains("signature")
    {
        "authority_problem"
    } else if lower.contains("insufficient funds")
        || lower.contains("insufficientfundsforrent")
        || lower.contains("insufficient payer balance")
        || lower.contains("balance critical")
        || lower.contains("no safe trade capacity")
        || lower.contains("insufficient safe balance")
        || lower.contains("priority fee")
        || lower.contains("compute")
        || lower.contains("rent")
        || lower.contains("budget")
    {
        "fee_compute_problem"
    } else if lower.contains("accountnotfound")
        || lower.contains("failed to fetch mint account")
        || lower.contains("missing canonical")
        || lower.contains("metadata missing")
        || lower.contains("account not visible")
        || lower.contains("invalid associated_bonding_curve")
    {
        "data_problem"
    } else if lower.contains("unsupported token account encoding")
        || lower.contains("token balance regressed")
        || lower.contains("zero token delta")
        || lower.contains("simulation error")
        || lower.contains("custom program error")
        || lower.contains("instructionerror")
    {
        "simulation_mismatch"
    } else if lower.contains("join error")
        || lower.contains("semaphore")
        || lower.contains("panic")
        || lower.contains("invariant")
        || lower.contains("not initialized")
    {
        "logic_invariant_problem"
    } else {
        "unclassified"
    }
}

pub fn shadow_error_retry_count(err: &anyhow::Error) -> usize {
    err.chain()
        .find_map(|cause| {
            cause
                .downcast_ref::<ShadowSimulationError>()
                .map(ShadowSimulationError::retry_count)
                .or_else(|| {
                    cause
                        .downcast_ref::<ShadowPreparationError>()
                        .map(ShadowPreparationError::retry_count)
                })
        })
        .unwrap_or(0)
}

fn resolve_simulated_entry_token_amount_raw(
    request: &PreparedBuyRequest,
    simulated_accounts: Option<&Vec<Option<UiAccount>>>,
) -> Result<Option<u64>> {
    let Some(simulated_account) = simulated_accounts
        .and_then(|accounts| accounts.first())
        .and_then(|account| account.as_ref())
    else {
        return Ok(None);
    };

    let Some(account_data) = simulated_account.data.decode() else {
        return Err(anyhow!(
            "shadow simulation returned unsupported token account encoding for {}",
            request.user_ata
        ));
    };
    let token_account = SplTokenAccount::unpack(&account_data).map_err(|error| {
        anyhow!(
            "failed to decode simulated token account {}: {}",
            request.user_ata,
            error
        )
    })?;
    let pre_submit_balance = request
        .pre_submit_token_balance
        .or_else(|| request.ata_missing_pre_submit.then_some(0))
        .ok_or_else(|| {
            anyhow!(
                "missing pre-submit token balance for simulated shadow buy {}",
                request.user_ata
            )
        })?;
    let delta = token_account
        .amount
        .checked_sub(pre_submit_balance)
        .ok_or_else(|| {
            anyhow!(
                "simulated token balance regressed for {}: pre_submit={} post_sim={}",
                request.user_ata,
                pre_submit_balance,
                token_account.amount
            )
        })?;
    if delta == 0 {
        return Err(anyhow!(
            "simulated shadow buy produced zero token delta for {}",
            request.user_ata
        ));
    }
    Ok(Some(delta))
}

pub async fn append_shadow_buy_record(
    log_path: &Path,
    record: &ShadowBuySimulationRecord,
) -> Result<()> {
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .await?;
    let json = serde_json::to_string(record)?;
    tokio::io::AsyncWriteExt::write_all(&mut file, json.as_bytes()).await?;
    tokio::io::AsyncWriteExt::write_all(&mut file, b"\n").await?;
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    Ok(())
}

pub async fn append_shadow_dispatch_lifecycle_record(
    log_path: &Path,
    record: &ShadowDispatchLifecycleRecord,
) -> Result<()> {
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .await?;
    let json = serde_json::to_string(record)?;
    tokio::io::AsyncWriteExt::write_all(&mut file, json.as_bytes()).await?;
    tokio::io::AsyncWriteExt::write_all(&mut file, b"\n").await?;
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    Ok(())
}

#[cfg(test)]
fn test_metrics_state() -> &'static Mutex<HashMap<&'static str, u64>> {
    static METRICS: OnceLock<Mutex<HashMap<&'static str, u64>>> = OnceLock::new();
    METRICS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
fn bump_test_metric(key: &'static str, by: u64) {
    let mut guard = test_metrics_state().lock().expect("metrics test mutex");
    *guard.entry(key).or_insert(0) += by;
}

#[cfg(not(test))]
fn bump_test_metric(_: &'static str, _: u64) {}

pub fn record_shadow_buy_metrics(record: &ShadowBuySimulationRecord) {
    increment_counter!(
        "shadow_buy_total",
        "entry_mode" => record.entry_mode.clone()
    );
    bump_test_metric("shadow_buy_total", 1);

    if let Some(err) = record.err.as_deref() {
        let class = record
            .error_class
            .clone()
            .unwrap_or_else(|| classify_shadow_error(err).to_string());
        increment_counter!(
            "shadow_buy_failure_total",
            "entry_mode" => record.entry_mode.clone(),
            "error_class" => class
        );
        bump_test_metric("shadow_buy_failure_total", 1);
    } else {
        increment_counter!(
            "shadow_buy_success_total",
            "entry_mode" => record.entry_mode.clone()
        );
        bump_test_metric("shadow_buy_success_total", 1);
    }

    if record.retry_count > 0 {
        counter!(
            "shadow_buy_retry_total",
            record.retry_count as u64,
            "entry_mode" => record.entry_mode.clone()
        );
        bump_test_metric("shadow_buy_retry_total", record.retry_count as u64);
    }

    histogram!(
        "shadow_buy_latency_ms",
        record.shadow_duration_ms as f64,
        "entry_mode" => record.entry_mode.clone()
    );

    if let Some(units_consumed) = record.units_consumed {
        histogram!(
            "shadow_buy_units_consumed",
            units_consumed as f64,
            "entry_mode" => record.entry_mode.clone()
        );
    }
}

pub fn record_shadow_buy_queue_overflow(entry_mode: TriggerEntryMode) {
    increment_counter!(
        "shadow_buy_queue_overflow_total",
        "entry_mode" => entry_mode.as_str()
    );
    bump_test_metric("shadow_buy_queue_overflow_total", 1);
}

#[cfg(test)]
pub fn reset_shadow_metrics_test_snapshot() {
    test_metrics_state()
        .lock()
        .expect("metrics test mutex")
        .clear();
}

#[cfg(test)]
pub fn shadow_metrics_test_snapshot() -> ShadowMetricsSnapshot {
    let guard = test_metrics_state().lock().expect("metrics test mutex");
    ShadowMetricsSnapshot {
        shadow_buy_total: *guard.get("shadow_buy_total").unwrap_or(&0),
        shadow_buy_success_total: *guard.get("shadow_buy_success_total").unwrap_or(&0),
        shadow_buy_failure_total: *guard.get("shadow_buy_failure_total").unwrap_or(&0),
        shadow_buy_retry_total: *guard.get("shadow_buy_retry_total").unwrap_or(&0),
        shadow_buy_queue_overflow_total: *guard
            .get("shadow_buy_queue_overflow_total")
            .unwrap_or(&0),
    }
}

#[async_trait]
impl ShadowSimulator for RpcShadowSimulator {
    async fn simulate_buy(
        &self,
        request: &PreparedBuyRequest,
        config: &TriggerShadowRunConfig,
    ) -> Result<ShadowBuySimulationReport> {
        if !config.enabled {
            return Err(anyhow!(
                "trigger.shadow_run.enabled=false but shadow entry mode requested"
            ));
        }

        let rpc = new_async_rpc_client(config.shadow_rpc_url.clone());
        let started_ts_ms = Self::now_ms();
        let mut retry_count = 0usize;

        loop {
            let sim_config = RpcSimulateTransactionConfig {
                sig_verify: config.sig_verify,
                replace_recent_blockhash: config.replace_recent_blockhash,
                commitment: Some(Self::commitment(config.commitment)),
                accounts: Some(RpcSimulateTransactionAccountsConfig {
                    encoding: Some(UiAccountEncoding::Base64),
                    addresses: vec![request.user_ata.to_string()],
                }),
                ..RpcSimulateTransactionConfig::default()
            };

            let simulation_result = tokio::time::timeout(
                std::time::Duration::from_millis(config.timeout_ms),
                rpc.simulate_transaction_with_config(&request.rpc_buy_tx, sim_config),
            )
            .await;

            match simulation_result {
                Ok(Ok(result)) => {
                    let rpc_slot = result.context.slot;
                    let result = result.value;
                    let finished_ts_ms = Self::now_ms();
                    let latency_ms = finished_ts_ms.saturating_sub(started_ts_ms);
                    let entry_token_amount_raw = match resolve_simulated_entry_token_amount_raw(
                        request,
                        result.accounts.as_ref(),
                    ) {
                        Ok(Some(amount_raw)) => Some(amount_raw),
                        Ok(None) => {
                            increment_counter!(
                                "shadow_buy_simulated_qty_fallback_total",
                                "reason" => "missing_simulated_account"
                            );
                            request.entry_token_amount_raw
                        }
                        Err(error) => {
                            increment_counter!(
                                "shadow_buy_simulated_qty_fallback_total",
                                "reason" => "invalid_simulated_account"
                            );
                            warn!(
                                user_ata = %request.user_ata,
                                error = %error,
                                fallback_entry_token_amount_raw = ?request.entry_token_amount_raw,
                                "Shadow buy simulation could not derive authoritative token delta from simulated account state; falling back to prepared quote quantity"
                            );
                            request.entry_token_amount_raw
                        }
                    };
                    return Ok(ShadowBuySimulationReport {
                        join_metadata: request.join_metadata.clone(),
                        mint: request.mint.to_string(),
                        live_signature: None,
                        payer_pubkey: request.payer_pubkey.to_string(),
                        payer_provenance: request.payer_provenance.to_string(),
                        amount_lamports: request.amount_lamports,
                        entry_token_amount_raw,
                        tip_lamports: request.tip_lamports,
                        decision_ts_ms: request.decision_ts_ms,
                        simulation_started_ts_ms: started_ts_ms,
                        simulation_finished_ts_ms: finished_ts_ms,
                        latency_ms,
                        shadow_duration_ms: latency_ms,
                        rpc_slot,
                        retry_count,
                        used_sig_verify: config.sig_verify,
                        used_replace_recent_blockhash: config.replace_recent_blockhash,
                        units_consumed: result.units_consumed,
                        logs: result.logs.unwrap_or_default(),
                        return_data: result.return_data.map(|data| format!("{:?}", data)),
                        err: result.err.map(|err| format!("{:?}", err)),
                    });
                }
                Ok(Err(err)) => {
                    if retry_count < config.max_retries && Self::is_retryable(&err.to_string()) {
                        retry_count += 1;
                        continue;
                    }
                    return Err(ShadowSimulationError::new(
                        format!("shadow RPC simulate failed: {}", err),
                        retry_count,
                    )
                    .into());
                }
                Err(_) => {
                    if retry_count < config.max_retries {
                        retry_count += 1;
                        continue;
                    }
                    return Err(ShadowSimulationError::new(
                        format!(
                            "shadow RPC simulate timed out after {}ms",
                            config.timeout_ms
                        ),
                        retry_count,
                    )
                    .into());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use solana_account_decoder::UiAccountData;
    use solana_sdk::program_option::COption;
    use solana_sdk::signer::Signer;
    use spl_token_2022::state::AccountState;

    fn sample_event(live_signature: Option<&str>, err: Option<&str>) -> ShadowBuySimulationEvent {
        ShadowBuySimulationEvent {
            join_metadata: ExecutionJoinMetadata::default(),
            account_diagnostics: ShadowSimulationAccountDiagnostics::default(),
            candidate_id: build_execution_candidate_id(
                "mint",
                "pool",
                live_signature.unwrap_or("1000"),
            ),
            pool_amm_id: "pool".to_string(),
            base_mint: "mint".to_string(),
            mint: "mint".to_string(),
            live_signature: live_signature.map(str::to_string),
            payer_pubkey: "payer".to_string(),
            payer_provenance: "configured".to_string(),
            amount_lamports: 100,
            entry_token_amount_raw: Some(250_000),
            tip_lamports: 10,
            decision_ts_ms: 1000,
            simulation_started_ts_ms: 1010,
            simulation_finished_ts_ms: 1035,
            latency_ms: 25,
            shadow_duration_ms: 25,
            rpc_slot: 777,
            retry_count: 2,
            used_sig_verify: false,
            used_replace_recent_blockhash: true,
            units_consumed: Some(42_000),
            logs: vec!["a".to_string(), "b".to_string()],
            return_data: None,
            err: err.map(str::to_string),
            error_class: None,
            error_code: None,
            error_detail_class: None,
        }
    }

    fn sample_prepared_buy_request(
        pre_submit_token_balance: Option<u64>,
        ata_missing_pre_submit: bool,
    ) -> PreparedBuyRequest {
        let payer = solana_sdk::signature::Keypair::new();
        let recent_blockhash = solana_sdk::hash::Hash::new_unique();
        let transfer_ix = solana_sdk::system_instruction::transfer(
            &payer.pubkey(),
            &solana_sdk::pubkey::Pubkey::new_unique(),
            1,
        );
        PreparedBuyRequest {
            join_metadata: ExecutionJoinMetadata::default(),
            mint: solana_sdk::pubkey::Pubkey::new_unique(),
            payer_pubkey: payer.pubkey(),
            payer_provenance: "configured",
            user_ata: solana_sdk::pubkey::Pubkey::new_unique(),
            token_program: solana_sdk::pubkey::Pubkey::new_unique(),
            attach_idempotent_ata_create: true,
            ata_missing_pre_submit,
            account_overrides: crate::components::trigger::BuyAccountOverrides::default(),
            pre_submit_token_balance,
            amount_lamports: 100,
            trade_value_sol: 0.1,
            entry_token_amount_raw: Some(250_000),
            tip_lamports: 10,
            min_tokens_out: 1,
            priority_fee_micro_lamports:
                crate::components::live_tx_sender::HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
            recent_blockhash,
            blockhash_source: "test",
            blockhash_age_ms: 0,
            blockhash_last_valid_block_height: 0,
            blockhash_observed_block_height: 0,
            blockhash_fetched_at: std::time::Instant::now(),
            blockhash_fetch_latency_ms: 0,
            post_blockhash_build_latency_ms: 0,
            reserve_slot_latency_ms: 0,
            shadow_spawn_latency_ms: 0,
            preparation_telemetry: Default::default(),
            build_profile: None,
            rpc_buy_tx: solana_sdk::transaction::Transaction::new_signed_with_payer(
                std::slice::from_ref(&transfer_ix),
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            ),
            buy_tx: solana_sdk::transaction::VersionedTransaction::try_new(
                solana_sdk::message::VersionedMessage::V0(
                    solana_sdk::message::v0::Message::try_compile(
                        &payer.pubkey(),
                        &[transfer_ix],
                        &[],
                        recent_blockhash,
                    )
                    .expect("message"),
                ),
                &[&payer],
            )
            .expect("buy tx"),
            tip_tx: None,
            decision_ts_ms: 10,
        }
    }

    fn sample_simulated_ui_account(amount: u64) -> UiAccount {
        let account = SplTokenAccount {
            mint: solana_sdk::pubkey::Pubkey::new_unique(),
            owner: solana_sdk::pubkey::Pubkey::new_unique(),
            amount,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };
        let mut data = vec![0u8; SplTokenAccount::LEN];
        SplTokenAccount::pack(account, &mut data).expect("pack token account");
        UiAccount {
            lamports: 1,
            data: UiAccountData::Binary(
                base64::engine::general_purpose::STANDARD.encode(data),
                UiAccountEncoding::Base64,
            ),
            owner: spl_token_2022::id().to_string(),
            executable: false,
            rent_epoch: 0,
            space: Some(SplTokenAccount::LEN as u64),
        }
    }

    #[test]
    fn shadow_jsonl_record_serializes_stably() {
        let record = ShadowBuySimulationRecord::from_event(
            TriggerEntryMode::LiveAndShadow,
            &sample_event(Some("sig"), None),
        );
        let value = serde_json::to_value(&record).expect("serialize shadow record");
        assert_eq!(value["candidate_id"], "mint_pool_sig");
        assert_eq!(value["entry_mode"], "live_and_shadow");
        assert_eq!(value["pool_amm_id"], "pool");
        assert_eq!(value["live_signature"], "sig");
        assert_eq!(value["decision_to_sim_start_ms"], 10);
        assert_eq!(value["shadow_duration_ms"], 25);
        assert_eq!(value["retry_count"], 2);
        assert!(value["logs_digest"].as_str().unwrap_or_default().len() > 10);
        assert_eq!(value["logs_excerpt"][0], "a");
        assert_eq!(value["logs_excerpt"][1], "b");
    }

    #[tokio::test]
    async fn shadow_jsonl_writer_creates_missing_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let nested = temp.path().join("missing").join("shadow.jsonl");
        let record = ShadowBuySimulationRecord::from_event(
            TriggerEntryMode::ShadowOnly,
            &sample_event(None, None),
        );

        append_shadow_buy_record(&nested, &record)
            .await
            .expect("write shadow record");

        let contents = tokio::fs::read_to_string(&nested)
            .await
            .expect("read shadow jsonl");
        assert!(contents.contains("\"entry_mode\":\"shadow_only\""));
    }

    #[test]
    fn p5_idempotency_key_uses_join_key_and_rollout_profile() {
        let first = make_shadow_idempotency_key("pool", "pool:mint:1000", "shadow-burnin");
        let same = make_shadow_idempotency_key("pool", "pool:mint:1000", "shadow-burnin");
        let different_join = make_shadow_idempotency_key("pool", "pool:mint:1001", "shadow-burnin");
        let different_rollout = make_shadow_idempotency_key("pool", "pool:mint:1000", "canary");

        assert_eq!(first, same);
        assert_ne!(first, different_join);
        assert_ne!(first, different_rollout);
    }

    #[tokio::test]
    async fn p5_failed_dispatch_writes_terminal_lifecycle_record() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lifecycle_path = temp.path().join("shadow_lifecycle.jsonl");
        let join_key = make_shadow_join_key("pool", "mint", 1000);
        let record = ShadowBuySimulationRecord::from_event(
            TriggerEntryMode::ShadowOnly,
            &sample_event(None, Some("custom program error: 0x1")),
        )
        .with_lifecycle_identity(join_key.clone(), "shadow-burnin");
        let lifecycle_record = ShadowDispatchLifecycleRecord::failed_from_shadow_buy_record(
            &record,
            join_key,
            "shadow-burnin",
        );

        append_shadow_dispatch_lifecycle_record(&lifecycle_path, &lifecycle_record)
            .await
            .expect("write lifecycle record");

        let contents = tokio::fs::read_to_string(&lifecycle_path)
            .await
            .expect("read lifecycle jsonl");
        let row: serde_json::Value =
            serde_json::from_str(contents.trim()).expect("parse lifecycle json");
        assert_eq!(row["record_type"], "shadow_dispatch");
        assert_eq!(row["dispatch_status"], "failed");
        assert_eq!(row["idempotency_key"], record.idempotency_key.unwrap());
        assert_eq!(row["join_key"], "pool:mint:1000");
        assert_eq!(row["rollout_profile"], "shadow-burnin");
        assert_eq!(row["classification"], "simulation_mismatch");
    }

    #[test]
    fn p5_terminal_lifecycle_marks_report_errors_as_failed() {
        let join_key = make_shadow_join_key("pool", "mint", 1000);
        let record = ShadowBuySimulationRecord::from_event(
            TriggerEntryMode::ShadowOnly,
            &sample_event(
                None,
                Some("ConstraintSeeds: InstructionError(3, Custom(2006))"),
            ),
        )
        .with_lifecycle_identity(join_key.clone(), "shadow-burnin");

        let lifecycle_record = ShadowDispatchLifecycleRecord::terminal_from_shadow_buy_record(
            &record,
            join_key,
            "shadow-burnin",
        );

        assert_eq!(
            lifecycle_record.dispatch_status,
            ShadowDispatchStatus::Failed
        );
        assert_eq!(
            lifecycle_record.classification,
            "seed_mismatch_constraint_seeds"
        );
    }

    #[test]
    fn p5_terminal_lifecycle_marks_success_reports_as_closed() {
        let join_key = make_shadow_join_key("pool", "mint", 1000);
        let record = ShadowBuySimulationRecord::from_event(
            TriggerEntryMode::ShadowOnly,
            &sample_event(None, None),
        )
        .with_lifecycle_identity(join_key.clone(), "shadow-burnin");

        let lifecycle_record = ShadowDispatchLifecycleRecord::terminal_from_shadow_buy_record(
            &record,
            join_key,
            "shadow-burnin",
        );

        assert_eq!(
            lifecycle_record.dispatch_status,
            ShadowDispatchStatus::Closed
        );
        assert_eq!(lifecycle_record.classification, "simulation_completed");
    }

    #[test]
    fn shadow_metrics_increment_for_success_and_failure() {
        reset_shadow_metrics_test_snapshot();
        record_shadow_buy_metrics(&ShadowBuySimulationRecord::from_event(
            TriggerEntryMode::LiveAndShadow,
            &sample_event(Some("sig"), None),
        ));
        record_shadow_buy_metrics(&ShadowBuySimulationRecord::from_event(
            TriggerEntryMode::ShadowOnly,
            &sample_event(None, Some("custom program error: 0x1")),
        ));
        record_shadow_buy_queue_overflow(TriggerEntryMode::LiveAndShadow);

        let snapshot = shadow_metrics_test_snapshot();
        assert_eq!(snapshot.shadow_buy_total, 2);
        assert_eq!(snapshot.shadow_buy_success_total, 1);
        assert_eq!(snapshot.shadow_buy_failure_total, 1);
        assert_eq!(snapshot.shadow_buy_retry_total, 4);
        assert_eq!(snapshot.shadow_buy_queue_overflow_total, 1);
    }

    #[test]
    fn resolve_simulated_entry_token_amount_raw_uses_post_balance_delta() {
        let request = sample_prepared_buy_request(Some(120), false);
        let simulated_account = sample_simulated_ui_account(420);
        let resolved = resolve_simulated_entry_token_amount_raw(
            &request,
            Some(&vec![Some(simulated_account)]),
        )
        .expect("resolved simulated qty");

        assert_eq!(resolved, Some(300));
    }

    #[test]
    fn resolve_simulated_entry_token_amount_raw_requires_known_pre_balance_for_existing_ata() {
        let request = sample_prepared_buy_request(None, false);
        let simulated_account = sample_simulated_ui_account(420);
        let error = resolve_simulated_entry_token_amount_raw(
            &request,
            Some(&vec![Some(simulated_account)]),
        )
        .expect_err("missing pre-submit balance should fail");

        assert!(
            error
                .to_string()
                .contains("missing pre-submit token balance"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn shadow_jsonl_contains_live_signature_only_when_live_exists() {
        let with_live = ShadowBuySimulationRecord::from_event(
            TriggerEntryMode::LiveAndShadow,
            &sample_event(Some("sig"), None),
        );
        let without_live = ShadowBuySimulationRecord::from_event(
            TriggerEntryMode::ShadowOnly,
            &sample_event(None, None),
        );

        assert_eq!(with_live.live_signature.as_deref(), Some("sig"));
        assert_eq!(without_live.live_signature, None);
    }

    #[test]
    fn shadow_failure_preserves_retry_count_in_record() {
        let payer = solana_sdk::signature::Keypair::new();
        let recent_blockhash = solana_sdk::hash::Hash::new_unique();
        let transfer_ix = solana_sdk::system_instruction::transfer(
            &payer.pubkey(),
            &solana_sdk::pubkey::Pubkey::new_unique(),
            1,
        );
        let request = PreparedBuyRequest {
            join_metadata: ExecutionJoinMetadata::default(),
            mint: solana_sdk::pubkey::Pubkey::new_unique(),
            payer_pubkey: payer.pubkey(),
            payer_provenance: "configured",
            user_ata: solana_sdk::pubkey::Pubkey::new_unique(),
            token_program: solana_sdk::pubkey::Pubkey::new_unique(),
            attach_idempotent_ata_create: true,
            ata_missing_pre_submit: false,
            account_overrides: crate::components::trigger::BuyAccountOverrides::default(),
            pre_submit_token_balance: Some(0),
            amount_lamports: 100,
            trade_value_sol: 0.1,
            entry_token_amount_raw: Some(250_000),
            tip_lamports: 10,
            min_tokens_out: 1,
            priority_fee_micro_lamports:
                crate::components::live_tx_sender::HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
            recent_blockhash,
            blockhash_source: "test",
            blockhash_age_ms: 0,
            blockhash_last_valid_block_height: 0,
            blockhash_observed_block_height: 0,
            blockhash_fetched_at: std::time::Instant::now(),
            blockhash_fetch_latency_ms: 0,
            post_blockhash_build_latency_ms: 0,
            reserve_slot_latency_ms: 0,
            shadow_spawn_latency_ms: 0,
            preparation_telemetry: Default::default(),
            build_profile: None,
            rpc_buy_tx: solana_sdk::transaction::Transaction::new_signed_with_payer(
                std::slice::from_ref(&transfer_ix),
                Some(&payer.pubkey()),
                &[&payer],
                recent_blockhash,
            ),
            buy_tx: solana_sdk::transaction::VersionedTransaction::try_new(
                solana_sdk::message::VersionedMessage::V0(
                    solana_sdk::message::v0::Message::try_compile(
                        &payer.pubkey(),
                        &[transfer_ix],
                        &[],
                        recent_blockhash,
                    )
                    .expect("message"),
                ),
                &[&payer],
            )
            .expect("buy tx"),
            tip_tx: None,
            decision_ts_ms: 10,
        };
        let err = anyhow!(ShadowSimulationError::new("shadow failed", 2));

        let record = ShadowBuySimulationRecord::from_failure(
            TriggerEntryMode::ShadowOnly,
            "pool",
            "mint",
            &request,
            None,
            &err,
        );

        assert_eq!(record.candidate_id, "mint_pool_10");
        assert_eq!(record.retry_count, 2);
    }

    #[test]
    fn classify_shadow_error_detail_extracts_known_custom_codes() {
        let err_2006 = "InstructionError(3, Custom(2006))";
        let err_6000 = "AnchorError occurred. Error Code: NotAuthorized. Error Number: 6000.";
        let err_6062 = "InstructionError(3, Custom(6062))";

        assert_eq!(extract_shadow_error_code(err_2006), Some("2006"));
        assert_eq!(
            classify_shadow_error_detail(err_2006),
            Some("seed_mismatch_constraint_seeds")
        );
        assert_eq!(extract_shadow_error_code(err_6000), Some("6000"));
        assert_eq!(
            classify_shadow_error_detail(err_6000),
            Some("protocol_not_authorized")
        );
        assert_eq!(extract_shadow_error_code(err_6062), Some("6062"));
        assert_eq!(
            classify_shadow_error_detail(err_6062),
            Some("buyback_fee_recipient_missing")
        );
        let err_6024 = "InstructionError(3, Custom(6024))";
        assert_eq!(extract_shadow_error_code(err_6024), Some("6024"));
        assert_eq!(
            classify_shadow_error_detail(err_6024),
            Some("legacy_buy_amount_overflow")
        );
    }

    #[test]
    fn summarize_logs_keeps_head_and_tail_for_large_log_sets() {
        let logs = (0..20).map(|idx| format!("log-{idx}")).collect::<Vec<_>>();
        let excerpt = summarize_logs(&logs);

        assert_eq!(excerpt.len(), 13);
        assert_eq!(excerpt[0], "log-0");
        assert_eq!(excerpt[5], "log-5");
        assert_eq!(excerpt[6], "... 8 log lines omitted ...");
        assert_eq!(excerpt[7], "log-14");
        assert_eq!(excerpt[12], "log-19");
    }

    #[test]
    fn classify_shadow_error_marks_invalid_account_for_fee_as_authority_problem() {
        assert_eq!(
            classify_shadow_error("InvalidAccountForFee"),
            "authority_problem"
        );
        assert_eq!(
            classify_shadow_error("transaction simulation failed: Invalid account for fee"),
            "authority_problem"
        );
    }

    #[test]
    fn classify_shadow_error_marks_bulkhead_balance_rejections_as_fee_compute_problem() {
        assert_eq!(
            classify_shadow_error("Balance critical: 0.007327349 SOL < 0.008 SOL emergency floor"),
            "fee_compute_problem"
        );
        assert_eq!(
            classify_shadow_error(
                "No safe trade capacity: balance=0.05 SOL, required_reserve=0.07 SOL"
            ),
            "fee_compute_problem"
        );
        assert_eq!(
            classify_shadow_error(
                "Insufficient safe balance: available=0.06 SOL, required=0.07 SOL"
            ),
            "fee_compute_problem"
        );
    }

    #[test]
    fn shadow_failure_preserves_prepare_retry_count_in_record() {
        let err = anyhow!(ShadowPreparationError::new("mint fetch failed", 4));
        let context = TriggerDispatchFailureContext {
            join_metadata: ExecutionJoinMetadata::default(),
            amount_lamports: 100,
            tip_lamports: 10,
            decision_ts_ms: 10,
            payer_provenance: "configured",
            payer_pubkey: Some("payer-configured".to_string()),
        };

        let record = ShadowBuySimulationRecord::from_failure_context(
            TriggerEntryMode::ShadowOnly,
            "pool",
            "mint",
            &context,
            None,
            &err,
        );

        assert_eq!(record.candidate_id, "mint_pool_10");
        assert_eq!(record.retry_count, 4);
        assert_eq!(record.error_class.as_deref(), Some("data_problem"));
        assert_eq!(record.payer_provenance, "configured");
        assert_eq!(record.payer_pubkey.as_deref(), Some("payer-configured"));
    }

    #[test]
    fn shadow_failure_context_builds_record() {
        let err = anyhow!("preflight failed");
        let context = TriggerDispatchFailureContext {
            join_metadata: ExecutionJoinMetadata::default(),
            amount_lamports: 100,
            tip_lamports: 10,
            decision_ts_ms: 10,
            payer_provenance: "ephemeral",
            payer_pubkey: Some("payer-ephemeral".to_string()),
        };

        let record = ShadowBuySimulationRecord::from_failure_context(
            TriggerEntryMode::ShadowOnly,
            "pool",
            "mint",
            &context,
            None,
            &err,
        );

        assert_eq!(record.candidate_id, "mint_pool_10");
        assert_eq!(record.amount_lamports, 100);
        assert_eq!(record.tip_lamports, 10);
        assert_eq!(record.decision_ts_ms, 10);
        assert_eq!(record.error_class.as_deref(), Some("unclassified"));
        assert_eq!(record.payer_provenance, "ephemeral");
        assert_eq!(record.payer_pubkey.as_deref(), Some("payer-ephemeral"));
    }

    #[test]
    fn shadow_failure_event_carries_full_diagnostics_and_payer_pubkey() {
        let err = anyhow!("InstructionError(3, Custom(2006))");
        let context = TriggerDispatchFailureContext {
            join_metadata: ExecutionJoinMetadata::default(),
            amount_lamports: 100,
            tip_lamports: 10,
            decision_ts_ms: 10,
            payer_provenance: "configured",
            payer_pubkey: Some("payer-configured".to_string()),
        };

        let event = shadow_failure_event_from_context(
            "pool",
            "mint",
            &context,
            "mint".to_string(),
            None,
            &err,
        );

        assert_eq!(event.payer_pubkey, "payer-configured");
        assert_eq!(
            event.error_class.as_deref(),
            Some(classify_shadow_error("InstructionError(3, Custom(2006))"))
        );
        assert_eq!(event.error_code.as_deref(), Some("2006"));
        assert_eq!(
            event.error_detail_class.as_deref(),
            Some("seed_mismatch_constraint_seeds")
        );
    }

    #[test]
    fn shadow_join_metadata_flows_from_request_to_transport_and_dispatch_records() {
        let pool_id = "pool";
        let base_mint = "mint";
        let decision_ts_ms = 10;
        let candidate_id =
            build_execution_candidate_id(base_mint, pool_id, decision_ts_ms.to_string());
        let metadata = ExecutionJoinMetadata {
            ab_record_id: Some("pool:1000:11000:BUY".to_string()),
            v3_feature_snapshot_hash: Some("feature-hash-j2b".to_string()),
            v3_policy_config_hash: Some("policy-hash-j2b".to_string()),
            decision_plane: Some("legacy_live".to_string()),
            rollout_namespace: Some("r14-j2b-harness".to_string()),
            ..Default::default()
        };
        let request =
            sample_prepared_buy_request(Some(0), true).with_join_metadata(metadata.clone());
        let err = anyhow!("preflight failed");

        let event = shadow_failure_event_from_request(pool_id, base_mint, &request, None, &err);
        assert_eq!(event.join_metadata, metadata);
        assert_eq!(event.candidate_id, candidate_id);
        assert_eq!(event.pool_amm_id, pool_id);
        assert_eq!(event.base_mint, base_mint);
        assert_eq!(event.decision_ts_ms, decision_ts_ms);

        let record = ShadowBuySimulationRecord::from_event(TriggerEntryMode::ShadowOnly, &event);
        assert_eq!(record.join_metadata, metadata);
        assert_eq!(record.candidate_id, candidate_id);
        assert_eq!(record.pool_amm_id, pool_id);
        assert_eq!(record.base_mint, base_mint);
        assert_eq!(record.decision_ts_ms, decision_ts_ms);
        let transport_row = serde_json::to_value(&record).expect("serialize shadow transport row");
        assert_eq!(transport_row["ab_record_id"], "pool:1000:11000:BUY");
        assert_eq!(transport_row["candidate_id"], candidate_id);
        assert_eq!(transport_row["pool_amm_id"], pool_id);
        assert_eq!(transport_row["base_mint"], base_mint);
        assert_eq!(transport_row["decision_ts_ms"], decision_ts_ms);
        assert_eq!(
            transport_row["v3_feature_snapshot_hash"],
            "feature-hash-j2b"
        );
        assert_eq!(transport_row["v3_policy_config_hash"], "policy-hash-j2b");
        assert_eq!(transport_row["decision_plane"], "legacy_live");
        assert_eq!(transport_row["rollout_namespace"], "r14-j2b-harness");

        let dispatch = ShadowDispatchLifecycleRecord::failed_from_shadow_buy_record(
            &record,
            "join-key",
            "r14-j2b-harness",
        );
        assert_eq!(dispatch.join_metadata, metadata);
        let dispatch_row =
            serde_json::to_value(&dispatch).expect("serialize shadow dispatch lifecycle row");
        assert_eq!(dispatch_row["ab_record_id"], "pool:1000:11000:BUY");
        assert_eq!(dispatch_row["candidate_id"], candidate_id);
        assert_eq!(dispatch_row["pool_id"], pool_id);
        assert_eq!(dispatch_row["mint_id"], base_mint);
        assert_eq!(dispatch_row["decision_ts_ms"], decision_ts_ms);
        assert_eq!(dispatch_row["v3_feature_snapshot_hash"], "feature-hash-j2b");
        assert_eq!(dispatch_row["v3_policy_config_hash"], "policy-hash-j2b");
        assert_eq!(dispatch_row["decision_plane"], "legacy_live");
        assert_eq!(dispatch_row["rollout_namespace"], "r14-j2b-harness");
    }

    #[test]
    fn legacy_shadow_transport_without_join_metadata_still_parses() {
        let row = r#"{
            "candidate_id":"mint_pool_10",
            "pool_amm_id":"pool",
            "base_mint":"mint",
            "entry_mode":"shadow_only",
            "decision_ts_ms":10,
            "sim_started_ts_ms":10,
            "sim_finished_ts_ms":12,
            "decision_to_sim_start_ms":0,
            "shadow_duration_ms":2,
            "amount_lamports":100,
            "tip_lamports":10,
            "payer_provenance":"configured",
            "err":null,
            "error_class":null,
            "units_consumed":null,
            "rpc_slot":null,
            "retry_count":0,
            "live_signature":null,
            "logs_digest":"legacy",
            "logs_excerpt":[]
        }"#;

        let record: ShadowBuySimulationRecord =
            serde_json::from_str(row).expect("legacy shadow transport parses");
        assert_eq!(record.join_metadata, ExecutionJoinMetadata::default());
        assert_eq!(record.candidate_id, "mint_pool_10");
    }
}
