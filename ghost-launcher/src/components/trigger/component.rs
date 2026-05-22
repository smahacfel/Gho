//! Trigger component wrapper
//!
//! This module integrates the IPC event processor with the gatekeeper logic
//! to process DetectedPoolEvents from Seer with position limit checks.
//! Now also receives events from the unified event bus.
//!
//! ## Oracle Integration
//!
//! All NewPoolDetected events are now filtered through the Oracle pipeline
//! before being processed. The pipeline runs multiple scoring components
//! in parallel async tasks:
//! - SimpleOracle: Basic risk threshold scoring
//! - score_enhanced(): Shadow Ledger integration
//! - QASS: Quantum-style amplitude superposition scoring
//! - HyperOracle: SCR/ULVF/POVC for T+2s analysis

use super::safety::{
    resolve_safe_trade_amount, ActivePositionLease, PositionLimitTracker, PositionSlotId,
    SafetyConfig, SafetyViolation,
};
use super::shadow_run::{
    RpcShadowSimulator, ShadowPreparationError, ShadowSimulator, TriggerBuyOutcome,
};
use super::tip_guard::{calculate_safe_tip, TipGuardConfig};
use crate::components::live_tx_sender::{
    select_sender_tip_account, BuyTipResolution, LiveTxSender, LiveTxSenderError,
    PriorityFeeCacheKey, PriorityFeeEstimate, TipFloorResolutionTelemetry,
    HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS, HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS,
    HELIUS_SENDER_MIN_TIP_LAMPORTS,
};
use crate::components::oracle_pipeline::OraclePipeline;
use crate::config::{
    OracleConfig, TriggerComponentConfig, TriggerEntryMode, TriggerShadowPayerStrategy,
};
use crate::events::{
    EventBusReceiver, EventBusSender, ExecutionJoinMetadata, GhostEvent, LegacyPathClassification,
    LegacyPathDescriptor, RuntimePlane,
};
use anyhow::{bail, Result};
use ghost_core::{
    account_state_core::reducer::AccountStateReducer,
    market_state::{BondingCurve, ShadowBondingCurve},
    shadow_ledger::{apply_slippage_bps, ShadowLedger},
};
use metrics::gauge;
use seer::new_async_rpc_client;
use serde::Deserialize;
use solana_client::client_error::{ClientError, ClientErrorKind};
use solana_client::rpc_request::RpcError;
use solana_sdk::account::Account;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::Instruction;
use solana_sdk::message::{v0, VersionedMessage};
use solana_sdk::program_pack::Pack;
use solana_sdk::signature::{read_keypair_file, Keypair};
use solana_sdk::signer::Signer;
use solana_sdk::transaction::{Transaction, VersionedTransaction};
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use solana_sdk::{system_instruction, system_program};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use spl_token_2022::extension::ExtensionType;
use spl_token_2022::state::Account as SplTokenAccount;
use std::collections::{HashMap, HashSet}; // ✅ ADDED: Required for caching pools
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, Semaphore};
use tracing::{debug, info, warn};
use trigger::direct_buy_builder::{TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID};
use trigger::DirectBuyBuilder;

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
const BUY_BALANCE_FEE_BUFFER_LAMPORTS: u64 = 50_000;
const BUY_COMPUTE_UNIT_LIMIT: u32 = 400_000;
const BUY_BLOCKHASH_CACHE_REFRESH_MS: u64 = 350;
const BUY_BLOCKHASH_CACHE_MAX_AGE_MS: u64 = 800;
const BUY_CONFIRM_POLL_MS: u64 = 150;
const BUY_CONFIRM_PRIMARY_TIMEOUT_MS: u64 = 1_800;
const BUY_RETRY_INITIAL_CONFIRM_WAIT_MS: u64 = 250;
const BUY_RETRY_RESEND_CONFIRM_WAIT_MS: u64 = 300;
const BUY_RETRY_MAX_ATTEMPTS: usize = 3;
const BUY_RETRY_PRIORITY_FEE_INCREMENT_MICRO_LAMPORTS: u64 = 10_000;
const BUY_RETRY_TIP_INCREMENT_LAMPORTS: u64 = 300_000;
const KNOWN_BAD_LEGACY_FEE_RECIPIENT: &str = "CebN5WGQ4jvEPvsVU4EoHEpgznyQQNDGNesDwrFs8YWj";
const TRIGGER_POOL_SCORED_OBSERVER_PATH: LegacyPathDescriptor = LegacyPathDescriptor::new(
    "trigger_pool_scored_observer",
    LegacyPathClassification::ObservabilityOnly,
    RuntimePlane::LegacyObservation,
    false,
    Some("2026-04-30"),
);
const TRIGGER_EMBEDDED_ORACLE_PIPELINE_PATH: LegacyPathDescriptor = LegacyPathDescriptor::new(
    "trigger_embedded_oracle_pipeline",
    LegacyPathClassification::CompatibilityOnly,
    RuntimePlane::LegacyObservation,
    false,
    None,
);
const TRIGGER_NO_EVENT_BUS_PATH: LegacyPathDescriptor = LegacyPathDescriptor::new(
    "trigger_no_event_bus_fallback",
    LegacyPathClassification::DisabledInProduction,
    RuntimePlane::LegacyObservation,
    false,
    None,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LegacyPoolScoredHandling {
    blocked_side_effect: bool,
    removed_pending_pool: bool,
}

fn record_legacy_path_event(path: &LegacyPathDescriptor) {
    metrics::counter!("legacy_path_event_total", 1u64, "path" => path.path);
}

fn record_legacy_path_side_effect_block(path: &LegacyPathDescriptor) {
    metrics::counter!(
        "legacy_path_side_effect_block_total",
        1u64,
        "path" => path.path
    );
}

fn log_trigger_legacy_path_contracts() {
    for descriptor in [
        TRIGGER_POOL_SCORED_OBSERVER_PATH,
        TRIGGER_EMBEDDED_ORACLE_PIPELINE_PATH,
        TRIGGER_NO_EVENT_BUS_PATH,
    ] {
        info!(
            path = descriptor.path,
            classification = descriptor.classification.as_str(),
            runtime_plane = descriptor.runtime_plane.as_str(),
            allows_authoritative_buy = descriptor.allows_authoritative_buy,
            removal_date = descriptor.removal_date.unwrap_or("none"),
            "Trigger Phase-6 legacy path classified"
        );
    }
}

fn saturating_elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn handle_legacy_pool_scored_event(
    pending_pools: &mut HashMap<String, Arc<crate::events::DetectedPool>>,
    scored: &crate::events::PoolScoredEvent,
) -> LegacyPoolScoredHandling {
    record_legacy_path_event(&TRIGGER_POOL_SCORED_OBSERVER_PATH);

    if pending_pools.remove(&scored.pool_amm_id).is_some() {
        if scored.passed {
            record_legacy_path_side_effect_block(&TRIGGER_POOL_SCORED_OBSERVER_PATH);
            debug!(
                runtime_plane = RuntimePlane::LegacyObservation.as_str(),
                path = TRIGGER_POOL_SCORED_OBSERVER_PATH.path,
                pool = %scored.pool_amm_id,
                score = scored.score,
                "Trigger: legacy PoolScored path blocked from emitting authoritative BUY side effects"
            );
            LegacyPoolScoredHandling {
                blocked_side_effect: true,
                removed_pending_pool: true,
            }
        } else {
            info!(
                runtime_plane = RuntimePlane::LegacyObservation.as_str(),
                path = TRIGGER_POOL_SCORED_OBSERVER_PATH.path,
                pool = %scored.pool_amm_id,
                score = scored.score,
                "Trigger: legacy PoolScored observation closed without side effects"
            );
            LegacyPoolScoredHandling {
                blocked_side_effect: false,
                removed_pending_pool: true,
            }
        }
    } else {
        warn!(
            runtime_plane = RuntimePlane::LegacyObservation.as_str(),
            path = TRIGGER_POOL_SCORED_OBSERVER_PATH.path,
            pool = %scored.pool_amm_id,
            "Trigger: received legacy PoolScored observation for unknown or expired pool"
        );
        LegacyPoolScoredHandling {
            blocked_side_effect: false,
            removed_pending_pool: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BuyPreparationTelemetry {
    pub tip_floor_cache_hit: bool,
    pub tip_floor_cache_age_ms: u64,
    pub tip_floor_fetch_latency_ms: u64,
    pub tip_floor_cache_mode: &'static str,
    pub tip_floor_source: &'static str,
    pub tip_floor_inflight_join_result: &'static str,
    pub tip_floor_inflight_wait_ms: u64,
    pub token_program_override_present: bool,
    pub token_program_proof_result: &'static str,
    pub token_program_source: &'static str,
    pub priority_fee_cache_hit: bool,
    pub priority_fee_cache_age_ms: u64,
    pub priority_fee_fetch_latency_ms: u64,
    pub priority_fee_cache_mode: &'static str,
    pub priority_fee_source: &'static str,
    pub priority_fee_inflight_join_result: &'static str,
    pub priority_fee_inflight_wait_ms: u64,
    pub payer_load_ms: u64,
    pub payer_balance_fetch_ms: u64,
    pub payer_account_fetch_ms: u64,
    pub mint_account_fetch_ms: u64,
    pub token_balance_probe_ms: u64,
    pub ata_rent_fetch_ms: u64,
    pub build_once_ms: u64,
    pub rebuild_ms: u64,
}

impl Default for BuyPreparationTelemetry {
    fn default() -> Self {
        Self {
            tip_floor_cache_hit: false,
            tip_floor_cache_age_ms: 0,
            tip_floor_fetch_latency_ms: 0,
            tip_floor_cache_mode: "not_collected",
            tip_floor_source: "not_collected",
            tip_floor_inflight_join_result: "not_attempted",
            tip_floor_inflight_wait_ms: 0,
            token_program_override_present: false,
            token_program_proof_result: "not_collected",
            token_program_source: "not_collected",
            priority_fee_cache_hit: false,
            priority_fee_cache_age_ms: 0,
            priority_fee_fetch_latency_ms: 0,
            priority_fee_cache_mode: "not_collected",
            priority_fee_source: "not_collected",
            priority_fee_inflight_join_result: "not_attempted",
            priority_fee_inflight_wait_ms: 0,
            payer_load_ms: 0,
            payer_balance_fetch_ms: 0,
            payer_account_fetch_ms: 0,
            mint_account_fetch_ms: 0,
            token_balance_probe_ms: 0,
            ata_rent_fetch_ms: 0,
            build_once_ms: 0,
            rebuild_ms: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BuyBuildProfile {
    pub mint: Pubkey,
    pub payer_pubkey: Pubkey,
    pub user_ata: Pubkey,
    pub token_program: Pubkey,
    pub attach_idempotent_ata_create: bool,
    pub ata_missing_pre_submit: bool,
    pub account_overrides: BuyAccountOverrides,
    pub amount_lamports: u64,
    pub trade_value_sol: f64,
    pub pre_submit_token_balance: Option<u64>,
    pub buy_variant: trigger::PumpfunBuyVariant,
    pub entry_token_amount_raw: Option<u64>,
    pub min_tokens_out: u64,
    pub token_param_role: &'static str,
    pub ata_instruction: Option<Instruction>,
    pub buy_instruction: Instruction,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedBuyTokenParam {
    entry_token_amount_raw: Option<u64>,
    min_tokens_out: u64,
}

#[derive(Debug, Clone)]
struct PreparedBuyRequestBuildMetadata {
    recent_blockhash: Hash,
    blockhash_source: &'static str,
    blockhash_age_ms: u64,
    blockhash_last_valid_block_height: u64,
    blockhash_observed_block_height: u64,
    blockhash_fetched_at: Instant,
    blockhash_fetch_latency_ms: u64,
    post_blockhash_build_latency_ms: u64,
    reserve_slot_latency_ms: u64,
    shadow_spawn_latency_ms: u64,
    decision_ts_ms: u64,
}

impl PreparedBuyRequestBuildMetadata {
    fn local(recent_blockhash: Hash) -> Self {
        Self {
            recent_blockhash,
            blockhash_source: "local",
            blockhash_age_ms: 0,
            blockhash_last_valid_block_height: 0,
            blockhash_observed_block_height: 0,
            blockhash_fetched_at: Instant::now(),
            blockhash_fetch_latency_ms: 0,
            post_blockhash_build_latency_ms: 0,
            reserve_slot_latency_ms: 0,
            shadow_spawn_latency_ms: 0,
            decision_ts_ms: TriggerComponent::now_ms(),
        }
    }

    fn live(
        snapshot: &CachedLiveBlockhash,
        blockhash_source: &'static str,
        blockhash_fetch_latency_ms: u64,
        post_blockhash_build_latency_ms: u64,
        decision_ts_ms: u64,
    ) -> Self {
        Self {
            recent_blockhash: snapshot.blockhash,
            blockhash_source,
            blockhash_age_ms: snapshot.age_ms(),
            blockhash_last_valid_block_height: snapshot.last_valid_block_height,
            blockhash_observed_block_height: snapshot.observed_block_height,
            blockhash_fetched_at: snapshot.fetched_at,
            blockhash_fetch_latency_ms,
            post_blockhash_build_latency_ms,
            reserve_slot_latency_ms: 0,
            shadow_spawn_latency_ms: 0,
            decision_ts_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreparedBuyRequest {
    pub join_metadata: ExecutionJoinMetadata,
    pub mint: Pubkey,
    pub payer_pubkey: Pubkey,
    pub payer_provenance: &'static str,
    pub user_ata: Pubkey,
    pub token_program: Pubkey,
    pub attach_idempotent_ata_create: bool,
    pub ata_missing_pre_submit: bool,
    pub account_overrides: BuyAccountOverrides,
    pub pre_submit_token_balance: Option<u64>,
    pub amount_lamports: u64,
    pub trade_value_sol: f64,
    pub entry_token_amount_raw: Option<u64>,
    pub tip_lamports: u64,
    pub min_tokens_out: u64,
    pub priority_fee_micro_lamports: u64,
    pub recent_blockhash: Hash,
    pub blockhash_source: &'static str,
    pub blockhash_age_ms: u64,
    pub blockhash_last_valid_block_height: u64,
    pub blockhash_observed_block_height: u64,
    pub blockhash_fetched_at: Instant,
    pub blockhash_fetch_latency_ms: u64,
    pub post_blockhash_build_latency_ms: u64,
    pub reserve_slot_latency_ms: u64,
    pub shadow_spawn_latency_ms: u64,
    pub preparation_telemetry: BuyPreparationTelemetry,
    pub build_profile: Option<BuyBuildProfile>,
    pub rpc_buy_tx: Transaction,
    pub buy_tx: VersionedTransaction,
    pub tip_tx: Option<VersionedTransaction>,
    pub decision_ts_ms: u64,
}

impl PreparedBuyRequest {
    pub fn with_join_metadata(mut self, join_metadata: ExecutionJoinMetadata) -> Self {
        self.join_metadata = join_metadata;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CounterfactualProbeMissingAccount {
    pub pubkey: Pubkey,
    pub role: String,
}

pub struct PendingShadowSimulation {
    pub request: PreparedBuyRequest,
    pub handle: tokio::task::JoinHandle<Result<super::shadow_run::ShadowBuySimulationReport>>,
}

#[derive(Debug, Clone)]
struct UserAtaProbeResult {
    user_ata: Pubkey,
    ata_missing_pre_submit: bool,
    pre_submit_token_balance: Option<u64>,
    expected_ata_rent_lamports: u64,
    ata_rent_fetch_ms: u64,
    elapsed_ms: u64,
}

pub struct TriggerDispatchReceipt {
    pub primary_outcome: Result<TriggerBuyOutcome>,
    pub shadow_task: Option<PendingShadowSimulation>,
    pub active_position_lease: Option<ActivePositionLease>,
    pub retain_position_slot_on_error: bool,
    pub failed_request: Option<PreparedBuyRequest>,
    pub failed_context: Option<TriggerDispatchFailureContext>,
}

#[derive(Debug, Clone)]
pub struct TriggerDispatchFailureContext {
    pub join_metadata: ExecutionJoinMetadata,
    pub amount_lamports: u64,
    pub tip_lamports: u64,
    pub decision_ts_ms: u64,
    pub payer_provenance: &'static str,
    pub payer_pubkey: Option<String>,
}

#[derive(Clone)]
struct ResolvedTriggerPayer {
    payer: Arc<Keypair>,
    provenance: &'static str,
    requires_balance_preflight: bool,
}

#[derive(Debug)]
enum SubmitPreparedViaSenderError {
    Failed(anyhow::Error),
    UncertainLanding(anyhow::Error),
}

impl SubmitPreparedViaSenderError {
    fn should_retain_position_slot(&self) -> bool {
        matches!(self, Self::UncertainLanding(_))
    }

    fn into_anyhow(self) -> anyhow::Error {
        match self {
            Self::Failed(error) | Self::UncertainLanding(error) => error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SenderBuyAttemptConfirmation {
    Confirmed {
        signature: Signature,
        source: &'static str,
        landed_slot: Option<u64>,
    },
    Failed {
        source: &'static str,
        detail: String,
    },
    Uncertain,
}

#[derive(Debug, Clone)]
struct CachedLiveBlockhash {
    blockhash: Hash,
    fetched_at: Instant,
    last_valid_block_height: u64,
    observed_block_height: u64,
}

impl CachedLiveBlockhash {
    fn age_ms(&self) -> u64 {
        saturating_elapsed_ms(self.fetched_at)
    }

    fn is_fresh(&self) -> bool {
        self.age_ms() <= BUY_BLOCKHASH_CACHE_MAX_AGE_MS
            && self.observed_block_height <= self.last_valid_block_height
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SignatureStatusObservation {
    Missing,
    Confirmed { slot: u64 },
    Failed { slot: u64, error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SignatureStatusEntry {
    signature: Signature,
    observation: SignatureStatusObservation,
}

#[derive(Debug, Clone, Default)]
pub struct BuyAccountOverrides {
    pub global_config: Option<Pubkey>,
    pub fee_recipient: Option<Pubkey>,
    pub token_program: Option<Pubkey>,
    pub creator_pubkey: Option<Pubkey>,
    pub buy_variant: Option<trigger::PumpfunBuyVariant>,
    pub associated_bonding_curve: Option<Pubkey>,
    pub legacy_buy_curve: Option<BondingCurve>,
}

#[derive(Debug, Clone)]
pub enum TriggerPrewarmAdvisory {
    TipFloor,
    BuyPriorityFee {
        mint: Pubkey,
        account_overrides: BuyAccountOverrides,
        tip_lamports: u64,
    },
}

// =============================================================================
// TriggerComponent - live BUY executor
// =============================================================================

/// TriggerComponent handles live fire execution of Solana BUY transactions.
///
/// This component is responsible for:
/// - Building a single signed BUY transaction with priority fee + inline tip
/// - Submitting the transaction via Helius Sender
/// - Confirming landing through Yellowstone gRPC
///
/// # Architecture
///
/// ```text
/// OracleRuntime (Gunshot Detection)
///     ↓
/// TriggerComponent::prepare_buy_request()
///     ↓
/// [Compute Budget, ATA?, Swap, Tip Transfer] → VersionedTransaction
///     ↓
/// Helius Sender → Yellowstone Confirmation
/// ```
pub struct TriggerComponent {
    /// Configuration for the trigger component
    config: TriggerComponentConfig,
    cached_payer: Option<Arc<Keypair>>,
    cached_shadow_ephemeral_payer: Option<Arc<Keypair>>,
    ata_rent_cache: Arc<RwLock<HashMap<Pubkey, u64>>>,
    primary_rpc_client: Arc<solana_client::nonblocking::rpc_client::RpcClient>,
    shadow_rpc_client: Option<Arc<solana_client::nonblocking::rpc_client::RpcClient>>,
    live_tx_sender: Option<Arc<LiveTxSender>>,
    live_blockhash_cache: Arc<RwLock<Option<CachedLiveBlockhash>>>,
    shadow_ledger: Arc<ShadowLedger>,
    account_state_core: Arc<AccountStateReducer>,
    shadow_simulator: Arc<dyn ShadowSimulator>,
    shadow_run_semaphore: Arc<Semaphore>,
    position_limit_tracker: PositionLimitTracker,
    #[cfg(test)]
    prepared_request_invocations: std::sync::atomic::AtomicU64,
}

impl TriggerComponent {
    const MINT_FETCH_ATTEMPTS: usize = 20;
    const MINT_FETCH_RETRY_DELAY_MS: u64 = 150;
    const MINT_NOT_FOUND_RETRY_DELAY_MS: u64 = 150;
    const ATA_FETCH_ATTEMPTS: usize = 4;
    const ATA_FETCH_RETRY_DELAY_MS: u64 = 50;
    const RPC_FETCH_ATTEMPTS: usize = 6;
    const RPC_FETCH_RETRY_DELAY_MS: u64 = 100;

    fn is_account_not_found_error(err: &ClientError) -> bool {
        matches!(
            err.kind(),
            ClientErrorKind::RpcError(RpcError::ForUser(message))
                if message.contains("AccountNotFound:")
        ) || err.to_string().contains("AccountNotFound")
    }

    fn is_retryable_account_fetch_error(err: &ClientError) -> bool {
        let message = err.to_string();
        message.contains("429")
            || message.contains("Too Many Requests")
            || message.contains("timed out")
            || message.contains("timeout")
            || message.contains("connection reset")
            || message.contains("connection refused")
            || Self::is_account_not_found_error(err)
    }

    fn is_retryable_rpc_error(err: &ClientError) -> bool {
        let message = err.to_string();
        message.contains("429")
            || message.contains("Too Many Requests")
            || message.contains("timed out")
            || message.contains("timeout")
            || message.contains("connection reset")
            || message.contains("connection refused")
    }

    fn should_retry_account_fetch(
        primary_retryable: bool,
        secondary_retryable: Option<bool>,
    ) -> bool {
        primary_retryable || secondary_retryable.unwrap_or(false)
    }

    fn account_fetch_retry_delay_ms(
        default_delay_ms: u64,
        not_found_delay_ms: u64,
        primary_not_found: bool,
        secondary_not_found: Option<bool>,
    ) -> u64 {
        if primary_not_found || secondary_not_found.unwrap_or(false) {
            not_found_delay_ms
        } else {
            default_delay_ms
        }
    }

    fn into_shadow_preparation_error(
        fallback_message: String,
        last_err: Option<anyhow::Error>,
        retries_performed: usize,
    ) -> anyhow::Error {
        let message = last_err
            .map(|err| err.to_string())
            .unwrap_or(fallback_message);
        ShadowPreparationError::new(message, retries_performed).into()
    }

    fn load_configured_payer(config: &TriggerComponentConfig) -> Result<Option<Arc<Keypair>>> {
        let Some(path) = config.keypair_path.as_deref() else {
            return Ok(None);
        };

        let payer = read_keypair_file(path)
            .map_err(|e| anyhow::anyhow!("Failed to read keypair from {}: {}", path, e))?;
        Ok(Some(Arc::new(payer)))
    }

    fn shadow_payer_strategy(config: &TriggerComponentConfig) -> TriggerShadowPayerStrategy {
        if matches!(config.entry_mode, TriggerEntryMode::ShadowOnly) {
            config.shadow_run.payer_strategy
        } else {
            TriggerShadowPayerStrategy::Configured
        }
    }

    fn payer_provenance_label(strategy: TriggerShadowPayerStrategy) -> &'static str {
        strategy.as_str()
    }

    fn requires_configured_payer(config: &TriggerComponentConfig) -> bool {
        !matches!(
            Self::shadow_payer_strategy(config),
            TriggerShadowPayerStrategy::Ephemeral
        )
    }

    fn should_prepare_on_shadow_rpc(&self) -> bool {
        self.config.shadow_run.enabled
            && matches!(self.config.entry_mode, TriggerEntryMode::ShadowOnly)
            && self.config.shadow_run.shadow_rpc_url != self.config.rpc_url
    }

    fn record_tx_send_latency(&self, transport: &'static str, decision_ts_ms: u64) {
        let latency_ms = Self::now_ms().saturating_sub(decision_ts_ms);
        ::metrics::histogram!(
            "tx_send_latency_ms",
            latency_ms as f64,
            "transport" => transport
        );
    }

    fn preparation_rpc(&self) -> &solana_client::nonblocking::rpc_client::RpcClient {
        if self.should_prepare_on_shadow_rpc() {
            self.shadow_rpc_client
                .as_deref()
                .unwrap_or(self.primary_rpc_client.as_ref())
        } else {
            self.primary_rpc_client.as_ref()
        }
    }

    fn secondary_shadow_rpc(&self) -> Option<&solana_client::nonblocking::rpc_client::RpcClient> {
        (!self.should_prepare_on_shadow_rpc()
            && self.config.shadow_run.enabled
            && self.config.shadow_run.shadow_rpc_url != self.config.rpc_url)
            .then_some(self.shadow_rpc_client.as_deref())
            .flatten()
    }

    fn spawn_live_blockhash_cache_task_if_needed(
        config: &TriggerComponentConfig,
        live_tx_sender: &Option<Arc<LiveTxSender>>,
        primary_rpc_client: Arc<solana_client::nonblocking::rpc_client::RpcClient>,
        secondary_rpc_client: Option<Arc<solana_client::nonblocking::rpc_client::RpcClient>>,
        live_blockhash_cache: Arc<RwLock<Option<CachedLiveBlockhash>>>,
    ) {
        if live_tx_sender.is_none()
            || !matches!(
                config.entry_mode,
                TriggerEntryMode::Live | TriggerEntryMode::LiveAndShadow
            )
        {
            return;
        }

        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };

        handle.spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_millis(BUY_BLOCKHASH_CACHE_REFRESH_MS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;
                match Self::fetch_latest_blockhash_snapshot_once(
                    primary_rpc_client.as_ref(),
                    secondary_rpc_client.as_deref(),
                )
                .await
                {
                    Ok(snapshot) => {
                        if let Ok(mut guard) = live_blockhash_cache.write() {
                            *guard = Some(snapshot);
                        }
                    }
                    Err(err) => {
                        warn!(
                            error = %err,
                            "Trigger: live BUY blockhash background refresh failed"
                        );
                    }
                }
            }
        });
    }

    fn read_cached_live_blockhash(&self) -> Option<CachedLiveBlockhash> {
        self.live_blockhash_cache
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn store_cached_live_blockhash(&self, snapshot: CachedLiveBlockhash) {
        if let Ok(mut guard) = self.live_blockhash_cache.write() {
            *guard = Some(snapshot);
        }
    }

    async fn fetch_mint_account_with_retry(
        &self,
        mint: &Pubkey,
        primary_rpc: &solana_client::nonblocking::rpc_client::RpcClient,
    ) -> Result<Account> {
        let secondary_rpc = self.secondary_shadow_rpc();

        let mut last_err: Option<anyhow::Error> = None;
        let mut retries_performed = 0usize;
        for attempt in 0..Self::MINT_FETCH_ATTEMPTS {
            let mut secondary_retryable = None;
            let retry_delay_ms;

            match primary_rpc
                .get_account_with_commitment(mint, CommitmentConfig::processed())
                .await
            {
                Ok(response) => {
                    if let Some(account) = response.value {
                        return Ok(account);
                    }

                    let primary_not_found = true;
                    let primary_retryable = true;
                    let mut local_retry_delay_ms = Self::account_fetch_retry_delay_ms(
                        Self::MINT_FETCH_RETRY_DELAY_MS,
                        Self::MINT_NOT_FOUND_RETRY_DELAY_MS,
                        primary_not_found,
                        None,
                    );

                    if let Some(secondary_rpc) = secondary_rpc.as_ref() {
                        match secondary_rpc
                            .get_account_with_commitment(mint, CommitmentConfig::processed())
                            .await
                        {
                            Ok(response) => {
                                if let Some(account) = response.value {
                                    return Ok(account);
                                }

                                let secondary_not_found = Some(true);
                                let secondary_retryable = Some(true);
                                local_retry_delay_ms = Self::account_fetch_retry_delay_ms(
                                    Self::MINT_FETCH_RETRY_DELAY_MS,
                                    Self::MINT_NOT_FOUND_RETRY_DELAY_MS,
                                    primary_not_found,
                                    secondary_not_found,
                                );
                                last_err = Some(anyhow::anyhow!(
                                    "primary=AccountNotFound: pubkey={} | secondary=AccountNotFound: pubkey={}",
                                    mint,
                                    mint
                                ));

                                if !Self::should_retry_account_fetch(
                                    primary_retryable,
                                    secondary_retryable,
                                ) || attempt + 1 == Self::MINT_FETCH_ATTEMPTS
                                {
                                    break;
                                }
                            }
                            Err(secondary_err) => {
                                secondary_retryable =
                                    Some(Self::is_retryable_account_fetch_error(&secondary_err));
                                let secondary_not_found =
                                    Some(Self::is_account_not_found_error(&secondary_err));
                                local_retry_delay_ms = Self::account_fetch_retry_delay_ms(
                                    Self::MINT_FETCH_RETRY_DELAY_MS,
                                    Self::MINT_NOT_FOUND_RETRY_DELAY_MS,
                                    primary_not_found,
                                    secondary_not_found,
                                );
                                last_err = Some(anyhow::anyhow!(
                                    "primary=AccountNotFound: pubkey={} | secondary={}",
                                    mint,
                                    secondary_err
                                ));

                                if !Self::should_retry_account_fetch(
                                    primary_retryable,
                                    secondary_retryable,
                                ) || attempt + 1 == Self::MINT_FETCH_ATTEMPTS
                                {
                                    break;
                                }
                            }
                        }
                    } else {
                        last_err = Some(anyhow::anyhow!("AccountNotFound: pubkey={}", mint));
                        if attempt + 1 == Self::MINT_FETCH_ATTEMPTS {
                            break;
                        }
                    }

                    retry_delay_ms = local_retry_delay_ms;
                }
                Err(primary_err) => {
                    let primary_not_found = Self::is_account_not_found_error(&primary_err);
                    let primary_retryable = Self::is_retryable_account_fetch_error(&primary_err);
                    let mut local_retry_delay_ms = Self::account_fetch_retry_delay_ms(
                        Self::MINT_FETCH_RETRY_DELAY_MS,
                        Self::MINT_NOT_FOUND_RETRY_DELAY_MS,
                        primary_not_found,
                        None,
                    );

                    if let Some(secondary_rpc) = secondary_rpc.as_ref() {
                        match secondary_rpc
                            .get_account_with_commitment(mint, CommitmentConfig::processed())
                            .await
                        {
                            Ok(response) => {
                                if let Some(account) = response.value {
                                    return Ok(account);
                                }

                                secondary_retryable = Some(true);
                                let secondary_not_found = Some(true);
                                local_retry_delay_ms = Self::account_fetch_retry_delay_ms(
                                    Self::MINT_FETCH_RETRY_DELAY_MS,
                                    Self::MINT_NOT_FOUND_RETRY_DELAY_MS,
                                    primary_not_found,
                                    secondary_not_found,
                                );
                                last_err = Some(anyhow::anyhow!(
                                    "primary={} | secondary=AccountNotFound: pubkey={}",
                                    primary_err,
                                    mint
                                ));
                            }
                            Err(secondary_err) => {
                                secondary_retryable =
                                    Some(Self::is_retryable_account_fetch_error(&secondary_err));
                                let secondary_not_found =
                                    Some(Self::is_account_not_found_error(&secondary_err));
                                local_retry_delay_ms = Self::account_fetch_retry_delay_ms(
                                    Self::MINT_FETCH_RETRY_DELAY_MS,
                                    Self::MINT_NOT_FOUND_RETRY_DELAY_MS,
                                    primary_not_found,
                                    secondary_not_found,
                                );
                                last_err = Some(anyhow::anyhow!(
                                    "primary={} | secondary={}",
                                    primary_err,
                                    secondary_err
                                ));
                            }
                        }
                    } else {
                        last_err = Some(anyhow::Error::new(primary_err));
                    }

                    if !Self::should_retry_account_fetch(primary_retryable, secondary_retryable)
                        || attempt + 1 == Self::MINT_FETCH_ATTEMPTS
                    {
                        break;
                    }

                    retry_delay_ms = local_retry_delay_ms;
                }
            }

            retries_performed = attempt + 1;
            tokio::time::sleep(Duration::from_millis(retry_delay_ms)).await;
        }

        Err(Self::into_shadow_preparation_error(
            format!("Failed to fetch mint account after retries: pubkey={mint}"),
            last_err,
            retries_performed,
        ))
    }

    async fn user_ata_exists_with_retry(
        &self,
        user_ata: &Pubkey,
        primary_rpc: &solana_client::nonblocking::rpc_client::RpcClient,
    ) -> Result<bool> {
        let secondary_rpc = self.secondary_shadow_rpc();

        let mut last_err: Option<anyhow::Error> = None;
        let mut retries_performed = 0usize;
        for attempt in 0..Self::ATA_FETCH_ATTEMPTS {
            let mut secondary_retryable = None;
            match primary_rpc.get_account(user_ata).await {
                Ok(_) => return Ok(true),
                Err(primary_err) => {
                    let primary_not_found = Self::is_account_not_found_error(&primary_err);
                    let primary_retryable = Self::is_retryable_account_fetch_error(&primary_err);

                    if let Some(secondary_rpc) = secondary_rpc.as_ref() {
                        match secondary_rpc.get_account(user_ata).await {
                            Ok(_) => return Ok(true),
                            Err(secondary_err) => {
                                let secondary_not_found =
                                    Self::is_account_not_found_error(&secondary_err);
                                secondary_retryable =
                                    Some(Self::is_retryable_account_fetch_error(&secondary_err));

                                if primary_not_found && secondary_not_found {
                                    return Ok(false);
                                }

                                last_err = Some(anyhow::anyhow!(
                                    "primary={} | secondary={}",
                                    primary_err,
                                    secondary_err
                                ));

                                if !Self::should_retry_account_fetch(
                                    primary_retryable,
                                    secondary_retryable,
                                ) || attempt + 1 == Self::ATA_FETCH_ATTEMPTS
                                {
                                    break;
                                }
                            }
                        }
                    } else {
                        if primary_not_found {
                            return Ok(false);
                        }

                        last_err = Some(anyhow::Error::new(primary_err));
                        if !Self::should_retry_account_fetch(primary_retryable, secondary_retryable)
                            || attempt + 1 == Self::ATA_FETCH_ATTEMPTS
                        {
                            break;
                        }
                    }
                }
            }

            retries_performed = attempt + 1;
            tokio::time::sleep(Duration::from_millis(Self::ATA_FETCH_RETRY_DELAY_MS)).await;
        }

        Err(Self::into_shadow_preparation_error(
            format!("Failed to fetch user ATA after retries: pubkey={user_ata}"),
            last_err,
            retries_performed,
        ))
    }

    async fn fetch_payer_balance_with_retry(
        &self,
        payer_pubkey: &Pubkey,
        primary_rpc: &solana_client::nonblocking::rpc_client::RpcClient,
    ) -> Result<u64> {
        let secondary_rpc = self.secondary_shadow_rpc();
        let mut last_err: Option<anyhow::Error> = None;
        let mut retries_performed = 0usize;

        for attempt in 0..Self::RPC_FETCH_ATTEMPTS {
            let mut secondary_retryable = None;
            match primary_rpc.get_balance(payer_pubkey).await {
                Ok(balance) => return Ok(balance),
                Err(primary_err) => {
                    let primary_retryable = Self::is_retryable_rpc_error(&primary_err);
                    if let Some(secondary_rpc) = secondary_rpc.as_ref() {
                        match secondary_rpc.get_balance(payer_pubkey).await {
                            Ok(balance) => return Ok(balance),
                            Err(secondary_err) => {
                                secondary_retryable =
                                    Some(Self::is_retryable_rpc_error(&secondary_err));
                                last_err = Some(anyhow::anyhow!(
                                    "primary={} | secondary={}",
                                    primary_err,
                                    secondary_err
                                ));
                            }
                        }
                    } else {
                        last_err = Some(anyhow::Error::new(primary_err));
                    }

                    if !Self::should_retry_account_fetch(primary_retryable, secondary_retryable)
                        || attempt + 1 == Self::RPC_FETCH_ATTEMPTS
                    {
                        break;
                    }
                }
            }

            retries_performed = attempt + 1;
            tokio::time::sleep(Duration::from_millis(Self::RPC_FETCH_RETRY_DELAY_MS)).await;
        }

        Err(Self::into_shadow_preparation_error(
            format!("Failed to fetch payer balance after retries: payer={payer_pubkey}"),
            last_err,
            retries_performed,
        ))
    }

    async fn fetch_payer_account_with_retry(
        &self,
        payer_pubkey: &Pubkey,
        primary_rpc: &solana_client::nonblocking::rpc_client::RpcClient,
    ) -> Result<Account> {
        let secondary_rpc = self.secondary_shadow_rpc();
        let mut last_err: Option<anyhow::Error> = None;
        let mut retries_performed = 0usize;

        for attempt in 0..Self::RPC_FETCH_ATTEMPTS {
            let mut secondary_retryable = None;
            match primary_rpc.get_account(payer_pubkey).await {
                Ok(account) => return Ok(account),
                Err(primary_err) => {
                    let primary_retryable = Self::is_retryable_rpc_error(&primary_err);
                    if let Some(secondary_rpc) = secondary_rpc.as_ref() {
                        match secondary_rpc.get_account(payer_pubkey).await {
                            Ok(account) => return Ok(account),
                            Err(secondary_err) => {
                                secondary_retryable =
                                    Some(Self::is_retryable_rpc_error(&secondary_err));
                                last_err = Some(anyhow::anyhow!(
                                    "primary={} | secondary={}",
                                    primary_err,
                                    secondary_err
                                ));
                            }
                        }
                    } else {
                        last_err = Some(anyhow::Error::new(primary_err));
                    }

                    if !Self::should_retry_account_fetch(primary_retryable, secondary_retryable)
                        || attempt + 1 == Self::RPC_FETCH_ATTEMPTS
                    {
                        break;
                    }
                }
            }

            retries_performed = attempt + 1;
            tokio::time::sleep(Duration::from_millis(Self::RPC_FETCH_RETRY_DELAY_MS)).await;
        }

        Err(Self::into_shadow_preparation_error(
            format!("Failed to fetch payer account after retries: payer={payer_pubkey}"),
            last_err,
            retries_performed,
        ))
    }

    async fn fetch_latest_blockhash_snapshot_from_rpc(
        rpc: &solana_client::nonblocking::rpc_client::RpcClient,
    ) -> std::result::Result<CachedLiveBlockhash, ClientError> {
        let (blockhash, last_valid_block_height) = rpc
            .get_latest_blockhash_with_commitment(CommitmentConfig::confirmed())
            .await?;
        let observed_block_height = rpc
            .get_block_height_with_commitment(CommitmentConfig::confirmed())
            .await?;

        Ok(CachedLiveBlockhash {
            blockhash,
            fetched_at: Instant::now(),
            last_valid_block_height,
            observed_block_height,
        })
    }

    async fn fetch_latest_blockhash_snapshot_once(
        primary_rpc: &solana_client::nonblocking::rpc_client::RpcClient,
        secondary_rpc: Option<&solana_client::nonblocking::rpc_client::RpcClient>,
    ) -> Result<CachedLiveBlockhash> {
        match Self::fetch_latest_blockhash_snapshot_from_rpc(primary_rpc).await {
            Ok(snapshot) => Ok(snapshot),
            Err(primary_err) => {
                if let Some(secondary_rpc) = secondary_rpc {
                    match Self::fetch_latest_blockhash_snapshot_from_rpc(secondary_rpc).await {
                        Ok(snapshot) => Ok(snapshot),
                        Err(secondary_err) => Err(anyhow::anyhow!(
                            "primary={} | secondary={}",
                            primary_err,
                            secondary_err
                        )),
                    }
                } else {
                    Err(anyhow::Error::new(primary_err))
                }
            }
        }
    }

    async fn fetch_latest_blockhash_snapshot_with_retry(
        &self,
        primary_rpc: &solana_client::nonblocking::rpc_client::RpcClient,
    ) -> Result<CachedLiveBlockhash> {
        let secondary_rpc = self.secondary_shadow_rpc();
        let mut last_err: Option<anyhow::Error> = None;
        let mut retries_performed = 0usize;

        for attempt in 0..Self::RPC_FETCH_ATTEMPTS {
            let mut secondary_retryable = None;
            match Self::fetch_latest_blockhash_snapshot_from_rpc(primary_rpc).await {
                Ok(snapshot) => return Ok(snapshot),
                Err(primary_err) => {
                    let primary_retryable = Self::is_retryable_rpc_error(&primary_err);
                    if let Some(secondary_rpc) = secondary_rpc.as_ref() {
                        match Self::fetch_latest_blockhash_snapshot_from_rpc(secondary_rpc).await {
                            Ok(snapshot) => return Ok(snapshot),
                            Err(secondary_err) => {
                                secondary_retryable =
                                    Some(Self::is_retryable_rpc_error(&secondary_err));
                                last_err = Some(anyhow::anyhow!(
                                    "primary={} | secondary={}",
                                    primary_err,
                                    secondary_err
                                ));
                            }
                        }
                    } else {
                        last_err = Some(anyhow::Error::new(primary_err));
                    }

                    if !Self::should_retry_account_fetch(primary_retryable, secondary_retryable)
                        || attempt + 1 == Self::RPC_FETCH_ATTEMPTS
                    {
                        break;
                    }
                }
            }

            retries_performed = attempt + 1;
            tokio::time::sleep(Duration::from_millis(Self::RPC_FETCH_RETRY_DELAY_MS)).await;
        }

        Err(Self::into_shadow_preparation_error(
            "Failed to fetch recent blockhash after retries".to_string(),
            last_err,
            retries_performed,
        ))
    }

    async fn resolve_live_blockhash(
        &self,
        primary_rpc: &solana_client::nonblocking::rpc_client::RpcClient,
    ) -> Result<(CachedLiveBlockhash, &'static str)> {
        if let Some(snapshot) = self.read_cached_live_blockhash() {
            if snapshot.is_fresh() {
                return Ok((snapshot, "cache"));
            }
        }

        let snapshot = self
            .fetch_latest_blockhash_snapshot_with_retry(primary_rpc)
            .await?;
        self.store_cached_live_blockhash(snapshot.clone());
        Ok((snapshot, "rpc_refresh"))
    }

    fn is_missing_token_account_balance_error(err: &ClientError) -> bool {
        let message = err.to_string();
        Self::is_account_not_found_error(err)
            || message.contains("could not find account")
            || message.contains("Invalid param")
    }

    fn is_yellowstone_resource_exhausted(err: &LiveTxSenderError) -> bool {
        match err {
            LiveTxSenderError::ConfirmationTransport { message, .. } => {
                let normalized = message.to_ascii_lowercase();
                normalized.contains("resourceexhausted")
                    || normalized.contains("concurrent yellowstone geyser stream limit reached")
                    || normalized.contains("stream limit reached")
            }
            _ => false,
        }
    }

    async fn fetch_signature_status_observations(
        &self,
        signatures: &[Signature],
    ) -> Result<Vec<SignatureStatusEntry>> {
        if signatures.is_empty() {
            return Ok(Vec::new());
        }

        let response = self
            .primary_rpc_client
            .get_signature_statuses_with_history(signatures)
            .await
            .map_err(|err| anyhow::anyhow!("getSignatureStatuses failed: {err}"))?;

        Ok(signatures
            .iter()
            .copied()
            .zip(response.value.into_iter())
            .map(|(signature, maybe_status)| SignatureStatusEntry {
                signature,
                observation: match maybe_status {
                    Some(status) => match status.err {
                        Some(err) => SignatureStatusObservation::Failed {
                            slot: status.slot,
                            error: format!("{err:?}"),
                        },
                        None => SignatureStatusObservation::Confirmed { slot: status.slot },
                    },
                    None => SignatureStatusObservation::Missing,
                },
            })
            .collect())
    }

    async fn fetch_signature_status_observation(
        &self,
        signature: &Signature,
    ) -> Result<SignatureStatusObservation> {
        self.fetch_signature_status_observations(&[*signature])
            .await?
            .into_iter()
            .next()
            .map(|entry| entry.observation)
            .ok_or_else(|| anyhow::anyhow!("missing signature status observation for {signature}"))
    }

    async fn fetch_token_account_balance(&self, ata: &Pubkey) -> Result<Option<u64>> {
        match self.primary_rpc_client.get_token_account_balance(ata).await {
            Ok(response) => {
                let amount = response.amount.parse::<u64>().map_err(|err| {
                    anyhow::anyhow!("invalid token balance response for {ata}: {err}")
                })?;
                Ok(Some(amount))
            }
            Err(err) if Self::is_missing_token_account_balance_error(&err) => Ok(None),
            Err(err) => Err(anyhow::anyhow!(
                "getTokenAccountBalance failed for {ata}: {err}"
            )),
        }
    }

    async fn probe_user_ata_pre_submit_legacy(
        &self,
        mint: &Pubkey,
        user_ata: Pubkey,
        token_program: &Pubkey,
        rpc: &solana_client::nonblocking::rpc_client::RpcClient,
    ) -> Result<UserAtaProbeResult> {
        let token_balance_probe_started_at = Instant::now();
        let ata_missing_pre_submit = !self
            .user_ata_exists_with_retry(&user_ata, rpc)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch user ATA: {}", e))?;
        let pre_submit_token_balance = if ata_missing_pre_submit {
            Some(0)
        } else {
            match self.fetch_token_account_balance(&user_ata).await {
                Ok(balance) => balance,
                Err(error) => {
                    warn!(
                        mint = %mint,
                        user_ata = %user_ata,
                        error = %error,
                        "Trigger: unable to resolve pre-submit ATA balance for BUY fallback; disabling balance-delta confirmation for this request"
                    );
                    None
                }
            }
        };
        let token_balance_probe_ms = saturating_elapsed_ms(token_balance_probe_started_at);
        let (expected_ata_rent_lamports, ata_rent_fetch_ms) = if ata_missing_pre_submit {
            let ata_rent_fetch_started_at = Instant::now();
            let expected_ata_rent_lamports = self
                .minimum_user_ata_rent_lamports(rpc, token_program)
                .await?;
            (
                expected_ata_rent_lamports,
                saturating_elapsed_ms(ata_rent_fetch_started_at),
            )
        } else {
            (0, 0)
        };

        Ok(UserAtaProbeResult {
            user_ata,
            ata_missing_pre_submit,
            pre_submit_token_balance,
            expected_ata_rent_lamports,
            ata_rent_fetch_ms,
            elapsed_ms: token_balance_probe_ms,
        })
    }

    async fn probe_user_ata_pre_submit(
        &self,
        mint: &Pubkey,
        user_ata: Pubkey,
        token_program: &Pubkey,
        rpc: &solana_client::nonblocking::rpc_client::RpcClient,
    ) -> Result<UserAtaProbeResult> {
        let probe_started_at = Instant::now();
        match self.fetch_token_account_balance(&user_ata).await {
            Ok(Some(balance)) => Ok(UserAtaProbeResult {
                user_ata,
                ata_missing_pre_submit: false,
                pre_submit_token_balance: Some(balance),
                expected_ata_rent_lamports: 0,
                ata_rent_fetch_ms: 0,
                elapsed_ms: saturating_elapsed_ms(probe_started_at),
            }),
            Ok(None) => {
                let fallback = self
                    .probe_user_ata_pre_submit_legacy(mint, user_ata, token_program, rpc)
                    .await?;
                if !fallback.ata_missing_pre_submit {
                    warn!(
                        mint = %mint,
                        user_ata = %user_ata,
                        "Trigger: ATA pre-submit fast probe disagreed with legacy existence probe; preserving conservative non-missing semantics"
                    );
                }
                Ok(UserAtaProbeResult {
                    elapsed_ms: saturating_elapsed_ms(probe_started_at)
                        .saturating_sub(fallback.ata_rent_fetch_ms),
                    ..fallback
                })
            }
            Err(error) => {
                warn!(
                    mint = %mint,
                    user_ata = %user_ata,
                    error = %error,
                    "Trigger: ATA pre-submit fast probe failed; falling back to legacy probe semantics"
                );
                let fallback = self
                    .probe_user_ata_pre_submit_legacy(mint, user_ata, token_program, rpc)
                    .await?;
                Ok(UserAtaProbeResult {
                    elapsed_ms: saturating_elapsed_ms(probe_started_at)
                        .saturating_sub(fallback.ata_rent_fetch_ms),
                    ..fallback
                })
            }
        }
    }

    async fn confirm_sender_buy_attempt(
        &self,
        client: &LiveTxSender,
        submission: &crate::components::live_tx_sender::SenderTransactionSubmission,
        tracked_signatures: &[Signature],
        user_ata: &Pubkey,
        pre_submit_token_balance: Option<u64>,
        max_wait_ms: u64,
    ) -> SenderBuyAttemptConfirmation {
        let deadline = Instant::now() + Duration::from_millis(max_wait_ms);
        let mut unique_signatures = Vec::with_capacity(tracked_signatures.len().max(1));
        for signature in tracked_signatures.iter().copied() {
            if !unique_signatures.contains(&signature) {
                unique_signatures.push(signature);
            }
        }
        if unique_signatures.is_empty() {
            unique_signatures.push(submission.signature);
        }

        let confirm_future = client.confirm_submission_with_timeout(submission, max_wait_ms);
        tokio::pin!(confirm_future);

        let mut balance_delta_observed = false;
        let mut yellowstone_finished = false;

        loop {
            if let Some(pre_submit_balance) = pre_submit_token_balance {
                match self.fetch_token_account_balance(user_ata).await {
                    Ok(Some(post_submit_balance)) if post_submit_balance > pre_submit_balance => {
                        balance_delta_observed = true;
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!(
                            signature = %submission.signature,
                            user_ata = %user_ata,
                            error = %err,
                            "Trigger: BUY fallback ATA balance check failed — retrying"
                        );
                    }
                }
            }

            match self
                .fetch_signature_status_observations(&unique_signatures)
                .await
            {
                Ok(observations) => {
                    if let Some((signature, slot)) =
                        observations
                            .iter()
                            .find_map(|entry| match entry.observation {
                                SignatureStatusObservation::Confirmed { slot } => {
                                    Some((entry.signature, slot))
                                }
                                _ => None,
                            })
                    {
                        return SenderBuyAttemptConfirmation::Confirmed {
                            signature,
                            source: if balance_delta_observed {
                                "balance_delta"
                            } else {
                                "signature_status"
                            },
                            landed_slot: Some(slot),
                        };
                    }

                    let failures: Vec<String> = observations
                        .iter()
                        .filter_map(|entry| match &entry.observation {
                            SignatureStatusObservation::Failed { slot, error } => {
                                Some(format!("{}@{}:{error}", entry.signature, slot))
                            }
                            _ => None,
                        })
                        .collect();
                    if !observations.is_empty() && failures.len() == observations.len() {
                        return SenderBuyAttemptConfirmation::Failed {
                            source: "signature_status",
                            detail: failures.join(" | "),
                        };
                    }
                }
                Err(err) => {
                    warn!(
                        signature = %submission.signature,
                        error = %err,
                        "Trigger: BUY fallback signature-status check failed — retrying"
                    );
                }
            }

            if Instant::now() >= deadline {
                if balance_delta_observed {
                    return SenderBuyAttemptConfirmation::Confirmed {
                        signature: submission.signature,
                        source: "balance_delta",
                        landed_slot: None,
                    };
                }
                return SenderBuyAttemptConfirmation::Uncertain;
            }

            tokio::select! {
                confirmation = &mut confirm_future, if !yellowstone_finished => {
                    yellowstone_finished = true;
                    match confirmation {
                        Ok(confirmed_transaction) => {
                            return SenderBuyAttemptConfirmation::Confirmed {
                                signature: confirmed_transaction.signature,
                                source: if balance_delta_observed {
                                    "balance_delta"
                                } else {
                                    "yellowstone"
                                },
                                landed_slot: confirmed_transaction.landed_slot,
                            };
                        }
                        Err(err @ LiveTxSenderError::ConfirmationTimeout { .. })
                        | Err(err @ LiveTxSenderError::ConfirmationTransport { .. }) => {
                            if Self::is_yellowstone_resource_exhausted(&err) {
                                warn!(
                                    signature = %submission.signature,
                                    user_ata = %user_ata,
                                    "Trigger: Yellowstone confirmation hit stream limits; deferring to BUY balance/status checks"
                                );
                            } else {
                                warn!(
                                    signature = %submission.signature,
                                    user_ata = %user_ata,
                                    error = %err,
                                    "Trigger: Yellowstone confirmation unavailable; deferring to BUY balance/status checks"
                                );
                            }
                        }
                        Err(LiveTxSenderError::ConfirmationRejected { signature, slot }) => {
                            if unique_signatures.len() == 1 {
                                return SenderBuyAttemptConfirmation::Failed {
                                    source: "yellowstone",
                                    detail: format!("{signature}@{slot}: rejected"),
                                };
                            }
                            warn!(
                                rejected_signature = %signature,
                                rejected_slot = slot,
                                tracked_signatures = ?unique_signatures,
                                "Trigger: Yellowstone rejected current BUY attempt while earlier tracked signature remains unresolved"
                            );
                        }
                        Err(LiveTxSenderError::Submit { message }) => {
                            return SenderBuyAttemptConfirmation::Failed {
                                source: "yellowstone",
                                detail: message,
                            };
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(BUY_CONFIRM_POLL_MS)) => {}
            }
        }
    }

    /// Create a new TriggerComponent instance
    pub fn new(config: TriggerComponentConfig) -> Self {
        let position_limit_tracker = PositionLimitTracker::new(config.max_concurrent_positions);
        Self::new_with_position_limit_tracker_and_runtime_state(
            config,
            position_limit_tracker,
            Arc::new(ShadowLedger::new()),
            Arc::new(AccountStateReducer::new()),
        )
    }

    pub fn new_with_position_limit_tracker(
        config: TriggerComponentConfig,
        position_limit_tracker: PositionLimitTracker,
    ) -> Self {
        Self::new_with_runtime_guards_and_runtime_state(
            config,
            Arc::new(RpcShadowSimulator),
            position_limit_tracker,
            Arc::new(ShadowLedger::new()),
            Arc::new(AccountStateReducer::new()),
        )
    }

    pub fn new_with_position_limit_tracker_and_runtime_state(
        config: TriggerComponentConfig,
        position_limit_tracker: PositionLimitTracker,
        shadow_ledger: Arc<ShadowLedger>,
        account_state_core: Arc<AccountStateReducer>,
    ) -> Self {
        Self::new_with_runtime_guards_and_runtime_state(
            config,
            Arc::new(RpcShadowSimulator),
            position_limit_tracker,
            shadow_ledger,
            account_state_core,
        )
    }

    pub fn new_with_position_limit_tracker_and_runtime_state_and_sender(
        config: TriggerComponentConfig,
        position_limit_tracker: PositionLimitTracker,
        shadow_ledger: Arc<ShadowLedger>,
        account_state_core: Arc<AccountStateReducer>,
        live_tx_sender: Option<Arc<LiveTxSender>>,
    ) -> Self {
        Self::new_with_runtime_guards_and_runtime_state_and_sender(
            config,
            Arc::new(RpcShadowSimulator),
            position_limit_tracker,
            shadow_ledger,
            account_state_core,
            live_tx_sender,
        )
    }

    pub fn new_with_shadow_simulator(
        config: TriggerComponentConfig,
        shadow_simulator: Arc<dyn ShadowSimulator>,
    ) -> Self {
        let position_limit_tracker = PositionLimitTracker::new(config.max_concurrent_positions);
        Self::new_with_runtime_guards_and_runtime_state(
            config,
            shadow_simulator,
            position_limit_tracker,
            Arc::new(ShadowLedger::new()),
            Arc::new(AccountStateReducer::new()),
        )
    }

    pub fn new_with_runtime_guards(
        config: TriggerComponentConfig,
        shadow_simulator: Arc<dyn ShadowSimulator>,
        position_limit_tracker: PositionLimitTracker,
    ) -> Self {
        Self::new_with_runtime_guards_and_runtime_state(
            config,
            shadow_simulator,
            position_limit_tracker,
            Arc::new(ShadowLedger::new()),
            Arc::new(AccountStateReducer::new()),
        )
    }

    pub fn new_with_runtime_guards_and_runtime_state(
        config: TriggerComponentConfig,
        shadow_simulator: Arc<dyn ShadowSimulator>,
        position_limit_tracker: PositionLimitTracker,
        shadow_ledger: Arc<ShadowLedger>,
        account_state_core: Arc<AccountStateReducer>,
    ) -> Self {
        Self::new_with_runtime_guards_and_runtime_state_and_sender(
            config,
            shadow_simulator,
            position_limit_tracker,
            shadow_ledger,
            account_state_core,
            None,
        )
    }

    pub fn new_with_runtime_guards_and_runtime_state_and_sender(
        config: TriggerComponentConfig,
        shadow_simulator: Arc<dyn ShadowSimulator>,
        position_limit_tracker: PositionLimitTracker,
        shadow_ledger: Arc<ShadowLedger>,
        account_state_core: Arc<AccountStateReducer>,
        live_tx_sender: Option<Arc<LiveTxSender>>,
    ) -> Self {
        let shadow_max_concurrent = config.shadow_run.max_concurrent.max(1);
        let primary_rpc_client = Arc::new(new_async_rpc_client(config.rpc_url.clone()));
        let shadow_rpc_client = (config.shadow_run.enabled
            && config.shadow_run.shadow_rpc_url != config.rpc_url)
            .then(|| {
                Arc::new(new_async_rpc_client(
                    config.shadow_run.shadow_rpc_url.clone(),
                ))
            });
        let live_blockhash_cache = Arc::new(RwLock::new(None));
        Self::spawn_live_blockhash_cache_task_if_needed(
            &config,
            &live_tx_sender,
            Arc::clone(&primary_rpc_client),
            shadow_rpc_client.clone(),
            Arc::clone(&live_blockhash_cache),
        );
        let cached_payer = match Self::load_configured_payer(&config) {
            Ok(payer) => payer,
            Err(err) if Self::requires_configured_payer(&config) => {
                panic!("TriggerComponent startup failed: {err}")
            }
            Err(err) => {
                warn!(
                    error = %err,
                    payer_strategy = Self::shadow_payer_strategy(&config).as_str(),
                    "Trigger: ignoring configured payer load failure because shadow_only uses local ephemeral payer"
                );
                None
            }
        };
        let cached_shadow_ephemeral_payer = (Self::shadow_payer_strategy(&config)
            == TriggerShadowPayerStrategy::Ephemeral)
            .then(|| Arc::new(Keypair::new()));
        Self {
            config,
            cached_payer,
            cached_shadow_ephemeral_payer,
            ata_rent_cache: Arc::new(RwLock::new(HashMap::new())),
            primary_rpc_client,
            shadow_rpc_client,
            live_tx_sender,
            live_blockhash_cache,
            shadow_ledger,
            account_state_core,
            shadow_simulator,
            shadow_run_semaphore: Arc::new(Semaphore::new(shadow_max_concurrent)),
            position_limit_tracker,
            #[cfg(test)]
            prepared_request_invocations: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Exposes the trigger entry mode for startup logs and tests.
    pub fn entry_mode(&self) -> TriggerEntryMode {
        self.config.entry_mode
    }

    pub fn shadow_run_output_path(&self) -> &str {
        &self.config.shadow_run.output_path
    }

    pub fn shadow_run_emit_event_bus(&self) -> bool {
        self.config.shadow_run.emit_event_bus
    }

    pub fn shadow_run_timeout_ms(&self) -> u64 {
        self.config.shadow_run.timeout_ms
    }

    pub fn shadow_run_enabled(&self) -> bool {
        self.config.shadow_run.enabled
    }

    pub fn supports_shadow_run(&self) -> bool {
        matches!(
            self.config.entry_mode,
            TriggerEntryMode::ShadowOnly | TriggerEntryMode::LiveAndShadow
        ) && self.config.shadow_run.enabled
    }

    pub fn position_limit_tracker(&self) -> PositionLimitTracker {
        self.position_limit_tracker.clone()
    }

    pub fn active_positions(&self) -> usize {
        self.position_limit_tracker.active_positions()
    }

    pub fn release_position_slot(&self, slot_id: PositionSlotId) -> bool {
        self.position_limit_tracker.release(slot_id)
    }

    #[cfg(test)]
    pub fn shadow_available_permits(&self) -> usize {
        self.shadow_run_semaphore.available_permits()
    }

    #[cfg(test)]
    pub fn prepared_request_invocations(&self) -> u64 {
        self.prepared_request_invocations
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Estimate trade value (SOL) for tip sizing.
    pub fn estimate_trade_value_sol(&self, initial_liquidity_sol: Option<f64>) -> f64 {
        let configured_size = self.config.max_position_size_sol.max(0.0);
        match initial_liquidity_sol {
            Some(liquidity) if liquidity.is_finite() && liquidity > 0.0 => {
                configured_size.min(liquidity)
            }
            _ => configured_size,
        }
    }

    pub async fn resolve_live_buy_tip(
        &self,
        trade_value_sol: f64,
        urgency: f64,
    ) -> BuyTipResolution {
        if let Some(sender) = self.live_tx_sender.as_ref() {
            let resolution = sender.resolve_buy_tip_lamports_with_telemetry().await;
            info!(
                tip_lamports = resolution.tip_lamports,
                baseline_tip_lamports = HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS,
                tip_floor_cache_hit = resolution.telemetry.cache_hit,
                tip_floor_cache_age_ms = resolution.telemetry.cache_age_ms,
                tip_floor_fetch_latency_ms = resolution.telemetry.fetch_latency_ms,
                tip_floor_cache_mode = resolution.telemetry.cache_mode,
                tip_floor_source = resolution.telemetry.source,
                tip_floor_inflight_join_result = resolution.telemetry.inflight_join_result,
                tip_floor_inflight_wait_ms = resolution.telemetry.inflight_wait_ms,
                trade_value_sol = trade_value_sol.max(0.0),
                urgency = urgency.clamp(0.0, 1.0),
                "Trigger: resolved BUY tip from Helius Sender policy"
            );
            return resolution;
        }

        warn!(
            trade_value_sol = trade_value_sol.max(0.0),
            urgency = urgency.clamp(0.0, 1.0),
            "Trigger: live BUY tip resolution refused legacy fallback because Sender transport is unavailable"
        );
        BuyTipResolution {
            tip_lamports: 0,
            telemetry: TipFloorResolutionTelemetry {
                cache_mode: "sender_unavailable",
                source: "sender_unavailable",
                ..TipFloorResolutionTelemetry::default()
            },
        }
    }

    pub async fn resolve_live_buy_tip_lamports(&self, trade_value_sol: f64, urgency: f64) -> u64 {
        self.resolve_live_buy_tip(trade_value_sol, urgency)
            .await
            .tip_lamports
    }

    fn advisory_prewarm_scope(
        advisory: &TriggerPrewarmAdvisory,
    ) -> (&'static str, &'static str, &'static str) {
        match advisory {
            TriggerPrewarmAdvisory::TipFloor => ("tip_floor", "early", "current_buy"),
            TriggerPrewarmAdvisory::BuyPriorityFee { .. } => {
                ("priority_fee", "late", "current_or_next_buy")
            }
        }
    }

    fn record_advisory_prewarm_started(
        kind: &'static str,
        hook_phase: &'static str,
        benefit_scope: &'static str,
    ) {
        metrics::counter!(
            "trigger_buy_advisory_prewarm_total",
            1u64,
            "kind" => kind,
            "hook_phase" => hook_phase,
            "benefit_scope" => benefit_scope,
            "result" => "started",
            "cache_mode" => "not_started",
            "source" => "not_started"
        );
    }

    fn record_advisory_prewarm_result(
        kind: &'static str,
        hook_phase: &'static str,
        benefit_scope: &'static str,
        result: &'static str,
        cache_mode: &'static str,
        source: &'static str,
        cache_age_ms: u64,
        fetch_latency_ms: u64,
    ) {
        metrics::counter!(
            "trigger_buy_advisory_prewarm_total",
            1u64,
            "kind" => kind,
            "hook_phase" => hook_phase,
            "benefit_scope" => benefit_scope,
            "result" => result,
            "cache_mode" => cache_mode,
            "source" => source
        );
        metrics::histogram!(
            "trigger_buy_advisory_prewarm_cache_age_ms",
            cache_age_ms as f64,
            "kind" => kind,
            "hook_phase" => hook_phase,
            "benefit_scope" => benefit_scope,
            "source" => source
        );
        metrics::histogram!(
            "trigger_buy_advisory_prewarm_fetch_latency_ms",
            fetch_latency_ms as f64,
            "kind" => kind,
            "hook_phase" => hook_phase,
            "benefit_scope" => benefit_scope,
            "source" => source
        );
    }

    fn spawn_tip_floor_prewarm_result_task(
        kind: &'static str,
        hook_phase: &'static str,
        benefit_scope: &'static str,
        receiver: tokio::sync::oneshot::Receiver<BuyTipResolution>,
    ) {
        tokio::spawn(async move {
            match receiver.await {
                Ok(resolution) => {
                    let result = if resolution.telemetry.source == "sender_fixed_tip" {
                        "skipped"
                    } else if resolution.telemetry.cache_hit {
                        "hit"
                    } else {
                        "miss"
                    };
                    Self::record_advisory_prewarm_result(
                        kind,
                        hook_phase,
                        benefit_scope,
                        result,
                        resolution.telemetry.cache_mode,
                        resolution.telemetry.source,
                        resolution.telemetry.cache_age_ms,
                        resolution.telemetry.fetch_latency_ms,
                    );
                    info!(
                        kind,
                        hook_phase,
                        benefit_scope,
                        tip_lamports = resolution.tip_lamports,
                        cache_hit = resolution.telemetry.cache_hit,
                        cache_mode = resolution.telemetry.cache_mode,
                        source = resolution.telemetry.source,
                        cache_age_ms = resolution.telemetry.cache_age_ms,
                        fetch_latency_ms = resolution.telemetry.fetch_latency_ms,
                        "Trigger: advisory prewarm completed"
                    );
                }
                Err(error) => {
                    warn!(
                        kind,
                        hook_phase,
                        benefit_scope,
                        error = %error,
                        "Trigger: tip-floor advisory prewarm task dropped before publishing a result"
                    );
                    Self::record_advisory_prewarm_result(
                        kind,
                        hook_phase,
                        benefit_scope,
                        "error",
                        "task_cancelled",
                        "task_cancelled",
                        0,
                        0,
                    );
                }
            }
        });
    }

    fn spawn_priority_fee_prewarm_result_task(
        mint: Pubkey,
        buy_variant: &'static str,
        token_program: Pubkey,
        ata_missing_pre_submit: bool,
        has_inline_tip: bool,
        kind: &'static str,
        hook_phase: &'static str,
        benefit_scope: &'static str,
        receiver: tokio::sync::oneshot::Receiver<PriorityFeeEstimate>,
    ) {
        tokio::spawn(async move {
            match receiver.await {
                Ok(estimate) => {
                    let result = if estimate.telemetry.cache_hit {
                        "hit"
                    } else {
                        "miss"
                    };
                    Self::record_advisory_prewarm_result(
                        kind,
                        hook_phase,
                        benefit_scope,
                        result,
                        estimate.telemetry.cache_mode,
                        estimate.telemetry.source,
                        estimate.telemetry.cache_age_ms,
                        estimate.telemetry.fetch_latency_ms,
                    );
                    info!(
                        mint = %mint,
                        buy_variant,
                        token_program = %token_program,
                        ata_missing_pre_submit,
                        has_inline_tip,
                        priority_fee_micro_lamports = estimate.micro_lamports,
                        cache_hit = estimate.telemetry.cache_hit,
                        cache_mode = estimate.telemetry.cache_mode,
                        source = estimate.telemetry.source,
                        cache_age_ms = estimate.telemetry.cache_age_ms,
                        fetch_latency_ms = estimate.telemetry.fetch_latency_ms,
                        kind,
                        hook_phase,
                        benefit_scope,
                        "Trigger: advisory prewarm completed"
                    );
                }
                Err(error) => {
                    warn!(
                        mint = %mint,
                        buy_variant,
                        token_program = %token_program,
                        ata_missing_pre_submit,
                        has_inline_tip,
                        kind,
                        hook_phase,
                        benefit_scope,
                        error = %error,
                        "Trigger: priority-fee advisory prewarm task dropped before publishing a result"
                    );
                    Self::record_advisory_prewarm_result(
                        kind,
                        hook_phase,
                        benefit_scope,
                        "error",
                        "task_cancelled",
                        "task_cancelled",
                        0,
                        0,
                    );
                }
            }
        });
    }

    pub async fn spawn_prewarm_advisory(
        self: &Arc<Self>,
        advisory: TriggerPrewarmAdvisory,
    ) -> bool {
        if self.live_tx_sender.is_none() {
            let (kind, hook_phase, benefit_scope) = Self::advisory_prewarm_scope(&advisory);
            Self::record_advisory_prewarm_result(
                kind,
                hook_phase,
                benefit_scope,
                "skipped",
                "skipped_no_sender",
                "sender_unavailable",
                0,
                0,
            );
            return false;
        }

        let (kind, hook_phase, benefit_scope) = Self::advisory_prewarm_scope(&advisory);
        Self::record_advisory_prewarm_started(kind, hook_phase, benefit_scope);
        self.run_prewarm_advisory(advisory).await;
        true
    }

    async fn run_prewarm_advisory(&self, advisory: TriggerPrewarmAdvisory) {
        let (kind, hook_phase, benefit_scope) = Self::advisory_prewarm_scope(&advisory);
        match advisory {
            TriggerPrewarmAdvisory::TipFloor => {
                let sender = self
                    .live_tx_sender
                    .as_ref()
                    .cloned()
                    .expect("prewarm sender checked before advisory launch");
                let receiver = sender.start_buy_tip_floor_prewarm_with_telemetry().await;
                Self::spawn_tip_floor_prewarm_result_task(
                    kind,
                    hook_phase,
                    benefit_scope,
                    receiver,
                );
            }
            TriggerPrewarmAdvisory::BuyPriorityFee {
                mint,
                account_overrides,
                tip_lamports,
            } => {
                let sender = self
                    .live_tx_sender
                    .as_ref()
                    .cloned()
                    .expect("prewarm sender checked before advisory launch");
                let ResolvedTriggerPayer { payer, .. } = match self.load_payer() {
                    Ok(payer) => payer,
                    Err(error) => {
                        warn!(
                            mint = %mint,
                            error = %error,
                            "Trigger: advisory priority-fee prewarm skipped because payer is unavailable"
                        );
                        Self::record_advisory_prewarm_result(
                            kind,
                            hook_phase,
                            benefit_scope,
                            "skipped",
                            "skipped_missing_payer",
                            "payer_unavailable",
                            0,
                            0,
                        );
                        return;
                    }
                };
                let amount_lamports = match self.configured_trade_amount_lamports() {
                    Ok(amount_lamports) => amount_lamports,
                    Err(error) => {
                        warn!(
                            mint = %mint,
                            error = %error,
                            "Trigger: advisory priority-fee prewarm skipped because configured trade size is invalid"
                        );
                        Self::record_advisory_prewarm_result(
                            kind,
                            hook_phase,
                            benefit_scope,
                            "skipped",
                            "skipped_zero_trade_amount",
                            "invalid_trade_amount",
                            0,
                            0,
                        );
                        return;
                    }
                };
                let mut sanitized_overrides = account_overrides.clone();
                sanitized_overrides.global_config =
                    Self::sanitize_global_config_override(sanitized_overrides.global_config);
                sanitized_overrides.fee_recipient =
                    Self::sanitize_fee_recipient_override(sanitized_overrides.fee_recipient);
                sanitized_overrides.buy_variant =
                    Self::sanitize_buy_variant_override(sanitized_overrides.buy_variant);
                let Some(token_program) = sanitized_overrides.token_program else {
                    warn!(
                        mint = %mint,
                        "Trigger: advisory priority-fee prewarm skipped because token_program metadata is unavailable"
                    );
                    Self::record_advisory_prewarm_result(
                        kind,
                        hook_phase,
                        benefit_scope,
                        "skipped",
                        "skipped_missing_token_program",
                        "metadata_incomplete",
                        0,
                        0,
                    );
                    return;
                };
                let buy_variant = sanitized_overrides
                    .buy_variant
                    .unwrap_or(trigger::PumpfunBuyVariant::RoutedExactSolIn);
                let build_profile = match self.create_buy_build_profile(
                    &payer.pubkey(),
                    &mint,
                    &token_program,
                    true,
                    &sanitized_overrides,
                    amount_lamports,
                    false,
                    None,
                ) {
                    Ok(build_profile) => build_profile,
                    Err(error) => {
                        warn!(
                            mint = %mint,
                            buy_variant = buy_variant.as_str(),
                            error = %error,
                            "Trigger: advisory priority-fee prewarm skipped because representative BUY profile could not be built"
                        );
                        Self::record_advisory_prewarm_result(
                            kind,
                            hook_phase,
                            benefit_scope,
                            "error",
                            "build_failed",
                            "build_failed",
                            0,
                            0,
                        );
                        return;
                    }
                };
                let recent_blockhash = match self
                    .resolve_live_blockhash(self.preparation_rpc())
                    .await
                {
                    Ok((snapshot, _)) => snapshot.blockhash,
                    Err(error) => {
                        warn!(
                            mint = %mint,
                            error = %error,
                            "Trigger: advisory priority-fee prewarm could not refresh live blockhash; falling back to synthetic blockhash"
                        );
                        Hash::new_unique()
                    }
                };
                let tip_seed = format!("{mint}:{recent_blockhash}");
                let tip_account = sender.select_tip_account(tip_seed.as_bytes());
                let (_, representative_buy_tx) = match self.build_buy_transaction_from_profile(
                    payer.as_ref(),
                    &build_profile,
                    HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
                    &tip_account,
                    tip_lamports,
                    recent_blockhash,
                ) {
                    Ok(transactions) => transactions,
                    Err(error) => {
                        warn!(
                            mint = %mint,
                            buy_variant = build_profile.buy_variant.as_str(),
                            error = %error,
                            "Trigger: advisory priority-fee prewarm skipped because representative BUY tx build failed"
                        );
                        Self::record_advisory_prewarm_result(
                            kind,
                            hook_phase,
                            benefit_scope,
                            "error",
                            "build_failed",
                            "build_failed",
                            0,
                            0,
                        );
                        return;
                    }
                };
                let cache_key = PriorityFeeCacheKey::buy(
                    build_profile.buy_variant.as_str(),
                    token_program,
                    false,
                    tip_lamports > 0,
                );
                let receiver = sender
                    .start_buy_priority_fee_prewarm_with_telemetry(representative_buy_tx, cache_key)
                    .await;
                Self::spawn_priority_fee_prewarm_result_task(
                    mint,
                    build_profile.buy_variant.as_str(),
                    token_program,
                    false,
                    tip_lamports > 0,
                    kind,
                    hook_phase,
                    benefit_scope,
                    receiver,
                );
            }
        }
    }

    async fn start_current_buy_priority_fee_prewarm(
        &self,
        build_profile: &BuyBuildProfile,
        tip_lamports: u64,
        cache_key: &PriorityFeeCacheKey,
        probe_buy_tx: VersionedTransaction,
    ) {
        let Some(sender) = self.live_tx_sender.as_ref().cloned() else {
            return;
        };
        let kind = "priority_fee";
        let hook_phase = "late";
        let benefit_scope = "current_buy";
        Self::record_advisory_prewarm_started(kind, hook_phase, benefit_scope);
        let receiver = sender
            .start_buy_priority_fee_prewarm_with_telemetry(probe_buy_tx, cache_key.clone())
            .await;
        Self::spawn_priority_fee_prewarm_result_task(
            build_profile.mint,
            build_profile.buy_variant.as_str(),
            build_profile.token_program,
            build_profile.ata_missing_pre_submit,
            tip_lamports > 0,
            kind,
            hook_phase,
            benefit_scope,
            receiver,
        );
    }

    fn load_payer(&self) -> Result<ResolvedTriggerPayer> {
        match Self::shadow_payer_strategy(&self.config) {
            TriggerShadowPayerStrategy::Configured => self
                .cached_payer
                .as_ref()
                .cloned()
                .map(|payer| ResolvedTriggerPayer {
                    payer,
                    provenance: Self::payer_provenance_label(
                        TriggerShadowPayerStrategy::Configured,
                    ),
                    requires_balance_preflight: true,
                })
                .ok_or_else(|| anyhow::anyhow!("Trigger keypair_path is not configured")),
            TriggerShadowPayerStrategy::Ephemeral => self
                .cached_shadow_ephemeral_payer
                .as_ref()
                .cloned()
                .map(|payer| ResolvedTriggerPayer {
                    payer,
                    provenance: Self::payer_provenance_label(TriggerShadowPayerStrategy::Ephemeral),
                    requires_balance_preflight: false,
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("Trigger ephemeral shadow payer is not initialized")
                }),
        }
    }

    fn configured_trade_amount_lamports(&self) -> Result<u64> {
        let amount = (self.config.max_position_size_sol.max(0.0) * LAMPORTS_PER_SOL).round() as u64;
        if amount == 0 {
            bail!("Configured max_position_size_sol results in zero lamports");
        }
        Ok(amount)
    }

    fn configured_buy_slippage_bps(&self) -> u64 {
        let tolerance = if self.config.slippage_tolerance.is_finite() {
            self.config.slippage_tolerance.clamp(0.0, 1.0)
        } else {
            0.0
        };
        (tolerance * 10_000.0).round() as u64
    }

    fn safety_config(&self) -> SafetyConfig {
        SafetyConfig {
            emergency_floor_sol: self.config.emergency_floor_sol.max(0.0),
            position_size_buffer_sol: self.config.position_size_buffer_sol.max(0.0),
            max_position_size_sol: self.config.max_position_size_sol.max(0.0),
        }
    }

    fn effective_tip_lamports(&self, requested_tip_lamports: u64, trade_value_sol: f64) -> u64 {
        if requested_tip_lamports == 0 {
            return 0;
        }

        if self.live_tx_sender.is_some() {
            return requested_tip_lamports.max(HELIUS_SENDER_MIN_TIP_LAMPORTS);
        }

        let requested_tip_sol = requested_tip_lamports as f64 / LAMPORTS_PER_SOL;
        let capped_tip_sol = calculate_safe_tip(
            requested_tip_sol,
            trade_value_sol.max(0.0),
            &TipGuardConfig {
                max_tip_absolute_sol: self.config.tip_guard.max_tip_absolute_sol,
                fallback_tip_sol: self.config.tip_guard.fallback_tip_sol,
            },
        )
        .min(requested_tip_sol);

        (capped_tip_sol * LAMPORTS_PER_SOL).round() as u64
    }

    fn resolve_safe_trade_budget(
        &self,
        payer_balance_lamports: u64,
        requested_tip_lamports: u64,
    ) -> Result<(u64, u64)> {
        let current_balance_sol = payer_balance_lamports as f64 / LAMPORTS_PER_SOL;
        let safety_config = self.safety_config();
        let safe_trade_sol = resolve_safe_trade_amount(current_balance_sol, &safety_config)?;
        let amount_lamports = (safe_trade_sol * LAMPORTS_PER_SOL).round() as u64;
        if amount_lamports == 0 {
            bail!(SafetyViolation::NoSafeTradeCapacity {
                current_balance: current_balance_sol,
                required_reserve: safety_config.emergency_floor_sol
                    + safety_config.position_size_buffer_sol,
            });
        }

        let effective_tip_lamports =
            self.effective_tip_lamports(requested_tip_lamports, safe_trade_sol);
        let configured_amount_lamports = self.configured_trade_amount_lamports()?;
        if amount_lamports < configured_amount_lamports {
            info!(
                configured_amount_lamports,
                effective_amount_lamports = amount_lamports,
                balance_lamports = payer_balance_lamports,
                current_balance_sol,
                emergency_floor_sol = safety_config.emergency_floor_sol,
                position_size_buffer_sol = safety_config.position_size_buffer_sol,
                "Trigger: bulkhead clipped BUY amount to safe runtime size"
            );
            metrics::counter!(
                "trigger_buy_safety_clamped_total",
                1u64,
                "entry_mode" => self.config.entry_mode.as_str()
            );
        }
        if effective_tip_lamports < requested_tip_lamports {
            info!(
                requested_tip_lamports,
                effective_tip_lamports,
                trade_value_sol = safe_trade_sol,
                "Trigger: capped requested BUY tip against actual safe BUY size"
            );
        }

        Ok((amount_lamports, effective_tip_lamports))
    }

    fn record_safety_rejection(&self, violation: &SafetyViolation) {
        warn!(
            reason = violation.reason_code(),
            entry_mode = self.config.entry_mode.as_str(),
            active_positions = self.position_limit_tracker.active_positions(),
            max_concurrent_positions = self.position_limit_tracker.max_positions(),
            "Trigger: BUY rejected by bulkhead safety"
        );
        metrics::counter!(
            "trigger_buy_safety_rejections_total",
            1u64,
            "reason" => violation.reason_code(),
            "entry_mode" => self.config.entry_mode.as_str()
        );
    }

    fn try_reserve_position_slot(
        &self,
        mint: &Pubkey,
        request: &PreparedBuyRequest,
    ) -> Result<ActivePositionLease> {
        self.position_limit_tracker
            .try_acquire(&request.payer_pubkey, mint, request.mint.to_string())
            .map_err(|err| {
                if let Some(violation) = err.downcast_ref::<SafetyViolation>() {
                    self.record_safety_rejection(violation);
                }
                err
            })
    }

    pub(crate) fn build_dispatch_failure_context(
        &self,
        tip_lamports: u64,
    ) -> Option<TriggerDispatchFailureContext> {
        let amount_lamports = self.configured_trade_amount_lamports().ok()?;
        let payer_strategy = Self::shadow_payer_strategy(&self.config);
        let payer_pubkey = match payer_strategy {
            TriggerShadowPayerStrategy::Configured => self
                .cached_payer
                .as_ref()
                .map(|payer| payer.pubkey().to_string()),
            TriggerShadowPayerStrategy::Ephemeral => self
                .cached_shadow_ephemeral_payer
                .as_ref()
                .map(|payer| payer.pubkey().to_string()),
        };
        Some(TriggerDispatchFailureContext {
            join_metadata: ExecutionJoinMetadata::default(),
            amount_lamports,
            tip_lamports,
            decision_ts_ms: Self::now_ms(),
            payer_provenance: Self::payer_provenance_label(payer_strategy),
            payer_pubkey,
        })
    }

    fn ensure_live_sender_transport(&self) -> Result<()> {
        if !matches!(
            self.config.entry_mode,
            TriggerEntryMode::Live | TriggerEntryMode::LiveAndShadow
        ) {
            return Ok(());
        }

        if self.live_tx_sender.is_none() {
            bail!(
                "live BUY dispatch requires initialized Helius Sender + Yellowstone transport; RPC fallback is disabled"
            );
        }

        Ok(())
    }

    fn build_pool_account_overrides(pool: &crate::events::DetectedPool) -> BuyAccountOverrides {
        let creator_pubkey = match Pubkey::from_str(&pool.creator).ok() {
            Some(pubkey) if pubkey != Pubkey::default() => Some(pubkey),
            _ => {
                warn!(
                    pool_amm_id = %pool.pool_amm_id,
                    creator = %pool.creator,
                    "Trigger: invalid or default pool creator pubkey, falling back to default buy account overrides"
                );
                None
            }
        };

        BuyAccountOverrides {
            creator_pubkey,
            ..BuyAccountOverrides::default()
        }
    }

    fn sanitize_fee_recipient_override(fee_recipient: Option<Pubkey>) -> Option<Pubkey> {
        fee_recipient.filter(DirectBuyBuilder::is_authorized_fee_recipient)
    }

    fn sanitize_global_config_override(global_config: Option<Pubkey>) -> Option<Pubkey> {
        let canonical = DirectBuyBuilder::canonical_global_config();
        global_config.filter(|pubkey| *pubkey == canonical)
    }

    fn sanitize_buy_variant_override(
        buy_variant: Option<trigger::PumpfunBuyVariant>,
    ) -> Option<trigger::PumpfunBuyVariant> {
        match buy_variant {
            Some(trigger::PumpfunBuyVariant::RoutedExactSolIn) => {
                Some(trigger::PumpfunBuyVariant::RoutedExactSolIn)
            }
            _ => None,
        }
    }

    fn sanitize_buy_variant_override_for_prepared_request(
        buy_variant: Option<trigger::PumpfunBuyVariant>,
        has_legacy_buy_curve: bool,
    ) -> Option<trigger::PumpfunBuyVariant> {
        match buy_variant {
            Some(trigger::PumpfunBuyVariant::RoutedExactSolIn) => {
                Some(trigger::PumpfunBuyVariant::RoutedExactSolIn)
            }
            Some(trigger::PumpfunBuyVariant::LegacyBuy) if has_legacy_buy_curve => {
                Some(trigger::PumpfunBuyVariant::LegacyBuy)
            }
            _ => None,
        }
    }

    fn sanitize_associated_bonding_curve_override(
        mint: &Pubkey,
        token_program: &Pubkey,
        associated_bonding_curve: Option<Pubkey>,
    ) -> Option<Pubkey> {
        associated_bonding_curve.filter(|candidate| {
            DirectBuyBuilder::validate_associated_bonding_curve(mint, token_program, candidate)
        })
    }

    fn validate_creator_pubkey_for_buy(
        mint: &Pubkey,
        creator_pubkey: Option<Pubkey>,
    ) -> Result<()> {
        match creator_pubkey {
            Some(pubkey) if pubkey != Pubkey::default() => Ok(()),
            _ => bail!(
                "Missing canonical creator_pubkey for trigger buy: mint={} refusing to derive creator_vault from default pubkey",
                mint
            ),
        }
    }

    fn validate_payer_balance_for_buy(
        payer_pubkey: &Pubkey,
        balance_lamports: u64,
        amount_lamports: u64,
        tip_lamports: u64,
    ) -> Result<()> {
        let required_lamports = amount_lamports
            .saturating_add(tip_lamports)
            .saturating_add(BUY_BALANCE_FEE_BUFFER_LAMPORTS);
        if balance_lamports < required_lamports {
            bail!(
                "Insufficient payer balance for trigger buy: payer={} have={} need={} amount={} tip={} fee_buffer={}",
                payer_pubkey,
                balance_lamports,
                required_lamports,
                amount_lamports,
                tip_lamports,
                BUY_BALANCE_FEE_BUFFER_LAMPORTS
            );
        }
        Ok(())
    }

    fn user_ata_account_len(token_program: &Pubkey) -> Result<usize> {
        let token_2022 = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022 program");
        if *token_program == token_2022 {
            ExtensionType::try_calculate_account_len::<SplTokenAccount>(&[
                ExtensionType::ImmutableOwner,
            ])
            .map_err(|e| anyhow::anyhow!("Failed to calculate token-2022 ATA size: {}", e))
        } else {
            Ok(SplTokenAccount::LEN)
        }
    }

    fn cached_user_ata_rent_lamports(&self, token_program: &Pubkey) -> Option<u64> {
        self.ata_rent_cache
            .read()
            .ok()
            .and_then(|cache| cache.get(token_program).copied())
    }

    fn store_cached_user_ata_rent_lamports(&self, token_program: Pubkey, rent_lamports: u64) {
        if let Ok(mut cache) = self.ata_rent_cache.write() {
            cache.insert(token_program, rent_lamports);
        }
    }

    async fn minimum_user_ata_rent_lamports(
        &self,
        rpc: &solana_client::nonblocking::rpc_client::RpcClient,
        token_program: &Pubkey,
    ) -> Result<u64> {
        if let Some(rent_lamports) = self.cached_user_ata_rent_lamports(token_program) {
            return Ok(rent_lamports);
        }

        let account_len = Self::user_ata_account_len(token_program)?;
        let rent_lamports = rpc
            .get_minimum_balance_for_rent_exemption(account_len)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch ATA rent exemption minimum: {}", e))?;
        self.store_cached_user_ata_rent_lamports(*token_program, rent_lamports);
        Ok(rent_lamports)
    }

    fn validate_payer_account_for_fee(
        payer_pubkey: &Pubkey,
        payer_account: &Account,
    ) -> Result<()> {
        if payer_account.owner != system_program::id() {
            bail!(
                "Invalid fee payer account owner for trigger buy: payer={} owner={} executable={}",
                payer_pubkey,
                payer_account.owner,
                payer_account.executable
            );
        }
        if payer_account.executable {
            bail!(
                "Invalid executable fee payer account for trigger buy: payer={}",
                payer_pubkey
            );
        }
        Ok(())
    }

    fn create_buy_build_profile(
        &self,
        payer_pubkey: &Pubkey,
        mint: &Pubkey,
        token_program: &Pubkey,
        attach_idempotent_ata_create: bool,
        account_overrides: &BuyAccountOverrides,
        amount_lamports: u64,
        ata_missing_pre_submit: bool,
        pre_submit_token_balance: Option<u64>,
    ) -> Result<BuyBuildProfile> {
        let buy_variant = account_overrides
            .buy_variant
            .unwrap_or(trigger::PumpfunBuyVariant::RoutedExactSolIn);
        if matches!(buy_variant, trigger::PumpfunBuyVariant::RoutedExactSolIn) {
            Self::validate_creator_pubkey_for_buy(mint, account_overrides.creator_pubkey)?;
        }
        let token_param_role = match buy_variant {
            trigger::PumpfunBuyVariant::LegacyBuy => "token_amount",
            trigger::PumpfunBuyVariant::RoutedExactSolIn => "min_tokens_out",
        };
        let resolved_token_param = self.resolve_buy_instruction_token_param(
            mint,
            buy_variant,
            account_overrides,
            amount_lamports,
        )?;
        let min_tokens_out = resolved_token_param.min_tokens_out;
        info!(
            mint = %mint,
            buy_variant = buy_variant.as_str(),
            amount_lamports,
            token_param_role,
            token_param = min_tokens_out,
            simulated_tokens_out = ?resolved_token_param.entry_token_amount_raw,
            configured_slippage_bps = self.configured_buy_slippage_bps(),
            "Trigger: resolved buy instruction parameters"
        );
        let buy_instruction = DirectBuyBuilder::build_buy_ix_with_accounts(
            payer_pubkey,
            mint,
            token_program,
            account_overrides.global_config,
            account_overrides.fee_recipient,
            account_overrides.creator_pubkey,
            account_overrides.buy_variant,
            account_overrides.associated_bonding_curve,
            amount_lamports,
            min_tokens_out,
        );
        let user_ata =
            get_associated_token_address_with_program_id(payer_pubkey, mint, token_program);
        let ata_instruction = attach_idempotent_ata_create.then(|| {
            create_associated_token_account_idempotent(
                payer_pubkey,
                payer_pubkey,
                mint,
                token_program,
            )
        });

        Ok(BuyBuildProfile {
            mint: *mint,
            payer_pubkey: *payer_pubkey,
            user_ata,
            token_program: *token_program,
            attach_idempotent_ata_create,
            ata_missing_pre_submit,
            account_overrides: account_overrides.clone(),
            amount_lamports,
            trade_value_sol: amount_lamports as f64 / LAMPORTS_PER_SOL,
            pre_submit_token_balance,
            buy_variant,
            entry_token_amount_raw: resolved_token_param.entry_token_amount_raw,
            min_tokens_out,
            token_param_role,
            ata_instruction,
            buy_instruction,
        })
    }

    fn build_buy_transaction_from_profile(
        &self,
        payer: &Keypair,
        build_profile: &BuyBuildProfile,
        priority_fee_micro_lamports: u64,
        tip_account: &Pubkey,
        tip_lamports: u64,
        recent_blockhash: Hash,
    ) -> Result<(Transaction, VersionedTransaction)> {
        if payer.pubkey() != build_profile.payer_pubkey {
            bail!(
                "BUY build profile payer mismatch: expected={} actual={}",
                build_profile.payer_pubkey,
                payer.pubkey()
            );
        }
        let mut instructions = Vec::with_capacity(5);
        instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(
            BUY_COMPUTE_UNIT_LIMIT,
        ));
        instructions.push(ComputeBudgetInstruction::set_compute_unit_price(
            priority_fee_micro_lamports,
        ));
        if let Some(ata_instruction) = build_profile.ata_instruction.clone() {
            instructions.push(ata_instruction);
        }
        instructions.push(build_profile.buy_instruction.clone());
        if tip_lamports > 0 {
            info!(
                payer = %payer.pubkey(),
                tip_account = %tip_account,
                tip_lamports,
                "Trigger: appended inline tip transfer instruction to BUY transaction"
            );
            instructions.push(system_instruction::transfer(
                &payer.pubkey(),
                tip_account,
                tip_lamports,
            ));
        }

        let rpc_buy_tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );
        let message =
            v0::Message::try_compile(&payer.pubkey(), &instructions, &[], recent_blockhash)?;
        let buy_tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &[payer])
            .map_err(|e| anyhow::anyhow!("Failed to sign BUY transaction: {}", e))?;
        Ok((rpc_buy_tx, buy_tx))
    }

    fn build_prepared_buy_request(
        &self,
        payer: &Keypair,
        mint: &Pubkey,
        token_program: &Pubkey,
        ata_missing_pre_submit: bool,
        account_overrides: &BuyAccountOverrides,
        amount_lamports: u64,
        tip_lamports: u64,
        recent_blockhash: Hash,
    ) -> Result<PreparedBuyRequest> {
        let metadata = PreparedBuyRequestBuildMetadata::local(recent_blockhash);
        let tip_seed = format!("{mint}:{}", metadata.recent_blockhash);
        let tip_account = select_sender_tip_account(tip_seed.as_bytes());
        let build_profile = self.create_buy_build_profile(
            &payer.pubkey(),
            mint,
            token_program,
            true,
            account_overrides,
            amount_lamports,
            ata_missing_pre_submit,
            if ata_missing_pre_submit {
                Some(0)
            } else {
                None
            },
        )?;
        self.build_prepared_buy_request_from_profile(
            payer,
            Self::payer_provenance_label(TriggerShadowPayerStrategy::Configured),
            &build_profile,
            tip_lamports,
            HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
            &tip_account,
            metadata,
        )
    }

    fn build_prepared_buy_request_from_profile(
        &self,
        payer: &Keypair,
        payer_provenance: &'static str,
        build_profile: &BuyBuildProfile,
        tip_lamports: u64,
        priority_fee_micro_lamports: u64,
        tip_account: &Pubkey,
        metadata: PreparedBuyRequestBuildMetadata,
    ) -> Result<PreparedBuyRequest> {
        let (rpc_buy_tx, buy_tx) = self.build_buy_transaction_from_profile(
            payer,
            build_profile,
            priority_fee_micro_lamports,
            tip_account,
            tip_lamports,
            metadata.recent_blockhash,
        )?;

        Ok(self.assemble_prepared_buy_request_from_profile(
            payer_provenance,
            build_profile,
            tip_lamports,
            priority_fee_micro_lamports,
            metadata,
            rpc_buy_tx,
            buy_tx,
        ))
    }

    fn assemble_prepared_buy_request_from_profile(
        &self,
        payer_provenance: &'static str,
        build_profile: &BuyBuildProfile,
        tip_lamports: u64,
        priority_fee_micro_lamports: u64,
        metadata: PreparedBuyRequestBuildMetadata,
        rpc_buy_tx: Transaction,
        buy_tx: VersionedTransaction,
    ) -> PreparedBuyRequest {
        PreparedBuyRequest {
            join_metadata: ExecutionJoinMetadata::default(),
            mint: build_profile.mint,
            payer_pubkey: build_profile.payer_pubkey,
            payer_provenance,
            user_ata: build_profile.user_ata,
            token_program: build_profile.token_program,
            attach_idempotent_ata_create: build_profile.attach_idempotent_ata_create,
            ata_missing_pre_submit: build_profile.ata_missing_pre_submit,
            account_overrides: build_profile.account_overrides.clone(),
            pre_submit_token_balance: build_profile.pre_submit_token_balance,
            amount_lamports: build_profile.amount_lamports,
            trade_value_sol: build_profile.trade_value_sol,
            entry_token_amount_raw: build_profile.entry_token_amount_raw,
            tip_lamports,
            min_tokens_out: build_profile.min_tokens_out,
            priority_fee_micro_lamports,
            recent_blockhash: metadata.recent_blockhash,
            blockhash_source: metadata.blockhash_source,
            blockhash_age_ms: metadata.blockhash_age_ms,
            blockhash_last_valid_block_height: metadata.blockhash_last_valid_block_height,
            blockhash_observed_block_height: metadata.blockhash_observed_block_height,
            blockhash_fetched_at: metadata.blockhash_fetched_at,
            blockhash_fetch_latency_ms: metadata.blockhash_fetch_latency_ms,
            post_blockhash_build_latency_ms: metadata.post_blockhash_build_latency_ms,
            reserve_slot_latency_ms: metadata.reserve_slot_latency_ms,
            shadow_spawn_latency_ms: metadata.shadow_spawn_latency_ms,
            preparation_telemetry: BuyPreparationTelemetry::default(),
            build_profile: Some(build_profile.clone()),
            rpc_buy_tx,
            buy_tx,
            tip_tx: None,
            decision_ts_ms: metadata.decision_ts_ms,
        }
    }

    fn build_prepared_buy_request_with_transport(
        &self,
        payer: &Keypair,
        payer_provenance: &'static str,
        mint: &Pubkey,
        token_program: &Pubkey,
        attach_idempotent_ata_create: bool,
        ata_missing_pre_submit: bool,
        account_overrides: &BuyAccountOverrides,
        amount_lamports: u64,
        tip_lamports: u64,
        priority_fee_micro_lamports: u64,
        pre_submit_token_balance: Option<u64>,
        tip_account: &Pubkey,
        recent_blockhash: Hash,
    ) -> Result<PreparedBuyRequest> {
        let metadata = PreparedBuyRequestBuildMetadata::local(recent_blockhash);
        let build_profile = self.create_buy_build_profile(
            &payer.pubkey(),
            mint,
            token_program,
            attach_idempotent_ata_create,
            account_overrides,
            amount_lamports,
            ata_missing_pre_submit,
            pre_submit_token_balance,
        )?;
        self.build_prepared_buy_request_from_profile(
            payer,
            payer_provenance,
            &build_profile,
            tip_lamports,
            priority_fee_micro_lamports,
            tip_account,
            metadata,
        )
    }

    async fn rebuild_prepared_buy_request_for_retry(
        &self,
        request: &PreparedBuyRequest,
    ) -> Result<PreparedBuyRequest> {
        let payer_load_started_at = Instant::now();
        let ResolvedTriggerPayer {
            payer,
            provenance: payer_provenance,
            ..
        } = self.load_payer()?;
        let payer_load_ms = saturating_elapsed_ms(payer_load_started_at);
        let rpc = self.preparation_rpc();
        let blockhash_fetch_started_at = Instant::now();
        let (blockhash_snapshot, blockhash_source) = self.resolve_live_blockhash(rpc).await?;
        let blockhash_fetch_latency_ms = saturating_elapsed_ms(blockhash_fetch_started_at);
        let tip_seed = format!("{}:{}", request.mint, blockhash_snapshot.blockhash);
        let tip_account = self
            .live_tx_sender
            .as_ref()
            .map(|sender| sender.select_tip_account(tip_seed.as_bytes()))
            .unwrap_or_else(|| select_sender_tip_account(tip_seed.as_bytes()));
        let build_started_at = Instant::now();
        let next_priority_fee_micro_lamports = request
            .priority_fee_micro_lamports
            .saturating_add(BUY_RETRY_PRIORITY_FEE_INCREMENT_MICRO_LAMPORTS);
        let next_tip_lamports = request
            .tip_lamports
            .saturating_add(BUY_RETRY_TIP_INCREMENT_LAMPORTS);
        let build_profile = request.build_profile.clone().unwrap_or_else(|| {
            self.create_buy_build_profile(
                &request.payer_pubkey,
                &request.mint,
                &request.token_program,
                request.attach_idempotent_ata_create,
                &request.account_overrides,
                request.amount_lamports,
                request.ata_missing_pre_submit,
                request.pre_submit_token_balance,
            )
            .expect("compat buy build profile")
        });

        let rebuild_started_at = Instant::now();
        let (rpc_buy_tx, buy_tx) = self.build_buy_transaction_from_profile(
            &payer,
            &build_profile,
            next_priority_fee_micro_lamports,
            &tip_account,
            next_tip_lamports,
            blockhash_snapshot.blockhash,
        )?;
        let rebuild_ms = saturating_elapsed_ms(rebuild_started_at);
        let rebuilt_metadata = PreparedBuyRequestBuildMetadata {
            reserve_slot_latency_ms: request.reserve_slot_latency_ms,
            shadow_spawn_latency_ms: request.shadow_spawn_latency_ms,
            decision_ts_ms: request.decision_ts_ms,
            ..PreparedBuyRequestBuildMetadata::live(
                &blockhash_snapshot,
                blockhash_source,
                blockhash_fetch_latency_ms,
                saturating_elapsed_ms(build_started_at),
                request.decision_ts_ms,
            )
        };
        let mut rebuilt = self.assemble_prepared_buy_request_from_profile(
            payer_provenance,
            &build_profile,
            next_tip_lamports,
            next_priority_fee_micro_lamports,
            rebuilt_metadata,
            rpc_buy_tx,
            buy_tx,
        );
        rebuilt.preparation_telemetry = request.preparation_telemetry.clone();
        rebuilt.preparation_telemetry.payer_load_ms = payer_load_ms;
        rebuilt.preparation_telemetry.rebuild_ms = rebuild_ms;
        metrics::histogram!(
            "payer_load_ms",
            rebuilt.preparation_telemetry.payer_load_ms as f64
        );
        metrics::histogram!(
            "rebuild_ms",
            rebuilt.preparation_telemetry.rebuild_ms as f64
        );
        Self::log_buy_preparation_breakdown(&rebuilt);
        Ok(rebuilt)
    }

    fn record_initial_buy_preparation_metrics(telemetry: &BuyPreparationTelemetry) {
        metrics::histogram!("payer_load_ms", telemetry.payer_load_ms as f64);
        metrics::histogram!(
            "payer_balance_fetch_ms",
            telemetry.payer_balance_fetch_ms as f64
        );
        metrics::histogram!(
            "payer_account_fetch_ms",
            telemetry.payer_account_fetch_ms as f64
        );
        metrics::histogram!(
            "mint_account_fetch_ms",
            telemetry.mint_account_fetch_ms as f64
        );
        metrics::histogram!(
            "token_balance_probe_ms",
            telemetry.token_balance_probe_ms as f64
        );
        metrics::histogram!("ata_rent_fetch_ms", telemetry.ata_rent_fetch_ms as f64);
        metrics::histogram!("build_once_ms", telemetry.build_once_ms as f64);
        metrics::histogram!("rebuild_ms", telemetry.rebuild_ms as f64);
        metrics::counter!(
            "trigger_buy_prewarm_join_total",
            1u64,
            "kind" => "tip_floor",
            "result" => telemetry.tip_floor_inflight_join_result
        );
        metrics::histogram!(
            "trigger_buy_prewarm_wait_ms",
            telemetry.tip_floor_inflight_wait_ms as f64,
            "kind" => "tip_floor",
            "result" => telemetry.tip_floor_inflight_join_result
        );
        metrics::counter!(
            "trigger_buy_prewarm_join_total",
            1u64,
            "kind" => "priority_fee",
            "result" => telemetry.priority_fee_inflight_join_result
        );
        metrics::histogram!(
            "trigger_buy_prewarm_wait_ms",
            telemetry.priority_fee_inflight_wait_ms as f64,
            "kind" => "priority_fee",
            "result" => telemetry.priority_fee_inflight_join_result
        );
        metrics::counter!(
            "trigger_buy_token_program_validation_total",
            1u64,
            "override_present" => if telemetry.token_program_override_present {
                "true"
            } else {
                "false"
            },
            "proof_result" => telemetry.token_program_proof_result,
            "source" => telemetry.token_program_source
        );
    }

    fn log_buy_preparation_breakdown(request: &PreparedBuyRequest) {
        let telemetry = &request.preparation_telemetry;
        info!(
            mint = %request.mint,
            payer = %request.payer_pubkey,
            attach_idempotent_ata_create = request.attach_idempotent_ata_create,
            ata_missing_pre_submit = request.ata_missing_pre_submit,
            tip_lamports = request.tip_lamports,
            priority_fee_micro_lamports = request.priority_fee_micro_lamports,
            tip_floor_cache_hit = telemetry.tip_floor_cache_hit,
            tip_floor_cache_age_ms = telemetry.tip_floor_cache_age_ms,
            tip_floor_fetch_latency_ms = telemetry.tip_floor_fetch_latency_ms,
            tip_floor_cache_mode = telemetry.tip_floor_cache_mode,
            tip_floor_source = telemetry.tip_floor_source,
            tip_floor_inflight_join_result = telemetry.tip_floor_inflight_join_result,
            tip_floor_inflight_wait_ms = telemetry.tip_floor_inflight_wait_ms,
            token_program_override_present = telemetry.token_program_override_present,
            token_program_proof_result = telemetry.token_program_proof_result,
            token_program_source = telemetry.token_program_source,
            priority_fee_cache_hit = telemetry.priority_fee_cache_hit,
            priority_fee_cache_age_ms = telemetry.priority_fee_cache_age_ms,
            priority_fee_fetch_latency_ms = telemetry.priority_fee_fetch_latency_ms,
            priority_fee_cache_mode = telemetry.priority_fee_cache_mode,
            priority_fee_source = telemetry.priority_fee_source,
            priority_fee_inflight_join_result = telemetry.priority_fee_inflight_join_result,
            priority_fee_inflight_wait_ms = telemetry.priority_fee_inflight_wait_ms,
            payer_load_ms = telemetry.payer_load_ms,
            payer_balance_fetch_ms = telemetry.payer_balance_fetch_ms,
            payer_account_fetch_ms = telemetry.payer_account_fetch_ms,
            mint_account_fetch_ms = telemetry.mint_account_fetch_ms,
            token_balance_probe_ms = telemetry.token_balance_probe_ms,
            ata_rent_fetch_ms = telemetry.ata_rent_fetch_ms,
            build_once_ms = telemetry.build_once_ms,
            rebuild_ms = telemetry.rebuild_ms,
            "Trigger: BUY preparation breakdown"
        );
    }

    fn log_live_buy_attempt_summary(
        mint: &Pubkey,
        request: &PreparedBuyRequest,
        tracked_signatures: &[Signature],
        attempt_number: usize,
        result: &str,
        confirm_source: Option<&str>,
        next_action: &str,
        detail: Option<&str>,
    ) {
        let buy_summary = format!(
            "attempt={attempt_number} result={result} next_action={next_action} confirm_source={} tracked_signature_count={} tip_lamports={} priority_fee_micro_lamports={} blockhash_source={} blockhash_age_ms={} last_valid_block_height={} observed_block_height={} recent_blockhash={}",
            confirm_source.unwrap_or("none"),
            tracked_signatures.len(),
            request.tip_lamports,
            request.priority_fee_micro_lamports,
            request.blockhash_source,
            request.blockhash_age_ms,
            request.blockhash_last_valid_block_height,
            request.blockhash_observed_block_height,
            request.recent_blockhash,
        );

        match result {
            "confirmed" => info!(
                mint = %mint,
                attempt_number,
                tracked_signatures = ?tracked_signatures,
                confirm_source = confirm_source.unwrap_or("none"),
                next_action,
                detail = detail.unwrap_or(""),
                buy_summary = %buy_summary,
                "Trigger: live BUY attempt summary"
            ),
            _ => warn!(
                mint = %mint,
                attempt_number,
                tracked_signatures = ?tracked_signatures,
                confirm_source = confirm_source.unwrap_or("none"),
                next_action,
                detail = detail.unwrap_or(""),
                buy_summary = %buy_summary,
                "Trigger: live BUY attempt summary"
            ),
        }
    }

    fn resolve_buy_instruction_token_param(
        &self,
        mint: &Pubkey,
        buy_variant: trigger::PumpfunBuyVariant,
        account_overrides: &BuyAccountOverrides,
        amount_lamports: u64,
    ) -> Result<ResolvedBuyTokenParam> {
        let slippage_bps = self.configured_buy_slippage_bps();
        let apply_configured_slippage = |tokens_out: u64| -> Result<u64> {
            if tokens_out == 0 {
                bail!(
                    "BUY token simulation returned zero tokens: mint={} amount_lamports={}",
                    mint,
                    amount_lamports
                );
            }
            Ok(apply_slippage_bps(tokens_out, slippage_bps).max(1))
        };
        match buy_variant {
            trigger::PumpfunBuyVariant::RoutedExactSolIn => match account_overrides
                .legacy_buy_curve
                .or_else(|| self.account_state_core.bonding_curve(mint))
            {
                Some(curve) => {
                    let entry_token_amount_raw = curve.simulate_buy(amount_lamports);
                    Ok(ResolvedBuyTokenParam {
                        entry_token_amount_raw: Some(entry_token_amount_raw),
                        min_tokens_out: apply_configured_slippage(entry_token_amount_raw)?,
                    })
                }
                None => Ok(ResolvedBuyTokenParam {
                    entry_token_amount_raw: None,
                    min_tokens_out: 1,
                }),
            },
            trigger::PumpfunBuyVariant::LegacyBuy => {
                let curve = account_overrides.legacy_buy_curve.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing canonical ghost-core curve for legacy_buy trigger buy: mint={} refusing to encode 1 raw token unit",
                        mint
                    )
                })?;
                let entry_token_amount_raw = curve.simulate_buy(amount_lamports);
                Ok(ResolvedBuyTokenParam {
                    entry_token_amount_raw: Some(entry_token_amount_raw),
                    min_tokens_out: apply_configured_slippage(entry_token_amount_raw)?,
                })
            }
        }
    }

    fn resolve_buy_token_program(mint_account_owner: &Pubkey) -> Result<Pubkey> {
        let legacy = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid legacy token program");
        let token_2022 = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022 program");
        if *mint_account_owner == legacy || *mint_account_owner == token_2022 {
            Ok(*mint_account_owner)
        } else {
            bail!(
                "Unsupported mint owner for trigger buy: owner={}",
                mint_account_owner
            );
        }
    }

    fn run_local_buy_preflight(
        &self,
        request: &PreparedBuyRequest,
    ) -> Result<(u64, ghost_core::shadow_ledger::types::BuySimulationResult)> {
        let current_slot = self
            .account_state_core
            .latest_observed_slot()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "AccountStateCore has no canonical slot yet for live BUY mint={}",
                    request.mint
                )
            })?;
        let canonical_state = self
            .account_state_core
            .get_canonical_state(&request.mint)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "AccountStateCore has no canonical state for live BUY mint={}",
                    request.mint
                )
            })?;
        let state_age_slots = current_slot.saturating_sub(canonical_state.last_update_slot);
        let max_state_age_slots = self.config.live_preflight_max_state_age_slots.max(1);
        if state_age_slots > max_state_age_slots {
            bail!(
                "AccountStateCore BUY preflight state is stale: mint={} current_slot={} state_slot={} age_slots={} max_age_slots={}",
                request.mint,
                current_slot,
                canonical_state.last_update_slot,
                state_age_slots,
                max_state_age_slots
            );
        }
        let curve = self
            .account_state_core
            .bonding_curve(&request.mint)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "AccountStateCore could not materialize bonding curve for live BUY mint={}",
                    request.mint
                )
            })?;
        let tokens_out = curve.simulate_buy(request.amount_lamports);
        let simulation = ghost_core::shadow_ledger::types::BuySimulationResult {
            tokens_out,
            min_tokens_out: request.min_tokens_out.min(tokens_out),
            sol_in: request.amount_lamports,
            effective_sol_in: request
                .amount_lamports
                .saturating_sub(request.amount_lamports / 100),
            price_impact_percent: 0.0,
            effective_price_per_token: 0.0,
            market_cap_sol: canonical_state.market_cap_sol.max(0.0) as u64,
            bonding_progress: canonical_state.bonding_curve_progress.max(0.0) as u64,
        };
        if simulation.tokens_out == 0 || simulation.min_tokens_out == 0 {
            bail!(
                "AccountStateCore BUY preflight produced zero output: mint={} current_slot={} tokens_out={} min_tokens_out={}",
                request.mint,
                current_slot,
                simulation.tokens_out,
                simulation.min_tokens_out
            );
        }
        if simulation.tokens_out < request.min_tokens_out {
            bail!(
                "AccountStateCore BUY preflight tokens_out below requested min_tokens_out: mint={} current_slot={} tokens_out={} requested_min_tokens_out={}",
                request.mint,
                current_slot,
                simulation.tokens_out,
                request.min_tokens_out
            );
        }
        Ok((current_slot, simulation))
    }

    async fn submit_prepared_via_sender(
        &self,
        request: PreparedBuyRequest,
    ) -> std::result::Result<
        crate::components::live_tx_sender::SenderConfirmedTransaction,
        SubmitPreparedViaSenderError,
    > {
        let client_setup_started_at = Instant::now();
        let client = self.live_tx_sender.as_ref().ok_or_else(|| {
            SubmitPreparedViaSenderError::Failed(anyhow::anyhow!(
                "live BUY dispatch requires initialized Helius Sender transport"
            ))
        })?;
        let client_setup_latency_ms = saturating_elapsed_ms(client_setup_started_at);
        let mint = request.mint;
        let amount_lamports = request.amount_lamports;
        let tip_lamports = request.tip_lamports;
        let recent_blockhash = request.recent_blockhash;
        let blockhash_fetched_at = request.blockhash_fetched_at;
        let blockhash_fetch_latency_ms = request.blockhash_fetch_latency_ms;
        let post_blockhash_build_latency_ms = request.post_blockhash_build_latency_ms;
        let reserve_slot_latency_ms = request.reserve_slot_latency_ms;
        let shadow_spawn_latency_ms = request.shadow_spawn_latency_ms;

        let local_preflight_started_at = Instant::now();
        let (local_preflight_slot, local_preflight) = match self.run_local_buy_preflight(&request) {
            Ok(result) => result,
            Err(sim_err) => {
                warn!(
                    mint = %mint,
                    amount_lamports,
                    tip_lamports,
                    local_preflight_latency_ms = saturating_elapsed_ms(local_preflight_started_at),
                    error = %sim_err,
                    "BUY local AccountStateCore pre-flight FAILED — aborting Helius Sender submission"
                );
                return Err(SubmitPreparedViaSenderError::Failed(anyhow::anyhow!(
                    "BUY local AccountStateCore pre-flight failed: {}",
                    sim_err
                )));
            }
        };
        let local_preflight_latency_ms = saturating_elapsed_ms(local_preflight_started_at);
        info!(
            mint = %mint,
            current_slot = local_preflight_slot,
            simulated_tokens_out = local_preflight.tokens_out,
            simulated_min_tokens_out = local_preflight.min_tokens_out,
            simulated_price_impact_pct = local_preflight.price_impact_percent,
            "BUY local AccountStateCore pre-flight passed"
        );

        let mut tracked_signatures = vec![request.buy_tx.signatures[0]];
        let slot_read_started_at = Instant::now();
        let submit_slot = self.account_state_core.latest_observed_slot();
        let slot_read_latency_ms = saturating_elapsed_ms(slot_read_started_at);
        let blockhash_to_send_transaction_ms = saturating_elapsed_ms(blockhash_fetched_at);
        metrics::histogram!(
            "trigger_buy_blockhash_fetch_latency_ms",
            blockhash_fetch_latency_ms as f64
        );
        metrics::histogram!(
            "trigger_buy_blockhash_to_send_transaction_ms",
            blockhash_to_send_transaction_ms as f64
        );
        metrics::histogram!(
            "trigger_buy_post_blockhash_build_latency_ms",
            post_blockhash_build_latency_ms as f64
        );
        metrics::histogram!(
            "trigger_buy_local_preflight_latency_ms",
            local_preflight_latency_ms as f64
        );
        metrics::histogram!(
            "trigger_buy_slot_read_latency_ms",
            slot_read_latency_ms as f64
        );
        info!(
            mint = %mint,
            amount_lamports,
            tip_lamports,
            recent_blockhash = %recent_blockhash,
            priority_fee_micro_lamports = request.priority_fee_micro_lamports,
            blockhash_source = request.blockhash_source,
            blockhash_age_ms = request.blockhash_age_ms,
            blockhash_last_valid_block_height = request.blockhash_last_valid_block_height,
            blockhash_observed_block_height = request.blockhash_observed_block_height,
            blockhash_fetch_latency_ms,
            post_blockhash_build_latency_ms,
            reserve_slot_latency_ms,
            shadow_spawn_latency_ms,
            client_setup_latency_ms,
            local_preflight_slot,
            local_preflight_latency_ms,
            slot_read_latency_ms,
            blockhash_to_send_transaction_ms,
            tracked_signature_count = tracked_signatures.len(),
            "Trigger: live BUY blockhash timing before Sender submit"
        );

        let log_confirmed_transaction =
            |confirmed_transaction: crate::components::live_tx_sender::SenderConfirmedTransaction,
             source: &'static str,
             attempt_number: usize,
             confirm_started_at: Instant,
             buy_submitted_at: u64,
             tracked_signatures: &[Signature],
             request: &PreparedBuyRequest| {
                let confirm_latency_ms = saturating_elapsed_ms(confirm_started_at);
                let buy_confirmed_at = Self::now_ms();
                let buy_submit_to_confirm_ms =
                    buy_confirmed_at.saturating_sub(buy_submitted_at);
                let submit_to_landed_slot_delta = confirmed_transaction
                    .landed_slot
                    .zip(submit_slot)
                    .map(|(landed_slot, submit_slot)| landed_slot.saturating_sub(submit_slot));
                let near_leader_slot = submit_to_landed_slot_delta.map(|delta| delta <= 1);
                info!(
                    mint = %mint,
                    buy_signature = %confirmed_transaction.signature,
                    attempt_number,
                    tracked_signatures = ?tracked_signatures,
                    tracked_signature_count = tracked_signatures.len(),
                    submit_slot = ?submit_slot,
                    landed_slot = ?confirmed_transaction.landed_slot,
                    submit_to_landed_slot_delta = ?submit_to_landed_slot_delta,
                    near_leader_slot = ?near_leader_slot,
                    confirm_source = source,
                    buy_submitted_at,
                    buy_confirmed_at,
                    buy_submit_to_confirm_ms,
                    confirm_latency_ms,
                    amount_lamports = request.amount_lamports,
                    tip_lamports = request.tip_lamports,
                    priority_fee_micro_lamports = request.priority_fee_micro_lamports,
                    blockhash_source = request.blockhash_source,
                    blockhash_age_ms = request.blockhash_age_ms,
                    blockhash_last_valid_block_height = request.blockhash_last_valid_block_height,
                    blockhash_observed_block_height = request.blockhash_observed_block_height,
                    "Trigger: live BUY sender telemetry"
                );
                confirmed_transaction
            };
        let mut current_request = request;
        let mut request_history = vec![current_request.clone()];
        for attempt_index in 0..BUY_RETRY_MAX_ATTEMPTS {
            let attempt_number = attempt_index + 1;
            let expected_signature = current_request.buy_tx.signatures[0];
            let buy_submitted_at = Self::now_ms();
            info!(
                mint = %mint,
                attempt_number,
                buy_submitted_at,
                tracked_signatures = ?tracked_signatures,
                tracked_signature_count = tracked_signatures.len(),
                amount_lamports = current_request.amount_lamports,
                tip_lamports = current_request.tip_lamports,
                priority_fee_micro_lamports = current_request.priority_fee_micro_lamports,
                recent_blockhash = %current_request.recent_blockhash,
                blockhash_source = current_request.blockhash_source,
                blockhash_age_ms = current_request.blockhash_age_ms,
                blockhash_last_valid_block_height = current_request.blockhash_last_valid_block_height,
                blockhash_observed_block_height = current_request.blockhash_observed_block_height,
                "Trigger: live BUY pre-submit timing checkpoint"
            );
            let submission = match client.send_transaction(&current_request.buy_tx).await {
                Ok(submission) => submission,
                Err(err) => {
                    let error = anyhow::anyhow!("Helius Sender BUY submission failed: {}", err);
                    let next_action = if attempt_index == 0 {
                        "stop"
                    } else {
                        "retain_slot"
                    };
                    Self::log_live_buy_attempt_summary(
                        &mint,
                        &current_request,
                        &tracked_signatures,
                        attempt_number,
                        "submit_failed",
                        None,
                        next_action,
                        Some(&error.to_string()),
                    );
                    return Err(if attempt_index == 0 {
                        SubmitPreparedViaSenderError::Failed(error)
                    } else {
                        SubmitPreparedViaSenderError::UncertainLanding(error)
                    });
                }
            };
            if submission.signature != expected_signature {
                let error = anyhow::anyhow!(
                    "Helius Sender BUY returned signature mismatch: signed={} returned={}",
                    expected_signature,
                    submission.signature
                );
                Self::log_live_buy_attempt_summary(
                    &mint,
                    &current_request,
                    &tracked_signatures,
                    attempt_number,
                    "submit_failed",
                    None,
                    "retain_slot",
                    Some(&error.to_string()),
                );
                return Err(SubmitPreparedViaSenderError::UncertainLanding(error));
            }
            if !tracked_signatures.contains(&submission.signature) {
                tracked_signatures.push(submission.signature);
            }
            info!(
                mint = %mint,
                attempt_number,
                buy_submitted_at,
                buy_signature = %submission.signature,
                tracked_signatures = ?tracked_signatures,
                tracked_signature_count = tracked_signatures.len(),
                amount_lamports = current_request.amount_lamports,
                tip_lamports = current_request.tip_lamports,
                priority_fee_micro_lamports = current_request.priority_fee_micro_lamports,
                recent_blockhash = %current_request.recent_blockhash,
                blockhash_source = current_request.blockhash_source,
                blockhash_age_ms = current_request.blockhash_age_ms,
                blockhash_last_valid_block_height = current_request.blockhash_last_valid_block_height,
                blockhash_observed_block_height = current_request.blockhash_observed_block_height,
                "Trigger: live BUY submitted via Helius Sender"
            );

            let confirm_started_at = Instant::now();
            let confirm_wait_ms = match attempt_index {
                0 => BUY_RETRY_INITIAL_CONFIRM_WAIT_MS,
                1 => BUY_RETRY_RESEND_CONFIRM_WAIT_MS,
                _ => BUY_CONFIRM_PRIMARY_TIMEOUT_MS,
            };

            match self
                .confirm_sender_buy_attempt(
                    client,
                    &submission,
                    &tracked_signatures,
                    &current_request.user_ata,
                    current_request.pre_submit_token_balance,
                    confirm_wait_ms,
                )
                .await
            {
                SenderBuyAttemptConfirmation::Confirmed {
                    signature,
                    source,
                    landed_slot,
                } => {
                    let confirmed_request = request_history
                        .iter()
                        .find(|candidate| candidate.buy_tx.signatures[0] == signature)
                        .unwrap_or(&current_request);
                    let confirmed_transaction = log_confirmed_transaction(
                        crate::components::live_tx_sender::SenderConfirmedTransaction {
                            signature,
                            landed_slot,
                        },
                        source,
                        attempt_number,
                        confirm_started_at,
                        buy_submitted_at,
                        &tracked_signatures,
                        confirmed_request,
                    );
                    Self::log_live_buy_attempt_summary(
                        &mint,
                        confirmed_request,
                        &tracked_signatures,
                        attempt_number,
                        "confirmed",
                        Some(source),
                        "stop",
                        None,
                    );
                    return Ok(confirmed_transaction);
                }
                SenderBuyAttemptConfirmation::Failed { source, detail } => {
                    Self::log_live_buy_attempt_summary(
                        &mint,
                        &current_request,
                        &tracked_signatures,
                        attempt_number,
                        "failed",
                        Some(source),
                        "stop",
                        Some(&detail),
                    );
                    return Err(SubmitPreparedViaSenderError::Failed(anyhow::anyhow!(
                        "Helius Sender BUY confirmation failed after signature {} via {}: {}",
                        submission.signature,
                        source,
                        detail
                    )));
                }
                SenderBuyAttemptConfirmation::Uncertain => {
                    if attempt_index + 1 == BUY_RETRY_MAX_ATTEMPTS {
                        let error = anyhow::anyhow!(
                            "Helius Sender BUY confirmation remained inconclusive after {} attempts; tracked_signatures={:?}",
                            BUY_RETRY_MAX_ATTEMPTS,
                            tracked_signatures
                        );
                        Self::log_live_buy_attempt_summary(
                            &mint,
                            &current_request,
                            &tracked_signatures,
                            attempt_number,
                            "uncertain",
                            None,
                            "retain_slot",
                            Some(&error.to_string()),
                        );
                        return Err(SubmitPreparedViaSenderError::UncertainLanding(error));
                    }

                    if attempt_index == 0 {
                        Self::log_live_buy_attempt_summary(
                            &mint,
                            &current_request,
                            &tracked_signatures,
                            attempt_number,
                            "uncertain",
                            None,
                            "resend_same_tx",
                            None,
                        );
                        continue;
                    }

                    Self::log_live_buy_attempt_summary(
                        &mint,
                        &current_request,
                        &tracked_signatures,
                        attempt_number,
                        "uncertain",
                        None,
                        "rebuild_retry",
                        None,
                    );
                    current_request = self
                        .rebuild_prepared_buy_request_for_retry(&current_request)
                        .await
                        .map_err(|err| {
                            SubmitPreparedViaSenderError::UncertainLanding(anyhow::anyhow!(
                                "Helius Sender BUY retry rebuild failed while prior signature(s) remained unresolved: {}",
                                err
                            ))
                        })?;
                    if !request_history.iter().any(|candidate| {
                        candidate.buy_tx.signatures == current_request.buy_tx.signatures
                    }) {
                        request_history.push(current_request.clone());
                    }
                }
            }
        }

        Err(SubmitPreparedViaSenderError::UncertainLanding(
            anyhow::anyhow!(
                "Helius Sender BUY confirmation exhausted retry loop without terminal outcome"
            ),
        ))
    }

    pub async fn prepare_buy_request(
        &self,
        mint: &Pubkey,
        account_overrides: &BuyAccountOverrides,
        tip_lamports: u64,
    ) -> Result<PreparedBuyRequest> {
        self.prepare_buy_request_with_tip_telemetry(mint, account_overrides, tip_lamports, None)
            .await
    }

    pub(crate) async fn prepare_buy_request_with_tip_telemetry(
        &self,
        mint: &Pubkey,
        account_overrides: &BuyAccountOverrides,
        tip_lamports: u64,
        tip_floor_telemetry: Option<TipFloorResolutionTelemetry>,
    ) -> Result<PreparedBuyRequest> {
        self.prepare_buy_request_with_tip_telemetry_and_amount_lamports(
            mint,
            account_overrides,
            tip_lamports,
            tip_floor_telemetry,
            None,
        )
        .await
    }

    async fn prepare_buy_request_with_tip_telemetry_and_amount_lamports(
        &self,
        mint: &Pubkey,
        account_overrides: &BuyAccountOverrides,
        tip_lamports: u64,
        tip_floor_telemetry: Option<TipFloorResolutionTelemetry>,
        amount_lamports_override: Option<u64>,
    ) -> Result<PreparedBuyRequest> {
        #[cfg(test)]
        self.prepared_request_invocations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let mut preparation_telemetry = BuyPreparationTelemetry {
            priority_fee_cache_mode: if self.live_tx_sender.is_some() {
                "not_collected"
            } else {
                "static_fallback"
            },
            priority_fee_source: if self.live_tx_sender.is_some() {
                "not_collected"
            } else {
                "static_fallback"
            },
            ..BuyPreparationTelemetry::default()
        };
        if let Some(tip_floor_telemetry) = tip_floor_telemetry {
            preparation_telemetry.tip_floor_cache_hit = tip_floor_telemetry.cache_hit;
            preparation_telemetry.tip_floor_cache_age_ms = tip_floor_telemetry.cache_age_ms;
            preparation_telemetry.tip_floor_fetch_latency_ms = tip_floor_telemetry.fetch_latency_ms;
            preparation_telemetry.tip_floor_cache_mode = tip_floor_telemetry.cache_mode;
            preparation_telemetry.tip_floor_source = tip_floor_telemetry.source;
            preparation_telemetry.tip_floor_inflight_join_result =
                tip_floor_telemetry.inflight_join_result;
            preparation_telemetry.tip_floor_inflight_wait_ms = tip_floor_telemetry.inflight_wait_ms;
        } else {
            if self.live_tx_sender.is_some() {
                preparation_telemetry.tip_floor_cache_mode = "not_provided";
                preparation_telemetry.tip_floor_source = "not_provided";
                preparation_telemetry.tip_floor_inflight_join_result = "not_provided";
            } else {
                preparation_telemetry.tip_floor_cache_mode = "sender_unavailable";
                preparation_telemetry.tip_floor_source = "sender_unavailable";
                preparation_telemetry.tip_floor_inflight_join_result = "sender_unavailable";
            }
        }

        let payer_load_started_at = Instant::now();
        let ResolvedTriggerPayer {
            payer,
            provenance: payer_provenance,
            requires_balance_preflight,
        } = self.load_payer()?;
        preparation_telemetry.payer_load_ms = saturating_elapsed_ms(payer_load_started_at);
        let payer_pubkey = payer.pubkey();
        let rpc = self.preparation_rpc();
        let tip_lamports = if self.live_tx_sender.is_some() {
            tip_lamports.max(HELIUS_SENDER_MIN_TIP_LAMPORTS)
        } else {
            tip_lamports.max(1)
        };
        let mut sanitized_overrides = account_overrides.clone();
        let original_fee_recipient = sanitized_overrides.fee_recipient;
        sanitized_overrides.fee_recipient =
            Self::sanitize_fee_recipient_override(sanitized_overrides.fee_recipient);
        let original_buy_variant = sanitized_overrides.buy_variant;
        sanitized_overrides.buy_variant = Self::sanitize_buy_variant_override_for_prepared_request(
            sanitized_overrides.buy_variant,
            sanitized_overrides.legacy_buy_curve.is_some(),
        );
        let speculative_ata_probe =
            sanitized_overrides
                .token_program
                .map(|override_token_program| {
                    let speculative_user_ata = get_associated_token_address_with_program_id(
                        &payer_pubkey,
                        mint,
                        &override_token_program,
                    );
                    async move {
                        self.probe_user_ata_pre_submit(
                            mint,
                            speculative_user_ata,
                            &override_token_program,
                            rpc,
                        )
                        .await
                        .map(|probe| (override_token_program, probe))
                    }
                });
        let payer_balance_fetch = async {
            let started_at = Instant::now();
            self.fetch_payer_balance_with_retry(&payer_pubkey, rpc)
                .await
                .map(|balance| (balance, saturating_elapsed_ms(started_at)))
                .map_err(|e| anyhow::anyhow!("Failed to fetch payer balance: {}", e))
        };
        let payer_account_fetch = async {
            let started_at = Instant::now();
            self.fetch_payer_account_with_retry(&payer_pubkey, rpc)
                .await
                .map(|account| (account, saturating_elapsed_ms(started_at)))
                .map_err(|e| anyhow::anyhow!("Failed to fetch payer account: {}", e))
        };
        let mint_account_fetch = async {
            let started_at = Instant::now();
            self.fetch_mint_account_with_retry(mint, rpc)
                .await
                .map(|account| (account, saturating_elapsed_ms(started_at)))
                .map_err(|e| anyhow::anyhow!("Failed to fetch mint account: {}", e))
        };
        let (
            amount_lamports,
            effective_tip_lamports,
            payer_balance_lamports,
            mint_account,
            mint_account_fetch_ms,
            speculative_ata_probe_result,
        ) = if requires_balance_preflight {
            let (
                (payer_balance_lamports, payer_balance_fetch_ms),
                (payer_account, payer_account_fetch_ms),
                (mint_account, mint_account_fetch_ms),
                speculative_ata_probe_result,
            ) = if let Some(speculative_ata_probe) = speculative_ata_probe {
                let (payer_balance, payer_account, mint_account, speculative_probe) = tokio::try_join!(
                    payer_balance_fetch,
                    payer_account_fetch,
                    mint_account_fetch,
                    speculative_ata_probe,
                )?;
                (
                    payer_balance,
                    payer_account,
                    mint_account,
                    Some(speculative_probe),
                )
            } else {
                let (payer_balance, payer_account, mint_account) =
                    tokio::try_join!(payer_balance_fetch, payer_account_fetch, mint_account_fetch)?;
                (payer_balance, payer_account, mint_account, None)
            };
            preparation_telemetry.payer_balance_fetch_ms = payer_balance_fetch_ms;
            preparation_telemetry.payer_account_fetch_ms = payer_account_fetch_ms;
            let (amount_lamports, effective_tip_lamports) =
                if let Some(override_lamports) = amount_lamports_override {
                    if override_lamports == 0 {
                        bail!("amount_lamports_override must be positive");
                    }
                    let (max_safe_lamports, _) = self
                        .resolve_safe_trade_budget(payer_balance_lamports, tip_lamports)
                        .map_err(|err| {
                            if let Some(violation) = err.downcast_ref::<SafetyViolation>() {
                                self.record_safety_rejection(violation);
                            }
                            err
                        })?;
                    if override_lamports > max_safe_lamports {
                        let violation = SafetyViolation::TradeAmountExceedsMax {
                            trade_amount: override_lamports as f64 / LAMPORTS_PER_SOL,
                            max_safe: max_safe_lamports as f64 / LAMPORTS_PER_SOL,
                        };
                        self.record_safety_rejection(&violation);
                        bail!(violation);
                    }
                    (
                        override_lamports,
                        self.effective_tip_lamports(
                            tip_lamports,
                            override_lamports as f64 / LAMPORTS_PER_SOL,
                        ),
                    )
                } else {
                    self.resolve_safe_trade_budget(payer_balance_lamports, tip_lamports)
                        .map_err(|err| {
                            if let Some(violation) = err.downcast_ref::<SafetyViolation>() {
                                self.record_safety_rejection(violation);
                            }
                            err
                        })?
                };
            Self::validate_payer_balance_for_buy(
                &payer_pubkey,
                payer_balance_lamports,
                amount_lamports,
                effective_tip_lamports,
            )?;
            Self::validate_payer_account_for_fee(&payer_pubkey, &payer_account)?;
            (
                amount_lamports,
                effective_tip_lamports,
                Some(payer_balance_lamports),
                mint_account,
                mint_account_fetch_ms,
                speculative_ata_probe_result,
            )
        } else if let Some(speculative_ata_probe) = speculative_ata_probe {
            let ((mint_account, mint_account_fetch_ms), speculative_probe) =
                tokio::try_join!(mint_account_fetch, speculative_ata_probe)?;
            (
                amount_lamports_override
                    .map(|amount| {
                        if amount == 0 {
                            bail!("amount_lamports_override must be positive");
                        }
                        Ok(amount)
                    })
                    .unwrap_or_else(|| self.configured_trade_amount_lamports())?,
                tip_lamports,
                None,
                mint_account,
                mint_account_fetch_ms,
                Some(speculative_probe),
            )
        } else {
            let (mint_account, mint_account_fetch_ms) = mint_account_fetch.await?;
            (
                amount_lamports_override
                    .map(|amount| {
                        if amount == 0 {
                            bail!("amount_lamports_override must be positive");
                        }
                        Ok(amount)
                    })
                    .unwrap_or_else(|| self.configured_trade_amount_lamports())?,
                tip_lamports,
                None,
                mint_account,
                mint_account_fetch_ms,
                None,
            )
        };
        preparation_telemetry.mint_account_fetch_ms = mint_account_fetch_ms;
        let original_global_config = sanitized_overrides.global_config;
        sanitized_overrides.global_config =
            Self::sanitize_global_config_override(sanitized_overrides.global_config);
        if original_global_config.is_some() && sanitized_overrides.global_config.is_none() {
            warn!(
                mint = %mint,
                global_config = %original_global_config
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                canonical_global_config = %DirectBuyBuilder::canonical_global_config(),
                "Trigger: dropping noncanonical global_config override"
            );
        }
        if original_fee_recipient.is_some() && sanitized_overrides.fee_recipient.is_none() {
            warn!(
                mint = %mint,
                fee_recipient = %original_fee_recipient
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                canonical_fee_recipient = %DirectBuyBuilder::canonical_fee_recipient(),
                "Trigger: dropping unauthorized fee_recipient override"
            );
        }
        if matches!(
            original_buy_variant,
            Some(trigger::PumpfunBuyVariant::LegacyBuy)
        ) && sanitized_overrides.buy_variant.is_none()
        {
            warn!(
                mint = %mint,
                "Trigger: dropping unverified legacy_buy override and falling back to routed_exact_sol_in"
            );
        }
        preparation_telemetry.token_program_override_present =
            sanitized_overrides.token_program.is_some();
        let canonical_token_program = Self::resolve_buy_token_program(&mint_account.owner)?;
        let token_program = if let Some(token_program) = sanitized_overrides.token_program {
            if token_program != canonical_token_program {
                warn!(
                    mint = %mint,
                    override_token_program = %token_program,
                    canonical_token_program = %canonical_token_program,
                    "Trigger: overriding mismatched token_program from runtime metadata"
                );
                preparation_telemetry.token_program_proof_result = "mismatched";
                preparation_telemetry.token_program_source = "canonical_mint_fetch_after_mismatch";
                canonical_token_program
            } else {
                preparation_telemetry.token_program_proof_result = "matched";
                preparation_telemetry.token_program_source = "runtime_override_validated";
                token_program
            }
        } else {
            preparation_telemetry.token_program_proof_result = "not_provided";
            preparation_telemetry.token_program_source = "canonical_mint_fetch";
            canonical_token_program
        };
        let original_associated_bonding_curve = sanitized_overrides.associated_bonding_curve;
        sanitized_overrides.associated_bonding_curve =
            Self::sanitize_associated_bonding_curve_override(
                mint,
                &token_program,
                sanitized_overrides.associated_bonding_curve,
            );
        if original_associated_bonding_curve.is_some()
            && sanitized_overrides.associated_bonding_curve.is_none()
        {
            warn!(
                mint = %mint,
                token_program = %token_program,
                associated_bonding_curve = %original_associated_bonding_curve
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                canonical_associated_bonding_curve = %DirectBuyBuilder::canonical_associated_bonding_curve(
                    mint,
                    &token_program,
                ),
                "Trigger: dropping invalid associated_bonding_curve override"
            );
        }
        let canonical_user_ata =
            get_associated_token_address_with_program_id(&payer_pubkey, mint, &token_program);
        let ata_probe = match speculative_ata_probe_result {
            Some((speculative_token_program, probe))
                if speculative_token_program == token_program =>
            {
                probe
            }
            Some((speculative_token_program, _)) => {
                warn!(
                    mint = %mint,
                    speculative_token_program = %speculative_token_program,
                    canonical_token_program = %token_program,
                    "Trigger: discarding speculative ATA probe due to token_program mismatch"
                );
                self.probe_user_ata_pre_submit(mint, canonical_user_ata, &token_program, rpc)
                    .await?
            }
            None => {
                self.probe_user_ata_pre_submit(mint, canonical_user_ata, &token_program, rpc)
                    .await?
            }
        };
        let user_ata = ata_probe.user_ata;
        let attach_idempotent_ata_create = true;
        let ata_missing_pre_submit = ata_probe.ata_missing_pre_submit;
        let pre_submit_token_balance = ata_probe.pre_submit_token_balance;
        let buy_variant = sanitized_overrides
            .buy_variant
            .unwrap_or(trigger::PumpfunBuyVariant::RoutedExactSolIn);
        preparation_telemetry.token_balance_probe_ms = ata_probe.elapsed_ms;
        preparation_telemetry.ata_rent_fetch_ms = ata_probe.ata_rent_fetch_ms;
        let user_ata_rent_lamports = ata_probe.expected_ata_rent_lamports;
        if let (true, true, Some(payer_balance_lamports)) = (
            requires_balance_preflight,
            ata_missing_pre_submit,
            payer_balance_lamports,
        ) {
            Self::validate_payer_balance_for_buy(
                &payer_pubkey,
                payer_balance_lamports,
                amount_lamports.saturating_add(user_ata_rent_lamports),
                effective_tip_lamports,
            )?;
        }
        info!(
            mint = %mint,
            payer = %payer_pubkey,
            payer_provenance,
            attach_idempotent_ata_create,
            ata_missing_pre_submit,
            user_ata = %user_ata,
            pre_submit_token_balance = pre_submit_token_balance.unwrap_or_default(),
            user_ata_rent_lamports,
            token_program = %token_program,
            global_config = %sanitized_overrides
                .global_config
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            fee_recipient = %sanitized_overrides
                .fee_recipient
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            creator_pubkey = %sanitized_overrides
                .creator_pubkey
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            associated_bonding_curve = %sanitized_overrides
                .associated_bonding_curve
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            has_legacy_buy_curve = sanitized_overrides.legacy_buy_curve.is_some(),
            buy_variant = buy_variant.as_str(),
            "Trigger: prepared buy request accounts"
        );
        let decision_ts_ms = Self::now_ms();
        let blockhash_fetch_started_at = Instant::now();
        let (blockhash_snapshot, blockhash_source) = self
            .resolve_live_blockhash(rpc)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch recent blockhash: {}", e))?;
        let blockhash_fetch_latency_ms = saturating_elapsed_ms(blockhash_fetch_started_at);
        let recent_blockhash = blockhash_snapshot.blockhash;
        let tip_seed = format!("{mint}:{recent_blockhash}");
        let tip_account = self
            .live_tx_sender
            .as_ref()
            .map(|sender| sender.select_tip_account(tip_seed.as_bytes()))
            .unwrap_or_else(|| select_sender_tip_account(tip_seed.as_bytes()));
        let build_profile_started_at = Instant::now();
        let build_profile = self.create_buy_build_profile(
            &payer.pubkey(),
            mint,
            &token_program,
            attach_idempotent_ata_create,
            &sanitized_overrides,
            amount_lamports,
            ata_missing_pre_submit,
            pre_submit_token_balance,
        )?;
        let priority_fee_cache_key = PriorityFeeCacheKey::buy(
            build_profile.buy_variant.as_str(),
            token_program,
            ata_missing_pre_submit,
            effective_tip_lamports > 0,
        );
        let cached_priority_fee = self
            .live_tx_sender
            .as_ref()
            .and_then(|sender| sender.get_cached_buy_priority_fee(&priority_fee_cache_key));
        let initial_priority_fee_micro_lamports = cached_priority_fee
            .as_ref()
            .map(|estimate| estimate.micro_lamports)
            .unwrap_or(HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS);
        if let Some(cached_priority_fee) = cached_priority_fee.as_ref() {
            preparation_telemetry.priority_fee_cache_hit = cached_priority_fee.telemetry.cache_hit;
            preparation_telemetry.priority_fee_cache_age_ms =
                cached_priority_fee.telemetry.cache_age_ms;
            preparation_telemetry.priority_fee_fetch_latency_ms =
                cached_priority_fee.telemetry.fetch_latency_ms;
            preparation_telemetry.priority_fee_cache_mode =
                cached_priority_fee.telemetry.cache_mode;
            preparation_telemetry.priority_fee_source = cached_priority_fee.telemetry.source;
            preparation_telemetry.priority_fee_inflight_join_result =
                cached_priority_fee.telemetry.inflight_join_result;
            preparation_telemetry.priority_fee_inflight_wait_ms =
                cached_priority_fee.telemetry.inflight_wait_ms;
        }
        let request;
        if let Some(sender) = self
            .live_tx_sender
            .as_ref()
            .filter(|_| cached_priority_fee.is_none())
        {
            let (_, probe_buy_tx) = self.build_buy_transaction_from_profile(
                &payer,
                &build_profile,
                HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
                &tip_account,
                effective_tip_lamports,
                recent_blockhash,
            )?;
            preparation_telemetry.build_once_ms = saturating_elapsed_ms(build_profile_started_at);
            self.start_current_buy_priority_fee_prewarm(
                &build_profile,
                effective_tip_lamports,
                &priority_fee_cache_key,
                probe_buy_tx.clone(),
            )
            .await;
            let priority_fee_estimate = sender
                .estimate_buy_priority_fee_micro_lamports_with_telemetry(
                    &probe_buy_tx,
                    Some(&priority_fee_cache_key),
                )
                .await;
            preparation_telemetry.priority_fee_cache_hit =
                priority_fee_estimate.telemetry.cache_hit;
            preparation_telemetry.priority_fee_cache_age_ms =
                priority_fee_estimate.telemetry.cache_age_ms;
            preparation_telemetry.priority_fee_fetch_latency_ms =
                priority_fee_estimate.telemetry.fetch_latency_ms;
            preparation_telemetry.priority_fee_cache_mode =
                priority_fee_estimate.telemetry.cache_mode;
            preparation_telemetry.priority_fee_source = priority_fee_estimate.telemetry.source;
            preparation_telemetry.priority_fee_inflight_join_result =
                priority_fee_estimate.telemetry.inflight_join_result;
            preparation_telemetry.priority_fee_inflight_wait_ms =
                priority_fee_estimate.telemetry.inflight_wait_ms;
            let rebuild_started_at = Instant::now();
            let (rpc_buy_tx, buy_tx) = self.build_buy_transaction_from_profile(
                &payer,
                &build_profile,
                priority_fee_estimate.micro_lamports,
                &tip_account,
                effective_tip_lamports,
                recent_blockhash,
            )?;
            preparation_telemetry.rebuild_ms = saturating_elapsed_ms(rebuild_started_at);
            let request_metadata = PreparedBuyRequestBuildMetadata {
                decision_ts_ms,
                ..PreparedBuyRequestBuildMetadata::live(
                    &blockhash_snapshot,
                    blockhash_source,
                    blockhash_fetch_latency_ms,
                    saturating_elapsed_ms(build_profile_started_at),
                    decision_ts_ms,
                )
            };
            request = self.assemble_prepared_buy_request_from_profile(
                payer_provenance,
                &build_profile,
                effective_tip_lamports,
                priority_fee_estimate.micro_lamports,
                request_metadata,
                rpc_buy_tx,
                buy_tx,
            );
        } else {
            let (rpc_buy_tx, buy_tx) = self.build_buy_transaction_from_profile(
                &payer,
                &build_profile,
                initial_priority_fee_micro_lamports,
                &tip_account,
                effective_tip_lamports,
                recent_blockhash,
            )?;
            preparation_telemetry.build_once_ms = saturating_elapsed_ms(build_profile_started_at);
            let request_metadata = PreparedBuyRequestBuildMetadata {
                decision_ts_ms,
                ..PreparedBuyRequestBuildMetadata::live(
                    &blockhash_snapshot,
                    blockhash_source,
                    blockhash_fetch_latency_ms,
                    saturating_elapsed_ms(build_profile_started_at),
                    decision_ts_ms,
                )
            };
            request = self.assemble_prepared_buy_request_from_profile(
                payer_provenance,
                &build_profile,
                effective_tip_lamports,
                initial_priority_fee_micro_lamports,
                request_metadata,
                rpc_buy_tx,
                buy_tx,
            );
        }
        let mut request = request;
        request.preparation_telemetry = preparation_telemetry;
        Self::record_initial_buy_preparation_metrics(&request.preparation_telemetry);
        Self::log_buy_preparation_breakdown(&request);
        Ok(request)
    }

    pub async fn prepare_buy_request_with_decision_ts(
        &self,
        mint: &Pubkey,
        account_overrides: &BuyAccountOverrides,
        tip_lamports: u64,
        decision_ts_ms: Option<u64>,
    ) -> Result<PreparedBuyRequest> {
        self.prepare_buy_request_with_decision_ts_and_amount_lamports(
            mint,
            account_overrides,
            tip_lamports,
            decision_ts_ms,
            None,
        )
        .await
    }

    pub async fn prepare_buy_request_with_decision_ts_and_amount_lamports(
        &self,
        mint: &Pubkey,
        account_overrides: &BuyAccountOverrides,
        tip_lamports: u64,
        decision_ts_ms: Option<u64>,
        amount_lamports_override: Option<u64>,
    ) -> Result<PreparedBuyRequest> {
        let mut request = self
            .prepare_buy_request_with_tip_telemetry_and_amount_lamports(
                mint,
                account_overrides,
                tip_lamports,
                None,
                amount_lamports_override,
            )
            .await?;
        if let Some(decision_ts_ms) = decision_ts_ms {
            request.decision_ts_ms = decision_ts_ms;
        }
        Ok(request)
    }

    pub async fn dispatch_prepared_buy(
        &self,
        request: PreparedBuyRequest,
    ) -> Result<TriggerBuyOutcome> {
        let receipt = self.dispatch_prepared_buy_with_shadow(request).await;
        if receipt.active_position_lease.is_some() {
            warn!(
                "Trigger: dispatch_prepared_buy() bypasses post-buy slot handoff; \
                 prefer dispatch_prepared_buy_with_shadow() in authoritative BUY paths"
            );
        }
        if receipt.shadow_task.is_some() {
            warn!(
                "Trigger: dispatch_prepared_buy() detached a live_and_shadow background simulation; \
                 prefer dispatch_prepared_buy_with_shadow() when telemetry must be observed"
            );
        }
        receipt.primary_outcome
    }

    pub async fn dispatch_prepared_buy_shadow_only(
        &self,
        request: PreparedBuyRequest,
    ) -> TriggerDispatchReceipt {
        let request_for_error = request.clone();
        let active_position_lease = match self.try_reserve_position_slot(&request.mint, &request) {
            Ok(lease) => Some(lease),
            Err(err) => {
                return TriggerDispatchReceipt {
                    primary_outcome: Err(err),
                    shadow_task: None,
                    active_position_lease: None,
                    retain_position_slot_on_error: false,
                    failed_request: Some(request_for_error),
                    failed_context: None,
                };
            }
        };
        let report = self
            .shadow_simulator
            .simulate_buy(&request, &self.config.shadow_run)
            .await;
        TriggerDispatchReceipt {
            primary_outcome: report.map(|report| TriggerBuyOutcome::ShadowSimulated { report }),
            shadow_task: None,
            active_position_lease,
            retain_position_slot_on_error: false,
            failed_request: Some(request_for_error),
            failed_context: None,
        }
    }

    pub async fn simulate_counterfactual_shadow_probe(
        &self,
        request: &PreparedBuyRequest,
    ) -> Result<super::shadow_run::ShadowBuySimulationReport> {
        self.shadow_simulator
            .simulate_buy(request, &self.config.shadow_run)
            .await
    }

    fn counterfactual_probe_can_create_missing_user_ata(
        request: &PreparedBuyRequest,
        pubkey: &Pubkey,
    ) -> bool {
        request.attach_idempotent_ata_create
            && request.ata_missing_pre_submit
            && *pubkey == request.user_ata
    }

    fn counterfactual_probe_can_use_missing_ephemeral_payer(
        request: &PreparedBuyRequest,
        pubkey: &Pubkey,
    ) -> bool {
        request.payer_provenance
            == Self::payer_provenance_label(TriggerShadowPayerStrategy::Ephemeral)
            && *pubkey == request.payer_pubkey
    }

    fn counterfactual_probe_can_use_missing_user_volume_accumulator(
        request: &PreparedBuyRequest,
        pubkey: &Pubkey,
        role: &str,
    ) -> bool {
        role == "user_volume_accumulator"
            && request
                .build_profile
                .as_ref()
                .map(|profile| {
                    profile
                        .buy_instruction
                        .accounts
                        .get(13)
                        .map(|account| account.pubkey == *pubkey)
                        .unwrap_or(false)
                })
                .unwrap_or(false)
    }

    pub(crate) fn counterfactual_probe_buy_instruction_account_role_for(
        request: &PreparedBuyRequest,
        pubkey: &Pubkey,
    ) -> Option<&'static str> {
        let profile = request.build_profile.as_ref()?;
        let index = profile
            .buy_instruction
            .accounts
            .iter()
            .position(|account| account.pubkey == *pubkey)?;
        Some(match index {
            0 => "global_config",
            1 => "fee_recipient",
            2 => "mint",
            3 => "bonding_curve",
            4 => "associated_bonding_curve",
            5 => "user_ata",
            6 => "payer_pubkey",
            7 => "system_program",
            8 => "token_program",
            9 => "creator_vault",
            10 => "event_authority",
            11 => "pump_program",
            12 => "global_volume_accumulator",
            13 => "user_volume_accumulator",
            14 => "fee_config",
            15 => "fee_program",
            16 => "bonding_curve_v2",
            17 => "buyback_fee_recipient",
            _ => match profile.buy_variant {
                trigger::PumpfunBuyVariant::LegacyBuy => "legacy_buy_instruction_account",
                trigger::PumpfunBuyVariant::RoutedExactSolIn => "routed_buy_instruction_account",
            },
        })
    }

    pub(crate) fn counterfactual_probe_account_role_for(
        request: &PreparedBuyRequest,
        pubkey: &Pubkey,
    ) -> String {
        if *pubkey == request.payer_pubkey {
            return "payer_pubkey".to_string();
        }
        if *pubkey == request.user_ata {
            return "user_ata".to_string();
        }
        if *pubkey == request.mint {
            return "mint".to_string();
        }
        if *pubkey == request.token_program {
            return "token_program".to_string();
        }
        if let Some(value) = request.account_overrides.global_config {
            if *pubkey == value {
                return "global_config".to_string();
            }
        }
        if let Some(value) = request.account_overrides.fee_recipient {
            if *pubkey == value {
                return "fee_recipient".to_string();
            }
        }
        if let Some(value) = request.account_overrides.creator_pubkey {
            if *pubkey == value {
                return "creator_pubkey".to_string();
            }
        }
        if let Some(value) = request.account_overrides.associated_bonding_curve {
            if *pubkey == value {
                return "associated_bonding_curve".to_string();
            }
        }
        if let Some(profile) = request.build_profile.as_ref() {
            if *pubkey == profile.payer_pubkey {
                return "payer_pubkey".to_string();
            }
            if *pubkey == profile.user_ata {
                return "user_ata".to_string();
            }
            if *pubkey == profile.mint {
                return "mint".to_string();
            }
            if *pubkey == profile.token_program {
                return "token_program".to_string();
            }
            if let Some(value) = profile.account_overrides.global_config {
                if *pubkey == value {
                    return "global_config".to_string();
                }
            }
            if let Some(value) = profile.account_overrides.fee_recipient {
                if *pubkey == value {
                    return "fee_recipient".to_string();
                }
            }
            if let Some(value) = profile.account_overrides.creator_pubkey {
                if *pubkey == value {
                    return "creator_pubkey".to_string();
                }
            }
            if let Some(value) = profile.account_overrides.associated_bonding_curve {
                if *pubkey == value {
                    return "associated_bonding_curve".to_string();
                }
            }
        }
        if let Some(role) =
            Self::counterfactual_probe_buy_instruction_account_role_for(request, pubkey)
        {
            return role.to_string();
        }
        "transaction_account".to_string()
    }

    pub(crate) fn counterfactual_probe_required_account_roles(
        request: &PreparedBuyRequest,
    ) -> Vec<(Pubkey, String)> {
        let mut seen = HashSet::new();
        let mut accounts = Vec::new();
        let mut push_account = |pubkey: Pubkey, role: String| {
            if Self::counterfactual_probe_can_create_missing_user_ata(request, &pubkey) {
                return;
            }
            if Self::counterfactual_probe_can_use_missing_ephemeral_payer(request, &pubkey) {
                return;
            }
            if Self::counterfactual_probe_can_use_missing_user_volume_accumulator(
                request, &pubkey, &role,
            ) {
                return;
            }
            if seen.insert(pubkey) {
                accounts.push((pubkey, role));
            }
        };

        push_account(request.payer_pubkey, "payer_pubkey".to_string());
        push_account(request.mint, "mint".to_string());
        push_account(request.token_program, "token_program".to_string());
        push_account(request.user_ata, "user_ata".to_string());
        if let Some(value) = request.account_overrides.global_config {
            push_account(value, "global_config".to_string());
        }
        if let Some(value) = request.account_overrides.fee_recipient {
            push_account(value, "fee_recipient".to_string());
        }
        if let Some(value) = request.account_overrides.associated_bonding_curve {
            push_account(value, "associated_bonding_curve".to_string());
        }

        for pubkey in request.rpc_buy_tx.message.account_keys.iter().copied() {
            push_account(
                pubkey,
                Self::counterfactual_probe_account_role_for(request, &pubkey),
            );
        }

        accounts
    }

    pub(crate) async fn counterfactual_probe_missing_required_account(
        &self,
        request: &PreparedBuyRequest,
    ) -> Result<Option<CounterfactualProbeMissingAccount>> {
        let rpc = self.preparation_rpc();
        for (pubkey, role) in Self::counterfactual_probe_required_account_roles(request) {
            match rpc
                .get_account_with_commitment(&pubkey, CommitmentConfig::processed())
                .await
            {
                Ok(response) if response.value.is_some() => {}
                Ok(_) => {
                    return Ok(Some(CounterfactualProbeMissingAccount { pubkey, role }));
                }
                Err(err) if Self::is_account_not_found_error(&err) => {
                    return Ok(Some(CounterfactualProbeMissingAccount { pubkey, role }));
                }
                Err(err) => {
                    return Err(anyhow::anyhow!(
                        "counterfactual probe account precheck failed: role={} pubkey={} error={}",
                        role,
                        pubkey,
                        err
                    ));
                }
            }
        }
        Ok(None)
    }

    pub(crate) async fn counterfactual_probe_missing_manifest_accounts(
        &self,
        accounts: &[(Pubkey, String)],
    ) -> Result<Vec<CounterfactualProbeMissingAccount>> {
        let rpc = self.preparation_rpc();
        let mut missing = Vec::new();
        let mut seen = HashSet::new();
        for (pubkey, role) in accounts {
            if !seen.insert(*pubkey) {
                continue;
            }
            match rpc
                .get_account_with_commitment(pubkey, CommitmentConfig::processed())
                .await
            {
                Ok(response) if response.value.is_some() => {}
                Ok(_) => missing.push(CounterfactualProbeMissingAccount {
                    pubkey: *pubkey,
                    role: role.clone(),
                }),
                Err(err) if Self::is_account_not_found_error(&err) => {
                    missing.push(CounterfactualProbeMissingAccount {
                        pubkey: *pubkey,
                        role: role.clone(),
                    });
                }
                Err(err) => {
                    return Err(anyhow::anyhow!(
                        "counterfactual probe manifest account check failed: role={} pubkey={} error={}",
                        role,
                        pubkey,
                        err
                    ));
                }
            }
        }
        Ok(missing)
    }

    pub fn spawn_shadow_simulation(&self, request: PreparedBuyRequest) -> PendingShadowSimulation {
        let shadow_simulator = Arc::clone(&self.shadow_simulator);
        let shadow_config = self.config.shadow_run.clone();
        let shadow_run_semaphore = Arc::clone(&self.shadow_run_semaphore);
        let entry_mode = self.config.entry_mode;
        let available_permits = shadow_run_semaphore.available_permits();
        if available_permits == 0 {
            super::shadow_run::record_shadow_buy_queue_overflow(entry_mode);
        }
        gauge!(
            "trigger_shadow_available_permits",
            available_permits as f64,
            "entry_mode" => entry_mode.as_str()
        );
        let request_for_task = request.clone();
        let handle = tokio::spawn(async move {
            let _permit = shadow_run_semaphore
                .acquire_owned()
                .await
                .map_err(|e| anyhow::anyhow!("shadow run semaphore closed: {}", e))?;
            shadow_simulator
                .simulate_buy(&request_for_task, &shadow_config)
                .await
        });
        PendingShadowSimulation { request, handle }
    }

    pub async fn dispatch_prepared_buy_with_shadow(
        &self,
        mut request: PreparedBuyRequest,
    ) -> TriggerDispatchReceipt {
        match self.config.entry_mode {
            TriggerEntryMode::DryRunMock => {
                let active_position_lease =
                    match self.try_reserve_position_slot(&request.mint, &request) {
                        Ok(lease) => Some(lease),
                        Err(err) => {
                            return TriggerDispatchReceipt {
                                primary_outcome: Err(err),
                                shadow_task: None,
                                active_position_lease: None,
                                retain_position_slot_on_error: false,
                                failed_request: Some(request),
                                failed_context: None,
                            };
                        }
                    };
                info!(
                    "🎯 [DRY RUN MOCK] Would execute BUY: mint={}, tip={} lamports ({:.4} SOL)",
                    request.mint,
                    request.tip_lamports,
                    request.tip_lamports as f64 / 1_000_000_000.0
                );
                return TriggerDispatchReceipt {
                    primary_outcome: Ok(TriggerBuyOutcome::DryRunMock {
                        signature: Signature::new_unique(),
                    }),
                    shadow_task: None,
                    active_position_lease,
                    retain_position_slot_on_error: false,
                    failed_request: None,
                    failed_context: None,
                };
            }
            TriggerEntryMode::ShadowOnly => {
                let request_for_error = request.clone();
                let active_position_lease =
                    match self.try_reserve_position_slot(&request.mint, &request) {
                        Ok(lease) => Some(lease),
                        Err(err) => {
                            return TriggerDispatchReceipt {
                                primary_outcome: Err(err),
                                shadow_task: None,
                                active_position_lease: None,
                                retain_position_slot_on_error: false,
                                failed_request: Some(request_for_error),
                                failed_context: None,
                            };
                        }
                    };
                let report = self
                    .shadow_simulator
                    .simulate_buy(&request, &self.config.shadow_run)
                    .await;
                return TriggerDispatchReceipt {
                    primary_outcome: report
                        .map(|report| TriggerBuyOutcome::ShadowSimulated { report }),
                    shadow_task: None,
                    active_position_lease,
                    retain_position_slot_on_error: false,
                    failed_request: Some(request_for_error),
                    failed_context: None,
                };
            }
            TriggerEntryMode::Live | TriggerEntryMode::LiveAndShadow => {}
        }

        let request_for_error = request.clone();
        if let Err(err) = self.ensure_live_sender_transport() {
            return TriggerDispatchReceipt {
                primary_outcome: Err(err),
                shadow_task: None,
                active_position_lease: None,
                retain_position_slot_on_error: false,
                failed_request: Some(request_for_error),
                failed_context: None,
            };
        }

        let mint = request.mint;
        let amount_lamports = request.amount_lamports;
        let tip_lamports = request.tip_lamports;
        let reserve_slot_started_at = Instant::now();
        let active_position_lease = match self.try_reserve_position_slot(&mint, &request) {
            Ok(lease) => Some(lease),
            Err(err) => {
                return TriggerDispatchReceipt {
                    primary_outcome: Err(err),
                    shadow_task: None,
                    active_position_lease: None,
                    retain_position_slot_on_error: false,
                    failed_request: Some(request_for_error),
                    failed_context: None,
                };
            }
        };
        request.reserve_slot_latency_ms = saturating_elapsed_ms(reserve_slot_started_at);
        let shadow_spawn_started_at = Instant::now();
        let shadow_task = matches!(self.config.entry_mode, TriggerEntryMode::LiveAndShadow)
            .then(|| self.spawn_shadow_simulation(request.clone()));
        request.shadow_spawn_latency_ms = if shadow_task.is_some() {
            saturating_elapsed_ms(shadow_spawn_started_at)
        } else {
            0
        };

        let decision_ts_ms = request.decision_ts_ms;
        let mut retain_position_slot_on_error = false;
        let primary_outcome = match self.submit_prepared_via_sender(request).await {
            Ok(confirmed_transaction) => {
                self.record_tx_send_latency("helius_sender_confirmed", decision_ts_ms);
                info!(
                    "🚀 [LIVE FIRE] Helius Sender BUY confirmed: mint={}, sig={}, landed_slot={:?}, amount_lamports={}, tip_lamports={}",
                    mint,
                    confirmed_transaction.signature,
                    confirmed_transaction.landed_slot,
                    amount_lamports,
                    tip_lamports
                );
                Ok(TriggerBuyOutcome::LiveConfirmed {
                    signature: confirmed_transaction.signature,
                    landed_slot: confirmed_transaction.landed_slot,
                })
            }
            Err(error) => {
                retain_position_slot_on_error = error.should_retain_position_slot();
                warn!(
                    mint = %mint,
                    amount_lamports,
                    tip_lamports,
                    retain_position_slot = retain_position_slot_on_error,
                    error = ?error,
                    "🔴 BUY dispatch FAILED — Helius Sender submission or Yellowstone confirmation error"
                );
                Err(error.into_anyhow())
            }
        };

        TriggerDispatchReceipt {
            primary_outcome,
            shadow_task,
            active_position_lease,
            retain_position_slot_on_error,
            failed_request: Some(request_for_error),
            failed_context: None,
        }
    }

    /// Executes a live BUY transaction for the given token mint.
    ///
    /// This method is the critical integration point between the Oracle Runtime
    /// and actual transaction execution.
    ///
    /// # Arguments
    /// * `mint` - Token mint to buy
    /// * `tip_lamports` - BUY tip amount resolved for the active transport
    ///
    /// # Returns
    /// * `Ok(Signature)` - transaction signature on success
    /// * `Err(anyhow::Error)` - Execution failure (network, insufficient funds, etc.)
    ///
    pub async fn execute_buy(
        &self,
        mint: &Pubkey,
        account_overrides: &BuyAccountOverrides,
        tip_lamports: u64,
    ) -> Result<TriggerBuyOutcome> {
        match self.config.entry_mode {
            TriggerEntryMode::DryRunMock => {
                info!(
                    "🎯 [DRY RUN MOCK] Would execute BUY: mint={}, tip={} lamports ({:.4} SOL)",
                    mint,
                    tip_lamports,
                    tip_lamports as f64 / 1_000_000_000.0
                );
                return Ok(TriggerBuyOutcome::DryRunMock {
                    signature: Signature::new_unique(),
                });
            }
            TriggerEntryMode::ShadowOnly => {
                let request = self
                    .prepare_buy_request(mint, account_overrides, tip_lamports)
                    .await?;
                return self.dispatch_prepared_buy(request).await;
            }
            TriggerEntryMode::LiveAndShadow => {}
            TriggerEntryMode::Live => {}
        }

        let request = self
            .prepare_buy_request(mint, account_overrides, tip_lamports)
            .await?;
        self.dispatch_prepared_buy(request).await
    }
}

async fn append_shadow_buy_report_record(
    log_path: &std::path::Path,
    entry_mode: TriggerEntryMode,
    record: &crate::events::ShadowBuySimulationEvent,
) -> Result<()> {
    let join_key = super::shadow_run::make_shadow_join_key(
        &record.pool_amm_id,
        &record.base_mint,
        record.decision_ts_ms,
    );
    let rollout_profile = super::shadow_run::derive_shadow_rollout_profile_from_path(log_path);
    let jsonl_record = super::shadow_run::ShadowBuySimulationRecord::from_event(entry_mode, record)
        .with_lifecycle_identity(join_key, rollout_profile);
    super::shadow_run::record_shadow_buy_metrics(&jsonl_record);
    crate::oracle_metrics::record_shadow_lifecycle_status(if jsonl_record.err.is_some() {
        "failed_reconciliation"
    } else {
        "dispatched"
    });
    super::shadow_run::append_shadow_buy_record(log_path, &jsonl_record).await
}

async fn persist_shadow_event_record(
    output_path: &str,
    entry_mode: TriggerEntryMode,
    event: &crate::events::ShadowBuySimulationEvent,
) -> Result<()> {
    append_shadow_buy_report_record(std::path::Path::new(output_path), entry_mode, event).await
}

async fn persist_shadow_failure_record(
    output_path: &str,
    entry_mode: TriggerEntryMode,
    pool_amm_id: &str,
    base_mint: &str,
    failed_request: Option<&PreparedBuyRequest>,
    failed_context: Option<&TriggerDispatchFailureContext>,
    err: &anyhow::Error,
) -> Result<()> {
    let decision_ts_ms = failed_request
        .map(|request| request.decision_ts_ms)
        .or_else(|| failed_context.map(|context| context.decision_ts_ms))
        .unwrap_or_default();
    let join_key = super::shadow_run::make_shadow_join_key(pool_amm_id, base_mint, decision_ts_ms);
    let rollout_profile = super::shadow_run::derive_shadow_rollout_profile_from_path(
        std::path::Path::new(output_path),
    );
    let record = if let Some(request) = failed_request {
        super::shadow_run::ShadowBuySimulationRecord::from_failure(
            entry_mode,
            pool_amm_id,
            base_mint,
            request,
            None,
            err,
        )
        .with_lifecycle_identity(join_key.clone(), rollout_profile.clone())
    } else if let Some(context) = failed_context {
        super::shadow_run::ShadowBuySimulationRecord::from_failure_context(
            entry_mode,
            pool_amm_id,
            base_mint,
            context,
            None,
            err,
        )
        .with_lifecycle_identity(join_key, rollout_profile)
    } else {
        return Ok(());
    };

    super::shadow_run::record_shadow_buy_metrics(&record);
    crate::oracle_metrics::record_shadow_lifecycle_status("failed_reconciliation");
    super::shadow_run::append_shadow_buy_record(std::path::Path::new(output_path), &record).await
}

fn spawn_background_shadow_event(
    event_bus_tx: Option<EventBusSender>,
    emit_event_bus: bool,
    output_path: String,
    pool_amm_id: String,
    base_mint: String,
    live_signature: Option<String>,
    shadow_task: PendingShadowSimulation,
) {
    tokio::spawn(async move {
        let PendingShadowSimulation { request, handle } = shadow_task;
        match handle.await {
            Ok(Ok(mut report)) => {
                report.live_signature = live_signature;
                let event = super::shadow_run::shadow_buy_event_from_report(
                    &pool_amm_id,
                    &base_mint,
                    report,
                );
                if emit_event_bus {
                    if let Some(event_bus_tx) = event_bus_tx.as_ref() {
                        if let Err(e) =
                            event_bus_tx.send(GhostEvent::shadow_buy_simulated(event.clone()))
                        {
                            warn!(
                                "Trigger: Failed to emit background ShadowBuySimulated event for pool {}: {}",
                                pool_amm_id, e
                            );
                        }
                    }
                }
                if let Err(e) = append_shadow_buy_report_record(
                    std::path::Path::new(&output_path),
                    TriggerEntryMode::LiveAndShadow,
                    &event,
                )
                .await
                {
                    warn!(
                        "Trigger: Failed to append background shadow report for pool {}: {}",
                        pool_amm_id, e
                    );
                }
            }
            Ok(Err(e)) => {
                let record = super::shadow_run::ShadowBuySimulationRecord::from_failure(
                    TriggerEntryMode::LiveAndShadow,
                    &pool_amm_id,
                    &base_mint,
                    &request,
                    live_signature,
                    &e,
                )
                .with_lifecycle_identity(
                    super::shadow_run::make_shadow_join_key(
                        &pool_amm_id,
                        &base_mint,
                        request.decision_ts_ms,
                    ),
                    super::shadow_run::derive_shadow_rollout_profile_from_path(
                        std::path::Path::new(&output_path),
                    ),
                );
                super::shadow_run::record_shadow_buy_metrics(&record);
                if let Err(write_err) = super::shadow_run::append_shadow_buy_record(
                    std::path::Path::new(&output_path),
                    &record,
                )
                .await
                {
                    warn!(
                        "Trigger: Failed to append background shadow failure record for pool {}: {}",
                        pool_amm_id, write_err
                    );
                }
                warn!(
                    "Trigger: background live_and_shadow simulation failed for pool {}: {}",
                    pool_amm_id, e
                );
            }
            Err(e) => {
                let join_error = anyhow::Error::new(e);
                let record = super::shadow_run::ShadowBuySimulationRecord::from_failure(
                    TriggerEntryMode::LiveAndShadow,
                    &pool_amm_id,
                    &base_mint,
                    &request,
                    live_signature,
                    &join_error,
                )
                .with_lifecycle_identity(
                    super::shadow_run::make_shadow_join_key(
                        &pool_amm_id,
                        &base_mint,
                        request.decision_ts_ms,
                    ),
                    super::shadow_run::derive_shadow_rollout_profile_from_path(
                        std::path::Path::new(&output_path),
                    ),
                );
                super::shadow_run::record_shadow_buy_metrics(&record);
                if let Err(write_err) = super::shadow_run::append_shadow_buy_record(
                    std::path::Path::new(&output_path),
                    &record,
                )
                .await
                {
                    warn!(
                        "Trigger: Failed to append background shadow join-failure record for pool {}: {}",
                        pool_amm_id, write_err
                    );
                }
                warn!(
                    "Trigger: background live_and_shadow join failed for pool {}: {}",
                    pool_amm_id, join_error
                );
            }
        }
    });
}

/// Run the Trigger component with Oracle integration (using default Oracle config)
#[allow(dead_code)]
pub async fn run(
    config: TriggerComponentConfig,
    shutdown_rx: broadcast::Receiver<()>,
    event_bus_rx: Option<EventBusReceiver>,
    event_bus_tx: Option<EventBusSender>,
) -> Result<()> {
    // Default Oracle config - in production this would come from LauncherConfig
    run_with_oracle(
        config,
        OracleConfig::default(),
        Arc::new(ShadowLedger::new()),
        shutdown_rx,
        event_bus_rx,
        event_bus_tx,
    )
    .await
}

/// Run the Trigger component with explicit Oracle configuration
pub async fn run_with_oracle(
    config: TriggerComponentConfig,
    oracle_config: OracleConfig,
    shadow_ledger: Arc<ShadowLedger>,
    mut shutdown_rx: broadcast::Receiver<()>,
    event_bus_rx: Option<EventBusReceiver>,
    _event_bus_tx: Option<EventBusSender>,
) -> Result<()> {
    info!("Trigger: Initializing component");
    info!("  RPC URL: {}", config.rpc_url);
    info!(
        "  Max Concurrent Positions: {}",
        config.max_concurrent_positions
    );
    info!(
        "  Live Preflight Max State Age Slots: {}",
        config.live_preflight_max_state_age_slots
    );

    // Initialize Oracle Pipeline
    let oracle_pipeline = Arc::new(OraclePipeline::new(
        oracle_config.clone(),
        Arc::clone(&shadow_ledger),
    ));

    log_trigger_legacy_path_contracts();
    record_legacy_path_event(&TRIGGER_EMBEDDED_ORACLE_PIPELINE_PATH);

    if oracle_pipeline.is_enabled() {
        info!(
            runtime_plane = RuntimePlane::LegacyObservation.as_str(),
            path = TRIGGER_EMBEDDED_ORACLE_PIPELINE_PATH.path,
            classification = TRIGGER_EMBEDDED_ORACLE_PIPELINE_PATH.classification.as_str(),
            "Trigger: embedded Oracle Pipeline remains compatibility-only and cannot emit authoritative BUY side effects"
        );
        info!(
            "  HyperPrediction: enabled, threshold={}",
            oracle_config.simple_oracle.min_score_threshold
        );
        info!(
            "  QASS: enabled={}, collapse_threshold={:.2}",
            oracle_config.qass.enabled, oracle_config.qass.collapse_threshold
        );
        info!(
            "  HyperOracle: enabled={}, SCR threshold={:.2}",
            oracle_config.hyper_oracle.enabled, oracle_config.hyper_oracle.scr_threshold
        );
        info!(
            "  Combined score threshold: {}",
            oracle_config.pipeline.combined_score_threshold
        );
    } else {
        info!("Trigger: Oracle Pipeline disabled - all pools will pass");
    }

    let _trigger_executor = Arc::new(TriggerComponent::new(config.clone()));

    // ✅ ADDED: Cache for pools waiting for Oracle verdict
    // Key: pool_amm_id, Value: DetectedPool data needed for execution
    let mut pending_pools: HashMap<String, Arc<crate::events::DetectedPool>> = HashMap::new();

    // If we have an event bus receiver, use it
    if let Some(mut rx) = event_bus_rx {
        info!("Trigger: 📡 Listening to unified event bus");

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Trigger: Shutdown signal received");

                    // Log final Oracle metrics
                    if oracle_pipeline.is_enabled() {
                        let metrics = oracle_pipeline.get_metrics();
                        info!("Trigger: Oracle Pipeline final metrics:");
                        info!("  Candidates processed: {}", metrics.candidates_processed);
                        info!("  Candidates passed: {}", metrics.candidates_passed);
                        info!("  Candidates failed: {}", metrics.candidates_failed);
                        info!("  Candidates timeout: {}", metrics.candidates_timeout);
                        info!("  Avg processing time: {}us", metrics.avg_processing_time_us);
                    }

                    break;
                }
                event = rx.recv() => {
                    match event {
                        Ok(ghost_event) => {
                            match ghost_event {
                                GhostEvent::NewPoolDetected(pool) => {
                                    // 🟢 CHANGED: The "Lobotomy" - Don't score immediately. Cache and wait.
                                    info!(
                                        "Trigger: 🎯 Received NewPoolDetected - pool={}, mint={} (Caching & Waiting for OracleRuntime)",
                                        pool.pool_amm_id, pool.base_mint
                                    );

                                    // SAVE TO WAITING AREA. DON'T SCORE INDEPENDENTLY.
                                    // Wait until OracleRuntime collects data (real/synthetic) and sends PoolScored.
                                    pending_pools.insert(pool.pool_amm_id.clone(), pool);
                                }
                                GhostEvent::GatekeeperCommitted { .. } => {
                                    // Ignored by trigger; scoring/approval handled by OracleRuntime
                                }
                                GhostEvent::PoolScored(scored) => {
                                    info!(
                                        runtime_plane = RuntimePlane::LegacyObservation.as_str(),
                                        path = TRIGGER_POOL_SCORED_OBSERVER_PATH.path,
                                        "Trigger: 📩 Received legacy PoolScored observation - pool={}, score={}, passed={}",
                                        scored.pool_amm_id, scored.score, scored.passed
                                    );

                                    let _ = handle_legacy_pool_scored_event(&mut pending_pools, &scored);
                                }
                                GhostEvent::PoolTransaction(_tx) => {
                                    // PoolTransaction events are handled by SnapshotListener
                                    // Trigger doesn't need to process them
                                }
                                GhostEvent::FundingTransferObserved(_) => {
                                    // Funding-transfer observations feed FSC rolling-state in
                                    // OracleRuntime; Trigger does not consume them.
                                }
                                GhostEvent::TransactionSent { signature, slot, tx_type } => {
                                    debug!(
                                        "Trigger: Received TransactionSent - sig={}, slot={:?}, type={}",
                                        signature, slot, tx_type
                                    );
                                }
                                GhostEvent::TradeExecuted(trade) => {
                                    info!(
                                        "Trigger: Received TradeExecuted - sig={}, mint={}, pnl={:?}",
                                        trade.signature, trade.mint, trade.pnl_sol
                                    );
                                }
                                GhostEvent::Custom(event_type, _data) => {
                                    debug!("Trigger: Received Custom event: {}", event_type);
                                }
                                // ✅ ADDED: Fix for "non-exhaustive patterns" error
                                GhostEvent::GeyserTransaction { .. } => {
                                    // Trigger ignores raw Geyser transactions (handled by OracleRuntime)
                                }
                                GhostEvent::PostBuySubmitted { .. } => {
                                    // Handled by PostBuyRuntime, not Trigger
                                }
                                GhostEvent::AccountUpdate(_) => {
                                    // AccountUpdate events are consumed by OracleRuntime for
                                    // reconciliation; Trigger does not need to process them.
                                }
                                GhostEvent::ShadowBuySimulated(_) => {
                                    // Compare-only shadow entry results are not consumed here.
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            crate::events::record_event_bus_lag("trigger", n as u64);
                            warn!("Trigger: Event bus lagged, missed {} events", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("Trigger: Event bus closed");
                            break;
                        }
                    }
                }
            }
        }
    } else {
        // Fallback: simulate the component running without event bus
        record_legacy_path_event(&TRIGGER_NO_EVENT_BUS_PATH);
        warn!(
            runtime_plane = RuntimePlane::LegacyObservation.as_str(),
            path = TRIGGER_NO_EVENT_BUS_PATH.path,
            classification = TRIGGER_NO_EVENT_BUS_PATH.classification.as_str(),
            "Trigger: running without event bus is disabled in production and cannot emit authoritative BUY side effects"
        );
        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Trigger: Shutdown signal received");
                    break;
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(5)) => {
                    // Trigger would process swap plans here
                }
            }
        }
    }

    info!("Trigger: Component stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{create_event_bus, DetectedPool};
    use async_trait::async_trait;
    use base64::Engine as _;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::broadcast;

    fn create_test_config() -> TriggerComponentConfig {
        TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::Live,
            rpc_url: "https://api.devnet.solana.com".to_string(),
            keypair_path: None,
            tip_guard: crate::config::TriggerTipGuardConfig::default(),
            metrics_port: 9091,
            max_concurrent_positions: 3,
            max_position_size_sol: 0.1,
            emergency_floor_sol: 0.05,
            position_size_buffer_sol: 0.02,
            slippage_tolerance: 0.20,
            live_preflight_max_state_age_slots: 10,
            live_exit_take_profit_pct: 0.02,
            live_exit_stop_loss_pct: 0.02,
            shadow_run: crate::config::TriggerShadowRunConfig::default(),
        }
    }

    fn valid_buy_account_overrides() -> BuyAccountOverrides {
        BuyAccountOverrides {
            creator_pubkey: Some(Pubkey::new_unique()),
            ..BuyAccountOverrides::default()
        }
    }

    fn test_live_tx_sender() -> Arc<LiveTxSender> {
        Arc::new(
            LiveTxSender::new(crate::components::live_tx_sender::LiveTxSenderConfig::new(
                "http://127.0.0.1:18080/fast",
                "http://127.0.0.1:18081",
                "http://127.0.0.1:18082",
                "test-yellowstone-token",
            ))
            .expect("test live tx sender"),
        )
    }

    fn test_live_tx_sender_with_confirmation(
        priority_fee_rpc_url: String,
        yellowstone_grpc_endpoint: &str,
    ) -> Arc<LiveTxSender> {
        test_live_tx_sender_with_endpoints(
            "test://sender-success".to_string(),
            priority_fee_rpc_url,
            yellowstone_grpc_endpoint,
        )
    }

    fn test_live_tx_sender_with_endpoints(
        sender_endpoint: String,
        priority_fee_rpc_url: String,
        yellowstone_grpc_endpoint: &str,
    ) -> Arc<LiveTxSender> {
        Arc::new(
            LiveTxSender::new(crate::components::live_tx_sender::LiveTxSenderConfig::new(
                sender_endpoint,
                priority_fee_rpc_url,
                yellowstone_grpc_endpoint,
                "test-yellowstone-token",
            ))
            .expect("test live tx sender"),
        )
    }

    fn create_test_config_with_rpc_url(rpc_url: String) -> TriggerComponentConfig {
        let mut config = create_test_config();
        config.rpc_url = rpc_url;
        config
    }

    async fn spawn_priority_fee_server(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, Arc<AtomicUsize>) {
        let delayed_responses = responses
            .into_iter()
            .map(|(status_code, response_body)| (status_code, response_body, 0_u64))
            .collect();
        spawn_priority_fee_server_with_delay(delayed_responses).await
    }

    async fn spawn_priority_fee_server_with_delay(
        responses: Vec<(u16, &'static str, u64)>,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind priority fee server");
        let addr = listener.local_addr().expect("priority fee addr");
        let request_count = Arc::new(AtomicUsize::new(0));
        let responses = Arc::new(responses);

        tokio::spawn({
            let request_count = Arc::clone(&request_count);
            let responses = Arc::clone(&responses);
            async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        return;
                    };
                    let mut buffer = vec![0u8; 16_384];
                    let n = match stream.read(&mut buffer).await {
                        Ok(n) if n > 0 => n,
                        _ => continue,
                    };
                    let _request = String::from_utf8_lossy(&buffer[..n]).to_string();
                    let request_index = request_count.fetch_add(1, Ordering::Relaxed);
                    let (status_code, response_body, delay_ms) = responses
                        .get(request_index)
                        .copied()
                        .unwrap_or_else(|| *responses.last().expect("last priority fee response"));
                    if delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    let status_text = if status_code == 200 {
                        "OK"
                    } else {
                        "Internal Server Error"
                    };
                    let response = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        status_code,
                        status_text,
                        response_body.len(),
                        response_body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                }
            }
        });

        (format!("http://{}", addr), request_count)
    }

    async fn spawn_tip_floor_server(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, Arc<AtomicUsize>) {
        let delayed_responses = responses
            .into_iter()
            .map(|(status_code, response_body)| (status_code, response_body, 0_u64))
            .collect();
        spawn_tip_floor_server_with_delay(delayed_responses).await
    }

    async fn spawn_tip_floor_server_with_delay(
        responses: Vec<(u16, &'static str, u64)>,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind tip floor server");
        let addr = listener.local_addr().expect("tip floor addr");
        let request_count = Arc::new(AtomicUsize::new(0));
        let responses = Arc::new(responses);

        tokio::spawn({
            let request_count = Arc::clone(&request_count);
            let responses = Arc::clone(&responses);
            async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        return;
                    };
                    let mut buffer = vec![0u8; 16_384];
                    let n = match stream.read(&mut buffer).await {
                        Ok(n) if n > 0 => n,
                        _ => continue,
                    };
                    let _request = String::from_utf8_lossy(&buffer[..n]).to_string();
                    let request_index = request_count.fetch_add(1, Ordering::Relaxed);
                    let (status_code, response_body, delay_ms) = responses
                        .get(request_index)
                        .copied()
                        .unwrap_or_else(|| *responses.last().expect("last tip floor response"));
                    if delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    let status_text = if status_code == 200 {
                        "OK"
                    } else {
                        "Internal Server Error"
                    };
                    let response = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        status_code,
                        status_text,
                        response_body.len(),
                        response_body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                }
            }
        });

        (format!("http://{}", addr), request_count)
    }

    fn seed_canonical_buy_state(account_state_core: &Arc<AccountStateReducer>, mint: Pubkey) {
        let update = ghost_core::account_state_core::types::AccountStateUpdate {
            pool_amm_id: Pubkey::new_unique(),
            base_mint: mint,
            bonding_curve: Pubkey::new_unique(),
            sol_reserves: 30_000_000_000,
            token_reserves: 1_073_000_000_000_000,
            is_complete: 0,
            slot: 100,
            write_version: Some(1),
            receive_ts_ms: 1_000,
            receive_seq: 1,
            curve_finality: ghost_core::CurveFinality::Provisional,
            source: ghost_core::account_state_core::types::UpdateSource::GeyserAccountUpdate,
        };
        let result = account_state_core.apply_account_update(update);
        assert!(
            matches!(
                result,
                ghost_core::account_state_core::types::AccountUpdateResult::Applied
                    | ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap
            ),
            "canonical state seed must apply cleanly"
        );
    }

    fn mock_token_account_balance_body(amount: u64) -> String {
        let ui_amount = amount as f64 / 1_000_000.0;
        format!(
            "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":{{\"amount\":\"{}\",\"decimals\":6,\"uiAmount\":{},\"uiAmountString\":\"{:.6}\"}}}},\"id\":1}}",
            amount, ui_amount, ui_amount
        )
    }

    fn mock_account_info_body(lamports: u64, owner: &Pubkey) -> String {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":{{\"data\":[\"\",\"base64\"],\"executable\":false,\"lamports\":{},\"owner\":\"{}\",\"rentEpoch\":0,\"space\":0}}}},\"id\":1}}",
            lamports, owner
        )
    }

    fn mock_signature_status_body(slot: u64) -> String {
        mock_signature_statuses_body(&[Some(slot)])
    }

    fn mock_signature_statuses_body(slots: &[Option<u64>]) -> String {
        let values = slots
            .iter()
            .map(|slot| match slot {
                Some(slot) => format!(
                    "{{\"slot\":{slot},\"confirmations\":null,\"status\":{{\"Ok\":null}},\"err\":null,\"confirmationStatus\":\"confirmed\"}}"
                ),
                None => "null".to_string(),
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":[{}]}},\"id\":1}}",
            values
        )
    }

    fn mock_latest_blockhash_body(blockhash: Hash, last_valid_block_height: u64) -> String {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":{{\"blockhash\":\"{}\",\"lastValidBlockHeight\":{}}}}},\"id\":1}}",
            blockhash, last_valid_block_height
        )
    }

    async fn spawn_sender_confirmation_rpc_server(
        expected_ata: Pubkey,
        token_balances: Vec<Option<u64>>,
        signature_statuses: Vec<Option<u64>>,
    ) -> String {
        assert!(
            !token_balances.is_empty(),
            "token balance sequence must not be empty"
        );
        assert!(
            !signature_statuses.is_empty(),
            "signature status sequence must not be empty"
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let addr = listener.local_addr().expect("rpc addr");
        let token_balances = Arc::new(token_balances);
        let signature_statuses = Arc::new(signature_statuses);
        let token_balance_request_count = Arc::new(AtomicUsize::new(0));
        let signature_status_request_count = Arc::new(AtomicUsize::new(0));
        let expected_ata = expected_ata.to_string();

        tokio::spawn({
            let token_balances = Arc::clone(&token_balances);
            let signature_statuses = Arc::clone(&signature_statuses);
            let token_balance_request_count = Arc::clone(&token_balance_request_count);
            let signature_status_request_count = Arc::clone(&signature_status_request_count);
            async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        break;
                    };
                    let mut buffer = vec![0u8; 16_384];
                    let n = match stream.read(&mut buffer).await {
                        Ok(n) if n > 0 => n,
                        _ => continue,
                    };
                    let request = String::from_utf8_lossy(&buffer[..n]);
                    let body = if request.contains("\"getTokenAccountBalance\"")
                        && request.contains(&expected_ata)
                    {
                        let request_index =
                            token_balance_request_count.fetch_add(1, Ordering::Relaxed);
                        match token_balances
                            .get(request_index)
                            .cloned()
                            .unwrap_or_else(|| *token_balances.last().expect("last token balance"))
                        {
                            Some(amount) => mock_token_account_balance_body(amount),
                            None => "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32602,\"message\":\"Invalid param: could not find account\"},\"id\":1}".to_string(),
                        }
                    } else if request.contains("\"getSignatureStatuses\"") {
                        let request_index =
                            signature_status_request_count.fetch_add(1, Ordering::Relaxed);
                        match signature_statuses
                            .get(request_index)
                            .cloned()
                            .unwrap_or_else(|| {
                                *signature_statuses.last().expect("last signature status")
                            }) {
                            Some(slot) => mock_signature_status_body(slot),
                            None => {
                                "{\"jsonrpc\":\"2.0\",\"result\":{\"context\":{\"slot\":1},\"value\":[null]},\"id\":1}".to_string()
                            }
                        }
                    } else if request.contains("\"getVersion\"") {
                        "{\"jsonrpc\":\"2.0\",\"result\":{\"solana-core\":\"1.18.26\",\"feature-set\":1},\"id\":1}".to_string()
                    } else {
                        "{\"jsonrpc\":\"2.0\",\"result\":\"ok\",\"id\":1}".to_string()
                    };

                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                }
            }
        });

        format!("http://{}", addr)
    }

    async fn spawn_blockhash_cache_rpc_server() -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind blockhash rpc");
        let addr = listener.local_addr().expect("blockhash rpc addr");
        let latest_blockhash_count = Arc::new(AtomicUsize::new(0));
        let block_height_count = Arc::new(AtomicUsize::new(0));
        let blockhashes = Arc::new(vec![
            Hash::new_unique(),
            Hash::new_unique(),
            Hash::new_unique(),
        ]);

        tokio::spawn({
            let latest_blockhash_count = Arc::clone(&latest_blockhash_count);
            let block_height_count = Arc::clone(&block_height_count);
            let blockhashes = Arc::clone(&blockhashes);
            async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        break;
                    };
                    let mut buffer = vec![0u8; 16_384];
                    let n = match stream.read(&mut buffer).await {
                        Ok(n) if n > 0 => n,
                        _ => continue,
                    };
                    let request = String::from_utf8_lossy(&buffer[..n]);
                    let body = if request.contains("\"getLatestBlockhash\"") {
                        let request_index = latest_blockhash_count.fetch_add(1, Ordering::Relaxed);
                        let blockhash = blockhashes
                            .get(request_index)
                            .copied()
                            .unwrap_or_else(|| *blockhashes.last().expect("last blockhash"));
                        mock_latest_blockhash_body(blockhash, 10_000 + request_index as u64)
                    } else if request.contains("\"getBlockHeight\"") {
                        let request_index = block_height_count.fetch_add(1, Ordering::Relaxed);
                        format!(
                            "{{\"jsonrpc\":\"2.0\",\"result\":{},\"id\":1}}",
                            1_000 + request_index as u64
                        )
                    } else if request.contains("\"getVersion\"") {
                        "{\"jsonrpc\":\"2.0\",\"result\":{\"solana-core\":\"1.18.26\",\"feature-set\":1},\"id\":1}".to_string()
                    } else {
                        "{\"jsonrpc\":\"2.0\",\"result\":\"ok\",\"id\":1}".to_string()
                    };

                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                }
            }
        });

        (
            format!("http://{}", addr),
            latest_blockhash_count,
            block_height_count,
        )
    }

    async fn spawn_prepare_buy_rpc_server(
        payer_pubkey: Pubkey,
        mint: Pubkey,
        user_ata: Pubkey,
        mint_owner: Pubkey,
    ) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind prepare-buy rpc");
        let addr = listener.local_addr().expect("prepare-buy rpc addr");
        let payer_pubkey = payer_pubkey.to_string();
        let mint = mint.to_string();
        let user_ata = user_ata.to_string();
        let system_owner = system_program::id();

        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buffer = vec![0u8; 16_384];
                let n = match stream.read(&mut buffer).await {
                    Ok(n) if n > 0 => n,
                    _ => continue,
                };
                let request = String::from_utf8_lossy(&buffer[..n]);
                let body = if request.contains("\"getBalance\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":{\"context\":{\"slot\":1},\"value\":1000000000},\"id\":1}".to_string()
                } else if request.contains("\"getAccountInfo\"") && request.contains(&payer_pubkey)
                {
                    mock_account_info_body(1_000_000_000, &system_owner)
                } else if request.contains("\"getAccountInfo\"") && request.contains(&mint) {
                    mock_account_info_body(1, &mint_owner)
                } else if request.contains("\"getAccountInfo\"") && request.contains(&user_ata) {
                    "{\"jsonrpc\":\"2.0\",\"result\":{\"context\":{\"slot\":1},\"value\":null},\"id\":1}".to_string()
                } else if request.contains("\"getMinimumBalanceForRentExemption\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":2074080,\"id\":1}".to_string()
                } else if request.contains("\"getLatestBlockhash\"") {
                    mock_latest_blockhash_body(Hash::new_unique(), 10_000)
                } else if request.contains("\"getBlockHeight\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":1000,\"id\":1}".to_string()
                } else if request.contains("\"getVersion\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":{\"solana-core\":\"1.18.26\",\"feature-set\":1},\"id\":1}".to_string()
                } else {
                    "{\"jsonrpc\":\"2.0\",\"result\":\"ok\",\"id\":1}".to_string()
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });

        format!("http://{}", addr)
    }

    async fn spawn_ata_rent_rpc_server(rent_sequence: Vec<u64>) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ata rent rpc");
        let addr = listener.local_addr().expect("ata rent rpc addr");
        let rent_sequence = Arc::new(rent_sequence);
        let rent_request_count = Arc::new(AtomicUsize::new(0));

        tokio::spawn({
            let rent_sequence = Arc::clone(&rent_sequence);
            let rent_request_count = Arc::clone(&rent_request_count);
            async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        break;
                    };
                    let mut buffer = vec![0u8; 16_384];
                    let n = match stream.read(&mut buffer).await {
                        Ok(n) if n > 0 => n,
                        _ => continue,
                    };
                    let request = String::from_utf8_lossy(&buffer[..n]);
                    let body = if request.contains("\"getMinimumBalanceForRentExemption\"") {
                        let request_index = rent_request_count.fetch_add(1, Ordering::Relaxed);
                        let rent_lamports = rent_sequence
                            .get(request_index)
                            .copied()
                            .unwrap_or_else(|| *rent_sequence.last().expect("last rent value"));
                        format!(
                            "{{\"jsonrpc\":\"2.0\",\"result\":{},\"id\":1}}",
                            rent_lamports
                        )
                    } else if request.contains("\"getVersion\"") {
                        "{\"jsonrpc\":\"2.0\",\"result\":{\"solana-core\":\"1.18.26\",\"feature-set\":1},\"id\":1}".to_string()
                    } else {
                        "{\"jsonrpc\":\"2.0\",\"result\":\"ok\",\"id\":1}".to_string()
                    };

                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                }
            }
        });

        (format!("http://{}", addr), rent_request_count)
    }

    #[derive(Clone, Copy)]
    enum TokenBalanceRpcResponse {
        Amount(u64),
        Missing,
        RpcError,
    }

    #[derive(Clone, Copy)]
    struct PrepareBuyAtaProbeRpcScenario {
        user_ata_exists: bool,
        token_balance: TokenBalanceRpcResponse,
        rent_lamports: u64,
    }

    async fn spawn_prepare_buy_ata_probe_rpc_server(
        payer_pubkey: Pubkey,
        mint: Pubkey,
        user_ata: Pubkey,
        token_program: Pubkey,
        scenario: PrepareBuyAtaProbeRpcScenario,
    ) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind prepare-buy ata probe rpc");
        let addr = listener
            .local_addr()
            .expect("prepare-buy ata probe rpc addr");
        let payer_pubkey = payer_pubkey.to_string();
        let mint = mint.to_string();
        let user_ata = user_ata.to_string();
        let system_owner = system_program::id();

        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buffer = vec![0u8; 16_384];
                let n = match stream.read(&mut buffer).await {
                    Ok(n) if n > 0 => n,
                    _ => continue,
                };
                let request = String::from_utf8_lossy(&buffer[..n]);
                let body = if request.contains("\"getBalance\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":{\"context\":{\"slot\":1},\"value\":1000000000},\"id\":1}".to_string()
                } else if request.contains("\"getAccountInfo\"") && request.contains(&payer_pubkey)
                {
                    mock_account_info_body(1_000_000_000, &system_owner)
                } else if request.contains("\"getAccountInfo\"") && request.contains(&mint) {
                    mock_account_info_body(1, &token_program)
                } else if request.contains("\"getAccountInfo\"") && request.contains(&user_ata) {
                    if scenario.user_ata_exists {
                        mock_account_info_body(1, &token_program)
                    } else {
                        "{\"jsonrpc\":\"2.0\",\"result\":{\"context\":{\"slot\":1},\"value\":null},\"id\":1}".to_string()
                    }
                } else if request.contains("\"getTokenAccountBalance\"")
                    && request.contains(&user_ata)
                {
                    match scenario.token_balance {
                        TokenBalanceRpcResponse::Amount(amount) => {
                            mock_token_account_balance_body(amount)
                        }
                        TokenBalanceRpcResponse::Missing => {
                            "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32602,\"message\":\"Invalid param: could not find account\"},\"id\":1}".to_string()
                        }
                        TokenBalanceRpcResponse::RpcError => {
                            "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32000,\"message\":\"rpc exploded\"},\"id\":1}".to_string()
                        }
                    }
                } else if request.contains("\"getMinimumBalanceForRentExemption\"") {
                    format!(
                        "{{\"jsonrpc\":\"2.0\",\"result\":{},\"id\":1}}",
                        scenario.rent_lamports
                    )
                } else if request.contains("\"getLatestBlockhash\"") {
                    mock_latest_blockhash_body(Hash::new_unique(), 10_000)
                } else if request.contains("\"getBlockHeight\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":1000,\"id\":1}".to_string()
                } else if request.contains("\"getVersion\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":{\"solana-core\":\"1.18.26\",\"feature-set\":1},\"id\":1}".to_string()
                } else {
                    "{\"jsonrpc\":\"2.0\",\"result\":\"ok\",\"id\":1}".to_string()
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });

        format!("http://{}", addr)
    }

    struct ParallelPrepareRpcMetrics {
        max_in_flight: Arc<AtomicUsize>,
        legacy_ata_requests: Arc<AtomicUsize>,
        token_2022_ata_requests: Arc<AtomicUsize>,
    }

    async fn spawn_parallel_prepare_buy_rpc_server(
        payer_pubkey: Pubkey,
        mint: Pubkey,
        legacy_user_ata: Pubkey,
        token_2022_user_ata: Pubkey,
        mint_owner: Pubkey,
        delay_ms: u64,
        legacy_ata_exists: bool,
        token_2022_ata_exists: bool,
    ) -> (String, ParallelPrepareRpcMetrics) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind parallel prepare rpc");
        let addr = listener.local_addr().expect("parallel prepare rpc addr");
        let payer_pubkey = payer_pubkey.to_string();
        let mint = mint.to_string();
        let legacy_user_ata = legacy_user_ata.to_string();
        let token_2022_user_ata = token_2022_user_ata.to_string();
        let system_owner = system_program::id();
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let legacy_ata_requests = Arc::new(AtomicUsize::new(0));
        let token_2022_ata_requests = Arc::new(AtomicUsize::new(0));

        tokio::spawn({
            let max_in_flight = Arc::clone(&max_in_flight);
            let in_flight = Arc::clone(&in_flight);
            let legacy_ata_requests = Arc::clone(&legacy_ata_requests);
            let token_2022_ata_requests = Arc::clone(&token_2022_ata_requests);
            async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        break;
                    };
                    let max_in_flight = Arc::clone(&max_in_flight);
                    let in_flight = Arc::clone(&in_flight);
                    let legacy_ata_requests = Arc::clone(&legacy_ata_requests);
                    let token_2022_ata_requests = Arc::clone(&token_2022_ata_requests);
                    let payer_pubkey = payer_pubkey.clone();
                    let mint = mint.clone();
                    let legacy_user_ata = legacy_user_ata.clone();
                    let token_2022_user_ata = token_2022_user_ata.clone();
                    tokio::spawn(async move {
                        let mut stream = stream;
                        let mut buffer = vec![0u8; 16_384];
                        let n = match stream.read(&mut buffer).await {
                            Ok(n) if n > 0 => n,
                            _ => return,
                        };
                        let request = String::from_utf8_lossy(&buffer[..n]).to_string();
                        let delayed = request.contains("\"getBalance\"")
                            || (request.contains("\"getAccountInfo\"")
                                && (request.contains(&payer_pubkey)
                                    || request.contains(&mint)
                                    || request.contains(&legacy_user_ata)
                                    || request.contains(&token_2022_user_ata)));
                        if delayed {
                            let current = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                            let _ = max_in_flight.fetch_update(
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                                |existing| (current > existing).then_some(current),
                            );
                            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                            in_flight.fetch_sub(1, Ordering::SeqCst);
                        }

                        let body = if request.contains("\"getBalance\"") {
                            "{\"jsonrpc\":\"2.0\",\"result\":{\"context\":{\"slot\":1},\"value\":1000000000},\"id\":1}".to_string()
                        } else if request.contains("\"getAccountInfo\"")
                            && request.contains(&payer_pubkey)
                        {
                            mock_account_info_body(1_000_000_000, &system_owner)
                        } else if request.contains("\"getAccountInfo\"") && request.contains(&mint)
                        {
                            mock_account_info_body(1, &mint_owner)
                        } else if request.contains("\"getAccountInfo\"")
                            && request.contains(&legacy_user_ata)
                        {
                            legacy_ata_requests.fetch_add(1, Ordering::Relaxed);
                            if legacy_ata_exists {
                                mock_account_info_body(
                                    1,
                                    &Pubkey::from_str(TOKEN_PROGRAM_ID)
                                        .expect("valid token program"),
                                )
                            } else {
                                "{\"jsonrpc\":\"2.0\",\"result\":{\"context\":{\"slot\":1},\"value\":null},\"id\":1}".to_string()
                            }
                        } else if request.contains("\"getAccountInfo\"")
                            && request.contains(&token_2022_user_ata)
                        {
                            token_2022_ata_requests.fetch_add(1, Ordering::Relaxed);
                            if token_2022_ata_exists {
                                mock_account_info_body(
                                    1,
                                    &Pubkey::from_str(TOKEN_2022_PROGRAM_ID)
                                        .expect("valid token2022"),
                                )
                            } else {
                                "{\"jsonrpc\":\"2.0\",\"result\":{\"context\":{\"slot\":1},\"value\":null},\"id\":1}".to_string()
                            }
                        } else if request.contains("\"getTokenAccountBalance\"")
                            && request.contains(&legacy_user_ata)
                        {
                            legacy_ata_requests.fetch_add(1, Ordering::Relaxed);
                            if legacy_ata_exists {
                                mock_token_account_balance_body(777)
                            } else {
                                "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32602,\"message\":\"Invalid param: could not find account\"},\"id\":1}".to_string()
                            }
                        } else if request.contains("\"getTokenAccountBalance\"")
                            && request.contains(&token_2022_user_ata)
                        {
                            token_2022_ata_requests.fetch_add(1, Ordering::Relaxed);
                            if token_2022_ata_exists {
                                mock_token_account_balance_body(888)
                            } else {
                                "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32602,\"message\":\"Invalid param: could not find account\"},\"id\":1}".to_string()
                            }
                        } else if request.contains("\"getMinimumBalanceForRentExemption\"") {
                            "{\"jsonrpc\":\"2.0\",\"result\":2074080,\"id\":1}".to_string()
                        } else if request.contains("\"getLatestBlockhash\"") {
                            mock_latest_blockhash_body(Hash::new_unique(), 10_000)
                        } else if request.contains("\"getBlockHeight\"") {
                            "{\"jsonrpc\":\"2.0\",\"result\":1000,\"id\":1}".to_string()
                        } else if request.contains("\"getVersion\"") {
                            "{\"jsonrpc\":\"2.0\",\"result\":{\"solana-core\":\"1.18.26\",\"feature-set\":1},\"id\":1}".to_string()
                        } else {
                            "{\"jsonrpc\":\"2.0\",\"result\":\"ok\",\"id\":1}".to_string()
                        };

                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.shutdown().await;
                    });
                }
            }
        });

        (
            format!("http://{}", addr),
            ParallelPrepareRpcMetrics {
                max_in_flight,
                legacy_ata_requests,
                token_2022_ata_requests,
            },
        )
    }

    async fn spawn_sender_submission_server() -> (
        String,
        Arc<AtomicUsize>,
        Arc<std::sync::Mutex<Vec<Signature>>>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind sender");
        let addr = listener.local_addr().expect("sender addr");
        let send_count = Arc::new(AtomicUsize::new(0));
        let seen_signatures = Arc::new(std::sync::Mutex::new(Vec::<Signature>::new()));

        tokio::spawn({
            let send_count = Arc::clone(&send_count);
            let seen_signatures = Arc::clone(&seen_signatures);
            async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        break;
                    };
                    let mut buffer = vec![0u8; 16_384];
                    let n = match stream.read(&mut buffer).await {
                        Ok(n) if n > 0 => n,
                        _ => continue,
                    };
                    let request = String::from_utf8_lossy(&buffer[..n]);
                    let json_body = request.split("\r\n\r\n").nth(1).unwrap_or("{}");
                    let request_json: serde_json::Value =
                        serde_json::from_str(json_body).expect("sender request json");
                    let encoded = request_json["params"][0]
                        .as_str()
                        .expect("sender transaction payload");
                    let tx_bytes = base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .expect("decode sender transaction");
                    let tx: VersionedTransaction =
                        bincode::deserialize(&tx_bytes).expect("deserialize sender transaction");
                    let signature = tx.signatures[0];

                    send_count.fetch_add(1, Ordering::Relaxed);
                    seen_signatures
                        .lock()
                        .expect("sender signatures lock")
                        .push(signature);

                    let body = format!(
                        "{{\"jsonrpc\":\"2.0\",\"result\":\"{}\",\"id\":\"ghost-live-sender\"}}",
                        signature
                    );
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                }
            }
        });

        (format!("http://{}/fast", addr), send_count, seen_signatures)
    }

    async fn spawn_sender_retry_rpc_server(
        expected_ata: Pubkey,
        send_count: Arc<AtomicUsize>,
        seen_signatures: Arc<std::sync::Mutex<Vec<Signature>>>,
    ) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind retry rpc");
        let addr = listener.local_addr().expect("retry rpc addr");
        let expected_ata = expected_ata.to_string();

        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buffer = vec![0u8; 16_384];
                let n = match stream.read(&mut buffer).await {
                    Ok(n) if n > 0 => n,
                    _ => continue,
                };
                let request = String::from_utf8_lossy(&buffer[..n]);
                let json_body = request.split("\r\n\r\n").nth(1).unwrap_or("{}");
                let request_json: serde_json::Value =
                    serde_json::from_str(json_body).unwrap_or(serde_json::Value::Null);
                let send_attempts = send_count.load(Ordering::Relaxed);

                let body = if request.contains("\"getTokenAccountBalance\"")
                    && request.contains(&expected_ata)
                {
                    if send_attempts >= 3 {
                        mock_token_account_balance_body(2_500)
                    } else {
                        mock_token_account_balance_body(0)
                    }
                } else if request.contains("\"getSignatureStatuses\"") {
                    let queried_signatures = request_json["params"][0]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    let seen = seen_signatures
                        .lock()
                        .expect("retry signatures lock")
                        .clone();
                    let latest_signature = seen.last().copied();
                    let statuses = queried_signatures
                        .iter()
                        .map(|value| {
                            let signature =
                                value.as_str().and_then(|raw| Signature::from_str(raw).ok());
                            if send_attempts >= 3
                                && signature == latest_signature
                                && seen
                                    .iter()
                                    .any(|candidate| Some(*candidate) != latest_signature)
                            {
                                Some(999)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    mock_signature_statuses_body(&statuses)
                } else if request.contains("\"getLatestBlockhash\"") {
                    mock_latest_blockhash_body(Hash::new_unique(), 10_000)
                } else if request.contains("\"getBlockHeight\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":1000,\"id\":1}".to_string()
                } else if request.contains("\"getVersion\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":{\"solana-core\":\"1.18.26\",\"feature-set\":1},\"id\":1}".to_string()
                } else {
                    "{\"jsonrpc\":\"2.0\",\"result\":\"ok\",\"id\":1}".to_string()
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });

        format!("http://{}", addr)
    }

    fn build_test_prepared_buy_request(
        trigger: &TriggerComponent,
        payer: &Keypair,
        mint: &Pubkey,
        token_program: &Pubkey,
        ata_missing_pre_submit: bool,
        pre_submit_token_balance: Option<u64>,
    ) -> PreparedBuyRequest {
        trigger
            .build_prepared_buy_request_with_transport(
                payer,
                "configured",
                mint,
                token_program,
                true,
                ata_missing_pre_submit,
                &valid_buy_account_overrides(),
                100_000,
                HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS,
                HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
                pre_submit_token_balance,
                &Pubkey::new_unique(),
                Hash::new_unique(),
            )
            .expect("test prepared buy request")
    }

    struct MockShadowSimulator;

    #[async_trait]
    impl ShadowSimulator for MockShadowSimulator {
        async fn simulate_buy(
            &self,
            request: &PreparedBuyRequest,
            config: &crate::config::TriggerShadowRunConfig,
        ) -> Result<crate::components::trigger::shadow_run::ShadowBuySimulationReport> {
            Ok(
                crate::components::trigger::shadow_run::ShadowBuySimulationReport {
                    join_metadata: request.join_metadata.clone(),
                    mint: request.mint.to_string(),
                    live_signature: None,
                    payer_pubkey: request.payer_pubkey.to_string(),
                    payer_provenance: request.payer_provenance.to_string(),
                    amount_lamports: request.amount_lamports,
                    entry_token_amount_raw: request.entry_token_amount_raw,
                    tip_lamports: request.tip_lamports,
                    decision_ts_ms: request.decision_ts_ms,
                    simulation_started_ts_ms: request.decision_ts_ms,
                    simulation_finished_ts_ms: request.decision_ts_ms + 5,
                    latency_ms: 5,
                    shadow_duration_ms: 5,
                    rpc_slot: 777,
                    retry_count: 0,
                    used_sig_verify: config.sig_verify,
                    used_replace_recent_blockhash: config.replace_recent_blockhash,
                    units_consumed: Some(42_000),
                    logs: vec!["mock-shadow-log".to_string()],
                    return_data: None,
                    err: None,
                },
            )
        }
    }

    struct ConcurrencyTrackingShadowSimulator {
        in_flight: Arc<AtomicUsize>,
        max_in_flight: Arc<AtomicUsize>,
        completed: Arc<AtomicUsize>,
        delay: Duration,
    }

    #[async_trait]
    impl ShadowSimulator for ConcurrencyTrackingShadowSimulator {
        async fn simulate_buy(
            &self,
            request: &PreparedBuyRequest,
            config: &crate::config::TriggerShadowRunConfig,
        ) -> Result<crate::components::trigger::shadow_run::ShadowBuySimulationReport> {
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            let _ =
                self.max_in_flight
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |existing| {
                        (current > existing).then_some(current)
                    });
            tokio::time::sleep(self.delay).await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            self.completed.fetch_add(1, Ordering::SeqCst);
            Ok(
                crate::components::trigger::shadow_run::ShadowBuySimulationReport {
                    join_metadata: request.join_metadata.clone(),
                    mint: request.mint.to_string(),
                    live_signature: None,
                    payer_pubkey: request.payer_pubkey.to_string(),
                    payer_provenance: request.payer_provenance.to_string(),
                    amount_lamports: request.amount_lamports,
                    entry_token_amount_raw: request.entry_token_amount_raw,
                    tip_lamports: request.tip_lamports,
                    decision_ts_ms: request.decision_ts_ms,
                    simulation_started_ts_ms: request.decision_ts_ms,
                    simulation_finished_ts_ms: request.decision_ts_ms
                        + self.delay.as_millis() as u64,
                    latency_ms: self.delay.as_millis() as u64,
                    shadow_duration_ms: self.delay.as_millis() as u64,
                    rpc_slot: 777,
                    retry_count: 0,
                    used_sig_verify: config.sig_verify,
                    used_replace_recent_blockhash: config.replace_recent_blockhash,
                    units_consumed: Some(42_000),
                    logs: vec!["tracked-shadow-log".to_string()],
                    return_data: None,
                    err: None,
                },
            )
        }
    }

    #[test]
    fn test_trigger_component_exposes_entry_mode() {
        let mut config = create_test_config();
        config.entry_mode = crate::config::TriggerEntryMode::ShadowOnly;

        let trigger = TriggerComponent::new(config);
        assert_eq!(
            trigger.entry_mode(),
            crate::config::TriggerEntryMode::ShadowOnly
        );
    }

    #[test]
    fn test_handle_legacy_pool_scored_event_blocks_authoritative_buy() {
        let mut pending_pools = HashMap::new();
        pending_pools.insert(
            "pool".to_string(),
            Arc::new(DetectedPool {
                semantic: ghost_core::EventSemanticEnvelope::default(),
                pool_amm_id: "pool".to_string(),
                base_mint: "mint".to_string(),
                quote_mint: "SOL".to_string(),
                amm_program: "amm".to_string(),
                bonding_curve: "curve".to_string(),
                creator: "creator".to_string(),
                slot: Some(1),
                timestamp_ms: 1,
                event_time: ghost_core::EventTimeMetadata::default(),
                detected_wall_ts_ms: Some(1),
                initial_liquidity_sol: Some(1.0),
                signature: "sig".to_string(),
            }),
        );

        let handled = handle_legacy_pool_scored_event(
            &mut pending_pools,
            &crate::events::PoolScoredEvent {
                pool_amm_id: "pool".to_string(),
                base_mint: "mint".to_string(),
                score: 95.0,
                passed: true,
                risk_level: "low".to_string(),
                interpretation: "legacy".to_string(),
                processing_time_us: 1,
                component_scores: serde_json::json!({}),
            },
        );

        assert!(handled.blocked_side_effect);
        assert!(handled.removed_pending_pool);
        assert!(pending_pools.is_empty());
    }

    #[test]
    fn test_phase6_legacy_path_descriptors_disallow_authoritative_buy() {
        let descriptors = [
            TRIGGER_POOL_SCORED_OBSERVER_PATH,
            TRIGGER_EMBEDDED_ORACLE_PIPELINE_PATH,
            TRIGGER_NO_EVENT_BUS_PATH,
        ];

        assert!(descriptors
            .iter()
            .all(|descriptor| !descriptor.allows_authoritative_buy));
        assert_eq!(
            TRIGGER_POOL_SCORED_OBSERVER_PATH.classification,
            LegacyPathClassification::ObservabilityOnly
        );
        assert_eq!(
            TRIGGER_EMBEDDED_ORACLE_PIPELINE_PATH.classification,
            LegacyPathClassification::CompatibilityOnly
        );
        assert_eq!(
            TRIGGER_NO_EVENT_BUS_PATH.classification,
            LegacyPathClassification::DisabledInProduction
        );
    }

    #[test]
    fn test_shadow_only_prepares_on_shadow_rpc() {
        let mut config = create_test_config();
        config.entry_mode = crate::config::TriggerEntryMode::ShadowOnly;
        config.rpc_url = "https://primary.invalid".to_string();
        config.shadow_run.enabled = true;
        config.shadow_run.shadow_rpc_url = "https://shadow.invalid".to_string();

        let trigger = TriggerComponent::new(config);
        assert!(trigger.should_prepare_on_shadow_rpc());
        assert!(trigger.secondary_shadow_rpc().is_none());
    }

    #[test]
    fn test_live_and_shadow_keeps_primary_prepare_rpc() {
        let mut config = create_test_config();
        config.entry_mode = crate::config::TriggerEntryMode::LiveAndShadow;
        config.rpc_url = "https://primary.invalid".to_string();
        config.shadow_run.enabled = true;
        config.shadow_run.shadow_rpc_url = "https://shadow.invalid".to_string();

        let trigger = TriggerComponent::new(config);
        assert!(!trigger.should_prepare_on_shadow_rpc());
        assert!(trigger.secondary_shadow_rpc().is_some());
    }

    #[test]
    fn test_sanitize_fee_recipient_override_drops_known_bad_legacy_address() {
        let known_bad =
            Pubkey::from_str(KNOWN_BAD_LEGACY_FEE_RECIPIENT).expect("known bad fee recipient");
        assert_eq!(
            TriggerComponent::sanitize_fee_recipient_override(Some(known_bad)),
            None
        );
    }

    #[test]
    fn test_sanitize_fee_recipient_override_keeps_primary_global_fee_address() {
        let current = Pubkey::from_str("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")
            .expect("primary global fee recipient");
        assert_eq!(
            TriggerComponent::sanitize_fee_recipient_override(Some(current)),
            Some(current)
        );
    }

    #[test]
    fn test_sanitize_fee_recipient_override_keeps_reserved_fee_address() {
        let reserved = Pubkey::from_str("GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS")
            .expect("reserved fee recipient");
        assert_eq!(
            TriggerComponent::sanitize_fee_recipient_override(Some(reserved)),
            Some(reserved)
        );
    }

    #[test]
    fn test_sanitize_fee_recipient_override_drops_unauthorized_observed_address() {
        let observed = Pubkey::new_unique();
        assert_eq!(
            TriggerComponent::sanitize_fee_recipient_override(Some(observed)),
            None
        );
    }

    #[test]
    fn test_sanitize_global_config_override_keeps_canonical_value() {
        let canonical = DirectBuyBuilder::canonical_global_config();
        assert_eq!(
            TriggerComponent::sanitize_global_config_override(Some(canonical)),
            Some(canonical)
        );
    }

    #[test]
    fn test_sanitize_global_config_override_drops_noncanonical_value() {
        assert_eq!(
            TriggerComponent::sanitize_global_config_override(Some(Pubkey::new_unique())),
            None
        );
    }

    #[test]
    fn test_sanitize_buy_variant_override_keeps_routed() {
        assert_eq!(
            TriggerComponent::sanitize_buy_variant_override(Some(
                trigger::PumpfunBuyVariant::RoutedExactSolIn,
            )),
            Some(trigger::PumpfunBuyVariant::RoutedExactSolIn)
        );
    }

    #[test]
    fn test_sanitize_buy_variant_override_drops_legacy() {
        assert_eq!(
            TriggerComponent::sanitize_buy_variant_override(Some(
                trigger::PumpfunBuyVariant::LegacyBuy,
            )),
            None
        );
    }

    #[test]
    fn test_prepared_request_buy_variant_sanitizer_keeps_legacy_with_curve_proof() {
        assert_eq!(
            TriggerComponent::sanitize_buy_variant_override_for_prepared_request(
                Some(trigger::PumpfunBuyVariant::LegacyBuy),
                true,
            ),
            Some(trigger::PumpfunBuyVariant::LegacyBuy)
        );
        assert_eq!(
            TriggerComponent::sanitize_buy_variant_override_for_prepared_request(
                Some(trigger::PumpfunBuyVariant::LegacyBuy),
                false,
            ),
            None
        );
    }

    #[test]
    fn test_sanitize_associated_bonding_curve_override_accepts_canonical_value() {
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let canonical = DirectBuyBuilder::canonical_associated_bonding_curve(&mint, &token_program);

        assert_eq!(
            TriggerComponent::sanitize_associated_bonding_curve_override(
                &mint,
                &token_program,
                Some(canonical),
            ),
            Some(canonical)
        );
    }

    #[test]
    fn test_sanitize_associated_bonding_curve_override_drops_invalid_value() {
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");

        assert_eq!(
            TriggerComponent::sanitize_associated_bonding_curve_override(
                &mint,
                &token_program,
                Some(Pubkey::new_unique()),
            ),
            None
        );
    }

    #[test]
    fn test_build_pool_account_overrides_uses_detected_pool_creator() {
        let creator = Pubkey::new_unique();
        let pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "pool".to_string(),
            base_mint: "mint".to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "amm".to_string(),
            bonding_curve: "curve".to_string(),
            creator: creator.to_string(),
            slot: Some(1),
            timestamp_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let overrides = TriggerComponent::build_pool_account_overrides(&pool);

        assert_eq!(overrides.creator_pubkey, Some(creator));
    }

    #[test]
    fn test_build_pool_account_overrides_filters_default_creator() {
        let pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "pool".to_string(),
            base_mint: "mint".to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "amm".to_string(),
            bonding_curve: "curve".to_string(),
            creator: Pubkey::default().to_string(),
            slot: Some(1),
            timestamp_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let overrides = TriggerComponent::build_pool_account_overrides(&pool);

        assert_eq!(overrides.creator_pubkey, None);
    }

    #[test]
    fn test_user_ata_account_len_token2022_exceeds_legacy() {
        let legacy = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let token_2022 = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022");

        let legacy_len = TriggerComponent::user_ata_account_len(&legacy).unwrap();
        let token_2022_len = TriggerComponent::user_ata_account_len(&token_2022).unwrap();

        assert!(token_2022_len > legacy_len);
    }

    #[test]
    fn test_build_prepared_buy_request_contains_expected_tip_and_amount() {
        let trigger = TriggerComponent::new(create_test_config());
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let recent_blockhash = Hash::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let overrides = valid_buy_account_overrides();
        let expected_amount = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");

        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                expected_amount,
                2_500_000,
                recent_blockhash,
            )
            .expect("prepared buy request should build");
        assert_eq!(request.mint, mint);
        assert_eq!(request.payer_pubkey, payer.pubkey());
        assert_eq!(request.amount_lamports, expected_amount);
        assert_eq!(request.tip_lamports, 2_500_000);
        assert_eq!(request.min_tokens_out, 1);
        assert_eq!(request.recent_blockhash, recent_blockhash);
        assert_eq!(
            request.trade_value_sol,
            expected_amount as f64 / LAMPORTS_PER_SOL
        );
        assert!(request.tip_tx.is_none());
        let message = match &request.buy_tx.message {
            VersionedMessage::V0(message) => message,
            _ => panic!("expected v0 buy transaction"),
        };
        assert!(
            message.instructions.len() >= 3,
            "buy transaction should include compute budget + buy path"
        );
    }

    #[test]
    fn test_build_prepared_buy_request_uses_ghost_core_curve_for_legacy_buy() {
        let trigger = TriggerComponent::new(create_test_config());
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let recent_blockhash = Hash::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 1_000_000_000_000,
            real_sol_reserves: 30_000_000_000,
            token_total_supply: 0,
            complete: 0,
            _padding: [0; 7],
        };
        let expected_amount = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let expected_token_amount =
            apply_slippage_bps(curve.simulate_buy(expected_amount), 2_000).max(1);
        let overrides = BuyAccountOverrides {
            creator_pubkey: Some(Pubkey::new_unique()),
            buy_variant: Some(trigger::PumpfunBuyVariant::LegacyBuy),
            legacy_buy_curve: Some(curve),
            ..BuyAccountOverrides::default()
        };

        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                expected_amount,
                0,
                recent_blockhash,
            )
            .expect("legacy prepared buy request should build");

        assert_eq!(request.min_tokens_out, expected_token_amount);
        let buy_ix = DirectBuyBuilder::build_buy_ix_with_accounts(
            &payer.pubkey(),
            &mint,
            &token_program,
            None,
            None,
            overrides.creator_pubkey,
            overrides.buy_variant,
            None,
            request.amount_lamports,
            request.min_tokens_out,
        );
        assert_eq!(
            u64::from_le_bytes(buy_ix.data[8..16].try_into().unwrap()),
            expected_token_amount
        );
        assert_eq!(
            u64::from_le_bytes(buy_ix.data[16..24].try_into().unwrap()),
            expected_amount
        );
    }

    #[test]
    fn test_build_prepared_buy_request_uses_account_state_slippage_for_routed_buy_when_available() {
        let mint = Pubkey::new_unique();
        let account_state_core = Arc::new(AccountStateReducer::new());
        seed_canonical_buy_state(&account_state_core, mint);
        let trigger = TriggerComponent::new_with_position_limit_tracker_and_runtime_state(
            create_test_config(),
            PositionLimitTracker::new(1),
            Arc::new(ShadowLedger::new()),
            Arc::clone(&account_state_core),
        );
        let payer = Keypair::new();
        let recent_blockhash = Hash::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let overrides = valid_buy_account_overrides();
        let expected_amount = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let curve = account_state_core
            .bonding_curve(&mint)
            .expect("seeded curve must be available");
        let expected_min_tokens_out =
            apply_slippage_bps(curve.simulate_buy(expected_amount), 2_000).max(1);

        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                expected_amount,
                0,
                recent_blockhash,
            )
            .expect("routed prepared buy request should build");

        assert_eq!(request.min_tokens_out, expected_min_tokens_out);
        let buy_ix = DirectBuyBuilder::build_buy_ix_with_accounts(
            &payer.pubkey(),
            &mint,
            &token_program,
            None,
            None,
            overrides.creator_pubkey,
            None,
            None,
            request.amount_lamports,
            request.min_tokens_out,
        );
        assert_eq!(
            u64::from_le_bytes(buy_ix.data[8..16].try_into().unwrap()),
            expected_amount
        );
        assert_eq!(
            u64::from_le_bytes(buy_ix.data[16..24].try_into().unwrap()),
            expected_min_tokens_out
        );
    }

    #[test]
    fn test_build_prepared_buy_request_fails_closed_for_legacy_buy_without_curve() {
        let trigger = TriggerComponent::new(create_test_config());
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let recent_blockhash = Hash::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let expected_amount = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let overrides = BuyAccountOverrides {
            creator_pubkey: Some(Pubkey::new_unique()),
            buy_variant: Some(trigger::PumpfunBuyVariant::LegacyBuy),
            ..BuyAccountOverrides::default()
        };

        let err = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                expected_amount,
                0,
                recent_blockhash,
            )
            .expect_err("legacy buy without curve must fail closed");

        assert!(
            err.to_string()
                .contains("Missing canonical ghost-core curve for legacy_buy trigger buy"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_build_prepared_buy_request_preserves_live_payload() {
        let trigger = TriggerComponent::new(create_test_config());
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let recent_blockhash = Hash::new_unique();
        let tip_lamports = 3_000_000;
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let overrides = valid_buy_account_overrides();
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");

        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                amount_lamports,
                tip_lamports,
                recent_blockhash,
            )
            .expect("prepared buy request should build");

        let buy_ix = DirectBuyBuilder::build_buy_ix_with_accounts(
            &payer.pubkey(),
            &mint,
            &token_program,
            None,
            None,
            overrides.creator_pubkey,
            None,
            None,
            request.amount_lamports,
            request.min_tokens_out,
        );
        let tip_seed = format!("{mint}:{recent_blockhash}");
        let tip_account = select_sender_tip_account(tip_seed.as_bytes());
        let ata_ix = create_associated_token_account_idempotent(
            &payer.pubkey(),
            &payer.pubkey(),
            &mint,
            &token_program,
        );
        let tip_ix = system_instruction::transfer(&payer.pubkey(), &tip_account, tip_lamports);
        let expected_instructions = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(BUY_COMPUTE_UNIT_LIMIT),
            ComputeBudgetInstruction::set_compute_unit_price(
                HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
            ),
            ata_ix,
            buy_ix,
            tip_ix,
        ];
        let manual_buy_tx = VersionedTransaction::try_new(
            VersionedMessage::V0(
                v0::Message::try_compile(
                    &payer.pubkey(),
                    &expected_instructions,
                    &[],
                    recent_blockhash,
                )
                .expect("manual buy message"),
            ),
            &[&payer],
        )
        .expect("manual buy tx");

        let manual_rpc_buy_tx = Transaction::new_signed_with_payer(
            &expected_instructions,
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );

        assert_eq!(request.rpc_buy_tx, manual_rpc_buy_tx);
        assert_eq!(request.buy_tx, manual_buy_tx);
        assert!(request.tip_tx.is_none());
    }

    #[test]
    fn test_build_prepared_buy_request_uses_idempotent_ata_create_when_missing() {
        let trigger = TriggerComponent::new(create_test_config());
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let recent_blockhash = Hash::new_unique();
        let token_program = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022");
        let overrides = valid_buy_account_overrides();
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");

        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                true,
                &overrides,
                amount_lamports,
                0,
                recent_blockhash,
            )
            .expect("prepared buy request should build");

        let buy_ix = DirectBuyBuilder::build_buy_ix_with_accounts(
            &payer.pubkey(),
            &mint,
            &token_program,
            None,
            None,
            overrides.creator_pubkey,
            None,
            None,
            request.amount_lamports,
            request.min_tokens_out,
        );
        let ata_ix = create_associated_token_account_idempotent(
            &payer.pubkey(),
            &payer.pubkey(),
            &mint,
            &token_program,
        );
        let expected_instructions = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(BUY_COMPUTE_UNIT_LIMIT),
            ComputeBudgetInstruction::set_compute_unit_price(
                HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
            ),
            ata_ix.clone(),
            buy_ix.clone(),
        ];
        let manual_rpc_buy_tx = Transaction::new_signed_with_payer(
            &expected_instructions,
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );
        let manual_buy_tx = VersionedTransaction::try_new(
            VersionedMessage::V0(
                v0::Message::try_compile(
                    &payer.pubkey(),
                    &expected_instructions,
                    &[],
                    recent_blockhash,
                )
                .expect("manual buy message"),
            ),
            &[&payer],
        )
        .expect("manual buy tx");

        assert_eq!(request.rpc_buy_tx, manual_rpc_buy_tx);
        assert_eq!(request.buy_tx, manual_buy_tx);
    }

    #[tokio::test]
    async fn retry_rebuild_reuses_same_build_profile_contract() {
        let payer = Keypair::new();
        let temp = tempfile::tempdir().expect("tempdir");
        let keypair_path = temp.path().join("payer.json");
        solana_sdk::signature::write_keypair_file(&payer, &keypair_path)
            .expect("write payer keypair");
        let (rpc_url, _latest_blockhash_count, _block_height_count) =
            spawn_blockhash_cache_rpc_server().await;
        let mut config = create_test_config_with_rpc_url(rpc_url);
        config.keypair_path = Some(keypair_path.to_string_lossy().to_string());
        let trigger = TriggerComponent::new(config);
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let overrides = valid_buy_account_overrides();
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");

        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("prepared request");
        let rebuilt = trigger
            .rebuild_prepared_buy_request_for_retry(&request)
            .await
            .expect("retry rebuild");

        let initial_profile = request
            .build_profile
            .as_ref()
            .expect("initial build profile");
        let rebuilt_profile = rebuilt
            .build_profile
            .as_ref()
            .expect("rebuilt build profile");

        assert_eq!(initial_profile.mint, rebuilt_profile.mint);
        assert_eq!(initial_profile.payer_pubkey, rebuilt_profile.payer_pubkey);
        assert_eq!(initial_profile.user_ata, rebuilt_profile.user_ata);
        assert_eq!(initial_profile.token_program, rebuilt_profile.token_program);
        assert_eq!(
            initial_profile.attach_idempotent_ata_create,
            rebuilt_profile.attach_idempotent_ata_create
        );
        assert_eq!(
            initial_profile.ata_missing_pre_submit,
            rebuilt_profile.ata_missing_pre_submit
        );
        assert_eq!(
            initial_profile.amount_lamports,
            rebuilt_profile.amount_lamports
        );
        assert_eq!(
            initial_profile.pre_submit_token_balance,
            rebuilt_profile.pre_submit_token_balance
        );
        assert_eq!(initial_profile.buy_variant, rebuilt_profile.buy_variant);
        assert_eq!(
            initial_profile.min_tokens_out,
            rebuilt_profile.min_tokens_out
        );
        assert_eq!(
            initial_profile.token_param_role,
            rebuilt_profile.token_param_role
        );
        assert_eq!(
            initial_profile.ata_instruction,
            rebuilt_profile.ata_instruction
        );
        assert_eq!(
            initial_profile.buy_instruction,
            rebuilt_profile.buy_instruction
        );

        assert_eq!(
            request.rpc_buy_tx.message.instructions[0],
            rebuilt.rpc_buy_tx.message.instructions[0]
        );
        assert_eq!(
            request.rpc_buy_tx.message.instructions.len(),
            rebuilt.rpc_buy_tx.message.instructions.len()
        );
        assert_ne!(request.recent_blockhash, rebuilt.recent_blockhash);
        assert_ne!(request.tip_lamports, rebuilt.tip_lamports);
        assert_ne!(
            request.priority_fee_micro_lamports,
            rebuilt.priority_fee_micro_lamports
        );
        assert_ne!(request.buy_tx.signatures, rebuilt.buy_tx.signatures);
    }

    #[test]
    fn test_build_prepared_buy_request_rejects_missing_creator_pubkey() {
        let trigger = TriggerComponent::new(create_test_config());
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let recent_blockhash = Hash::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");

        let err = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &BuyAccountOverrides::default(),
                amount_lamports,
                0,
                recent_blockhash,
            )
            .expect_err("missing creator must fail before transaction build");

        assert!(
            err.to_string().contains("Missing canonical creator_pubkey"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_resolve_safe_trade_budget_clamps_amount_to_bulkhead_capacity() {
        let trigger = TriggerComponent::new(create_test_config());

        let (amount_lamports, effective_tip_lamports) = trigger
            .resolve_safe_trade_budget(80_000_000, 3_000_000)
            .expect("safe trade budget should resolve");

        assert_eq!(amount_lamports, 10_000_000);
        assert!(effective_tip_lamports <= 3_000_000);
    }

    #[test]
    fn test_resolve_safe_trade_budget_preserves_sender_buy_tip_policy() {
        let trigger =
            TriggerComponent::new_with_position_limit_tracker_and_runtime_state_and_sender(
                create_test_config(),
                PositionLimitTracker::new(3),
                Arc::new(ShadowLedger::new()),
                Arc::new(AccountStateReducer::new()),
                Some(test_live_tx_sender()),
            );

        let (amount_lamports, effective_tip_lamports) = trigger
            .resolve_safe_trade_budget(80_000_000, HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS)
            .expect("sender buy budget should resolve");

        assert_eq!(amount_lamports, 10_000_000);
        assert_eq!(
            effective_tip_lamports,
            HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS
        );
    }

    #[tokio::test]
    async fn live_buy_tip_resolution_refuses_legacy_fallback_without_sender() {
        let trigger = TriggerComponent::new(create_test_config());

        let tip_lamports = trigger.resolve_live_buy_tip_lamports(0.0001, 1.0).await;

        assert_eq!(tip_lamports, 0);
    }

    #[tokio::test]
    async fn test_execute_buy_dry_run_mock_returns_mock_signature() {
        let mut config = create_test_config();
        config.entry_mode = crate::config::TriggerEntryMode::DryRunMock;

        let trigger = TriggerComponent::new(config);
        let outcome = trigger
            .execute_buy(
                &Pubkey::new_unique(),
                &BuyAccountOverrides::default(),
                1_000_000,
            )
            .await
            .expect("dry-run mock should succeed");

        match outcome {
            TriggerBuyOutcome::DryRunMock { signature } => {
                assert_ne!(signature, Signature::default());
            }
            other => panic!("expected DryRunMock outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn live_dispatch_fails_closed_without_sender_transport() {
        let trigger = TriggerComponent::new(create_test_config());
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let overrides = valid_buy_account_overrides();
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("prepared request");

        let receipt = trigger.dispatch_prepared_buy_with_shadow(request).await;
        let err = receipt
            .primary_outcome
            .expect_err("live dispatch must fail without Sender transport");

        assert!(err
            .to_string()
            .contains("Helius Sender + Yellowstone transport"));
        assert!(receipt.shadow_task.is_none());
        assert!(receipt.active_position_lease.is_none());
        assert_eq!(trigger.active_positions(), 0);
    }

    #[test]
    fn live_transport_guard_rejects_missing_sender() {
        let trigger = TriggerComponent::new(create_test_config());
        let err = trigger
            .ensure_live_sender_transport()
            .expect_err("missing sender must fail closed");

        assert!(err
            .to_string()
            .contains("Helius Sender + Yellowstone transport"));
    }

    #[test]
    fn live_transport_guard_accepts_initialized_sender() {
        let trigger = TriggerComponent::new_with_runtime_guards_and_runtime_state_and_sender(
            create_test_config(),
            Arc::new(MockShadowSimulator),
            PositionLimitTracker::new(1),
            Arc::new(ShadowLedger::new()),
            Arc::new(AccountStateReducer::new()),
            Some(test_live_tx_sender()),
        );
        trigger
            .ensure_live_sender_transport()
            .expect("initialized sender should allow live transport");
    }

    #[tokio::test]
    async fn sender_buy_resource_exhausted_falls_back_to_signature_status() {
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token-2022");
        let user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token_program);
        let rpc_url =
            spawn_sender_confirmation_rpc_server(user_ata, vec![None], vec![Some(777)]).await;
        let account_state_core = Arc::new(AccountStateReducer::new());
        seed_canonical_buy_state(&account_state_core, mint);
        let trigger =
            TriggerComponent::new_with_position_limit_tracker_and_runtime_state_and_sender(
                create_test_config_with_rpc_url(rpc_url.clone()),
                PositionLimitTracker::new(1),
                Arc::new(ShadowLedger::new()),
                Arc::clone(&account_state_core),
                Some(test_live_tx_sender_with_confirmation(
                    rpc_url,
                    "test://yellowstone-resource-exhausted",
                )),
            );
        let request =
            build_test_prepared_buy_request(&trigger, &payer, &mint, &token_program, false, None);
        let expected_signature = request.buy_tx.signatures[0];

        let confirmed = trigger
            .submit_prepared_via_sender(request)
            .await
            .expect("resource exhausted must fall back to signature status");

        assert_eq!(confirmed.signature, expected_signature);
        assert_eq!(confirmed.landed_slot, Some(777));
    }

    #[tokio::test]
    async fn sender_buy_resource_exhausted_falls_back_to_balance_delta() {
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token-2022");
        let user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token_program);
        let rpc_url = spawn_sender_confirmation_rpc_server(
            user_ata,
            vec![Some(0), Some(2_500)],
            vec![None, None, None],
        )
        .await;
        let account_state_core = Arc::new(AccountStateReducer::new());
        seed_canonical_buy_state(&account_state_core, mint);
        let trigger =
            TriggerComponent::new_with_position_limit_tracker_and_runtime_state_and_sender(
                create_test_config_with_rpc_url(rpc_url.clone()),
                PositionLimitTracker::new(1),
                Arc::new(ShadowLedger::new()),
                Arc::clone(&account_state_core),
                Some(test_live_tx_sender_with_confirmation(
                    rpc_url,
                    "test://yellowstone-resource-exhausted",
                )),
            );
        let request =
            build_test_prepared_buy_request(&trigger, &payer, &mint, &token_program, true, Some(0));
        let expected_signature = request.buy_tx.signatures[0];

        let confirmed = trigger
            .submit_prepared_via_sender(request)
            .await
            .expect("resource exhausted must fall back to ATA balance delta");

        assert_eq!(confirmed.signature, expected_signature);
        assert_eq!(confirmed.landed_slot, None);
    }

    #[tokio::test]
    async fn resolve_live_blockhash_uses_cache_until_stale_then_refreshes() {
        let (rpc_url, latest_blockhash_count, block_height_count) =
            spawn_blockhash_cache_rpc_server().await;
        let trigger = TriggerComponent::new(create_test_config_with_rpc_url(rpc_url));

        let (first_snapshot, first_source) = trigger
            .resolve_live_blockhash(trigger.preparation_rpc())
            .await
            .expect("first blockhash fetch");
        let (second_snapshot, second_source) = trigger
            .resolve_live_blockhash(trigger.preparation_rpc())
            .await
            .expect("second blockhash fetch");

        assert_eq!(first_source, "rpc_refresh");
        assert_eq!(second_source, "cache");
        assert_eq!(first_snapshot.blockhash, second_snapshot.blockhash);
        assert_eq!(latest_blockhash_count.load(Ordering::Relaxed), 1);
        assert_eq!(block_height_count.load(Ordering::Relaxed), 1);

        tokio::time::sleep(Duration::from_millis(BUY_BLOCKHASH_CACHE_MAX_AGE_MS + 50)).await;

        let (third_snapshot, third_source) = trigger
            .resolve_live_blockhash(trigger.preparation_rpc())
            .await
            .expect("third blockhash fetch");

        assert_eq!(third_source, "rpc_refresh");
        assert_ne!(third_snapshot.blockhash, first_snapshot.blockhash);
        assert!(latest_blockhash_count.load(Ordering::Relaxed) >= 2);
        assert!(block_height_count.load(Ordering::Relaxed) >= 2);
    }

    #[tokio::test]
    async fn sender_buy_retries_same_tx_before_rebuild() {
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token-2022");
        let user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token_program);
        let temp = tempfile::tempdir().expect("tempdir");
        let keypair_path = temp.path().join("payer.json");
        solana_sdk::signature::write_keypair_file(&payer, &keypair_path)
            .expect("write payer keypair");
        let (sender_endpoint, send_count, seen_signatures) = spawn_sender_submission_server().await;
        let rpc_url = spawn_sender_retry_rpc_server(
            user_ata,
            Arc::clone(&send_count),
            Arc::clone(&seen_signatures),
        )
        .await;
        let account_state_core = Arc::new(AccountStateReducer::new());
        seed_canonical_buy_state(&account_state_core, mint);
        let mut config = create_test_config_with_rpc_url(rpc_url.clone());
        config.keypair_path = Some(keypair_path.to_string_lossy().to_string());
        let trigger =
            TriggerComponent::new_with_position_limit_tracker_and_runtime_state_and_sender(
                config,
                PositionLimitTracker::new(1),
                Arc::new(ShadowLedger::new()),
                Arc::clone(&account_state_core),
                Some(test_live_tx_sender_with_endpoints(
                    sender_endpoint,
                    rpc_url,
                    "test://yellowstone-resource-exhausted",
                )),
            );
        std::fs::remove_file(&keypair_path).expect("remove payer keypair after startup cache");
        let request =
            build_test_prepared_buy_request(&trigger, &payer, &mint, &token_program, true, Some(0));
        let first_signature = request.buy_tx.signatures[0];

        let confirmed = trigger
            .submit_prepared_via_sender(request)
            .await
            .expect("retry path should confirm after rebuild");

        let seen = seen_signatures
            .lock()
            .expect("seen signatures lock")
            .clone();
        assert_eq!(send_count.load(Ordering::Relaxed), 3);
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0], first_signature);
        assert_eq!(seen[1], first_signature);
        assert_ne!(seen[2], first_signature);
        assert_eq!(confirmed.signature, seen[2]);
        assert_eq!(confirmed.landed_slot, Some(999));
    }

    #[tokio::test]
    async fn prepare_buy_request_uses_cached_payer_after_keypair_file_deleted() {
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token_program);
        let rpc_url =
            spawn_prepare_buy_rpc_server(payer.pubkey(), mint, user_ata, token_program).await;
        let temp = tempfile::tempdir().expect("tempdir");
        let keypair_path = temp.path().join("payer.json");
        solana_sdk::signature::write_keypair_file(&payer, &keypair_path)
            .expect("write payer keypair");

        let mut config = create_test_config_with_rpc_url(rpc_url);
        config.keypair_path = Some(keypair_path.to_string_lossy().to_string());
        let trigger = TriggerComponent::new(config);

        std::fs::remove_file(&keypair_path).expect("remove payer keypair after startup cache");

        let request = trigger
            .prepare_buy_request(&mint, &valid_buy_account_overrides(), 1_000_000)
            .await
            .expect("prepare_buy_request should use cached payer");

        assert_eq!(request.payer_pubkey, payer.pubkey());
        assert_eq!(request.user_ata, user_ata);
        assert!(request.attach_idempotent_ata_create);
        assert!(request.ata_missing_pre_submit);
    }

    #[tokio::test]
    async fn prepare_buy_request_preserves_real_pre_submit_balance_when_ata_exists() {
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token_program);
        let rpc_url = spawn_prepare_buy_ata_probe_rpc_server(
            payer.pubkey(),
            mint,
            user_ata,
            token_program,
            PrepareBuyAtaProbeRpcScenario {
                user_ata_exists: true,
                token_balance: TokenBalanceRpcResponse::Amount(7_777),
                rent_lamports: 2_074_080,
            },
        )
        .await;
        let temp = tempfile::tempdir().expect("tempdir");
        let keypair_path = temp.path().join("payer.json");
        solana_sdk::signature::write_keypair_file(&payer, &keypair_path)
            .expect("write payer keypair");

        let mut config = create_test_config_with_rpc_url(rpc_url);
        config.keypair_path = Some(keypair_path.to_string_lossy().to_string());
        let trigger = TriggerComponent::new(config);

        let request = trigger
            .prepare_buy_request(&mint, &valid_buy_account_overrides(), 1_000_000)
            .await
            .expect("prepare_buy_request should preserve pre-submit ATA balance");

        assert!(request.attach_idempotent_ata_create);
        assert!(!request.ata_missing_pre_submit);
        assert_eq!(request.pre_submit_token_balance, Some(7_777));
        assert!(request.rpc_buy_tx.message.instructions.iter().any(|ix| {
            request.rpc_buy_tx.message.account_keys[ix.program_id_index as usize]
                == spl_associated_token_account::id()
        }));
    }

    #[tokio::test]
    async fn probe_user_ata_pre_submit_preserves_non_missing_on_primary_secondary_disagreement() {
        let mint = Pubkey::new_unique();
        let payer = Keypair::new();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token_program);
        let primary_rpc_url = spawn_prepare_buy_ata_probe_rpc_server(
            payer.pubkey(),
            mint,
            user_ata,
            token_program,
            PrepareBuyAtaProbeRpcScenario {
                user_ata_exists: false,
                token_balance: TokenBalanceRpcResponse::Missing,
                rent_lamports: 2_074_080,
            },
        )
        .await;
        let secondary_rpc_url = spawn_prepare_buy_ata_probe_rpc_server(
            payer.pubkey(),
            mint,
            user_ata,
            token_program,
            PrepareBuyAtaProbeRpcScenario {
                user_ata_exists: true,
                token_balance: TokenBalanceRpcResponse::Amount(15),
                rent_lamports: 2_074_080,
            },
        )
        .await;
        let mut config = create_test_config_with_rpc_url(primary_rpc_url);
        config.entry_mode = crate::config::TriggerEntryMode::LiveAndShadow;
        config.shadow_run.enabled = true;
        config.shadow_run.shadow_rpc_url = secondary_rpc_url;
        let trigger = TriggerComponent::new(config);

        let probe = trigger
            .probe_user_ata_pre_submit(&mint, user_ata, &token_program, trigger.preparation_rpc())
            .await
            .expect("disagreement should stay conservative");

        assert!(!probe.ata_missing_pre_submit);
        assert_eq!(probe.pre_submit_token_balance, None);
        assert_eq!(probe.expected_ata_rent_lamports, 0);
    }

    #[tokio::test]
    async fn probe_user_ata_pre_submit_does_not_guess_missing_on_token_balance_rpc_error() {
        let mint = Pubkey::new_unique();
        let payer = Keypair::new();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token_program);
        let rpc_url = spawn_prepare_buy_ata_probe_rpc_server(
            payer.pubkey(),
            mint,
            user_ata,
            token_program,
            PrepareBuyAtaProbeRpcScenario {
                user_ata_exists: true,
                token_balance: TokenBalanceRpcResponse::RpcError,
                rent_lamports: 2_074_080,
            },
        )
        .await;
        let trigger = TriggerComponent::new(create_test_config_with_rpc_url(rpc_url));

        let probe = trigger
            .probe_user_ata_pre_submit(&mint, user_ata, &token_program, trigger.preparation_rpc())
            .await
            .expect("rpc error should fall back to conservative legacy semantics");

        assert!(!probe.ata_missing_pre_submit);
        assert_eq!(probe.pre_submit_token_balance, None);
        assert_eq!(probe.expected_ata_rent_lamports, 0);
    }

    #[test]
    #[should_panic(expected = "TriggerComponent startup failed")]
    fn trigger_constructor_fails_fast_for_invalid_configured_keypair() {
        let mut config = create_test_config();
        config.keypair_path = Some("/definitely/missing/trigger-payer.json".to_string());

        let _ = TriggerComponent::new(config);
    }

    #[test]
    fn shadow_only_ephemeral_payer_does_not_require_live_keypair() {
        let mut config = create_test_config();
        config.entry_mode = TriggerEntryMode::ShadowOnly;
        config.keypair_path = Some("/definitely/missing/trigger-payer.json".to_string());
        config.shadow_run.enabled = true;
        config.shadow_run.payer_strategy = TriggerShadowPayerStrategy::Ephemeral;

        let trigger = TriggerComponent::new(config);
        let payer = trigger
            .load_payer()
            .expect("shadow_only ephemeral payer should be initialized");

        assert_eq!(payer.provenance, "ephemeral");
    }

    #[tokio::test]
    async fn minimum_user_ata_rent_lamports_uses_cache_for_same_token_program() {
        let (rpc_url, rent_request_count) = spawn_ata_rent_rpc_server(vec![2_074_080]).await;
        let trigger = TriggerComponent::new(create_test_config_with_rpc_url(rpc_url.clone()));
        let rpc = seer::new_async_rpc_client(rpc_url);
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");

        let first_rent = trigger
            .minimum_user_ata_rent_lamports(&rpc, &token_program)
            .await
            .expect("first rent fetch");
        let second_rent = trigger
            .minimum_user_ata_rent_lamports(&rpc, &token_program)
            .await
            .expect("second rent fetch");

        assert_eq!(first_rent, 2_074_080);
        assert_eq!(second_rent, 2_074_080);
        assert_eq!(rent_request_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn minimum_user_ata_rent_lamports_keeps_separate_cache_per_token_program() {
        let (rpc_url, rent_request_count) =
            spawn_ata_rent_rpc_server(vec![2_074_080, 2_300_000]).await;
        let trigger = TriggerComponent::new(create_test_config_with_rpc_url(rpc_url.clone()));
        let rpc = seer::new_async_rpc_client(rpc_url);
        let legacy_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let token_2022_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token-2022 program");

        let legacy_rent = trigger
            .minimum_user_ata_rent_lamports(&rpc, &legacy_program)
            .await
            .expect("legacy rent fetch");
        let token_2022_rent = trigger
            .minimum_user_ata_rent_lamports(&rpc, &token_2022_program)
            .await
            .expect("token-2022 rent fetch");

        assert_eq!(legacy_rent, 2_074_080);
        assert_eq!(token_2022_rent, 2_300_000);
        assert_eq!(rent_request_count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn prepare_buy_request_parallelizes_independent_primary_rpc_fetches() {
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let legacy_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let token_2022_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token-2022 program");
        let legacy_user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &legacy_program);
        let token_2022_user_ata = get_associated_token_address_with_program_id(
            &payer.pubkey(),
            &mint,
            &token_2022_program,
        );
        let (rpc_url, metrics) = spawn_parallel_prepare_buy_rpc_server(
            payer.pubkey(),
            mint,
            legacy_user_ata,
            token_2022_user_ata,
            legacy_program,
            150,
            false,
            false,
        )
        .await;
        let temp = tempfile::tempdir().expect("tempdir");
        let keypair_path = temp.path().join("payer.json");
        solana_sdk::signature::write_keypair_file(&payer, &keypair_path)
            .expect("write payer keypair");
        let mut config = create_test_config_with_rpc_url(rpc_url);
        config.keypair_path = Some(keypair_path.to_string_lossy().to_string());
        let trigger = TriggerComponent::new(config);

        let request = trigger
            .prepare_buy_request(&mint, &valid_buy_account_overrides(), 1_000_000)
            .await
            .expect("prepare buy request");

        assert_eq!(request.payer_pubkey, payer.pubkey());
        assert!(metrics.max_in_flight.load(Ordering::SeqCst) >= 3);
        assert!(!request.preparation_telemetry.token_program_override_present);
        assert_eq!(
            request.preparation_telemetry.token_program_proof_result,
            "not_provided"
        );
        assert_eq!(
            request.preparation_telemetry.token_program_source,
            "canonical_mint_fetch"
        );
    }

    #[tokio::test]
    async fn prepare_buy_request_discards_speculative_ata_probe_on_token_program_mismatch() {
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let legacy_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let token_2022_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token-2022 program");
        let legacy_user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &legacy_program);
        let token_2022_user_ata = get_associated_token_address_with_program_id(
            &payer.pubkey(),
            &mint,
            &token_2022_program,
        );
        let (rpc_url, metrics) = spawn_parallel_prepare_buy_rpc_server(
            payer.pubkey(),
            mint,
            legacy_user_ata,
            token_2022_user_ata,
            token_2022_program,
            0,
            true,
            false,
        )
        .await;
        let temp = tempfile::tempdir().expect("tempdir");
        let keypair_path = temp.path().join("payer.json");
        solana_sdk::signature::write_keypair_file(&payer, &keypair_path)
            .expect("write payer keypair");
        let mut config = create_test_config_with_rpc_url(rpc_url);
        config.keypair_path = Some(keypair_path.to_string_lossy().to_string());
        let trigger = TriggerComponent::new(config);
        let overrides = BuyAccountOverrides {
            creator_pubkey: Some(Pubkey::new_unique()),
            token_program: Some(legacy_program),
            ..BuyAccountOverrides::default()
        };

        let request = trigger
            .prepare_buy_request(&mint, &overrides, 1_000_000)
            .await
            .expect("prepare buy request");

        assert_eq!(request.token_program, token_2022_program);
        assert_eq!(request.user_ata, token_2022_user_ata);
        assert!(request.attach_idempotent_ata_create);
        assert!(request.ata_missing_pre_submit);
        assert_eq!(request.pre_submit_token_balance, Some(0));
        assert!(request.preparation_telemetry.token_program_override_present);
        assert_eq!(
            request.preparation_telemetry.token_program_proof_result,
            "mismatched"
        );
        assert_eq!(
            request.preparation_telemetry.token_program_source,
            "canonical_mint_fetch_after_mismatch"
        );
        assert!(metrics.legacy_ata_requests.load(Ordering::Relaxed) >= 1);
        assert!(metrics.token_2022_ata_requests.load(Ordering::Relaxed) >= 1);
    }

    #[tokio::test]
    async fn prepare_buy_request_marks_token_program_override_as_matched_when_canonical() {
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let legacy_user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token_program);
        let token_2022_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token-2022 program");
        let token_2022_user_ata = get_associated_token_address_with_program_id(
            &payer.pubkey(),
            &mint,
            &token_2022_program,
        );
        let (rpc_url, _metrics) = spawn_parallel_prepare_buy_rpc_server(
            payer.pubkey(),
            mint,
            legacy_user_ata,
            token_2022_user_ata,
            token_program,
            0,
            false,
            false,
        )
        .await;
        let temp = tempfile::tempdir().expect("tempdir");
        let keypair_path = temp.path().join("payer.json");
        solana_sdk::signature::write_keypair_file(&payer, &keypair_path)
            .expect("write payer keypair");
        let mut config = create_test_config_with_rpc_url(rpc_url);
        config.keypair_path = Some(keypair_path.to_string_lossy().to_string());
        let trigger = TriggerComponent::new(config);
        let overrides = BuyAccountOverrides {
            creator_pubkey: Some(Pubkey::new_unique()),
            token_program: Some(token_program),
            ..BuyAccountOverrides::default()
        };

        let request = trigger
            .prepare_buy_request(&mint, &overrides, 1_000_000)
            .await
            .expect("prepare buy request");

        assert_eq!(request.token_program, token_program);
        assert!(request.preparation_telemetry.token_program_override_present);
        assert_eq!(
            request.preparation_telemetry.token_program_proof_result,
            "matched"
        );
        assert_eq!(
            request.preparation_telemetry.token_program_source,
            "runtime_override_validated"
        );
    }

    #[tokio::test]
    async fn advisory_tip_floor_prewarm_becomes_fixed_tip_noop() {
        let response_body =
            "[{\"landed_tips_75th_percentile\":0.0003,\"landed_tips_50th_percentile\":null,\"landed_tips_25th_percentile\":null}]";
        let (tip_floor_endpoint, request_count) =
            spawn_tip_floor_server(vec![(200, response_body)]).await;
        let sender = Arc::new(
            LiveTxSender::new(
                crate::components::live_tx_sender::LiveTxSenderConfig::new(
                    "test://sender-success",
                    "http://127.0.0.1:1",
                    "http://127.0.0.1:1",
                    "test-yellowstone-token",
                )
                .with_tip_floor_endpoint(tip_floor_endpoint),
            )
            .expect("test live tx sender"),
        );
        let trigger = Arc::new(
            TriggerComponent::new_with_position_limit_tracker_and_runtime_state_and_sender(
                create_test_config(),
                PositionLimitTracker::new(3),
                Arc::new(ShadowLedger::new()),
                Arc::new(AccountStateReducer::new()),
                Some(sender.clone()),
            ),
        );

        assert!(
            trigger
                .spawn_prewarm_advisory(TriggerPrewarmAdvisory::TipFloor)
                .await
        );

        tokio::time::sleep(Duration::from_millis(20)).await;

        let resolved = sender.resolve_buy_tip_lamports_with_telemetry().await;

        assert_eq!(sender.cached_tip_floor_lamports(), None);
        assert_eq!(
            resolved.tip_lamports,
            HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS
        );
        assert_eq!(resolved.telemetry.source, "sender_fixed_tip");
        assert_eq!(resolved.telemetry.cache_mode, "fixed_baseline");
        assert_eq!(resolved.telemetry.fetch_latency_ms, 0);
        assert_eq!(request_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn resolve_live_buy_tip_uses_fixed_sender_tip_without_tip_floor_fetch() {
        let response_body =
            "[{\"landed_tips_75th_percentile\":0.0003,\"landed_tips_50th_percentile\":null,\"landed_tips_25th_percentile\":null}]";
        let (tip_floor_endpoint, request_count) =
            spawn_tip_floor_server_with_delay(vec![(200, response_body, 120)]).await;
        let sender = Arc::new(
            LiveTxSender::new(
                crate::components::live_tx_sender::LiveTxSenderConfig::new(
                    "test://sender-success",
                    "http://127.0.0.1:1",
                    "http://127.0.0.1:1",
                    "test-yellowstone-token",
                )
                .with_tip_floor_endpoint(tip_floor_endpoint),
            )
            .expect("test live tx sender"),
        );
        let trigger = Arc::new(
            TriggerComponent::new_with_position_limit_tracker_and_runtime_state_and_sender(
                create_test_config(),
                PositionLimitTracker::new(3),
                Arc::new(ShadowLedger::new()),
                Arc::new(AccountStateReducer::new()),
                Some(sender),
            ),
        );

        let resolved = trigger.resolve_live_buy_tip(0.0001, 1.0).await;

        assert_eq!(
            resolved.tip_lamports,
            HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS
        );
        assert_eq!(resolved.telemetry.source, "sender_fixed_tip");
        assert_eq!(resolved.telemetry.cache_mode, "fixed_baseline");
        assert_eq!(resolved.telemetry.fetch_latency_ms, 0);
        assert_eq!(resolved.telemetry.inflight_join_result, "disabled");
        assert_eq!(request_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn advisory_priority_fee_prewarm_populates_sender_cache_for_probable_buy_class() {
        let response_body =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":42000},\"id\":\"ghost-live-priority-fee\"}";
        let (priority_fee_rpc_url, request_count) =
            spawn_priority_fee_server(vec![(200, response_body)]).await;
        let payer = Keypair::new();
        let temp = tempfile::tempdir().expect("tempdir");
        let keypair_path = temp.path().join("payer.json");
        solana_sdk::signature::write_keypair_file(&payer, &keypair_path)
            .expect("write payer keypair");

        let sender = Arc::new(
            LiveTxSender::new(crate::components::live_tx_sender::LiveTxSenderConfig::new(
                "test://sender-success",
                priority_fee_rpc_url,
                "http://127.0.0.1:1",
                "test-yellowstone-token",
            ))
            .expect("test live tx sender"),
        );
        let mut config = create_test_config();
        config.keypair_path = Some(keypair_path.to_string_lossy().to_string());
        let trigger = Arc::new(
            TriggerComponent::new_with_position_limit_tracker_and_runtime_state_and_sender(
                config,
                PositionLimitTracker::new(3),
                Arc::new(ShadowLedger::new()),
                Arc::new(AccountStateReducer::new()),
                Some(sender.clone()),
            ),
        );
        trigger.store_cached_live_blockhash(CachedLiveBlockhash {
            blockhash: Hash::new_unique(),
            fetched_at: Instant::now(),
            last_valid_block_height: 10_000,
            observed_block_height: 1_000,
        });
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let account_overrides = BuyAccountOverrides {
            creator_pubkey: Some(Pubkey::new_unique()),
            token_program: Some(token_program),
            ..BuyAccountOverrides::default()
        };
        let cache_key = PriorityFeeCacheKey::buy(
            trigger::PumpfunBuyVariant::RoutedExactSolIn.as_str(),
            token_program,
            false,
            true,
        );

        assert!(
            trigger
                .spawn_prewarm_advisory(TriggerPrewarmAdvisory::BuyPriorityFee {
                    mint,
                    account_overrides,
                    tip_lamports: HELIUS_SENDER_BUY_BASELINE_TIP_LAMPORTS,
                })
                .await
        );

        let cached_estimate = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(cached_estimate) = sender.get_cached_buy_priority_fee(&cache_key) {
                    break cached_estimate;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("priority fee prewarm should populate sender cache");

        assert_eq!(cached_estimate.micro_lamports, 42_000);
        assert_eq!(cached_estimate.telemetry.source, "priority_fee_cache");
        assert!(cached_estimate.telemetry.cache_hit);
        assert_eq!(request_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn prepare_buy_request_joins_exact_priority_fee_prewarm_for_actual_ata_class() {
        let response_body =
            "{\"jsonrpc\":\"2.0\",\"result\":{\"priorityFeeEstimate\":42000},\"id\":\"ghost-live-priority-fee\"}";
        let (priority_fee_rpc_url, request_count) =
            spawn_priority_fee_server_with_delay(vec![(200, response_body, 80)]).await;
        let sender = Arc::new(
            LiveTxSender::new(crate::components::live_tx_sender::LiveTxSenderConfig::new(
                "test://sender-success",
                priority_fee_rpc_url,
                "http://127.0.0.1:1",
                "test-yellowstone-token",
            ))
            .expect("test live tx sender"),
        );
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let token_2022_program =
            Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token-2022 program");
        let legacy_user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token_program);
        let token_2022_user_ata = get_associated_token_address_with_program_id(
            &payer.pubkey(),
            &mint,
            &token_2022_program,
        );
        let (rpc_url, _metrics) = spawn_parallel_prepare_buy_rpc_server(
            payer.pubkey(),
            mint,
            legacy_user_ata,
            token_2022_user_ata,
            token_program,
            0,
            false,
            false,
        )
        .await;
        let temp = tempfile::tempdir().expect("tempdir");
        let keypair_path = temp.path().join("payer.json");
        solana_sdk::signature::write_keypair_file(&payer, &keypair_path)
            .expect("write payer keypair");
        let mut config = create_test_config_with_rpc_url(rpc_url);
        config.keypair_path = Some(keypair_path.to_string_lossy().to_string());
        let trigger =
            TriggerComponent::new_with_position_limit_tracker_and_runtime_state_and_sender(
                config,
                PositionLimitTracker::new(3),
                Arc::new(ShadowLedger::new()),
                Arc::new(AccountStateReducer::new()),
                Some(sender),
            );
        let overrides = BuyAccountOverrides {
            creator_pubkey: Some(Pubkey::new_unique()),
            token_program: Some(token_program),
            ..BuyAccountOverrides::default()
        };

        let request = trigger
            .prepare_buy_request(&mint, &overrides, 1_000_000)
            .await
            .expect("prepare buy request");

        assert!(request.ata_missing_pre_submit);
        assert_eq!(request.priority_fee_micro_lamports, 42_000);
        assert_eq!(
            request
                .preparation_telemetry
                .priority_fee_inflight_join_result,
            "joined"
        );
        assert!(request.preparation_telemetry.priority_fee_inflight_wait_ms > 0);
        assert_eq!(request_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn dry_run_dispatch_rejects_when_position_limit_is_reached() {
        let mut config = create_test_config();
        config.entry_mode = crate::config::TriggerEntryMode::DryRunMock;
        config.max_concurrent_positions = 1;

        let tracker = PositionLimitTracker::new(1);
        let trigger = TriggerComponent::new_with_runtime_guards(
            config,
            Arc::new(MockShadowSimulator),
            tracker,
        );
        let payer = Keypair::new();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let overrides = valid_buy_account_overrides();
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");

        let first_request = trigger
            .build_prepared_buy_request(
                &payer,
                &Pubkey::new_unique(),
                &token_program,
                false,
                &overrides,
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("first request");
        let first_receipt = trigger
            .dispatch_prepared_buy_with_shadow(first_request)
            .await;
        assert!(first_receipt.primary_outcome.is_ok());
        assert!(first_receipt.active_position_lease.is_some());

        let second_request = trigger
            .build_prepared_buy_request(
                &payer,
                &Pubkey::new_unique(),
                &token_program,
                false,
                &overrides,
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("second request");
        let second_receipt = trigger
            .dispatch_prepared_buy_with_shadow(second_request)
            .await;
        let err = second_receipt
            .primary_outcome
            .expect_err("second request must be rejected by position limit");
        assert!(matches!(
            err.downcast_ref::<SafetyViolation>(),
            Some(SafetyViolation::MaxConcurrentPositionsReached { .. })
        ));

        drop(first_receipt);
        assert_eq!(trigger.active_positions(), 0);
    }

    #[tokio::test]
    async fn shadow_report_contains_cu_logs_and_latency() {
        let mut config = create_test_config();
        config.entry_mode = crate::config::TriggerEntryMode::ShadowOnly;
        config.shadow_run.enabled = true;

        let trigger =
            TriggerComponent::new_with_shadow_simulator(config, Arc::new(MockShadowSimulator));
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let overrides = valid_buy_account_overrides();
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("prepared request");

        let outcome = trigger
            .dispatch_prepared_buy(request)
            .await
            .expect("shadow_only should return simulation report");

        match outcome {
            TriggerBuyOutcome::ShadowSimulated { report } => {
                assert_eq!(report.mint, mint.to_string());
                assert_eq!(report.units_consumed, Some(42_000));
                assert_eq!(report.logs, vec!["mock-shadow-log".to_string()]);
                assert_eq!(report.latency_ms, 5);
                assert_eq!(report.rpc_slot, 777);
            }
            other => panic!("expected ShadowSimulated outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_prepared_buy_with_shadow_shadow_only_reserves_position_slot() {
        let mut config = create_test_config();
        config.entry_mode = crate::config::TriggerEntryMode::ShadowOnly;
        config.shadow_run.enabled = true;
        config.max_concurrent_positions = 1;

        let trigger =
            TriggerComponent::new_with_shadow_simulator(config, Arc::new(MockShadowSimulator));
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let overrides = valid_buy_account_overrides();
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("prepared request");

        let receipt = trigger.dispatch_prepared_buy_with_shadow(request).await;
        assert!(receipt.primary_outcome.is_ok());
        assert!(receipt.active_position_lease.is_some());
        assert_eq!(trigger.active_positions(), 1);

        drop(receipt);
        assert_eq!(trigger.active_positions(), 0);
    }

    #[tokio::test]
    async fn dispatch_prepared_buy_shadow_only_runs_inline_shadow_without_live_side_effects() {
        let mut config = create_test_config();
        config.entry_mode = crate::config::TriggerEntryMode::LiveAndShadow;
        config.shadow_run.enabled = true;

        let trigger =
            TriggerComponent::new_with_shadow_simulator(config, Arc::new(MockShadowSimulator));
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let overrides = valid_buy_account_overrides();
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("prepared request");

        let receipt = trigger.dispatch_prepared_buy_shadow_only(request).await;
        let TriggerDispatchReceipt {
            primary_outcome,
            shadow_task,
            active_position_lease,
            retain_position_slot_on_error,
            failed_request,
            failed_context,
        } = receipt;
        assert!(shadow_task.is_none());
        assert!(active_position_lease.is_some());
        assert!(!retain_position_slot_on_error);
        assert!(failed_context.is_none());
        assert!(failed_request.is_some());

        match primary_outcome.expect("shadow-only helper should return a shadow report") {
            TriggerBuyOutcome::ShadowSimulated { report } => {
                assert_eq!(report.mint, mint.to_string());
                assert_eq!(report.units_consumed, Some(42_000));
            }
            other => panic!("expected ShadowSimulated outcome, got {other:?}"),
        }

        drop(active_position_lease);
        assert_eq!(trigger.active_positions(), 0);
    }

    #[tokio::test]
    async fn p37_counterfactual_probe_simulates_without_active_position_slot() {
        let mut config = create_test_config();
        config.entry_mode = crate::config::TriggerEntryMode::ShadowOnly;
        config.shadow_run.enabled = true;
        config.max_concurrent_positions = 1;

        let trigger =
            TriggerComponent::new_with_shadow_simulator(config, Arc::new(MockShadowSimulator));
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let overrides = valid_buy_account_overrides();
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("prepared request");

        let report = trigger
            .simulate_counterfactual_shadow_probe(&request)
            .await
            .expect("counterfactual probe simulation");

        assert_eq!(report.mint, mint.to_string());
        assert_eq!(report.units_consumed, Some(42_000));
        assert_eq!(trigger.active_positions(), 0);
    }

    #[tokio::test]
    async fn p37_counterfactual_probe_fixed_lamports_override_sets_request_amount() {
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let user_ata =
            get_associated_token_address_with_program_id(&payer.pubkey(), &mint, &token_program);
        let rpc_url =
            spawn_prepare_buy_rpc_server(payer.pubkey(), mint, user_ata, token_program).await;
        let temp = tempfile::tempdir().expect("tempdir");
        let keypair_path = temp.path().join("payer.json");
        solana_sdk::signature::write_keypair_file(&payer, &keypair_path)
            .expect("write payer keypair");

        let mut config = create_test_config_with_rpc_url(rpc_url);
        config.keypair_path = Some(keypair_path.to_string_lossy().to_string());
        config.max_position_size_sol = 0.02;
        let trigger =
            TriggerComponent::new_with_shadow_simulator(config, Arc::new(MockShadowSimulator));
        let fixed_lamports = 7_000_000;

        let request = trigger
            .prepare_buy_request_with_decision_ts_and_amount_lamports(
                &mint,
                &valid_buy_account_overrides(),
                1_000_000,
                Some(1_234),
                Some(fixed_lamports),
            )
            .await
            .expect("fixed-lamports probe request");

        assert_eq!(request.amount_lamports, fixed_lamports);
        assert_ne!(
            request.amount_lamports,
            trigger
                .configured_trade_amount_lamports()
                .expect("configured amount")
        );

        let report = trigger
            .simulate_counterfactual_shadow_probe(&request)
            .await
            .expect("counterfactual probe simulation");
        assert_eq!(report.amount_lamports, fixed_lamports);
        assert_eq!(trigger.active_positions(), 0);
    }

    #[test]
    fn p37_counterfactual_probe_required_accounts_skip_creatable_user_ata() {
        let config = create_test_config();
        let trigger =
            TriggerComponent::new_with_shadow_simulator(config, Arc::new(MockShadowSimulator));
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                true,
                &valid_buy_account_overrides(),
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("prepared request");

        let roles = TriggerComponent::counterfactual_probe_required_account_roles(&request);

        assert!(roles
            .iter()
            .any(|(pubkey, role)| *pubkey == request.payer_pubkey && role == "payer_pubkey"));
        assert!(roles
            .iter()
            .any(|(pubkey, role)| *pubkey == request.mint && role == "mint"));
        assert!(roles
            .iter()
            .any(|(pubkey, role)| *pubkey == request.token_program && role == "token_program"));
        assert!(!roles
            .iter()
            .any(|(pubkey, role)| *pubkey == request.user_ata && role == "user_ata"));
    }

    #[test]
    fn p37_counterfactual_probe_required_accounts_skip_ephemeral_payer() {
        let config = create_test_config();
        let trigger =
            TriggerComponent::new_with_shadow_simulator(config, Arc::new(MockShadowSimulator));
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let mut request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &valid_buy_account_overrides(),
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("prepared request");
        request.payer_provenance =
            TriggerComponent::payer_provenance_label(TriggerShadowPayerStrategy::Ephemeral);

        let roles = TriggerComponent::counterfactual_probe_required_account_roles(&request);

        assert!(!roles
            .iter()
            .any(|(pubkey, role)| *pubkey == request.payer_pubkey && role == "payer_pubkey"));
        assert!(roles
            .iter()
            .any(|(pubkey, role)| *pubkey == request.mint && role == "mint"));
        assert!(roles
            .iter()
            .any(|(pubkey, role)| *pubkey == request.token_program && role == "token_program"));
        assert!(roles
            .iter()
            .any(|(pubkey, role)| *pubkey == request.user_ata && role == "user_ata"));
    }

    #[test]
    fn p37_counterfactual_probe_required_accounts_skip_routed_user_volume_accumulator() {
        let config = create_test_config();
        let trigger =
            TriggerComponent::new_with_shadow_simulator(config, Arc::new(MockShadowSimulator));
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &valid_buy_account_overrides(),
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("prepared request");
        let user_volume_accumulator = request
            .build_profile
            .as_ref()
            .expect("build profile")
            .buy_instruction
            .accounts
            .get(13)
            .expect("routed user volume accumulator")
            .pubkey;

        assert_eq!(
            TriggerComponent::counterfactual_probe_account_role_for(
                &request,
                &user_volume_accumulator,
            ),
            "user_volume_accumulator"
        );

        let roles = TriggerComponent::counterfactual_probe_required_account_roles(&request);

        assert!(!roles
            .iter()
            .any(|(pubkey, role)| *pubkey == user_volume_accumulator
                && role == "user_volume_accumulator"));
        assert!(roles
            .iter()
            .any(|(pubkey, role)| *pubkey == request.mint && role == "mint"));
        assert!(roles.iter().any(|(_, role)| role == "bonding_curve"));
        assert!(roles.iter().any(|(pubkey, role)| {
            let fee_config_pubkey = request
                .build_profile
                .as_ref()
                .and_then(|profile| profile.buy_instruction.accounts.get(14))
                .map(|account| account.pubkey);
            fee_config_pubkey == Some(*pubkey) && role == "fee_config"
        }));
        assert!(roles.iter().any(|(pubkey, role)| {
            let bonding_curve_v2_pubkey = request
                .build_profile
                .as_ref()
                .and_then(|profile| profile.buy_instruction.accounts.get(16))
                .map(|account| account.pubkey);
            bonding_curve_v2_pubkey == Some(*pubkey) && role == "bonding_curve_v2"
        }));
    }

    #[test]
    fn p37_counterfactual_probe_required_accounts_include_existing_user_ata() {
        let config = create_test_config();
        let trigger =
            TriggerComponent::new_with_shadow_simulator(config, Arc::new(MockShadowSimulator));
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &valid_buy_account_overrides(),
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("prepared request");

        let roles = TriggerComponent::counterfactual_probe_required_account_roles(&request);

        assert!(roles
            .iter()
            .any(|(pubkey, role)| *pubkey == request.user_ata && role == "user_ata"));
    }

    #[test]
    fn p37_counterfactual_probe_required_accounts_include_simulation_loaded_bonding_curve_v2() {
        let config = create_test_config();
        let trigger =
            TriggerComponent::new_with_shadow_simulator(config, Arc::new(MockShadowSimulator));
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 1_000_000_000_000,
            real_sol_reserves: 30_000_000_000,
            token_total_supply: 0,
            complete: 0,
            _padding: [0; 7],
        };
        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &BuyAccountOverrides {
                    buy_variant: Some(trigger::PumpfunBuyVariant::LegacyBuy),
                    legacy_buy_curve: Some(curve),
                    associated_bonding_curve: Some(
                        DirectBuyBuilder::canonical_associated_bonding_curve(&mint, &token_program),
                    ),
                    ..BuyAccountOverrides::default()
                },
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("legacy probe request");
        let buy_accounts = &request
            .build_profile
            .as_ref()
            .expect("build profile")
            .buy_instruction
            .accounts;

        assert_eq!(buy_accounts.len(), 18);
        assert_eq!(
            TriggerComponent::counterfactual_probe_account_role_for(
                &request,
                &buy_accounts[9].pubkey,
            ),
            "creator_vault"
        );
        assert_eq!(
            TriggerComponent::counterfactual_probe_account_role_for(
                &request,
                &buy_accounts[16].pubkey,
            ),
            "bonding_curve_v2"
        );

        let roles = TriggerComponent::counterfactual_probe_required_account_roles(&request);

        assert!(roles.iter().any(|(_, role)| role == "bonding_curve"));
        assert!(roles.iter().any(|(_, role)| role == "creator_vault"));
        assert!(roles.iter().any(|(_, role)| role == "bonding_curve_v2"));
        assert!(roles
            .iter()
            .any(|(_, role)| role == "global_volume_accumulator"));
        assert!(roles.iter().any(|(_, role)| role == "fee_config"));
        assert!(!roles.iter().any(|(_, role)| role == "creator_pubkey"));
    }

    #[test]
    fn shadow_retry_happens_only_for_transient_error() {
        assert!(
            crate::components::trigger::shadow_run::RpcShadowSimulator::is_retryable(
                "blockhash not found"
            )
        );
        assert!(
            crate::components::trigger::shadow_run::RpcShadowSimulator::is_retryable(
                "transport connection reset by peer"
            )
        );
        assert!(
            !crate::components::trigger::shadow_run::RpcShadowSimulator::is_retryable(
                "custom program error: 0x1"
            )
        );
        assert!(
            !crate::components::trigger::shadow_run::RpcShadowSimulator::is_retryable(
                "insufficient funds"
            )
        );
    }

    #[tokio::test]
    async fn live_and_shadow_enforces_max_concurrent_shadow_jobs() {
        let mut config = create_test_config();
        config.entry_mode = crate::config::TriggerEntryMode::LiveAndShadow;
        config.shadow_run.enabled = true;
        config.shadow_run.max_concurrent = 2;

        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let simulator = ConcurrencyTrackingShadowSimulator {
            in_flight: Arc::clone(&in_flight),
            max_in_flight: Arc::clone(&max_in_flight),
            completed: Arc::clone(&completed),
            delay: Duration::from_millis(50),
        };
        let trigger = TriggerComponent::new_with_shadow_simulator(config, Arc::new(simulator));
        let payer = Keypair::new();

        let mut handles = Vec::new();
        for _ in 0..4 {
            let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
            let overrides = valid_buy_account_overrides();
            let amount_lamports = trigger
                .configured_trade_amount_lamports()
                .expect("configured amount");
            let request = trigger
                .build_prepared_buy_request(
                    &payer,
                    &Pubkey::new_unique(),
                    &token_program,
                    false,
                    &overrides,
                    amount_lamports,
                    1_000_000,
                    Hash::new_unique(),
                )
                .expect("prepared request");
            handles.push(trigger.spawn_shadow_simulation(request));
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            trigger.shadow_available_permits(),
            0,
            "exhausted shadow permits should expose saturation to telemetry"
        );

        for handle in handles {
            handle
                .handle
                .await
                .expect("join")
                .expect("shadow simulation");
        }

        assert_eq!(completed.load(Ordering::SeqCst), 4);
        assert!(
            max_in_flight.load(Ordering::SeqCst) <= 2,
            "shadow concurrency should respect trigger.shadow_run.max_concurrent"
        );
    }

    #[tokio::test]
    async fn shadow_only_writes_jsonl_even_when_emit_event_bus_false() {
        let temp = tempfile::tempdir().expect("tempdir");
        let output_path = temp.path().join("shadow").join("buys.jsonl");
        let event = crate::events::ShadowBuySimulationEvent {
            join_metadata: ExecutionJoinMetadata::default(),
            account_diagnostics: crate::events::ShadowSimulationAccountDiagnostics::default(),
            candidate_id: crate::events::build_execution_candidate_id("mint", "pool", "10"),
            pool_amm_id: "pool".to_string(),
            base_mint: "mint".to_string(),
            mint: "mint".to_string(),
            live_signature: None,
            payer_pubkey: Pubkey::new_unique().to_string(),
            payer_provenance: "configured".to_string(),
            amount_lamports: 100,
            entry_token_amount_raw: Some(250_000),
            tip_lamports: 10,
            decision_ts_ms: 10,
            simulation_started_ts_ms: 11,
            simulation_finished_ts_ms: 16,
            latency_ms: 5,
            shadow_duration_ms: 5,
            rpc_slot: 777,
            retry_count: 0,
            used_sig_verify: false,
            used_replace_recent_blockhash: true,
            units_consumed: Some(42_000),
            logs: vec!["shadow".to_string()],
            return_data: None,
            err: None,
            error_class: None,
            error_code: None,
            error_detail_class: None,
        };

        persist_shadow_event_record(
            output_path.to_str().expect("utf8 output path"),
            TriggerEntryMode::ShadowOnly,
            &event,
        )
        .await
        .expect("shadow jsonl should be written without event bus");

        let contents = tokio::fs::read_to_string(&output_path)
            .await
            .expect("read jsonl");
        assert!(contents.contains("\"entry_mode\":\"shadow_only\""));
        assert!(contents.contains("\"pool_amm_id\":\"pool\""));
    }

    #[tokio::test]
    async fn persist_shadow_failure_record_uses_failure_context() {
        let temp = tempfile::tempdir().expect("tempdir");
        let output_path = temp.path().join("shadow").join("failures.jsonl");
        let context = TriggerDispatchFailureContext {
            join_metadata: ExecutionJoinMetadata::default(),
            amount_lamports: 100,
            tip_lamports: 10,
            decision_ts_ms: 10,
            payer_provenance: "ephemeral",
            payer_pubkey: Some("payer-ephemeral".to_string()),
        };
        let err = anyhow::Error::new(ShadowPreparationError::new(
            "transport connection reset by peer",
            3,
        ));

        persist_shadow_failure_record(
            output_path.to_str().expect("utf8 output path"),
            TriggerEntryMode::ShadowOnly,
            "pool",
            "mint",
            None,
            Some(&context),
            &err,
        )
        .await
        .expect("shadow failure jsonl should be written");

        let contents = tokio::fs::read_to_string(&output_path)
            .await
            .expect("read jsonl");
        assert!(contents.contains("\"pool_amm_id\":\"pool\""));
        assert!(contents.contains("\"retry_count\":3"));
        assert!(contents.contains("\"error_class\":\"network_provider_problem\""));
    }

    #[tokio::test]
    async fn live_and_shadow_background_task_persists_without_event_bus() {
        let temp = tempfile::tempdir().expect("tempdir");
        let output_path = temp.path().join("shadow").join("background.jsonl");

        let mut config = create_test_config();
        config.entry_mode = crate::config::TriggerEntryMode::LiveAndShadow;
        config.shadow_run.enabled = true;
        config.shadow_run.emit_event_bus = false;

        let trigger =
            TriggerComponent::new_with_shadow_simulator(config, Arc::new(MockShadowSimulator));
        let payer = Keypair::new();
        let mint = Pubkey::new_unique();
        let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).expect("valid token program");
        let overrides = valid_buy_account_overrides();
        let amount_lamports = trigger
            .configured_trade_amount_lamports()
            .expect("configured amount");
        let request = trigger
            .build_prepared_buy_request(
                &payer,
                &mint,
                &token_program,
                false,
                &overrides,
                amount_lamports,
                1_000_000,
                Hash::new_unique(),
            )
            .expect("prepared request");
        let shadow_task = trigger.spawn_shadow_simulation(request);

        spawn_background_shadow_event(
            None,
            false,
            output_path.to_str().expect("utf8 output path").to_string(),
            "pool".to_string(),
            "mint".to_string(),
            None,
            shadow_task,
        );

        let contents = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Ok(contents) = tokio::fs::read_to_string(&output_path).await {
                    break contents;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("background shadow record should be persisted");

        assert!(contents.contains("\"entry_mode\":\"live_and_shadow\""));
        assert!(contents.contains("\"pool_amm_id\":\"pool\""));
        assert!(contents.contains("\"base_mint\":\"mint\""));
    }

    #[tokio::test]
    async fn test_trigger_with_oracle_pipeline() {
        let config = create_test_config();
        let oracle_config = OracleConfig::default();
        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let (event_tx, event_rx) = create_event_bus();

        // Clone event_tx before moving into spawn
        let event_tx_for_spawn = event_tx.clone();

        // Spawn trigger in background
        let handle = tokio::spawn(async move {
            run_with_oracle(
                config,
                oracle_config,
                Arc::new(ShadowLedger::new()),
                shutdown_rx,
                Some(event_rx),
                Some(event_tx_for_spawn),
            )
            .await
        });

        // Give trigger time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Send a test pool
        let pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "test_pool".to_string(),
            base_mint: "test_mint".to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            bonding_curve: "test_curve".to_string(),
            creator: "test_creator".to_string(),
            slot: Some(12345),
            timestamp_ms: 1700000000000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1700000000123),
            initial_liquidity_sol: Some(10.0),
            signature: "test_sig".to_string(),
        };

        event_tx.send(GhostEvent::new_pool_detected(pool)).unwrap();

        // Give time to process
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Shutdown
        shutdown_tx.send(()).unwrap();

        // Wait for trigger to stop
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }

    #[tokio::test]
    async fn test_trigger_oracle_disabled() {
        let config = create_test_config();
        let mut oracle_config = OracleConfig::default();
        oracle_config.enabled = false;

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);
        let (event_tx, event_rx) = create_event_bus();

        // Clone event_tx before moving into spawn
        let event_tx_for_spawn = event_tx.clone();

        let handle = tokio::spawn(async move {
            run_with_oracle(
                config,
                oracle_config,
                Arc::new(ShadowLedger::new()),
                shutdown_rx,
                Some(event_rx),
                Some(event_tx_for_spawn),
            )
            .await
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        let pool = DetectedPool {
            semantic: ghost_core::EventSemanticEnvelope::default(),
            pool_amm_id: "test_pool".to_string(),
            base_mint: "test_mint".to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            bonding_curve: "test_curve".to_string(),
            creator: "test_creator".to_string(),
            slot: Some(12345),
            timestamp_ms: 1700000000000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1700000000123),
            initial_liquidity_sol: Some(10.0),
            signature: "test_sig".to_string(),
        };

        event_tx.send(GhostEvent::new_pool_detected(pool)).unwrap();

        tokio::time::sleep(Duration::from_millis(200)).await;

        shutdown_tx.send(()).unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }

    #[test]
    fn test_validate_payer_balance_for_buy_rejects_underfunded_wallet() {
        let payer = Pubkey::new_unique();
        let err = TriggerComponent::validate_payer_balance_for_buy(
            &payer,
            2_039_280,
            100_000_000,
            3_000_000,
        )
        .expect_err("underfunded payer should be rejected");

        assert!(err
            .to_string()
            .contains("Insufficient payer balance for trigger buy"));
        assert!(err.to_string().contains(&payer.to_string()));
    }

    #[test]
    fn test_validate_payer_account_for_fee_rejects_non_system_owner() {
        let payer = Pubkey::new_unique();
        let payer_account = Account {
            lamports: 2_039_280,
            data: vec![0; 165],
            owner: Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                .expect("token program"),
            executable: false,
            rent_epoch: 0,
        };

        let err = TriggerComponent::validate_payer_account_for_fee(&payer, &payer_account)
            .expect_err("token account cannot pay transaction fees");
        assert!(err
            .to_string()
            .contains("Invalid fee payer account owner for trigger buy"));
    }

    #[test]
    fn test_resolve_buy_token_program_accepts_token2022() {
        let token_2022 = Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("valid token2022 program");
        assert_eq!(
            TriggerComponent::resolve_buy_token_program(&token_2022).unwrap(),
            token_2022
        );
    }

    #[test]
    fn test_is_account_not_found_error_matches_for_user_rpc_message() {
        let err: ClientError = RpcError::ForUser(
            "AccountNotFound: pubkey=3zV9GEKYDDHAVUfCiKWZNyrp5Z4TR4F4FqGUDj6jpump".to_string(),
        )
        .into();
        assert!(TriggerComponent::is_account_not_found_error(&err));
    }

    #[test]
    fn test_should_retry_account_fetch_considers_secondary_rpc() {
        assert!(TriggerComponent::should_retry_account_fetch(
            false,
            Some(true)
        ));
        assert!(TriggerComponent::should_retry_account_fetch(
            true,
            Some(false)
        ));
        assert!(!TriggerComponent::should_retry_account_fetch(
            false,
            Some(false)
        ));
        assert!(!TriggerComponent::should_retry_account_fetch(false, None));
    }

    #[test]
    fn test_account_fetch_retry_delay_prefers_not_found_backoff() {
        assert_eq!(
            TriggerComponent::account_fetch_retry_delay_ms(75, 150, true, None),
            150
        );
        assert_eq!(
            TriggerComponent::account_fetch_retry_delay_ms(75, 150, false, Some(true)),
            150
        );
        assert_eq!(
            TriggerComponent::account_fetch_retry_delay_ms(75, 150, false, Some(false)),
            75
        );
    }
}
