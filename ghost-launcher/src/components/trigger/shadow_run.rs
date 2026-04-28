use crate::components::trigger::component::PreparedBuyRequest;
use crate::components::trigger::component::TriggerDispatchFailureContext;
use crate::config::{ShadowRunCommitment, TriggerEntryMode, TriggerShadowRunConfig};
use crate::events::{build_execution_candidate_id, ShadowBuySimulationEvent};
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShadowBuySimulationReport {
    pub mint: String,
    pub live_signature: Option<String>,
    pub payer_pubkey: String,
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
    pub err: Option<String>,
    pub error_class: Option<String>,
    pub units_consumed: Option<u64>,
    pub rpc_slot: Option<u64>,
    pub retry_count: usize,
    pub live_signature: Option<String>,
    pub logs_digest: String,
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
    pub fn from_event(
        entry_mode: TriggerEntryMode,
        event: &ShadowBuySimulationEvent,
    ) -> ShadowBuySimulationRecord {
        ShadowBuySimulationRecord {
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
            err: event.err.clone(),
            error_class: event
                .err
                .as_deref()
                .map(classify_shadow_error)
                .map(str::to_string),
            units_consumed: event.units_consumed,
            rpc_slot: Some(event.rpc_slot),
            retry_count: event.retry_count,
            live_signature: event.live_signature.clone(),
            logs_digest: logs_digest(&event.logs),
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
        ShadowBuySimulationRecord {
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
            err: Some(err_string.clone()),
            error_class: Some(classify_shadow_error(&err_string).to_string()),
            units_consumed: None,
            rpc_slot: None,
            retry_count: shadow_error_retry_count(err),
            live_signature,
            logs_digest: logs_digest(&[]),
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
        ShadowBuySimulationRecord {
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
            err: Some(err_string.clone()),
            error_class: Some(classify_shadow_error(&err_string).to_string()),
            units_consumed: None,
            rpc_slot: None,
            retry_count: shadow_error_retry_count(err),
            live_signature,
            logs_digest: logs_digest(&[]),
        }
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

pub fn classify_shadow_error(err: &str) -> &'static str {
    let lower = err.to_lowercase();
    if RpcShadowSimulator::is_retryable(&lower)
        || lower.contains("rpc")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("accountnotfound")
        || lower.contains("failed to fetch mint account")
        || lower.contains("too many requests")
        || lower.contains("join error")
        || lower.contains("semaphore")
    {
        "transport"
    } else if lower.contains("custom program error")
        || lower.contains("insufficient funds")
        || lower.contains("insufficientfundsforrent")
        || lower.contains("insufficient payer balance")
        || lower.contains("balance critical")
        || lower.contains("no safe trade capacity")
        || lower.contains("insufficient safe balance")
        || lower.contains("invalidaccountforfee")
        || lower.contains("invalid account for fee")
        || lower.contains("instructionerror")
        || lower.contains("simulation error")
    {
        "semantic"
    } else {
        "unknown"
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
                        mint: request.mint.to_string(),
                        live_signature: None,
                        payer_pubkey: request.payer_pubkey.to_string(),
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
            mint: solana_sdk::pubkey::Pubkey::new_unique(),
            payer_pubkey: payer.pubkey(),
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
            mint: solana_sdk::pubkey::Pubkey::new_unique(),
            payer_pubkey: payer.pubkey(),
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
    fn classify_shadow_error_marks_invalid_account_for_fee_as_semantic() {
        assert_eq!(classify_shadow_error("InvalidAccountForFee"), "semantic");
        assert_eq!(
            classify_shadow_error("transaction simulation failed: Invalid account for fee"),
            "semantic"
        );
    }

    #[test]
    fn classify_shadow_error_marks_bulkhead_balance_rejections_as_semantic() {
        assert_eq!(
            classify_shadow_error("Balance critical: 0.007327349 SOL < 0.008 SOL emergency floor"),
            "semantic"
        );
        assert_eq!(
            classify_shadow_error(
                "No safe trade capacity: balance=0.05 SOL, required_reserve=0.07 SOL"
            ),
            "semantic"
        );
        assert_eq!(
            classify_shadow_error(
                "Insufficient safe balance: available=0.06 SOL, required=0.07 SOL"
            ),
            "semantic"
        );
    }

    #[test]
    fn shadow_failure_preserves_prepare_retry_count_in_record() {
        let err = anyhow!(ShadowPreparationError::new("mint fetch failed", 4));
        let context = TriggerDispatchFailureContext {
            amount_lamports: 100,
            tip_lamports: 10,
            decision_ts_ms: 10,
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
    }

    #[test]
    fn shadow_failure_context_builds_record() {
        let err = anyhow!("preflight failed");
        let context = TriggerDispatchFailureContext {
            amount_lamports: 100,
            tip_lamports: 10,
            decision_ts_ms: 10,
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
        assert_eq!(record.error_class.as_deref(), Some("unknown"));
    }
}
