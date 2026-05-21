//! PostBuyRuntime — thin adapter from ghost-launcher handoff events to the canonical
//! post-buy runtime for each lane.
//!
//! Listens on the event bus for `PostBuySubmitted` events and routes them:
//!
//! - **lane == "live"** + `live_sell` configured: persists confirmed BUY entry metadata, monitors
//!   canonical price, and executes a single 100% SELL via Helius Sender when +2% or -2% is hit.
//! - **lane == "shadow"**: registers the position in ghost-brain `MonitoringEngine` backed by the
//!   lane-aware `ShadowPositionBook`, so canonical shadow lifecycle proof lands in
//!   `shadow_lifecycle.jsonl`.
//! - **lane == "probe"**: registers counterfactual shadow-probe positions in a separate
//!   `MonitoringEngine` backed by an isolated `ShadowPositionBook`, so probe lifecycle proof lands
//!   in the configured `p37_shadow_probe.lifecycle_log_path` without consuming active position
//!   slots or canonical shadow position state.
//! - **lane == "paper"**: delegates the entire lifecycle to
//!   `ghost_brain::PaperPositionLifecycle` (legacy compatibility path).
//!
//! ## Design invariant
//!
//! Paper/shadow lifecycle logic has zero business logic here; ghost-brain is SSOT for those paths.
//! Live sell logic is canonical-first and fail-closed:
//! persist entry price → monitor price → submit/confirm full exit through Sender only.
//! The price loop remains canonical-first:
//! `AccountStateCore` is the primary live truth source and read-only RPC point
//! queries are the bounded fallback when in-process canonical state is absent.
//! ShadowLedger may still be consulted for diagnostic compare only, never as
//! live execution truth.

use crate::components::live_position_registry::{LivePositionRegistry, RecoveryTrackedPosition};
use crate::components::live_tx_sender::{
    LiveTxSender, LiveTxSenderError, SenderConfirmedTransaction, SenderTransactionSubmission,
    HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
};
use crate::components::trigger::safety::{PositionLimitTracker, PositionSlotId, SafetyViolation};
use crate::events::{EventBusReceiver, GhostEvent, PostBuySource, RuntimePlane};
use ghost_brain::events::{EventEmitter, EventWriterConfig};
use ghost_brain::execution::paper_lifecycle::{PaperLifecycleConfig, PaperPositionLifecycle};
use ghost_brain::execution::{CandidateRef, Lane};
use ghost_brain::guardian::post_buy::engine::{PositionEventContext, PositionJoinMetadata};
use ghost_brain::guardian::post_buy::{
    MonitoringEngine, PositionRuntimeRouter, PostBuyGuardianConfig, ShadowPositionBook,
    SignalRouter,
};
use ghost_brain::quotes::{ExecutableQuoteProvider, QuoteProviderConfig};
use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_core::{BondingCurve, LAMPORTS_PER_SOL};
use seer::parse_curve_from_account;
use solana_client::client_error::ClientError;
use solana_client::nonblocking::rpc_client::RpcClient as AsyncRpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;
use solana_sdk::transaction::VersionedTransaction;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};
use tracing::{debug, info, warn};
use trigger::{
    derive_bonding_curve_pda, extract_exit_price_after_sell, AmmProtocol, EntryPriceExtractor,
    EntryPriceInfo, SellTxBuilder, SellTxConfig, BONK_PROGRAM_ID, PUMP_PROGRAM_ID,
};

// ─── Config ─────────────────────────────────────────────────────────────────

const PUMP_TOKEN_DECIMAL_FACTOR: f64 = 1_000_000.0;
const SHADOW_CANONICAL_HANDOFF_WAIT_MS: u64 = 750;
const SHADOW_CANONICAL_HANDOFF_POLL_MS: u64 = 25;

/// Resources needed for live sell execution via launcher-owned Sender submit.
#[derive(Clone)]
pub struct LiveSellHandle {
    /// Async RPC client used for canonical reads needed by the live sell loop.
    pub rpc_client: Arc<AsyncRpcClient>,
    /// Helius Sender + Yellowstone confirmation — authoritative live SELL transport.
    pub live_tx_sender: Arc<LiveTxSender>,
    /// Payer keypair — must be the same key that signed the BUY transaction.
    pub payer: Arc<Keypair>,
    /// Shared canonical account-state runtime truth.
    pub account_state_core: Arc<AccountStateReducer>,
    /// Shadow Ledger retained only for diagnostic dual-read compare.
    pub shadow_ledger: Arc<ShadowLedger>,
}

impl std::fmt::Debug for LiveSellHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use solana_sdk::signer::Signer as _;
        f.debug_struct("LiveSellHandle")
            .field("payer", &self.payer.pubkey().to_string())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectPostBuyHandoffAck {
    Accepted,
    Rejected(&'static str),
}

pub struct DirectPostBuyHandoff {
    event: GhostEvent,
    ack_tx: Option<oneshot::Sender<DirectPostBuyHandoffAck>>,
}

impl DirectPostBuyHandoff {
    pub fn without_ack(event: GhostEvent) -> Self {
        Self {
            event,
            ack_tx: None,
        }
    }

    pub fn with_ack(event: GhostEvent) -> (Self, oneshot::Receiver<DirectPostBuyHandoffAck>) {
        let (ack_tx, ack_rx) = oneshot::channel();
        (
            Self {
                event,
                ack_tx: Some(ack_tx),
            },
            ack_rx,
        )
    }

    pub fn into_parts(self) -> (GhostEvent, Option<oneshot::Sender<DirectPostBuyHandoffAck>>) {
        (self.event, self.ack_tx)
    }
}

pub type DirectPostBuySender = mpsc::UnboundedSender<DirectPostBuyHandoff>;
pub type DirectPostBuyReceiver = mpsc::UnboundedReceiver<DirectPostBuyHandoff>;

pub fn create_direct_post_buy_handoff_channel() -> (DirectPostBuySender, DirectPostBuyReceiver) {
    mpsc::unbounded_channel()
}

/// Configuration for the PostBuyRuntime adapter.
#[derive(Clone)]
pub struct PostBuyRuntimeConfig {
    /// Output directory for ghost-brain EventWriter JSONL files.
    pub events_output_path: PathBuf,
    /// Paper fill delay range (min ms).
    pub paper_fill_delay_min_ms: u64,
    /// Paper fill delay range (max ms).
    pub paper_fill_delay_max_ms: u64,
    /// AEM tick interval in ms.
    pub tick_interval_ms: u64,
    /// Number of ticks before automatic exit (paper mode safety net).
    pub max_ticks_before_exit: u64,
    /// Execution mode: "paper", "live", "dual".
    pub execution_mode: String,
    /// AEM outcome horizon in seconds (ghost-brain `AemConfig.t_s`).
    /// Use a short value (e.g. 1) in tests for deterministic ManagementDecision emission.
    pub aem_t_s: u64,
    /// Runtime limit for concurrently active post-buy positions.
    pub max_concurrent_positions: usize,
    /// Shared bulkhead tracker used by authoritative BUY path.
    pub position_limit_tracker: Option<PositionLimitTracker>,
    /// Live sell engine — when present, live-lane events use Sender-only execution instead of paper.
    pub live_sell: Option<LiveSellHandle>,
    /// Durable registry of open/closed live positions for restart hydration.
    pub live_position_registry: Option<LivePositionRegistry>,
    /// Maximum slippage tolerance mirrored from trigger config (0.20 = 20%).
    pub slippage_tolerance: f64,
    /// Live take-profit threshold as a fraction of entry price (0.02 = +2%).
    pub live_exit_take_profit_pct: f64,
    /// Live stop-loss threshold as a fraction of entry price (0.02 = -2%).
    pub live_exit_stop_loss_pct: f64,
    /// Canonical ShadowLedger shared with the shadow Guardian runtime.
    pub shadow_ledger: Option<Arc<ShadowLedger>>,
    /// Canonical account-state runtime truth shared with shadow guardian.
    pub account_state_core: Option<Arc<AccountStateReducer>>,
    /// Canonical shadow lifecycle/PnL proof log path derived from execution.shadow.*.
    pub shadow_lifecycle_log_path: Option<PathBuf>,
    /// Counterfactual probe lifecycle proof log path derived from p37_shadow_probe.*.
    pub probe_lifecycle_log_path: Option<PathBuf>,
}

impl Default for PostBuyRuntimeConfig {
    fn default() -> Self {
        Self {
            events_output_path: PathBuf::from("datasets/events/events.jsonl"),
            paper_fill_delay_min_ms: 200,
            paper_fill_delay_max_ms: 400,
            tick_interval_ms: 500,
            max_ticks_before_exit: 240, // 120s at 500ms tick
            execution_mode: "paper".to_string(),
            aem_t_s: 120,
            max_concurrent_positions: 1,
            position_limit_tracker: None,
            live_sell: None,
            live_position_registry: None,
            slippage_tolerance: 0.20,
            live_exit_take_profit_pct: 0.02,
            live_exit_stop_loss_pct: 0.02,
            shadow_ledger: None,
            account_state_core: None,
            shadow_lifecycle_log_path: None,
            probe_lifecycle_log_path: None,
        }
    }
}

impl PostBuyRuntimeConfig {
    fn live_exit_slippage_bps(&self) -> u16 {
        percent_fraction_to_bps(self.slippage_tolerance)
    }

    fn live_exit_take_profit_bps(&self) -> u16 {
        percent_fraction_to_bps(self.live_exit_take_profit_pct)
    }

    fn live_exit_stop_loss_bps(&self) -> u16 {
        percent_fraction_to_bps(self.live_exit_stop_loss_pct)
    }
}

fn slippage_tolerance_to_bps(tolerance: f64) -> u16 {
    percent_fraction_to_bps(tolerance)
}

fn percent_fraction_to_bps(value: f64) -> u16 {
    let clamped = if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (clamped * 10_000.0).round() as u16
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Grace window after shutdown during which late `PostBuySubmitted` events are still accepted.
/// This closes the race where shutdown starts while a background shadow simulation is still
/// finalizing and only emits its post-buy handoff a few seconds later.
const POST_BUY_SHUTDOWN_DRAIN_MS: u64 = 10_000;
const POST_BUY_DEDUP_CACHE_CAPACITY: usize = 16_384;

/// Price poll cadence for the live sell monitoring loop.
const LIVE_SELL_POLL_MS: u64 = 500;
/// Bounded retries for post-buy ATA visibility after a confirmed BUY.
const LIVE_SELL_ATA_LOOKUP_MAX_RETRIES: u32 = 5;
const LIVE_SELL_ATA_LOOKUP_RETRY_MS: u64 = 800;
/// Soft warning threshold for live-sell RPC operations.
const LIVE_SELL_RPC_SLOW_MS: u64 = 200;
/// Diagnostic warning threshold for canonical-vs-shadow price divergence.
const POST_BUY_PRICE_DIVERGENCE_WARN_BPS: u64 = 250;
/// Pump.fun tokens use 6 decimals in raw on-chain reserve accounting.
const PUMP_TOKEN_RAW_UNITS_PER_TOKEN: u128 = 1_000_000;
const LIVE_EXIT_PRICE_SCALE_NUMERATOR: u128 = 1_000_000_000;
const LIVE_EXIT_PRICE_SOL_SCALE_FACTOR: f64 =
    LIVE_EXIT_PRICE_SCALE_NUMERATOR as f64 / PUMP_TOKEN_RAW_UNITS_PER_TOKEN as f64;
const LIVE_EXIT_LEGACY_TOKEN_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const LIVE_EXIT_TOKEN_2022_PROGRAM_ID: Pubkey =
    solana_sdk::pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const LIVE_EXIT_ENTRY_PRICE_MAX_RETRIES: u32 = 5;
const LIVE_EXIT_MONITORING_UNAVAILABLE_MAX_POLLS: u32 = 20;
const LIVE_EXIT_BUILD_MAX_RETRIES: u32 = 3;
const LIVE_EXIT_BUILD_RETRY_MS: u64 = 500;
const LIVE_EXIT_EXECUTION_MAX_RETRIES: u32 = 3;
const LIVE_EXIT_EXECUTION_RETRY_MS: u64 = 1_000;
const LIVE_EXIT_EXECUTION_RETRY_MAX_DELAY_MS: u64 = 3_000;
/// Absolute minimum tip floor for SELL sender transactions (lamports).
const LIVE_EXIT_MIN_TIP_LAMPORTS: u64 = 200_000;
/// Hard ceiling for live SELL tips before dynamic floor expansion.
const LIVE_EXIT_MAX_TIP_LAMPORTS: u64 = 1_500_000;
const LIVE_EXIT_THRESHOLD_DENOMINATOR_BPS: u64 = 10_000;

fn resolve_live_exit_tip_lamports(session_tip_lamports: u64) -> u64 {
    session_tip_lamports
        .min(LIVE_EXIT_MAX_TIP_LAMPORTS)
        .max(LIVE_EXIT_MIN_TIP_LAMPORTS)
}

fn saturating_elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn live_exit_retry_delay_ms(retry_attempt: u32) -> u64 {
    LIVE_EXIT_EXECUTION_RETRY_MS
        .saturating_mul(u64::from(retry_attempt.max(1)))
        .min(LIVE_EXIT_EXECUTION_RETRY_MAX_DELAY_MS)
}

fn shadow_entry_price_from_post_buy(
    amount_sol: f64,
    entry_token_amount_raw: Option<u64>,
) -> Option<f64> {
    if !amount_sol.is_finite() || amount_sol <= 0.0 {
        return None;
    }

    entry_token_amount_raw
        .filter(|tokens| *tokens > 0)
        .map(|tokens| amount_sol / (tokens as f64 / PUMP_TOKEN_DECIMAL_FACTOR))
        .filter(|price| price.is_finite() && *price > 0.0)
}

fn build_shadow_guardian_config(config: &PostBuyRuntimeConfig) -> PostBuyGuardianConfig {
    let mut guardian = PostBuyGuardianConfig::default();
    guardian.tick_interval_ms = config.tick_interval_ms;
    guardian.max_monitored_positions = config.max_concurrent_positions;
    guardian.aem.t_s = config.aem_t_s;
    guardian
}

fn record_live_sell_rpc_latency(stage: &'static str, latency_ms: u64, outcome: &'static str) {
    ::metrics::histogram!(
        "post_buy_live_sell_rpc_latency_ms",
        latency_ms as f64,
        "stage" => stage,
        "outcome" => outcome,
    );

    if latency_ms > LIVE_SELL_RPC_SLOW_MS {
        ::metrics::counter!(
            "post_buy_live_sell_rpc_slow_total",
            1u64,
            "stage" => stage,
            "outcome" => outcome,
        );
    }
}

fn record_live_sell_transport_latency(
    stage: &'static str,
    transport: &'static str,
    latency_ms: u64,
    outcome: &'static str,
) {
    ::metrics::histogram!(
        "post_buy_live_sell_transport_latency_ms",
        latency_ms as f64,
        "stage" => stage,
        "transport" => transport,
        "outcome" => outcome,
    );

    if latency_ms > LIVE_SELL_RPC_SLOW_MS {
        ::metrics::counter!(
            "post_buy_live_sell_transport_slow_total",
            1u64,
            "stage" => stage,
            "transport" => transport,
            "outcome" => outcome,
        );
    }
}

#[derive(Debug, Default)]
struct RecentPostBuyCache {
    outcomes: HashMap<String, DirectPostBuyHandoffAck>,
    order: VecDeque<String>,
}

impl RecentPostBuyCache {
    fn reserve(&mut self, candidate_id: &str) -> Option<DirectPostBuyHandoffAck> {
        if let Some(outcome) = self.outcomes.get(candidate_id).copied() {
            return Some(outcome);
        }

        self.outcomes
            .insert(candidate_id.to_string(), DirectPostBuyHandoffAck::Accepted);
        self.order.push_back(candidate_id.to_string());

        while self.order.len() > POST_BUY_DEDUP_CACHE_CAPACITY {
            if let Some(evicted) = self.order.pop_front() {
                self.outcomes.remove(&evicted);
            }
        }

        None
    }

    fn set_outcome(&mut self, candidate_id: &str, outcome: DirectPostBuyHandoffAck) {
        if let Some(entry) = self.outcomes.get_mut(candidate_id) {
            *entry = outcome;
        }
    }
}

fn finish_direct_handoff(
    recent_handoffs: &mut RecentPostBuyCache,
    candidate_id: &str,
    outcome: DirectPostBuyHandoffAck,
) -> DirectPostBuyHandoffAck {
    recent_handoffs.set_outcome(candidate_id, outcome);
    outcome
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LivePriceSource {
    CanonicalAccountState,
    RpcPointQuery,
}

impl LivePriceSource {
    const fn as_label(self) -> &'static str {
        match self {
            Self::CanonicalAccountState => "canonical_account_state",
            Self::RpcPointQuery => "rpc_point_query",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LivePriceSample {
    price: u64,
    source: LivePriceSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveExitStatus {
    BuyConfirmed,
    EntryPricePending,
    Armed,
    Monitoring,
    ExitTriggeredTakeProfit,
    ExitTriggeredStopLoss,
    ExitSubmitted,
    ExitConfirmed,
    EntryPriceFailed,
    MonitoringUnavailable,
    ExitBuildFailed,
    ExitSubmitFailed,
    ExitConfirmFailed,
    LifecycleAbortedWithReason,
}

impl LiveExitStatus {
    const fn as_label(self) -> &'static str {
        match self {
            Self::BuyConfirmed => "buy_confirmed",
            Self::EntryPricePending => "entry_price_pending",
            Self::Armed => "armed",
            Self::Monitoring => "monitoring",
            Self::ExitTriggeredTakeProfit => "exit_triggered_take_profit",
            Self::ExitTriggeredStopLoss => "exit_triggered_stop_loss",
            Self::ExitSubmitted => "exit_submitted",
            Self::ExitConfirmed => "exit_confirmed",
            Self::EntryPriceFailed => "entry_price_failed",
            Self::MonitoringUnavailable => "monitoring_unavailable",
            Self::ExitBuildFailed => "exit_build_failed",
            Self::ExitSubmitFailed => "exit_submit_failed",
            Self::ExitConfirmFailed => "exit_confirm_failed",
            Self::LifecycleAbortedWithReason => "lifecycle_aborted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveExitTrigger {
    TakeProfit,
    StopLoss,
}

impl LiveExitTrigger {
    const fn as_label(self) -> &'static str {
        match self {
            Self::TakeProfit => "take_profit",
            Self::StopLoss => "stop_loss",
        }
    }
}

type LiveExitResult = std::result::Result<(), (LiveExitStatus, String)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveWalletPosition {
    token_account: Pubkey,
    token_program: Pubkey,
    token_amount: u64,
}

#[derive(Debug, Clone)]
struct LiveExitSession {
    candidate_id: String,
    pool_amm_id: Pubkey,
    base_mint: Pubkey,
    creator_pubkey: Option<Pubkey>,
    fee_recipient: Option<Pubkey>,
    buy_signature: String,
    buy_landed_slot: Option<u64>,
    tip_lamports: u64,
    position_slot_id: Option<PositionSlotId>,
    token_account: Option<Pubkey>,
    token_program: Option<Pubkey>,
    token_decimals: Option<u8>,
    token_balance_after_buy: Option<u64>,
    visible_token_balance: Option<u64>,
    tokens_received: Option<u64>,
    sol_spent_lamports: Option<u64>,
    entry_price_lamports_per_token: Option<u64>,
    upper_exit_price_lamports_per_token: Option<u64>,
    lower_exit_price_lamports_per_token: Option<u64>,
    latest_price_lamports_per_token: Option<u64>,
    latest_pnl_pct: Option<f64>,
    status: LiveExitStatus,
    exit_signature: Option<String>,
    last_exit_recent_blockhash: Option<solana_sdk::hash::Hash>,
    last_exit_blockhash_fetched_at: Option<Instant>,
    last_exit_blockhash_fetch_latency_ms: Option<u64>,
    last_exit_submit_slot: Option<u64>,
    exit_landed_slot: Option<u64>,
    terminal_reason: Option<String>,
}

#[derive(Debug)]
struct BuiltLiveExitTransaction {
    transaction: VersionedTransaction,
    blockhash_fetched_at: Instant,
    blockhash_fetch_latency_ms: u64,
    tip_lamports: u64,
    priority_fee_micro_lamports: u64,
}

impl LiveExitSession {
    fn new(
        candidate_id: String,
        pool_amm_id: Pubkey,
        base_mint: Pubkey,
        creator_pubkey: Option<Pubkey>,
        buy_signature: String,
        buy_landed_slot: Option<u64>,
        tip_lamports: u64,
        position_slot_id: Option<PositionSlotId>,
    ) -> Self {
        let mut session = Self {
            candidate_id,
            pool_amm_id,
            base_mint,
            creator_pubkey,
            fee_recipient: None,
            buy_signature,
            buy_landed_slot,
            tip_lamports,
            position_slot_id,
            token_account: None,
            token_program: None,
            token_decimals: None,
            token_balance_after_buy: None,
            visible_token_balance: None,
            tokens_received: None,
            sol_spent_lamports: None,
            entry_price_lamports_per_token: None,
            upper_exit_price_lamports_per_token: None,
            lower_exit_price_lamports_per_token: None,
            latest_price_lamports_per_token: None,
            latest_pnl_pct: None,
            status: LiveExitStatus::BuyConfirmed,
            exit_signature: None,
            last_exit_recent_blockhash: None,
            last_exit_blockhash_fetched_at: None,
            last_exit_blockhash_fetch_latency_ms: None,
            last_exit_submit_slot: None,
            exit_landed_slot: None,
            terminal_reason: None,
        };
        session.transition(LiveExitStatus::BuyConfirmed);
        session
    }

    fn transition(&mut self, status: LiveExitStatus) {
        self.status = status;
        record_live_exit_status(status);
        info!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id = %self.candidate_id,
            pool_amm_id = %self.pool_amm_id,
            base_mint = %self.base_mint,
            buy_signature = %self.buy_signature,
            status = status.as_label(),
            "LiveExit: state transition"
        );
    }

    fn transition_terminal(&mut self, status: LiveExitStatus, reason: impl Into<String>) {
        let reason = reason.into();
        self.status = status;
        self.terminal_reason = Some(reason.clone());
        record_live_exit_status(status);
        record_live_exit_terminal(status, &reason);
        warn!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id = %self.candidate_id,
            pool_amm_id = %self.pool_amm_id,
            base_mint = %self.base_mint,
            buy_signature = %self.buy_signature,
            status = status.as_label(),
            reason = %reason,
            "LiveExit: terminal transition"
        );
    }

    fn rearm_after_retryable_failure(
        &mut self,
        status: LiveExitStatus,
        reason: &str,
        retry_attempt: u32,
        max_retries: u32,
        retry_delay_ms: u64,
    ) {
        warn!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id = %self.candidate_id,
            pool_amm_id = %self.pool_amm_id,
            base_mint = %self.base_mint,
            buy_signature = %self.buy_signature,
            failed_status = status.as_label(),
            reason = %reason,
            retry_attempt,
            retry_escalation_level = retry_attempt,
            max_retries,
            retry_delay_ms,
            previous_exit_signature = %self.exit_signature.as_deref().unwrap_or("none"),
            previous_exit_recent_blockhash = ?self.last_exit_recent_blockhash,
            previous_exit_submit_slot = ?self.last_exit_submit_slot,
            "LiveExit: retrying SELL after retryable failure"
        );
        self.exit_signature = None;
        self.exit_landed_slot = None;
        self.last_exit_blockhash_fetched_at = None;
        self.last_exit_blockhash_fetch_latency_ms = None;
        self.last_exit_submit_slot = None;
        self.terminal_reason = None;
        self.transition(LiveExitStatus::Monitoring);
    }

    fn populate_entry_price(
        &mut self,
        entry_info: &EntryPriceInfo,
        config: &PostBuyRuntimeConfig,
    ) -> std::result::Result<(), String> {
        let upper_exit_price = scale_live_exit_price(
            entry_info.price_lamports_per_token,
            LIVE_EXIT_THRESHOLD_DENOMINATOR_BPS
                .saturating_add(u64::from(config.live_exit_take_profit_bps())),
            LIVE_EXIT_THRESHOLD_DENOMINATOR_BPS,
        )?;
        let lower_exit_price = scale_live_exit_price(
            entry_info.price_lamports_per_token,
            LIVE_EXIT_THRESHOLD_DENOMINATOR_BPS
                .saturating_sub(u64::from(config.live_exit_stop_loss_bps())),
            LIVE_EXIT_THRESHOLD_DENOMINATOR_BPS,
        )?;

        self.tokens_received = Some(entry_info.tokens_received);
        self.sol_spent_lamports = Some(entry_info.sol_spent);
        self.entry_price_lamports_per_token = Some(entry_info.price_lamports_per_token);
        self.buy_landed_slot = self.buy_landed_slot.or(Some(entry_info.slot));
        self.token_account = Some(entry_info.token_account);
        self.token_decimals = Some(entry_info.token_decimals);
        self.token_balance_after_buy = Some(entry_info.token_balance_after_buy);
        self.fee_recipient = self.fee_recipient.or(entry_info.fee_recipient);
        self.upper_exit_price_lamports_per_token = Some(upper_exit_price);
        self.lower_exit_price_lamports_per_token = Some(lower_exit_price);
        self.latest_price_lamports_per_token = Some(entry_info.price_lamports_per_token);
        self.latest_pnl_pct = Some(0.0);
        if let Some(token_program) = entry_info.token_program {
            self.set_token_program(token_program);
        }

        info!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id = %self.candidate_id,
            pool_amm_id = %self.pool_amm_id,
            base_mint = %self.base_mint,
            buy_signature = %self.buy_signature,
            buy_landed_slot = ?self.buy_landed_slot,
            tokens_received = entry_info.tokens_received,
            token_account = %entry_info.token_account,
            token_balance_after_buy = entry_info.token_balance_after_buy,
            token_decimals = entry_info.token_decimals,
            token_program = ?entry_info.token_program,
            fee_recipient = ?self.fee_recipient,
            sol_spent_lamports = entry_info.sol_spent,
            entry_price_lamports_per_token = entry_info.price_lamports_per_token,
            take_profit_pct = config.live_exit_take_profit_pct,
            stop_loss_pct = config.live_exit_stop_loss_pct,
            upper_exit_price_lamports_per_token = upper_exit_price,
            lower_exit_price_lamports_per_token = lower_exit_price,
            "LiveExit: persisted confirmed BUY entry metadata"
        );

        Ok(())
    }

    fn set_token_program(&mut self, token_program: Pubkey) {
        if self.token_program == Some(token_program) {
            return;
        }
        self.token_program = Some(token_program);
        info!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id = %self.candidate_id,
            pool_amm_id = %self.pool_amm_id,
            base_mint = %self.base_mint,
            token_program = %token_program,
            "LiveExit: resolved sell token program"
        );
    }

    fn apply_visible_wallet_position(&mut self, position: LiveWalletPosition) {
        self.token_account = self.token_account.or(Some(position.token_account));
        self.visible_token_balance = Some(position.token_amount);
        self.set_token_program(position.token_program);
    }

    fn record_price_sample(&mut self, price: u64) {
        self.latest_price_lamports_per_token = Some(price);
        self.latest_pnl_pct = self
            .entry_price_lamports_per_token
            .map(|entry_price| live_exit_pnl_pct(entry_price, price));
    }

    fn mark_exit_submitted(&mut self, submission: &SenderTransactionSubmission) {
        self.exit_signature = Some(submission.signature.to_string());
        info!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id = %self.candidate_id,
            pool_amm_id = %self.pool_amm_id,
            base_mint = %self.base_mint,
            buy_signature = %self.buy_signature,
            exit_signature = %submission.signature,
            "LiveExit: submitted SELL transaction and awaiting Yellowstone confirmation"
        );
        self.transition(LiveExitStatus::ExitSubmitted);
    }

    fn mark_exit_confirmed(
        &mut self,
        confirmed: &SenderConfirmedTransaction,
        trigger: LiveExitTrigger,
    ) {
        let reason = format!("{}_confirmed", trigger.as_label());
        self.exit_signature = Some(confirmed.signature.to_string());
        self.exit_landed_slot = confirmed.landed_slot;
        self.status = LiveExitStatus::ExitConfirmed;
        self.terminal_reason = Some(reason.clone());
        record_live_exit_status(LiveExitStatus::ExitConfirmed);
        record_live_exit_terminal(LiveExitStatus::ExitConfirmed, &reason);
        info!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id = %self.candidate_id,
            pool_amm_id = %self.pool_amm_id,
            base_mint = %self.base_mint,
            buy_signature = %self.buy_signature,
            exit_signature = %confirmed.signature,
            exit_landed_slot = ?confirmed.landed_slot,
            trigger = trigger.as_label(),
            "LiveExit: confirmed full exit"
        );
    }

    fn should_release_position_slot(&self) -> bool {
        // Release only after a confirmed on-chain exit.
        // Terminal SELL failure may still leave wallet exposure stranded, so the slot must remain
        // reserved fail-closed until recovery/hydration can reconcile it explicitly.
        matches!(self.status, LiveExitStatus::ExitConfirmed)
    }

    fn sellable_token_amount(&self) -> Option<u64> {
        self.visible_token_balance
            .or(self.token_balance_after_buy)
            .or(self.tokens_received)
    }
}

fn scale_live_exit_price(
    price: u64,
    numerator: u64,
    denominator: u64,
) -> std::result::Result<u64, String> {
    let scaled = u128::from(price)
        .checked_mul(u128::from(numerator))
        .ok_or_else(|| format!("price scaling overflow: price={price} numerator={numerator}"))?
        .checked_div(u128::from(denominator))
        .ok_or_else(|| format!("price scaling underflow: denominator={denominator}"))?;
    u64::try_from(scaled).map_err(|_| format!("scaled price does not fit u64: {scaled}"))
}

fn live_exit_pnl_pct(entry_price: u64, current_price: u64) -> f64 {
    if entry_price == 0 {
        return 0.0;
    }

    ((current_price as f64 / entry_price as f64) - 1.0) * 100.0
}

async fn log_realized_exit_price_after_confirmation(
    live: &LiveSellHandle,
    session: &LiveExitSession,
    confirmed: &SenderConfirmedTransaction,
    trigger: LiveExitTrigger,
) {
    let extraction_started_at = Instant::now();
    match extract_exit_price_after_sell(
        Arc::clone(&live.rpc_client),
        &confirmed.signature,
        &live.payer.pubkey(),
        &session.base_mint,
    )
    .await
    {
        Ok(metadata) => {
            let realized_pnl_pct = session
                .entry_price_lamports_per_token
                .map(|entry_price| live_exit_pnl_pct(entry_price, metadata.exit_price));
            let token_decimals = metadata.token_decimals;
            let extraction_latency_ms = saturating_elapsed_ms(extraction_started_at);
            ::metrics::counter!(
                "post_buy_live_exit_realized_price_extraction_total",
                1u64,
                "result" => "ok"
            );
            ::metrics::gauge!(
                "post_buy_live_exit_realized_price_lamports_per_token",
                metadata.exit_price as f64
            );
            ::metrics::gauge!(
                "post_buy_live_exit_realized_sol_received_lamports",
                metadata.sol_received as f64
            );
            if let Some(pnl_pct) = realized_pnl_pct {
                ::metrics::gauge!("post_buy_live_exit_realized_pnl_pct", pnl_pct);
            }
            info!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id = %session.candidate_id,
                pool_amm_id = %session.pool_amm_id,
                base_mint = %session.base_mint,
                buy_signature = %session.buy_signature,
                exit_signature = %confirmed.signature,
                exit_landed_slot = ?confirmed.landed_slot,
                trigger = trigger.as_label(),
                rpc_extract_latency_ms = extraction_latency_ms,
                entry_price_lamports_per_token = ?session.entry_price_lamports_per_token,
                sell_trigger_price_lamports_per_token = ?session.latest_price_lamports_per_token,
                upper_exit_price_lamports_per_token = ?session.upper_exit_price_lamports_per_token,
                lower_exit_price_lamports_per_token = ?session.lower_exit_price_lamports_per_token,
                realized_exit_price_lamports_per_token = metadata.exit_price,
                realized_pnl_pct = ?realized_pnl_pct,
                sol_received_lamports = metadata.sol_received,
                wallet_net_sol_change_lamports = metadata.payer_wallet_net_change,
                payer_outgoing_transfer_lamports = metadata.payer_outgoing_transfer_lamports,
                network_fee_lamports = metadata.network_fee_lamports,
                tokens_sold_raw = metadata.tokens_sold,
                tokens_sold_ui = raw_token_amount_to_ui(metadata.tokens_sold, token_decimals),
                token_account = %metadata.token_account,
                token_balance_before_sell_raw = metadata.token_balance_before_sell,
                token_balance_before_sell_ui =
                    raw_token_amount_to_ui(metadata.token_balance_before_sell, token_decimals),
                token_balance_after_sell_raw = metadata.token_balance_after_sell,
                token_balance_after_sell_ui =
                    raw_token_amount_to_ui(metadata.token_balance_after_sell, token_decimals),
                token_decimals = token_decimals,
                token_program = ?metadata.token_program,
                "LiveExit: realized exit price extracted from confirmed SELL"
            );
        }
        Err(error) => {
            let extraction_latency_ms = saturating_elapsed_ms(extraction_started_at);
            ::metrics::counter!(
                "post_buy_live_exit_realized_price_extraction_total",
                1u64,
                "result" => "error"
            );
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id = %session.candidate_id,
                pool_amm_id = %session.pool_amm_id,
                base_mint = %session.base_mint,
                buy_signature = %session.buy_signature,
                exit_signature = %confirmed.signature,
                exit_landed_slot = ?confirmed.landed_slot,
                trigger = trigger.as_label(),
                rpc_extract_latency_ms = extraction_latency_ms,
                error = %error,
                "LiveExit: failed to extract realized exit price from confirmed SELL"
            );
        }
    }
}

fn determine_live_exit_trigger(
    session: &LiveExitSession,
    current_price: u64,
) -> Option<LiveExitTrigger> {
    let lower = session.lower_exit_price_lamports_per_token?;
    let upper = session.upper_exit_price_lamports_per_token?;

    if current_price <= lower {
        Some(LiveExitTrigger::StopLoss)
    } else if current_price >= upper {
        Some(LiveExitTrigger::TakeProfit)
    } else {
        None
    }
}

fn record_live_exit_status(status: LiveExitStatus) {
    ::metrics::counter!(
        "post_buy_live_exit_status_total",
        1u64,
        "status" => status.as_label()
    );
}

fn record_live_exit_terminal(status: LiveExitStatus, reason: &str) {
    ::metrics::counter!(
        "post_buy_live_exit_terminal_total",
        1u64,
        "status" => status.as_label(),
        "reason" => reason.to_string()
    );
}

fn record_live_exit_trigger(trigger: LiveExitTrigger) {
    ::metrics::counter!(
        "post_buy_live_exit_trigger_total",
        1u64,
        "trigger" => trigger.as_label()
    );
}

fn record_live_exit_retry(status: LiveExitStatus) {
    ::metrics::counter!(
        "post_buy_live_exit_retry_total",
        1u64,
        "status" => status.as_label()
    );
}

fn is_retryable_live_exit_failure(status: LiveExitStatus) -> bool {
    matches!(
        status,
        LiveExitStatus::ExitSubmitFailed | LiveExitStatus::ExitConfirmFailed
    )
}

fn record_post_buy_price_source(source: &'static str) {
    ::metrics::counter!("post_buy_price_source_total", 1u64, "source" => source);
}

fn raw_token_amount_to_ui(raw_amount: u64, decimals: u8) -> f64 {
    if decimals == 0 {
        return raw_amount as f64;
    }

    raw_amount as f64 / 10f64.powi(i32::from(decimals))
}

fn record_live_exit_snapshot_metrics(
    session: &LiveExitSession,
    source: &'static str,
    price_available: bool,
) {
    let decimals = session.token_decimals.unwrap_or(6);
    ::metrics::counter!(
        "post_buy_live_exit_snapshot_total",
        1u64,
        "source" => source,
        "price_available" => if price_available { "true" } else { "false" },
        "wallet_position_visible" => if session.visible_token_balance.is_some() {
            "true"
        } else {
            "false"
        }
    );
    ::metrics::gauge!(
        "post_buy_live_exit_price_available",
        if price_available { 1.0 } else { 0.0 }
    );
    ::metrics::gauge!(
        "post_buy_live_exit_wallet_position_visible",
        if session.visible_token_balance.is_some() {
            1.0
        } else {
            0.0
        }
    );
    ::metrics::gauge!("post_buy_live_exit_token_decimals", decimals as f64);
    ::metrics::gauge!(
        "post_buy_live_exit_entry_price_lamports_per_token",
        session.entry_price_lamports_per_token.unwrap_or_default() as f64
    );
    ::metrics::gauge!(
        "post_buy_live_exit_current_price_lamports_per_token",
        session.latest_price_lamports_per_token.unwrap_or_default() as f64
    );
    ::metrics::gauge!(
        "post_buy_live_exit_upper_price_lamports_per_token",
        session
            .upper_exit_price_lamports_per_token
            .unwrap_or_default() as f64
    );
    ::metrics::gauge!(
        "post_buy_live_exit_lower_price_lamports_per_token",
        session
            .lower_exit_price_lamports_per_token
            .unwrap_or_default() as f64
    );
    ::metrics::gauge!(
        "post_buy_live_exit_pnl_pct",
        session.latest_pnl_pct.unwrap_or_default()
    );
    let tokens_received = session.tokens_received.unwrap_or_default();
    ::metrics::gauge!(
        "post_buy_live_exit_tokens_received_raw",
        tokens_received as f64
    );
    ::metrics::gauge!(
        "post_buy_live_exit_tokens_received_ui",
        raw_token_amount_to_ui(tokens_received, decimals)
    );
    let token_balance_after_buy = session.token_balance_after_buy.unwrap_or_default();
    ::metrics::gauge!(
        "post_buy_live_exit_token_balance_after_buy_raw",
        token_balance_after_buy as f64
    );
    ::metrics::gauge!(
        "post_buy_live_exit_token_balance_after_buy_ui",
        raw_token_amount_to_ui(token_balance_after_buy, decimals)
    );
    let visible_token_balance = session.visible_token_balance.unwrap_or_default();
    ::metrics::gauge!(
        "post_buy_live_exit_visible_token_balance_raw",
        visible_token_balance as f64
    );
    ::metrics::gauge!(
        "post_buy_live_exit_visible_token_balance_ui",
        raw_token_amount_to_ui(visible_token_balance, decimals)
    );
}

fn log_live_exit_snapshot(session: &LiveExitSession, source: &'static str, price_available: bool) {
    let decimals = session.token_decimals.unwrap_or(6);
    info!(
        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
        candidate_id = %session.candidate_id,
        base_mint = %session.base_mint,
        token_account = ?session.token_account,
        token_program = ?session.token_program,
        token_decimals = decimals,
        wallet_position_visible = session.visible_token_balance.is_some(),
        price_source = source,
        price_available,
        entry_price_lamports_per_token = ?session.entry_price_lamports_per_token,
        current_price_lamports_per_token = ?session.latest_price_lamports_per_token,
        upper_exit_price_lamports_per_token = ?session.upper_exit_price_lamports_per_token,
        lower_exit_price_lamports_per_token = ?session.lower_exit_price_lamports_per_token,
        pnl_pct = ?session.latest_pnl_pct,
        tokens_received_raw = ?session.tokens_received,
        tokens_received_ui = session
            .tokens_received
            .map(|raw| raw_token_amount_to_ui(raw, decimals)),
        token_balance_after_buy_raw = ?session.token_balance_after_buy,
        token_balance_after_buy_ui = session
            .token_balance_after_buy
            .map(|raw| raw_token_amount_to_ui(raw, decimals)),
        visible_token_balance_raw = ?session.visible_token_balance,
        visible_token_balance_ui = session
            .visible_token_balance
            .map(|raw| raw_token_amount_to_ui(raw, decimals)),
        "LiveExit: price snapshot"
    );
}

fn record_post_buy_shadow_compare(
    mint: &Pubkey,
    primary_source: LivePriceSource,
    primary_price: u64,
    shadow_price: Option<u64>,
) {
    let primary_source = primary_source.as_label();

    let Some(shadow_price) = shadow_price else {
        ::metrics::counter!(
            "post_buy_shadow_compare_total",
            1u64,
            "primary_source" => primary_source,
            "result" => "shadow_missing"
        );
        return;
    };

    let diff_bps = if primary_price == 0 {
        0
    } else {
        let abs_diff = primary_price.abs_diff(shadow_price);
        ((abs_diff as u128) * 10_000 / u128::from(primary_price)) as u64
    };

    ::metrics::histogram!(
        "post_buy_shadow_compare_diff_bps",
        diff_bps as f64,
        "primary_source" => primary_source
    );
    ::metrics::counter!(
        "post_buy_shadow_compare_total",
        1u64,
        "primary_source" => primary_source,
        "result" => if diff_bps == 0 { "match" } else { "diverged" }
    );

    if diff_bps >= POST_BUY_PRICE_DIVERGENCE_WARN_BPS {
        warn!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            mint = %mint,
            primary_source,
            primary_price,
            shadow_price,
            diff_bps,
            "LiveSell: canonical live price diverged from diagnostic shadow compare"
        );
    } else if diff_bps > 0 {
        debug!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            mint = %mint,
            primary_source,
            primary_price,
            shadow_price,
            diff_bps,
            "LiveSell: diagnostic shadow compare observed bounded price divergence"
        );
    }
}

/// Convert raw reserves into the same 1e9-scaled lamports/token contract used by
/// `EntryPriceExtractor` and sell min-output calculations.
fn price_lamports_from_raw_reserves(sol_reserves: u64, token_reserves: u64) -> Option<u64> {
    if sol_reserves == 0 || token_reserves == 0 {
        return None;
    }

    let numerator = u128::from(sol_reserves).saturating_mul(LIVE_EXIT_PRICE_SCALE_NUMERATOR);
    let denominator = u128::from(token_reserves);
    let rounded = numerator
        .saturating_add(denominator / 2)
        .checked_div(denominator)?;
    u64::try_from(rounded).ok()
}

#[derive(Debug, Clone, Copy, Default)]
struct LiveCurveExecutionHints {
    cashback_enabled: bool,
    real_sol_reserves: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SignatureStatusObservation {
    Confirmed { slot: u64 },
    Failed { slot: u64, error: String },
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SenderSellAttemptConfirmation {
    Confirmed {
        source: &'static str,
        landed_slot: Option<u64>,
    },
    Failed {
        source: &'static str,
        detail: String,
    },
    Uncertain,
}

fn curve_cashback_enabled_from_account_data(data: &[u8]) -> bool {
    data.len() > 82 && data[82] != 0
}

fn cap_live_exit_min_output(min_output: u64, real_sol_reserves: Option<u64>) -> u64 {
    match real_sol_reserves {
        Some(reserves) if reserves > 0 => min_output.min(reserves.saturating_sub(1).max(1)),
        _ => min_output,
    }
}

async fn fetch_curve_account_data_processed(
    rpc_client: &AsyncRpcClient,
    curve_key: &Pubkey,
    metric_name: &'static str,
) -> std::result::Result<Vec<u8>, String> {
    const CURVE_ACCOUNT_FETCH_MAX_ATTEMPTS: usize = 4;
    const CURVE_ACCOUNT_FETCH_RETRY_DELAY_MS: u64 = 75;

    let mut last_error = None;
    for attempt in 1..=CURVE_ACCOUNT_FETCH_MAX_ATTEMPTS {
        let rpc_started_at = Instant::now();
        match rpc_client
            .get_account_with_commitment(curve_key, CommitmentConfig::processed())
            .await
        {
            Ok(response) => {
                let latency_ms = saturating_elapsed_ms(rpc_started_at);
                match response.value {
                    Some(account) => {
                        record_live_sell_rpc_latency(metric_name, latency_ms, "ok");
                        return Ok(account.data);
                    }
                    None => {
                        record_live_sell_rpc_latency(metric_name, latency_ms, "account_not_found");
                        last_error = Some(format!("AccountNotFound: pubkey={curve_key}"));
                    }
                }
            }
            Err(error) => {
                record_live_sell_rpc_latency(
                    metric_name,
                    saturating_elapsed_ms(rpc_started_at),
                    "rpc_error",
                );
                last_error = Some(error.to_string());
            }
        }

        if attempt < CURVE_ACCOUNT_FETCH_MAX_ATTEMPTS {
            tokio::time::sleep(Duration::from_millis(CURVE_ACCOUNT_FETCH_RETRY_DELAY_MS)).await;
        }
    }

    Err(last_error.unwrap_or_else(|| format!("AccountNotFound: pubkey={curve_key}")))
}

async fn read_live_curve_execution_hints(
    rpc_client: &AsyncRpcClient,
    mint: &Pubkey,
) -> std::result::Result<LiveCurveExecutionHints, String> {
    let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID)
        .map_err(|error| format!("pump_program_parse_failed: {error}"))?;
    let curve_key = derive_bonding_curve_pda(mint, &pump_program).0;
    let account_data =
        fetch_curve_account_data_processed(rpc_client, &curve_key, "live_exit_curve_hints")
            .await
            .map_err(|error| {
                format!(
                    "curve_hints_get_account_data_failed: mint={} curve={} error={}",
                    mint, curve_key, error
                )
            })?;
    let latency_ms = 0;
    let curve = match parse_curve_from_account(&account_data) {
        Ok(curve) => curve,
        Err(error) => {
            record_live_sell_rpc_latency("live_exit_curve_hints", latency_ms, "parse_error");
            return Err(format!(
                "curve_hints_parse_failed: mint={} curve={} error={}",
                mint, curve_key, error
            ));
        }
    };
    Ok(LiveCurveExecutionHints {
        cashback_enabled: curve_cashback_enabled_from_account_data(&account_data),
        real_sol_reserves: (curve.real_sol_reserves > 0).then_some(curve.real_sol_reserves),
    })
}

fn is_missing_token_account_balance_error(err: &ClientError) -> bool {
    let message = err.to_string();
    message.contains("AccountNotFound")
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

async fn fetch_signature_status_observation(
    rpc_client: &AsyncRpcClient,
    signature: &Signature,
) -> std::result::Result<SignatureStatusObservation, String> {
    let response = rpc_client
        .get_signature_statuses_with_history(&[*signature])
        .await
        .map_err(|err| format!("getSignatureStatuses failed for {signature}: {err}"))?;
    let maybe_status = response.value.into_iter().next().flatten();

    Ok(match maybe_status {
        Some(status) => match status.err {
            Some(err) => SignatureStatusObservation::Failed {
                slot: status.slot,
                error: format!("{err:?}"),
            },
            None => SignatureStatusObservation::Confirmed { slot: status.slot },
        },
        None => SignatureStatusObservation::Missing,
    })
}

async fn fetch_token_account_balance(
    rpc_client: &AsyncRpcClient,
    ata: &Pubkey,
) -> std::result::Result<Option<u64>, String> {
    match rpc_client.get_token_account_balance(ata).await {
        Ok(response) => response
            .amount
            .parse::<u64>()
            .map(Some)
            .map_err(|err| format!("invalid token balance response for {ata}: {err}")),
        Err(err) if is_missing_token_account_balance_error(&err) => Ok(None),
        Err(err) => Err(format!("getTokenAccountBalance failed for {ata}: {err}")),
    }
}

async fn confirm_sender_sell_attempt(
    live: &LiveSellHandle,
    candidate_id: String,
    base_mint: Pubkey,
    token_account: Option<Pubkey>,
    expected_pre_submit_balance: u64,
    submission: &SenderTransactionSubmission,
) -> SenderSellAttemptConfirmation {
    confirm_sender_sell_attempt_with_timeout(
        live,
        candidate_id,
        base_mint,
        token_account,
        expected_pre_submit_balance,
        submission,
        12_000,
    )
    .await
}

async fn confirm_sender_sell_attempt_with_timeout(
    live: &LiveSellHandle,
    candidate_id: String,
    base_mint: Pubkey,
    token_account: Option<Pubkey>,
    expected_pre_submit_balance: u64,
    submission: &SenderTransactionSubmission,
    max_wait_ms: u64,
) -> SenderSellAttemptConfirmation {
    const SELL_CONFIRM_POLL_MS: u64 = 250;

    let deadline = Instant::now() + Duration::from_millis(max_wait_ms);
    let mut yellowstone_finished = false;
    let mut balance_delta_observed = false;
    let mut balance_zero_observed = false;
    let mut wallet_absent_observed = false;
    let signature = submission.signature;
    let confirm_future = live
        .live_tx_sender
        .confirm_submission_with_timeout(submission, max_wait_ms);
    tokio::pin!(confirm_future);

    loop {
        if expected_pre_submit_balance > 0 {
            if let Some(token_account) = token_account {
                match fetch_token_account_balance(&live.rpc_client, &token_account).await {
                    Ok(Some(post_submit_balance)) => {
                        if post_submit_balance < expected_pre_submit_balance {
                            balance_delta_observed = true;
                        }
                        if post_submit_balance == 0 {
                            balance_zero_observed = true;
                        }
                    }
                    Ok(None) => {
                        balance_delta_observed = true;
                        balance_zero_observed = true;
                    }
                    Err(err) => {
                        warn!(
                            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                            candidate_id = %candidate_id,
                            base_mint = %base_mint,
                            exit_signature = %signature,
                            token_account = %token_account,
                            error = %err,
                            "LiveExit: SELL fallback token-balance check failed — retrying"
                        );
                    }
                }
            } else if query_live_wallet_position(&live.rpc_client, &live.payer.pubkey(), &base_mint)
                .await
                .is_none()
            {
                wallet_absent_observed = true;
            }
        }

        match fetch_signature_status_observation(&live.rpc_client, &signature).await {
            Ok(SignatureStatusObservation::Confirmed { slot }) => {
                return SenderSellAttemptConfirmation::Confirmed {
                    source: if balance_delta_observed {
                        "balance_delta"
                    } else {
                        "signature_status"
                    },
                    landed_slot: Some(slot),
                };
            }
            Ok(SignatureStatusObservation::Failed { slot, error }) => {
                return SenderSellAttemptConfirmation::Failed {
                    source: "signature_status",
                    detail: format!("slot={slot} err={error}"),
                };
            }
            Ok(SignatureStatusObservation::Missing) => {}
            Err(err) => {
                warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    candidate_id = %candidate_id,
                    base_mint = %base_mint,
                    exit_signature = %signature,
                    error = %err,
                    "LiveExit: SELL fallback signature-status check failed — retrying"
                );
            }
        }

        if Instant::now() >= deadline {
            if balance_zero_observed {
                return SenderSellAttemptConfirmation::Confirmed {
                    source: "balance_zero",
                    landed_slot: None,
                };
            }
            if wallet_absent_observed {
                return SenderSellAttemptConfirmation::Confirmed {
                    source: "wallet_absent",
                    landed_slot: None,
                };
            }
            return SenderSellAttemptConfirmation::Uncertain;
        }

        tokio::select! {
            confirmation = &mut confirm_future, if !yellowstone_finished => {
                yellowstone_finished = true;
                match confirmation {
                    Ok(confirmed_transaction) => {
                        return SenderSellAttemptConfirmation::Confirmed {
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
                        if is_yellowstone_resource_exhausted(&err) {
                            warn!(
                                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                                candidate_id = %candidate_id,
                                base_mint = %base_mint,
                                exit_signature = %signature,
                                "LiveExit: Yellowstone confirmation hit stream limits; deferring to SELL balance/status checks"
                            );
                        } else {
                            warn!(
                                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                                candidate_id = %candidate_id,
                                base_mint = %base_mint,
                                exit_signature = %signature,
                                error = %err,
                                "LiveExit: Yellowstone confirmation unavailable; deferring to SELL balance/status checks"
                            );
                        }
                    }
                    Err(LiveTxSenderError::ConfirmationRejected { signature, slot }) => {
                        return SenderSellAttemptConfirmation::Failed {
                            source: "yellowstone",
                            detail: format!("{signature}@{slot}: rejected"),
                        };
                    }
                    Err(LiveTxSenderError::Submit { message }) => {
                        return SenderSellAttemptConfirmation::Failed {
                            source: "yellowstone",
                            detail: message,
                        };
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(SELL_CONFIRM_POLL_MS)) => {}
        }
    }
}

fn try_canonical_live_price(
    account_state_core: &AccountStateReducer,
    mint: &Pubkey,
) -> Option<u64> {
    let state = account_state_core.get_canonical_state(mint)?;
    if state.price_sol.is_finite() && state.price_sol > 0.0 {
        return Some(
            (state.price_sol * LAMPORTS_PER_SOL as f64 * LIVE_EXIT_PRICE_SOL_SCALE_FACTOR).round()
                as u64,
        );
    }

    let token_reserves = if state.real_token_reserves > 0 {
        state.real_token_reserves
    } else {
        state.virtual_token_reserves
    };
    let sol_reserves = if state.real_sol_reserves > 0 {
        state.real_sol_reserves
    } else {
        state.virtual_sol_reserves
    };

    if token_reserves == 0 || sol_reserves == 0 {
        return None;
    }

    price_lamports_from_raw_reserves(sol_reserves, token_reserves)
}

async fn read_price_from_rpc_point_query(
    rpc_client: &AsyncRpcClient,
    mint: &Pubkey,
) -> Option<u64> {
    let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).ok()?;
    let bonk_program = Pubkey::from_str(BONK_PROGRAM_ID).ok()?;
    let candidates = [
        derive_bonding_curve_pda(mint, &pump_program).0,
        derive_bonding_curve_pda(mint, &bonk_program).0,
    ];

    for curve_key in candidates {
        match fetch_curve_account_data_processed(
            rpc_client,
            &curve_key,
            "post_buy_price_point_query",
        )
        .await
        {
            Ok(account_data) => {
                let latency_ms = 0;
                let Ok(curve) = parse_curve_from_account(&account_data) else {
                    record_live_sell_rpc_latency(
                        "post_buy_price_point_query",
                        latency_ms,
                        "parse_error",
                    );
                    debug!(
                        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                        mint = %mint,
                        curve = %curve_key,
                        latency_ms,
                        "LiveSell: RPC point query returned non-bonding-curve data"
                    );
                    continue;
                };
                if curve.virtual_token_reserves == 0 || curve.virtual_sol_reserves == 0 {
                    record_live_sell_rpc_latency(
                        "post_buy_price_point_query",
                        latency_ms,
                        "zero_reserves",
                    );
                    debug!(
                        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                        mint = %mint,
                        curve = %curve_key,
                        latency_ms,
                        "LiveSell: RPC point query returned zero-reserve bonding curve"
                    );
                    continue;
                }

                let Some(price) = price_lamports_from_raw_reserves(
                    curve.virtual_sol_reserves,
                    curve.virtual_token_reserves,
                ) else {
                    record_live_sell_rpc_latency(
                        "post_buy_price_point_query",
                        latency_ms,
                        "zero_reserves",
                    );
                    continue;
                };
                record_live_sell_rpc_latency("post_buy_price_point_query", latency_ms, "ok");
                return Some(price);
            }
            Err(error) => {
                debug!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    mint = %mint,
                    curve = %curve_key,
                    error = %error,
                    "LiveSell: RPC point query failed for bonding curve"
                );
            }
        }
    }

    None
}

fn read_shadow_price_for_compare(shadow_ledger: &ShadowLedger, mint: &Pubkey) -> Option<u64> {
    let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).ok()?;
    let bonk_program = Pubkey::from_str(BONK_PROGRAM_ID).ok()?;
    let candidates = [
        derive_bonding_curve_pda(mint, &pump_program).0,
        derive_bonding_curve_pda(mint, &bonk_program).0,
        *mint, // legacy direct-mint alias preserved only for diagnostic compare
    ];

    let mut best: Option<(u64, u64)> = None;
    for key in &candidates {
        if let Some(shadow) = shadow_ledger.get_old(key) {
            let vt = shadow.curve.virtual_token_reserves;
            let vs = shadow.curve.virtual_sol_reserves;
            if vt == 0 || vs == 0 {
                continue;
            }
            let Some(price) = price_lamports_from_raw_reserves(vs, vt) else {
                continue;
            };
            if best.map_or(true, |(_, slot)| shadow.last_updated_slot > slot) {
                best = Some((price, shadow.last_updated_slot));
            }
        }
    }
    best.map(|(price, _)| price)
}

async fn read_live_price_sample(live: &LiveSellHandle, mint: &Pubkey) -> Option<LivePriceSample> {
    if let Some(price) = try_canonical_live_price(&live.account_state_core, mint) {
        record_post_buy_price_source(LivePriceSource::CanonicalAccountState.as_label());
        return Some(LivePriceSample {
            price,
            source: LivePriceSource::CanonicalAccountState,
        });
    }

    if let Some(price) = read_price_from_rpc_point_query(&live.rpc_client, mint).await {
        record_post_buy_price_source(LivePriceSource::RpcPointQuery.as_label());
        return Some(LivePriceSample {
            price,
            source: LivePriceSource::RpcPointQuery,
        });
    }

    record_post_buy_price_source("unavailable");
    None
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Start the PostBuyRuntime subscriber loop.
///
/// MUST be called BEFORE any event producers start sending events.
pub async fn run(
    mut event_rx: EventBusReceiver,
    mut shutdown_rx: broadcast::Receiver<()>,
    mut direct_handoff_rx: Option<DirectPostBuyReceiver>,
    config: PostBuyRuntimeConfig,
) {
    let output_dir = config.events_output_path.to_string_lossy().to_string();

    // Initialize ghost-brain EventEmitter (shared by canonical paper/shadow post-buy paths).
    let lane = match config.execution_mode.as_str() {
        "live" => Lane::Live,
        "shadow" => Lane::Shadow,
        "dual" => Lane::Single,
        _ => Lane::Paper,
    };
    let run_id = format!("launcher-{}", now_ms());
    let writer_config = EventWriterConfig {
        output_dir,
        enable_aem_ticks: true,
        enable_optional_events: true,
        flush_interval_ms: 100,
        ..EventWriterConfig::default()
    };
    let emitter = match EventEmitter::new(writer_config, run_id.clone(), lane) {
        Ok(e) => Arc::new(e),
        Err(e) => {
            warn!("PostBuyRuntime: failed to create EventEmitter: {}", e);
            return;
        }
    };

    // Shared QuoteProvider for ghost-brain PaperBroker (paper compatibility path only)
    let quote_provider = Arc::new(RwLock::new(ExecutableQuoteProvider::new(
        QuoteProviderConfig {
            max_quote_age_ms: 5000,
            ring_buffer_size: 256,
            generation_interval_ms: 100,
            stale_warning_threshold_ms: 3000,
        },
    )));

    // Build ghost-brain lifecycle config (paper compatibility path only)
    let lifecycle_config = PaperLifecycleConfig {
        fill_delay_min_ms: config.paper_fill_delay_min_ms,
        fill_delay_max_ms: config.paper_fill_delay_max_ms,
        tick_interval_ms: config.tick_interval_ms,
        max_ticks: config.max_ticks_before_exit,
        aem_t_s: config.aem_t_s,
        max_open_positions: config.max_concurrent_positions,
    };

    let lifecycle = Arc::new(PaperPositionLifecycle::new(
        lifecycle_config,
        emitter.clone(),
        quote_provider,
    ));

    let mut shadow_runtime_handle: Option<tokio::task::JoinHandle<()>> = None;
    let mut shadow_signal_router_handle: Option<tokio::task::JoinHandle<()>> = None;
    let shadow_monitor = if config.execution_mode == "shadow" {
        match config.shadow_ledger.clone() {
            Some(shadow_ledger) => {
                let guardian_config = build_shadow_guardian_config(&config);
                let (signal_tx, signal_rx) =
                    mpsc::channel(guardian_config.signal_channel_buffer.max(1));
                let runtime_router = Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
                    RwLock::new(ShadowPositionBook::new()),
                )));
                let mut monitoring_engine =
                    MonitoringEngine::new(guardian_config, shadow_ledger, signal_tx);
                monitoring_engine.set_shadow_simple_exit_thresholds(
                    config.live_exit_take_profit_pct,
                    config.live_exit_stop_loss_pct,
                );
                if let Some(account_state_core) = config.account_state_core.clone() {
                    monitoring_engine.set_account_state_core(account_state_core);
                }
                monitoring_engine.set_position_router(Arc::clone(&runtime_router));
                monitoring_engine.set_event_emitter(emitter.clone());
                monitoring_engine
                    .set_shadow_lifecycle_log_path(config.shadow_lifecycle_log_path.clone());
                let monitoring_engine = Arc::new(monitoring_engine);
                shadow_signal_router_handle = Some(tokio::spawn(
                    SignalRouter::new(signal_rx, runtime_router).run(),
                ));
                shadow_runtime_handle = Some(Arc::clone(&monitoring_engine).start());
                Some(monitoring_engine)
            }
            None => {
                warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    "PostBuyRuntime: execution_mode=shadow but no ShadowLedger configured; canonical shadow lifecycle handoff disabled"
                );
                None
            }
        }
    } else {
        None
    };
    let mut probe_runtime_handle: Option<tokio::task::JoinHandle<()>> = None;
    let mut probe_signal_router_handle: Option<tokio::task::JoinHandle<()>> = None;
    let probe_monitor = if config.execution_mode == "shadow" {
        match (
            config.shadow_ledger.clone(),
            config.probe_lifecycle_log_path.clone(),
        ) {
            (Some(shadow_ledger), Some(probe_lifecycle_log_path)) => {
                let guardian_config = build_shadow_guardian_config(&config);
                let (signal_tx, signal_rx) =
                    mpsc::channel(guardian_config.signal_channel_buffer.max(1));
                let runtime_router = Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
                    RwLock::new(ShadowPositionBook::new()),
                )));
                let mut monitoring_engine =
                    MonitoringEngine::new(guardian_config, shadow_ledger, signal_tx);
                monitoring_engine.set_shadow_simple_exit_thresholds(
                    config.live_exit_take_profit_pct,
                    config.live_exit_stop_loss_pct,
                );
                if let Some(account_state_core) = config.account_state_core.clone() {
                    monitoring_engine.set_account_state_core(account_state_core);
                }
                monitoring_engine.set_position_router(Arc::clone(&runtime_router));
                monitoring_engine.set_shadow_lifecycle_log_path(Some(probe_lifecycle_log_path));
                let monitoring_engine = Arc::new(monitoring_engine);
                probe_signal_router_handle = Some(tokio::spawn(
                    SignalRouter::new(signal_rx, runtime_router).run(),
                ));
                probe_runtime_handle = Some(Arc::clone(&monitoring_engine).start());
                Some(monitoring_engine)
            }
            (None, Some(_)) => {
                warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    "PostBuyRuntime: p37 probe lifecycle path configured but no ShadowLedger is available; probe lifecycle handoff disabled"
                );
                None
            }
            _ => None,
        }
    } else {
        None
    };

    info!(
        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
        "PostBuyRuntime adapter started (mode={}, run_id={}, live_sell={}, shadow_guardian={}, probe_guardian={})",
        config.execution_mode,
        run_id,
        config.live_sell.is_some(),
        shadow_monitor.is_some(),
        probe_monitor.is_some(),
    );

    let mut epoch_counter: u64 = 1;
    let mut lifecycle_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let mut draining_shutdown = false;
    let mut shutdown_deadline: Option<tokio::time::Instant> = None;
    let mut recent_handoffs = RecentPostBuyCache::default();
    let mut event_bus_closed = false;

    loop {
        if event_bus_closed && direct_handoff_rx.is_none() {
            if shadow_monitor
                .as_ref()
                .is_some_and(|monitor| monitor.active_position_count() > 0)
                || probe_monitor
                    .as_ref()
                    .is_some_and(|monitor| monitor.active_position_count() > 0)
            {
                debug!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    active_shadow_positions = shadow_monitor
                        .as_ref()
                        .map(|monitor| monitor.active_position_count())
                        .unwrap_or(0),
                    active_probe_positions = probe_monitor
                        .as_ref()
                        .map(|monitor| monitor.active_position_count())
                        .unwrap_or(0),
                    "PostBuyRuntime: handoff transports closed but shadow closeout is still active"
                );
            } else {
                info!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    "PostBuyRuntime: all post-buy handoff transports closed"
                );
                break;
            }
        }

        let idle_sleep = if draining_shutdown {
            std::time::Duration::from_millis(50)
        } else {
            std::time::Duration::from_secs(1)
        };

        tokio::select! {
            _ = shutdown_rx.recv(), if !draining_shutdown => {
                draining_shutdown = true;
                shutdown_deadline = Some(
                    tokio::time::Instant::now()
                        + tokio::time::Duration::from_millis(POST_BUY_SHUTDOWN_DRAIN_MS),
                );
                info!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    "PostBuyRuntime received shutdown signal; draining late PostBuySubmitted events for {}ms",
                    POST_BUY_SHUTDOWN_DRAIN_MS,
                );
            }
            event = event_rx.recv(), if !event_bus_closed => {
                match event {
                    Ok(event) => {
                        handle_post_buy_event(
                            event,
                            &config,
                            &lifecycle,
                            shadow_monitor.as_ref(),
                            probe_monitor.as_ref(),
                            &mut epoch_counter,
                            &mut lifecycle_handles,
                            &mut recent_handoffs,
                        )
                        .await;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        crate::events::record_event_bus_lag("post_buy_runtime", n as u64);
                        warn!(
                            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                            direct_handoff_enabled = direct_handoff_rx.is_some(),
                            "PostBuyRuntime: lagged by {} events on broadcast handoff transport",
                            n
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        event_bus_closed = true;
                        warn!(
                            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                            direct_handoff_enabled = direct_handoff_rx.is_some(),
                            "PostBuyRuntime: broadcast handoff transport closed"
                        );
                    }
                }
            }
            direct_event = async {
                match direct_handoff_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => None,
                }
            }, if direct_handoff_rx.is_some() => {
                match direct_event {
                    Some(handoff) => {
                        let (event, ack_tx) = handoff.into_parts();
                        let ack = handle_post_buy_event(
                            event,
                            &config,
                            &lifecycle,
                            shadow_monitor.as_ref(),
                            probe_monitor.as_ref(),
                            &mut epoch_counter,
                            &mut lifecycle_handles,
                            &mut recent_handoffs,
                        )
                        .await;
                        if let Some(ack_tx) = ack_tx {
                            let _ = ack_tx.send(ack);
                        }
                    }
                    None => {
                        warn!(
                            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                            "PostBuyRuntime: direct handoff transport closed"
                        );
                        direct_handoff_rx = None;
                    }
                }
            }
            _ = tokio::time::sleep(idle_sleep) => {
                if draining_shutdown
                    && shutdown_deadline
                        .is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
                {
                    let active_shadow_positions = shadow_monitor
                        .as_ref()
                        .map(|monitor| monitor.active_position_count())
                        .unwrap_or(0);
                    let active_probe_positions = probe_monitor
                        .as_ref()
                        .map(|monitor| monitor.active_position_count())
                        .unwrap_or(0);
                    if active_shadow_positions == 0 && active_probe_positions == 0 {
                        info!(
                            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                            "PostBuyRuntime shutdown drain elapsed; stopping subscriber"
                        );
                        break;
                    }
                    debug!(
                        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                        active_shadow_positions,
                        active_probe_positions,
                        "PostBuyRuntime shutdown drain elapsed but canonical shadow closeout is still active; waiting for shadow lifecycle completion"
                    );
                }
            }
        }
    }

    // Wait for all in-flight lifecycle tasks to complete before flushing.
    if !lifecycle_handles.is_empty() {
        info!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            "PostBuyRuntime: waiting for {} lifecycle task(s) to finish",
            lifecycle_handles.len()
        );
        for handle in lifecycle_handles {
            if let Err(e) = handle.await {
                warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    "PostBuyRuntime: lifecycle task failed: {:?}", e
                );
            }
        }
    }

    if let Some(handle) = shadow_runtime_handle.take() {
        handle.abort();
        let _ = handle.await;
    }
    if let Some(handle) = shadow_signal_router_handle.take() {
        handle.abort();
        let _ = handle.await;
    }
    if let Some(handle) = probe_runtime_handle.take() {
        handle.abort();
        let _ = handle.await;
    }
    if let Some(handle) = probe_signal_router_handle.take() {
        handle.abort();
        let _ = handle.await;
    }

    if let Err(e) = emitter.flush() {
        warn!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            "PostBuyRuntime: flush error on shutdown: {}", e
        );
    }

    info!(
        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
        "PostBuyRuntime exiting"
    );
}

async fn handle_post_buy_event(
    event: GhostEvent,
    config: &PostBuyRuntimeConfig,
    lifecycle: &Arc<PaperPositionLifecycle>,
    shadow_monitor: Option<&Arc<MonitoringEngine>>,
    probe_monitor: Option<&Arc<MonitoringEngine>>,
    epoch_counter: &mut u64,
    lifecycle_handles: &mut Vec<tokio::task::JoinHandle<()>>,
    recent_handoffs: &mut RecentPostBuyCache,
) -> DirectPostBuyHandoffAck {
    let GhostEvent::PostBuySubmitted {
        candidate_id,
        pool_amm_id,
        base_mint,
        signature,
        amount_sol,
        tip_lamports,
        lane,
        epoch_id: _,
        position_slot_id,
        source,
        min_tokens_out,
        entry_token_amount_raw,
        buy_landed_slot,
        creator_pubkey,
        join_metadata,
    } = event
    else {
        return DirectPostBuyHandoffAck::Accepted;
    };

    if let Some(previous_ack) = recent_handoffs.reserve(&candidate_id) {
        ::metrics::counter!("post_buy_runtime_duplicate_handoff_total", 1u64, "lane" => lane.clone());
        debug!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id,
            lane = %lane,
            "PostBuyRuntime: duplicate PostBuySubmitted suppressed"
        );
        return previous_ack;
    }

    let epoch = *epoch_counter;
    *epoch_counter = epoch.saturating_add(1);

    info!(
        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
        pool = %pool_amm_id,
        base_mint = %base_mint,
        lane = %lane,
        source = ?source,
        tip_lamports,
        min_tokens_out = ?min_tokens_out,
        entry_token_amount_raw = ?entry_token_amount_raw,
        buy_landed_slot = ?buy_landed_slot,
        "PostBuyRuntime: received PostBuySubmitted"
    );

    if lane == "live" {
        let position_limit_tracker = config.position_limit_tracker.clone();
        if matches!(source, PostBuySource::Recovery) {
            if let (Some(tracker), Some(slot_id)) = (&position_limit_tracker, position_slot_id) {
                if let Err(error) =
                    tracker.register_existing(slot_id, pool_amm_id.clone(), base_mint.clone())
                {
                    if matches!(
                        error.downcast_ref::<SafetyViolation>(),
                        Some(SafetyViolation::PositionSlotAlreadyActive { .. })
                    ) {
                        info!(
                            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                            candidate_id = %candidate_id,
                            slot_id = %slot_id,
                            "PostBuyRuntime: recovered position slot already active; skipping duplicate registration"
                        );
                        return finish_direct_handoff(
                            recent_handoffs,
                            &candidate_id,
                            DirectPostBuyHandoffAck::Accepted,
                        );
                    }
                    record_live_exit_status(LiveExitStatus::LifecycleAbortedWithReason);
                    record_live_exit_terminal(
                        LiveExitStatus::LifecycleAbortedWithReason,
                        "recovery_slot_register_failed",
                    );
                    warn!(
                        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                        candidate_id = %candidate_id,
                        slot_id = %slot_id,
                        error = %error,
                        "PostBuyRuntime: failed to register recovered position slot"
                    );
                    return finish_direct_handoff(
                        recent_handoffs,
                        &candidate_id,
                        DirectPostBuyHandoffAck::Accepted,
                    );
                }
            }
        }
        let pool_pubkey = match Pubkey::from_str(&pool_amm_id) {
            Ok(pubkey) => pubkey,
            Err(error) => {
                record_live_exit_status(LiveExitStatus::LifecycleAbortedWithReason);
                record_live_exit_terminal(
                    LiveExitStatus::LifecycleAbortedWithReason,
                    "invalid_pool_pubkey",
                );
                warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    candidate_id = %candidate_id,
                    pool_amm_id = %pool_amm_id,
                    error = %error,
                    "PostBuyRuntime: invalid live pool_amm_id pubkey — aborting lifecycle"
                );
                retain_live_slot(
                    position_slot_id,
                    LiveExitStatus::LifecycleAbortedWithReason,
                    Some("invalid_pool_pubkey"),
                );
                return finish_direct_handoff(
                    recent_handoffs,
                    &candidate_id,
                    DirectPostBuyHandoffAck::Accepted,
                );
            }
        };
        let mint_pubkey = match Pubkey::from_str(&base_mint) {
            Ok(pubkey) => pubkey,
            Err(error) => {
                record_live_exit_status(LiveExitStatus::LifecycleAbortedWithReason);
                record_live_exit_terminal(
                    LiveExitStatus::LifecycleAbortedWithReason,
                    "invalid_base_mint_pubkey",
                );
                warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    candidate_id = %candidate_id,
                    base_mint = %base_mint,
                    error = %error,
                    "PostBuyRuntime: invalid live base_mint pubkey — aborting lifecycle"
                );
                retain_live_slot(
                    position_slot_id,
                    LiveExitStatus::LifecycleAbortedWithReason,
                    Some("invalid_base_mint_pubkey"),
                );
                return finish_direct_handoff(
                    recent_handoffs,
                    &candidate_id,
                    DirectPostBuyHandoffAck::Accepted,
                );
            }
        };
        if let Some(live) = config.live_sell.clone() {
            let sell_slippage_bps = config.live_exit_slippage_bps();
            let live_position_registry = config.live_position_registry.clone();
            let live_config = config.clone();
            let creator_pubkey = creator_pubkey
                .as_deref()
                .and_then(|value| Pubkey::from_str(value).ok());
            let session = LiveExitSession::new(
                candidate_id.clone(),
                pool_pubkey,
                mint_pubkey,
                creator_pubkey,
                signature,
                buy_landed_slot,
                tip_lamports,
                position_slot_id,
            );
            let handle = tokio::spawn(async move {
                run_live_sell_lifecycle(
                    live,
                    session,
                    live_config,
                    position_limit_tracker,
                    sell_slippage_bps,
                    live_position_registry,
                )
                .await;
            });
            lifecycle_handles.push(handle);
            return finish_direct_handoff(
                recent_handoffs,
                &candidate_id,
                DirectPostBuyHandoffAck::Accepted,
            );
        }

        record_live_exit_status(LiveExitStatus::LifecycleAbortedWithReason);
        record_live_exit_terminal(
            LiveExitStatus::LifecycleAbortedWithReason,
            "live_handle_missing",
        );
        warn!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            base_mint = %base_mint,
            "PostBuyRuntime: live lane but no LiveSellHandle configured — refusing paper fallback and keeping slot reserved"
        );
        retain_live_slot(
            position_slot_id,
            LiveExitStatus::LifecycleAbortedWithReason,
            Some("live_handle_missing"),
        );
        return finish_direct_handoff(
            recent_handoffs,
            &candidate_id,
            DirectPostBuyHandoffAck::Accepted,
        );
    }

    if lane == "shadow" {
        let handoff = handle_shadow_post_buy_handoff(
            shadow_monitor,
            &candidate_id,
            &pool_amm_id,
            &base_mint,
            amount_sol,
            entry_token_amount_raw,
            buy_landed_slot,
            epoch,
            PositionJoinMetadata {
                ab_record_id: join_metadata.ab_record_id.clone(),
                source_ab_record_id: join_metadata.source_ab_record_id.clone(),
                probe_id: join_metadata.probe_id.clone(),
                dispatch_source: join_metadata.dispatch_source.clone(),
                collection_plane: join_metadata.collection_plane.clone(),
                probe_plane: join_metadata.probe_plane.clone(),
                v3_feature_snapshot_hash: join_metadata.v3_feature_snapshot_hash.clone(),
                v3_policy_config_hash: join_metadata.v3_policy_config_hash.clone(),
                decision_plane: join_metadata.decision_plane.clone(),
                rollout_namespace: join_metadata.rollout_namespace.clone(),
            },
        )
        .await;
        if matches!(handoff.ack, DirectPostBuyHandoffAck::Accepted) {
            if let (Some(tracker), Some(slot_id), Some(mint_pubkey), Some(shadow_monitor)) = (
                config.position_limit_tracker.clone(),
                position_slot_id,
                handoff.mint_pubkey,
                shadow_monitor.cloned(),
            ) {
                lifecycle_handles.push(spawn_shadow_slot_release_watcher(
                    shadow_monitor,
                    tracker,
                    slot_id,
                    mint_pubkey,
                    candidate_id.clone(),
                    config.tick_interval_ms.max(50),
                ));
            }
        }
        return finish_direct_handoff(recent_handoffs, &candidate_id, handoff.ack);
    }

    if lane == "probe" {
        info!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id = %candidate_id,
            probe_id = ?join_metadata.probe_id,
            "PostBuyRuntime: probe lifecycle monitor requested"
        );
        let handoff = handle_shadow_post_buy_handoff(
            probe_monitor,
            &candidate_id,
            &pool_amm_id,
            &base_mint,
            amount_sol,
            entry_token_amount_raw,
            buy_landed_slot,
            epoch,
            PositionJoinMetadata {
                ab_record_id: join_metadata.ab_record_id.clone(),
                source_ab_record_id: join_metadata.source_ab_record_id.clone(),
                probe_id: join_metadata.probe_id.clone(),
                dispatch_source: join_metadata.dispatch_source.clone(),
                collection_plane: join_metadata.collection_plane.clone(),
                probe_plane: join_metadata.probe_plane.clone(),
                v3_feature_snapshot_hash: join_metadata.v3_feature_snapshot_hash.clone(),
                v3_policy_config_hash: join_metadata.v3_policy_config_hash.clone(),
                decision_plane: join_metadata.decision_plane.clone(),
                rollout_namespace: join_metadata.rollout_namespace.clone(),
            },
        )
        .await;
        match handoff.ack {
            DirectPostBuyHandoffAck::Accepted => {
                info!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    candidate_id = %candidate_id,
                    probe_id = ?join_metadata.probe_id,
                    "PostBuyRuntime: probe lifecycle monitor started"
                );
            }
            DirectPostBuyHandoffAck::Rejected(reason) => {
                warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    candidate_id = %candidate_id,
                    probe_id = ?join_metadata.probe_id,
                    probe_lifecycle_skip_reason = reason,
                    "PostBuyRuntime: probe lifecycle monitor skipped"
                );
            }
        }
        return finish_direct_handoff(recent_handoffs, &candidate_id, handoff.ack);
    }

    let pool_pubkey = Pubkey::from_str(&pool_amm_id).unwrap_or_else(|_| {
        debug!(
            "PostBuyRuntime: pool_amm_id '{}' is not a valid Pubkey, using fallback",
            pool_amm_id
        );
        Pubkey::new_unique()
    });
    let mint_pubkey = Pubkey::from_str(&base_mint).unwrap_or_else(|_| {
        debug!(
            "PostBuyRuntime: base_mint '{}' is not a valid Pubkey, using fallback",
            base_mint
        );
        Pubkey::new_unique()
    });
    let entry_price = if amount_sol > 0.0 { amount_sol } else { 0.001 };
    let amount_lamports = (amount_sol * 1_000_000_000.0) as u64;

    let candidate_ref = CandidateRef {
        candidate_id: candidate_id.clone(),
        base_mint: mint_pubkey,
        pool_amm_id: pool_pubkey,
        entry_amount_lamports: amount_lamports,
        min_tokens_out: 1,
    };

    let lifecycle_clone = lifecycle.clone();
    let position_limit_tracker = config.position_limit_tracker.clone();
    let handle = tokio::spawn(async move {
        lifecycle_clone.run(candidate_ref, epoch, entry_price).await;
        if let (Some(tracker), Some(slot_id)) = (position_limit_tracker, position_slot_id) {
            if !tracker.release(slot_id) {
                warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    slot_id = %slot_id,
                    "PostBuyRuntime: position slot already released before lifecycle completion"
                );
            }
        }
    });
    lifecycle_handles.push(handle);
    finish_direct_handoff(
        recent_handoffs,
        &candidate_id,
        DirectPostBuyHandoffAck::Accepted,
    )
}

struct ShadowPostBuyHandoffResult {
    ack: DirectPostBuyHandoffAck,
    mint_pubkey: Option<Pubkey>,
}

async fn handle_shadow_post_buy_handoff(
    shadow_monitor: Option<&Arc<MonitoringEngine>>,
    candidate_id: &str,
    pool_amm_id: &str,
    base_mint: &str,
    amount_sol: f64,
    entry_token_amount_raw: Option<u64>,
    buy_landed_slot: Option<u64>,
    epoch: u64,
    join_metadata: PositionJoinMetadata,
) -> ShadowPostBuyHandoffResult {
    let Some(shadow_monitor) = shadow_monitor else {
        warn!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id,
            "PostBuyRuntime: shadow handoff received but canonical shadow guardian is not running"
        );
        return ShadowPostBuyHandoffResult {
            ack: DirectPostBuyHandoffAck::Rejected("guardian_unavailable"),
            mint_pubkey: None,
        };
    };

    let pool_pubkey = match Pubkey::from_str(pool_amm_id) {
        Ok(pubkey) => pubkey,
        Err(error) => {
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id,
                pool_amm_id,
                error = %error,
                "PostBuyRuntime: invalid shadow pool_amm_id pubkey — refusing canonical shadow handoff"
            );
            return ShadowPostBuyHandoffResult {
                ack: DirectPostBuyHandoffAck::Rejected("invalid_pool_pubkey"),
                mint_pubkey: None,
            };
        }
    };
    let mint_pubkey = match Pubkey::from_str(base_mint) {
        Ok(pubkey) => pubkey,
        Err(error) => {
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id,
                base_mint,
                error = %error,
                "PostBuyRuntime: invalid shadow base_mint pubkey — refusing canonical shadow handoff"
            );
            return ShadowPostBuyHandoffResult {
                ack: DirectPostBuyHandoffAck::Rejected("invalid_base_mint_pubkey"),
                mint_pubkey: None,
            };
        }
    };
    let Some(entry_price) = shadow_entry_price_from_post_buy(amount_sol, entry_token_amount_raw)
    else {
        warn!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id,
            amount_sol,
            entry_token_amount_raw = ?entry_token_amount_raw,
            "PostBuyRuntime: shadow handoff missing canonical entry price inputs — refusing synthetic position registration"
        );
        return ShadowPostBuyHandoffResult {
            ack: DirectPostBuyHandoffAck::Rejected("missing_entry_price"),
            mint_pubkey: None,
        };
    };
    let entry_amount_lamports = if amount_sol.is_finite() && amount_sol > 0.0 {
        (amount_sol * LAMPORTS_PER_SOL as f64).round() as u64
    } else {
        0
    };
    let probe_position_id = join_metadata
        .probe_id
        .as_ref()
        .filter(|_| join_metadata.dispatch_source.as_deref() == Some("counterfactual_shadow_probe"))
        .map(|probe_id| format!("probe-position:{probe_id}"));
    let entry_order_id = if probe_position_id.is_some() {
        format!("probe-entry-{candidate_id}")
    } else {
        format!("shadow-entry-{candidate_id}")
    };
    let quote_id = if probe_position_id.is_some() {
        format!("probe-quote-{candidate_id}")
    } else {
        format!("shadow-quote-{candidate_id}")
    };
    let canonical_ready = shadow_monitor
        .wait_for_canonical_snapshot(
            &mint_pubkey,
            buy_landed_slot,
            Duration::from_millis(SHADOW_CANONICAL_HANDOFF_WAIT_MS),
            Duration::from_millis(SHADOW_CANONICAL_HANDOFF_POLL_MS),
        )
        .await;
    if !canonical_ready {
        warn!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id,
            base_mint,
            buy_landed_slot = ?buy_landed_slot,
            wait_ms = SHADOW_CANONICAL_HANDOFF_WAIT_MS,
            "PostBuyRuntime: shadow handoff timed out waiting for canonical post-buy snapshot; proceeding fail-closed if guardian still cannot seed truth"
        );
    }
    let context = PositionEventContext {
        join_metadata,
        candidate_id: candidate_id.to_string(),
        entry_order_id,
        quote_id,
        slot: buy_landed_slot,
        lane: Lane::Shadow,
        position_id: probe_position_id,
        position_epoch: Some(epoch),
    };
    let registered = shadow_monitor.register_position_with_context(
        pool_pubkey,
        mint_pubkey,
        pool_pubkey,
        Some(entry_price),
        Some(entry_amount_lamports),
        entry_token_amount_raw,
        Some(context),
    );
    match registered {
        Some(registered) => {
            info!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id,
                position_id = %registered.position_id,
                position_epoch = registered.position_epoch,
                entry_price,
                "PostBuyRuntime: canonical shadow position handed off to MonitoringEngine"
            );
            ShadowPostBuyHandoffResult {
                ack: DirectPostBuyHandoffAck::Accepted,
                mint_pubkey: Some(mint_pubkey),
            }
        }
        None => {
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id,
                "PostBuyRuntime: canonical shadow handoff was rejected by MonitoringEngine"
            );
            ShadowPostBuyHandoffResult {
                ack: DirectPostBuyHandoffAck::Rejected("monitoring_rejected"),
                mint_pubkey: None,
            }
        }
    }
}

fn spawn_shadow_slot_release_watcher(
    shadow_monitor: Arc<MonitoringEngine>,
    position_limit_tracker: PositionLimitTracker,
    slot_id: PositionSlotId,
    mint_pubkey: Pubkey,
    candidate_id: String,
    poll_interval_ms: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let poll_interval = Duration::from_millis(poll_interval_ms.max(50));
        loop {
            if shadow_monitor.get_position_health(&mint_pubkey).is_none() {
                if !position_limit_tracker.release(slot_id) {
                    warn!(
                        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                        candidate_id = %candidate_id,
                        slot_id = %slot_id,
                        "PostBuyRuntime: shadow position slot already released before lifecycle close watcher fired"
                    );
                }
                break;
            }
            tokio::time::sleep(poll_interval).await;
        }
    })
}

// ─── Live sell lifecycle ─────────────────────────────────────────────────────

/// Full lifecycle for a live on-chain position:
/// 1. Persist confirmed BUY metadata and real entry price from transaction metadata.
/// 2. Poll canonical price from `AccountStateCore` and use read-only RPC point
///    queries only when canonical state is unavailable.
/// 3. Trigger a single full 100% SELL at +2% or -2%.
/// 4. Submit and confirm that exit only via Helius Sender transport.
/// 5. Release the bulkhead slot only after an explicit terminal outcome proves the
///    live position is closed on-chain; otherwise keep the slot reserved fail-closed.
///
/// # Architectural note — MonitoringEngine
///
/// This function is the **SSOT for live position exit** in the launcher.
/// ghost-brain's `MonitoringEngine` / `Guardian` pipeline is explicitly **not used** for the
/// live lane. The launcher owns the Sender-only exit path and the bulkhead; there is no
/// ghost-brain guardian session started or registered here.
///
/// Rationale: MonitoringEngine lives in the ghost-brain analytics domain and is designed for
/// paper-mode AEM telemetry, not for direct on-chain submission. Wiring a live position through
/// MonitoringEngine would couple the hot sell path to the analytics runtime and introduce latency.
///
/// See ADR-0050 (docs/ADR/ADR-0050-live-sell-ssot-launcher-no-monitoring-engine.md).
async fn initialize_live_exit_session(
    live: &LiveSellHandle,
    session: &mut LiveExitSession,
    live_position_registry: Option<&LivePositionRegistry>,
    config: &PostBuyRuntimeConfig,
) -> LiveExitResult {
    session.transition(LiveExitStatus::EntryPricePending);

    let buy_signature = Signature::from_str(&session.buy_signature).map_err(|error| {
        (
            LiveExitStatus::LifecycleAbortedWithReason,
            format!("invalid_buy_signature: {error}"),
        )
    })?;

    let entry_info = EntryPriceExtractor::new(Arc::clone(&live.rpc_client))
        .extract_with_retry(
            &buy_signature,
            &live.payer.pubkey(),
            &session.base_mint,
            LIVE_EXIT_ENTRY_PRICE_MAX_RETRIES,
        )
        .await
        .map_err(|error| (LiveExitStatus::EntryPriceFailed, error.to_string()))?;

    session
        .populate_entry_price(&entry_info, config)
        .map_err(|error| (LiveExitStatus::EntryPriceFailed, error))?;
    if let Some(wallet_position) = query_best_effort_live_wallet_position(live, session).await {
        session.apply_visible_wallet_position(wallet_position);
    }
    if session.token_program.is_none() {
        let wallet_position = resolve_live_exit_wallet_position_with_retry(live, session)
            .await
            .map_err(|error| (LiveExitStatus::LifecycleAbortedWithReason, error))?;
        session.apply_visible_wallet_position(wallet_position);
    }
    record_live_exit_snapshot_metrics(session, "entry_metadata", true);
    log_live_exit_snapshot(session, "entry_metadata", true);
    if let Some(registry) = live_position_registry {
        registry
            .record_open(
                RecoveryTrackedPosition {
                    base_mint: session.base_mint.to_string(),
                    pool_amm_id: session.pool_amm_id.to_string(),
                    buy_signature: session.buy_signature.clone(),
                    creator_pubkey: session.creator_pubkey.map(|pubkey| pubkey.to_string()),
                    buy_landed_slot: session.buy_landed_slot,
                    token_account: session.token_account.map(|pubkey| pubkey.to_string()),
                    token_amount: session
                        .visible_token_balance
                        .or(session.token_balance_after_buy)
                        .or(session.tokens_received),
                },
                now_ms(),
            )
            .await
            .map_err(|error| {
                (
                    LiveExitStatus::LifecycleAbortedWithReason,
                    format!("live_position_registry_open_failed: {error}"),
                )
            })?;
    }
    session.transition(LiveExitStatus::Armed);
    session.transition(LiveExitStatus::Monitoring);

    Ok(())
}

async fn resolve_live_exit_wallet_position_with_retry(
    live: &LiveSellHandle,
    session: &LiveExitSession,
) -> std::result::Result<LiveWalletPosition, String> {
    let owner = live.payer.pubkey();
    let position = query_live_wallet_position_with_retry(live, session)
        .await
        .ok_or_else(|| {
            format!(
                "resolve_wallet_position_failed: owner={} mint={} token_account={:?} retries={}",
                owner, session.base_mint, session.token_account, LIVE_SELL_ATA_LOOKUP_MAX_RETRIES
            )
        })?;

    info!(
        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
        owner = %owner,
        mint = %session.base_mint,
        token_account = %position.token_account,
        token_program = %position.token_program,
        token_amount = position.token_amount,
        "LiveExit: resolved visible wallet position for SELL"
    );

    Ok(position)
}

async fn build_full_exit_transaction_with_retry(
    live: &LiveSellHandle,
    session: &LiveExitSession,
    current_price: u64,
    sell_slippage_bps: u16,
) -> std::result::Result<BuiltLiveExitTransaction, String> {
    let sellable_token_amount = session
        .sellable_token_amount()
        .ok_or_else(|| "sellable_token_amount_missing".to_string())?;
    if sellable_token_amount == 0 {
        return Err("sellable_token_amount_zero".to_string());
    }
    let token_program = session
        .token_program
        .ok_or_else(|| "token_program_missing".to_string())?;
    let curve_hints = read_live_curve_execution_hints(&live.rpc_client, &session.base_mint).await?;
    let raw_min_output = SellTxBuilder::calculate_min_output(
        sellable_token_amount,
        current_price,
        sell_slippage_bps,
    )
    .map_err(|error| format!("min_output_calculation_failed: {error}"))?
    .max(1);
    let min_output = cap_live_exit_min_output(raw_min_output, curve_hints.real_sol_reserves).max(1);
    if min_output < raw_min_output {
        info!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id = %session.candidate_id,
            base_mint = %session.base_mint,
            raw_min_output,
            capped_min_output = min_output,
            real_sol_reserves = ?curve_hints.real_sol_reserves,
            "LiveExit: capped SELL min_output to live real SOL reserves"
        );
    }
    let tip_lamports = live
        .live_tx_sender
        .raise_tip_to_dynamic_floor(resolve_live_exit_tip_lamports(session.tip_lamports))
        .await;
    let fee_recipient = session
        .fee_recipient
        .ok_or_else(|| "pump_fee_recipient_missing".to_string())?;
    let mut sell_config = SellTxConfig::default();
    sell_config.pump_fee_recipient = fee_recipient;
    let sell_builder = SellTxBuilder::new(live.payer.insecure_clone(), sell_config);
    let previous_blockhash = session.last_exit_recent_blockhash;
    let mut last_error = None;

    for attempt in 1..=LIVE_EXIT_BUILD_MAX_RETRIES {
        let blockhash_started_at = Instant::now();
        let (blockhash, blockhash_fetch_latency_ms, blockhash_fetched_at) = match live
            .rpc_client
            .get_latest_blockhash_with_commitment(CommitmentConfig::confirmed())
            .await
        {
            Ok((blockhash, _)) => {
                let blockhash_fetch_latency_ms = saturating_elapsed_ms(blockhash_started_at);
                record_live_sell_rpc_latency(
                    "live_exit_get_latest_blockhash",
                    blockhash_fetch_latency_ms,
                    "ok",
                );
                (blockhash, blockhash_fetch_latency_ms, Instant::now())
            }
            Err(error) => {
                record_live_sell_rpc_latency(
                    "live_exit_get_latest_blockhash",
                    saturating_elapsed_ms(blockhash_started_at),
                    "error",
                );
                last_error = Some(format!("get_latest_blockhash_failed: {error}"));
                if attempt < LIVE_EXIT_BUILD_MAX_RETRIES {
                    tokio::time::sleep(Duration::from_millis(LIVE_EXIT_BUILD_RETRY_MS)).await;
                }
                continue;
            }
        };

        if previous_blockhash == Some(blockhash) {
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id = %session.candidate_id,
                base_mint = %session.base_mint,
                previous_blockhash = ?previous_blockhash,
                "LiveExit: refusing to reuse recent_blockhash for SELL retry; waiting for a fresh blockhash"
            );
            last_error = Some(format!(
                "fresh_exit_blockhash_unavailable: reused_previous_blockhash={blockhash}"
            ));
            if attempt < LIVE_EXIT_BUILD_MAX_RETRIES {
                tokio::time::sleep(Duration::from_millis(LIVE_EXIT_BUILD_RETRY_MS)).await;
            }
            continue;
        }

        let tip_seed = format!(
            "{}:{}:{}",
            session.base_mint, session.buy_signature, blockhash
        );
        let tip_account = live.live_tx_sender.select_tip_account(tip_seed.as_bytes());
        let provisional_tx_bytes = match sell_builder
            .build_signed_sell_tx_with_token_program_and_priority_tip(
                session.base_mint,
                session.creator_pubkey,
                sellable_token_amount,
                min_output,
                blockhash,
                AmmProtocol::PumpFun,
                token_program,
                curve_hints.cashback_enabled,
                HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS,
                Some((tip_account, tip_lamports)),
            )
            .await
        {
            Ok(tx_bytes) => tx_bytes,
            Err(error) => {
                last_error = Some(format!("build_signed_sell_tx_failed: {error}"));
                if attempt < LIVE_EXIT_BUILD_MAX_RETRIES {
                    tokio::time::sleep(Duration::from_millis(LIVE_EXIT_BUILD_RETRY_MS)).await;
                }
                continue;
            }
        };

        let provisional_transaction =
            match bincode::deserialize::<VersionedTransaction>(&provisional_tx_bytes) {
                Ok(transaction) => transaction,
                Err(error) => {
                    last_error = Some(format!("deserialize_full_exit_tx_failed: {error}"));
                    if attempt < LIVE_EXIT_BUILD_MAX_RETRIES {
                        tokio::time::sleep(Duration::from_millis(LIVE_EXIT_BUILD_RETRY_MS)).await;
                    }
                    continue;
                }
            };
        let priority_fee_micro_lamports = live
            .live_tx_sender
            .estimate_priority_fee_micro_lamports(&provisional_transaction)
            .await;
        let transaction = if priority_fee_micro_lamports
            == HELIUS_PRIORITY_FEE_FALLBACK_MICRO_LAMPORTS
        {
            provisional_transaction
        } else {
            let rebuilt_tx_bytes = match sell_builder
                .build_signed_sell_tx_with_token_program_and_priority_tip(
                    session.base_mint,
                    session.creator_pubkey,
                    sellable_token_amount,
                    min_output,
                    blockhash,
                    AmmProtocol::PumpFun,
                    token_program,
                    curve_hints.cashback_enabled,
                    priority_fee_micro_lamports,
                    Some((tip_account, tip_lamports)),
                )
                .await
            {
                Ok(tx_bytes) => tx_bytes,
                Err(error) => {
                    last_error = Some(format!("rebuild_signed_sell_tx_failed: {error}"));
                    if attempt < LIVE_EXIT_BUILD_MAX_RETRIES {
                        tokio::time::sleep(Duration::from_millis(LIVE_EXIT_BUILD_RETRY_MS)).await;
                    }
                    continue;
                }
            };
            match bincode::deserialize::<VersionedTransaction>(&rebuilt_tx_bytes) {
                Ok(transaction) => transaction,
                Err(error) => {
                    last_error = Some(format!("deserialize_rebuilt_full_exit_tx_failed: {error}"));
                    if attempt < LIVE_EXIT_BUILD_MAX_RETRIES {
                        tokio::time::sleep(Duration::from_millis(LIVE_EXIT_BUILD_RETRY_MS)).await;
                    }
                    continue;
                }
            }
        };

        if let Some(exit_signature) = transaction.signatures.first() {
            info!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id = %session.candidate_id,
                base_mint = %session.base_mint,
                exit_signature = %exit_signature,
                token_program = %token_program,
                sellable_token_amount,
                sellable_token_amount_ui =
                    raw_token_amount_to_ui(sellable_token_amount, session.token_decimals.unwrap_or(6)),
                min_output,
                cashback_enabled = curve_hints.cashback_enabled,
                real_sol_reserves = ?curve_hints.real_sol_reserves,
                tip_lamports,
                priority_fee_micro_lamports,
                current_price_lamports_per_token = current_price,
                "LiveExit: built full exit transaction"
            );
        }
        return Ok(BuiltLiveExitTransaction {
            transaction,
            blockhash_fetched_at,
            blockhash_fetch_latency_ms,
            tip_lamports,
            priority_fee_micro_lamports,
        });
    }

    Err(last_error.unwrap_or_else(|| "full_exit_build_failed".to_string()))
}

async fn submit_live_exit_transaction(
    live: &LiveSellHandle,
    session: &mut LiveExitSession,
    built_transaction: BuiltLiveExitTransaction,
    trigger: LiveExitTrigger,
    attempt_number: usize,
) -> LiveExitResult {
    let BuiltLiveExitTransaction {
        transaction,
        blockhash_fetched_at,
        blockhash_fetch_latency_ms,
        tip_lamports,
        priority_fee_micro_lamports,
    } = built_transaction;
    // Pre-flight simulation for diagnostics (non-aborting — SELL must proceed regardless).
    if let Err(sim_err) = live.rpc_client.simulate_transaction(&transaction).await {
        warn!(
            base_mint = %session.base_mint,
            error = %sim_err,
            "SELL pre-flight simulation FAILED (proceeding anyway)"
        );
    } else {
        info!(base_mint = %session.base_mint, "SELL pre-flight simulation passed");
    }

    let recent_blockhash = match &transaction.message {
        solana_sdk::message::VersionedMessage::Legacy(message) => message.recent_blockhash,
        solana_sdk::message::VersionedMessage::V0(message) => message.recent_blockhash,
    };
    session.last_exit_recent_blockhash = Some(recent_blockhash);
    session.last_exit_blockhash_fetched_at = Some(blockhash_fetched_at);
    session.last_exit_blockhash_fetch_latency_ms = Some(blockhash_fetch_latency_ms);
    let submit_slot = live.rpc_client.get_slot().await.ok();
    session.last_exit_submit_slot = submit_slot;
    let blockhash_to_send_transaction_ms = saturating_elapsed_ms(blockhash_fetched_at);
    metrics::histogram!(
        "live_exit_blockhash_fetch_latency_ms",
        blockhash_fetch_latency_ms as f64
    );
    metrics::histogram!(
        "live_exit_blockhash_to_send_transaction_ms",
        blockhash_to_send_transaction_ms as f64
    );
    info!(
        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
        candidate_id = %session.candidate_id,
        pool_amm_id = %session.pool_amm_id,
        base_mint = %session.base_mint,
        buy_signature = %session.buy_signature,
        trigger = trigger.as_label(),
        recent_blockhash = %recent_blockhash,
        blockhash_fetch_latency_ms,
        blockhash_to_send_transaction_ms,
        tip_lamports,
        priority_fee_micro_lamports,
        submit_slot = ?submit_slot,
        "LiveExit: SELL blockhash timing before Sender submit"
    );
    let submit_started_at = Instant::now();
    let expected_signature = transaction.signatures.first().copied().ok_or((
        LiveExitStatus::ExitSubmitFailed,
        "signed SELL transaction did not contain a payer signature".to_string(),
    ))?;
    let summary_candidate_id = session.candidate_id.clone();
    let summary_pool_amm_id = session.pool_amm_id;
    let summary_base_mint = session.base_mint;
    let summary_buy_signature = session.buy_signature.clone();

    let log_live_sell_attempt_summary =
        |result: &str,
         confirm_source: Option<&str>,
         status: LiveExitStatus,
         detail: Option<&str>| {
            let next_action = if result == "confirmed" {
                "stop"
            } else if is_retryable_live_exit_failure(status)
                && attempt_number < (LIVE_EXIT_EXECUTION_MAX_RETRIES as usize + 1)
            {
                "retry"
            } else {
                "stop"
            };
            let sell_summary = format!(
                "attempt={attempt_number} result={result} next_action={next_action} trigger={} confirm_source={} exit_signature={} tip_lamports={} priority_fee_micro_lamports={} recent_blockhash={} blockhash_fetch_latency_ms={} blockhash_to_send_transaction_ms={}",
                trigger.as_label(),
                confirm_source.unwrap_or("none"),
                expected_signature,
                tip_lamports,
                priority_fee_micro_lamports,
                recent_blockhash,
                blockhash_fetch_latency_ms,
                blockhash_to_send_transaction_ms,
            );
            match result {
                "confirmed" => info!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    candidate_id = %summary_candidate_id,
                    pool_amm_id = %summary_pool_amm_id,
                    base_mint = %summary_base_mint,
                    buy_signature = %summary_buy_signature,
                    trigger = trigger.as_label(),
                    attempt_number,
                    confirm_source = confirm_source.unwrap_or("none"),
                    next_action,
                    detail = detail.unwrap_or(""),
                    sell_summary = %sell_summary,
                    "LiveExit: SELL attempt summary"
                ),
                _ => warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    candidate_id = %summary_candidate_id,
                    pool_amm_id = %summary_pool_amm_id,
                    base_mint = %summary_base_mint,
                    buy_signature = %summary_buy_signature,
                    trigger = trigger.as_label(),
                    attempt_number,
                    confirm_source = confirm_source.unwrap_or("none"),
                    next_action,
                    detail = detail.unwrap_or(""),
                    sell_summary = %sell_summary,
                    "LiveExit: SELL attempt summary"
                ),
            }
        };

    let submission = live
        .live_tx_sender
        .send_transaction(&transaction)
        .await
        .map_err(|error| {
            record_live_sell_transport_latency(
                "send_transaction",
                "helius_sender",
                saturating_elapsed_ms(submit_started_at),
                "error",
            );
            log_live_sell_attempt_summary(
                "submit_failed",
                None,
                LiveExitStatus::ExitSubmitFailed,
                Some(&error.to_string()),
            );
            (LiveExitStatus::ExitSubmitFailed, error.to_string())
        })?;
    if submission.signature != expected_signature {
        let detail = format!(
            "Helius Sender SELL returned signature mismatch: signed={} returned={}",
            expected_signature, submission.signature
        );
        log_live_sell_attempt_summary(
            "submit_failed",
            None,
            LiveExitStatus::ExitSubmitFailed,
            Some(&detail),
        );
        return Err((LiveExitStatus::ExitSubmitFailed, detail));
    }
    let submit_latency_ms = saturating_elapsed_ms(submit_started_at);
    record_live_sell_transport_latency(
        "send_transaction",
        "helius_sender",
        submit_latency_ms,
        "ok",
    );
    session.mark_exit_submitted(&submission);
    info!(
        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
        candidate_id = %session.candidate_id,
        pool_amm_id = %session.pool_amm_id,
        base_mint = %session.base_mint,
        buy_signature = %session.buy_signature,
        exit_signature = %submission.signature,
        attempt_number,
        tip_lamports,
        priority_fee_micro_lamports,
        recent_blockhash = %recent_blockhash,
        "LiveExit: SELL submitted via Helius Sender"
    );

    let confirm_started_at = Instant::now();
    let confirm_result = confirm_sender_sell_attempt(
        live,
        session.candidate_id.clone(),
        session.base_mint,
        session.token_account,
        session.sellable_token_amount().unwrap_or_default(),
        &submission,
    )
    .await;

    let finalize_confirmed_exit = |session: &mut LiveExitSession,
                                   confirmed: SenderConfirmedTransaction,
                                   source: &'static str| {
        let confirm_latency_ms = saturating_elapsed_ms(confirm_started_at);
        record_live_sell_transport_latency("confirm_submission", source, confirm_latency_ms, "ok");
        let submit_to_landed_slot_delta = confirmed
            .landed_slot
            .zip(submit_slot)
            .map(|(landed_slot, submit_slot)| landed_slot.saturating_sub(submit_slot));
        let near_leader_slot = submit_to_landed_slot_delta.map(|delta| delta <= 1);
        if source == "balance_zero" || source == "wallet_absent" {
            session.visible_token_balance = Some(0);
        }
        session.mark_exit_confirmed(&confirmed, trigger);
        info!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id = %session.candidate_id,
            pool_amm_id = %session.pool_amm_id,
            base_mint = %session.base_mint,
            buy_signature = %session.buy_signature,
            exit_signature = %confirmed.signature,
            attempt_number,
            submit_slot = ?submit_slot,
            landed_slot = ?confirmed.landed_slot,
            submit_to_landed_slot_delta = ?submit_to_landed_slot_delta,
            near_leader_slot = ?near_leader_slot,
            confirm_source = source,
            submit_transport_latency_ms = submit_latency_ms,
            confirm_transport_latency_ms = confirm_latency_ms,
            tip_lamports,
            priority_fee_micro_lamports,
            "LiveExit: SELL sender telemetry"
        );
    };

    match confirm_result {
        SenderSellAttemptConfirmation::Confirmed {
            source,
            landed_slot,
        } => {
            let confirmed = SenderConfirmedTransaction {
                signature: submission.signature,
                landed_slot,
            };
            finalize_confirmed_exit(session, confirmed.clone(), source);
            log_realized_exit_price_after_confirmation(live, session, &confirmed, trigger).await;
            log_live_sell_attempt_summary(
                "confirmed",
                Some(source),
                LiveExitStatus::ExitConfirmed,
                None,
            );
            Ok(())
        }
        SenderSellAttemptConfirmation::Failed { source, detail } => {
            record_live_sell_transport_latency(
                "confirm_submission",
                source,
                saturating_elapsed_ms(confirm_started_at),
                "error",
            );
            log_live_sell_attempt_summary(
                "failed",
                Some(source),
                LiveExitStatus::ExitConfirmFailed,
                Some(&detail),
            );
            Err((
                LiveExitStatus::ExitConfirmFailed,
                format!(
                    "Helius Sender SELL confirmation failed after signature {} via {}: {}",
                    submission.signature, source, detail
                ),
            ))
        }
        SenderSellAttemptConfirmation::Uncertain => {
            record_live_sell_transport_latency(
                "confirm_submission",
                "none",
                saturating_elapsed_ms(confirm_started_at),
                "error",
            );
            let detail = format!(
                "Helius Sender SELL confirmation remained inconclusive after signature {}",
                submission.signature
            );
            log_live_sell_attempt_summary(
                "uncertain",
                None,
                LiveExitStatus::ExitConfirmFailed,
                Some(&detail),
            );
            Err((LiveExitStatus::ExitConfirmFailed, detail))
        }
    }
}

async fn monitor_live_exit_session(
    live: &LiveSellHandle,
    session: &mut LiveExitSession,
    sell_slippage_bps: u16,
) -> LiveExitResult {
    let poll_interval = Duration::from_millis(LIVE_SELL_POLL_MS);
    let snapshot_interval = Duration::from_secs(1);
    let mut last_snapshot_at = Instant::now()
        .checked_sub(snapshot_interval)
        .unwrap_or_else(Instant::now);
    let mut unavailable_polls = 0u32;
    let mut execution_retry_count = 0u32;

    loop {
        let price_sample = read_live_price_sample(live, &session.base_mint).await;

        if let Some(price_sample) = price_sample {
            unavailable_polls = 0;
            record_post_buy_shadow_compare(
                &session.base_mint,
                price_sample.source,
                price_sample.price,
                read_shadow_price_for_compare(&live.shadow_ledger, &session.base_mint),
            );
            session.record_price_sample(price_sample.price);
            if last_snapshot_at.elapsed() >= snapshot_interval {
                if let Some(wallet_position) =
                    query_best_effort_live_wallet_position(live, session).await
                {
                    session.apply_visible_wallet_position(wallet_position);
                }
                record_live_exit_snapshot_metrics(session, price_sample.source.as_label(), true);
                log_live_exit_snapshot(session, price_sample.source.as_label(), true);
                last_snapshot_at = Instant::now();
            }

            if let Some(trigger) = determine_live_exit_trigger(session, price_sample.price) {
                record_live_exit_trigger(trigger);
                session.transition(match trigger {
                    LiveExitTrigger::TakeProfit => LiveExitStatus::ExitTriggeredTakeProfit,
                    LiveExitTrigger::StopLoss => LiveExitStatus::ExitTriggeredStopLoss,
                });
                let attempt_number = execution_retry_count as usize + 1;

                let built_transaction = build_full_exit_transaction_with_retry(
                    live,
                    session,
                    price_sample.price,
                    sell_slippage_bps,
                )
                .await
                .map_err(|reason| (LiveExitStatus::ExitBuildFailed, reason))?;
                match submit_live_exit_transaction(
                    live,
                    session,
                    built_transaction,
                    trigger,
                    attempt_number,
                )
                .await
                {
                    Ok(()) => return Ok(()),
                    Err((status, reason))
                        if is_retryable_live_exit_failure(status)
                            && execution_retry_count < LIVE_EXIT_EXECUTION_MAX_RETRIES =>
                    {
                        execution_retry_count = execution_retry_count.saturating_add(1);
                        record_live_exit_retry(status);
                        let retry_delay_ms = live_exit_retry_delay_ms(execution_retry_count);
                        session.rearm_after_retryable_failure(
                            status,
                            &reason,
                            execution_retry_count,
                            LIVE_EXIT_EXECUTION_MAX_RETRIES,
                            retry_delay_ms,
                        );
                        tokio::time::sleep(Duration::from_millis(retry_delay_ms)).await;
                        continue;
                    }
                    Err((status, reason)) => return Err((status, reason)),
                }
            }
        } else {
            unavailable_polls = unavailable_polls.saturating_add(1);
            if unavailable_polls >= LIVE_EXIT_MONITORING_UNAVAILABLE_MAX_POLLS {
                return Err((
                    LiveExitStatus::MonitoringUnavailable,
                    format!("price_unavailable_for_{unavailable_polls}_polls"),
                ));
            }
            debug!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id = %session.candidate_id,
                base_mint = %session.base_mint,
                unavailable_polls,
                max_unavailable_polls = LIVE_EXIT_MONITORING_UNAVAILABLE_MAX_POLLS,
                "LiveExit: no canonical or point-query price available"
            );
            if last_snapshot_at.elapsed() >= snapshot_interval {
                if let Some(wallet_position) =
                    query_best_effort_live_wallet_position(live, session).await
                {
                    session.apply_visible_wallet_position(wallet_position);
                }
                record_live_exit_snapshot_metrics(session, "unavailable", false);
                log_live_exit_snapshot(session, "unavailable", false);
                last_snapshot_at = Instant::now();
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}

async fn run_live_sell_lifecycle_inner(
    live: &LiveSellHandle,
    session: &mut LiveExitSession,
    config: &PostBuyRuntimeConfig,
    sell_slippage_bps: u16,
    live_position_registry: Option<&LivePositionRegistry>,
) -> LiveExitResult {
    initialize_live_exit_session(live, session, live_position_registry, config).await?;
    monitor_live_exit_session(live, session, sell_slippage_bps).await
}

async fn run_live_sell_lifecycle(
    live: LiveSellHandle,
    session: LiveExitSession,
    config: PostBuyRuntimeConfig,
    position_limit_tracker: Option<PositionLimitTracker>,
    sell_slippage_bps: u16,
    live_position_registry: Option<LivePositionRegistry>,
) {
    let mut session = session;
    if let Err((status, reason)) = run_live_sell_lifecycle_inner(
        &live,
        &mut session,
        &config,
        sell_slippage_bps,
        live_position_registry.as_ref(),
    )
    .await
    {
        session.transition_terminal(status, reason);
    }

    if session.status == LiveExitStatus::ExitConfirmed {
        if let Some(registry) = live_position_registry.as_ref() {
            if let Err(error) = registry
                .record_closed(
                    &session.base_mint.to_string(),
                    &session.pool_amm_id.to_string(),
                    &session.buy_signature,
                    now_ms(),
                )
                .await
            {
                warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    candidate_id = %session.candidate_id,
                    base_mint = %session.base_mint,
                    error = %error,
                    "LiveExit: failed to record closed position in recovery registry"
                );
            }
        }
    }

    if session.should_release_position_slot() {
        release_slot(position_limit_tracker, session.position_slot_id);
    } else {
        retain_live_slot(
            session.position_slot_id,
            session.status,
            session.terminal_reason.as_deref(),
        );
    }
}

/// Query the user's visible token position from on-chain ATA state after confirmed BUY.
/// Tries Token-2022 first, then legacy SPL token. Returns `None` if neither ATA is visible
/// with a positive token balance.
async fn query_live_wallet_position(
    rpc: &AsyncRpcClient,
    owner: &Pubkey,
    mint: &Pubkey,
) -> Option<LiveWalletPosition> {
    let total_started_at = Instant::now();
    // Token-2022 program (all new PumpFun mints since Q4-2025).
    const TOKEN_2022: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
    // Legacy SPL token program (older mints).
    const TOKEN_LEGACY: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

    for program_id_str in [TOKEN_2022, TOKEN_LEGACY] {
        let Ok(prog) = Pubkey::from_str(program_id_str) else {
            continue;
        };
        let ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            owner, mint, &prog,
        );
        let rpc_started_at = Instant::now();
        match rpc.get_token_account_balance(&ata).await {
            Ok(resp) => {
                let latency_ms = saturating_elapsed_ms(rpc_started_at);
                let amount = resp.amount.parse::<u64>().unwrap_or_default();
                let outcome = if amount > 0 { "ok" } else { "zero" };
                record_live_sell_rpc_latency("get_token_account_balance", latency_ms, outcome);

                if latency_ms > LIVE_SELL_RPC_SLOW_MS {
                    warn!(
                        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                        mint = %mint,
                        ata = %ata,
                        token_program = program_id_str,
                        latency_ms,
                        amount,
                        "LiveSell: ATA balance RPC slower than target"
                    );
                }

                if amount > 0 {
                    let total_latency_ms = saturating_elapsed_ms(total_started_at);
                    record_live_sell_rpc_latency(
                        "query_actual_ata_balance",
                        total_latency_ms,
                        "ok",
                    );
                    info!(
                        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                        mint = %mint,
                        ata = %ata,
                        token_program = program_id_str,
                        latency_ms,
                        total_latency_ms,
                        amount,
                        "LiveSell: resolved actual ATA balance"
                    );
                    return Some(LiveWalletPosition {
                        token_account: ata,
                        token_program: prog,
                        token_amount: amount,
                    });
                }
            }
            Err(e) => {
                let latency_ms = saturating_elapsed_ms(rpc_started_at);
                record_live_sell_rpc_latency("get_token_account_balance", latency_ms, "rpc_error");
                debug!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    mint = %mint,
                    ata = %ata,
                    token_program = program_id_str,
                    latency_ms,
                    error = %e,
                    "LiveSell: ATA balance query failed for token program"
                );
            }
        }
    }

    record_live_sell_rpc_latency(
        "query_actual_ata_balance",
        saturating_elapsed_ms(total_started_at),
        "miss",
    );
    None
}

async fn query_known_token_account_position(
    rpc: &AsyncRpcClient,
    token_account: &Pubkey,
    known_token_program: Option<Pubkey>,
) -> Option<LiveWalletPosition> {
    let rpc_started_at = Instant::now();
    let resp = rpc.get_token_account_balance(token_account).await.ok()?;
    let latency_ms = saturating_elapsed_ms(rpc_started_at);
    let amount = resp.amount.parse::<u64>().unwrap_or_default();
    let outcome = if amount > 0 { "ok" } else { "zero" };
    record_live_sell_rpc_latency("get_known_token_account_balance", latency_ms, outcome);
    if amount == 0 {
        return None;
    }

    let token_program = if let Some(token_program) = known_token_program {
        token_program
    } else {
        let account_started_at = Instant::now();
        let account = rpc.get_account(token_account).await.ok()?;
        record_live_sell_rpc_latency(
            "get_known_token_account_info",
            saturating_elapsed_ms(account_started_at),
            "ok",
        );
        match account.owner {
            owner
                if owner == LIVE_EXIT_LEGACY_TOKEN_PROGRAM_ID
                    || owner == LIVE_EXIT_TOKEN_2022_PROGRAM_ID =>
            {
                owner
            }
            _ => return None,
        }
    };

    Some(LiveWalletPosition {
        token_account: *token_account,
        token_program,
        token_amount: amount,
    })
}

/// Query actual ATA balance from on-chain — canonical account state after confirmed BUY.
async fn query_best_effort_live_wallet_position(
    live: &LiveSellHandle,
    session: &LiveExitSession,
) -> Option<LiveWalletPosition> {
    use solana_sdk::signer::Signer as _;

    if let Some(token_account) = session.token_account {
        if let Some(position) = query_known_token_account_position(
            &live.rpc_client,
            &token_account,
            session.token_program,
        )
        .await
        {
            return Some(position);
        }
    }

    query_live_wallet_position(&live.rpc_client, &live.payer.pubkey(), &session.base_mint).await
}

async fn query_live_wallet_position_with_retry(
    live: &LiveSellHandle,
    session: &LiveExitSession,
) -> Option<LiveWalletPosition> {
    use solana_sdk::signer::Signer as _;

    let owner = live.payer.pubkey();
    for attempt in 1..=LIVE_SELL_ATA_LOOKUP_MAX_RETRIES {
        if let Some(position) = query_best_effort_live_wallet_position(live, session).await {
            return Some(position);
        }

        if attempt < LIVE_SELL_ATA_LOOKUP_MAX_RETRIES {
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                mint = %session.base_mint,
                owner = %owner,
                token_account = ?session.token_account,
                attempt,
                max_retries = LIVE_SELL_ATA_LOOKUP_MAX_RETRIES,
                retry_delay_ms = LIVE_SELL_ATA_LOOKUP_RETRY_MS,
                "LiveSell: wallet position not visible yet — retrying"
            );
            tokio::time::sleep(Duration::from_millis(LIVE_SELL_ATA_LOOKUP_RETRY_MS)).await;
        }
    }

    ::metrics::counter!("post_buy_live_sell_ata_resolution_failed_total", 1u64);
    None
}
fn release_slot(
    position_limit_tracker: Option<PositionLimitTracker>,
    slot_id: Option<PositionSlotId>,
) {
    if let (Some(tracker), Some(id)) = (position_limit_tracker, slot_id) {
        if !tracker.release(id) {
            warn!(
                slot_id = %id,
                "PostBuyRuntime (live): position slot already released"
            );
        }
    }
}

fn retain_live_slot(slot_id: Option<PositionSlotId>, status: LiveExitStatus, reason: Option<&str>) {
    if let Some(id) = slot_id {
        ::metrics::counter!(
            "post_buy_live_slot_retained_total",
            1u64,
            "status" => status.as_label()
        );
        warn!(
            slot_id = %id,
            status = status.as_label(),
            reason = reason.unwrap_or("unknown"),
            "PostBuyRuntime (live): keeping position slot reserved because the position may still be open"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{create_event_bus, create_event_bus_with_capacity, GhostEvent};
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use ghost_brain::events::{EventKind, ExecutionEvent};
    use ghost_core::account_state_core::types::{AccountStateUpdate, UpdateSource};
    use ghost_core::CurveFinality;
    use metrics::{
        Counter, CounterFn, Gauge, Histogram, Key, KeyName, Recorder, SharedString, Unit,
    };
    use solana_sdk::pubkey::Pubkey;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct RecordedCounter {
        name: String,
        labels: Vec<(String, String)>,
    }

    #[derive(Clone)]
    struct TestMetricsHandle {
        counters: Arc<Mutex<Vec<RecordedCounter>>>,
    }

    struct TestMetricsRecorder {
        handle: TestMetricsHandle,
    }

    struct TestCounter {
        handle: TestMetricsHandle,
        metric: RecordedCounter,
    }

    impl CounterFn for TestCounter {
        fn increment(&self, _value: u64) {
            self.handle
                .counters
                .lock()
                .expect("counter lock")
                .push(self.metric.clone());
        }

        fn absolute(&self, _value: u64) {
            self.handle
                .counters
                .lock()
                .expect("counter lock")
                .push(self.metric.clone());
        }
    }

    impl Recorder for TestMetricsRecorder {
        fn describe_counter(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {
        }

        fn describe_gauge(&self, _key: KeyName, _unit: Option<Unit>, _description: SharedString) {}

        fn describe_histogram(
            &self,
            _key: KeyName,
            _unit: Option<Unit>,
            _description: SharedString,
        ) {
        }

        fn register_counter(&self, key: &Key) -> Counter {
            Counter::from_arc(Arc::new(TestCounter {
                handle: self.handle.clone(),
                metric: RecordedCounter {
                    name: key.name().to_string(),
                    labels: key
                        .labels()
                        .map(|label| (label.key().to_string(), label.value().to_string()))
                        .collect(),
                },
            }))
        }

        fn register_gauge(&self, _key: &Key) -> Gauge {
            Gauge::noop()
        }

        fn register_histogram(&self, _key: &Key) -> Histogram {
            Histogram::noop()
        }
    }

    static TEST_METRICS_HANDLE: OnceLock<TestMetricsHandle> = OnceLock::new();

    fn metrics_handle() -> TestMetricsHandle {
        TEST_METRICS_HANDLE
            .get_or_init(|| {
                let handle = TestMetricsHandle {
                    counters: Arc::new(Mutex::new(Vec::new())),
                };
                metrics::set_boxed_recorder(Box::new(TestMetricsRecorder {
                    handle: handle.clone(),
                }))
                .expect("install test metrics recorder");
                handle
            })
            .clone()
    }

    fn clear_recorded_counters() {
        metrics_handle()
            .counters
            .lock()
            .expect("counter lock")
            .clear();
    }

    fn saw_counter(name: &str, expected_labels: &[(&str, &str)]) -> bool {
        metrics_handle()
            .counters
            .lock()
            .expect("counter lock")
            .iter()
            .any(|counter| {
                counter.name == name
                    && expected_labels.iter().all(|(key, value)| {
                        counter.labels.iter().any(|(observed_key, observed_value)| {
                            observed_key == key && observed_value == value
                        })
                    })
            })
    }

    fn apply_canonical_update(
        account_state_core: &AccountStateReducer,
        mint: Pubkey,
        sol_reserves: u64,
        token_reserves: u64,
    ) {
        let update = AccountStateUpdate {
            pool_amm_id: Pubkey::new_unique(),
            base_mint: mint,
            bonding_curve: Pubkey::new_unique(),
            sol_reserves,
            token_reserves,
            is_complete: 0,
            slot: 42,
            write_version: Some(1),
            receive_ts_ms: now_ms(),
            receive_seq: 1,
            curve_finality: CurveFinality::Provisional,
            source: UpdateSource::GeyserAccountUpdate,
        };
        let _ = account_state_core.apply_account_update(update);
    }

    fn serialize_bonding_curve(curve: &BondingCurve) -> [u8; 56] {
        let mut bytes = [0u8; 56];
        bytes[0..8].copy_from_slice(&curve.discriminator.to_le_bytes());
        bytes[8..16].copy_from_slice(&curve.virtual_token_reserves.to_le_bytes());
        bytes[16..24].copy_from_slice(&curve.virtual_sol_reserves.to_le_bytes());
        bytes[24..32].copy_from_slice(&curve.real_token_reserves.to_le_bytes());
        bytes[32..40].copy_from_slice(&curve.real_sol_reserves.to_le_bytes());
        bytes[40..48].copy_from_slice(&curve.token_total_supply.to_le_bytes());
        bytes[48] = curve.complete;
        bytes[49..56].copy_from_slice(&curve._padding);
        bytes
    }

    fn mock_curve_account_info_body(curve: &BondingCurve) -> String {
        let mut bytes = vec![0u8; 83];
        bytes[0..8].copy_from_slice(&0xDEAD_BEEF_u64.to_le_bytes());
        bytes[8..16].copy_from_slice(&curve.virtual_token_reserves.to_le_bytes());
        bytes[16..24].copy_from_slice(&curve.virtual_sol_reserves.to_le_bytes());
        bytes[24..32].copy_from_slice(&curve.real_token_reserves.to_le_bytes());
        bytes[32..40].copy_from_slice(&curve.real_sol_reserves.to_le_bytes());
        bytes[40..48].copy_from_slice(&curve.token_total_supply.to_le_bytes());
        bytes[48] = curve.complete;
        let encoded = BASE64_STANDARD.encode(bytes);
        format!(
            "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":{{\"data\":[\"{}\",\"base64\"],\"executable\":false,\"lamports\":1,\"owner\":\"{}\",\"rentEpoch\":0,\"space\":83}}}},\"id\":1}}",
            encoded, PUMP_PROGRAM_ID
        )
    }

    async fn spawn_curve_rpc_server(
        curve_key: Pubkey,
        curve: BondingCurve,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let addr = listener.local_addr().expect("rpc addr");
        let request_count = Arc::new(AtomicUsize::new(0));
        let request_count_task = Arc::clone(&request_count);
        let curve_key = curve_key.to_string();
        let success_body = mock_curve_account_info_body(&curve);

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
                let body = if request.contains("\"getAccountInfo\"") && request.contains(&curve_key)
                {
                    request_count_task.fetch_add(1, Ordering::Relaxed);
                    success_body.clone()
                } else if request.contains("\"getAccountInfo\"") {
                    request_count_task.fetch_add(1, Ordering::Relaxed);
                    "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32002,\"message\":\"AccountNotFound\"},\"id\":1}".to_string()
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

        (format!("http://{}", addr), request_count)
    }

    async fn spawn_retrying_curve_rpc_server(
        curve_key: Pubkey,
        curve: BondingCurve,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let addr = listener.local_addr().expect("rpc addr");
        let request_count = Arc::new(AtomicUsize::new(0));
        let request_count_task = Arc::clone(&request_count);
        let curve_key = curve_key.to_string();
        let success_body = mock_curve_account_info_body(&curve);

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
                let body = if request.contains("\"getAccountInfo\"") && request.contains(&curve_key)
                {
                    let request_index = request_count_task.fetch_add(1, Ordering::Relaxed);
                    if request_index == 0 {
                        "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32002,\"message\":\"AccountNotFound\"},\"id\":1}".to_string()
                    } else {
                        success_body.clone()
                    }
                } else if request.contains("\"getAccountInfo\"") {
                    request_count_task.fetch_add(1, Ordering::Relaxed);
                    "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32002,\"message\":\"AccountNotFound\"},\"id\":1}".to_string()
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

        (format!("http://{}", addr), request_count)
    }

    async fn spawn_blockhash_rpc_server(
        latest_blockhash: solana_sdk::hash::Hash,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let addr = listener.local_addr().expect("rpc addr");
        let request_count = Arc::new(AtomicUsize::new(0));
        let request_count_task = Arc::clone(&request_count);
        let default_curve = BondingCurve {
            discriminator: 6966180631402821399,
            virtual_token_reserves: 1_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 1_000_000_000_000,
            real_sol_reserves: 1_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        let curve_body = mock_curve_account_info_body(&default_curve);

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
                let body = if request.contains("\"getLatestBlockhash\"") {
                    request_count_task.fetch_add(1, Ordering::Relaxed);
                    format!(
                        "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":{{\"blockhash\":\"{}\",\"lastValidBlockHeight\":123456}}}},\"id\":1}}",
                        latest_blockhash
                    )
                } else if request.contains("\"getAccountInfo\"") {
                    curve_body.clone()
                } else if request.contains("\"getTokenAccountBalance\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":{\"context\":{\"slot\":1},\"value\":{\"amount\":\"0\",\"decimals\":6,\"uiAmount\":0.0,\"uiAmountString\":\"0\"}},\"id\":1}".to_string()
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

        (format!("http://{}", addr), request_count)
    }

    async fn spawn_sequenced_blockhash_rpc_server(
        blockhashes: Vec<solana_sdk::hash::Hash>,
    ) -> (String, Arc<AtomicUsize>) {
        assert!(
            !blockhashes.is_empty(),
            "blockhash sequence must not be empty"
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let addr = listener.local_addr().expect("rpc addr");
        let request_count = Arc::new(AtomicUsize::new(0));
        let request_count_task = Arc::clone(&request_count);
        let blockhashes = Arc::new(blockhashes);
        let default_curve = BondingCurve {
            discriminator: 6966180631402821399,
            virtual_token_reserves: 1_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 1_000_000_000_000,
            real_sol_reserves: 1_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        let curve_body = mock_curve_account_info_body(&default_curve);

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
                let body = if request.contains("\"getLatestBlockhash\"") {
                    let request_index = request_count_task.fetch_add(1, Ordering::Relaxed);
                    let blockhash = blockhashes
                        .get(request_index)
                        .copied()
                        .unwrap_or_else(|| *blockhashes.last().expect("last blockhash"));
                    format!(
                        "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":{{\"blockhash\":\"{}\",\"lastValidBlockHeight\":123456}}}},\"id\":1}}",
                        blockhash
                    )
                } else if request.contains("\"getAccountInfo\"") {
                    curve_body.clone()
                } else if request.contains("\"getTokenAccountBalance\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":{\"context\":{\"slot\":1},\"value\":{\"amount\":\"0\",\"decimals\":6,\"uiAmount\":0.0,\"uiAmountString\":\"0\"}},\"id\":1}".to_string()
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

        (format!("http://{}", addr), request_count)
    }

    fn mock_token_account_balance_body(amount: u64) -> String {
        let ui_amount = amount as f64 / 1_000_000.0;
        format!(
            "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":{{\"amount\":\"{}\",\"decimals\":6,\"uiAmount\":{},\"uiAmountString\":\"{:.6}\"}}}},\"id\":1}}",
            amount, ui_amount, ui_amount
        )
    }

    async fn spawn_token_balance_rpc_server(
        expected_ata: Pubkey,
        amount: u64,
    ) -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let addr = listener.local_addr().expect("rpc addr");
        let token_balance_requests = Arc::new(AtomicUsize::new(0));
        let account_info_requests = Arc::new(AtomicUsize::new(0));
        let token_balance_requests_task = Arc::clone(&token_balance_requests);
        let account_info_requests_task = Arc::clone(&account_info_requests);
        let expected_ata = expected_ata.to_string();
        let success_body = mock_token_account_balance_body(amount);

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
                let body = if request.contains("\"getTokenAccountBalance\"")
                    && request.contains(&expected_ata)
                {
                    token_balance_requests_task.fetch_add(1, Ordering::Relaxed);
                    success_body.clone()
                } else if request.contains("\"getTokenAccountBalance\"") {
                    token_balance_requests_task.fetch_add(1, Ordering::Relaxed);
                    "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32002,\"message\":\"AccountNotFound\"},\"id\":1}".to_string()
                } else if request.contains("\"getAccountInfo\"") {
                    account_info_requests_task.fetch_add(1, Ordering::Relaxed);
                    "{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32002,\"message\":\"AccountNotFound\"},\"id\":1}".to_string()
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

        (
            format!("http://{}", addr),
            token_balance_requests,
            account_info_requests,
        )
    }

    fn test_live_sell_handle_with_sender(
        rpc_url: String,
        account_state_core: Arc<AccountStateReducer>,
        yellowstone_grpc_endpoint: &str,
    ) -> LiveSellHandle {
        let rpc_client = Arc::new(AsyncRpcClient::new(rpc_url));

        LiveSellHandle {
            rpc_client,
            live_tx_sender: Arc::new(
                LiveTxSender::new(crate::components::live_tx_sender::LiveTxSenderConfig::new(
                    "test://sender-success",
                    "http://127.0.0.1:18081",
                    yellowstone_grpc_endpoint,
                    "test-yellowstone-token",
                ))
                .expect("test live tx sender"),
            ),
            payer: Arc::new(Keypair::new()),
            account_state_core,
            shadow_ledger: Arc::new(ShadowLedger::new()),
        }
    }

    fn test_live_sell_handle(
        rpc_url: String,
        account_state_core: Arc<AccountStateReducer>,
    ) -> LiveSellHandle {
        test_live_sell_handle_with_sender(
            rpc_url,
            account_state_core,
            "test://yellowstone-confirmed",
        )
    }

    fn test_entry_price_info(
        mint: Pubkey,
        entry_price: u64,
        tokens_received: u64,
        sol_spent: u64,
        slot: u64,
    ) -> EntryPriceInfo {
        EntryPriceInfo {
            price_lamports_per_token: entry_price,
            tokens_received,
            sol_spent,
            signature: Signature::new_unique(),
            slot,
            mint,
            token_account: Pubkey::new_unique(),
            token_balance_after_buy: tokens_received,
            token_decimals: 6,
            token_program: Some(LIVE_EXIT_TOKEN_2022_PROGRAM_ID),
            fee_recipient: Some(Pubkey::new_unique()),
        }
    }

    fn seeded_live_exit_session(
        mint: Pubkey,
        entry_price: u64,
        tokens_received: u64,
    ) -> LiveExitSession {
        let mut session = LiveExitSession::new(
            "candidate".to_string(),
            Pubkey::new_unique(),
            mint,
            Some(Pubkey::new_unique()),
            Signature::new_unique().to_string(),
            None,
            2_000_000,
            Some(PositionSlotId::derive(&Pubkey::new_unique(), &mint)),
        );
        session
            .populate_entry_price(
                &test_entry_price_info(mint, entry_price, tokens_received, 1_000_000_000, 55),
                &PostBuyRuntimeConfig::default(),
            )
            .expect("seed entry price");
        session.set_token_program(LIVE_EXIT_TOKEN_2022_PROGRAM_ID);
        session.transition(LiveExitStatus::Armed);
        session.transition(LiveExitStatus::Monitoring);
        session
    }

    #[test]
    fn phase4_post_buy_price_source_metrics_are_wired() {
        let source = include_str!("post_buy_runtime.rs");
        let implementation = source
            .split("#[cfg(test)]")
            .next()
            .expect("implementation section should exist");
        assert!(
            implementation.contains("\"post_buy_price_source_total\""),
            "post-buy runtime must expose live price source telemetry"
        );
        assert!(
            implementation.contains("canonical_account_state"),
            "post-buy runtime must label canonical AccountStateCore price hits"
        );
        assert!(
            implementation.contains("rpc_point_query"),
            "post-buy runtime must label RPC point-query fallback hits"
        );
        assert!(
            implementation.contains("\"unavailable\""),
            "post-buy runtime must surface unavailable price cycles explicitly"
        );
        assert!(
            implementation.contains("\"post_buy_shadow_compare_total\""),
            "post-buy runtime must expose diagnostic dual-read compare telemetry"
        );
    }

    #[test]
    fn phase4_post_buy_live_lane_no_longer_uses_shadow_as_truth_source() {
        let source = include_str!("post_buy_runtime.rs");
        let implementation = source
            .split("#[cfg(test)]")
            .next()
            .expect("implementation section should exist");
        let legacy_shadow_helper = ["fn ", "read_price_from_", "shadow("].concat();

        assert!(
            !implementation.contains(&legacy_shadow_helper),
            "Phase 4 must remove the live shadow truth helper from post-buy runtime"
        );
        assert!(
            !implementation.contains("\"source\" => \"shadow_ledger\""),
            "Phase 4 live price source telemetry must not report ShadowLedger as truth"
        );
        assert!(
            !implementation.contains("\"shadow_truth_fallback_total\""),
            "Phase 4 post-buy runtime must not meter shadow reads as live truth fallback"
        );
    }

    #[test]
    fn canonical_live_price_uses_account_state_core_contract() {
        let account_state_core = AccountStateReducer::new();
        let mint = Pubkey::new_unique();
        apply_canonical_update(&account_state_core, mint, 30_000_000_000, 1_000_000_000_000);

        let price = try_canonical_live_price(&account_state_core, &mint)
            .expect("canonical state should price");
        assert_eq!(price, 30_000_000);
    }

    #[test]
    fn live_exit_session_persists_confirmed_entry_contract() {
        let mint = Pubkey::new_unique();
        let mut session = LiveExitSession::new(
            "candidate".to_string(),
            Pubkey::new_unique(),
            mint,
            Some(Pubkey::new_unique()),
            Signature::new_unique().to_string(),
            Some(42),
            2_000_000,
            Some(PositionSlotId::derive(&Pubkey::new_unique(), &mint)),
        );
        let entry_info = test_entry_price_info(mint, 10_000_000, 2_000_000, 900_000_000, 55);
        session
            .populate_entry_price(&entry_info, &PostBuyRuntimeConfig::default())
            .expect("populate confirmed entry");

        assert_eq!(session.buy_landed_slot, Some(42));
        assert_eq!(session.tokens_received, Some(2_000_000));
        assert_eq!(session.sol_spent_lamports, Some(900_000_000));
        assert_eq!(session.entry_price_lamports_per_token, Some(10_000_000));
        assert_eq!(session.fee_recipient, entry_info.fee_recipient);
        assert_eq!(
            session.upper_exit_price_lamports_per_token,
            Some(10_200_000)
        );
        assert_eq!(session.lower_exit_price_lamports_per_token, Some(9_800_000));
        assert_eq!(session.latest_price_lamports_per_token, Some(10_000_000));
        assert_eq!(session.latest_pnl_pct, Some(0.0));
    }

    #[test]
    fn live_exit_session_uses_configured_thresholds() {
        let mint = Pubkey::new_unique();
        let mut session = LiveExitSession::new(
            "candidate".to_string(),
            Pubkey::new_unique(),
            mint,
            Some(Pubkey::new_unique()),
            Signature::new_unique().to_string(),
            Some(42),
            2_000_000,
            Some(PositionSlotId::derive(&Pubkey::new_unique(), &mint)),
        );
        let entry_info = test_entry_price_info(mint, 10_000_000, 2_000_000, 900_000_000, 55);
        let config = PostBuyRuntimeConfig {
            live_exit_take_profit_pct: 0.30,
            live_exit_stop_loss_pct: 0.30,
            ..PostBuyRuntimeConfig::default()
        };

        session
            .populate_entry_price(&entry_info, &config)
            .expect("populate configured entry thresholds");

        assert_eq!(
            session.upper_exit_price_lamports_per_token,
            Some(13_000_000)
        );
        assert_eq!(session.lower_exit_price_lamports_per_token, Some(7_000_000));
    }

    #[test]
    fn live_exit_trigger_matches_stage1_plus_minus_2_contract() {
        let mint = Pubkey::new_unique();
        let session = seeded_live_exit_session(mint, 10_000_000, 1_500_000);

        assert_eq!(determine_live_exit_trigger(&session, 9_999_999), None);
        assert_eq!(
            determine_live_exit_trigger(&session, 10_200_000),
            Some(LiveExitTrigger::TakeProfit)
        );
        assert_eq!(
            determine_live_exit_trigger(&session, 10_500_000),
            Some(LiveExitTrigger::TakeProfit)
        );
        assert_eq!(
            determine_live_exit_trigger(&session, 9_800_000),
            Some(LiveExitTrigger::StopLoss)
        );
        assert_eq!(
            determine_live_exit_trigger(&session, 9_500_000),
            Some(LiveExitTrigger::StopLoss)
        );
    }

    #[test]
    fn live_exit_position_slot_release_requires_confirmed_exit() {
        let mint = Pubkey::new_unique();
        let mut session = seeded_live_exit_session(mint, 10_000_000, 1_000_000);

        assert!(
            !session.should_release_position_slot(),
            "armed live position must keep the slot reserved"
        );

        session.transition_terminal(LiveExitStatus::ExitBuildFailed, "build_failed");
        assert!(
            !session.should_release_position_slot(),
            "failed live exit must keep the slot reserved"
        );

        session.transition_terminal(LiveExitStatus::ExitConfirmFailed, "confirm_failed");
        assert!(
            !session.should_release_position_slot(),
            "terminal live exit confirmation failure must keep the slot reserved"
        );

        session.status = LiveExitStatus::ExitConfirmed;
        assert!(
            session.should_release_position_slot(),
            "confirmed live exit must release the slot"
        );
    }

    #[test]
    fn live_exit_retry_policy_only_retries_submit_and_confirm_failures() {
        assert!(is_retryable_live_exit_failure(
            LiveExitStatus::ExitSubmitFailed
        ));
        assert!(is_retryable_live_exit_failure(
            LiveExitStatus::ExitConfirmFailed
        ));
        assert!(!is_retryable_live_exit_failure(
            LiveExitStatus::ExitBuildFailed
        ));
        assert!(!is_retryable_live_exit_failure(
            LiveExitStatus::MonitoringUnavailable
        ));
    }

    #[test]
    fn curve_cashback_enabled_detection_reads_upgrade_flag_byte() {
        let mut non_cashback = vec![0u8; 151];
        non_cashback[82] = 0;
        assert!(!curve_cashback_enabled_from_account_data(&non_cashback));

        let mut cashback = vec![0u8; 151];
        cashback[82] = 1;
        assert!(curve_cashback_enabled_from_account_data(&cashback));

        let legacy_layout = vec![0u8; 56];
        assert!(
            !curve_cashback_enabled_from_account_data(&legacy_layout),
            "legacy layouts without byte[82] must default to non-cashback"
        );
    }

    #[test]
    fn live_exit_min_output_cap_respects_real_sol_reserves() {
        assert_eq!(cap_live_exit_min_output(61_233, Some(56_152)), 56_151);
        assert_eq!(cap_live_exit_min_output(50_000, Some(56_152)), 50_000);
        assert_eq!(cap_live_exit_min_output(50_000, None), 50_000);
        assert_eq!(cap_live_exit_min_output(50_000, Some(0)), 50_000);
    }

    #[test]
    fn live_exit_retry_rearms_monitoring_and_clears_pending_submission_tracking() {
        let mint = Pubkey::new_unique();
        let mut session = seeded_live_exit_session(mint, 10_000_000, 1_000_000);
        session.exit_signature = Some(Signature::new_unique().to_string());
        let previous_blockhash = solana_sdk::hash::Hash::new_unique();
        session.last_exit_recent_blockhash = Some(previous_blockhash);
        session.last_exit_submit_slot = Some(42);
        session.exit_landed_slot = Some(123);
        session.status = LiveExitStatus::ExitSubmitted;
        session.terminal_reason = Some("old_reason".to_string());

        session.rearm_after_retryable_failure(
            LiveExitStatus::ExitConfirmFailed,
            "submission_rejected",
            1,
            LIVE_EXIT_EXECUTION_MAX_RETRIES,
            live_exit_retry_delay_ms(1),
        );

        assert_eq!(session.status, LiveExitStatus::Monitoring);
        assert_eq!(session.exit_signature, None);
        assert_eq!(session.last_exit_recent_blockhash, Some(previous_blockhash));
        assert_eq!(session.last_exit_submit_slot, None);
        assert_eq!(session.exit_landed_slot, None);
        assert_eq!(session.terminal_reason, None);
        assert!(
            !session.should_release_position_slot(),
            "rearmed live exit must keep the slot reserved"
        );
    }

    #[test]
    fn live_exit_retry_delay_ms_escalates_and_caps() {
        assert_eq!(live_exit_retry_delay_ms(1), 1_000);
        assert_eq!(live_exit_retry_delay_ms(2), 2_000);
        assert_eq!(
            live_exit_retry_delay_ms(3),
            LIVE_EXIT_EXECUTION_RETRY_MAX_DELAY_MS
        );
        assert_eq!(
            live_exit_retry_delay_ms(99),
            LIVE_EXIT_EXECUTION_RETRY_MAX_DELAY_MS
        );
    }

    #[tokio::test]
    async fn live_price_sample_prefers_canonical_state_before_rpc_point_query() {
        clear_recorded_counters();

        let mint = Pubkey::new_unique();
        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).expect("pump program id");
        let curve_key = derive_bonding_curve_pda(&mint, &pump_program).0;
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 2_000_000_000_000,
            virtual_sol_reserves: 80_000_000_000,
            real_token_reserves: 0,
            real_sol_reserves: 0,
            token_total_supply: 2_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        let (rpc_url, request_count) = spawn_curve_rpc_server(curve_key, curve).await;
        let account_state_core = Arc::new(AccountStateReducer::new());
        apply_canonical_update(&account_state_core, mint, 30_000_000_000, 1_000_000_000_000);
        let live = test_live_sell_handle(rpc_url, Arc::clone(&account_state_core));

        let sample = read_live_price_sample(&live, &mint)
            .await
            .expect("canonical price sample");

        assert_eq!(sample.source, LivePriceSource::CanonicalAccountState);
        assert_eq!(sample.price, 30_000_000);
        assert_eq!(
            request_count.load(Ordering::Relaxed),
            0,
            "canonical price hit must not touch RPC fallback"
        );
        assert!(
            saw_counter(
                "post_buy_price_source_total",
                &[("source", "canonical_account_state")]
            ),
            "canonical path must emit canonical_account_state telemetry"
        );
    }

    #[tokio::test]
    async fn live_price_sample_falls_back_to_rpc_point_query_when_canonical_missing() {
        clear_recorded_counters();

        let mint = Pubkey::new_unique();
        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).expect("pump program id");
        let curve_key = derive_bonding_curve_pda(&mint, &pump_program).0;
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 0,
            real_sol_reserves: 0,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        let (rpc_url, request_count) = spawn_curve_rpc_server(curve_key, curve).await;
        let live = test_live_sell_handle(rpc_url, Arc::new(AccountStateReducer::new()));

        let sample = read_live_price_sample(&live, &mint)
            .await
            .expect("rpc fallback price sample");

        assert_eq!(sample.source, LivePriceSource::RpcPointQuery);
        assert_eq!(sample.price, 30_000_000);
        assert!(
            request_count.load(Ordering::Relaxed) > 0,
            "missing canonical state must hit RPC point-query fallback"
        );
        assert!(
            saw_counter(
                "post_buy_price_source_total",
                &[("source", "rpc_point_query")]
            ),
            "rpc fallback must emit rpc_point_query telemetry"
        );
    }

    #[tokio::test]
    async fn build_full_exit_transaction_uses_full_token_amount() {
        let mint = Pubkey::new_unique();
        let tokens_received = 1_750_000;
        let (rpc_url, request_count) =
            spawn_blockhash_rpc_server(solana_sdk::hash::Hash::new_unique()).await;
        let live = test_live_sell_handle(rpc_url, Arc::new(AccountStateReducer::new()));
        let session = seeded_live_exit_session(mint, 10_000_000, tokens_received);

        let built_transaction =
            build_full_exit_transaction_with_retry(&live, &session, 11_000_000, 2_000)
                .await
                .expect("build full exit tx");
        let transaction = built_transaction.transaction;

        let (amount, min_output, token_program) = match &transaction.message {
            solana_sdk::message::VersionedMessage::Legacy(message) => {
                // Sell instruction is after 2 ComputeBudget instructions (CU limit + CU price)
                let ix = &message.instructions[2];
                (
                    u64::from_le_bytes(ix.data[8..16].try_into().expect("amount bytes")),
                    u64::from_le_bytes(ix.data[16..24].try_into().expect("min_output bytes")),
                    message.account_keys[ix.accounts[9] as usize],
                )
            }
            solana_sdk::message::VersionedMessage::V0(message) => {
                // Sell instruction is after 2 ComputeBudget instructions (CU limit + CU price)
                let ix = &message.instructions[2];
                (
                    u64::from_le_bytes(ix.data[8..16].try_into().expect("amount bytes")),
                    u64::from_le_bytes(ix.data[16..24].try_into().expect("min_output bytes")),
                    message.account_keys[ix.accounts[9] as usize],
                )
            }
        };

        let expected_min_output =
            SellTxBuilder::calculate_min_output(tokens_received, 11_000_000, 2_000)
                .expect("expected min output")
                .max(1);
        assert_eq!(amount, tokens_received);
        assert_eq!(min_output, expected_min_output);
        assert_eq!(token_program, LIVE_EXIT_TOKEN_2022_PROGRAM_ID);
        assert!(
            request_count.load(Ordering::Relaxed) > 0,
            "building the full exit should fetch a fresh blockhash"
        );
    }

    #[tokio::test]
    async fn read_live_curve_execution_hints_retries_account_not_found() {
        let mint = Pubkey::new_unique();
        let pump_program = Pubkey::from_str(PUMP_PROGRAM_ID).expect("valid pump program");
        let curve_key = derive_bonding_curve_pda(&mint, &pump_program).0;
        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_057_649_144_177_255,
            virtual_sol_reserves: 20_143_033_402,
            real_token_reserves: 777_749_144_177_255,
            real_sol_reserves: 366_007_146,
            token_total_supply: 1_000_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        let (rpc_url, request_count) = spawn_retrying_curve_rpc_server(curve_key, curve).await;
        let rpc_client = AsyncRpcClient::new(rpc_url);

        let hints = read_live_curve_execution_hints(&rpc_client, &mint)
            .await
            .expect("curve hints should retry past initial AccountNotFound");

        assert!(!hints.cashback_enabled);
        assert_eq!(hints.real_sol_reserves, Some(curve.real_sol_reserves));
        assert!(
            request_count.load(Ordering::Relaxed) >= 2,
            "curve fetch should retry after initial AccountNotFound"
        );
    }

    #[tokio::test]
    async fn build_full_exit_transaction_appends_inline_tip_transfer() {
        let mint = Pubkey::new_unique();
        let tokens_received = 1_750_000;
        let buy_tip_lamports = 2_000_000;
        let (rpc_url, _request_count) =
            spawn_blockhash_rpc_server(solana_sdk::hash::Hash::new_unique()).await;
        let live = test_live_sell_handle(rpc_url, Arc::new(AccountStateReducer::new()));
        let mut session = seeded_live_exit_session(mint, 10_000_000, tokens_received);
        session.tip_lamports = buy_tip_lamports;

        let built_transaction =
            build_full_exit_transaction_with_retry(&live, &session, 11_000_000, 2_000)
                .await
                .expect("build full exit tx");
        assert_eq!(built_transaction.tip_lamports, LIVE_EXIT_MAX_TIP_LAMPORTS);
        let transaction = built_transaction.transaction;
        match &transaction.message {
            solana_sdk::message::VersionedMessage::Legacy(_) => {
                panic!("full exit transaction should use a v0 message")
            }
            solana_sdk::message::VersionedMessage::V0(message) => {
                let instruction = message.instructions.last().expect("inline tip instruction");
                assert!(
                    instruction.accounts.len() >= 2,
                    "inline tip must write-lock both payer and tip destination accounts"
                );
                let encoded_tip_lamports =
                    u64::from_le_bytes(instruction.data[4..12].try_into().expect("tip bytes"));
                assert_eq!(encoded_tip_lamports, LIVE_EXIT_MAX_TIP_LAMPORTS);
            }
        }
    }

    #[tokio::test]
    async fn build_full_exit_transaction_retry_waits_for_fresh_blockhash() {
        let mint = Pubkey::new_unique();
        let previous_blockhash = solana_sdk::hash::Hash::new_unique();
        let fresh_blockhash = solana_sdk::hash::Hash::new_unique();
        let (rpc_url, request_count) =
            spawn_sequenced_blockhash_rpc_server(vec![previous_blockhash, fresh_blockhash]).await;
        let live = test_live_sell_handle(rpc_url, Arc::new(AccountStateReducer::new()));
        let mut session = seeded_live_exit_session(mint, 10_000_000, 1_750_000);
        session.last_exit_recent_blockhash = Some(previous_blockhash);

        let built_transaction =
            build_full_exit_transaction_with_retry(&live, &session, 11_000_000, 2_000)
                .await
                .expect("build full exit tx with fresh retry blockhash");
        let transaction = built_transaction.transaction;

        let recent_blockhash = match &transaction.message {
            solana_sdk::message::VersionedMessage::Legacy(message) => message.recent_blockhash,
            solana_sdk::message::VersionedMessage::V0(message) => message.recent_blockhash,
        };

        assert_eq!(recent_blockhash, fresh_blockhash);
        assert!(
            request_count.load(Ordering::Relaxed) >= 2,
            "retry should keep polling until a fresh blockhash is available"
        );
    }

    #[tokio::test]
    async fn live_exit_retry_rebuilds_sell_and_tip_signatures_without_reuse() {
        let mint = Pubkey::new_unique();
        let first_blockhash = solana_sdk::hash::Hash::new_unique();
        let second_blockhash = solana_sdk::hash::Hash::new_unique();
        let (rpc_url, _request_count) =
            spawn_sequenced_blockhash_rpc_server(vec![first_blockhash, second_blockhash]).await;
        let live = test_live_sell_handle(rpc_url, Arc::new(AccountStateReducer::new()));
        let mut session = seeded_live_exit_session(mint, 10_000_000, 1_750_000);
        session.tip_lamports = 250_000;

        let first_transaction =
            build_full_exit_transaction_with_retry(&live, &session, 11_000_000, 2_000)
                .await
                .expect("build first full exit tx");
        let first_signature = first_transaction.transaction.signatures[0];
        let first_recent_blockhash = match &first_transaction.transaction.message {
            solana_sdk::message::VersionedMessage::Legacy(message) => message.recent_blockhash,
            solana_sdk::message::VersionedMessage::V0(message) => message.recent_blockhash,
        };
        session.last_exit_recent_blockhash = Some(first_recent_blockhash);

        let second_transaction =
            build_full_exit_transaction_with_retry(&live, &session, 11_000_000, 2_000)
                .await
                .expect("build second full exit tx");
        let second_signature = second_transaction.transaction.signatures[0];
        let second_recent_blockhash = match &second_transaction.transaction.message {
            solana_sdk::message::VersionedMessage::Legacy(message) => message.recent_blockhash,
            solana_sdk::message::VersionedMessage::V0(message) => message.recent_blockhash,
        };

        assert_ne!(first_recent_blockhash, second_recent_blockhash);
        assert_ne!(
            first_signature, second_signature,
            "sell retry must not reuse the previous signed Sender transaction"
        );
    }

    #[tokio::test]
    async fn submit_live_exit_transaction_confirms_via_balance_zero_fallback() {
        let mint = Pubkey::new_unique();
        let tokens_received = 1_750_000;
        let (rpc_url, _request_count) =
            spawn_blockhash_rpc_server(solana_sdk::hash::Hash::new_unique()).await;
        let live = test_live_sell_handle_with_sender(
            rpc_url,
            Arc::new(AccountStateReducer::new()),
            "test://yellowstone-resource-exhausted",
        );
        let mut session = seeded_live_exit_session(mint, 10_000_000, tokens_received);

        let built_transaction =
            build_full_exit_transaction_with_retry(&live, &session, 9_000_000, 2_000)
                .await
                .expect("build full exit tx");
        submit_live_exit_transaction(
            &live,
            &mut session,
            built_transaction,
            LiveExitTrigger::StopLoss,
            1,
        )
        .await
        .expect("balance-zero fallback should confirm the SELL");

        assert_eq!(session.status, LiveExitStatus::ExitConfirmed);
        assert_eq!(
            session.terminal_reason.as_deref(),
            Some("stop_loss_confirmed")
        );
        assert_eq!(session.visible_token_balance, Some(0));
    }

    #[tokio::test]
    async fn confirm_sender_sell_attempt_uses_balance_delta_when_yellowstone_confirms() {
        let mint = Pubkey::new_unique();
        let token_account = Pubkey::new_unique();
        let expected_pre_submit_balance = 1_750_000;
        let observed_post_submit_balance = 1_000_000;
        let (rpc_url, token_balance_requests, _account_info_requests) =
            spawn_token_balance_rpc_server(token_account, observed_post_submit_balance).await;
        let live = test_live_sell_handle_with_sender(
            rpc_url,
            Arc::new(AccountStateReducer::new()),
            "test://yellowstone-confirmed",
        );
        let submission = SenderTransactionSubmission {
            signature: Signature::new_unique(),
        };

        let confirmation = confirm_sender_sell_attempt_with_timeout(
            &live,
            "candidate".to_string(),
            mint,
            Some(token_account),
            expected_pre_submit_balance,
            &submission,
            250,
        )
        .await;

        assert_eq!(
            confirmation,
            SenderSellAttemptConfirmation::Confirmed {
                source: "balance_delta",
                landed_slot: Some(777),
            }
        );
        assert!(
            token_balance_requests.load(Ordering::Relaxed) > 0,
            "SELL confirmation should poll the token balance before trusting Yellowstone alone"
        );
    }

    #[test]
    fn resolve_live_exit_tip_lamports_caps_buy_sized_tip() {
        assert_eq!(
            resolve_live_exit_tip_lamports(2_000_000),
            LIVE_EXIT_MAX_TIP_LAMPORTS
        );
    }

    #[test]
    fn resolve_live_exit_tip_lamports_raises_small_tip_to_floor() {
        assert_eq!(
            resolve_live_exit_tip_lamports(10_000),
            LIVE_EXIT_MIN_TIP_LAMPORTS
        );
    }

    #[test]
    fn resolve_live_exit_tip_lamports_preserves_midrange_tip() {
        assert_eq!(resolve_live_exit_tip_lamports(450_000), 450_000);
    }

    #[tokio::test]
    async fn monitor_live_exit_session_confirms_take_profit_full_exit() {
        let mint = Pubkey::new_unique();
        let (rpc_url, _request_count) = spawn_sequenced_blockhash_rpc_server(vec![
            solana_sdk::hash::Hash::new_unique(),
            solana_sdk::hash::Hash::new_unique(),
            solana_sdk::hash::Hash::new_unique(),
            solana_sdk::hash::Hash::new_unique(),
        ])
        .await;
        let account_state_core = Arc::new(AccountStateReducer::new());
        apply_canonical_update(&account_state_core, mint, 11_000_000_000, 1_000_000_000_000);
        let live = test_live_sell_handle(rpc_url, account_state_core);
        let mut session = seeded_live_exit_session(mint, 10_000_000, 1_000_000);

        monitor_live_exit_session(&live, &mut session, 2_000)
            .await
            .expect("take-profit exit should confirm");

        assert_eq!(session.status, LiveExitStatus::ExitConfirmed);
        assert_eq!(
            session.terminal_reason.as_deref(),
            Some("take_profit_confirmed")
        );
        assert!(session.exit_signature.is_some());
        assert!(session.exit_landed_slot.is_some());
    }

    #[tokio::test]
    async fn monitor_live_exit_session_confirms_stop_loss_full_exit() {
        let mint = Pubkey::new_unique();
        let (rpc_url, _request_count) = spawn_sequenced_blockhash_rpc_server(vec![
            solana_sdk::hash::Hash::new_unique(),
            solana_sdk::hash::Hash::new_unique(),
            solana_sdk::hash::Hash::new_unique(),
            solana_sdk::hash::Hash::new_unique(),
        ])
        .await;
        let account_state_core = Arc::new(AccountStateReducer::new());
        apply_canonical_update(&account_state_core, mint, 8_900_000_000, 1_000_000_000_000);
        let live = test_live_sell_handle(rpc_url, account_state_core);
        let mut session = seeded_live_exit_session(mint, 10_000_000, 1_000_000);

        monitor_live_exit_session(&live, &mut session, 2_000)
            .await
            .expect("stop-loss exit should confirm");

        assert_eq!(session.status, LiveExitStatus::ExitConfirmed);
        assert_eq!(
            session.terminal_reason.as_deref(),
            Some("stop_loss_confirmed")
        );
        assert!(session.exit_signature.is_some());
        assert!(session.exit_landed_slot.is_some());
    }

    #[tokio::test]
    async fn initialize_live_exit_session_fails_closed_on_invalid_buy_signature() {
        let mint = Pubkey::new_unique();
        let live = test_live_sell_handle(
            "http://127.0.0.1:1".to_string(),
            Arc::new(AccountStateReducer::new()),
        );
        let mut session = LiveExitSession::new(
            "candidate".to_string(),
            Pubkey::new_unique(),
            mint,
            Some(Pubkey::new_unique()),
            "not-a-signature".to_string(),
            None,
            2_000_000,
            Some(PositionSlotId::derive(&Pubkey::new_unique(), &mint)),
        );

        let err = initialize_live_exit_session(
            &live,
            &mut session,
            None,
            &PostBuyRuntimeConfig::default(),
        )
        .await
        .expect_err("invalid buy signature should fail closed");

        assert_eq!(err.0, LiveExitStatus::LifecycleAbortedWithReason);
        assert!(err.1.contains("invalid_buy_signature"));
    }

    #[tokio::test]
    async fn resolve_live_exit_wallet_position_uses_visible_ata_without_mint_lookup() {
        let mint = Pubkey::new_unique();
        let payer = Arc::new(Keypair::new());
        let expected_amount = 42_500_000;
        let expected_ata =
            spl_associated_token_account::get_associated_token_address_with_program_id(
                &payer.pubkey(),
                &mint,
                &LIVE_EXIT_TOKEN_2022_PROGRAM_ID,
            );
        let (rpc_url, token_balance_requests, account_info_requests) =
            spawn_token_balance_rpc_server(expected_ata, expected_amount).await;
        let rpc_client = Arc::new(AsyncRpcClient::new(rpc_url));
        let live = LiveSellHandle {
            rpc_client: Arc::clone(&rpc_client),
            live_tx_sender: Arc::new(
                LiveTxSender::new(crate::components::live_tx_sender::LiveTxSenderConfig::new(
                    "test://sender-success",
                    "http://127.0.0.1:18081",
                    "test://yellowstone-confirmed",
                    "test-yellowstone-token",
                ))
                .expect("test live tx sender"),
            ),
            payer,
            account_state_core: Arc::new(AccountStateReducer::new()),
            shadow_ledger: Arc::new(ShadowLedger::new()),
        };
        let session = LiveExitSession::new(
            "candidate".to_string(),
            Pubkey::new_unique(),
            mint,
            Some(Pubkey::new_unique()),
            Signature::new_unique().to_string(),
            None,
            2_000_000,
            Some(PositionSlotId::derive(&Pubkey::new_unique(), &mint)),
        );

        let position = resolve_live_exit_wallet_position_with_retry(&live, &session)
            .await
            .expect("wallet position should resolve from visible ATA");

        assert_eq!(position.token_account, expected_ata);
        assert_eq!(position.token_program, LIVE_EXIT_TOKEN_2022_PROGRAM_ID);
        assert_eq!(position.token_amount, expected_amount);
        assert!(
            token_balance_requests.load(Ordering::Relaxed) > 0,
            "resolver should query token-account balance on the visible ATA"
        );
        assert_eq!(
            account_info_requests.load(Ordering::Relaxed),
            0,
            "resolver must not fall back to direct mint account lookup"
        );
    }

    #[tokio::test]
    async fn post_buy_runtime_drains_late_post_buy_submitted_after_shutdown() {
        let tmp_dir = tempfile::tempdir().expect("temp dir");
        let events_dir = tmp_dir.path().join("events");
        std::fs::create_dir_all(&events_dir).expect("create events dir");

        let (event_tx, event_rx) = create_event_bus();
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let config = PostBuyRuntimeConfig {
            events_output_path: events_dir.clone(),
            paper_fill_delay_min_ms: 10,
            paper_fill_delay_max_ms: 20,
            tick_interval_ms: 10,
            max_ticks_before_exit: 2,
            execution_mode: "paper".to_string(),
            aem_t_s: 1,
            max_concurrent_positions: 1,
            position_limit_tracker: None,
            live_sell: None,
            live_position_registry: None,
            slippage_tolerance: 0.20,
            live_exit_take_profit_pct: 0.02,
            live_exit_stop_loss_pct: 0.02,
            shadow_ledger: None,
            account_state_core: None,
            shadow_lifecycle_log_path: None,
            probe_lifecycle_log_path: None,
        };

        let runtime_handle = tokio::spawn(run(event_rx, shutdown_rx, None, config));
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;

        shutdown_tx.send(()).expect("send shutdown");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let pool_amm_id = Pubkey::new_unique().to_string();
        let base_mint = Pubkey::new_unique().to_string();
        let signature = "late_shutdown_sig";
        let expected_candidate_id = format!("{}_{}_{}", base_mint, pool_amm_id, signature);

        event_tx
            .send(GhostEvent::post_buy_submitted(
                pool_amm_id.clone(),
                base_mint.clone(),
                signature,
                0.5,
                0,
                "paper",
                1,
                None,
                PostBuySource::LiveBuy,
                None,
                None,
                None,
                None,
            ))
            .expect("send post-buy event during shutdown drain");

        tokio::time::timeout(std::time::Duration::from_secs(15), runtime_handle)
            .await
            .expect("runtime should finish")
            .expect("runtime task should join");

        let mut saw_candidate = false;
        let mut saw_closed = false;
        if let Ok(entries) = std::fs::read_dir(&events_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "jsonl") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for line in content.lines() {
                            if let Ok(event) = serde_json::from_str::<ExecutionEvent>(line) {
                                if event.envelope.candidate_id == expected_candidate_id {
                                    saw_candidate = true;
                                    if matches!(event.kind, EventKind::PositionClosed(_)) {
                                        saw_closed = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        assert!(
            saw_candidate,
            "late shutdown PostBuySubmitted should still emit lifecycle events"
        );
        assert!(
            saw_closed,
            "late shutdown PostBuySubmitted should still complete before exit"
        );
    }

    #[tokio::test]
    async fn post_buy_runtime_direct_handoff_survives_broadcast_lag() {
        let tmp_dir = tempfile::tempdir().expect("temp dir");
        let events_dir = tmp_dir.path().join("events");
        std::fs::create_dir_all(&events_dir).expect("create events dir");

        let (event_tx, _event_rx) = create_event_bus_with_capacity(1);
        let event_rx = event_tx.subscribe();
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let (direct_tx, direct_rx) = create_direct_post_buy_handoff_channel();

        let config = PostBuyRuntimeConfig {
            events_output_path: events_dir.clone(),
            paper_fill_delay_min_ms: 10,
            paper_fill_delay_max_ms: 20,
            tick_interval_ms: 10,
            max_ticks_before_exit: 2,
            execution_mode: "paper".to_string(),
            aem_t_s: 1,
            max_concurrent_positions: 1,
            position_limit_tracker: None,
            live_sell: None,
            live_position_registry: None,
            slippage_tolerance: 0.20,
            live_exit_take_profit_pct: 0.02,
            live_exit_stop_loss_pct: 0.02,
            shadow_ledger: None,
            account_state_core: None,
            shadow_lifecycle_log_path: None,
            probe_lifecycle_log_path: None,
        };

        event_tx
            .send(GhostEvent::transaction_sent(
                "noise-sig-1",
                None,
                "lag_noise",
            ))
            .expect("send noise 1");
        event_tx
            .send(GhostEvent::transaction_sent(
                "noise-sig-2",
                None,
                "lag_noise",
            ))
            .expect("send noise 2");

        let pool_amm_id = Pubkey::new_unique().to_string();
        let base_mint = Pubkey::new_unique().to_string();
        let signature = "direct_handoff_sig";
        let expected_candidate_id = format!("{}_{}_{}", base_mint, pool_amm_id, signature);
        let post_buy = GhostEvent::post_buy_submitted(
            pool_amm_id,
            base_mint,
            signature,
            0.25,
            0,
            "paper",
            7,
            None,
            PostBuySource::LiveBuy,
            None,
            None,
            None,
            None,
        );

        let runtime_handle = tokio::spawn(run(event_rx, shutdown_rx, Some(direct_rx), config));
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;

        event_tx
            .send(post_buy.clone())
            .expect("send broadcast handoff");
        direct_tx
            .send(DirectPostBuyHandoff::without_ack(post_buy))
            .expect("send direct handoff");

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        drop(direct_tx);
        let _ = shutdown_tx.send(());
        tokio::time::timeout(std::time::Duration::from_secs(15), runtime_handle)
            .await
            .expect("runtime should finish")
            .expect("runtime task should join");

        let mut candidate_hits = 0usize;
        if let Ok(entries) = std::fs::read_dir(&events_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "jsonl") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for line in content.lines() {
                            if let Ok(event) = serde_json::from_str::<ExecutionEvent>(line) {
                                if event.envelope.candidate_id == expected_candidate_id {
                                    candidate_hits += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        assert!(
            candidate_hits > 0,
            "direct handoff should preserve lifecycle even when broadcast path lagged"
        );
    }

    #[tokio::test]
    async fn post_buy_runtime_direct_handoff_survives_broadcast_closure() {
        let tmp_dir = tempfile::tempdir().expect("temp dir");
        let events_dir = tmp_dir.path().join("events");
        std::fs::create_dir_all(&events_dir).expect("create events dir");

        let (event_tx, _event_rx) = create_event_bus();
        let event_rx = event_tx.subscribe();
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let (direct_tx, direct_rx) = create_direct_post_buy_handoff_channel();

        let config = PostBuyRuntimeConfig {
            events_output_path: events_dir.clone(),
            paper_fill_delay_min_ms: 10,
            paper_fill_delay_max_ms: 20,
            tick_interval_ms: 10,
            max_ticks_before_exit: 2,
            execution_mode: "paper".to_string(),
            aem_t_s: 1,
            max_concurrent_positions: 1,
            position_limit_tracker: None,
            live_sell: None,
            live_position_registry: None,
            slippage_tolerance: 0.20,
            live_exit_take_profit_pct: 0.02,
            live_exit_stop_loss_pct: 0.02,
            shadow_ledger: None,
            account_state_core: None,
            shadow_lifecycle_log_path: None,
            probe_lifecycle_log_path: None,
        };

        drop(event_tx);
        let runtime_handle = tokio::spawn(run(event_rx, shutdown_rx, Some(direct_rx), config));
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;

        let pool_amm_id = Pubkey::new_unique().to_string();
        let base_mint = Pubkey::new_unique().to_string();
        let signature = "direct_handoff_closed_sig";
        let expected_candidate_id = format!("{}_{}_{}", base_mint, pool_amm_id, signature);
        direct_tx
            .send(DirectPostBuyHandoff::without_ack(
                GhostEvent::post_buy_submitted(
                    pool_amm_id,
                    base_mint,
                    signature,
                    0.15,
                    0,
                    "paper",
                    11,
                    None,
                    PostBuySource::LiveBuy,
                    None,
                    None,
                    None,
                    None,
                ),
            ))
            .expect("send direct handoff");

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        drop(direct_tx);
        let _ = shutdown_tx.send(());
        tokio::time::timeout(std::time::Duration::from_secs(15), runtime_handle)
            .await
            .expect("runtime should finish")
            .expect("runtime task should join");

        let mut saw_candidate = false;
        if let Ok(entries) = std::fs::read_dir(&events_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "jsonl") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for line in content.lines() {
                            if let Ok(event) = serde_json::from_str::<ExecutionEvent>(line) {
                                if event.envelope.candidate_id == expected_candidate_id {
                                    saw_candidate = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        assert!(
            saw_candidate,
            "direct handoff should preserve lifecycle even when broadcast transport is closed"
        );
    }

    #[test]
    fn shadow_entry_price_from_post_buy_normalizes_raw_token_amount_to_sol_per_token() {
        let price =
            shadow_entry_price_from_post_buy(0.007, Some(250_000)).expect("shadow entry price");
        assert!((price - 0.028).abs() < 1e-12);
    }

    #[tokio::test]
    async fn shadow_handoff_registers_canonical_monitoring_position() {
        let tmp_dir = tempfile::tempdir().expect("temp dir");
        let events_dir = tmp_dir.path().join("events");
        let lifecycle_log_path = tmp_dir.path().join("shadow_lifecycle.jsonl");
        std::fs::create_dir_all(&events_dir).expect("create events dir");

        let writer_config = EventWriterConfig {
            output_dir: events_dir.to_string_lossy().into_owned(),
            enable_aem_ticks: true,
            enable_optional_events: true,
            flush_interval_ms: 10,
            ..EventWriterConfig::default()
        };
        let emitter = Arc::new(
            EventEmitter::new(writer_config, "test-shadow-run".to_string(), Lane::Shadow)
                .expect("shadow emitter"),
        );
        let config = PostBuyRuntimeConfig {
            execution_mode: "shadow".to_string(),
            shadow_ledger: Some(Arc::new(ShadowLedger::new())),
            shadow_lifecycle_log_path: Some(lifecycle_log_path),
            ..PostBuyRuntimeConfig::default()
        };
        let guardian_config = build_shadow_guardian_config(&config);
        let (signal_tx, _signal_rx) = mpsc::channel(guardian_config.signal_channel_buffer.max(1));
        let runtime_router = Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            RwLock::new(ShadowPositionBook::new()),
        )));
        let mut monitoring_engine = MonitoringEngine::new(
            guardian_config,
            config
                .shadow_ledger
                .clone()
                .expect("shadow ledger for canonical handoff"),
            signal_tx,
        );
        monitoring_engine.set_position_router(runtime_router);
        monitoring_engine.set_event_emitter(Arc::clone(&emitter));
        monitoring_engine.set_shadow_lifecycle_log_path(config.shadow_lifecycle_log_path.clone());
        let monitoring_engine = Arc::new(monitoring_engine);

        let pool_amm_id = Pubkey::new_unique().to_string();
        let base_mint = Pubkey::new_unique().to_string();
        let candidate_id = format!("{}_{}_{}", base_mint, pool_amm_id, 1234);
        let handoff = handle_shadow_post_buy_handoff(
            Some(&monitoring_engine),
            &candidate_id,
            &pool_amm_id,
            &base_mint,
            0.25,
            Some(250_000),
            Some(777),
            9,
            PositionJoinMetadata::default(),
        )
        .await;

        assert_eq!(handoff.ack, DirectPostBuyHandoffAck::Accepted);
        assert_eq!(monitoring_engine.active_position_count(), 1);
        emitter.flush().expect("flush emitter");

        let mut saw_position_opened = false;
        if let Ok(entries) = std::fs::read_dir(&events_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "jsonl") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for line in content.lines() {
                            if let Ok(event) = serde_json::from_str::<ExecutionEvent>(line) {
                                if event.envelope.candidate_id == candidate_id
                                    && matches!(event.kind, EventKind::PositionOpened(_))
                                {
                                    saw_position_opened = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        assert!(
            saw_position_opened,
            "shadow handoff must emit canonical shadow PositionOpened instrumentation"
        );
    }

    #[tokio::test]
    async fn probe_handoff_uses_isolated_probe_monitor_and_lifecycle_path() {
        let tmp_dir = tempfile::tempdir().expect("temp dir");
        let events_dir = tmp_dir.path().join("events");
        let shadow_lifecycle_log_path = tmp_dir.path().join("shadow_lifecycle.jsonl");
        let probe_lifecycle_log_path = tmp_dir.path().join("probe_shadow_lifecycle.jsonl");
        std::fs::create_dir_all(&events_dir).expect("create events dir");

        let writer_config = EventWriterConfig {
            output_dir: events_dir.to_string_lossy().into_owned(),
            enable_optional_events: true,
            flush_interval_ms: 10,
            ..EventWriterConfig::default()
        };
        let emitter = Arc::new(
            EventEmitter::new(writer_config, "test-probe-run".to_string(), Lane::Paper)
                .expect("paper emitter"),
        );
        let quote_provider = Arc::new(RwLock::new(ExecutableQuoteProvider::new(
            QuoteProviderConfig {
                max_quote_age_ms: 5_000,
                ring_buffer_size: 16,
                generation_interval_ms: 100,
                stale_warning_threshold_ms: 3_000,
            },
        )));
        let lifecycle = Arc::new(PaperPositionLifecycle::new(
            PaperLifecycleConfig {
                fill_delay_min_ms: 10,
                fill_delay_max_ms: 20,
                tick_interval_ms: 10,
                max_ticks: 2,
                aem_t_s: 1,
                max_open_positions: 1,
            },
            emitter,
            quote_provider,
        ));

        let shadow_ledger = Arc::new(ShadowLedger::new());
        let config = PostBuyRuntimeConfig {
            execution_mode: "shadow".to_string(),
            shadow_ledger: Some(Arc::clone(&shadow_ledger)),
            shadow_lifecycle_log_path: Some(shadow_lifecycle_log_path.clone()),
            probe_lifecycle_log_path: Some(probe_lifecycle_log_path.clone()),
            ..PostBuyRuntimeConfig::default()
        };
        let guardian_config = build_shadow_guardian_config(&config);
        let (shadow_signal_tx, _shadow_signal_rx) =
            mpsc::channel(guardian_config.signal_channel_buffer.max(1));
        let mut shadow_monitor = MonitoringEngine::new(
            guardian_config.clone(),
            Arc::clone(&shadow_ledger),
            shadow_signal_tx,
        );
        shadow_monitor.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(
            Arc::new(RwLock::new(ShadowPositionBook::new())),
        )));
        shadow_monitor.set_shadow_lifecycle_log_path(Some(shadow_lifecycle_log_path.clone()));
        let shadow_monitor = Arc::new(shadow_monitor);

        let (probe_signal_tx, _probe_signal_rx) =
            mpsc::channel(guardian_config.signal_channel_buffer.max(1));
        let mut probe_monitor =
            MonitoringEngine::new(guardian_config, Arc::clone(&shadow_ledger), probe_signal_tx);
        probe_monitor.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(
            Arc::new(RwLock::new(ShadowPositionBook::new())),
        )));
        probe_monitor.set_shadow_lifecycle_log_path(Some(probe_lifecycle_log_path.clone()));
        let probe_monitor = Arc::new(probe_monitor);

        let pool_amm_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let probe_id = "probe-lifecycle-handoff";
        let join_metadata = crate::events::ExecutionJoinMetadata {
            ab_record_id: Some("pool:1000:1200:REJECT".to_string()),
            source_ab_record_id: Some("pool:1000:1200:REJECT".to_string()),
            probe_id: Some(probe_id.to_string()),
            dispatch_source: Some("counterfactual_shadow_probe".to_string()),
            collection_plane: Some("counterfactual_shadow_probe".to_string()),
            probe_plane: Some("p37_shadow_probe".to_string()),
            v3_feature_snapshot_hash: Some("feature-hash".to_string()),
            v3_policy_config_hash: Some("policy-hash".to_string()),
            decision_plane: Some("v3_mfs_replay".to_string()),
            rollout_namespace: Some("j4-test".to_string()),
            ..Default::default()
        };
        let event = GhostEvent::post_buy_submitted(
            pool_amm_id.to_string(),
            base_mint.to_string(),
            "probe-sig",
            0.007,
            0,
            "probe",
            1,
            None,
            PostBuySource::CounterfactualShadowProbe,
            Some(1),
            Some(250_000),
            Some(777),
            None,
        )
        .with_execution_join_metadata(join_metadata);

        let mut epoch_counter = 1;
        let mut lifecycle_handles = Vec::new();
        let mut recent_handoffs = RecentPostBuyCache::default();
        let ack = handle_post_buy_event(
            event,
            &config,
            &lifecycle,
            Some(&shadow_monitor),
            Some(&probe_monitor),
            &mut epoch_counter,
            &mut lifecycle_handles,
            &mut recent_handoffs,
        )
        .await;

        assert_eq!(ack, DirectPostBuyHandoffAck::Accepted);
        assert_eq!(shadow_monitor.active_position_count(), 0);
        assert_eq!(probe_monitor.active_position_count(), 1);
        assert!(lifecycle_handles.is_empty());

        probe_monitor.unregister_position(&base_mint);
        assert_eq!(probe_monitor.active_position_count(), 0);

        let probe_rows =
            std::fs::read_to_string(&probe_lifecycle_log_path).expect("probe lifecycle row");
        let first_row: serde_json::Value =
            serde_json::from_str(probe_rows.lines().next().expect("first row"))
                .expect("valid probe lifecycle json");
        assert_eq!(first_row["probe_id"], probe_id);
        assert_eq!(first_row["dispatch_source"], "counterfactual_shadow_probe");
        assert_eq!(
            first_row["position_id"],
            format!("probe-position:{probe_id}")
        );
        assert!(
            !shadow_lifecycle_log_path.exists()
                || std::fs::read_to_string(&shadow_lifecycle_log_path)
                    .unwrap_or_default()
                    .trim()
                    .is_empty(),
            "probe lifecycle must not write into canonical shadow lifecycle path"
        );
    }

    #[tokio::test]
    async fn shadow_handoff_rejects_when_monitoring_engine_refuses_position() {
        let config = PostBuyRuntimeConfig {
            execution_mode: "shadow".to_string(),
            shadow_ledger: Some(Arc::new(ShadowLedger::new())),
            max_concurrent_positions: 1,
            ..PostBuyRuntimeConfig::default()
        };
        let guardian_config = build_shadow_guardian_config(&config);
        let (signal_tx, _signal_rx) = mpsc::channel(guardian_config.signal_channel_buffer.max(1));
        let runtime_router = Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            RwLock::new(ShadowPositionBook::new()),
        )));
        let mut monitoring_engine = MonitoringEngine::new(
            guardian_config,
            config
                .shadow_ledger
                .clone()
                .expect("shadow ledger for canonical handoff"),
            signal_tx,
        );
        monitoring_engine.set_position_router(runtime_router);
        let monitoring_engine = Arc::new(monitoring_engine);

        let first_pool = Pubkey::new_unique().to_string();
        let first_mint = Pubkey::new_unique().to_string();
        let first_candidate = format!("{}_{}_{}", first_mint, first_pool, 1);
        let first = handle_shadow_post_buy_handoff(
            Some(&monitoring_engine),
            &first_candidate,
            &first_pool,
            &first_mint,
            0.25,
            Some(250_000),
            Some(111),
            1,
            PositionJoinMetadata::default(),
        )
        .await;
        assert_eq!(first.ack, DirectPostBuyHandoffAck::Accepted);

        let second_pool = Pubkey::new_unique().to_string();
        let second_mint = Pubkey::new_unique().to_string();
        let second_candidate = format!("{}_{}_{}", second_mint, second_pool, 2);
        let second = handle_shadow_post_buy_handoff(
            Some(&monitoring_engine),
            &second_candidate,
            &second_pool,
            &second_mint,
            0.50,
            Some(500_000),
            Some(222),
            2,
            PositionJoinMetadata::default(),
        )
        .await;
        assert_eq!(
            second.ack,
            DirectPostBuyHandoffAck::Rejected("monitoring_rejected")
        );
        assert_eq!(monitoring_engine.active_position_count(), 1);
    }

    #[tokio::test]
    async fn shadow_slot_release_watcher_releases_reserved_position_slot_after_close() {
        let config = PostBuyRuntimeConfig {
            execution_mode: "shadow".to_string(),
            shadow_ledger: Some(Arc::new(ShadowLedger::new())),
            max_concurrent_positions: 1,
            ..PostBuyRuntimeConfig::default()
        };
        let guardian_config = build_shadow_guardian_config(&config);
        let (signal_tx, _signal_rx) = mpsc::channel(guardian_config.signal_channel_buffer.max(1));
        let runtime_router = Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            RwLock::new(ShadowPositionBook::new()),
        )));
        let mut monitoring_engine = MonitoringEngine::new(
            guardian_config,
            config
                .shadow_ledger
                .clone()
                .expect("shadow ledger for canonical handoff"),
            signal_tx,
        );
        monitoring_engine.set_position_router(runtime_router);
        let monitoring_engine = Arc::new(monitoring_engine);

        let pool_amm_id = Pubkey::new_unique().to_string();
        let mint_pubkey = Pubkey::new_unique();
        let base_mint = mint_pubkey.to_string();
        let candidate_id = format!("{}_{}_{}", base_mint, pool_amm_id, 1234);
        let handoff = handle_shadow_post_buy_handoff(
            Some(&monitoring_engine),
            &candidate_id,
            &pool_amm_id,
            &base_mint,
            0.25,
            Some(250_000),
            Some(777),
            9,
            PositionJoinMetadata::default(),
        )
        .await;
        assert_eq!(handoff.ack, DirectPostBuyHandoffAck::Accepted);

        let tracker = PositionLimitTracker::new(1);
        let slot_owner = Pubkey::new_unique();
        let slot_id = PositionSlotId::derive(&slot_owner, &mint_pubkey);
        tracker
            .register_existing(slot_id, pool_amm_id.clone(), base_mint.clone())
            .expect("slot must register");
        assert_eq!(tracker.active_positions(), 1);

        let watcher = spawn_shadow_slot_release_watcher(
            Arc::clone(&monitoring_engine),
            tracker.clone(),
            slot_id,
            mint_pubkey,
            candidate_id,
            10,
        );

        monitoring_engine.unregister_position(&mint_pubkey);
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if tracker.active_positions() == 0 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("watcher should release slot after close");
        watcher.await.expect("watcher should finish");
        assert_eq!(tracker.active_positions(), 0);
    }

    #[tokio::test]
    async fn shadow_handoff_waits_for_delayed_canonical_snapshot_before_registration() {
        let config = PostBuyRuntimeConfig {
            execution_mode: "shadow".to_string(),
            shadow_ledger: Some(Arc::new(ShadowLedger::new())),
            ..PostBuyRuntimeConfig::default()
        };
        let guardian_config = build_shadow_guardian_config(&config);
        let (signal_tx, _signal_rx) = mpsc::channel(guardian_config.signal_channel_buffer.max(1));
        let runtime_router = Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
            RwLock::new(ShadowPositionBook::new()),
        )));
        let mut monitoring_engine = MonitoringEngine::new(
            guardian_config,
            config
                .shadow_ledger
                .clone()
                .expect("shadow ledger for canonical handoff"),
            signal_tx,
        );
        let account_state_core = Arc::new(AccountStateReducer::new());
        monitoring_engine.set_account_state_core(Arc::clone(&account_state_core));
        monitoring_engine.set_position_router(runtime_router);
        let monitoring_engine = Arc::new(monitoring_engine);

        let pool_amm_id = Pubkey::new_unique().to_string();
        let mint_pubkey = Pubkey::new_unique();
        let base_mint = mint_pubkey.to_string();
        let candidate_id = format!("{}_{}_{}", base_mint, pool_amm_id, 1234);
        let landed_slot = 1u64;

        let delayed_core = Arc::clone(&account_state_core);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(75)).await;
            apply_canonical_update(
                &delayed_core,
                mint_pubkey,
                30_000_000_000,
                1_000_000_000_000,
            );
        });

        let started = Instant::now();
        let handoff = handle_shadow_post_buy_handoff(
            Some(&monitoring_engine),
            &candidate_id,
            &pool_amm_id,
            &base_mint,
            0.25,
            Some(250_000),
            Some(landed_slot),
            9,
            PositionJoinMetadata::default(),
        )
        .await;

        assert_eq!(handoff.ack, DirectPostBuyHandoffAck::Accepted);
        assert!(
            started.elapsed() >= Duration::from_millis(50),
            "shadow handoff should wait for delayed canonical snapshot instead of registering immediately"
        );
        assert_eq!(monitoring_engine.active_position_count(), 1);
    }
}
