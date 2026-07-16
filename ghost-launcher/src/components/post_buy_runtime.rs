//! PostBuyRuntime — thin adapter from ghost-launcher handoff events to the canonical
//! post-buy runtime for each lane.
//!
//! Listens on the event bus for `PostBuySubmitted` events and routes them:
//!
//! - **lane == "live"** + `live_sell` configured: persists confirmed BUY entry metadata, monitors
//!   canonical price, and executes a single 100% SELL via Helius Sender when the configured
//!   dormant-live take-profit or stop-loss threshold is hit.
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
use crate::events::{
    EventBusReceiver, GhostEvent, PostBuySource, RuntimePlane, ShadowV2EntryBoundaryPayload,
};
use ghost_brain::config::ghost_brain_config::ShadowV2BurninConfig;
use ghost_brain::events::{EventEmitter, EventWriterConfig};
use ghost_brain::execution::paper_lifecycle::{PaperLifecycleConfig, PaperPositionLifecycle};
use ghost_brain::execution::{CandidateRef, Lane};
use ghost_brain::guardian::post_buy::engine::{PositionEventContext, PositionJoinMetadata};
use ghost_brain::guardian::post_buy::shadow_v2::{
    executable_dynamic_exit_candidate_policies_from_labels_v1, ClockDomain, ClockedTimestamp,
    EventOrderComponent, EventOrderKey, ExecutableDynamicExitEvidenceV1,
    ExecutableDynamicExitObservationV1, ExecutableDynamicExitPolicyEvaluatorV1, MeasurementGrade,
    PoolStateSampleV2, ShadowEntryAttemptV2, ShadowEntryFillModelConfig, ShadowEntryFillV2,
    ShadowPathSampleV2, ShadowPathSamplingModeV2, ShadowPathSamplingReasonV2, ShadowPositionV2,
    ShadowV2Envelope, ShadowV2Record, ShadowV2ValidationEvidenceStatus, ShadowV2ValidationHarness,
    ShadowV2ValidationHarnessConfig, SimulationLevel, TemporalClass,
    SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
};
use ghost_brain::guardian::post_buy::{
    validate_exit_policy_v1_config, CrashGuardMode, ExitPolicyV1Status, MonitoringEngine,
    PositionRuntimeRouter, PostBuyGuardianConfig, ShadowPositionBook, ShadowTerminalDisposition,
    SignalRouter,
};
use ghost_brain::quotes::{ExecutableQuoteProvider, QuoteProviderConfig};
use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_core::{ShadowV2PoolPhase, LAMPORTS_PER_SOL};
use parking_lot::Mutex as ParkingMutex;
use seer::parse_curve_from_account;
use solana_client::client_error::ClientError;
use solana_client::nonblocking::rpc_client::RpcClient as AsyncRpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;
use solana_sdk::transaction::VersionedTransaction;
use std::collections::{HashMap, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
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
const ENTRY_MARKET_ANCHOR_SOURCE_SHADOW_SIMULATION_RPC_CONTEXT: &str =
    "shadow_simulation_rpc_context";
const ENTRY_MARKET_ANCHOR_SOURCE_BUY_LANDED_SLOT: &str = "buy_landed_slot";
const ENTRY_LANDED_SLOT_SOURCE_SYNTHETIC_AFTER_ENTRY_SIMULATION_RPC_SLOT: &str =
    "synthetic_next_slot_after_entry_simulation_rpc_slot";
const ENTRY_LANDED_SLOT_SOURCE_BUY_LANDED_SLOT: &str = "buy_landed_slot";
const SHADOW_V2_ENTRY_FEE_BPS_FALLBACK: u16 = 100;

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

pub type DirectPostBuySender = mpsc::Sender<DirectPostBuyHandoff>;
pub type DirectPostBuyReceiver = mpsc::Receiver<DirectPostBuyHandoff>;

pub fn direct_post_buy_handoff_capacity(max_concurrent_positions: usize) -> usize {
    max_concurrent_positions.saturating_mul(4).clamp(8, 256)
}

pub fn create_direct_post_buy_handoff_channel(
    max_concurrent_positions: usize,
) -> (DirectPostBuySender, DirectPostBuyReceiver) {
    mpsc::channel(direct_post_buy_handoff_capacity(max_concurrent_positions))
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
    /// Effective trigger admission mode. It is copied into this boundary so a
    /// future authoritative CrashGuard cannot be enabled under a live-capable
    /// entry profile by accident.
    pub entry_mode: String,
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
    /// Live take-profit threshold as a fraction of entry price.
    pub live_exit_take_profit_pct: f64,
    /// Live stop-loss threshold as a fraction of entry price.
    pub live_exit_stop_loss_pct: f64,
    /// Complete Guardian configuration loaded from the brain config. Runtime
    /// may only overlay capacity and artifact paths.
    pub shadow_guardian: Option<PostBuyGuardianConfig>,
    /// Canonical ShadowLedger shared with the shadow Guardian runtime.
    pub shadow_ledger: Option<Arc<ShadowLedger>>,
    /// Canonical account-state runtime truth shared with shadow guardian.
    pub account_state_core: Option<Arc<AccountStateReducer>>,
    /// Canonical shadow lifecycle/PnL proof log path derived from execution.shadow.*.
    pub shadow_lifecycle_log_path: Option<PathBuf>,
    /// Counterfactual probe lifecycle proof log path derived from p37_shadow_probe.*.
    pub probe_lifecycle_log_path: Option<PathBuf>,
    /// Optional Shadow V2 logging-only validation config. This must never feed decisions.
    pub shadow_v2_burnin: Option<ShadowV2BurninConfig>,
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
            entry_mode: "dry_run_mock".to_string(),
            aem_t_s: 120,
            max_concurrent_positions: 1,
            position_limit_tracker: None,
            live_sell: None,
            live_position_registry: None,
            slippage_tolerance: 0.20,
            live_exit_take_profit_pct: 0.02,
            live_exit_stop_loss_pct: 0.02,
            shadow_guardian: None,
            shadow_ledger: None,
            account_state_core: None,
            shadow_lifecycle_log_path: None,
            probe_lifecycle_log_path: None,
            shadow_v2_burnin: None,
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

    pub fn validate(&self) -> Result<Option<ExitPolicyV1Status>, String> {
        let crash_guard_authoritative = self.shadow_guardian.as_ref().is_some_and(|guardian| {
            matches!(
                guardian.exit_policy_v1.crash_guard_mode,
                CrashGuardMode::AuthoritativeShadow
            )
        });
        if crash_guard_authoritative {
            if self.execution_mode != "shadow" {
                return Err(
                    "CrashGuard authoritative_shadow requires execution_mode=shadow".to_string(),
                );
            }
            if self.entry_mode != "shadow_only" {
                return Err(
                    "CrashGuard authoritative_shadow requires entry_mode=shadow_only".to_string(),
                );
            }
            if self.live_sell.is_some() {
                return Err(
                    "CrashGuard authoritative_shadow requires live sell dispatch to be disabled"
                        .to_string(),
                );
            }
            if self.shadow_ledger.is_none() {
                return Err(
                    "CrashGuard authoritative_shadow requires the canonical shadow monitor to be wired"
                        .to_string(),
                );
            }
            if self.shadow_lifecycle_log_path.is_none() {
                return Err(
                    "CrashGuard authoritative_shadow requires shadow lifecycle evidence logging"
                        .to_string(),
                );
            }
            if self.events_output_path.as_os_str().is_empty() {
                return Err(
                    "CrashGuard authoritative_shadow requires canonical terminal evidence output"
                        .to_string(),
                );
            }
        }
        if self.execution_mode != "shadow" {
            return Ok(None);
        }

        if self.shadow_guardian.is_none() {
            return Err(
                "shadow mode requires the complete post_buy_guardian configuration".to_string(),
            );
        }
        let guardian = build_shadow_guardian_config(self);
        if !guardian.enabled {
            return Err("shadow mode requires post_buy_guardian.enabled=true".to_string());
        }
        if guardian.aem.enabled {
            return Err(
                "Position Manager Lite V1 requires post_buy_guardian.aem.enabled=false".to_string(),
            );
        }

        validate_exit_policy_v1_config(&guardian)
            .map(Some)
            .map_err(|error| error.to_string())
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

fn init_shadow_v2_validation_harness(
    config: Option<&ShadowV2BurninConfig>,
) -> Result<Option<ShadowV2ValidationHarness>, String> {
    let Some(config) = config.filter(|config| config.enabled) else {
        return Ok(None);
    };
    let Some(harness_config) = ShadowV2ValidationHarnessConfig::from_burnin_config(config)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    ShadowV2ValidationHarness::new(harness_config)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn init_position_manager_terminal_harness(
    config: Option<&ShadowV2BurninConfig>,
    events_output_path: &Path,
    run_id: &str,
) -> Result<Option<ShadowV2ValidationHarness>, String> {
    if config.is_some_and(|config| config.enabled) {
        return init_shadow_v2_validation_harness(config);
    }

    let root = events_output_path.join("position_manager_terminal_truth_v2");
    let harness_config = ShadowV2ValidationHarnessConfig::new(
        run_id,
        root.join("shadow_position_event_v2.jsonl"),
        root.join("shadow_replay_v2.jsonl"),
        root.join("shadow_lifecycle_v2.jsonl"),
        root.join("shadow_path_density_v2.jsonl"),
    );
    ShadowV2ValidationHarness::new(harness_config)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn init_probe_position_manager_terminal_harness(
    events_output_path: &Path,
    run_id: &str,
) -> Result<ShadowV2ValidationHarness, String> {
    let root = events_output_path.join("position_manager_probe_terminal_truth_v2");
    let harness_config = ShadowV2ValidationHarnessConfig::new(
        run_id,
        root.join("shadow_position_event_v2.jsonl"),
        root.join("shadow_replay_v2.jsonl"),
        root.join("shadow_lifecycle_v2.jsonl"),
        root.join("shadow_path_density_v2.jsonl"),
    );
    ShadowV2ValidationHarness::new(harness_config).map_err(|error| error.to_string())
}

fn shadow_v2_scope_root(config: &ShadowV2BurninConfig) -> Result<&str, String> {
    config
        .scope_root_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "missing scope_root_path".to_string())
}

fn executable_dynamic_exit_evidence_path(
    config: &ShadowV2BurninConfig,
) -> Result<Option<PathBuf>, String> {
    if !config.executable_dynamic_exit_evidence_enabled {
        return Ok(None);
    }
    if let Some(path) = config
        .executable_dynamic_exit_evidence_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return Ok(Some(PathBuf::from(path)));
    }
    Ok(Some(
        Path::new(shadow_v2_scope_root(config)?).join("executable_dynamic_exit_evidence_v1.jsonl"),
    ))
}

fn append_executable_dynamic_exit_sidecar_row(
    path: &Path,
    row: &ExecutableDynamicExitEvidenceV1,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create sidecar parent {}: {error}",
                parent.display()
            )
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("failed to open sidecar {}: {error}", path.display()))?;
    serde_json::to_writer(&mut file, row)
        .map_err(|error| format!("failed to serialize dynamic-exit sidecar row: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("failed to write dynamic-exit sidecar newline: {error}"))?;
    file.flush()
        .map_err(|error| format!("failed to flush dynamic-exit sidecar: {error}"))?;
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "the sidecar schema requires each immutable identity and evidence input to stay explicit"
)]
fn emit_executable_dynamic_exit_sidecar_rows(
    config: &ShadowV2BurninConfig,
    position_id: &str,
    candidate_id: &str,
    pool_amm_id: &str,
    base_mint: &str,
    entry_fill: &ShadowEntryFillV2,
    entry_token_amount_raw: Option<u64>,
    path_sample: &ShadowPathSampleV2,
    pool_state: &PoolStateSampleV2,
) {
    let path = match executable_dynamic_exit_evidence_path(config) {
        Ok(Some(path)) => path,
        Ok(None) => return,
        Err(error) => {
            ::metrics::counter!(
                "executable_dynamic_exit_evidence_write_failed_total",
                1u64,
                "reason" => "path_resolution"
            );
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                position_id,
                error,
                "PostBuyRuntime: dynamic-exit evidence sidecar path resolution failed; runtime continues"
            );
            return;
        }
    };
    let policies = match executable_dynamic_exit_candidate_policies_from_labels_v1(
        &config.executable_dynamic_exit_candidate_policies,
    ) {
        Ok(policies) => policies,
        Err(error) => {
            ::metrics::counter!(
                "executable_dynamic_exit_evidence_write_failed_total",
                1u64,
                "reason" => "policy_parse"
            );
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                position_id,
                error,
                "PostBuyRuntime: dynamic-exit evidence sidecar policy parse failed; runtime continues"
            );
            return;
        }
    };
    let token_amount_source = if entry_fill.output_amount_raw.is_some() {
        "shadow_entry_fill_v2.output_amount_raw"
    } else if entry_token_amount_raw.is_some() {
        "post_buy_handoff.entry_token_amount_raw"
    } else {
        "MISSING_TOKEN_AMOUNT"
    };
    let mut evaluator = ExecutableDynamicExitPolicyEvaluatorV1::new(
        position_id,
        entry_fill.envelope.event_id.clone(),
        entry_fill.output_amount_raw.or(entry_token_amount_raw),
        entry_fill.fill_amount_tokens,
        entry_fill.fill_amount_sol,
        entry_fill.fill_price,
        token_amount_source,
        policies,
    );
    let rows = evaluator.observe_path_sample(ExecutableDynamicExitObservationV1 {
        run_id: config.run_namespace.as_deref().unwrap_or("UNKNOWN_RUN"),
        candidate_id: Some(candidate_id),
        pool_id: pool_amm_id,
        base_mint,
        path_sample,
        pool_state,
        trigger_observed_at_ms: path_sample.event_order_key.observed_at_wall_ms,
        slippage_bps: entry_fill
            .slippage_tolerance_bps
            .and_then(|value| u16::try_from(value.max(0)).ok())
            .unwrap_or_else(|| slippage_tolerance_to_bps(0.05)),
        fee_bps: entry_fill
            .fee_bps
            .and_then(|value| u16::try_from(value.max(0)).ok())
            .unwrap_or(SHADOW_V2_ENTRY_FEE_BPS_FALLBACK),
    });
    for row in rows {
        if let Err(error) = append_executable_dynamic_exit_sidecar_row(&path, &row) {
            ::metrics::counter!(
                "executable_dynamic_exit_evidence_write_failed_total",
                1u64,
                "reason" => "write"
            );
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                position_id,
                error,
                "PostBuyRuntime: dynamic-exit evidence sidecar write failed; runtime continues"
            );
            return;
        }
    }
}

async fn run_shadow_v2_manifest_command(
    config: &ShadowV2BurninConfig,
    args: &[String],
) -> Result<(), String> {
    let script = config.manifest_audit_script.trim();
    if script.is_empty() {
        return Err("missing manifest_audit_script".to_string());
    }
    let output = tokio::process::Command::new("python3")
        .arg(script)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| format!("failed to run {script}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(format!(
        "{script} exited with status {} stdout={} stderr={}",
        output.status, stdout, stderr
    ))
}

fn shadow_v2_post_run_generation_args(
    config: &ShadowV2BurninConfig,
) -> Result<Vec<String>, String> {
    let scope_root = shadow_v2_scope_root(config)?;
    let post_run_manifest_path = config
        .post_run_manifest_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "missing post_run_manifest_path".to_string())?;
    let run_id = config
        .run_namespace
        .as_deref()
        .map(str::trim)
        .filter(|run_id| !run_id.is_empty())
        .ok_or_else(|| "missing run_namespace".to_string())?;
    let report_path = Path::new(scope_root).join("shadow_v2_manifest_report.csv");
    Ok(vec![
        "--scope-root".to_string(),
        scope_root.to_string(),
        "--manifest-phase".to_string(),
        "post_run".to_string(),
        "--run-id".to_string(),
        run_id.to_string(),
        "--write-manifest".to_string(),
        post_run_manifest_path.to_string(),
        "--write-report-csv".to_string(),
        report_path.to_string_lossy().to_string(),
        "--schema-manifest".to_string(),
        config.required_schema_manifest_path.clone(),
        "--acceptance-gates".to_string(),
        config.acceptance_gates_path.clone(),
        "--executable-dynamic-exit-evidence-enabled".to_string(),
        config.executable_dynamic_exit_evidence_enabled.to_string(),
    ])
}

fn shadow_v2_post_run_verification_args(
    config: &ShadowV2BurninConfig,
) -> Result<Vec<String>, String> {
    let scope_root = shadow_v2_scope_root(config)?;
    Ok(vec![
        "--scope-root".to_string(),
        scope_root.to_string(),
        "--manifest-phase".to_string(),
        "post_run".to_string(),
        "--schema-manifest".to_string(),
        config.required_schema_manifest_path.clone(),
        "--acceptance-gates".to_string(),
        config.acceptance_gates_path.clone(),
        "--executable-dynamic-exit-evidence-enabled".to_string(),
        config.executable_dynamic_exit_evidence_enabled.to_string(),
        "--strict".to_string(),
    ])
}

async fn run_shadow_v2_post_run_manifest_generation_and_audit(
    config: &ShadowV2BurninConfig,
) -> Result<(), String> {
    if !config.enabled {
        return Ok(());
    }
    let generation_args = shadow_v2_post_run_generation_args(config)?;
    run_shadow_v2_manifest_command(config, &generation_args).await?;

    let verification_args = shadow_v2_post_run_verification_args(config)?;
    run_shadow_v2_manifest_command(config, &verification_args).await
}

async fn run_shadow_v2_post_run_manifest_generation_and_audit_with_timeout(
    config: &ShadowV2BurninConfig,
) -> Result<(), String> {
    let budget = Duration::from_millis(config.post_run_manifest_drain_timeout_ms.max(1));
    match tokio::time::timeout(
        budget,
        run_shadow_v2_post_run_manifest_generation_and_audit(config),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(format!(
            "SHADOW_V2_POST_RUN_MANIFEST_DRAIN_TIMEOUT: exceeded {}ms",
            config.post_run_manifest_drain_timeout_ms
        )),
    }
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
    session_tip_lamports.clamp(LIVE_EXIT_MIN_TIP_LAMPORTS, LIVE_EXIT_MAX_TIP_LAMPORTS)
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

fn shadow_entry_timeline_join_metadata(
    mut metadata: PositionJoinMetadata,
    entry_simulation_rpc_slot: Option<u64>,
    buy_landed_slot: Option<u64>,
) -> PositionJoinMetadata {
    let entry_simulation_rpc_slot =
        entry_simulation_rpc_slot.or(metadata.entry_simulation_rpc_slot);

    if let Some(slot) = entry_simulation_rpc_slot {
        metadata.entry_simulation_rpc_slot = Some(slot);
        metadata.entry_market_anchor_slot = Some(slot);
        metadata.entry_market_anchor_tx_signature = None;
        metadata.entry_market_anchor_source =
            Some(ENTRY_MARKET_ANCHOR_SOURCE_SHADOW_SIMULATION_RPC_CONTEXT.to_string());
        metadata.entry_landed_slot = slot.checked_add(1);
        metadata.entry_landed_slot_source = metadata.entry_landed_slot.map(|_| {
            ENTRY_LANDED_SLOT_SOURCE_SYNTHETIC_AFTER_ENTRY_SIMULATION_RPC_SLOT.to_string()
        });
        return metadata;
    }

    if let Some(slot) = buy_landed_slot {
        metadata.entry_market_anchor_slot = Some(slot);
        metadata.entry_market_anchor_source =
            Some(ENTRY_MARKET_ANCHOR_SOURCE_BUY_LANDED_SLOT.to_string());
        metadata.entry_landed_slot = Some(slot);
        metadata.entry_landed_slot_source =
            Some(ENTRY_LANDED_SLOT_SOURCE_BUY_LANDED_SLOT.to_string());
    }

    metadata
}

fn shadow_v2_post_buy_event_order_key(
    slot: Option<u64>,
    signature: Option<&str>,
    event_seq_in_process: u64,
    observed_at_wall_ms: u64,
) -> EventOrderKey {
    shadow_v2_post_buy_event_order_key_with_components(
        slot,
        None,
        signature,
        None,
        None,
        None,
        None,
        event_seq_in_process,
        observed_at_wall_ms,
    )
}

fn shadow_v2_log_message_index_component(
    log_message_index_internal: Option<u32>,
) -> EventOrderComponent<u32> {
    log_message_index_internal
        .map(EventOrderComponent::known)
        .unwrap_or_else(EventOrderComponent::not_applicable)
}

fn shadow_v2_entry_boundary_has_exact_source_join(boundary: &ShadowV2EntryBoundaryPayload) -> bool {
    boundary
        .source_tx_signature
        .as_deref()
        .map(str::trim)
        .is_some_and(|signature| !signature.is_empty())
}

#[allow(clippy::too_many_arguments)]
fn shadow_v2_post_buy_event_order_key_with_components(
    slot: Option<u64>,
    block_time: Option<i64>,
    signature: Option<&str>,
    transaction_index: Option<u32>,
    instruction_index: Option<u32>,
    inner_instruction_index: Option<u32>,
    log_message_index_internal: Option<u32>,
    event_seq_in_process: u64,
    observed_at_wall_ms: u64,
) -> EventOrderKey {
    EventOrderKey {
        slot: slot
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::unknown),
        block_time: block_time
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::unknown),
        signature: signature
            .filter(|signature| !signature.trim().is_empty())
            .map(|signature| EventOrderComponent::known(signature.to_string()))
            .unwrap_or_else(EventOrderComponent::unknown),
        transaction_index_or_unknown: transaction_index
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::unknown),
        instruction_index_or_unknown: instruction_index
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::unknown),
        inner_instruction_index_or_unknown: inner_instruction_index
            .map(EventOrderComponent::known)
            .unwrap_or_else(EventOrderComponent::unknown),
        // Solana has no native EVM-style logIndex. A known value here is
        // reserved for an internal ordinal produced by enumerating
        // meta.logMessages, not for provider-native chain order.
        log_index_or_unknown: shadow_v2_log_message_index_component(log_message_index_internal),
        event_seq_in_process,
        observed_at_wall_ms,
    }
}

fn shadow_v2_entry_boundary_source_order_key(
    boundary: &ShadowV2EntryBoundaryPayload,
    event_seq_in_process: u64,
    observed_at_wall_ms: u64,
) -> EventOrderKey {
    let has_exact_source_join = shadow_v2_entry_boundary_has_exact_source_join(boundary);
    shadow_v2_post_buy_event_order_key_with_components(
        Some(boundary.state_slot),
        has_exact_source_join
            .then_some(boundary.source_block_time)
            .flatten(),
        has_exact_source_join
            .then_some(boundary.source_tx_signature.as_deref())
            .flatten(),
        has_exact_source_join
            .then_some(boundary.source_transaction_index)
            .flatten(),
        has_exact_source_join
            .then_some(boundary.source_instruction_index)
            .flatten(),
        None,
        None,
        event_seq_in_process,
        observed_at_wall_ms,
    )
}

fn shadow_v2_post_buy_event_seq(timestamp_ms: u64, offset: u64) -> u64 {
    timestamp_ms.saturating_mul(10).saturating_add(offset)
}

fn build_shadow_guardian_config(config: &PostBuyRuntimeConfig) -> PostBuyGuardianConfig {
    let mut guardian = config.shadow_guardian.clone().unwrap_or_default();
    guardian.max_monitored_positions = config.max_concurrent_positions;
    guardian
}

fn derive_shadow_exit_replay_log_path(lifecycle_log_path: &Path) -> PathBuf {
    lifecycle_log_path.with_file_name("shadow_exit_replay_v1.jsonl")
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
    ExitConfirmationUnknown,
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
            Self::ExitConfirmationUnknown => "exit_confirmation_unknown",
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
    #[expect(
        clippy::too_many_arguments,
        reason = "live entry facts are independently derived and must not be reconstructed from mutable state"
    )]
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
            (state.price_sol * LAMPORTS_PER_SOL * LIVE_EXIT_PRICE_SOL_SCALE_FACTOR).round() as u64,
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
            if best.is_none_or(|(_, slot)| shadow.last_updated_slot > slot) {
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
    let effective_policy_status = match config.validate() {
        Ok(status) => status,
        Err(error) => {
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                error = %error,
                "PostBuyRuntime: invalid effective Position Manager Lite configuration"
            );
            return;
        }
    };
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
    let shadow_v2_harness = match if config.execution_mode == "shadow" {
        init_position_manager_terminal_harness(
            config.shadow_v2_burnin.as_ref(),
            &config.events_output_path,
            &run_id,
        )
    } else {
        init_shadow_v2_validation_harness(config.shadow_v2_burnin.as_ref())
    } {
        Ok(harness) => harness.map(|harness| Arc::new(ParkingMutex::new(harness))),
        Err(error) => {
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                error = %error,
                validation_harness_status = "FAILED",
                "PostBuyRuntime: SHADOW_V2_VALIDATION_PREFLIGHT_FAILED"
            );
            return;
        }
    };
    let probe_shadow_v2_harness = if config.execution_mode == "shadow"
        && config.probe_lifecycle_log_path.is_some()
    {
        match init_probe_position_manager_terminal_harness(&config.events_output_path, &run_id) {
            Ok(harness) => Some(Arc::new(ParkingMutex::new(harness))),
            Err(error) => {
                warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    error = %error,
                    validation_harness_status = "FAILED",
                    "PostBuyRuntime: PROBE_TERMINAL_TRUTH_PREFLIGHT_FAILED"
                );
                return;
            }
        }
    } else {
        None
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
                let wait_for_timestop_ms = guardian_config.wait_for_timestop_ms();
                let exit_replay_enabled = guardian_config.exit_replay_v1.enabled;
                let (signal_tx, signal_rx) =
                    mpsc::channel(guardian_config.signal_channel_buffer.max(1));
                let runtime_router = Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
                    RwLock::new(ShadowPositionBook::with_time_stop_ms(wait_for_timestop_ms)),
                )));
                let mut monitoring_engine =
                    match MonitoringEngine::try_new(guardian_config, shadow_ledger, signal_tx) {
                        Ok(engine) => engine,
                        Err(error) => {
                            warn!(
                                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                                error = %error,
                                "PostBuyRuntime: shadow Position Manager policy validation failed"
                            );
                            return;
                        }
                    };
                if let Some(account_state_core) = config.account_state_core.clone() {
                    monitoring_engine.set_account_state_core(account_state_core);
                }
                if let Some(shadow_v2_harness) = shadow_v2_harness.as_ref() {
                    monitoring_engine
                        .set_shadow_v2_validation_harness(Arc::clone(shadow_v2_harness));
                }
                monitoring_engine.set_position_router(Arc::clone(&runtime_router));
                monitoring_engine.set_event_emitter(emitter.clone());
                monitoring_engine
                    .set_shadow_lifecycle_log_path(config.shadow_lifecycle_log_path.clone());
                let exit_replay_log_path = if exit_replay_enabled {
                    config
                        .shadow_lifecycle_log_path
                        .as_deref()
                        .map(derive_shadow_exit_replay_log_path)
                } else {
                    None
                };
                monitoring_engine.set_shadow_exit_replay_log_path(exit_replay_log_path);
                let monitoring_engine = Arc::new(monitoring_engine);
                shadow_signal_router_handle = Some(tokio::spawn(
                    SignalRouter::new_observation_only(signal_rx, runtime_router).run(),
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
                let wait_for_timestop_ms = guardian_config.wait_for_timestop_ms();
                let (signal_tx, signal_rx) =
                    mpsc::channel(guardian_config.signal_channel_buffer.max(1));
                let runtime_router = Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
                    RwLock::new(ShadowPositionBook::with_time_stop_ms(wait_for_timestop_ms)),
                )));
                let mut monitoring_engine =
                    match MonitoringEngine::try_new(guardian_config, shadow_ledger, signal_tx) {
                        Ok(engine) => engine,
                        Err(error) => {
                            warn!(
                                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                                error = %error,
                                "PostBuyRuntime: probe Position Manager policy validation failed"
                            );
                            return;
                        }
                    };
                if let Some(account_state_core) = config.account_state_core.clone() {
                    monitoring_engine.set_account_state_core(account_state_core);
                }
                if let Some(probe_shadow_v2_harness) = probe_shadow_v2_harness.as_ref() {
                    monitoring_engine
                        .set_shadow_v2_validation_harness(Arc::clone(probe_shadow_v2_harness));
                }
                monitoring_engine.set_position_router(Arc::clone(&runtime_router));
                monitoring_engine.set_shadow_lifecycle_log_path(Some(probe_lifecycle_log_path));
                let monitoring_engine = Arc::new(monitoring_engine);
                probe_signal_router_handle = Some(tokio::spawn(
                    SignalRouter::new_observation_only(signal_rx, runtime_router).run(),
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

    if let Some(status) = effective_policy_status.as_ref() {
        info!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            policy_id = %status.policy_id,
            policy_version = status.policy_version,
            policy_config_hash = %status.config_hash,
            lane = "shadow",
            take_profit_fraction = status.take_profit_fraction,
            stop_loss_fraction = status.stop_loss_fraction,
            inactivity_timeout_ms = status.inactivity_timeout_ms,
            quote_recovery_ms = status.quote_recovery_ms,
            absolute_max_hold_enabled = status.absolute_max_hold_enabled,
            absolute_max_hold_ms = status.absolute_max_hold_ms,
            crash_guard_mode = ?status.crash_guard_mode,
            crash_window_ms = status.crash_window_ms,
            crash_min_short_window_drop_pct = status.crash_min_short_window_drop_pct,
            crash_min_peak_drawdown_pct = status.crash_min_peak_drawdown_pct,
            crash_min_distinct_slots = status.crash_min_distinct_slots,
            crash_max_sample_age_ms = status.crash_max_sample_age_ms,
            crash_max_executable_return_pct = status.crash_max_executable_return_pct,
            crash_guard_authority = match status.crash_guard_mode {
                CrashGuardMode::Disabled => "disabled",
                CrashGuardMode::ObserveOnly => "observation_only",
                CrashGuardMode::AuthoritativeShadow => "shadow_only",
            },
            guardian_authority = "observation_only",
            aem_authority = "disabled",
            revolver_authority = "disabled",
            live_authority = "disabled",
            "PostBuyRuntime: effective Position Manager Lite V1 configuration"
        );
    }

    info!(
        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
        "PostBuyRuntime adapter started (mode={}, run_id={}, live_sell={}, shadow_guardian={}, probe_guardian={})",
        config.execution_mode,
        run_id,
        config.live_sell.is_some(),
        shadow_monitor.is_some(),
        probe_monitor.is_some(),
    );
    maybe_emit_shadow_v2_validation_smoke_marker(&shadow_v2_harness, &config);

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
                            &shadow_v2_harness,
                        )
                        .await;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        crate::events::record_event_bus_lag("post_buy_runtime", n);
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
                            &shadow_v2_harness,
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
    if let Some(monitor) = shadow_monitor.as_ref() {
        monitor.flush_exit_replay_for_shutdown().await;
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
    if let Some(shadow_v2_burnin) = config
        .shadow_v2_burnin
        .as_ref()
        .filter(|shadow_v2_burnin| shadow_v2_burnin.enabled)
    {
        match run_shadow_v2_post_run_manifest_generation_and_audit_with_timeout(shadow_v2_burnin)
            .await
        {
            Ok(()) => info!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                validation_harness_status = "POST_RUN_MANIFEST_AUDIT_PASS",
                "PostBuyRuntime: Shadow V2 post-run manifest generated and strict-verified"
            ),
            Err(error) if error.contains("SHADOW_V2_POST_RUN_MANIFEST_DRAIN_TIMEOUT") => {
                warn!(
                    runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                    validation_harness_status = "FAILED",
                    error = %error,
                    "PostBuyRuntime: SHADOW_V2_POST_RUN_MANIFEST_DRAIN_TIMEOUT"
                )
            }
            Err(error) => warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                validation_harness_status = "FAILED",
                error = %error,
                "PostBuyRuntime: SHADOW_V2_POST_RUN_MANIFEST_AUDIT_FAILED"
            ),
        }
    }

    info!(
        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
        "PostBuyRuntime exiting"
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "runtime orchestration keeps independently owned dependencies explicit at the handoff boundary"
)]
async fn handle_post_buy_event(
    event: GhostEvent,
    config: &PostBuyRuntimeConfig,
    lifecycle: &Arc<PaperPositionLifecycle>,
    shadow_monitor: Option<&Arc<MonitoringEngine>>,
    probe_monitor: Option<&Arc<MonitoringEngine>>,
    epoch_counter: &mut u64,
    lifecycle_handles: &mut Vec<tokio::task::JoinHandle<()>>,
    recent_handoffs: &mut RecentPostBuyCache,
    shadow_v2_harness: &Option<Arc<ParkingMutex<ShadowV2ValidationHarness>>>,
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
        entry_simulation_rpc_slot,
        entry_opened_at_ms,
        creator_pubkey,
        join_metadata,
        shadow_v2_entry_boundary,
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
        entry_simulation_rpc_slot = ?entry_simulation_rpc_slot,
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
        let position_join_metadata = PositionJoinMetadata {
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
            run_id: join_metadata.run_id.clone(),
            session_id: join_metadata.session_id.clone(),
            brain_config_path: join_metadata.brain_config_path.clone(),
            brain_config_hash: join_metadata.brain_config_hash.clone(),
            ..Default::default()
        };
        let mut handoff = handle_shadow_post_buy_handoff(
            shadow_monitor,
            &candidate_id,
            &pool_amm_id,
            &base_mint,
            amount_sol,
            entry_token_amount_raw,
            buy_landed_slot,
            entry_simulation_rpc_slot,
            entry_opened_at_ms,
            epoch,
            position_join_metadata.clone(),
        )
        .await;
        if matches!(handoff.ack, DirectPostBuyHandoffAck::Accepted) {
            maybe_emit_shadow_v2_position_created(
                shadow_v2_harness,
                config,
                &candidate_id,
                &pool_amm_id,
                &base_mint,
                handoff.position_id.as_deref(),
                buy_landed_slot,
                entry_simulation_rpc_slot,
                entry_opened_at_ms,
                &position_join_metadata,
            );
            maybe_emit_shadow_v2_entry_evidence(
                shadow_v2_harness,
                config,
                &candidate_id,
                &pool_amm_id,
                &base_mint,
                handoff.position_id.as_deref(),
                &signature,
                amount_sol,
                min_tokens_out,
                entry_token_amount_raw,
                buy_landed_slot,
                entry_simulation_rpc_slot,
                entry_opened_at_ms,
                &position_join_metadata,
                shadow_v2_entry_boundary,
            );
            if let (Some(tracker), Some(slot_id)) =
                (config.position_limit_tracker.clone(), position_slot_id)
            {
                if let Some(terminal_rx) = handoff.terminal_rx.take() {
                    lifecycle_handles.push(spawn_shadow_terminal_watcher(
                        terminal_rx,
                        tracker,
                        slot_id,
                        candidate_id.clone(),
                    ));
                } else {
                    ::metrics::counter!(
                        "post_buy_shadow_terminal_total",
                        1u64,
                        "disposition" => "terminal_channel_dropped",
                        "reason" => "terminal_receiver_missing"
                    );
                    let _ = tracker.release(slot_id);
                    warn!(
                        runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                        candidate_id = %candidate_id,
                        slot_id = %slot_id,
                        "PostBuyRuntime: accepted shadow handoff had no terminal receiver; slot released fail-closed for shadow"
                    );
                }
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
            entry_simulation_rpc_slot,
            entry_opened_at_ms,
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
                run_id: join_metadata.run_id.clone(),
                session_id: join_metadata.session_id.clone(),
                brain_config_path: join_metadata.brain_config_path.clone(),
                brain_config_hash: join_metadata.brain_config_hash.clone(),
                ..Default::default()
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

#[expect(
    clippy::too_many_arguments,
    reason = "Shadow V2 projection records explicit join and entry provenance without mutable reconstruction"
)]
fn maybe_emit_shadow_v2_position_created(
    harness: &Option<Arc<ParkingMutex<ShadowV2ValidationHarness>>>,
    config: &PostBuyRuntimeConfig,
    candidate_id: &str,
    pool_amm_id: &str,
    base_mint: &str,
    position_id: Option<&str>,
    buy_landed_slot: Option<u64>,
    entry_simulation_rpc_slot: Option<u64>,
    entry_opened_at_ms: Option<u64>,
    join_metadata: &PositionJoinMetadata,
) {
    let Some(harness) = harness.as_ref() else {
        return;
    };
    let Some(position_id) = position_id.filter(|value| !value.trim().is_empty()) else {
        warn!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id,
            "PostBuyRuntime: Shadow V2 harness skipped position row without position_id"
        );
        return;
    };
    let Some(shadow_v2_config) = config.shadow_v2_burnin.as_ref() else {
        return;
    };
    let run_id = shadow_v2_config
        .run_namespace
        .as_deref()
        .unwrap_or("UNKNOWN_RUN")
        .to_string();
    let created_at_ms = entry_opened_at_ms.unwrap_or_else(now_ms);
    let created_at_slot = buy_landed_slot.or(entry_simulation_rpc_slot);
    let mut envelope = ShadowV2Envelope::contract_header(
        "shadow_position_v2",
        run_id,
        position_id.to_string(),
        format!("shadow_v2_position_created:{position_id}"),
        pool_amm_id.to_string(),
        base_mint.to_string(),
    );
    envelope.session_id = join_metadata
        .session_id
        .clone()
        .or_else(|| Some("UNKNOWN_SESSION".to_string()));
    envelope.candidate_id = Some(candidate_id.to_string());
    envelope.produced_at_ms = created_at_ms;
    envelope.produced_at_slot = created_at_slot;
    envelope.temporal_class = TemporalClass::PostEntry;
    envelope.clock_domain = ClockDomain::WallClockMs;
    envelope.simulation_level = SimulationLevel::MarkOnly;
    envelope.measurement_grade = MeasurementGrade::DiagnosticOnly;
    envelope.quality = "VALIDATION_HARNESS_POSITION_CREATED".to_string();
    envelope
        .source_refs
        .push("post_buy_runtime:accepted_shadow_handoff".to_string());
    envelope
        .limitations
        .push("PR15_MINIMAL_POSITION_CREATED_ONLY".to_string());
    envelope
        .limitations
        .push("NO_ENTRY_FILL_EXIT_FILL_OR_PATH_INFERENCE_IN_PR15".to_string());
    envelope
        .limitations
        .push("SHADOW_V2_RECORD_NOT_CONSUMED_BY_DECISIONS".to_string());
    if join_metadata.session_id.is_none() {
        envelope
            .limitations
            .push("SESSION_ID_MISSING_FROM_HANDOFF_EXPLICIT_UNKNOWN".to_string());
    }
    let record = ShadowPositionV2 {
        envelope,
        created_at_wall_ms: ClockedTimestamp {
            field_name: "created_at_wall_ms".to_string(),
            value: Some(created_at_ms as i64),
            clock_domain: ClockDomain::WallClockMs,
            clock_source: "post_buy_runtime".to_string(),
            causal_boundary: "POST_ENTRY_HANDOFF".to_string(),
        },
        created_at_slot,
        decision_id: join_metadata
            .ab_record_id
            .clone()
            .or_else(|| join_metadata.source_ab_record_id.clone()),
        strategy_context: join_metadata
            .decision_plane
            .clone()
            .or_else(|| join_metadata.dispatch_source.clone()),
        lane: "shadow".to_string(),
    };
    let outcome = harness
        .lock()
        .append_record(ShadowV2Record::ShadowPositionV2(record));
    if outcome.validation_evidence_status == ShadowV2ValidationEvidenceStatus::Complete {
        debug!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id,
            position_id,
            "PostBuyRuntime: Shadow V2 position-created evidence emitted"
        );
    } else {
        warn!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            candidate_id,
            position_id,
            status = ?outcome.validation_evidence_status,
            canonical_write = ?outcome.canonical_write,
            replay_write = ?outcome.replay_write,
            lifecycle_write = ?outcome.lifecycle_write,
            density_write = ?outcome.density_write,
            "PostBuyRuntime: Shadow V2 validation evidence append incomplete"
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn maybe_emit_shadow_v2_entry_evidence(
    harness: &Option<Arc<ParkingMutex<ShadowV2ValidationHarness>>>,
    config: &PostBuyRuntimeConfig,
    candidate_id: &str,
    pool_amm_id: &str,
    base_mint: &str,
    position_id: Option<&str>,
    signature: &str,
    amount_sol: f64,
    min_tokens_out: Option<u64>,
    entry_token_amount_raw: Option<u64>,
    buy_landed_slot: Option<u64>,
    entry_simulation_rpc_slot: Option<u64>,
    entry_opened_at_ms: Option<u64>,
    join_metadata: &PositionJoinMetadata,
    entry_boundary: Option<ShadowV2EntryBoundaryPayload>,
) {
    let position_id_value = position_id.map(str::to_string);
    let entry_ts_ms = entry_opened_at_ms.unwrap_or_else(now_ms);
    let run_id = config
        .shadow_v2_burnin
        .as_ref()
        .and_then(|burnin| burnin.run_namespace.clone())
        .unwrap_or_else(|| "UNKNOWN_RUN".to_string());
    let mut entry_boundary_blockers = Vec::new();
    let entry_pool_state_before = position_id_value
        .as_deref()
        .zip(entry_boundary.as_ref())
        .and_then(|(position_id, boundary)| {
            let blockers =
                shadow_v2_entry_boundary_handoff_blockers(pool_amm_id, base_mint, boundary);
            if !blockers.is_empty() {
                entry_boundary_blockers = blockers;
                return None;
            }
            Some(shadow_v2_entry_pool_state_from_boundary(
                &run_id,
                join_metadata.session_id.clone(),
                candidate_id,
                position_id,
                pool_amm_id,
                base_mint,
                entry_ts_ms,
                boundary,
            ))
        });
    maybe_emit_shadow_v2_entry_evidence_with_pool_state(
        harness,
        config,
        candidate_id,
        pool_amm_id,
        base_mint,
        position_id,
        signature,
        amount_sol,
        min_tokens_out,
        entry_token_amount_raw,
        buy_landed_slot,
        entry_simulation_rpc_slot,
        entry_opened_at_ms,
        join_metadata,
        entry_pool_state_before,
        entry_boundary.as_ref(),
        &entry_boundary_blockers,
    );
}

fn shadow_v2_entry_boundary_handoff_blockers(
    pool_amm_id: &str,
    base_mint: &str,
    boundary: &ShadowV2EntryBoundaryPayload,
) -> Vec<String> {
    let mut blockers = Vec::new();
    let boundary_base_mint = boundary.canonical_pool_state.base_mint.to_string();
    if boundary_base_mint != base_mint {
        blockers.push("ENTRY_BOUNDARY_BASE_MINT_MISMATCH".to_string());
    }
    let boundary_pool_amm_id = boundary.canonical_pool_state.pool_amm_id.to_string();
    if boundary_pool_amm_id != pool_amm_id {
        blockers.push("ENTRY_BOUNDARY_POOL_ID_MISMATCH".to_string());
    }
    if !blockers.is_empty() {
        blockers.push("ENTRY_BOUNDARY_HANDOFF_VALIDATION_FAILED".to_string());
        blockers.push("ENTRY_POOL_STATE_BEFORE_REJECTED_BY_BOUNDARY_VALIDATION".to_string());
    }
    blockers
}

#[allow(clippy::too_many_arguments)]
fn shadow_v2_entry_pool_state_from_boundary(
    run_id: &str,
    session_id: Option<String>,
    candidate_id: &str,
    position_id: &str,
    pool_amm_id: &str,
    base_mint: &str,
    entry_ts_ms: u64,
    boundary: &ShadowV2EntryBoundaryPayload,
) -> PoolStateSampleV2 {
    let event_id = format!("pool_state_sample_v2:{position_id}:{entry_ts_ms}:entry_before");
    let mut envelope = ShadowV2Envelope::contract_header(
        "pool_state_sample_v2",
        run_id.to_string(),
        position_id.to_string(),
        event_id,
        pool_amm_id.to_string(),
        base_mint.to_string(),
    );
    envelope.session_id = session_id.or_else(|| Some("UNKNOWN_SESSION".to_string()));
    envelope.candidate_id = Some(candidate_id.to_string());
    envelope.produced_at_ms = boundary.captured_at_wall_ms;
    envelope.produced_at_slot = Some(boundary.state_slot);
    envelope.temporal_class = TemporalClass::AtDecision;
    envelope.clock_domain = ClockDomain::StreamObservedMs;
    envelope.simulation_level = SimulationLevel::MarkOnly;
    envelope.measurement_grade = MeasurementGrade::DiagnosticOnly;
    envelope.quality = "ENTRY_BEFORE_FROM_TRIGGER_BOUNDARY".to_string();
    envelope
        .source_refs
        .push("trigger_component:account_state_core_canonical_state".to_string());
    envelope
        .source_refs
        .push("post_buy_submitted:shadow_v2_entry_boundary".to_string());
    envelope
        .source_refs
        .push(format!("entry_boundary_kind:{}", boundary.boundary_kind));
    envelope
        .limitations
        .push("ENTRY_BEFORE_CAPTURED_UPSTREAM_BEFORE_POST_BUY_HANDOFF".to_string());
    envelope
        .limitations
        .push("SHADOW_V2_RECORD_NOT_CONSUMED_BY_DECISIONS".to_string());
    envelope.limitations.extend(boundary.limitations.clone());
    if boundary
        .source_tx_signature
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        envelope
            .limitations
            .push("ENTRY_BOUNDARY_SOURCE_JOIN_NOT_PROVEN".to_string());
        envelope
            .limitations
            .push("ENTRY_BOUNDARY_SOURCE_SIGNATURE_UNAVAILABLE".to_string());
    }
    if boundary.source_transaction_index.is_none() {
        envelope
            .limitations
            .push("ENTRY_BOUNDARY_SOURCE_TRANSACTION_INDEX_UNAVAILABLE".to_string());
    }
    if boundary.source_instruction_index.is_none() {
        envelope
            .limitations
            .push("ENTRY_BOUNDARY_SOURCE_INSTRUCTION_INDEX_UNAVAILABLE".to_string());
    }
    if boundary.source_inner_instruction_index.is_none() {
        envelope
            .limitations
            .push("ENTRY_BOUNDARY_SOURCE_INNER_INSTRUCTION_INDEX_UNAVAILABLE".to_string());
    }
    envelope
        .limitations
        .push("INNER_GROUP_INDEX_NOT_EXACT_INNER_INSTRUCTION_INDEX".to_string());
    envelope
        .limitations
        .push("SOLANA_NATIVE_LOG_INDEX_NOT_APPLICABLE".to_string());
    envelope
        .limitations
        .push("LOG_MESSAGE_INDEX_INTERNAL_UNAVAILABLE".to_string());

    PoolStateSampleV2::from_account_state_core(
        envelope,
        shadow_v2_entry_boundary_source_order_key(
            boundary,
            shadow_v2_post_buy_event_seq(entry_ts_ms, 2),
            boundary.captured_at_wall_ms,
        ),
        &boundary.canonical_pool_state,
        boundary.captured_at_wall_ms,
        boundary.account_data_hash.clone(),
        TemporalClass::AtDecision,
        ClockDomain::StreamObservedMs,
        boundary.token_decimals,
    )
}

#[allow(clippy::too_many_arguments)]
fn maybe_emit_shadow_v2_entry_evidence_with_pool_state(
    harness: &Option<Arc<ParkingMutex<ShadowV2ValidationHarness>>>,
    config: &PostBuyRuntimeConfig,
    candidate_id: &str,
    pool_amm_id: &str,
    base_mint: &str,
    position_id: Option<&str>,
    _signature: &str,
    amount_sol: f64,
    min_tokens_out: Option<u64>,
    entry_token_amount_raw: Option<u64>,
    buy_landed_slot: Option<u64>,
    entry_simulation_rpc_slot: Option<u64>,
    entry_opened_at_ms: Option<u64>,
    join_metadata: &PositionJoinMetadata,
    entry_pool_state_before: Option<PoolStateSampleV2>,
    entry_boundary: Option<&ShadowV2EntryBoundaryPayload>,
    entry_boundary_blockers: &[String],
) {
    let Some(harness) = harness.as_ref() else {
        return;
    };
    let Some(position_id) = position_id.filter(|value| !value.trim().is_empty()) else {
        return;
    };
    let Some(shadow_v2_config) = config.shadow_v2_burnin.as_ref() else {
        return;
    };

    let run_id = shadow_v2_config
        .run_namespace
        .as_deref()
        .unwrap_or("UNKNOWN_RUN")
        .to_string();
    let entry_ts_ms = entry_opened_at_ms.unwrap_or_else(now_ms);
    let entry_slot = entry_simulation_rpc_slot.or(buy_landed_slot);
    let entry_price = shadow_entry_price_from_post_buy(amount_sol, entry_token_amount_raw);
    let entry_source_boundary = entry_boundary.filter(|_| entry_boundary_blockers.is_empty());
    let event_order_key = entry_source_boundary
        .map(|boundary| {
            shadow_v2_entry_boundary_source_order_key(
                boundary,
                shadow_v2_post_buy_event_seq(entry_ts_ms, 1),
                entry_ts_ms,
            )
        })
        .unwrap_or_else(|| {
            shadow_v2_post_buy_event_order_key(
                entry_slot,
                None,
                shadow_v2_post_buy_event_seq(entry_ts_ms, 1),
                entry_ts_ms,
            )
        });

    let mut attempt_envelope = ShadowV2Envelope::contract_header(
        "shadow_entry_attempt_v2",
        run_id.clone(),
        position_id.to_string(),
        format!("shadow_v2_entry_attempt:{position_id}:{entry_ts_ms}"),
        pool_amm_id.to_string(),
        base_mint.to_string(),
    );
    attempt_envelope.session_id = join_metadata
        .session_id
        .clone()
        .or_else(|| Some("UNKNOWN_SESSION".to_string()));
    attempt_envelope.candidate_id = Some(candidate_id.to_string());
    attempt_envelope.produced_at_ms = entry_ts_ms;
    attempt_envelope.produced_at_slot = entry_slot;
    attempt_envelope.temporal_class = TemporalClass::PostEntry;
    attempt_envelope.clock_domain = ClockDomain::SubmitTsMs;
    attempt_envelope.simulation_level = SimulationLevel::MarkOnly;
    attempt_envelope.measurement_grade = MeasurementGrade::MarkPriceReplay;
    attempt_envelope.quality = "ENTRY_ATTEMPT_FROM_POST_BUY_HANDOFF".to_string();
    attempt_envelope
        .source_refs
        .push("post_buy_runtime:accepted_shadow_handoff".to_string());
    attempt_envelope
        .source_refs
        .push("post_buy_submitted:shadow_simulation".to_string());
    attempt_envelope
        .limitations
        .push("ENTRY_ATTEMPT_NOT_LIVE_SUBMIT".to_string());
    attempt_envelope
        .limitations
        .push("ENTRY_ATTEMPT_DERIVED_FROM_SHADOW_POST_BUY_HANDOFF".to_string());
    attempt_envelope
        .limitations
        .push("SHADOW_V2_RECORD_NOT_CONSUMED_BY_DECISIONS".to_string());
    attempt_envelope
        .limitations
        .push("ENTRY_HANDOFF_SIGNATURE_NOT_CHAIN_SOURCE".to_string());
    if entry_source_boundary
        .and_then(|boundary| boundary.source_tx_signature.as_ref())
        .and_then(|signature| (!signature.trim().is_empty()).then_some(signature))
        .is_none()
    {
        attempt_envelope
            .limitations
            .push("ENTRY_BOUNDARY_SOURCE_JOIN_NOT_PROVEN".to_string());
    }
    if entry_price.is_none() {
        attempt_envelope
            .limitations
            .push("ENTRY_ATTEMPT_QUOTE_PRICE_MISSING".to_string());
    }
    if min_tokens_out.is_none() {
        attempt_envelope
            .limitations
            .push("ENTRY_ATTEMPT_MIN_OUT_MISSING".to_string());
    }
    if entry_slot.is_none() {
        attempt_envelope
            .limitations
            .push("ENTRY_ATTEMPT_SLOT_UNKNOWN".to_string());
    }

    let attempt = ShadowEntryAttemptV2 {
        envelope: attempt_envelope,
        event_order_key: event_order_key.clone(),
        intended_entry_ts_ms: ClockedTimestamp {
            field_name: "intended_entry_ts_ms".to_string(),
            value: Some(entry_ts_ms as i64),
            clock_domain: ClockDomain::SubmitTsMs,
            clock_source: "post_buy_runtime.entry_opened_at_ms".to_string(),
            causal_boundary: "POST_ENTRY_SHADOW_SIMULATION_HANDOFF".to_string(),
        },
        intended_entry_slot: entry_slot,
        intended_price_source: "post_buy_shadow_entry_price_from_amount_and_tokens".to_string(),
        intended_quote: entry_price,
        decision_mark_price: entry_price,
        entry_quote_price: entry_price,
        entry_quote_tokens_out: entry_token_amount_raw,
        entry_quote_min_out: min_tokens_out,
        simulated_submit_ts_ms: Some(ClockedTimestamp {
            field_name: "simulated_submit_ts_ms".to_string(),
            value: Some(entry_ts_ms as i64),
            clock_domain: ClockDomain::SubmitTsMs,
            clock_source: "post_buy_runtime.entry_opened_at_ms".to_string(),
            causal_boundary: "POST_ENTRY_SHADOW_SIMULATION_HANDOFF".to_string(),
        }),
        simulated_landing_slot: buy_landed_slot
            .or_else(|| entry_simulation_rpc_slot.and_then(|slot| slot.checked_add(1))),
        simulated_landing_delay_ms: None,
        entry_failure_mode: None,
        executable_fill_model_version: Some(SHADOW_V2_ENTRY_FILL_MODEL_VERSION.to_string()),
    };

    let mut fill_envelope = ShadowV2Envelope::contract_header(
        "shadow_entry_fill_v2",
        run_id.clone(),
        position_id.to_string(),
        format!("shadow_v2_entry_fill:{position_id}:{entry_ts_ms}"),
        pool_amm_id.to_string(),
        base_mint.to_string(),
    );
    fill_envelope.session_id = join_metadata
        .session_id
        .clone()
        .or_else(|| Some("UNKNOWN_SESSION".to_string()));
    fill_envelope.candidate_id = Some(candidate_id.to_string());
    fill_envelope.produced_at_ms = entry_ts_ms;
    fill_envelope.produced_at_slot = entry_slot;
    fill_envelope.source_refs.push(format!(
        "shadow_entry_attempt_v2:shadow_v2_entry_attempt:{position_id}:{entry_ts_ms}"
    ));
    fill_envelope
        .source_refs
        .push("post_buy_runtime:accepted_shadow_handoff".to_string());

    let mut fill_order = event_order_key;
    fill_order.event_seq_in_process = shadow_v2_post_buy_event_seq(
        entry_ts_ms,
        if entry_pool_state_before.is_some() {
            3
        } else {
            2
        },
    );
    let fill = if let Some(pool_state_before) = entry_pool_state_before.as_ref() {
        let (input_lamports, min_out_raw, slippage_bps, fee_bps) = entry_boundary
            .map(|boundary| {
                (
                    boundary.amount_lamports,
                    Some(boundary.min_tokens_out),
                    boundary
                        .slippage_tolerance_bps
                        .unwrap_or_else(|| slippage_tolerance_to_bps(config.slippage_tolerance)),
                    boundary.fee_bps.unwrap_or(SHADOW_V2_ENTRY_FEE_BPS_FALLBACK),
                )
            })
            .unwrap_or_else(|| {
                (
                    (amount_sol.max(0.0) * LAMPORTS_PER_SOL).round() as u64,
                    min_tokens_out,
                    slippage_tolerance_to_bps(config.slippage_tolerance),
                    SHADOW_V2_ENTRY_FEE_BPS_FALLBACK,
                )
            });
        fill_envelope
            .source_refs
            .push("shadow_v2_entry_boundary:trigger_capture".to_string());
        fill_envelope
            .limitations
            .push("ENTRY_FILL_STATIC_MODEL_NOT_LIVE_CONFIRMED".to_string());
        fill_envelope
            .limitations
            .push("ENTRY_FILL_DIAGNOSTIC_SIM_UNLESS_PROVENANCE_READY".to_string());
        ShadowEntryFillV2::from_static_buy_model(
            fill_envelope,
            fill_order,
            pool_state_before,
            &ShadowEntryFillModelConfig::bonding_curve(
                input_lamports,
                slippage_bps,
                fee_bps,
                SHADOW_V2_ENTRY_FILL_MODEL_VERSION,
            )
            .with_min_out_raw(min_out_raw),
        )
    } else {
        let mut blockers = vec![
            "ENTRY_FILL_DERIVED_FROM_SHADOW_SIMULATION_HANDOFF".to_string(),
            "ENTRY_POOL_STATE_BEFORE_UNAVAILABLE".to_string(),
            "ENTRY_FILL_POOL_STATE_SAMPLE_NOT_AVAILABLE_IN_RUNTIME_HANDOFF".to_string(),
            "ENTRY_POOL_STATE_AFTER_UNAVAILABLE".to_string(),
            "FILL_PRICE_UNAVAILABLE".to_string(),
            "SLIPPAGE_BPS_UNAVAILABLE".to_string(),
            "OWN_IMPACT_BPS_UNAVAILABLE".to_string(),
            "FEE_BPS_UNAVAILABLE".to_string(),
            "LANDING_TELEMETRY_UNAVAILABLE".to_string(),
            "QUOTE_FILL_DIVERGENCE_UNAVAILABLE".to_string(),
        ];
        blockers.extend(entry_boundary_blockers.iter().cloned());
        if entry_price.is_none() {
            blockers.push("ENTRY_FILL_ENTRY_PRICE_MISSING".to_string());
        }
        if entry_token_amount_raw.is_none() {
            blockers.push("ENTRY_FILL_TOKEN_AMOUNT_RAW_MISSING".to_string());
        }
        ShadowEntryFillV2::blocked_without_pool_state(fill_envelope, fill_order, blockers)
    };

    let mut harness = harness.lock();
    let mut records = vec![ShadowV2Record::ShadowEntryAttemptV2(attempt)];
    if let Some(pool_state_before) = entry_pool_state_before {
        let mut path_envelope = ShadowV2Envelope::contract_header(
            "shadow_path_sample_v2",
            run_id.clone(),
            position_id.to_string(),
            format!("shadow_v2_entry_path_sample:{position_id}:{entry_ts_ms}:age0"),
            pool_amm_id.to_string(),
            base_mint.to_string(),
        );
        path_envelope.session_id = join_metadata
            .session_id
            .clone()
            .or_else(|| Some("UNKNOWN_SESSION".to_string()));
        path_envelope.candidate_id = Some(candidate_id.to_string());
        path_envelope.parent_event_id = Some(pool_state_before.envelope.event_id.clone());
        path_envelope.produced_at_ms = entry_ts_ms;
        path_envelope.produced_at_slot = entry_slot;
        path_envelope
            .source_refs
            .push("post_buy_runtime:entry_boundary_path_sample".to_string());
        path_envelope.source_refs.push(format!(
            "pool_state_sample_v2:{}",
            pool_state_before.envelope.event_id
        ));
        path_envelope
            .limitations
            .push("ENTRY_PATH_SAMPLE_FROM_ENTRY_BOUNDARY_POOL_STATE".to_string());
        path_envelope
            .limitations
            .push("SHADOW_V2_RECORD_NOT_CONSUMED_BY_DECISIONS".to_string());

        let path_sample = ShadowPathSampleV2::from_pool_state_mark(
            path_envelope,
            pool_state_before.event_order_key.clone(),
            ClockedTimestamp {
                field_name: "sample_ts_ms".to_string(),
                value: Some(entry_ts_ms as i64),
                clock_domain: ClockDomain::StreamObservedMs,
                clock_source: "post_buy_runtime.entry_boundary_pool_state".to_string(),
                causal_boundary: "POST_ENTRY_BOUNDARY_PATH_SAMPLE".to_string(),
            },
            0,
            &pool_state_before,
            ShadowV2PoolPhase::BondingCurve,
            entry_price,
            ShadowPathSamplingModeV2::Standard120s,
            ShadowPathSamplingReasonV2::EventSample,
        );
        emit_executable_dynamic_exit_sidecar_rows(
            shadow_v2_config,
            position_id,
            candidate_id,
            pool_amm_id,
            base_mint,
            &fill,
            entry_token_amount_raw,
            &path_sample,
            &pool_state_before,
        );
        records.push(ShadowV2Record::PoolStateSampleV2(pool_state_before));
        records.push(ShadowV2Record::ShadowPathSampleV2(path_sample));
    }
    records.push(ShadowV2Record::ShadowEntryFillV2(fill));

    for record in records {
        let event_id = record.envelope().event_id.clone();
        let outcome = harness.append_record(record);
        if outcome.validation_evidence_status == ShadowV2ValidationEvidenceStatus::Complete {
            debug!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                event_id, position_id, "PostBuyRuntime: Shadow V2 entry evidence emitted"
            );
        } else {
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                event_id,
                position_id,
                status = ?outcome.validation_evidence_status,
                canonical_write = ?outcome.canonical_write,
                replay_write = ?outcome.replay_write,
                lifecycle_write = ?outcome.lifecycle_write,
                density_write = ?outcome.density_write,
                "PostBuyRuntime: Shadow V2 entry evidence append incomplete"
            );
        }
    }
}

fn maybe_emit_shadow_v2_validation_smoke_marker(
    harness: &Option<Arc<ParkingMutex<ShadowV2ValidationHarness>>>,
    config: &PostBuyRuntimeConfig,
) {
    let Some(harness) = harness.as_ref() else {
        return;
    };
    let Some(shadow_v2_config) = config
        .shadow_v2_burnin
        .as_ref()
        .filter(|config| config.enabled && config.logging_only)
    else {
        return;
    };

    let created_at_ms = now_ms();
    let run_id = shadow_v2_config
        .run_namespace
        .as_deref()
        .unwrap_or("UNKNOWN_RUN")
        .to_string();
    let position_id = format!("validation-smoke-marker:{run_id}:{created_at_ms}");
    let mut envelope = ShadowV2Envelope::contract_header(
        "shadow_position_v2",
        run_id,
        position_id.clone(),
        format!("validation_smoke_marker_v2:{position_id}"),
        "VALIDATION_SMOKE_POOL_UNKNOWN",
        "VALIDATION_SMOKE_BASE_MINT_UNKNOWN",
    );
    envelope.session_id = Some(format!("validation-smoke-session:{created_at_ms}"));
    envelope.candidate_id = Some("VALIDATION_SMOKE_MARKER".to_string());
    envelope.produced_at_ms = created_at_ms;
    envelope.produced_at_slot = None;
    envelope.temporal_class = TemporalClass::Unknown;
    envelope.clock_domain = ClockDomain::WallClockMs;
    envelope.simulation_level = SimulationLevel::MarkOnly;
    envelope.measurement_grade = MeasurementGrade::DiagnosticOnly;
    envelope.quality = "VALIDATION_SMOKE_MARKER_BLOCKED_BY_DATA".to_string();
    envelope
        .source_refs
        .push("post_buy_runtime:shadow_v2_validation_harness_startup".to_string());
    envelope
        .source_refs
        .push("shadow_v2_burnin:logging_only_validation".to_string());
    envelope
        .limitations
        .push("VALIDATION_SMOKE_MARKER_V2".to_string());
    envelope
        .limitations
        .push("DIAGNOSTIC_ONLY_NOT_STRATEGY_POSITION".to_string());
    envelope
        .limitations
        .push("BLOCKED_BY_DATA_NO_ENTRY_FILL_EXIT_FILL_OR_PATH".to_string());
    envelope
        .limitations
        .push("NOT_CONSUMED_BY_DECISIONS".to_string());
    envelope
        .limitations
        .push("NOT_STRATEGY_EVIDENCE".to_string());
    envelope.limitations.push("NOT_LIVE_EQUIVALENT".to_string());
    envelope
        .limitations
        .push("NO_BUY_REJECT_CHANGE".to_string());
    let record = ShadowPositionV2 {
        envelope,
        created_at_wall_ms: ClockedTimestamp {
            field_name: "created_at_wall_ms".to_string(),
            value: Some(created_at_ms as i64),
            clock_domain: ClockDomain::WallClockMs,
            clock_source: "post_buy_runtime".to_string(),
            causal_boundary: "VALIDATION_HARNESS_STARTUP".to_string(),
        },
        created_at_slot: None,
        decision_id: None,
        strategy_context: Some("validation_smoke_marker_v2".to_string()),
        lane: "diagnostic".to_string(),
    };

    let outcome = harness
        .lock()
        .append_record(ShadowV2Record::ShadowPositionV2(record));
    if outcome.validation_evidence_status == ShadowV2ValidationEvidenceStatus::Complete {
        info!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            position_id, "PostBuyRuntime: Shadow V2 validation smoke marker emitted"
        );
    } else {
        warn!(
            runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
            position_id,
            status = ?outcome.validation_evidence_status,
            canonical_write = ?outcome.canonical_write,
            replay_write = ?outcome.replay_write,
            lifecycle_write = ?outcome.lifecycle_write,
            density_write = ?outcome.density_write,
            "PostBuyRuntime: Shadow V2 validation smoke marker incomplete"
        );
    }
}

struct ShadowPostBuyHandoffResult {
    ack: DirectPostBuyHandoffAck,
    position_id: Option<String>,
    terminal_rx: Option<oneshot::Receiver<ShadowTerminalDisposition>>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "shadow handoff preserves distinct immutable entry, epoch, and join-contract values"
)]
async fn handle_shadow_post_buy_handoff(
    shadow_monitor: Option<&Arc<MonitoringEngine>>,
    candidate_id: &str,
    pool_amm_id: &str,
    base_mint: &str,
    amount_sol: f64,
    entry_token_amount_raw: Option<u64>,
    buy_landed_slot: Option<u64>,
    entry_simulation_rpc_slot: Option<u64>,
    entry_opened_at_ms: Option<u64>,
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
            position_id: None,
            terminal_rx: None,
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
                position_id: None,
                terminal_rx: None,
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
                position_id: None,
                terminal_rx: None,
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
            position_id: None,
            terminal_rx: None,
        };
    };
    let entry_amount_lamports = if amount_sol.is_finite() && amount_sol > 0.0 {
        (amount_sol * LAMPORTS_PER_SOL).round() as u64
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
            entry_simulation_rpc_slot = ?entry_simulation_rpc_slot,
            wait_ms = SHADOW_CANONICAL_HANDOFF_WAIT_MS,
            "PostBuyRuntime: shadow handoff timed out waiting for canonical post-buy snapshot; proceeding fail-closed if guardian still cannot seed truth"
        );
    }
    let join_metadata = shadow_entry_timeline_join_metadata(
        join_metadata,
        entry_simulation_rpc_slot,
        buy_landed_slot,
    );
    let context = PositionEventContext {
        join_metadata,
        candidate_id: candidate_id.to_string(),
        entry_order_id,
        quote_id,
        slot: buy_landed_slot,
        lane: Lane::Shadow,
        position_id: probe_position_id,
        position_epoch: Some(epoch),
        opened_at_ms: entry_opened_at_ms,
    };
    let registered = shadow_monitor.register_shadow_position_with_terminal(
        pool_pubkey,
        mint_pubkey,
        pool_pubkey,
        Some(entry_price),
        Some(entry_amount_lamports),
        entry_token_amount_raw,
        context,
    );
    match registered {
        Some(registered) => {
            let (registration, terminal_rx) = registered.into_parts();
            info!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id,
                position_id = %registration.position_id,
                position_epoch = registration.position_epoch,
                entry_price,
                "PostBuyRuntime: canonical shadow position handed off to MonitoringEngine"
            );
            ShadowPostBuyHandoffResult {
                ack: DirectPostBuyHandoffAck::Accepted,
                position_id: Some(registration.position_id),
                terminal_rx: Some(terminal_rx),
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
                position_id: None,
                terminal_rx: None,
            }
        }
    }
}

fn spawn_shadow_terminal_watcher(
    terminal_rx: oneshot::Receiver<ShadowTerminalDisposition>,
    position_limit_tracker: PositionLimitTracker,
    slot_id: PositionSlotId,
    candidate_id: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let (disposition, action_id, reason) = match terminal_rx.await {
            Ok(ShadowTerminalDisposition::SimulatedClosed { action_id, reason }) => {
                ("simulated_closed", Some(action_id), reason)
            }
            Ok(ShadowTerminalDisposition::SimulationBlocked { action_id, reason }) => (
                "simulation_blocked",
                Some(action_id),
                reason.as_label().to_string(),
            ),
            Err(_) => (
                "terminal_channel_dropped",
                None,
                "terminal_channel_dropped".to_string(),
            ),
        };
        ::metrics::counter!(
            "post_buy_shadow_terminal_total",
            1u64,
            "disposition" => disposition,
            "reason" => reason.clone()
        );
        if !position_limit_tracker.release(slot_id) {
            warn!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id = %candidate_id,
                slot_id = %slot_id,
                disposition,
                action_id = ?action_id,
                reason,
                "PostBuyRuntime: shadow position slot already released before terminal notification"
            );
        } else {
            info!(
                runtime_plane = RuntimePlane::PostBuyMonitoring.as_str(),
                candidate_id = %candidate_id,
                slot_id = %slot_id,
                disposition,
                action_id = ?action_id,
                reason,
                "PostBuyRuntime: shadow position slot released from typed terminal notification"
            );
        }
    })
}

// ─── Live sell lifecycle ─────────────────────────────────────────────────────

/// Full lifecycle for a live on-chain position:
/// 1. Persist confirmed BUY metadata and real entry price from transaction metadata.
/// 2. Poll canonical price from `AccountStateCore` and use read-only RPC point
///    queries only when canonical state is unavailable.
/// 3. Trigger a single full 100% SELL at the configured dormant-live thresholds.
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
    let sell_config = SellTxConfig {
        pump_fee_recipient: fee_recipient,
        ..SellTxConfig::default()
    };
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
                LiveExitStatus::ExitConfirmationUnknown,
                Some(&detail),
            );
            Err((LiveExitStatus::ExitConfirmationUnknown, detail))
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
    use ghost_brain::guardian::post_buy::ShadowUnresolvedReason;
    use ghost_core::account_state_core::types::{
        AccountStateUpdate, CanonicalPoolState, StatePhase, UpdateSource,
    };
    use ghost_core::{BondingCurve, CurveFinality};
    use metrics::{
        Counter, CounterFn, Gauge, Histogram, Key, KeyName, Recorder, SharedString, Unit,
    };
    use solana_sdk::pubkey::Pubkey;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn complete_shadow_v2_burnin_config_for_test() -> ShadowV2BurninConfig {
        ShadowV2BurninConfig {
            enabled: true,
            mode:
                ghost_brain::config::ghost_brain_config::ShadowV2BurninMode::LoggingOnlyValidation,
            validation_profile: "shadow_v2_fidelity_validation_logging_only".to_string(),
            run_namespace: Some("shadow-burnin-v2-fidelity-validation-logging-only".to_string()),
            scope_root_path: Some("reports/selector/shadow-v2-fidelity-validation".to_string()),
            pre_run_manifest_path: Some(
                "reports/selector/shadow-v2-fidelity-validation/pre_run_manifest.json".to_string(),
            ),
            post_run_manifest_path: Some(
                "reports/selector/shadow-v2-fidelity-validation/post_run_manifest.json".to_string(),
            ),
            canonical_event_stream_path: Some(
                "reports/selector/shadow-v2-fidelity-validation/shadow_position_event_v2.jsonl"
                    .to_string(),
            ),
            replay_v2_path: Some(
                "reports/selector/shadow-v2-fidelity-validation/shadow_replay_v2.jsonl".to_string(),
            ),
            lifecycle_v2_path: Some(
                "reports/selector/shadow-v2-fidelity-validation/shadow_lifecycle_v2.jsonl"
                    .to_string(),
            ),
            path_density_v2_path: Some(
                "reports/selector/shadow-v2-fidelity-validation/shadow_path_density_v2.jsonl"
                    .to_string(),
            ),
            ..ShadowV2BurninConfig::default()
        }
    }

    fn shadow_v2_burnin_config_for_temp_scope(root: &Path) -> ShadowV2BurninConfig {
        let mut config = complete_shadow_v2_burnin_config_for_test();
        config.scope_root_path = Some(root.display().to_string());
        config.pre_run_manifest_path =
            Some(root.join("pre_run_manifest.json").display().to_string());
        config.post_run_manifest_path =
            Some(root.join("post_run_manifest.json").display().to_string());
        config.canonical_event_stream_path = Some(
            root.join("shadow_position_event_v2.jsonl")
                .display()
                .to_string(),
        );
        config.replay_v2_path = Some(root.join("shadow_replay_v2.jsonl").display().to_string());
        config.lifecycle_v2_path =
            Some(root.join("shadow_lifecycle_v2.jsonl").display().to_string());
        config.path_density_v2_path = Some(
            root.join("shadow_path_density_v2.jsonl")
                .display()
                .to_string(),
        );
        config
    }

    fn shadow_v2_entry_boundary_test_state(
        entry_ts_ms: u64,
        last_update_slot: u64,
    ) -> CanonicalPoolState {
        CanonicalPoolState {
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            virtual_sol_reserves: 30_000_000_000,
            virtual_token_reserves: 1_000_000_000_000,
            real_sol_reserves: 7_000_000_000,
            real_token_reserves: 500_000_000_000,
            bonding_curve_progress: 42.5,
            price_sol: 0.00003,
            market_cap_sol: 30.0,
            token_total_supply: 1_000_000_000_000,
            is_complete: false,
            last_update_slot,
            last_update_ts_ms: entry_ts_ms,
            source_write_version: None,
            source_account_pubkey: None,
            source_account_owner_or_program: None,
            account_data_len: None,
            account_data_hash: None,
            curve_finality: CurveFinality::Provisional,
            state_phase: StatePhase::Canonical,
            update_count: 3,
            initial_price_sol: 0.00001,
            price_change_since_t0_pct: 200.0,
            reserve_velocity_sol_per_sec: 0.5,
        }
    }

    fn shadow_v2_entry_boundary_test_payload(
        entry_ts_ms: u64,
        state: CanonicalPoolState,
    ) -> ShadowV2EntryBoundaryPayload {
        ShadowV2EntryBoundaryPayload {
            boundary_kind: "ENTRY_BEFORE".to_string(),
            source: "TRIGGER_ACCOUNT_STATE_CORE_CANONICAL_STATE".to_string(),
            captured_at_wall_ms: entry_ts_ms,
            latest_observed_slot: state.last_update_slot.checked_add(1),
            state_slot: state.last_update_slot,
            state_ts_ms: state.last_update_ts_ms,
            amount_lamports: 7_000_000,
            min_tokens_out: 1,
            fee_bps: Some(100),
            slippage_tolerance_bps: Some(500),
            token_decimals: 6,
            sol_lamports: 1_000_000_000,
            account_data_hash: state.account_data_hash.clone(),
            account_data_len: state.account_data_len,
            source_account_pubkey: state.source_account_pubkey,
            source_account_owner_or_program: state.source_account_owner_or_program,
            source_write_version: state.source_write_version,
            source_block_time: None,
            source_tx_signature: None,
            source_transaction_index: None,
            source_instruction_index: None,
            source_inner_instruction_index: None,
            source_log_index: None,
            canonical_pool_state: state,
            limitations: vec!["ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME".to_string()],
        }
    }

    #[test]
    fn shadow_v2_event_order_available_source_components_are_propagated() {
        let order = shadow_v2_post_buy_event_order_key_with_components(
            Some(430_000_010),
            Some(1_785_000_000),
            Some("source-signature"),
            Some(7),
            Some(3),
            None,
            None,
            42,
            1_785_000_000_123,
        );

        assert_eq!(order.slot.as_known(), Some(&430_000_010));
        assert_eq!(order.block_time.as_known(), Some(&1_785_000_000));
        assert_eq!(
            order.signature.as_known().map(String::as_str),
            Some("source-signature")
        );
        assert_eq!(order.transaction_index_or_unknown.as_known(), Some(&7));
        assert_eq!(order.instruction_index_or_unknown.as_known(), Some(&3));
        assert!(order.inner_instruction_index_or_unknown.is_unknown());
        assert_eq!(
            order.log_index_or_unknown.non_known_classification(),
            Some("NOT_APPLICABLE")
        );
        assert_eq!(order.event_seq_in_process, 42);
        assert_eq!(order.observed_at_wall_ms, 1_785_000_000_123);
        assert!(!order.has_complete_chain_order());
        assert_eq!(
            order.explicit_unknown_chain_order_components(),
            vec!["inner_instruction_index_or_unknown"]
        );
    }

    #[test]
    fn shadow_v2_event_order_not_applicable_log_does_not_complete_chain_order() {
        let order = shadow_v2_post_buy_event_order_key_with_components(
            Some(430_000_010),
            Some(1_785_000_000),
            Some("source-signature"),
            Some(7),
            Some(3),
            Some(2),
            None,
            42,
            1_785_000_000_123,
        );

        assert_eq!(
            order.inner_instruction_index_or_unknown.as_known(),
            Some(&2)
        );
        assert_eq!(
            order.log_index_or_unknown.non_known_classification(),
            Some("NOT_APPLICABLE")
        );
        assert!(order.explicit_unknown_chain_order_components().is_empty());
        assert!(!order.has_complete_chain_order());
    }

    #[test]
    fn shadow_v2_event_order_missing_source_components_remain_explicit_unknown() {
        let order = shadow_v2_post_buy_event_order_key(
            Some(430_000_010),
            Some("entry-handoff-signature"),
            42,
            1_785_000_000_123,
        );

        assert_eq!(
            order.signature.as_known().map(String::as_str),
            Some("entry-handoff-signature")
        );
        assert_eq!(
            order.explicit_unknown_chain_order_components(),
            vec![
                "block_time",
                "transaction_index_or_unknown",
                "instruction_index_or_unknown",
                "inner_instruction_index_or_unknown",
            ]
        );
        assert_eq!(
            order.log_index_or_unknown.non_known_classification(),
            Some("NOT_APPLICABLE")
        );
        assert!(!order.has_complete_chain_order());
        assert!(order
            .ambiguity_labels()
            .contains(&"EVENT_ORDER_UNKNOWN_BUT_REQUIRED_FOR_RESEARCH".to_string()));
    }

    fn write_fake_shadow_v2_manifest_audit_script(root: &Path) -> PathBuf {
        let script_path = root.join("fake_shadow_v2_manifest_audit.py");
        std::fs::write(
            &script_path,
            r#"import json
import pathlib
import sys

args = sys.argv[1:]
if "--write-manifest" in args:
    manifest_path = pathlib.Path(args[args.index("--write-manifest") + 1])
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(json.dumps({"status": "PASS", "blockers": []}))
if "--write-report-csv" in args:
    report_path = pathlib.Path(args[args.index("--write-report-csv") + 1])
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text("artifact,status\npost_run_manifest.json,PASS\n")
sys.exit(0)
"#,
        )
        .expect("write fake manifest audit script");
        script_path
    }

    #[test]
    fn shadow_v2_post_run_manifest_uses_generate_then_strict_verify() {
        let config = complete_shadow_v2_burnin_config_for_test();

        let generation = shadow_v2_post_run_generation_args(&config).unwrap();
        assert!(generation.contains(&"--write-manifest".to_string()));
        assert!(generation.contains(&"--write-report-csv".to_string()));
        assert!(!generation.contains(&"--strict".to_string()));

        let verification = shadow_v2_post_run_verification_args(&config).unwrap();
        assert!(verification.contains(&"--strict".to_string()));
        assert!(!verification.contains(&"--write-manifest".to_string()));
        assert!(!verification.contains(&"--write-report-csv".to_string()));
    }

    #[tokio::test]
    async fn post_buy_runtime_shutdown_waits_for_shadow_v2_post_run_manifest() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let events_dir = tmp.path().join("events");
        std::fs::create_dir_all(&events_dir).expect("create events dir");
        let script_path = write_fake_shadow_v2_manifest_audit_script(tmp.path());

        let mut burnin = shadow_v2_burnin_config_for_temp_scope(tmp.path());
        burnin.manifest_audit_script = script_path.display().to_string();
        burnin.post_run_manifest_drain_timeout_ms = 30_000;

        let (event_tx, event_rx) = create_event_bus();
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let runtime_config = PostBuyRuntimeConfig {
            events_output_path: events_dir,
            shadow_v2_burnin: Some(burnin),
            ..PostBuyRuntimeConfig::default()
        };

        let runtime_handle = tokio::spawn(run(event_rx, shutdown_rx, None, runtime_config));
        tokio::time::sleep(Duration::from_millis(25)).await;
        let _ = shutdown_tx.send(());
        drop(event_tx);

        tokio::time::timeout(Duration::from_secs(15), runtime_handle)
            .await
            .expect("PostBuyRuntime should join before launcher timeout")
            .expect("PostBuyRuntime task should not panic");

        let post_run_manifest_path = tmp.path().join("post_run_manifest.json");
        let manifest =
            std::fs::read_to_string(&post_run_manifest_path).expect("post_run_manifest.json");
        assert!(
            manifest.contains("\"status\":\"PASS\"") || manifest.contains("\"status\": \"PASS\""),
            "post-run manifest must be generated before PostBuyRuntime returns: {manifest}"
        );
    }

    #[test]
    fn shadow_mode_initializes_terminal_truth_harness_without_optional_burnin() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let harness = init_position_manager_terminal_harness(None, tmp.path(), "runtime-run")
            .expect("terminal harness init")
            .expect("shadow mode terminal harness");

        assert_eq!(
            harness.canonical_event_stream_path(),
            tmp.path()
                .join("position_manager_terminal_truth_v2")
                .join("shadow_position_event_v2.jsonl")
        );
    }

    #[test]
    fn shadow_v2_validation_smoke_marker_writes_required_artifacts_without_handoff() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let burnin = shadow_v2_burnin_config_for_temp_scope(tmp.path());
        let runtime_config = PostBuyRuntimeConfig {
            shadow_v2_burnin: Some(burnin),
            ..PostBuyRuntimeConfig::default()
        };
        let harness = init_shadow_v2_validation_harness(runtime_config.shadow_v2_burnin.as_ref())
            .expect("harness init")
            .map(|harness| Arc::new(ParkingMutex::new(harness)));

        assert!(harness.is_some());
        maybe_emit_shadow_v2_validation_smoke_marker(&harness, &runtime_config);

        let canonical_path = tmp.path().join("shadow_position_event_v2.jsonl");
        let canonical = std::fs::read_to_string(&canonical_path).expect("canonical jsonl");
        let canonical_rows: Vec<_> = canonical.lines().collect();
        assert_eq!(canonical_rows.len(), 1);
        let canonical_event: serde_json::Value =
            serde_json::from_str(canonical_rows[0]).expect("canonical event json");
        assert_eq!(canonical_event["event_kind"], "POSITION_CREATED");
        assert_eq!(
            canonical_event["payload"]["record_type"],
            "shadow_position_v2"
        );
        assert_eq!(
            canonical_event["payload"]["record"]["envelope"]["measurement_grade"],
            "DIAGNOSTIC_ONLY"
        );
        assert_eq!(
            canonical_event["payload"]["record"]["envelope"]["quality"],
            "VALIDATION_SMOKE_MARKER_BLOCKED_BY_DATA"
        );
        let limitations = canonical_event["payload"]["record"]["envelope"]["limitations"]
            .as_array()
            .expect("limitations");
        assert!(limitations
            .iter()
            .any(|value| value == "VALIDATION_SMOKE_MARKER_V2"));
        assert!(limitations
            .iter()
            .any(|value| value == "NOT_CONSUMED_BY_DECISIONS"));

        let replay = std::fs::read_to_string(tmp.path().join("shadow_replay_v2.jsonl"))
            .expect("replay jsonl");
        assert_eq!(replay.lines().count(), 1);
        let lifecycle = std::fs::read_to_string(tmp.path().join("shadow_lifecycle_v2.jsonl"))
            .expect("lifecycle jsonl");
        assert_eq!(lifecycle.lines().count(), 1);
        let density = std::fs::read_to_string(tmp.path().join("shadow_path_density_v2.jsonl"))
            .expect("density jsonl");
        let density_rows: Vec<serde_json::Value> = density
            .lines()
            .map(|line| serde_json::from_str(line).expect("density row"))
            .collect();
        assert_eq!(density_rows.len(), 7);
        assert!(density_rows
            .iter()
            .all(|row| row["schema"] == "shadow_path_density_v2"));
        assert!(density_rows
            .iter()
            .all(|row| row["verdict"] == "NOT_EVALUABLE_NO_COVERAGE"));
    }

    #[test]
    fn shadow_v2_entry_evidence_writes_attempt_and_blocked_fill() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let burnin = shadow_v2_burnin_config_for_temp_scope(tmp.path());
        let runtime_config = PostBuyRuntimeConfig {
            shadow_v2_burnin: Some(burnin),
            ..PostBuyRuntimeConfig::default()
        };
        let harness = init_shadow_v2_validation_harness(runtime_config.shadow_v2_burnin.as_ref())
            .expect("harness init")
            .map(|harness| Arc::new(ParkingMutex::new(harness)));
        let join_metadata = PositionJoinMetadata {
            session_id: Some("session-entry-test".to_string()),
            decision_plane: Some("shadow_v2_pr18_test".to_string()),
            ..Default::default()
        };

        maybe_emit_shadow_v2_entry_evidence(
            &harness,
            &runtime_config,
            "candidate-entry-test",
            "pool-entry-test",
            "mint-entry-test",
            Some("position-entry-test"),
            "signature-entry-test",
            0.007,
            Some(1_000_000),
            Some(7_000_000_000),
            Some(430_000_011),
            Some(430_000_010),
            Some(1_785_000_100_000),
            &join_metadata,
            None,
        );

        let canonical_path = tmp.path().join("shadow_position_event_v2.jsonl");
        let canonical = std::fs::read_to_string(&canonical_path).expect("canonical jsonl");
        let rows: Vec<serde_json::Value> = canonical
            .lines()
            .map(|line| serde_json::from_str(line).expect("canonical row"))
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["event_kind"], "ENTRY_ATTEMPT");
        assert_eq!(rows[1]["event_kind"], "ENTRY_FILL");
        assert_eq!(
            rows[1]["payload"]["record"]["fill_status"],
            "BLOCKED_BY_DATA"
        );
        assert_eq!(
            rows[1]["payload"]["record"]["envelope"]["measurement_grade"],
            "BLOCKED_BY_DATA"
        );
        let limitations = rows[1]["payload"]["record"]["limitations"]
            .as_array()
            .expect("limitations array");
        assert!(limitations
            .iter()
            .any(|value| value == "ENTRY_FILL_POOL_STATE_SAMPLE_MISSING"));
    }

    #[test]
    fn shadow_v2_postbuy_entry_fill_uses_available_pool_state_refs() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let burnin = shadow_v2_burnin_config_for_temp_scope(tmp.path());
        let runtime_config = PostBuyRuntimeConfig {
            shadow_v2_burnin: Some(burnin.clone()),
            ..PostBuyRuntimeConfig::default()
        };
        let harness = init_shadow_v2_validation_harness(runtime_config.shadow_v2_burnin.as_ref())
            .expect("harness init")
            .map(|harness| Arc::new(ParkingMutex::new(harness)));
        let join_metadata = PositionJoinMetadata {
            session_id: Some("session-entry-pool-state-test".to_string()),
            decision_plane: Some("shadow_v2_pr30_test".to_string()),
            ..Default::default()
        };
        let entry_ts_ms = 1_785_000_100_000;
        let run_id = burnin.run_namespace.clone().expect("test run namespace");
        let position_id = "position-entry-pool-state-test";
        let pool_state_event_id = format!("pool_state_sample_v2:{position_id}:{entry_ts_ms}");
        let mut envelope = ShadowV2Envelope::contract_header(
            "pool_state_sample_v2",
            run_id,
            position_id.to_string(),
            pool_state_event_id.clone(),
            "pool-entry-test".to_string(),
            "mint-entry-test".to_string(),
        );
        envelope.session_id = Some("session-entry-pool-state-test".to_string());
        envelope.candidate_id = Some("candidate-entry-pool-state-test".to_string());
        envelope.produced_at_ms = entry_ts_ms;
        envelope.produced_at_slot = Some(430_000_010);
        envelope.temporal_class = TemporalClass::PostEntry;
        envelope.clock_domain = ClockDomain::StreamObservedMs;
        let state = CanonicalPoolState {
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            virtual_sol_reserves: 30_000_000_000,
            virtual_token_reserves: 1_000_000_000_000,
            real_sol_reserves: 7_000_000_000,
            real_token_reserves: 500_000_000_000,
            bonding_curve_progress: 42.5,
            price_sol: 0.00003,
            market_cap_sol: 30.0,
            token_total_supply: 1_000_000_000_000,
            is_complete: false,
            last_update_slot: 430_000_010,
            last_update_ts_ms: entry_ts_ms,
            source_write_version: Some(11),
            source_account_pubkey: Some(Pubkey::new_unique()),
            source_account_owner_or_program: Some(Pubkey::new_unique()),
            account_data_len: Some(b"entry-pool-state-test".len() as u64),
            account_data_hash: Some(
                ghost_brain::guardian::post_buy::shadow_v2::account_data_hash_blake3(
                    b"entry-pool-state-test",
                ),
            ),
            curve_finality: CurveFinality::Provisional,
            state_phase: StatePhase::Canonical,
            update_count: 3,
            initial_price_sol: 0.00001,
            price_change_since_t0_pct: 200.0,
            reserve_velocity_sol_per_sec: 0.5,
        };
        let pool_state = PoolStateSampleV2::from_account_state_core(
            envelope,
            shadow_v2_post_buy_event_order_key(
                Some(430_000_010),
                Some("signature-entry-pool-state-test"),
                shadow_v2_post_buy_event_seq(entry_ts_ms, 2),
                entry_ts_ms,
            ),
            &state,
            entry_ts_ms,
            Some(
                ghost_brain::guardian::post_buy::shadow_v2::account_data_hash_blake3(
                    b"entry-pool-state-test",
                ),
            ),
            TemporalClass::PostEntry,
            ClockDomain::StreamObservedMs,
            6,
        );

        maybe_emit_shadow_v2_entry_evidence_with_pool_state(
            &harness,
            &runtime_config,
            "candidate-entry-pool-state-test",
            "pool-entry-test",
            "mint-entry-test",
            Some(position_id),
            "signature-entry-pool-state-test",
            0.007,
            Some(1_000_000),
            Some(7_000_000_000),
            Some(430_000_011),
            Some(430_000_011),
            Some(entry_ts_ms),
            &join_metadata,
            Some(pool_state),
            None,
            &[],
        );

        let canonical_path = tmp.path().join("shadow_position_event_v2.jsonl");
        let canonical = std::fs::read_to_string(&canonical_path).expect("canonical jsonl");
        let rows: Vec<serde_json::Value> = canonical
            .lines()
            .map(|line| serde_json::from_str(line).expect("canonical row"))
            .collect();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["event_kind"], "ENTRY_ATTEMPT");
        assert_eq!(rows[1]["event_kind"], "POOL_STATE_SAMPLE");
        assert_eq!(rows[2]["event_kind"], "PATH_SAMPLE");
        assert_eq!(rows[3]["event_kind"], "ENTRY_FILL");
        assert_eq!(rows[2]["payload"]["record"]["age_ms"], 0);
        assert_eq!(
            rows[2]["payload"]["record"]["pool_state_ref"].as_str(),
            Some(pool_state_event_id.as_str())
        );
        assert_eq!(
            rows[2]["payload"]["record"]["sampling_reason"].as_str(),
            Some("EVENT_SAMPLE")
        );
        let path_limitations = rows[2]["payload"]["record"]["envelope"]["limitations"]
            .as_array()
            .expect("path limitations");
        assert!(path_limitations
            .iter()
            .any(|value| value == "ENTRY_PATH_SAMPLE_FROM_ENTRY_BOUNDARY_POOL_STATE"));
        assert_eq!(
            rows[3]["payload"]["record"]["pool_state_before"].as_str(),
            Some(pool_state_event_id.as_str())
        );
        assert_eq!(rows[3]["payload"]["record"]["fill_status"], "FILLED");
        assert_eq!(
            rows[3]["payload"]["record"]["execution_simulation_ready"],
            true
        );
        assert_eq!(
            rows[3]["payload"]["record"]["execution_label_grade"],
            "RESEARCH_CANDIDATE"
        );
        assert_eq!(
            rows[3]["payload"]["record"]["research_provenance_ready"],
            true
        );
        assert!(rows[3]["payload"]["record"]["fill_price"].is_number());
        assert!(rows[3]["payload"]["record"]["pool_state_after"]
            .as_str()
            .is_some());
        assert!(rows[3]["payload"]["record"]
            .get("provenance_blockers")
            .and_then(|value| value.as_array())
            .is_none_or(|blockers| blockers.is_empty()));
    }

    #[test]
    fn shadow_v2_postbuy_entry_fill_executes_diagnostic_sim_from_entry_boundary_payload() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let burnin = shadow_v2_burnin_config_for_temp_scope(tmp.path());
        let runtime_config = PostBuyRuntimeConfig {
            shadow_v2_burnin: Some(burnin),
            ..PostBuyRuntimeConfig::default()
        };
        let harness = init_shadow_v2_validation_harness(runtime_config.shadow_v2_burnin.as_ref())
            .expect("harness init")
            .map(|harness| Arc::new(ParkingMutex::new(harness)));
        let join_metadata = PositionJoinMetadata {
            session_id: Some("session-entry-boundary-test".to_string()),
            decision_plane: Some("shadow_v2_pr34b_test".to_string()),
            ..Default::default()
        };
        let entry_ts_ms = 1_785_000_200_000;
        let state = CanonicalPoolState {
            pool_amm_id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            virtual_sol_reserves: 30_000_000_000,
            virtual_token_reserves: 1_000_000_000_000,
            real_sol_reserves: 7_000_000_000,
            real_token_reserves: 500_000_000_000,
            bonding_curve_progress: 42.5,
            price_sol: 0.00003,
            market_cap_sol: 30.0,
            token_total_supply: 1_000_000_000_000,
            is_complete: false,
            last_update_slot: 430_000_010,
            last_update_ts_ms: entry_ts_ms,
            source_write_version: None,
            source_account_pubkey: None,
            source_account_owner_or_program: None,
            account_data_len: None,
            account_data_hash: None,
            curve_finality: CurveFinality::Provisional,
            state_phase: StatePhase::Canonical,
            update_count: 3,
            initial_price_sol: 0.00001,
            price_change_since_t0_pct: 200.0,
            reserve_velocity_sol_per_sec: 0.5,
        };
        let pool_amm_id = state.pool_amm_id.to_string();
        let base_mint = state.base_mint.to_string();
        let boundary = ShadowV2EntryBoundaryPayload {
            boundary_kind: "ENTRY_BEFORE".to_string(),
            source: "TRIGGER_ACCOUNT_STATE_CORE_CANONICAL_STATE".to_string(),
            captured_at_wall_ms: entry_ts_ms,
            latest_observed_slot: Some(430_000_011),
            state_slot: state.last_update_slot,
            state_ts_ms: state.last_update_ts_ms,
            amount_lamports: 7_000_000,
            min_tokens_out: 1,
            fee_bps: Some(100),
            slippage_tolerance_bps: Some(500),
            token_decimals: 6,
            sol_lamports: 1_000_000_000,
            account_data_hash: None,
            account_data_len: None,
            source_account_pubkey: None,
            source_account_owner_or_program: None,
            source_write_version: None,
            source_block_time: None,
            source_tx_signature: None,
            source_transaction_index: None,
            source_instruction_index: None,
            source_inner_instruction_index: None,
            source_log_index: None,
            canonical_pool_state: state,
            limitations: vec!["ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME".to_string()],
        };

        maybe_emit_shadow_v2_entry_evidence(
            &harness,
            &runtime_config,
            "candidate-entry-boundary-test",
            &pool_amm_id,
            &base_mint,
            Some("position-entry-boundary-test"),
            "signature-entry-boundary-test",
            0.007,
            Some(1),
            Some(7_000_000_000),
            Some(430_000_012),
            Some(430_000_011),
            Some(entry_ts_ms),
            &join_metadata,
            Some(boundary),
        );

        let canonical_path = tmp.path().join("shadow_position_event_v2.jsonl");
        let canonical = std::fs::read_to_string(&canonical_path).expect("canonical jsonl");
        let rows: Vec<serde_json::Value> = canonical
            .lines()
            .map(|line| serde_json::from_str(line).expect("canonical row"))
            .collect();
        assert_eq!(rows.len(), 4);
        let pool_state = rows
            .iter()
            .find(|row| row["event_kind"] == "POOL_STATE_SAMPLE")
            .expect("pool state sample");
        let path_sample = rows
            .iter()
            .find(|row| row["event_kind"] == "PATH_SAMPLE")
            .expect("entry path sample");
        let fill = rows
            .iter()
            .find(|row| row["event_kind"] == "ENTRY_FILL")
            .expect("entry fill");
        assert_eq!(
            pool_state["payload"]["record"]["event_order_key"]["signature"],
            "UNKNOWN"
        );
        assert_eq!(
            pool_state["payload"]["record"]["event_signature"],
            "UNKNOWN"
        );
        assert_eq!(
            path_sample["payload"]["record"]["pool_state_ref"],
            pool_state["envelope"]["event_id"]
        );
        let pool_state_limitations = pool_state["envelope"]["limitations"]
            .as_array()
            .expect("pool state limitations");
        assert!(pool_state_limitations
            .iter()
            .any(|value| value == "ENTRY_BOUNDARY_SOURCE_SIGNATURE_UNAVAILABLE"));
        let fill = &fill["payload"]["record"];
        assert_eq!(fill["fill_status"], "FILLED");
        assert_eq!(fill["execution_simulation_ready"], true);
        assert_eq!(fill["execution_label_grade"], "DIAGNOSTIC_SIM");
        assert_eq!(fill["research_provenance_ready"], false);
        assert!(fill["fill_price"].is_number());
        assert!(fill["fill_amount_tokens"].is_number());
        assert!(fill["own_impact_bps"].is_number());
        assert_eq!(fill["fee_bps"], 100);
        assert_eq!(fill["min_out"], 1);
        assert!(fill["pool_state_after"].as_str().is_some());
        let provenance_blockers = fill["provenance_blockers"]
            .as_array()
            .expect("provenance blockers");
        assert!(provenance_blockers
            .iter()
            .any(|value| value == "POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME"));
    }

    #[test]
    fn shadow_v2_postbuy_entry_boundary_source_order_components_reach_pool_state_sample() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let burnin = shadow_v2_burnin_config_for_temp_scope(tmp.path());
        let runtime_config = PostBuyRuntimeConfig {
            shadow_v2_burnin: Some(burnin),
            ..PostBuyRuntimeConfig::default()
        };
        let harness = init_shadow_v2_validation_harness(runtime_config.shadow_v2_burnin.as_ref())
            .expect("harness init")
            .map(|harness| Arc::new(ParkingMutex::new(harness)));
        let join_metadata = PositionJoinMetadata {
            session_id: Some("session-entry-boundary-order-test".to_string()),
            decision_plane: Some("shadow_v2_pr43b_test".to_string()),
            ..Default::default()
        };
        let entry_ts_ms = 1_785_000_205_000;
        let state = shadow_v2_entry_boundary_test_state(entry_ts_ms, 430_000_010);
        let pool_amm_id = state.pool_amm_id.to_string();
        let base_mint = state.base_mint.to_string();
        let mut boundary = shadow_v2_entry_boundary_test_payload(entry_ts_ms, state);
        boundary.source_block_time = Some(1_785_000_000);
        boundary.source_tx_signature = Some("entry-boundary-source-signature".to_string());
        boundary.source_transaction_index = Some(9);
        boundary.source_instruction_index = Some(4);

        maybe_emit_shadow_v2_entry_evidence(
            &harness,
            &runtime_config,
            "candidate-entry-boundary-order-test",
            &pool_amm_id,
            &base_mint,
            Some("position-entry-boundary-order-test"),
            "entry-handoff-signature-not-used-for-pool-state",
            0.007,
            Some(1),
            Some(7_000_000_000),
            Some(430_000_012),
            Some(430_000_011),
            Some(entry_ts_ms),
            &join_metadata,
            Some(boundary),
        );

        let canonical = std::fs::read_to_string(tmp.path().join("shadow_position_event_v2.jsonl"))
            .expect("canonical jsonl");
        let rows: Vec<serde_json::Value> = canonical
            .lines()
            .map(|line| serde_json::from_str(line).expect("canonical row"))
            .collect();
        let pool_state = rows
            .iter()
            .find(|row| row["event_kind"] == "POOL_STATE_SAMPLE")
            .expect("pool state sample");
        let order = &pool_state["payload"]["record"]["event_order_key"];
        assert_eq!(order["block_time"], 1_785_000_000);
        assert_eq!(order["signature"], "entry-boundary-source-signature");
        assert_eq!(order["transaction_index_or_unknown"], 9);
        assert_eq!(order["instruction_index_or_unknown"], 4);
        assert_eq!(order["inner_instruction_index_or_unknown"], "UNKNOWN");
        assert_eq!(order["log_index_or_unknown"], "NOT_APPLICABLE");
        assert_eq!(
            pool_state["payload"]["record"]["event_signature"],
            "entry-boundary-source-signature"
        );
        assert_eq!(
            pool_state["payload"]["record"]["event_index"].as_u64(),
            Some(u32::MAX as u64)
        );

        let entry_attempt = rows
            .iter()
            .find(|row| row["event_kind"] == "ENTRY_ATTEMPT")
            .expect("entry attempt");
        let attempt_order = &entry_attempt["payload"]["record"]["event_order_key"];
        assert_eq!(attempt_order["block_time"], 1_785_000_000);
        assert_eq!(
            attempt_order["signature"],
            "entry-boundary-source-signature"
        );
        assert_eq!(attempt_order["transaction_index_or_unknown"], 9);
        assert_eq!(attempt_order["instruction_index_or_unknown"], 4);
        assert_eq!(
            attempt_order["inner_instruction_index_or_unknown"],
            "UNKNOWN"
        );
        assert_eq!(attempt_order["log_index_or_unknown"], "NOT_APPLICABLE");

        let entry_fill = rows
            .iter()
            .find(|row| row["event_kind"] == "ENTRY_FILL")
            .expect("entry fill");
        let fill_order = &entry_fill["payload"]["record"]["event_order_key"];
        assert_eq!(fill_order["signature"], "entry-boundary-source-signature");
        assert_eq!(fill_order["transaction_index_or_unknown"], 9);
        assert_ne!(
            fill_order["signature"],
            "entry-handoff-signature-not-used-for-pool-state"
        );
    }

    #[test]
    fn shadow_v2_postbuy_entry_boundary_without_source_does_not_reuse_handoff_signature() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let burnin = shadow_v2_burnin_config_for_temp_scope(tmp.path());
        let runtime_config = PostBuyRuntimeConfig {
            shadow_v2_burnin: Some(burnin),
            ..PostBuyRuntimeConfig::default()
        };
        let harness = init_shadow_v2_validation_harness(runtime_config.shadow_v2_burnin.as_ref())
            .expect("harness init")
            .map(|harness| Arc::new(ParkingMutex::new(harness)));
        let join_metadata = PositionJoinMetadata {
            session_id: Some("session-entry-boundary-no-source-test".to_string()),
            decision_plane: Some("shadow_v2_pr43e_test".to_string()),
            ..Default::default()
        };
        let entry_ts_ms = 1_785_000_206_000;
        let state = shadow_v2_entry_boundary_test_state(entry_ts_ms, 430_000_020);
        let pool_amm_id = state.pool_amm_id.to_string();
        let base_mint = state.base_mint.to_string();
        let mut boundary = shadow_v2_entry_boundary_test_payload(entry_ts_ms, state);
        boundary.source_block_time = Some(1_785_000_206);
        boundary.source_transaction_index = Some(99);
        boundary.source_instruction_index = Some(8);

        maybe_emit_shadow_v2_entry_evidence(
            &harness,
            &runtime_config,
            "candidate-entry-boundary-no-source-test",
            &pool_amm_id,
            &base_mint,
            Some("position-entry-boundary-no-source-test"),
            "handoff-signature-must-not-be-chain-source",
            0.007,
            Some(1),
            Some(7_000_000_000),
            Some(430_000_022),
            Some(430_000_021),
            Some(entry_ts_ms),
            &join_metadata,
            Some(boundary),
        );

        let canonical = std::fs::read_to_string(tmp.path().join("shadow_position_event_v2.jsonl"))
            .expect("canonical jsonl");
        let rows: Vec<serde_json::Value> = canonical
            .lines()
            .map(|line| serde_json::from_str(line).expect("canonical row"))
            .collect();
        for event_kind in ["POOL_STATE_SAMPLE", "ENTRY_ATTEMPT", "ENTRY_FILL"] {
            let row = rows
                .iter()
                .find(|row| row["event_kind"] == event_kind)
                .unwrap_or_else(|| panic!("{event_kind} row"));
            let order = &row["payload"]["record"]["event_order_key"];
            assert_eq!(order["block_time"], "UNKNOWN", "{event_kind}");
            assert_eq!(order["signature"], "UNKNOWN", "{event_kind}");
            assert_ne!(
                order["signature"], "handoff-signature-must-not-be-chain-source",
                "{event_kind}"
            );
            assert_eq!(
                order["transaction_index_or_unknown"], "UNKNOWN",
                "{event_kind}"
            );
            assert_eq!(
                order["instruction_index_or_unknown"], "UNKNOWN",
                "{event_kind}"
            );
            assert_eq!(
                order["inner_instruction_index_or_unknown"], "UNKNOWN",
                "{event_kind}"
            );
            assert_eq!(
                order["log_index_or_unknown"], "NOT_APPLICABLE",
                "{event_kind}"
            );
        }
    }

    #[test]
    fn shadow_v2_postbuy_entry_boundary_blocks_base_mint_mismatch() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let burnin = shadow_v2_burnin_config_for_temp_scope(tmp.path());
        let runtime_config = PostBuyRuntimeConfig {
            shadow_v2_burnin: Some(burnin),
            ..PostBuyRuntimeConfig::default()
        };
        let harness = init_shadow_v2_validation_harness(runtime_config.shadow_v2_burnin.as_ref())
            .expect("harness init")
            .map(|harness| Arc::new(ParkingMutex::new(harness)));
        let join_metadata = PositionJoinMetadata {
            session_id: Some("session-entry-boundary-base-mismatch-test".to_string()),
            decision_plane: Some("shadow_v2_pr34c_test".to_string()),
            ..Default::default()
        };
        let entry_ts_ms = 1_785_000_210_000;
        let state = shadow_v2_entry_boundary_test_state(entry_ts_ms, 430_000_010);
        let pool_amm_id = state.pool_amm_id.to_string();
        let boundary = shadow_v2_entry_boundary_test_payload(entry_ts_ms, state);

        maybe_emit_shadow_v2_entry_evidence(
            &harness,
            &runtime_config,
            "candidate-entry-boundary-base-mismatch-test",
            &pool_amm_id,
            &Pubkey::new_unique().to_string(),
            Some("position-entry-boundary-base-mismatch-test"),
            "signature-entry-boundary-base-mismatch-test",
            0.007,
            Some(1),
            Some(7_000_000_000),
            Some(430_000_012),
            Some(430_000_011),
            Some(entry_ts_ms),
            &join_metadata,
            Some(boundary),
        );

        let canonical = std::fs::read_to_string(tmp.path().join("shadow_position_event_v2.jsonl"))
            .expect("canonical jsonl");
        let rows: Vec<serde_json::Value> = canonical
            .lines()
            .map(|line| serde_json::from_str(line).expect("canonical row"))
            .collect();
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|row| row["event_kind"] != "POOL_STATE_SAMPLE"));
        assert_eq!(rows[1]["event_kind"], "ENTRY_FILL");
        assert_eq!(
            rows[1]["payload"]["record"]["fill_status"],
            "BLOCKED_BY_DATA"
        );
        let limitations = rows[1]["payload"]["record"]["limitations"]
            .as_array()
            .expect("limitations array");
        for expected in [
            "ENTRY_BOUNDARY_BASE_MINT_MISMATCH",
            "ENTRY_BOUNDARY_HANDOFF_VALIDATION_FAILED",
            "ENTRY_POOL_STATE_BEFORE_REJECTED_BY_BOUNDARY_VALIDATION",
        ] {
            assert!(
                limitations.iter().any(|value| value == expected),
                "expected limitation {expected}, got {limitations:?}"
            );
        }
    }

    #[test]
    fn shadow_v2_postbuy_entry_boundary_blocks_pool_id_mismatch() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let burnin = shadow_v2_burnin_config_for_temp_scope(tmp.path());
        let runtime_config = PostBuyRuntimeConfig {
            shadow_v2_burnin: Some(burnin),
            ..PostBuyRuntimeConfig::default()
        };
        let harness = init_shadow_v2_validation_harness(runtime_config.shadow_v2_burnin.as_ref())
            .expect("harness init")
            .map(|harness| Arc::new(ParkingMutex::new(harness)));
        let join_metadata = PositionJoinMetadata {
            session_id: Some("session-entry-boundary-pool-mismatch-test".to_string()),
            decision_plane: Some("shadow_v2_pr34c_test".to_string()),
            ..Default::default()
        };
        let entry_ts_ms = 1_785_000_220_000;
        let state = shadow_v2_entry_boundary_test_state(entry_ts_ms, 430_000_010);
        let base_mint = state.base_mint.to_string();
        let boundary = shadow_v2_entry_boundary_test_payload(entry_ts_ms, state);

        maybe_emit_shadow_v2_entry_evidence(
            &harness,
            &runtime_config,
            "candidate-entry-boundary-pool-mismatch-test",
            &Pubkey::new_unique().to_string(),
            &base_mint,
            Some("position-entry-boundary-pool-mismatch-test"),
            "signature-entry-boundary-pool-mismatch-test",
            0.007,
            Some(1),
            Some(7_000_000_000),
            Some(430_000_012),
            Some(430_000_011),
            Some(entry_ts_ms),
            &join_metadata,
            Some(boundary),
        );

        let canonical = std::fs::read_to_string(tmp.path().join("shadow_position_event_v2.jsonl"))
            .expect("canonical jsonl");
        let rows: Vec<serde_json::Value> = canonical
            .lines()
            .map(|line| serde_json::from_str(line).expect("canonical row"))
            .collect();
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|row| row["event_kind"] != "POOL_STATE_SAMPLE"));
        assert_eq!(rows[1]["event_kind"], "ENTRY_FILL");
        assert_eq!(
            rows[1]["payload"]["record"]["fill_status"],
            "BLOCKED_BY_DATA"
        );
        let limitations = rows[1]["payload"]["record"]["limitations"]
            .as_array()
            .expect("limitations array");
        for expected in [
            "ENTRY_BOUNDARY_POOL_ID_MISMATCH",
            "ENTRY_BOUNDARY_HANDOFF_VALIDATION_FAILED",
            "ENTRY_POOL_STATE_BEFORE_REJECTED_BY_BOUNDARY_VALIDATION",
        ] {
            assert!(
                limitations.iter().any(|value| value == expected),
                "expected limitation {expected}, got {limitations:?}"
            );
        }
    }

    #[test]
    fn shadow_v2_postbuy_entry_boundary_preserves_same_slot_ordering_provenance_blocker() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let burnin = shadow_v2_burnin_config_for_temp_scope(tmp.path());
        let runtime_config = PostBuyRuntimeConfig {
            shadow_v2_burnin: Some(burnin),
            ..PostBuyRuntimeConfig::default()
        };
        let harness = init_shadow_v2_validation_harness(runtime_config.shadow_v2_burnin.as_ref())
            .expect("harness init")
            .map(|harness| Arc::new(ParkingMutex::new(harness)));
        let join_metadata = PositionJoinMetadata {
            session_id: Some("session-entry-boundary-same-slot-test".to_string()),
            decision_plane: Some("shadow_v2_pr34c_test".to_string()),
            ..Default::default()
        };
        let entry_ts_ms = 1_785_000_230_000;
        let state = shadow_v2_entry_boundary_test_state(entry_ts_ms, 430_000_011);
        let pool_amm_id = state.pool_amm_id.to_string();
        let base_mint = state.base_mint.to_string();
        let boundary = shadow_v2_entry_boundary_test_payload(entry_ts_ms, state);

        maybe_emit_shadow_v2_entry_evidence(
            &harness,
            &runtime_config,
            "candidate-entry-boundary-same-slot-test",
            &pool_amm_id,
            &base_mint,
            Some("position-entry-boundary-same-slot-test"),
            "signature-entry-boundary-same-slot-test",
            0.007,
            Some(1),
            Some(7_000_000_000),
            Some(430_000_011),
            Some(430_000_011),
            Some(entry_ts_ms),
            &join_metadata,
            Some(boundary),
        );

        let canonical = std::fs::read_to_string(tmp.path().join("shadow_position_event_v2.jsonl"))
            .expect("canonical jsonl");
        let rows: Vec<serde_json::Value> = canonical
            .lines()
            .map(|line| serde_json::from_str(line).expect("canonical row"))
            .collect();
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().any(|row| row["event_kind"] == "PATH_SAMPLE"));
        let fill = rows
            .iter()
            .find(|row| row["event_kind"] == "ENTRY_FILL")
            .expect("entry fill");
        assert_eq!(fill["payload"]["record"]["fill_status"], "FILLED");
        assert_eq!(
            fill["payload"]["record"]["execution_label_grade"],
            "DIAGNOSTIC_SIM"
        );
        assert_eq!(
            fill["payload"]["record"]["execution_simulation_ready"],
            true
        );
        assert_eq!(
            fill["payload"]["record"]["research_provenance_ready"],
            false
        );
        let limitations = fill["payload"]["record"]["limitations"]
            .as_array()
            .expect("limitations array");
        assert!(limitations
            .iter()
            .any(|value| { value == "ENTRY_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS" }));
        let provenance_blockers = fill["payload"]["record"]["provenance_blockers"]
            .as_array()
            .expect("provenance blockers array");
        assert!(provenance_blockers
            .iter()
            .any(|value| { value == "ENTRY_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS" }));
        let blocked_reasons_contains_same_slot = fill["payload"]["record"]
            .get("blocked_reasons")
            .and_then(|value| value.as_array())
            .is_some_and(|blocked_reasons| {
                blocked_reasons
                    .iter()
                    .any(|value| value == "ENTRY_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS")
            });
        assert!(!blocked_reasons_contains_same_slot);
    }

    #[test]
    fn shadow_v2_postbuy_does_not_late_read_account_state_for_entry_boundary() {
        let source = include_str!("post_buy_runtime.rs");
        let start = source
            .find("fn maybe_emit_shadow_v2_entry_evidence")
            .expect("entry evidence helper");
        let end = source[start..]
            .find("fn handle_shadow_post_buy_handoff")
            .map(|offset| start + offset)
            .expect("following helper boundary");
        let body = &source[start..end];
        assert!(!body.contains(".account_state_core"));
        assert!(!body.contains("get_canonical_state("));
    }

    #[test]
    fn shadow_v2_manifest_audit_not_invoked_from_event_path() {
        let source = include_str!("post_buy_runtime.rs");
        let handle_event_start = source
            .find("async fn handle_post_buy_event")
            .expect("handle_post_buy_event should exist");
        let handle_event_end = handle_event_start
            + source[handle_event_start..]
                .find("fn maybe_emit_shadow_v2_position_created")
                .expect("shadow v2 helper should follow handle_post_buy_event");
        let handle_event_body = &source[handle_event_start..handle_event_end];
        assert!(!handle_event_body.contains("Command::new(\"python3\")"));
        assert!(!handle_event_body.contains("run_shadow_v2_manifest_command"));
        assert_eq!(source.matches("Command::new(\"python3\")").count(), 1);
    }

    #[test]
    fn shadow_v2_no_decision_consumption_static_guard() {
        let forbidden_needles = [
            "shadow_v2_burnin",
            "ShadowV2ValidationHarness",
            "ShadowV2Record",
            "ExecutableDynamicExitEvidenceV1",
            "ExecutableDynamicExitPolicyEvaluatorV1",
            "executable_dynamic_exit_evidence_v1",
            "shadow_position_event_v2",
            "shadow_replay_v2",
            "shadow_lifecycle_v2",
            "shadow_path_density_v2",
        ];
        for (name, source) in [
            ("gatekeeper", include_str!("gatekeeper.rs")),
            ("trigger", include_str!("trigger/mod.rs")),
            ("live_tx_sender", include_str!("live_tx_sender.rs")),
        ] {
            for needle in forbidden_needles {
                assert!(
                    !source.contains(needle),
                    "Shadow V2 marker {needle} must remain out of {name} decision/execution source"
                );
            }
        }
    }

    #[test]
    fn executable_dynamic_exit_sidecar_does_not_change_decisions() {
        let source = include_str!("post_buy_runtime.rs");
        let start = source
            .find("fn emit_executable_dynamic_exit_sidecar_rows")
            .expect("dynamic exit sidecar helper should exist");
        let end = source[start..]
            .find("async fn run_shadow_v2_manifest_command")
            .map(|offset| start + offset)
            .expect("manifest command should follow sidecar helper");
        let helper_body = &source[start..end];

        for forbidden in [
            "Gatekeeper",
            "BUY",
            "REJECT",
            "ShadowV2Record::ShadowExitFillV2",
            "LiveTxSender",
            "shadow_close_only",
            "active_close",
        ] {
            assert!(
                !helper_body.contains(forbidden),
                "dynamic-exit sidecar helper must not consume or change {forbidden}"
            );
        }
    }

    #[test]
    fn executable_dynamic_exit_write_failure_does_not_change_execution_eligibility() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let directory_path = tmp.path().join("sidecar-as-directory.jsonl");
        std::fs::create_dir(&directory_path).expect("sidecar directory");
        let row = ExecutableDynamicExitEvidenceV1 {
            schema: "executable_dynamic_exit_evidence_v1".to_string(),
            schema_version: 1,
            run_id: "run-a".to_string(),
            position_id: "pos-a".to_string(),
            candidate_id: None,
            pool_id: "pool-a".to_string(),
            base_mint: "mint-a".to_string(),
            entry_fill_event_id: "entry-fill-a".to_string(),
            candidate_exit_policy: "fixed_exit_2s".to_string(),
            candidate_exit_policy_version: "executable_dynamic_exit_policy_v1".to_string(),
            candidate_exit_policy_hash: "hash-a".to_string(),
            candidate_exit_age_ms: 2_000,
            candidate_exit_trigger_reason: "FIXED_AGE".to_string(),
            mark_pnl_bps_at_trigger: Some(100),
            pool_state_ref_at_trigger: "pool-state-a".to_string(),
            quote_source: "static".to_string(),
            executable_exit_quote_available: false,
            estimated_executable_pnl_bps: None,
            estimated_slippage_bps: None,
            quote_fill_divergence_bps: None,
            pool_state_staleness_ms: None,
            pool_state_staleness_slots: None,
            evidence_quality: "MARK_ONLY_NO_EXECUTABLE_QUOTE".to_string(),
            limitations: vec!["TEST_WRITE_FAILURE".to_string()],
            static_exit_quote_model_version: "shadow_v2_exit_static_quote_model_v1".to_string(),
            trigger_sample_event_id: "path-a".to_string(),
            trigger_pool_state_event_id: "pool-a".to_string(),
            trigger_observed_at_ms: 1,
            trigger_age_ms: 2_000,
            trigger_eval_seq: 1,
            entry_fill_amount_tokens: None,
            entry_fill_amount_sol: None,
            entry_fill_price: None,
            position_token_amount_source: "missing".to_string(),
            estimated_output_sol: None,
            estimated_output_tokens_sold: None,
            estimated_fee_bps: None,
            own_impact_bps: None,
            slippage_tolerance_bps: None,
            min_out_if_available: None,
            pool_state_source_quality: "test".to_string(),
            decision_neutral: true,
            runtime_close_triggered: false,
            changes_gatekeeper_decision: false,
            changes_execution: false,
            static_model_only: true,
            not_live_fill: true,
            not_canonical_exit: true,
        };

        let error = append_executable_dynamic_exit_sidecar_row(&directory_path, &row)
            .expect_err("directory path should fail sidecar append");
        assert!(error.contains("failed to open sidecar"));
        assert!(row.decision_neutral);
        assert!(!row.changes_execution);
        assert!(!row.runtime_close_triggered);
    }

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
            source_account_pubkey: None,
            source_account_owner_or_program: None,
            account_data_len: None,
            account_data_hash: None,
            receive_ts_ms: now_ms(),
            receive_seq: 1,
            curve_finality: CurveFinality::Provisional,
            source: UpdateSource::GeyserAccountUpdate,
        };
        let _ = account_state_core.apply_account_update(update);
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
    fn direct_post_buy_handoff_capacity_is_bounded_and_scaled() {
        assert_eq!(direct_post_buy_handoff_capacity(0), 8);
        assert_eq!(direct_post_buy_handoff_capacity(1), 8);
        assert_eq!(direct_post_buy_handoff_capacity(2), 8);
        assert_eq!(direct_post_buy_handoff_capacity(3), 12);
        assert_eq!(direct_post_buy_handoff_capacity(64), 256);
        assert_eq!(direct_post_buy_handoff_capacity(1_000), 256);
    }

    #[test]
    fn direct_post_buy_handoff_distinguishes_full_and_closed() {
        let event = GhostEvent::post_buy_submitted(
            Pubkey::new_unique().to_string(),
            Pubkey::new_unique().to_string(),
            "bounded-direct-handoff",
            0.1,
            0,
            "shadow",
            1,
            None,
            PostBuySource::LiveBuy,
            None,
            None,
            None,
            None,
        );
        let (sender, receiver) = create_direct_post_buy_handoff_channel(1);
        for _ in 0..8 {
            sender
                .try_send(DirectPostBuyHandoff::without_ack(event.clone()))
                .expect("bounded queue should accept exactly its capacity");
        }
        assert!(matches!(
            sender.try_send(DirectPostBuyHandoff::without_ack(event.clone())),
            Err(mpsc::error::TrySendError::Full(_))
        ));

        drop(receiver);
        assert!(matches!(
            sender.try_send(DirectPostBuyHandoff::without_ack(event)),
            Err(mpsc::error::TrySendError::Closed(_))
        ));
    }

    #[test]
    fn shadow_exit_thresholds_use_post_buy_guardian_percent_fields() {
        let supplied_guardian = PostBuyGuardianConfig {
            target_threshold: Some(150.0),
            stoploss_threshold: Some(50.0),
            wait_for_timestop: Some(45_000),
            tick_interval_ms: 777,
            signal_channel_buffer: 31,
            ligma_warning_impact_bps: 1_234.0,
            whf_min_confidence: 0.73,
            tcf_consecutive_low_max: 9,
            panic_rate_window_ms: 2_345,
            exit_policy_v1: ghost_brain::guardian::post_buy::ExitPolicyV1Config {
                quote_recovery_ms: 6_789,
                ..Default::default()
            },
            aem: ghost_brain::aem::config::AemConfig {
                enabled: false,
                ..Default::default()
            },
            ..PostBuyGuardianConfig::default()
        };
        let config = PostBuyRuntimeConfig {
            live_exit_take_profit_pct: 0.02,
            live_exit_stop_loss_pct: 0.02,
            execution_mode: "shadow".to_string(),
            max_concurrent_positions: 17,
            shadow_guardian: Some(supplied_guardian.clone()),
            ..PostBuyRuntimeConfig::default()
        };

        let guardian_config = build_shadow_guardian_config(&config);
        let mut expected_guardian = supplied_guardian;
        expected_guardian.max_monitored_positions = 17;
        assert_eq!(
            serde_json::to_value(&guardian_config).expect("effective guardian JSON"),
            serde_json::to_value(&expected_guardian).expect("expected guardian JSON"),
            "runtime may overlay only max_monitored_positions; all other Guardian fields must survive"
        );
        assert_eq!(guardian_config.target_threshold, Some(150.0));
        assert_eq!(guardian_config.stoploss_threshold, Some(50.0));
        assert_eq!(guardian_config.wait_for_timestop_ms(), 45_000);
        let status = config
            .validate()
            .expect("valid complete shadow config")
            .expect("shadow policy status");
        assert_eq!(status.take_profit_fraction, 1.5);
        assert_eq!(status.stop_loss_fraction, 0.5);
        assert_eq!(status.quote_recovery_ms, 6_789);
        assert!(!status.absolute_max_hold_enabled);
        assert_eq!(status.crash_guard_mode, CrashGuardMode::Disabled);
    }

    #[test]
    fn shadow_stoploss_above_full_loss_is_rejected() {
        let config = PostBuyRuntimeConfig {
            execution_mode: "shadow".to_string(),
            shadow_guardian: Some(PostBuyGuardianConfig {
                target_threshold: Some(50.0),
                stoploss_threshold: Some(250.0),
                wait_for_timestop: Some(30_000),
                aem: ghost_brain::aem::config::AemConfig {
                    enabled: false,
                    ..Default::default()
                },
                ..PostBuyGuardianConfig::default()
            }),
            ..PostBuyRuntimeConfig::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn shadow_position_manager_startup_validation_fails_closed() {
        let guardian = PostBuyGuardianConfig {
            enabled: true,
            target_threshold: Some(50.0),
            stoploss_threshold: Some(50.0),
            wait_for_timestop: Some(30_000),
            exit_policy_v1: ghost_brain::guardian::post_buy::ExitPolicyV1Config {
                quote_recovery_ms: 5_000,
                ..Default::default()
            },
            aem: ghost_brain::aem::config::AemConfig {
                enabled: false,
                ..Default::default()
            },
            ..PostBuyGuardianConfig::default()
        };
        let base = PostBuyRuntimeConfig {
            execution_mode: "shadow".to_string(),
            shadow_guardian: Some(guardian),
            ..PostBuyRuntimeConfig::default()
        };
        assert!(base.validate().is_ok());

        let mut missing_guardian = base.clone();
        missing_guardian.shadow_guardian = None;
        assert!(missing_guardian.validate().is_err());

        let mut guardian_disabled = base.clone();
        guardian_disabled
            .shadow_guardian
            .as_mut()
            .expect("guardian")
            .enabled = false;
        assert!(guardian_disabled.validate().is_err());

        let mut aem_enabled = base.clone();
        aem_enabled
            .shadow_guardian
            .as_mut()
            .expect("guardian")
            .aem
            .enabled = true;
        assert!(aem_enabled.validate().is_err());

        let mut missing_take_profit = base.clone();
        missing_take_profit
            .shadow_guardian
            .as_mut()
            .expect("guardian")
            .target_threshold = None;
        assert!(missing_take_profit.validate().is_err());

        let mut invalid_take_profit = base.clone();
        invalid_take_profit
            .shadow_guardian
            .as_mut()
            .expect("guardian")
            .target_threshold = Some(f64::NAN);
        assert!(invalid_take_profit.validate().is_err());

        let mut invalid_stop_loss = base.clone();
        invalid_stop_loss
            .shadow_guardian
            .as_mut()
            .expect("guardian")
            .stoploss_threshold = Some(-0.1);
        assert!(invalid_stop_loss.validate().is_err());

        let mut zero_inactivity = base.clone();
        zero_inactivity
            .shadow_guardian
            .as_mut()
            .expect("guardian")
            .wait_for_timestop = Some(0);
        assert!(zero_inactivity.validate().is_err());

        let mut zero_quote_recovery = base.clone();
        zero_quote_recovery
            .shadow_guardian
            .as_mut()
            .expect("guardian")
            .exit_policy_v1
            .quote_recovery_ms = 0;
        assert!(zero_quote_recovery.validate().is_err());

        let mut zero_enabled_max_hold = base.clone();
        let policy = &mut zero_enabled_max_hold
            .shadow_guardian
            .as_mut()
            .expect("guardian")
            .exit_policy_v1;
        policy.absolute_max_hold_enabled = true;
        policy.absolute_max_hold_ms = 0;
        assert!(zero_enabled_max_hold.validate().is_err());

        let mut invalid_observe_crash = base;
        let policy = &mut invalid_observe_crash
            .shadow_guardian
            .as_mut()
            .expect("guardian")
            .exit_policy_v1;
        policy.crash_guard_mode = CrashGuardMode::ObserveOnly;
        policy.crash_min_distinct_slots = 1;
        assert!(invalid_observe_crash.validate().is_err());
    }

    #[test]
    fn authoritative_crash_guard_is_allowed_only_for_a_complete_shadow_profile() {
        let guardian = PostBuyGuardianConfig {
            enabled: true,
            target_threshold: Some(50.0),
            stoploss_threshold: Some(50.0),
            wait_for_timestop: Some(30_000),
            exit_policy_v1: ghost_brain::guardian::post_buy::ExitPolicyV1Config {
                crash_guard_mode: CrashGuardMode::AuthoritativeShadow,
                ..Default::default()
            },
            aem: ghost_brain::aem::config::AemConfig {
                enabled: false,
                ..Default::default()
            },
            ..PostBuyGuardianConfig::default()
        };
        let base = PostBuyRuntimeConfig {
            execution_mode: "shadow".to_string(),
            entry_mode: "shadow_only".to_string(),
            shadow_ledger: Some(Arc::new(ShadowLedger::new())),
            shadow_lifecycle_log_path: Some(PathBuf::from("/tmp/crash-guard-lifecycle.jsonl")),
            shadow_guardian: Some(guardian),
            ..PostBuyRuntimeConfig::default()
        };
        assert!(base.validate().is_ok());

        let mut wrong_execution = base.clone();
        wrong_execution.execution_mode = "live".to_string();
        assert!(wrong_execution.validate().is_err());

        let mut wrong_entry_mode = base.clone();
        wrong_entry_mode.entry_mode = "dry_run_mock".to_string();
        assert!(wrong_entry_mode.validate().is_err());

        let mut missing_lifecycle_log = base.clone();
        missing_lifecycle_log.shadow_lifecycle_log_path = None;
        assert!(missing_lifecycle_log.validate().is_err());

        let mut missing_shadow_monitor = base.clone();
        missing_shadow_monitor.shadow_ledger = None;
        assert!(missing_shadow_monitor.validate().is_err());

        let mut live_dispatch_present = base;
        live_dispatch_present.live_sell = Some(test_live_sell_handle(
            "http://127.0.0.1:1".to_string(),
            Arc::new(AccountStateReducer::new()),
        ));
        assert!(live_dispatch_present.validate().is_err());
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

        session.transition_terminal(
            LiveExitStatus::ExitConfirmationUnknown,
            "confirmation_unknown",
        );
        assert!(
            !session.should_release_position_slot(),
            "unknown live confirmation must keep the slot reserved"
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
        assert!(!is_retryable_live_exit_failure(
            LiveExitStatus::ExitConfirmationUnknown
        ));
    }

    #[test]
    fn live_confirmation_unknown_preserves_signature_and_visible_quantity() {
        let mint = Pubkey::new_unique();
        let mut session = seeded_live_exit_session(mint, 10_000_000, 1_500_000);
        session.exit_signature = Some("submitted-signature".to_string());
        session.visible_token_balance = Some(1_234_567);

        session.transition_terminal(
            LiveExitStatus::ExitConfirmationUnknown,
            "confirmation remained inconclusive",
        );

        assert_eq!(session.status, LiveExitStatus::ExitConfirmationUnknown);
        assert_eq!(
            session.exit_signature.as_deref(),
            Some("submitted-signature")
        );
        assert_eq!(session.visible_token_balance, Some(1_234_567));
        assert_eq!(session.sellable_token_amount(), Some(1_234_567));
        assert!(!session.should_release_position_slot());
        assert!(!is_retryable_live_exit_failure(session.status));
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
            entry_mode: "paper".to_string(),
            aem_t_s: 1,
            max_concurrent_positions: 1,
            position_limit_tracker: None,
            live_sell: None,
            live_position_registry: None,
            slippage_tolerance: 0.20,
            live_exit_take_profit_pct: 0.02,
            live_exit_stop_loss_pct: 0.02,
            shadow_guardian: None,
            shadow_ledger: None,
            account_state_core: None,
            shadow_lifecycle_log_path: None,
            probe_lifecycle_log_path: None,
            shadow_v2_burnin: None,
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
        let (direct_tx, direct_rx) = create_direct_post_buy_handoff_channel(1);

        let config = PostBuyRuntimeConfig {
            events_output_path: events_dir.clone(),
            paper_fill_delay_min_ms: 10,
            paper_fill_delay_max_ms: 20,
            tick_interval_ms: 10,
            max_ticks_before_exit: 2,
            execution_mode: "paper".to_string(),
            entry_mode: "paper".to_string(),
            aem_t_s: 1,
            max_concurrent_positions: 1,
            position_limit_tracker: None,
            live_sell: None,
            live_position_registry: None,
            slippage_tolerance: 0.20,
            live_exit_take_profit_pct: 0.02,
            live_exit_stop_loss_pct: 0.02,
            shadow_guardian: None,
            shadow_ledger: None,
            account_state_core: None,
            shadow_lifecycle_log_path: None,
            probe_lifecycle_log_path: None,
            shadow_v2_burnin: None,
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
            .try_send(DirectPostBuyHandoff::without_ack(post_buy))
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
        let (direct_tx, direct_rx) = create_direct_post_buy_handoff_channel(1);

        let config = PostBuyRuntimeConfig {
            events_output_path: events_dir.clone(),
            paper_fill_delay_min_ms: 10,
            paper_fill_delay_max_ms: 20,
            tick_interval_ms: 10,
            max_ticks_before_exit: 2,
            execution_mode: "paper".to_string(),
            entry_mode: "paper".to_string(),
            aem_t_s: 1,
            max_concurrent_positions: 1,
            position_limit_tracker: None,
            live_sell: None,
            live_position_registry: None,
            slippage_tolerance: 0.20,
            live_exit_take_profit_pct: 0.02,
            live_exit_stop_loss_pct: 0.02,
            shadow_guardian: None,
            shadow_ledger: None,
            account_state_core: None,
            shadow_lifecycle_log_path: None,
            probe_lifecycle_log_path: None,
            shadow_v2_burnin: None,
        };

        drop(event_tx);
        let runtime_handle = tokio::spawn(run(event_rx, shutdown_rx, Some(direct_rx), config));
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;

        let pool_amm_id = Pubkey::new_unique().to_string();
        let base_mint = Pubkey::new_unique().to_string();
        let signature = "direct_handoff_closed_sig";
        let expected_candidate_id = format!("{}_{}_{}", base_mint, pool_amm_id, signature);
        direct_tx
            .try_send(DirectPostBuyHandoff::without_ack(
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
        let opened_at_ms = now_ms().saturating_sub(42_000);
        let handoff = handle_shadow_post_buy_handoff(
            Some(&monitoring_engine),
            &candidate_id,
            &pool_amm_id,
            &base_mint,
            0.25,
            Some(250_000),
            Some(777),
            Some(777),
            Some(opened_at_ms),
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
                                if event.envelope.candidate_id == candidate_id {
                                    if let EventKind::PositionOpened(payload) = event.kind {
                                        assert_eq!(event.envelope.event_time_ms, opened_at_ms);
                                        assert_eq!(payload.entry_time_ms, opened_at_ms);
                                        saw_position_opened = true;
                                    }
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
        let guardian_config = PostBuyGuardianConfig {
            enabled: true,
            tick_interval_ms: 5,
            target_threshold: Some(50.0),
            stoploss_threshold: Some(50.0),
            wait_for_timestop: Some(1),
            exit_policy_v1: ghost_brain::guardian::post_buy::ExitPolicyV1Config {
                quote_recovery_ms: 1,
                ..Default::default()
            },
            aem: ghost_brain::aem::config::AemConfig {
                enabled: false,
                ..Default::default()
            },
            ..PostBuyGuardianConfig::default()
        };
        let config = PostBuyRuntimeConfig {
            execution_mode: "shadow".to_string(),
            shadow_ledger: Some(Arc::clone(&shadow_ledger)),
            shadow_guardian: Some(guardian_config.clone()),
            shadow_lifecycle_log_path: Some(shadow_lifecycle_log_path.clone()),
            probe_lifecycle_log_path: Some(probe_lifecycle_log_path.clone()),
            ..PostBuyRuntimeConfig::default()
        };
        let (shadow_signal_tx, _shadow_signal_rx) =
            mpsc::channel(guardian_config.signal_channel_buffer.max(1));
        let mut shadow_monitor = MonitoringEngine::try_new(
            guardian_config.clone(),
            Arc::clone(&shadow_ledger),
            shadow_signal_tx,
        )
        .expect("valid shadow Position Manager config");
        shadow_monitor.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(
            Arc::new(RwLock::new(ShadowPositionBook::new())),
        )));
        shadow_monitor.set_shadow_lifecycle_log_path(Some(shadow_lifecycle_log_path.clone()));
        let shadow_monitor = Arc::new(shadow_monitor);

        let (probe_signal_tx, _probe_signal_rx) =
            mpsc::channel(guardian_config.signal_channel_buffer.max(1));
        let mut probe_monitor =
            MonitoringEngine::try_new(guardian_config, Arc::clone(&shadow_ledger), probe_signal_tx)
                .expect("valid probe Position Manager config");
        probe_monitor.set_position_router(Arc::new(PositionRuntimeRouter::with_shadow_book(
            Arc::new(RwLock::new(ShadowPositionBook::new())),
        )));
        probe_monitor.set_shadow_lifecycle_log_path(Some(probe_lifecycle_log_path.clone()));
        let probe_terminal_harness =
            init_probe_position_manager_terminal_harness(tmp_dir.path(), "test-probe-terminal-run")
                .expect("probe terminal harness");
        let probe_terminal_path = probe_terminal_harness
            .canonical_event_stream_path()
            .to_path_buf();
        probe_monitor
            .set_shadow_v2_validation_harness(Arc::new(ParkingMutex::new(probe_terminal_harness)));
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
        .with_execution_join_metadata(join_metadata)
        .with_entry_simulation_rpc_slot(Some(777));

        let mut epoch_counter = 1;
        let mut lifecycle_handles = Vec::new();
        let mut recent_handoffs = RecentPostBuyCache::default();
        let shadow_v2_harness: Option<Arc<ParkingMutex<ShadowV2ValidationHarness>>> = None;
        let ack = handle_post_buy_event(
            event,
            &config,
            &lifecycle,
            Some(&shadow_monitor),
            Some(&probe_monitor),
            &mut epoch_counter,
            &mut lifecycle_handles,
            &mut recent_handoffs,
            &shadow_v2_harness,
        )
        .await;

        assert_eq!(ack, DirectPostBuyHandoffAck::Accepted);
        assert_eq!(shadow_monitor.active_position_count(), 0);
        assert_eq!(probe_monitor.active_position_count(), 1);
        assert!(lifecycle_handles.is_empty());

        let probe_runtime_handle = Arc::clone(&probe_monitor).start();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if probe_monitor.active_position_count() == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("probe Position Manager must commit terminal truth and remove the position");
        probe_runtime_handle.abort();
        let _ = probe_runtime_handle.await;

        assert_eq!(probe_monitor.active_position_count(), 0);
        let canonical_probe_rows: Vec<serde_json::Value> =
            std::fs::read_to_string(&probe_terminal_path)
                .expect("probe canonical terminal stream")
                .lines()
                .map(|line| serde_json::from_str(line).expect("canonical probe row"))
                .collect();
        let canonical_probe_terminals: Vec<&serde_json::Value> = canonical_probe_rows
            .iter()
            .filter(|row| row["event_kind"] == "TERMINAL_TRUTH")
            .collect();
        assert_eq!(canonical_probe_terminals.len(), 1);
        let canonical_probe_terminal = canonical_probe_terminals[0];
        assert_eq!(
            canonical_probe_terminal["payload"]["record"]["truth_slot"],
            serde_json::Value::Null
        );
        assert_eq!(
            canonical_probe_terminal["payload"]["record"]["terminal_observed_slot"],
            serde_json::Value::Null
        );
        assert_eq!(
            canonical_probe_terminal["payload"]["record"]["terminal_slot"],
            serde_json::Value::Null
        );

        let second_handoff = handle_shadow_post_buy_handoff(
            Some(&probe_monitor),
            "probe-lifecycle-handoff-second",
            &pool_amm_id.to_string(),
            &base_mint.to_string(),
            0.007,
            Some(250_000),
            Some(778),
            Some(778),
            None,
            2,
            PositionJoinMetadata {
                probe_id: Some("probe-lifecycle-handoff-second".to_string()),
                dispatch_source: Some("counterfactual_shadow_probe".to_string()),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(second_handoff.ack, DirectPostBuyHandoffAck::Accepted);
        assert_eq!(probe_monitor.active_position_count(), 1);
        probe_monitor.remove_position_administratively(&base_mint);

        let probe_rows =
            std::fs::read_to_string(&probe_lifecycle_log_path).expect("probe lifecycle row");
        let first_row: serde_json::Value =
            serde_json::from_str(probe_rows.lines().next().expect("first row"))
                .expect("valid probe lifecycle json");
        assert_eq!(first_row["probe_id"], probe_id);
        assert_eq!(first_row["dispatch_source"], "counterfactual_shadow_probe");
        assert_eq!(first_row["entry_simulation_rpc_slot"], 777);
        assert_eq!(first_row["entry_market_anchor_slot"], 777);
        assert_eq!(
            first_row["entry_market_anchor_source"],
            "shadow_simulation_rpc_context"
        );
        assert!(first_row.get("entry_market_anchor_tx_signature").is_none());
        assert_eq!(first_row["entry_landed_slot"], 778);
        assert_eq!(
            first_row["entry_landed_slot_source"],
            "synthetic_next_slot_after_entry_simulation_rpc_slot"
        );
        assert_eq!(first_row["exit_sample_slot"], serde_json::Value::Null);
        assert_eq!(
            first_row["exit_market_anchor_slot"],
            serde_json::Value::Null
        );
        assert!(first_row.get("exit_market_anchor_tx_signature").is_none());
        assert!(first_row["exit_reason_evaluation_ts_ms"].as_u64().is_some());
        assert_eq!(first_row["exit_landed_slot"], serde_json::Value::Null);
        assert_eq!(
            first_row["exit_landed_slot_source"],
            serde_json::Value::Null
        );
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
            Some(111),
            None,
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
            Some(222),
            None,
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
    async fn shadow_terminal_watcher_releases_reserved_slot_after_blocked_outcome() {
        let pool_amm_id = Pubkey::new_unique().to_string();
        let mint_pubkey = Pubkey::new_unique();
        let base_mint = mint_pubkey.to_string();
        let candidate_id = format!("{}_{}_{}", base_mint, pool_amm_id, 1234);
        let tracker = PositionLimitTracker::new(1);
        let slot_owner = Pubkey::new_unique();
        let slot_id = PositionSlotId::derive(&slot_owner, &mint_pubkey);
        tracker
            .register_existing(slot_id, pool_amm_id.clone(), base_mint.clone())
            .expect("slot must register");
        assert_eq!(tracker.active_positions(), 1);

        let (terminal_tx, terminal_rx) = oneshot::channel();
        let watcher =
            spawn_shadow_terminal_watcher(terminal_rx, tracker.clone(), slot_id, candidate_id);
        terminal_tx
            .send(ShadowTerminalDisposition::SimulationBlocked {
                action_id: "shadow-action:1".to_string(),
                reason: ShadowUnresolvedReason::BlockedByData,
            })
            .expect("terminal receiver must remain active");
        watcher.await.expect("watcher should finish");
        assert_eq!(tracker.active_positions(), 0);
    }

    #[tokio::test]
    async fn shadow_terminal_watcher_releases_reserved_slot_after_resolved_close() {
        let mint = Pubkey::new_unique();
        let tracker = PositionLimitTracker::new(1);
        let slot_id = PositionSlotId::derive(&Pubkey::new_unique(), &mint);
        tracker
            .register_existing(slot_id, Pubkey::new_unique().to_string(), mint.to_string())
            .expect("slot must register");

        let (terminal_tx, terminal_rx) = oneshot::channel();
        let watcher = spawn_shadow_terminal_watcher(
            terminal_rx,
            tracker.clone(),
            slot_id,
            "candidate-shadow-closed".to_string(),
        );
        terminal_tx
            .send(ShadowTerminalDisposition::SimulatedClosed {
                action_id: "shadow-action:closed".to_string(),
                reason: "target".to_string(),
            })
            .expect("terminal receiver must remain active");
        watcher.await.expect("watcher should finish");
        assert_eq!(tracker.active_positions(), 0);
    }

    #[tokio::test]
    async fn shadow_terminal_watcher_releases_reserved_slot_when_channel_is_dropped() {
        let mint = Pubkey::new_unique();
        let tracker = PositionLimitTracker::new(1);
        let slot_id = PositionSlotId::derive(&Pubkey::new_unique(), &mint);
        tracker
            .register_existing(slot_id, Pubkey::new_unique().to_string(), mint.to_string())
            .expect("slot must register");

        let (terminal_tx, terminal_rx) = oneshot::channel();
        let watcher = spawn_shadow_terminal_watcher(
            terminal_rx,
            tracker.clone(),
            slot_id,
            "candidate-shadow-dropped".to_string(),
        );
        drop(terminal_tx);
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
            Some(landed_slot),
            None,
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
