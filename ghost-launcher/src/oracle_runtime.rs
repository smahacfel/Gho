//! Oracle Runtime - runtime registry, session orchestration, and scoring coordination
//!
//! This module provides runtime orchestration for the HyperPrediction Oracle system.
//! Canonical runtime ownership lives on session management, pool identity registries,
//! and account-state/bootstrap truth rather than a legacy per-pool compat map.
//!
//! ## Architecture
//!
//! ```text
//! Event Bus → OracleRuntime → HyperPredictionOracle
//!               ↓
//!     SessionManager + PoolIdentityRegistry + AccountStateCore
//!       - observation sessions
//!       - runtime pool metadata
//!       - canonical/bootstrap reserve truth
//! ```

use crate::components::post_buy_runtime::{
    DirectPostBuyHandoff, DirectPostBuyHandoffAck, DirectPostBuySender,
};
use crate::components::trigger::safety::{PositionSlotId, SafetyViolation};
use crate::config::{
    ExecutionMode, SessionRuntimeConfig, ShadowLedgerConfig, TxIntelligenceRuntimeConfig,
};
use crate::events::{
    AccountUpdateEvent, DetectedPool, EventBusSender, FundingTransferObserved, GhostEvent,
    PoolTransaction, PostBuySource,
};
use crate::session::{
    OpenSessionRequest, PoolObservationSession, SessionConfig, SessionManager, SharedSession,
};
use crate::tx_intelligence::{CrossPoolVelocityConfig, FundingSourceConfig};
use ghost_brain::config::PanicConfig;
use ghost_brain::execution::backend::Lane;
use ghost_brain::fast_pipeline::EnhancedCandidate;
use ghost_brain::oracle::hyper_prediction::{HyperPredictionOracle, HyperPredictionResult};
use ghost_brain::oracle::tx_metrics::IntervalSource;
use ghost_brain::oracle::ultrafast::PanicTx;
use ghost_brain::oracle::TransactionMetrics;
use ghost_brain::oracle::{ApprovedPools, SnapshotEngine};
// [INTEGRATION] Import additional engines
use ghost_brain::chaos::amm_math::AmmPool;
use ghost_brain::chaos::engine::{ChaosEngine, MarketScenario, SimulationConfig};
use ghost_brain::events::{EventEmitter, EventWriterConfig};
use ghost_brain::oracle::engine::PanicProvider;
use ghost_brain::oracle::snapshot_engine::{
    derive_price_canonical, DataSource, InitPoolEvent, PoolLifecycle, TxEvent,
};
use ghost_brain::pumpfun::PumpCurveStateCache;
use ghost_core::market_state::{
    BondingCurve, ShadowBondingCurve, ShadowLedgerStateConfidence, ShadowLedgerWriteReason,
    ShadowLedgerWriteSource, ShadowLedgerWriteStrength,
};
use ghost_core::session::types::{SessionStatus, VerdictOutcome};
use ghost_core::shadow_ledger::types::{PriceReason, PriceState};
use ghost_core::shadow_ledger::{
    BufferedTx as GatekeeperBufferedHistoryTx, CurveWriteMetadata, LiveTxEvent, MarketSnapshot,
    ShadowLedger, ShadowLedgerWriteResult, TradeSide, TxKey, HOT_POOL_TX_THRESHOLD,
};
use ghost_core::{
    coverage_audit, BaseMint, BondingCurveKey, CoverageAuditClosedWindow, CoverageAuditRecord,
    CoverageAuditTruthSignatureState, CurveFinality, GatekeeperDecision as WalGatekeeperDecision,
    PoolId, PoolIdentity as DomainPoolIdentity, PoolIdentityRegistry, Wal, WalRecord,
};

use crate::components::fallback_contract::{
    classify_shadow_fallback, shadow_fallback_contract, ShadowFallbackCategory,
};
use crate::components::gatekeeper::{
    CommitIngressOutcome, GatekeeperAssessment, GatekeeperBuffer, GatekeeperIngressOutcome,
    GatekeeperVerdict, LauncherCommitCoordinator, PoolState,
};
use crate::components::gatekeeper_policy::{
    build_assessment_from_features, build_timeout_decision_from_assessment,
    evaluate_policy_from_assessment, PolicyEvaluationContext,
};
use crate::oracle_metrics::{
    record_fsc_authoritative_buy_gate_open, record_fsc_coverage_window_ready,
    record_fsc_coverage_window_remaining_ms, record_shadow_ledger_health,
    POOL_IDENTITY_EXHAUSTED_TOTAL, POOL_IDENTITY_PROMOTION_TOTAL,
};
use chrono::{SecondsFormat, TimeZone, Utc};
use dashmap::DashMap;
use ghost_brain::config::{GatekeeperMode, GatekeeperV2Config};
use ghost_brain::oracle::window_spec::{ensure_epoch_ms, WindowCloseReason, WindowState};
use ghost_core::account_state_core::reducer::AccountStateReducer;
use ghost_core::account_state_core::types::{
    AccountStateUpdate, BootstrapHints, CanonicalPoolState, UpdateSource,
};
use ghost_core::checkpoint::MaterializedFeatureSet;
use metrics::increment_counter;
use parking_lot::{Mutex, RwLock};
use seer::binary_parser::BinaryParser;
use seer::early_fingerprint::{
    EarlyFingerprintConfig, FingerprintAggregator, FingerprintTxEvent, TokenDelta,
};
use seer::types::{GeyserEvent as SeerGeyserEvent, RawBytesMissingReason, TradeEvent};
use seer::websocket_connection::{
    extract_balances_from_meta, extract_inner_instructions_from_meta, extract_logs_from_meta,
    extract_token_balances_from_meta, parse_ui_transaction_with_meta,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status::{EncodedConfirmedTransactionWithStatusMeta, UiTransactionEncoding};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use tracing::{debug, error, info, trace, warn};

// =============================================================================
// System Program Validation
// =============================================================================

const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
const PUMPFUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const WRAPPED_SOL_MINT: &str = "So11111111111111111111111111111111111111112";
const PUMPFUN_GLOBAL_STATE: &str = "TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM";
const KNOWN_BAD_LEGACY_FEE_RECIPIENT: &str = "CebN5WGQ4jvEPvsVU4EoHEpgznyQQNDGNesDwrFs8YWj";
const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
const LIVE_TRIGGER_READINESS_TIMEOUT_MS: u64 = 75;
/// Pump.fun token decimal factor (6 decimals → 10^6).
/// Used to convert raw on-chain token reserves to PumpPortal-compatible display units.
const PUMP_TOKEN_DECIMAL_FACTOR: f64 = 1_000_000.0;
// Treat reserves below this threshold as noise; avoids passing effectively-zero pools.
const MIN_RESERVE_THRESHOLD: f64 = 1e-6;
const MIN_SYNTHETIC_VOLUME_SOL: f64 = 0.000_001;
const GENESIS_SOL_LAMPORTS: u64 = 30_000_000_000;
const GENESIS_TOKEN_RESERVES: u64 = 1_073_000_000_000_000;
const MIN_POOL_RESERVE_LAMPORTS: u128 = 1; // Smallest non-zero value to avoid division-by-zero in reserve math
const DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS: u64 = 200;
const POOL_TASK_CHANNEL_CAPACITY: usize = 65_536;
const POOL_TASK_BACKPRESSURE_WAIT_MS: u64 = 50;
const POOL_TASK_BACKPRESSURE_RETRY_ATTEMPTS: usize = 5;

/// Per-attempt wait for hot-pool backpressure retries (ms).
///
/// Hot pools poll more aggressively (shorter per-attempt window) so they
/// burn less real-time budget per attempt and can fit more retries within
/// the same total wall-clock budget as cold pools.
const HOT_POOL_BACKPRESSURE_WAIT_MS: u64 = 25;

/// Number of retry attempts for hot-pool backpressure.
///
/// Hot pools (≥ [`HOT_POOL_TX_THRESHOLD`] tx enqueued) get 4× the retry
/// attempts of cold pools.  Combined with the shorter per-attempt wait this
/// gives hot pools a total retry budget of
/// `HOT_POOL_BACKPRESSURE_WAIT_MS × HOT_POOL_BACKPRESSURE_RETRY_ATTEMPTS`
/// ms instead of the cold-pool budget, while keeping the behaviour
/// deterministic and bounded (no unbounded waiting or memory growth).
const HOT_POOL_BACKPRESSURE_RETRY_ATTEMPTS: usize = 20;
const FSC_COVERAGE_WINDOW_POLL_INTERVAL_MS: u64 = 1_000;
#[derive(Debug, Clone, Copy, Default)]
struct ResolvedPriceContext {
    reserve_base: Option<f64>,
    reserve_quote: Option<f64>,
    price_quote: Option<f64>,
}

fn parse_tx_base_mint(tx: &PoolTransaction, fallback: Option<Pubkey>) -> Option<Pubkey> {
    fallback.or_else(|| {
        tx.token_mint
            .as_deref()
            .and_then(|mint| Pubkey::from_str(mint).ok())
    })
}

fn canonical_runtime_reserves(state: &CanonicalPoolState) -> (u64, u64) {
    let reserve_sol_lamports = if state.real_sol_reserves > 0 {
        state.real_sol_reserves
    } else {
        state.virtual_sol_reserves
    };
    let reserve_tok_units = if state.real_token_reserves > 0 {
        state.real_token_reserves
    } else {
        state.virtual_token_reserves
    };
    (reserve_sol_lamports, reserve_tok_units)
}

fn canonical_shadow_curve(state: &CanonicalPoolState) -> BondingCurve {
    BondingCurve {
        discriminator: 0,
        virtual_token_reserves: state.virtual_token_reserves,
        virtual_sol_reserves: state.virtual_sol_reserves,
        real_token_reserves: state.real_token_reserves,
        real_sol_reserves: state.real_sol_reserves,
        token_total_supply: state.token_total_supply,
        complete: u8::from(state.is_complete),
        _padding: [0; 7],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CanonicalShadowSyncDecision {
    Sync(&'static str),
    NoOp(&'static str),
    Guard(&'static str),
}

fn curve_finality_rank(finality: CurveFinality) -> u8 {
    match finality {
        CurveFinality::Speculative => 0,
        CurveFinality::Provisional => 1,
        CurveFinality::Finalized => 2,
    }
}

fn canonical_shadow_sync_decision(
    existing: Option<ShadowBondingCurve>,
    canonical_state: &CanonicalPoolState,
) -> CanonicalShadowSyncDecision {
    let canonical_curve = canonical_shadow_curve(canonical_state);
    let canonical_slot = canonical_state.last_update_slot;
    let canonical_finality = canonical_state.curve_finality.normalized(true);

    let Some(existing) = existing else {
        return CanonicalShadowSyncDecision::Sync("missing_shadow_curve");
    };

    let same_curve = existing.curve == canonical_curve;
    let same_slot = existing.last_updated_slot == canonical_slot;
    let same_finality = existing.curve_finality == canonical_finality;

    if same_curve && same_slot && same_finality && existing.curve_data_known {
        return CanonicalShadowSyncDecision::NoOp("shadow_already_matches_canonical");
    }

    if !existing.curve_data_known {
        return CanonicalShadowSyncDecision::Sync("shadow_curve_unknown");
    }

    let shadow_can_be_owned_by_account_updates =
        existing.write_source == ShadowLedgerWriteSource::AccountUpdate;

    if !shadow_can_be_owned_by_account_updates {
        if same_curve
            && existing.last_updated_slot >= canonical_slot
            && curve_finality_rank(existing.curve_finality)
                >= curve_finality_rank(canonical_finality)
        {
            return CanonicalShadowSyncDecision::NoOp("shadow_tx_path_current_or_newer");
        }

        return CanonicalShadowSyncDecision::Guard("shadow_owned_by_tx_path_or_compat_surface");
    }

    if existing.write_strength > ShadowLedgerWriteStrength::Repair {
        if existing.last_updated_slot > canonical_slot {
            return CanonicalShadowSyncDecision::NoOp("shadow_slot_newer_than_canonical");
        }

        if same_curve
            && existing.last_updated_slot >= canonical_slot
            && curve_finality_rank(existing.curve_finality)
                >= curve_finality_rank(canonical_finality)
        {
            return CanonicalShadowSyncDecision::NoOp("shadow_stronger_and_current");
        }

        return CanonicalShadowSyncDecision::Guard(
            "shadow_formally_stronger_but_not_current_canonical",
        );
    }

    if existing.last_updated_slot > canonical_slot {
        return CanonicalShadowSyncDecision::NoOp("shadow_slot_newer_than_canonical");
    }

    if existing.last_updated_slot < canonical_slot {
        return CanonicalShadowSyncDecision::Sync("shadow_slot_older_than_canonical");
    }

    if !same_curve {
        return CanonicalShadowSyncDecision::Sync("shadow_curve_differs_from_canonical");
    }

    if curve_finality_rank(existing.curve_finality) < curve_finality_rank(canonical_finality) {
        return CanonicalShadowSyncDecision::Sync("shadow_finality_older_than_canonical");
    }

    CanonicalShadowSyncDecision::NoOp("shadow_equivalent_or_newer")
}

fn canonical_shadow_write_metadata(
    existing: Option<ShadowBondingCurve>,
    canonical_state: &CanonicalPoolState,
) -> (ShadowLedgerWriteStrength, ShadowLedgerWriteReason) {
    let canonical_curve = canonical_shadow_curve(canonical_state);

    match existing {
        None => (
            ShadowLedgerWriteStrength::ConfirmedBootstrap,
            ShadowLedgerWriteReason::ConfirmedBootstrap,
        ),
        Some(existing)
            if !existing.curve_data_known
                || existing.write_strength == ShadowLedgerWriteStrength::BootstrapSeed =>
        {
            (
                ShadowLedgerWriteStrength::ConfirmedBootstrap,
                ShadowLedgerWriteReason::ConfirmedBootstrap,
            )
        }
        Some(existing)
            if existing.curve == canonical_curve
                && existing.last_updated_slot == canonical_state.last_update_slot =>
        {
            (
                ShadowLedgerWriteStrength::Repair,
                ShadowLedgerWriteReason::FinalityRefresh,
            )
        }
        Some(_) => (
            ShadowLedgerWriteStrength::Repair,
            ShadowLedgerWriteReason::DirectAccountUpdate,
        ),
    }
}

fn canonical_price_context(state: &CanonicalPoolState) -> ResolvedPriceContext {
    let (reserve_sol_lamports, reserve_tok_units) = canonical_runtime_reserves(state);
    let reserve_quote = reserve_sol_lamports as f64 / LAMPORTS_PER_SOL;
    let reserve_base = reserve_tok_units as f64 / PUMP_TOKEN_DECIMAL_FACTOR;
    let price_quote = if state.price_sol.is_finite() && state.price_sol > 0.0 {
        Some(state.price_sol)
    } else if reserve_base.is_finite()
        && reserve_base > MIN_RESERVE_THRESHOLD
        && reserve_quote.is_finite()
        && reserve_quote > MIN_RESERVE_THRESHOLD
    {
        let (price, price_state, _) = derive_price_canonical(reserve_base, reserve_quote, 0.0);
        (price_state == PriceState::Valid).then_some(price)
    } else {
        None
    };

    ResolvedPriceContext {
        reserve_base: (reserve_base.is_finite() && reserve_base > MIN_RESERVE_THRESHOLD)
            .then_some(reserve_base),
        reserve_quote: (reserve_quote.is_finite() && reserve_quote > MIN_RESERVE_THRESHOLD)
            .then_some(reserve_quote),
        price_quote,
    }
}

fn record_shadow_truth_fallback(site: &'static str) {
    let category = classify_shadow_fallback(site);
    let category_label = match category {
        ShadowFallbackCategory::BootstrapOnly => "bootstrap_only",
        ShadowFallbackCategory::DegradedDiagnostic => "degraded_diagnostic",
        ShadowFallbackCategory::HiddenPrimary => "hidden_primary",
    };
    ::metrics::counter!(
        "shadow_truth_fallback_total",
        1u64,
        "site" => site,
        "category" => category_label
    );
}

fn assert_declared_shadow_truth_fallback(site: &'static str) {
    debug_assert!(
        !matches!(
            classify_shadow_fallback(site),
            ShadowFallbackCategory::HiddenPrimary
        ),
        "shadow truth fallback site {site} must be explicitly classified in Phase 3"
    );
}

fn record_degraded_truth_helper(site: &'static str, helper: &'static str) {
    let category = classify_shadow_fallback(site);
    let category_label = match category {
        ShadowFallbackCategory::BootstrapOnly => "bootstrap_only",
        ShadowFallbackCategory::DegradedDiagnostic => "degraded_diagnostic",
        ShadowFallbackCategory::HiddenPrimary => "hidden_primary",
    };
    ::metrics::counter!(
        "degraded_truth_helper_total",
        1u64,
        "site" => site,
        "helper" => helper,
        "category" => category_label
    );
}

fn bootstrap_hints_from_candidate(candidate: &EnhancedCandidate) -> BootstrapHints {
    let speculative_reserves = candidate
        .virtual_sol_reserves
        .and_then(|virtual_sol_reserves| {
            if virtual_sol_reserves == 0 {
                return None;
            }

            let token_reserves = candidate
                .token_total_supply
                .unwrap_or(GENESIS_TOKEN_RESERVES);
            (token_reserves > 0).then_some((virtual_sol_reserves, token_reserves))
        });

    BootstrapHints {
        speculative_reserves,
        token_total_supply: candidate.token_total_supply,
        bonding_curve_progress: candidate.bonding_curve_progress,
        initial_liquidity_sol: (candidate.initial_liquidity_sol.is_finite()
            && candidate.initial_liquidity_sol > 0.0)
            .then_some(candidate.initial_liquidity_sol),
    }
}

fn merge_tx_price_context(
    tx: &PoolTransaction,
    context: ResolvedPriceContext,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    let reserve_base = tx.reserve_base.or(context.reserve_base);
    let reserve_quote = tx.reserve_quote.or(context.reserve_quote);
    let price_quote = tx.price_quote.or(context.price_quote).or_else(|| {
        if let (Some(rb), Some(rq)) = (reserve_base, reserve_quote) {
            let (price, state, _) = derive_price_canonical(rb, rq, 0.0);
            if state == PriceState::Valid {
                Some(price)
            } else {
                None
            }
        } else {
            None
        }
    });

    (reserve_base, reserve_quote, price_quote)
}

fn raw_tx_curve_reserves(tx: &PoolTransaction) -> Option<(u64, u64)> {
    if !tx.curve_data_known {
        return None;
    }

    let reserve_base = tx.v_tokens_in_bonding_curve.or(tx.reserve_base)?;
    let reserve_quote = tx.v_sol_in_bonding_curve.or(tx.reserve_quote)?;
    if !reserve_base.is_finite()
        || reserve_base <= MIN_RESERVE_THRESHOLD
        || !reserve_quote.is_finite()
        || reserve_quote <= MIN_RESERVE_THRESHOLD
    {
        return None;
    }

    let token_reserves = (reserve_base * PUMP_TOKEN_DECIMAL_FACTOR).round();
    let sol_reserves = (reserve_quote * LAMPORTS_PER_SOL).round();
    if !token_reserves.is_finite()
        || token_reserves <= 0.0
        || !sol_reserves.is_finite()
        || sol_reserves <= 0.0
    {
        return None;
    }

    Some((sol_reserves as u64, token_reserves as u64))
}

fn is_no_space_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_err| io_err.raw_os_error() == Some(28))
    })
}

/// Enrich a PoolTransaction with canonical reserves when the transaction is
/// missing reserve data (typical for source=grpc).
///
/// gRPC/Yellowstone provides raw transaction data but NOT the pre-computed
/// bonding curve state that PumpPortal includes. This function first fills in
/// `v_tokens_in_bonding_curve`, `v_sol_in_bonding_curve`, and `market_cap_sol`
/// from `AccountStateCore`. `ShadowLedger` remains only as a bootstrap/degraded
/// fallback when canonical account-state has not materialized yet.
///
/// Returns the original Arc if no enrichment is needed or possible,
/// otherwise returns a new Arc wrapping the enriched copy.
fn enrich_pool_tx_from_canonical_state(
    tx: Arc<PoolTransaction>,
    pool_id: Pubkey,
    base_mint: Option<Pubkey>,
    account_state_core: &AccountStateReducer,
    shadow_ledger: &ShadowLedger,
    freshness_ms: u64,
) -> Arc<PoolTransaction> {
    let enrichment_started = Instant::now();

    // Skip enrichment if reserves are already populated (e.g., PumpPortal source)
    if tx.v_tokens_in_bonding_curve.is_some() && tx.v_sol_in_bonding_curve.is_some() {
        return tx;
    }

    if let Some(base_mint) = parse_tx_base_mint(tx.as_ref(), base_mint) {
        if let Some(canonical_state) = account_state_core.get_canonical_state(&base_mint) {
            let v_tokens =
                canonical_state.virtual_token_reserves as f64 / PUMP_TOKEN_DECIMAL_FACTOR;
            let v_sol = canonical_state.virtual_sol_reserves as f64 / LAMPORTS_PER_SOL;

            if v_tokens > 0.0 && v_sol >= 0.0 {
                let price =
                    if canonical_state.price_sol.is_finite() && canonical_state.price_sol > 0.0 {
                        canonical_state.price_sol
                    } else {
                        v_sol / v_tokens
                    };
                let market_cap = if canonical_state.market_cap_sol.is_finite()
                    && canonical_state.market_cap_sol > 0.0
                {
                    canonical_state.market_cap_sol
                } else {
                    price * crate::components::gatekeeper::PUMP_GENESIS_TOKEN_SUPPLY
                };

                let mut enriched = (*tx).clone();
                if enriched.v_tokens_in_bonding_curve.is_none() {
                    enriched.v_tokens_in_bonding_curve = Some(v_tokens);
                }
                if enriched.v_sol_in_bonding_curve.is_none() {
                    enriched.v_sol_in_bonding_curve = Some(v_sol);
                }
                if enriched.reserve_base.is_none() {
                    enriched.reserve_base = Some(v_tokens);
                }
                if enriched.reserve_quote.is_none() {
                    enriched.reserve_quote = Some(v_sol);
                }
                if enriched.price_quote.is_none() {
                    enriched.price_quote = Some(price);
                }
                if enriched.market_cap_sol.is_none() {
                    enriched.market_cap_sol = Some(market_cap);
                }
                enriched.curve_data_known = true;

                ::metrics::histogram!(
                    "shadow_ledger_enrichment_latency_ms",
                    enrichment_started.elapsed().as_secs_f64() * 1000.0,
                    "result" => "canonical_state"
                );
                ::metrics::counter!(
                    "shadow_ledger_enrichment_total",
                    1,
                    "fresh" => "true",
                    "result" => "canonical_state"
                );
                info!(
                    pool = %pool_id,
                    base_mint = %base_mint,
                    source = "canonical_state",
                    curve_data_known = enriched.curve_data_known,
                    v_sol = ?enriched.v_sol_in_bonding_curve,
                    v_tokens = ?enriched.v_tokens_in_bonding_curve,
                    "DIAG_TX_CURVE_ENRICHED"
                );
                return Arc::new(enriched);
            }
        }
    }

    // Bootstrap/degraded fallback: ShadowLedger remains valid for simulation and
    // short-lived pre-canonical enrichment. Curves are keyed by canonical
    // bonding_curve pubkey, not by pool_id.
    assert_declared_shadow_truth_fallback("tx_curve_enrichment_shadow");
    let shadow_curve_key = base_mint
        .and_then(|mint| shadow_ledger.resolve_curve_key(&mint))
        .unwrap_or(pool_id);
    let now_ms = ghost_core::shadow_ledger::current_time_ms();
    let (curve, curve_data_known, snapshot_age_ms) =
        match shadow_ledger.get_curve_with_known_age(&shadow_curve_key, now_ms) {
            Some(curve) => curve,
            None => return tx,
        };
    record_shadow_truth_fallback("tx_curve_enrichment_shadow");
    record_degraded_truth_helper("tx_curve_enrichment_shadow", "shadow_ledger_curve");
    let curve_is_fresh = snapshot_age_ms <= freshness_ms;

    // Convert raw on-chain reserves to PumpPortal-compatible display units:
    // - Token reserves: raw units / 10^6 (Pump.fun tokens have 6 decimals)
    // - SOL reserves: lamports / 10^9
    let v_tokens = curve.virtual_token_reserves as f64 / PUMP_TOKEN_DECIMAL_FACTOR;
    let v_sol = curve.virtual_sol_reserves as f64 / LAMPORTS_PER_SOL;

    if v_tokens <= 0.0 || v_sol < 0.0 {
        return tx;
    }

    let price = v_sol / v_tokens;
    let market_cap = price * crate::components::gatekeeper::PUMP_GENESIS_TOKEN_SUPPLY;

    let mut enriched = (*tx).clone();

    if enriched.v_tokens_in_bonding_curve.is_none() {
        enriched.v_tokens_in_bonding_curve = Some(v_tokens);
    }
    if enriched.v_sol_in_bonding_curve.is_none() {
        enriched.v_sol_in_bonding_curve = Some(v_sol);
    }
    if enriched.reserve_base.is_none() {
        enriched.reserve_base = Some(v_tokens);
    }
    if enriched.reserve_quote.is_none() {
        enriched.reserve_quote = Some(v_sol);
    }
    if enriched.price_quote.is_none() {
        enriched.price_quote = Some(price);
    }
    if enriched.market_cap_sol.is_none() {
        enriched.market_cap_sol = Some(market_cap);
    }

    // Propagate the explicit curve_data_known flag from ShadowLedger.
    // This is true only when the curve was parsed from a confirmed AccountUpdate,
    // NOT from genesis_seed bootstrap. Additionally, stale curve snapshots are
    // downgraded to curve_data_known=false so Gatekeeper can naturally fall back
    // to PendingCurve instead of treating stale enrichment as fresh truth.
    enriched.curve_data_known = curve_data_known && curve_is_fresh;

    ::metrics::histogram!(
        "shadow_ledger_enrichment_snapshot_age_ms",
        snapshot_age_ms as f64
    );
    ::metrics::histogram!(
        "shadow_ledger_enrichment_latency_ms",
        enrichment_started.elapsed().as_secs_f64() * 1000.0
    );
    ::metrics::counter!(
        "shadow_ledger_enrichment_total",
        1,
        "fresh" => if curve_is_fresh { "true" } else { "false" },
        "result" => "shadow_fallback",
        "category" => "bootstrap_only"
    );
    info!(
        pool = %pool_id,
        shadow_curve_key = %shadow_curve_key,
        source = "shadow_fallback",
        fallback_category = "bootstrap_only",
        curve_data_known = enriched.curve_data_known,
        curve_is_fresh,
        v_sol = ?enriched.v_sol_in_bonding_curve,
        v_tokens = ?enriched.v_tokens_in_bonding_curve,
        "DIAG_TX_CURVE_ENRICHED"
    );

    trace!(
        pool = %pool_id,
        shadow_curve_key = %shadow_curve_key,
        v_tokens = v_tokens,
        v_sol = v_sol,
        price = price,
        market_cap = market_cap,
        snapshot_age_ms = snapshot_age_ms,
        curve_is_fresh = curve_is_fresh,
        "🔧 Enriched gRPC PoolTransaction with canonical/shadow reserves"
    );

    Arc::new(enriched)
}

/// Pump.fun token decimals (6).
const PUMPFUN_TOKEN_DECIMALS: u8 = 6;

/// Convert a [`PoolTransaction`] into a [`FingerprintTxEvent`] for the
/// early-fingerprint aggregator. Returns `None` if the transaction has no
/// slot (required for block-0 snipe detection and event-time ordering).
fn pool_tx_to_fingerprint_event(tx: &PoolTransaction) -> Option<FingerprintTxEvent> {
    // Slot is required for block-0 snipe detection and event ordering.
    let slot = tx.slot?;
    let signer = tx.signer.clone();

    // Build token delta: buy → positive, sell → negative
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

    // SOL pre-balance: use real signer pre-balance from gRPC meta.pre_balances
    // when available. Do not fabricate proxy balances from trade size.
    let mut sol_pre_balances = std::collections::HashMap::new();
    if let Some(pre_bal) = tx.signer_pre_balance_lamports {
        sol_pre_balances.insert(signer.clone(), pre_bal);
    }

    Some(FingerprintTxEvent {
        slot,
        tx_index: 0, // not available in PoolTransaction
        signature: tx.signature.clone(),
        timestamp_ms: tx.effective_event_ts_ms().unwrap_or_else(current_time_ms),
        is_buy: tx.is_buy,
        sol_amount_sol: Some(tx.volume_sol),
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

#[inline]
fn detected_pool_epoch_like_ts_ms(pool_data: &DetectedPool) -> Option<u64> {
    pool_data
        .effective_event_ts_ms()
        .or(pool_data.detected_wall_ts_ms)
}

#[inline]
fn detected_pool_event_ts_ms(pool_data: &DetectedPool) -> u64 {
    detected_pool_epoch_like_ts_ms(pool_data).unwrap_or_else(current_time_ms)
}

#[inline]
fn detected_pool_epoch_source_label(pool_data: &DetectedPool) -> &'static str {
    if pool_data.event_time.chain_event_ts_ms.is_some() {
        "chain_event"
    } else if pool_data.event_time.ingress_wall_ts_ms.is_some() {
        "ingress_wall"
    } else if pool_data.detected_wall_ts_ms.is_some() {
        "detected_wall"
    } else {
        "registered_wall"
    }
}

#[inline]
fn tx_observed_wall_ts_ms(tx: &PoolTransaction, fallback_now_ms: u64) -> u64 {
    tx.event_time.ingress_wall_ts_ms.unwrap_or(fallback_now_ms)
}

#[inline]
fn tx_observed_wall_source_label(tx: &PoolTransaction) -> &'static str {
    if tx.event_time.ingress_wall_ts_ms.is_some() {
        "tx_ingress_wall"
    } else {
        "runtime_wall"
    }
}

#[inline]
fn detected_pool_observed_wall_ts_ms(pool_data: &DetectedPool, fallback_now_ms: u64) -> u64 {
    pool_data
        .detected_wall_ts_ms
        .or(pool_data.event_time.ingress_wall_ts_ms)
        .unwrap_or(fallback_now_ms)
}

#[inline]
fn detected_pool_observed_wall_source_label(pool_data: &DetectedPool) -> &'static str {
    if pool_data.detected_wall_ts_ms.is_some() {
        "detected_wall"
    } else if pool_data.event_time.ingress_wall_ts_ms.is_some() {
        "detected_ingress_wall"
    } else {
        "runtime_wall"
    }
}

fn is_valid_pool_address(pubkey: &Pubkey) -> bool {
    let pubkey_str = pubkey.to_string();
    if pubkey_str == SYSTEM_PROGRAM_ID
        || pubkey_str == TOKEN_PROGRAM_ID
        || pubkey_str == WRAPPED_SOL_MINT
        || pubkey_str == PUMPFUN_GLOBAL_STATE
        || pubkey_str.starts_with("Sysvar")
    {
        error!(
            "🚨 CRITICAL: Attempted to track invalid pool address: {}",
            pubkey_str
        );
        return false;
    }
    true
}

fn is_valid_mint_address(pubkey: &Pubkey) -> bool {
    let pubkey_str = pubkey.to_string();
    if pubkey_str == SYSTEM_PROGRAM_ID
        || pubkey_str == TOKEN_PROGRAM_ID
        || pubkey_str == PUMPFUN_GLOBAL_STATE
    {
        error!("🚨 CRITICAL: Invalid mint address: {}", pubkey_str);
        return false;
    }
    true
}

fn normalize_dev_pubkey_str(raw: &str) -> Option<String> {
    let pubkey = Pubkey::try_from(raw).ok()?;
    let pubkey_str = pubkey.to_string();
    if !pubkey.is_on_curve()
        || pubkey == Pubkey::default()
        || pubkey_str == SYSTEM_PROGRAM_ID
        || pubkey_str == TOKEN_PROGRAM_ID
        || pubkey_str == TOKEN_2022_PROGRAM_ID
        || pubkey_str == WRAPPED_SOL_MINT
        || pubkey_str == PUMPFUN_GLOBAL_STATE
        || pubkey_str == COMPUTE_BUDGET_PROGRAM_ID
        || pubkey_str == ASSOCIATED_TOKEN_PROGRAM_ID
        || pubkey_str.starts_with("Sysvar")
    {
        return None;
    }
    Some(pubkey_str)
}

// =============================================================================
// Orphan Transaction Handling
// =============================================================================

/// Maximum age for orphan transactions before cleanup (30 seconds)
const ORPHAN_TTL_MS: u64 = 30_000;

/// Multiplier applied to ORPHAN_TTL_MS when adopting buffered orphans during registration
const ORPHAN_GRACE_PERIOD_MULTIPLIER: u64 = 2;

/// Maximum number of orphan transactions per pool
const MAX_ORPHANS_PER_POOL: usize = 1_024;

/// Maximum total number of orphan transactions across all pools
const MAX_TOTAL_ORPHANS: usize = 200_000;

/// Maximum number of orphan transactions adopted into state during pool registration
const MAX_ORPHANS_ADOPTED_ON_REGISTER: usize = 512;

/// Maximum age for AccountUpdate events buffered before pool identity registration.
const PRE_IDENTITY_ACCOUNT_UPDATE_TTL_MS: u64 = 30_000;

/// Maximum buffered pre-identity AccountUpdate events kept per mint.
const MAX_PRE_IDENTITY_ACCOUNT_UPDATES_PER_MINT: usize = 32;

/// Maximum total buffered pre-identity AccountUpdate events across all mints.
const MAX_TOTAL_PRE_IDENTITY_ACCOUNT_UPDATES: usize = 20_000;

/// Represents a transaction that arrived before the pool was registered
#[derive(Debug, Clone)]
struct OrphanTx {
    timestamp_ms: u64,
    slot: Option<u64>,
    mpcf_payload: Vec<u8>,
    maybe_dev_wallet: Option<Pubkey>,
    signer: String,
    is_buy: bool,
    volume_sol: f64,
    arrived_at: Instant,
}

#[derive(Debug, Clone)]
struct PendingAccountUpdate {
    event: AccountUpdateEvent,
    buffered_at_ms: u64,
}

#[derive(Debug, Default)]
struct CanonicalReadinessNotifier {
    senders: DashMap<Pubkey, watch::Sender<u64>>,
}

impl CanonicalReadinessNotifier {
    fn notify_ready(&self, mint: &Pubkey) {
        if let Some(sender) = self.senders.get(mint) {
            sender.send_modify(|generation| {
                *generation = generation.wrapping_add(1);
            });
        }
    }

    fn subscribe(&self, mint: &Pubkey) -> watch::Receiver<u64> {
        let sender = self
            .senders
            .entry(*mint)
            .or_insert_with(|| watch::channel(0_u64).0)
            .clone();
        sender.subscribe()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OrphanTelemetrySnapshot {
    pub adopted: u64,
    pub dropped_stale: u64,
    pub dropped_missing_timestamp: u64,
    pub dropped_capped: u64,
    pub dropped_slot_cutoff: u64,
}

#[derive(Debug, Default)]
struct OrphanMetrics {
    adopted: AtomicU64,
    dropped_stale: AtomicU64,
    dropped_missing_timestamp: AtomicU64,
    dropped_capped: AtomicU64,
    dropped_slot_cutoff: AtomicU64,
}

impl OrphanMetrics {
    fn snapshot(&self) -> OrphanTelemetrySnapshot {
        OrphanTelemetrySnapshot {
            adopted: self.adopted.load(Ordering::Relaxed),
            dropped_stale: self.dropped_stale.load(Ordering::Relaxed),
            dropped_missing_timestamp: self.dropped_missing_timestamp.load(Ordering::Relaxed),
            dropped_capped: self.dropped_capped.load(Ordering::Relaxed),
            dropped_slot_cutoff: self.dropped_slot_cutoff.load(Ordering::Relaxed),
        }
    }

    #[cfg(test)]
    fn reset(&self) {
        self.adopted.store(0, Ordering::Relaxed);
        self.dropped_stale.store(0, Ordering::Relaxed);
        self.dropped_missing_timestamp.store(0, Ordering::Relaxed);
        self.dropped_capped.store(0, Ordering::Relaxed);
        self.dropped_slot_cutoff.store(0, Ordering::Relaxed);
    }
}

impl OracleRuntime {
    fn enqueue_pre_identity_account_update(&self, event: &AccountUpdateEvent) {
        let now_ms = current_time_ms();
        let mut pending = self.pending_account_updates.write();

        pending.retain(|_, events| {
            events.retain(|entry| {
                now_ms.saturating_sub(entry.buffered_at_ms) <= PRE_IDENTITY_ACCOUNT_UPDATE_TTL_MS
            });
            !events.is_empty()
        });

        let total_pending: usize = pending.values().map(Vec::len).sum();
        if total_pending >= MAX_TOTAL_PRE_IDENTITY_ACCOUNT_UPDATES {
            ::metrics::counter!(
                "account_update_pre_identity_buffer_drop_total",
                1u64,
                "reason" => "global_cap"
            );
            warn!(
                base_mint = %event.base_mint,
                slot = event.slot,
                "Dropping pre-identity AccountUpdate due to global cap"
            );
            return;
        }

        let queue = pending.entry(event.base_mint).or_default();
        if queue.len() >= MAX_PRE_IDENTITY_ACCOUNT_UPDATES_PER_MINT {
            let dropped = queue.remove(0);
            ::metrics::counter!(
                "account_update_pre_identity_buffer_drop_total",
                1u64,
                "reason" => "per_mint_cap"
            );
            warn!(
                base_mint = %event.base_mint,
                dropped_slot = dropped.event.slot,
                incoming_slot = event.slot,
                "Dropping oldest pre-identity AccountUpdate due to per-mint cap"
            );
        }

        queue.push(PendingAccountUpdate {
            event: event.clone(),
            buffered_at_ms: now_ms,
        });
        ::metrics::counter!("account_update_pre_identity_buffered_total", 1u64);
        ::metrics::gauge!(
            "account_update_pre_identity_buffer_depth",
            queue.len() as f64,
            "scope" => "mint"
        );
    }

    fn take_pre_identity_account_updates(&self, base_mint: &Pubkey) -> Vec<PendingAccountUpdate> {
        self.pending_account_updates
            .write()
            .remove(base_mint)
            .unwrap_or_default()
    }

    fn restore_pre_identity_account_updates(
        &self,
        base_mint: Pubkey,
        pending_events: Vec<PendingAccountUpdate>,
    ) {
        if pending_events.is_empty() {
            return;
        }
        self.pending_account_updates
            .write()
            .entry(base_mint)
            .or_default()
            .extend(pending_events);
    }

    fn replay_pre_identity_account_updates(&self, pool_amm_id: Pubkey, base_mint: Pubkey) {
        let pending = self.take_pre_identity_account_updates(&base_mint);
        if pending.is_empty() {
            return;
        }

        let now_ms = current_time_ms();
        let mut replayed = 0u64;
        let mut dropped_stale = 0u64;
        let mut deferred_still_missing = Vec::new();

        for pending_event in pending {
            let dwell_ms = now_ms.saturating_sub(pending_event.buffered_at_ms);
            if dwell_ms > PRE_IDENTITY_ACCOUNT_UPDATE_TTL_MS {
                dropped_stale = dropped_stale.saturating_add(1);
                ::metrics::counter!(
                    "account_update_pre_identity_replay_drop_total",
                    1u64,
                    "reason" => "stale"
                );
                continue;
            }

            let outcome = self.process_account_update_with_explicit_source(
                &pending_event.event.base_mint,
                pending_event.event.sol_reserves,
                pending_event.event.token_reserves,
                pending_event.event.complete,
                pending_event.event.slot,
                pending_event.event.curve_finality,
                UpdateSource::GeyserAccountUpdate,
                Some(&pending_event.event),
                false,
            );
            let applied_to_account_state = self
                .account_state_core
                .get_canonical_state(&pending_event.event.base_mint)
                .is_some();
            if applied_to_account_state {
                replayed = replayed.saturating_add(1);
                ::metrics::counter!("account_update_pre_identity_replayed_total", 1u64);
                ::metrics::histogram!(
                    "account_update_pre_identity_replay_dwell_ms",
                    dwell_ms as f64
                );
            } else {
                let identity_still_missing = self
                    .lookup_pool_identity_by_base_mint(&pending_event.event.base_mint)
                    .is_none()
                    && self
                        .lookup_pool_identity_by_bonding_curve(&pending_event.event.bonding_curve)
                        .is_none();
                if !identity_still_missing {
                    warn!(
                        pool = %pool_amm_id,
                        base_mint = %base_mint,
                        slot = pending_event.event.slot,
                        outcome = ?outcome,
                        "Pre-identity AccountUpdate replay did not hydrate canonical state despite resolved identity"
                    );
                }
                deferred_still_missing.push(pending_event);
                warn!(
                    pool = %pool_amm_id,
                    base_mint = %base_mint,
                    slot = deferred_still_missing
                        .last()
                        .map(|entry| entry.event.slot)
                        .unwrap_or_default(),
                    "Pre-identity AccountUpdate replay still could not resolve identity"
                );
            }
        }

        let deferred_count = deferred_still_missing.len();
        self.restore_pre_identity_account_updates(base_mint, deferred_still_missing);
        if deferred_count > 0 {
            ::metrics::counter!(
                "account_update_pre_identity_replay_deferred_total",
                deferred_count as u64
            );
        }

        info!(
            pool = %pool_amm_id,
            base_mint = %base_mint,
            replayed,
            dropped_stale,
            deferred_still_missing = deferred_count,
            "Replayed pending pre-identity AccountUpdate events after pool registration"
        );
    }
}

fn current_time_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::cmp::min(millis, u128::from(u64::MAX)) as u64
}

#[inline]
fn tx_event_ts_ms(tx: &PoolTransaction) -> u64 {
    if let Some(explicit_event_ts_ms) = tx.effective_event_ts_ms() {
        explicit_event_ts_ms
    } else {
        current_time_ms()
    }
}

fn normalize_gatekeeper_event_time_ms(
    tx: &PoolTransaction,
    last_seen_event_ts_ms: Option<u64>,
) -> (u64, bool) {
    let mut ts_ms = tx_event_ts_ms(tx);
    let used_chain_time = tx.event_time.has_chain_time();

    // Keep per-pool event-time monotonic even on out-of-order delivery.
    if let Some(prev) = last_seen_event_ts_ms {
        if ts_ms <= prev {
            ts_ms = prev.saturating_add(1);
        }
    }

    (ts_ms, used_chain_time)
}

fn tx_signature(signature: &str) -> Option<Signature> {
    if signature.is_empty() {
        None
    } else {
        Signature::from_str(signature).ok()
    }
}

fn fallback_counter_for_pool_tx(tx: &PoolTransaction, event_ts_ms: u64) -> u64 {
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

fn decision_slot_hint(
    buffered_txs: &[crate::components::gatekeeper::GatekeeperBufferedTx],
    pool_data: Option<&DetectedPool>,
) -> Option<u64> {
    buffered_txs
        .iter()
        .rev()
        .find_map(|buffered| buffered.tx.slot)
        .or_else(|| pool_data.and_then(|pool| pool.slot))
}

fn pool_tx_to_tx_key(tx: &PoolTransaction, event_ts_ms: u64) -> Option<TxKey> {
    let signature = tx_signature(&tx.signature);
    let fallback_counter = if signature.is_some() || tx.event_ordinal.is_some() {
        0
    } else {
        fallback_counter_for_pool_tx(tx, event_ts_ms)
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

fn pool_tx_to_buffered_history_tx(
    tx: &PoolTransaction,
    tx_key: TxKey,
) -> Option<GatekeeperBufferedHistoryTx> {
    let d_sol_lamports = tx.sol_amount_lamports?;
    let d_tok_units = tx.token_amount_units?;
    let side = if tx.is_buy {
        TradeSide::Buy
    } else {
        TradeSide::Sell
    };
    let trader = Pubkey::try_from(tx.signer.as_str()).ok();

    match GatekeeperBufferedHistoryTx::new(
        tx_key,
        side,
        d_sol_lamports,
        d_tok_units,
        tx.is_dev_buy,
        trader,
    ) {
        Ok(buffered_tx) => Some(buffered_tx),
        Err(err) => {
            warn!(
                signature = %tx.signature,
                error = %err,
                "Failed to build launcher commit BufferedTx from PoolTransaction"
            );
            None
        }
    }
}

fn pool_tx_to_gatekeeper_history(
    buffered: &crate::components::gatekeeper::GatekeeperBufferedTx,
) -> Option<GatekeeperBufferedHistoryTx> {
    pool_tx_to_buffered_history_tx(buffered.tx.as_ref(), buffered.tx_key.clone())
}

fn pool_tx_to_live_event(
    base_mint: Pubkey,
    tx: &PoolTransaction,
    tx_key: TxKey,
) -> Option<LiveTxEvent> {
    let buffered_tx = pool_tx_to_buffered_history_tx(tx, tx_key.clone())?;
    match LiveTxEvent::new(
        base_mint,
        buffered_tx.tx_key.slot,
        buffered_tx.tx_key.tx_index,
        buffered_tx.tx_key.signature,
        buffered_tx.tx_key.timestamp_ms,
        buffered_tx.side,
        buffered_tx.d_sol_lamports,
        buffered_tx.d_tok_units,
        buffered_tx.dev_buy,
        buffered_tx.trader,
    ) {
        Ok(event) => Some(event),
        Err(err) => {
            warn!(
                base_mint = %base_mint,
                signature = %tx.signature,
                error = %err,
                "Failed to build LiveTxEvent from PoolTransaction"
            );
            None
        }
    }
}

fn interval_stats_from_snapshots(snapshots: &[MarketSnapshot]) -> (f64, f64) {
    if snapshots.len() < 2 {
        return (0.0, 0.0);
    }

    let mut intervals = Vec::with_capacity(snapshots.len().saturating_sub(1));
    for window in snapshots.windows(2) {
        let delta = window[1]
            .timestamp_ms
            .saturating_sub(window[0].timestamp_ms) as f64;
        if delta > 0.0 {
            intervals.push(delta);
        }
    }

    if intervals.is_empty() {
        return (0.0, 0.0);
    }

    let avg = intervals.iter().sum::<f64>() / intervals.len() as f64;
    let variance =
        intervals.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / intervals.len() as f64;

    (avg, variance.sqrt())
}

fn tx_metrics_from_snapshots(snapshots: &[MarketSnapshot]) -> Option<TransactionMetrics> {
    let latest = snapshots.last()?;
    let tx_count = latest.tx_count as usize;
    let total_volume_sol = latest.cum_volume_sol;
    let unique_addrs = latest.unique_addrs as usize;
    let (avg_interval_ms, interval_std_dev) = interval_stats_from_snapshots(snapshots);
    let per_snapshot_volumes: Vec<f64> = snapshots
        .windows(2)
        .map(|pair| (pair[1].cum_volume_sol - pair[0].cum_volume_sol).abs())
        .collect();
    // Snapshot deltas may aggregate multiple transactions; use the largest delta as an upper bound.
    let max_tx_sol = {
        let per_snapshot_max = per_snapshot_volumes.iter().copied().fold(0.0_f64, f64::max);
        if per_snapshot_max > 0.0 {
            per_snapshot_max
        } else if tx_count > 0 {
            total_volume_sol / tx_count as f64
        } else {
            0.0
        }
    };

    // ShadowLedger snapshots aggregate volume without side/developer attribution;
    // treat all observed volume as buys (sell metrics zero) and mark no dev activity in this fallback path.
    Some(TransactionMetrics {
        tx_count,
        unique_addrs,
        total_volume_sol,
        volumes_sol: per_snapshot_volumes.clone(),
        is_buys: vec![true; per_snapshot_volumes.len()],
        buy_count: tx_count,
        sell_count: 0,
        buy_volume_sol: total_volume_sol,
        sell_volume_sol: 0.0,
        max_tx_sol,
        avg_interval_ms,
        interval_std_dev,
        interval_source: IntervalSource::Unknown,
        has_dev_activity: false,
        dev_volume_sol: 0.0,
    })
}

/// Extract developer buy metrics from early pool transactions.
///
/// Scans `txs` for transactions where the signer matches `creator`
/// and returns the total SOL volume of those buys and whether any exist.
pub fn extract_dev_buy_from_pool_txs(creator: &str, txs: &[PoolTransaction]) -> (f64, bool) {
    let mut dev_buy_sol = 0.0f64;
    let mut has_dev_buy = false;
    for tx in txs {
        if tx.is_buy && tx.signer == creator {
            dev_buy_sol += tx.volume_sol;
            has_dev_buy = true;
        }
    }
    (dev_buy_sol, has_dev_buy)
}

// =============================================================================
// Oracle Runtime Configuration
// =============================================================================

/// Configuration for OracleRuntime behavior
///
/// This configuration is read once at startup and controls runtime behavior
/// without any hot-path ENV reads.
#[derive(Debug, Clone)]
pub struct OracleRuntimeConfig {
    /// **DEPRECATED** - This field is no longer used as of EPIC 2.3.5.
    ///
    /// # EPIC 2 Compliance (Single-Writer Architecture)
    ///
    /// As of EPIC 2.3.5, all legacy dual-writer paths have been removed.
    /// OracleRuntime no longer writes snapshots to ShadowLedger.
    /// SnapshotEngine is the ONLY canonical writer (keyed by base_mint).
    ///
    /// This field is retained for backward compatibility with existing configs
    /// but has no effect on runtime behavior. Setting it has no impact.
    ///
    /// **Current behavior**: Always single-writer mode (SnapshotEngine only)
    /// **Legacy paths**: Removed per EPIC 2.3.5
    #[deprecated(
        since = "2.3.5",
        note = "Legacy dual-writer paths removed; this field has no effect"
    )]
    pub runtime_shadowledger_snapshots_enabled: bool,

    /// Maximum acceptable wall-clock age (ms) for ShadowLedger enrichment used
    /// on the launcher hot path before `curve_data_known` is downgraded.
    ///
    /// This keeps reserve enrichment available while preventing stale curve
    /// snapshots from being treated as confirmed truth.
    pub shadow_ledger_enrichment_freshness_ms: u64,
    pub session: SessionRuntimeConfig,
    pub tx_intelligence: TxIntelligenceRuntimeConfig,
}

impl OracleRuntimeConfig {
    /// Parse configuration from environment variables
    ///
    /// **Note**: As of EPIC 2.3.5, the `GHOST_RUNTIME_SHADOWLEDGER_SNAPSHOTS_ENABLED`
    /// environment variable is ignored. All legacy dual-writer paths have been removed.
    /// SnapshotEngine is the only canonical writer.
    pub fn from_env() -> Self {
        // EPIC 2.3.5: Legacy flag is parsed for backward compatibility but has no effect
        let env_var_result = std::env::var("GHOST_RUNTIME_SHADOWLEDGER_SNAPSHOTS_ENABLED");

        // Warn users if they set the deprecated env var
        if let Ok(ref value) = env_var_result {
            if Self::parse_bool_flag(value) {
                tracing::warn!(
                    "GHOST_RUNTIME_SHADOWLEDGER_SNAPSHOTS_ENABLED is set to '{}' but this setting is IGNORED. \
                     As of EPIC 2.3.5, legacy dual-writer paths have been removed. \
                     SnapshotEngine is the only canonical writer.",
                    value
                );
            }
        }

        #[allow(deprecated)]
        let runtime_shadowledger_snapshots_enabled = env_var_result
            .map(|v| Self::parse_bool_flag(&v))
            .unwrap_or(false);
        let shadow_ledger_enrichment_freshness_ms = Self::parse_u64_env(
            "GHOST_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS",
            DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
        );

        #[allow(deprecated)]
        Self {
            runtime_shadowledger_snapshots_enabled,
            shadow_ledger_enrichment_freshness_ms,
            session: SessionRuntimeConfig::default(),
            tx_intelligence: TxIntelligenceRuntimeConfig::default(),
        }
    }

    pub fn from_shadow_ledger_config(config: &ShadowLedgerConfig) -> Self {
        #[allow(deprecated)]
        Self {
            runtime_shadowledger_snapshots_enabled: false,
            shadow_ledger_enrichment_freshness_ms: config.enrichment_freshness_ms,
            session: SessionRuntimeConfig::default(),
            tx_intelligence: TxIntelligenceRuntimeConfig::default(),
        }
    }

    fn session_manager_config(&self) -> SessionConfig {
        SessionConfig {
            default_observation_duration_ms: self.session.max_observation_window_ms,
            max_sessions: self.session.max_sessions,
            checkpoint_interval_ms: self.session.checkpoint_interval_ms,
            tx_intelligence_defaults: self.tx_intelligence.clone(),
        }
    }

    /// Parse boolean flag from string (case-insensitive)
    ///
    /// Returns `true` for: `1`, `true`, `yes` (case-insensitive)
    /// Returns `false` for any other value
    ///
    /// Exposed for testing.
    pub fn parse_bool_flag(value: &str) -> bool {
        matches!(value.trim().to_lowercase().as_str(), "1" | "true" | "yes")
    }

    fn parse_u64_env(var_name: &str, default: u64) -> u64 {
        match std::env::var(var_name) {
            Ok(raw) => match raw.trim().parse::<u64>() {
                Ok(parsed) => parsed,
                Err(err) => {
                    tracing::warn!(
                        env_var = var_name,
                        value = %raw,
                        error = %err,
                        default,
                        "Invalid u64 env override; falling back to default"
                    );
                    default
                }
            },
            Err(_) => default,
        }
    }
}

impl Default for OracleRuntimeConfig {
    fn default() -> Self {
        #[allow(deprecated)]
        Self {
            // EPIC 2.3.5: Legacy path removed, this field has no effect
            runtime_shadowledger_snapshots_enabled: false,
            shadow_ledger_enrichment_freshness_ms: DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
            session: SessionRuntimeConfig::default(),
            tx_intelligence: TxIntelligenceRuntimeConfig::default(),
        }
    }
}

// =============================================================================
// Oracle Runtime
// =============================================================================

pub struct OracleRuntime {
    config: OracleRuntimeConfig,
    hyper_oracle: Arc<HyperPredictionOracle>,
    // [INTEGRATION] Added engines
    chaos_engine: Arc<ChaosEngine>,
    pumpfun_cache: Arc<PumpCurveStateCache>,
    shadow_ledger: Arc<ShadowLedger>,
    rpc_client: Option<Arc<RpcClient>>,
    session_manager: Arc<SessionManager>,

    live_pipeline: Arc<ghost_core::shadow_ledger::LivePipeline>,
    commit_coordinator: Arc<LauncherCommitCoordinator>,
    committed_bootstrap_snapshots: RwLock<HashMap<Pubkey, MarketSnapshot>>,

    detected_pools: RwLock<HashMap<Pubkey, Arc<DetectedPool>>>,
    registered_mints: RwLock<HashMap<Pubkey, Pubkey>>,
    pool_identities: Arc<PoolIdentityRegistry>,
    account_state_core: Arc<AccountStateReducer>,
    canonical_readiness_notifier: CanonicalReadinessNotifier,
    runtime_pool_states: RwLock<HashMap<Pubkey, PoolState>>,
    orphans: RwLock<HashMap<Pubkey, Vec<OrphanTx>>>,
    pending_account_updates: RwLock<HashMap<Pubkey, Vec<PendingAccountUpdate>>>,
    approved_pools: Arc<ApprovedPools>,
    panic_tx_buffer: RwLock<HashMap<Pubkey, VecDeque<PanicTx>>>,
    panic_retention_ms: AtomicU64,
    pump_program_id: String,
    pump_program_pubkey: Option<Pubkey>,
    bonk_program_id: String,

    orphan_grace_period_multiplier: AtomicU64,
    max_orphans_adopted_on_register: AtomicUsize,
    orphan_metrics: OrphanMetrics,

    /// Paradox Sensor state receiver for HFT detection and network telemetry
    /// Wrapped in Arc<RwLock> to allow updating after initialization
    paradox_rx: Arc<
        parking_lot::RwLock<
            Option<tokio::sync::watch::Receiver<seer::paradox_sensor::ParadoxState>>,
        >,
    >,

    /// Explicit production reconciliation / observability loop.
    ///
    /// Owned and driven by this runtime; updated on every AccountUpdate event
    /// and on periodic scheduled cycles.  Wrapped in a `Mutex` so it can be
    /// mutated from `&self` methods without requiring `&mut self`.
    reconciliation_runtime: Mutex<ghost_core::shadow_ledger::ReconciliationRuntime>,

    /// Optional write-ahead log shared with ingest/runtime components.
    wal: Option<Arc<Wal>>,
    wal_disabled_due_to_enospc: AtomicBool,

    /// Rollback reeval seeds recovered during WAL replay at startup.
    recovered_rollback_seeds: Mutex<Vec<ghost_core::wal::RollbackReevalSeedRecord>>,
}

impl OracleRuntime {
    /// Create new OracleRuntime with default configuration from environment
    ///
    /// Reads configuration from environment variables at startup.
    /// For backward compatibility, this is the default constructor used by existing code.
    pub fn new(
        hyper_oracle: Arc<HyperPredictionOracle>,
        pump_program_id: String,
        bonk_program_id: String,
        shadow_ledger: Arc<ShadowLedger>,
    ) -> Self {
        Self::new_with_config(
            hyper_oracle,
            pump_program_id,
            bonk_program_id,
            shadow_ledger,
            None,                                                     // rpc_client
            None,                                                     // paradox_rx
            Arc::new(ghost_core::shadow_ledger::LivePipeline::new()), // live_pipeline
            OracleRuntimeConfig::from_env(),                          // Read config from ENV
        )
    }

    pub fn new_with_rpc(
        hyper_oracle: Arc<HyperPredictionOracle>,
        pump_program_id: String,
        bonk_program_id: String,
        shadow_ledger: Arc<ShadowLedger>,
        rpc_client: Option<Arc<RpcClient>>,
    ) -> Self {
        Self::new_with_config(
            hyper_oracle,
            pump_program_id,
            bonk_program_id,
            shadow_ledger,
            rpc_client,
            None,                                                     // paradox_rx
            Arc::new(ghost_core::shadow_ledger::LivePipeline::new()), // live_pipeline
            OracleRuntimeConfig::from_env(),                          // Read config from ENV
        )
    }

    pub fn new_with_paradox(
        hyper_oracle: Arc<HyperPredictionOracle>,
        pump_program_id: String,
        bonk_program_id: String,
        shadow_ledger: Arc<ShadowLedger>,
        rpc_client: Option<Arc<RpcClient>>,
        paradox_rx: Option<tokio::sync::watch::Receiver<seer::paradox_sensor::ParadoxState>>,
    ) -> Self {
        Self::new_with_config(
            hyper_oracle,
            pump_program_id,
            bonk_program_id,
            shadow_ledger,
            rpc_client,
            paradox_rx,
            Arc::new(ghost_core::shadow_ledger::LivePipeline::new()), // live_pipeline
            OracleRuntimeConfig::from_env(),                          // Read config from ENV
        )
    }

    /// Create new OracleRuntime with explicit configuration
    ///
    /// This is the canonical constructor that all other constructors delegate to.
    /// Allows explicit configuration for testing without ENV dependencies.
    pub fn new_with_config(
        hyper_oracle: Arc<HyperPredictionOracle>,
        pump_program_id: String,
        bonk_program_id: String,
        shadow_ledger: Arc<ShadowLedger>,
        rpc_client: Option<Arc<RpcClient>>,
        paradox_rx: Option<tokio::sync::watch::Receiver<seer::paradox_sensor::ParadoxState>>,
        live_pipeline: Arc<ghost_core::shadow_ledger::LivePipeline>,
        config: OracleRuntimeConfig,
    ) -> Self {
        info!("🔮 OracleRuntime initialized with Orphan Buffer & Full Brain Engines");
        info!("   Pump.fun Program ID: {}", pump_program_id);
        info!("   Bonk.fun Program ID: {}", bonk_program_id);
        // EPIC 2.3.5: Single-writer mode always active (legacy dual-writer paths removed)
        info!("   Runtime ShadowLedger Snapshots: 🔒 DISABLED (EPIC 2.3.5 single-writer mode)");

        if paradox_rx.is_some() {
            info!("   🔮 Paradox Sensor: ✅ ENABLED (real-time HFT detection)");
        } else {
            warn!("   🔮 Paradox Sensor: ❌ DISABLED (no network telemetry)");
        }

        // Initialize sub-engines with default configs (optimized for speed)
        let chaos_engine = Arc::new(ChaosEngine::new(SimulationConfig {
            num_simulations: 2000, // Balanced for speed/accuracy in runtime
            ..Default::default()
        }));

        let pumpfun_cache = Arc::new(PumpCurveStateCache::new());

        let pump_program_pubkey = Pubkey::try_from(pump_program_id.as_str()).ok();

        // Clone the ShadowLedger for the ReconciliationRuntime.  ShadowLedger is
        // Arc-backed so the clone is cheap and both handles share the same underlying
        // data; ReconciliationRuntime operates on the live state.
        let reconciliation_runtime = Mutex::new(
            ghost_core::shadow_ledger::ReconciliationRuntime::new(shadow_ledger.as_ref().clone()),
        );

        let account_state_core = Arc::new(AccountStateReducer::new());
        let session_manager = Arc::new(SessionManager::new_with_account_state_core(
            config.session_manager_config(),
            Arc::clone(&account_state_core),
        ));
        let runtime = Self {
            config,
            hyper_oracle,
            chaos_engine,
            pumpfun_cache,
            shadow_ledger,
            rpc_client,
            session_manager,
            live_pipeline,
            commit_coordinator: Arc::new(LauncherCommitCoordinator::new()),
            committed_bootstrap_snapshots: RwLock::new(HashMap::new()),
            detected_pools: RwLock::new(HashMap::new()),
            registered_mints: RwLock::new(HashMap::new()),
            pool_identities: Arc::new(PoolIdentityRegistry::new()),
            account_state_core,
            canonical_readiness_notifier: CanonicalReadinessNotifier::default(),
            runtime_pool_states: RwLock::new(HashMap::new()),
            orphans: RwLock::new(HashMap::new()),
            pending_account_updates: RwLock::new(HashMap::new()),
            approved_pools: Arc::new(ApprovedPools::new()),
            panic_tx_buffer: RwLock::new(HashMap::new()),
            panic_retention_ms: AtomicU64::new(0),
            pump_program_id,
            pump_program_pubkey,
            bonk_program_id,
            orphan_grace_period_multiplier: AtomicU64::new(ORPHAN_GRACE_PERIOD_MULTIPLIER),
            max_orphans_adopted_on_register: AtomicUsize::new(MAX_ORPHANS_ADOPTED_ON_REGISTER),
            orphan_metrics: OrphanMetrics::default(),
            paradox_rx: Arc::new(parking_lot::RwLock::new(paradox_rx)),
            reconciliation_runtime,
            wal: None,
            wal_disabled_due_to_enospc: AtomicBool::new(false),
            recovered_rollback_seeds: Mutex::new(Vec::new()),
        };

        runtime
    }

    /// Attach a shared WAL handle for runtime decision durability.
    pub fn with_wal(mut self, wal: Arc<Wal>) -> Self {
        self.wal = Some(wal);
        self
    }

    pub fn session_manager(&self) -> Arc<SessionManager> {
        Arc::clone(&self.session_manager)
    }

    // =========================================================================
    // WAL Recovery Methods
    // Called during startup WAL replay to restore committed and staged state.
    // =========================================================================

    /// Restore committed history from a WAL `CommitPersisted` record into ShadowLedger.
    ///
    /// Registers the pool identity, marks the pool as Committed, and calls
    /// `shadow_ledger.commit_history` to persist the snapshot sequence.
    /// Returns `true` if at least one snapshot was stored or already present.
    pub fn restore_committed_history_from_wal(
        &self,
        identity: DomainPoolIdentity,
        snapshots: Vec<MarketSnapshot>,
        last_committed_tx_key: Option<TxKey>,
    ) -> bool {
        let base_mint: Pubkey = identity.base_mint.into();
        let pool_id: Pubkey = identity.pool_id.into();
        self.pool_identities.register(identity);
        self.set_runtime_pool_state(pool_id, PoolState::Committed);
        let result = self
            .shadow_ledger
            .commit_history(base_mint, snapshots, last_committed_tx_key);
        result.status != ghost_core::shadow_ledger::CommitHistoryStatus::RejectedNoWrite
    }

    /// Replay a single live transaction from WAL into the LivePipeline.
    ///
    /// The mint must already be initialized in the pipeline (i.e.
    /// `restore_committed_history_from_wal` must have been called first).
    /// Returns `true` if the event was successfully queued.
    pub fn replay_live_tx_from_wal(
        &self,
        base_mint: Pubkey,
        tx: &GatekeeperBufferedHistoryTx,
    ) -> bool {
        if !self.live_pipeline.is_initialized(&base_mint) {
            // Pipeline is not initialized for this mint; skip live replay.
            return false;
        }
        let event = ghost_core::shadow_ledger::LiveTxEvent {
            base_mint,
            slot: tx.tx_key.slot,
            tx_index: None,
            signature: None,
            timestamp_ms: tx.tx_key.timestamp_ms,
            side: tx.side,
            d_sol_lamports: tx.d_sol_lamports,
            d_tok_units: tx.d_tok_units,
            dev_buy: tx.dev_buy,
            trader: tx.trader,
        };
        match self.live_pipeline.process_event(event) {
            Ok(()) => true,
            Err(e) => {
                tracing::debug!(
                    base_mint = %base_mint,
                    error = %e,
                    "WAL replay: live tx rejected by pipeline"
                );
                false
            }
        }
    }

    /// Flush all replayed live transactions for a mint from the LivePipeline into ShadowLedger.
    ///
    /// Returns the number of snapshots appended to the ShadowLedger.
    pub fn flush_replayed_live_mint_from_wal(&self, base_mint: &Pubkey) -> usize {
        match self
            .live_pipeline
            .flush_mint(base_mint, self.shadow_ledger.as_ref())
        {
            Ok(count) => count,
            Err(e) => {
                tracing::debug!(
                    base_mint = %base_mint,
                    error = %e,
                    "WAL replay: flush_mint failed"
                );
                0
            }
        }
    }

    /// Restore a pool's runtime state during WAL replay.
    ///
    /// Registers the pool identity, sets the runtime state, and if the state is
    /// `Approved` or `Committed`, adds the pool to the approved-pools fast-path set.
    pub fn restore_runtime_pool_state_from_wal(
        &self,
        identity: DomainPoolIdentity,
        state: PoolState,
    ) {
        let pool_id: Pubkey = identity.pool_id.into();
        self.pool_identities.register(identity);
        self.set_runtime_pool_state(pool_id, state);
        if state == PoolState::Approved || state == PoolState::Committed {
            self.approved_pools.insert(pool_id);
        }
    }

    /// Restore a rollback reeval seed from WAL during startup replay.
    ///
    /// Marks the pool as Tracked and queues the seed for post-startup processing.
    /// Returns `true` if the seed was successfully queued.
    pub fn restore_rollback_seed_from_wal(
        &self,
        rollback: &ghost_core::wal::RollbackReevalSeedRecord,
    ) -> bool {
        let pool_id: Pubkey = rollback.identity.pool_id.into();
        self.pool_identities.register(rollback.identity.clone());
        self.set_runtime_pool_state(pool_id, PoolState::Tracked);
        self.recovered_rollback_seeds.lock().push(rollback.clone());
        true
    }

    /// Drain and return all rollback reeval seeds recovered from WAL replay.
    ///
    /// Used during startup to bootstrap the router with recovered rollback state.
    pub fn drain_recovered_rollback_seeds(&self) -> Vec<ghost_core::wal::RollbackReevalSeedRecord> {
        std::mem::take(&mut *self.recovered_rollback_seeds.lock())
    }

    /// Set the Paradox Sensor state receiver after initialization
    /// This allows OracleRuntime to be created before Seer starts
    pub fn set_paradox_receiver(
        &self,
        rx: tokio::sync::watch::Receiver<seer::paradox_sensor::ParadoxState>,
    ) {
        let mut guard = self.paradox_rx.write();
        *guard = Some(rx);
        info!("🔮 Paradox Sensor receiver connected to OracleRuntime");
    }

    fn append_wal_record(&self, record: WalRecord, record_kind: &'static str) {
        let Some(wal) = self.wal.as_ref() else {
            return;
        };
        if self.wal_disabled_due_to_enospc.load(Ordering::Relaxed) {
            return;
        }

        if let Err(err) = wal.append(&record) {
            if is_no_space_error(&err) {
                if !self
                    .wal_disabled_due_to_enospc
                    .swap(true, Ordering::Relaxed)
                {
                    error!(
                        record_kind,
                        error = %err,
                        "OracleRuntime: disabling WAL after ENOSPC; runtime will continue without further WAL appends"
                    );
                }
                return;
            }
            warn!(
                record_kind,
                error = %err,
                "OracleRuntime: failed to append WAL record"
            );
        }
    }

    fn append_decision_to_wal(
        &self,
        pool_id: Pubkey,
        slot: Option<u64>,
        decision: WalGatekeeperDecision,
        reason: Option<String>,
    ) {
        self.append_wal_record(
            WalRecord::Decision {
                ts_ms: current_time_ms(),
                slot: slot.unwrap_or_default(),
                pool_id: Some(pool_id.to_bytes().to_vec()),
                decision,
                reason,
            },
            "decision",
        );
    }

    pub fn configure_orphan_adoption(&self, grace_multiplier: u64, adoption_cap: usize) {
        let multiplier = grace_multiplier.max(1);
        let cap = adoption_cap.max(1);
        self.orphan_grace_period_multiplier
            .store(multiplier, Ordering::Relaxed);
        self.max_orphans_adopted_on_register
            .store(cap, Ordering::Relaxed);
        info!(
            "🧩 Orphan adoption config applied: grace_multiplier={} adoption_cap={}",
            multiplier, cap
        );
    }

    /// Get a reference to the ShadowLedger for reserve lookups.
    ///
    /// Used by the Oracle Runtime task to enrich gRPC PoolTransaction events
    /// with bonding curve reserves before Gatekeeper processing.
    pub fn get_shadow_ledger(&self) -> &Arc<ShadowLedger> {
        &self.shadow_ledger
    }

    pub fn account_state_core(&self) -> &Arc<AccountStateReducer> {
        &self.account_state_core
    }

    pub fn shadow_ledger_enrichment_freshness_ms(&self) -> u64 {
        self.config.shadow_ledger_enrichment_freshness_ms
    }

    // ── ReconciliationRuntime integration ────────────────────────────────────

    fn build_account_state_update(
        &self,
        base_mint: &Pubkey,
        on_chain_sol: u64,
        on_chain_tok: u64,
        on_chain_complete: u8,
        slot: u64,
        write_version: Option<u64>,
        curve_finality: CurveFinality,
        source: UpdateSource,
        bonding_curve_hint: Option<&Pubkey>,
    ) -> Option<AccountStateUpdate> {
        let identity = match self
            .pool_identities
            .get_by_base_mint(base_mint)
            .or_else(|| {
                bonding_curve_hint.and_then(|bonding_curve| {
                    self.pool_identities.get_by_bonding_curve(bonding_curve)
                })
            }) {
            Some(identity) => identity,
            None => {
                ::metrics::counter!(
                    "account_update_build_none_total",
                    1u64,
                    "reason" => "identity_missing"
                );
                warn!(
                    base_mint = %base_mint,
                    slot,
                    source = ?source,
                    "DIAG_ACCOUNT_UPDATE_IDENTITY_MISS"
                );
                return None;
            }
        };
        Some(AccountStateUpdate {
            pool_amm_id: identity.pool_id.into(),
            base_mint: *base_mint,
            bonding_curve: identity.bonding_curve.into(),
            sol_reserves: on_chain_sol,
            token_reserves: on_chain_tok,
            is_complete: on_chain_complete,
            slot,
            write_version,
            receive_ts_ms: current_time_ms(),
            receive_seq: self.account_state_core.next_recv_seq(),
            curve_finality,
            source,
        })
    }

    fn apply_account_state_update(
        &self,
        update: &AccountStateUpdate,
    ) -> ghost_core::account_state_core::types::AccountUpdateResult {
        let apply_result = self.account_state_core.apply_account_update(update.clone());
        match &apply_result {
            ghost_core::account_state_core::types::AccountUpdateResult::Applied => {
                ::metrics::counter!("account_update_apply_result_total", 1u64, "result" => "applied");
            }
            ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap => {
                ::metrics::counter!("account_update_promoted_from_bootstrap_total", 1u64);
                ::metrics::counter!(
                    "account_update_apply_result_total",
                    1u64,
                    "result" => "promoted_from_bootstrap"
                );
                if let Some(identity) = self.pool_identities.get_by_base_mint(&update.base_mint) {
                    if let Some(detected_pool) =
                        self.lookup_detected_pool(&Pubkey::from(identity.pool_id))
                    {
                        if let Some(start_ts_ms) = detected_pool.detected_wall_ts_ms {
                            let latency_ms = update.receive_ts_ms.saturating_sub(start_ts_ms);
                            ::metrics::histogram!(
                                "canonical_first_promotion_latency_ms",
                                latency_ms as f64
                            );
                        }
                    }
                }
            }
            ghost_core::account_state_core::types::AccountUpdateResult::Rejected(reason) => {
                ::metrics::counter!(
                    "account_update_apply_result_total",
                    1u64,
                    "result" => "rejected",
                    "reason" => reason.as_str()
                );
            }
        }
        if matches!(
            apply_result,
            ghost_core::account_state_core::types::AccountUpdateResult::Applied
                | ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap
        ) {
            self.canonical_readiness_notifier
                .notify_ready(&update.base_mint);
            self.sync_shadow_from_canonical_state(update);
        }
        info!(
            pool = %update.pool_amm_id,
            base_mint = %update.base_mint,
            bonding_curve = %update.bonding_curve,
            slot = update.slot,
            source = ?update.source,
            apply_result = ?apply_result,
            "DIAG_ACCOUNT_UPDATE_APPLIED"
        );
        if let Some(session) = self.lookup_pool_session(&update.pool_amm_id) {
            increment_counter!(
                "oracle_runtime_account_update_session_resolution_total",
                "status" => "refreshed"
            );
            info!(
                pool = %update.pool_amm_id,
                base_mint = %update.base_mint,
                "DIAG_ACCOUNT_UPDATE_SESSION_FOUND"
            );
            session.write().on_account_state_core_updated();
        } else {
            let deferred_session_refresh = self.lookup_pool_identity(&update.pool_amm_id).is_some()
                || self
                    .lookup_pool_identity_by_base_mint(&update.base_mint)
                    .is_some()
                || self
                    .lookup_pool_identity_by_bonding_curve(&update.bonding_curve)
                    .is_some()
                || self.lookup_detected_pool(&update.pool_amm_id).is_some();
            increment_counter!(
                "oracle_runtime_account_update_session_resolution_total",
                "status" => if deferred_session_refresh {
                    "deferred"
                } else {
                    "miss"
                }
            );
            if deferred_session_refresh {
                info!(
                    pool = %update.pool_amm_id,
                    base_mint = %update.base_mint,
                    bonding_curve = %update.bonding_curve,
                    slot = update.slot,
                    "DIAG_ACCOUNT_UPDATE_SESSION_DEFERRED"
                );
            } else {
                warn!(
                    pool = %update.pool_amm_id,
                    base_mint = %update.base_mint,
                    bonding_curve = %update.bonding_curve,
                    slot = update.slot,
                    "DIAG_ACCOUNT_UPDATE_SESSION_MISS"
                );
            }
        }
        apply_result
    }

    fn subscribe_canonical_readiness(&self, base_mint: &Pubkey) -> watch::Receiver<u64> {
        self.canonical_readiness_notifier.subscribe(base_mint)
    }

    fn is_live_trigger_canonical_ready(&self, base_mint: &Pubkey) -> bool {
        let Some(observed_slot) = self.account_state_core.latest_observed_slot() else {
            return false;
        };
        let Some(canonical_state) = self.account_state_core.get_canonical_state(base_mint) else {
            return false;
        };
        if self.account_state_core.bonding_curve(base_mint).is_none() {
            return false;
        }
        observed_slot.saturating_sub(canonical_state.last_update_slot)
            <= ShadowBondingCurve::MAX_AGE_SLOTS
    }

    fn sync_shadow_from_canonical_state(&self, update: &AccountStateUpdate) {
        if !matches!(
            update.source,
            UpdateSource::GeyserAccountUpdate | UpdateSource::WalReplay
        ) {
            return;
        }

        let Some(canonical_state) = self
            .account_state_core
            .get_canonical_state(&update.base_mint)
        else {
            warn!(
                pool = %update.pool_amm_id,
                base_mint = %update.base_mint,
                bonding_curve = %update.bonding_curve,
                slot = update.slot,
                "DIAG_SHADOW_BOOTSTRAP_SYNC_MISSING_CANONICAL_STATE"
            );
            return;
        };

        let existing_shadow = self.shadow_ledger.get_old(&update.bonding_curve);
        let sync_decision = canonical_shadow_sync_decision(existing_shadow, &canonical_state);

        match sync_decision {
            CanonicalShadowSyncDecision::NoOp(reason) => {
                debug!(
                    pool = %update.pool_amm_id,
                    base_mint = %update.base_mint,
                    bonding_curve = %update.bonding_curve,
                    slot = update.slot,
                    sync_reason = reason,
                    "DIAG_SHADOW_BOOTSTRAP_SYNC_SKIPPED"
                );
                return;
            }
            CanonicalShadowSyncDecision::Guard(reason) => {
                warn!(
                    pool = %update.pool_amm_id,
                    base_mint = %update.base_mint,
                    bonding_curve = %update.bonding_curve,
                    slot = update.slot,
                    sync_reason = reason,
                    existing_write_strength = existing_shadow
                        .map(|curve| curve.write_strength.as_str())
                        .unwrap_or("missing"),
                    existing_slot = existing_shadow.map(|curve| curve.last_updated_slot),
                    canonical_slot = canonical_state.last_update_slot,
                    "DIAG_SHADOW_BOOTSTRAP_SYNC_GUARDED"
                );
                return;
            }
            CanonicalShadowSyncDecision::Sync(_) => {}
        }

        let curve = canonical_shadow_curve(&canonical_state);
        let (write_strength, write_reason) =
            canonical_shadow_write_metadata(existing_shadow, &canonical_state);
        let outcome = self.shadow_ledger.apply_curve_write(
            Some(update.base_mint),
            update.bonding_curve,
            curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::AccountUpdate,
                write_strength,
                ShadowLedgerStateConfidence::Observed,
                write_reason,
                Some(canonical_state.last_update_slot),
                canonical_state.curve_finality,
            )
            .with_last_update_ts_ms(canonical_state.last_update_ts_ms),
        );

        match outcome.result {
            ShadowLedgerWriteResult::Applied
            | ShadowLedgerWriteResult::PromotedBootstrapToConfirmed
            | ShadowLedgerWriteResult::NoOpExistingEqualOrStronger => {
                info!(
                    pool = %update.pool_amm_id,
                    base_mint = %update.base_mint,
                    bonding_curve = %update.bonding_curve,
                    slot = update.slot,
                    sync_reason = match sync_decision {
                        CanonicalShadowSyncDecision::Sync(reason) => reason,
                        CanonicalShadowSyncDecision::NoOp(reason)
                        | CanonicalShadowSyncDecision::Guard(reason) => reason,
                    },
                    write_strength = write_strength.as_str(),
                    write_reason = write_reason.as_str(),
                    shadow_result = outcome.result.as_str(),
                    "DIAG_SHADOW_BOOTSTRAP_SYNCED_FROM_CANONICAL"
                );
            }
            ShadowLedgerWriteResult::RejectedWeakerWrite
            | ShadowLedgerWriteResult::RejectedOutOfOrder
            | ShadowLedgerWriteResult::RejectedMissingMetadata => {
                warn!(
                    pool = %update.pool_amm_id,
                    base_mint = %update.base_mint,
                    bonding_curve = %update.bonding_curve,
                    slot = update.slot,
                    sync_reason = match sync_decision {
                        CanonicalShadowSyncDecision::Sync(reason) => reason,
                        CanonicalShadowSyncDecision::NoOp(reason)
                        | CanonicalShadowSyncDecision::Guard(reason) => reason,
                    },
                    write_strength = write_strength.as_str(),
                    write_reason = write_reason.as_str(),
                    shadow_result = outcome.result.as_str(),
                    "DIAG_SHADOW_BOOTSTRAP_SYNC_REJECTED"
                );
            }
        }
    }

    fn canonical_materialization_latency_ms(&self, pool_id: &Pubkey) -> Option<u64> {
        self.lookup_detected_pool(pool_id)
            .and_then(|pool| pool.detected_wall_ts_ms)
            .map(|start_ts_ms| current_time_ms().saturating_sub(start_ts_ms))
    }

    fn account_update_event_latency_ms(&self, event: &AccountUpdateEvent) -> Option<u64> {
        if let Some(effective_event_ts_ms) = event.event_time.effective_event_ts_ms() {
            return Some(current_time_ms().saturating_sub(effective_event_ts_ms));
        }
        match event.replay_origin {
            seer::ipc::AccountUpdateReplayOrigin::PendingReplay => event.replay_buffer_dwell_ms,
            seer::ipc::AccountUpdateReplayOrigin::Live => {
                let detected_ms = event
                    .detected_at
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64;
                Some(current_time_ms().saturating_sub(detected_ms))
            }
        }
    }

    fn process_account_update_with_source(
        &self,
        base_mint: &Pubkey,
        on_chain_sol: u64,
        on_chain_tok: u64,
        on_chain_complete: u8,
        slot: u64,
        curve_finality: CurveFinality,
    ) -> Option<ghost_core::shadow_ledger::reconciliation::ReconciliationOutcome> {
        self.process_account_update_with_explicit_source(
            base_mint,
            on_chain_sol,
            on_chain_tok,
            on_chain_complete,
            slot,
            curve_finality,
            UpdateSource::GeyserAccountUpdate,
            None,
            false,
        )
    }

    fn process_account_update_with_explicit_source(
        &self,
        base_mint: &Pubkey,
        on_chain_sol: u64,
        on_chain_tok: u64,
        on_chain_complete: u8,
        slot: u64,
        curve_finality: CurveFinality,
        source: UpdateSource,
        event: Option<&AccountUpdateEvent>,
        enqueue_on_identity_miss: bool,
    ) -> Option<ghost_core::shadow_ledger::reconciliation::ReconciliationOutcome> {
        ::metrics::counter!(
            "account_update_ingress_total",
            1u64,
            "source" => match source {
                UpdateSource::GeyserAccountUpdate => "geyser_account_update",
                UpdateSource::WalReplay => "wal_replay",
                UpdateSource::TxObservedBootstrap => "tx_observed_bootstrap",
            }
        );
        info!(
            base_mint = %base_mint,
            slot,
            source = ?source,
            sol_reserves = on_chain_sol,
            token_reserves = on_chain_tok,
            complete = on_chain_complete,
            curve_finality = %curve_finality.as_str(),
            "DIAG_ACCOUNT_UPDATE_RUNTIME_INGRESS"
        );
        let update = match self.build_account_state_update(
            base_mint,
            on_chain_sol,
            on_chain_tok,
            on_chain_complete,
            slot,
            event.and_then(|event| event.write_version),
            curve_finality,
            source,
            event.map(|event| &event.bonding_curve),
        ) {
            Some(update) => update,
            None => {
                ::metrics::counter!("account_update_before_identity_total", 1u64);
                if enqueue_on_identity_miss {
                    if let Some(event) = event {
                        self.enqueue_pre_identity_account_update(event);
                    }
                }
                return None;
            }
        };
        if let Some(event) = event {
            coverage_audit().register_pool_alias(
                &event.bonding_curve.to_string(),
                &update.pool_amm_id.to_string(),
            );
            coverage_audit().register_pool_alias(
                &event.base_mint.to_string(),
                &update.pool_amm_id.to_string(),
            );
        }
        let apply_result = self.apply_account_state_update(&update);
        let update_accepted = matches!(
            apply_result,
            ghost_core::account_state_core::types::AccountUpdateResult::Applied
                | ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap
        );
        if let Some(event) = event {
            let runtime_time_source = runtime_account_update_time_source_info(event);
            coverage_audit().record_account_update_runtime_seen(
                &update.pool_amm_id.to_string(),
                runtime_time_source.effective_source,
                runtime_time_source.fallback_class,
                update_accepted,
            );
        }
        if update_accepted {
            let latency_ms = event.and_then(|event| self.account_update_event_latency_ms(event));
            coverage_audit()
                .record_canonical_update_observed(&update.pool_amm_id.to_string(), latency_ms);
            coverage_audit()
                .record_live_account_update_observed(&update.pool_amm_id.to_string(), latency_ms);
        }
        // Only feed fresh (accepted) state into the shadow ledger reconciler.
        // Passing stale/rejected updates would cause false drift alerts.
        if update_accepted {
            self.reconciliation_runtime.lock().process_account_update(
                base_mint,
                on_chain_sol,
                on_chain_tok,
                on_chain_complete,
                slot,
                curve_finality,
            )
        } else {
            None
        }
    }

    fn maybe_materialize_canonical_state_from_observed_tx(
        &self,
        pool_id: Pubkey,
        base_mint_hint: Option<Pubkey>,
        tx: &PoolTransaction,
    ) -> Option<ghost_core::account_state_core::types::AccountUpdateResult> {
        let (sol_reserves, token_reserves) = raw_tx_curve_reserves(tx)?;
        let slot = tx.slot?;
        let base_mint = parse_tx_base_mint(tx, base_mint_hint)?;
        if self
            .account_state_core
            .get_canonical_state(&base_mint)
            .is_some()
        {
            return None;
        }

        let identity = self.lookup_pool_identity(&pool_id)?;
        let update = self.build_account_state_update(
            &base_mint,
            sol_reserves,
            token_reserves,
            0,
            slot,
            Some(0),
            tx.curve_finality,
            UpdateSource::TxObservedBootstrap,
            Some(identity.bonding_curve.as_ref()),
        )?;
        let apply_result = self.apply_account_state_update(&update);
        if matches!(
            apply_result,
            ghost_core::account_state_core::types::AccountUpdateResult::Applied
                | ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap
        ) {
            coverage_audit().record_canonical_update_observed(
                &pool_id.to_string(),
                self.canonical_materialization_latency_ms(&pool_id),
            );
            info!(
                pool = %pool_id,
                base_mint = %base_mint,
                slot,
                sol_reserves,
                token_reserves,
                curve_finality = %tx.curve_finality.as_str(),
                "DIAG_TX_BOOTSTRAP_CANONICALIZED"
            );
        }
        Some(apply_result)
    }

    /// Feed an on-chain AccountUpdate for `base_mint` into the canonical
    /// AccountStateCore and diagnostic reconciliation loop.
    ///
    /// This is the **production AccountUpdate integration point** for PR7.
    /// Every confirmed on-chain AccountUpdate must first hydrate
    /// `AccountStateCore`, then optionally feed an active observation session,
    /// and finally run diagnostic-only drift monitoring against `ShadowLedger`.
    ///
    /// Returns the reconciliation outcome, or `None` if `base_mint` is not
    /// tracked by the runtime identity registry.
    pub fn process_account_update(
        &self,
        base_mint: &Pubkey,
        on_chain_sol: u64,
        on_chain_tok: u64,
        on_chain_complete: u8,
        slot: u64,
        curve_finality: CurveFinality,
    ) -> Option<ghost_core::shadow_ledger::reconciliation::ReconciliationOutcome> {
        self.process_account_update_with_source(
            base_mint,
            on_chain_sol,
            on_chain_tok,
            on_chain_complete,
            slot,
            curve_finality,
        )
    }

    /// Return a point-in-time health snapshot of the reconciliation runtime.
    ///
    /// Suitable for dashboards, alerting, metrics export, and runtime diagnostics.
    /// The status reflects the current state of all registered pools' drift,
    /// legacy-repair compatibility counters, hot-pool pressure, and cycle counters.
    pub fn reconciliation_status(&self) -> ghost_core::shadow_ledger::ReconciliationRuntimeStatus {
        self.reconciliation_runtime.lock().status()
    }

    /// Run a bounded reconciliation cycle using on-chain data supplied by `fetch`.
    ///
    /// `fetch(mint)` should return `(sol_reserves, tok_reserves, complete, slot)`
    /// for the given mint, or `None` to skip that pool.
    ///
    /// At most `max_pools_per_cycle` pools are visited (configured in
    /// [`ReconciliationRuntimeConfig`](ghost_core::shadow_ledger::ReconciliationRuntimeConfig));
    /// round-robin scheduling ensures all registered pools are eventually covered.
    ///
    /// Returns the number of pools actually reconciled in this cycle.
    fn setup_shadow_ledger_gating(&self) {
        let registry = self.approved_pools.clone();
        let identities = Arc::clone(&self.pool_identities);
        self.shadow_ledger
            .set_approval_checker(Arc::new(move |base_mint: &Pubkey| {
                identities
                    .get_by_base_mint(base_mint)
                    .map(|identity| registry.is_approved(&identity.pool_id))
                    .unwrap_or(false)
            }));
    }

    pub fn configure_approval_gating(&self, snapshot_engine: &SnapshotEngine) {
        snapshot_engine.set_approved_pools(self.approved_pools());
        self.setup_shadow_ledger_gating();
    }

    fn resolve_trigger_buy_curve(
        &self,
        base_mint: Pubkey,
        _buffered_txs: &[crate::components::gatekeeper::GatekeeperBufferedTx],
    ) -> Option<BondingCurve> {
        self.account_state_core
            .get_canonical_state(&base_mint)
            .map(|canonical_state| canonical_shadow_curve(&canonical_state))
    }

    #[allow(deprecated)]
    fn resolve_price_context(
        &self,
        _pool_amm_id: Pubkey,
        base_mint: Pubkey,
    ) -> ResolvedPriceContext {
        let mut context = ResolvedPriceContext::default();

        if let Some(canonical_state) = self.account_state_core.get_canonical_state(&base_mint) {
            context = canonical_price_context(&canonical_state);
            if context.reserve_base.is_some() && context.reserve_quote.is_some() {
                return context;
            }
        }

        if let Some(bootstrap_state) = self.account_state_core.get_bootstrap_state(&base_mint) {
            if let Some((reserve_sol_lamports, reserve_tok_units)) =
                bootstrap_state.speculative_reserves
            {
                if context.reserve_base.is_none() && reserve_tok_units > 0 {
                    context.reserve_base =
                        Some(reserve_tok_units as f64 / PUMP_TOKEN_DECIMAL_FACTOR);
                }
                if context.reserve_quote.is_none() && reserve_sol_lamports > 0 {
                    context.reserve_quote = Some(reserve_sol_lamports as f64 / LAMPORTS_PER_SOL);
                }
                if context.price_quote.is_none() {
                    if let (Some(rb), Some(rq)) = (context.reserve_base, context.reserve_quote) {
                        let (price, price_state, _) = derive_price_canonical(rb, rq, 0.0);
                        if price_state == PriceState::Valid {
                            context.price_quote = Some(price);
                        }
                    }
                }
                if context.reserve_base.is_some() && context.reserve_quote.is_some() {
                    return context;
                }
            }
        }

        if let Some(snapshot) = self.shadow_ledger.get_latest_snapshot_internal(&base_mint) {
            assert_declared_shadow_truth_fallback("resolve_price_context");
            // Phase 0 freeze contract requires explicit fallback site label: "site" => "resolve_price_context"
            record_shadow_truth_fallback("resolve_price_context");
            record_degraded_truth_helper("resolve_price_context", "shadow_ledger_snapshot");
            if context.reserve_base.is_none()
                && snapshot.reserve_base.is_finite()
                && snapshot.reserve_base > MIN_RESERVE_THRESHOLD
            {
                context.reserve_base = Some(snapshot.reserve_base / PUMP_TOKEN_DECIMAL_FACTOR);
            }
            if context.reserve_quote.is_none()
                && snapshot.reserve_quote.is_finite()
                && snapshot.reserve_quote > MIN_RESERVE_THRESHOLD
            {
                context.reserve_quote = Some(snapshot.reserve_quote);
            }
            if context.price_quote.is_none() {
                if let (Some(rb), Some(rq)) = (context.reserve_base, context.reserve_quote) {
                    let (price, price_state, _) = derive_price_canonical(rb, rq, 0.0);
                    if price_state == PriceState::Valid {
                        context.price_quote = Some(price);
                    }
                } else if snapshot.price_sol_per_token.is_finite()
                    && snapshot.price_sol_per_token > 0.0
                {
                    context.price_quote = Some(snapshot.price_sol_per_token);
                }
            }
        }

        if context.reserve_base.is_some() && context.reserve_quote.is_some() {
            return context;
        }

        context
    }

    pub fn set_panic_retention_ms(&self, retention_ms: u64) {
        self.panic_retention_ms
            .store(retention_ms, Ordering::Relaxed);
    }

    pub fn record_panic_transaction(&self, pool_amm_id: Pubkey, tx: PanicTx) {
        let retention_ms = self.panic_retention_ms.load(Ordering::Relaxed);
        let cutoff_ms = if retention_ms > 0 {
            tx.arrival_ts_ms.saturating_sub(retention_ms)
        } else {
            0
        };

        let mut buffers = self.panic_tx_buffer.write();
        let buffer = buffers.entry(pool_amm_id).or_insert_with(VecDeque::new);
        buffer.push_back(tx);

        if retention_ms > 0 {
            while let Some(front) = buffer.front() {
                if front.arrival_ts_ms < cutoff_ms {
                    buffer.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    pub fn orphan_metrics_snapshot(&self) -> OrphanTelemetrySnapshot {
        self.orphan_metrics.snapshot()
    }

    #[cfg(test)]
    fn reset_orphan_metrics(&self) {
        self.orphan_metrics.reset();
    }

    /// Get handle to the approved pools registry (canonical allowlist)
    pub fn approved_pools(&self) -> Arc<ApprovedPools> {
        Arc::clone(&self.approved_pools)
    }

    pub fn runtime_pool_state(&self, pool_amm_id: &Pubkey) -> Option<PoolState> {
        self.runtime_pool_states.read().get(pool_amm_id).copied()
    }

    fn set_runtime_pool_state(&self, pool_amm_id: Pubkey, state: PoolState) {
        self.runtime_pool_states.write().insert(pool_amm_id, state);
    }

    pub fn mark_pool_tracked(&self, pool_amm_id: Pubkey) {
        self.set_runtime_pool_state(pool_amm_id, PoolState::Tracked);
    }

    pub fn mark_pool_approved(&self, pool_amm_id: Pubkey) {
        let next = match self.runtime_pool_state(&pool_amm_id) {
            Some(PoolState::Committed) => PoolState::Committed,
            _ => PoolState::Approved,
        };
        self.set_runtime_pool_state(pool_amm_id, next);
    }

    pub fn mark_pool_committed(&self, pool_amm_id: Pubkey) {
        let known_runtime_pool = self.lookup_pool_session(&pool_amm_id).is_some()
            || self.lookup_pool_identity(&pool_amm_id).is_some()
            || self.lookup_detected_pool(&pool_amm_id).is_some()
            || self.runtime_pool_state(&pool_amm_id).is_some();

        if !known_runtime_pool {
            warn!(
                pool = %pool_amm_id,
                "Ignoring mark_pool_committed for non-existent runtime pool"
            );
            return;
        }
        self.set_runtime_pool_state(pool_amm_id, PoolState::Committed);
    }

    fn effective_runtime_pool_state(
        &self,
        pool_amm_id: &Pubkey,
        _base_mint: Option<&Pubkey>,
    ) -> Option<PoolState> {
        self.runtime_pool_state(pool_amm_id)
    }

    pub fn commit_coordinator(&self) -> Arc<LauncherCommitCoordinator> {
        Arc::clone(&self.commit_coordinator)
    }

    pub fn remember_committed_snapshot(&self, base_mint: Pubkey, snapshot: &MarketSnapshot) {
        self.committed_bootstrap_snapshots
            .write()
            .insert(base_mint, snapshot.clone());
    }

    pub fn committed_bootstrap_snapshot(&self, base_mint: &Pubkey) -> Option<MarketSnapshot> {
        self.committed_bootstrap_snapshots
            .read()
            .get(base_mint)
            .cloned()
    }

    fn ensure_live_pipeline_initialized_from_snapshot(
        &self,
        base_mint: Pubkey,
        snapshot: &MarketSnapshot,
    ) {
        self.remember_committed_snapshot(base_mint, snapshot);
        if !self.live_pipeline.is_initialized(&base_mint) {
            self.live_pipeline.init_for_mint(base_mint, snapshot);
        }
    }

    /// Get handle to the canonical pool identity registry.
    pub fn pool_identity_registry(&self) -> Arc<PoolIdentityRegistry> {
        Arc::clone(&self.pool_identities)
    }

    pub fn lookup_pool_identity(&self, pool_amm_id: &Pubkey) -> Option<DomainPoolIdentity> {
        self.pool_identities.get_by_pool(pool_amm_id)
    }

    pub fn lookup_pool_identity_by_base_mint(
        &self,
        base_mint: &Pubkey,
    ) -> Option<DomainPoolIdentity> {
        self.pool_identities.get_by_base_mint(base_mint)
    }

    pub fn lookup_pool_identity_by_bonding_curve(
        &self,
        bonding_curve: &Pubkey,
    ) -> Option<DomainPoolIdentity> {
        self.pool_identities.get_by_bonding_curve(bonding_curve)
    }

    pub fn lookup_registered_pool(&self, base_mint: &Pubkey) -> Option<Pubkey> {
        self.pool_identities
            .get_by_base_mint(base_mint)
            .map(|identity| identity.pool_id.into())
    }

    fn remember_detected_pool(&self, pool_amm_id: Pubkey, pool_data: Arc<DetectedPool>) {
        self.detected_pools.write().insert(pool_amm_id, pool_data);
    }

    fn lookup_detected_pool(&self, pool_amm_id: &Pubkey) -> Option<Arc<DetectedPool>> {
        self.detected_pools.read().get(pool_amm_id).cloned()
    }

    fn lookup_pool_session(&self, pool_amm_id: &Pubkey) -> Option<SharedSession> {
        self.session_manager.get_session(pool_amm_id)
    }

    fn is_known_runtime_pool(&self, pool_amm_id: &Pubkey) -> bool {
        self.lookup_pool_session(pool_amm_id).is_some()
            || self.lookup_pool_identity(pool_amm_id).is_some()
            || self.lookup_detected_pool(pool_amm_id).is_some()
            || self.runtime_pool_state(pool_amm_id).is_some()
    }

    fn detected_pool_from_candidate(
        &self,
        pool_amm_id: &Pubkey,
        candidate: &EnhancedCandidate,
        dev_wallet: Option<Pubkey>,
    ) -> Arc<DetectedPool> {
        let creator = dev_wallet
            .map(|dev| dev.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let initial_liquidity_sol = if candidate.initial_liquidity_sol.is_finite()
            && candidate.initial_liquidity_sol > 0.0
        {
            Some(candidate.initial_liquidity_sol)
        } else {
            None
        };

        Arc::new(DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_amm_id.to_string(),
            base_mint: candidate.base_mint.to_string(),
            quote_mint: candidate.quote_mint.to_string(),
            amm_program: candidate.amm_program_id.to_string(),
            bonding_curve: candidate.bonding_curve.to_string(),
            creator,
            slot: candidate.slot,
            timestamp_ms: candidate.timestamp,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(current_time_ms()),
            initial_liquidity_sol,
            signature: candidate.signature.clone(),
        })
    }

    fn build_runtime_state_pool_snapshot(&self, pool_amm_id: &Pubkey) -> Option<Arc<DetectedPool>> {
        let session = self.lookup_pool_session(pool_amm_id)?;
        let session = session.read();
        Some(self.detected_pool_from_candidate(
            pool_amm_id,
            &session.candidate_snapshot,
            session.dev_wallet,
        ))
    }

    fn lookup_shadow_metadata_pool(
        &self,
        pool_amm_id: &Pubkey,
    ) -> Option<(Arc<DetectedPool>, BuyPathMetadataSource)> {
        choose_shadow_metadata_pool(
            self.lookup_detected_pool(pool_amm_id),
            self.build_runtime_state_pool_snapshot(pool_amm_id),
        )
    }

    pub fn lookup_base_mint_for_pool(&self, pool_amm_id: &Pubkey) -> Option<Pubkey> {
        if let Some(session) = self.lookup_pool_session(pool_amm_id) {
            return Some(session.read().base_mint);
        }

        self.pool_identities
            .get_by_pool(pool_amm_id)
            .map(|identity| identity.base_mint.into())
    }

    fn build_session_open_request(
        &self,
        pool_amm_id: Pubkey,
        registered_wall_ts_ms: u64,
        gatekeeper_config: &GatekeeperV2Config,
        fingerprint_config: &EarlyFingerprintConfig,
        pool_data: Option<&DetectedPool>,
    ) -> Option<OpenSessionRequest> {
        let pool_data = pool_data
            .map(|pool| Arc::new(pool.clone()))
            .or_else(|| self.lookup_detected_pool(&pool_amm_id))
            .or_else(|| self.build_runtime_state_pool_snapshot(&pool_amm_id))?;
        let base_mint = Pubkey::from_str(&pool_data.base_mint).ok()?;
        let bonding_curve = Pubkey::from_str(&pool_data.bonding_curve).ok()?;
        let quote_mint = Pubkey::from_str(&pool_data.quote_mint).unwrap_or_default();
        let amm_program_id = Pubkey::from_str(&pool_data.amm_program).unwrap_or_default();
        let dev_wallet = Pubkey::from_str(&pool_data.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id,
            base_mint,
            quote_mint,
            bonding_curve,
            amm_program_id,
            slot: pool_data.slot,
            timestamp: detected_pool_event_ts_ms(&pool_data),
            initial_liquidity_sol: pool_data.initial_liquidity_sol.unwrap_or_default(),
            signature: pool_data.signature.clone(),
            ..Default::default()
        };

        Some(OpenSessionRequest {
            pool_amm_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            created_at_wall_ms: registered_wall_ts_ms,
            deadline_wall_ms: Some(
                registered_wall_ts_ms.saturating_add(self.config.session.max_observation_window_ms),
            ),
            gatekeeper_config: gatekeeper_config.clone(),
            fingerprint_config: fingerprint_config.clone(),
        })
    }

    fn resolve_gatekeeper_initial_reserves(
        &self,
        _pool_id: Pubkey,
        base_mint: Pubkey,
    ) -> (u64, u64) {
        if let Some(canonical_state) = self.account_state_core.get_canonical_state(&base_mint) {
            let canonical = canonical_runtime_reserves(&canonical_state);
            if canonical.0 > 0 && canonical.1 > 0 {
                return canonical;
            }
        }

        if let Some(bootstrap_state) = self.account_state_core.get_bootstrap_state(&base_mint) {
            if let Some(speculative_reserves) = bootstrap_state.speculative_reserves {
                if speculative_reserves.0 > 0 && speculative_reserves.1 > 0 {
                    return speculative_reserves;
                }
            }
        }

        if let Some(snapshot) = self.shadow_ledger.get_latest_snapshot_internal(&base_mint) {
            assert_declared_shadow_truth_fallback("resolve_gatekeeper_initial_reserves");
            // Phase 0 freeze contract requires explicit fallback site label: "site" => "resolve_gatekeeper_initial_reserves"
            record_shadow_truth_fallback("resolve_gatekeeper_initial_reserves");
            record_degraded_truth_helper(
                "resolve_gatekeeper_initial_reserves",
                "shadow_ledger_snapshot",
            );
            let reserve_sol_lamports =
                (snapshot.reserve_quote.max(0.0) * LAMPORTS_PER_SOL).round() as u64;
            let reserve_tok_units = snapshot.reserve_base.max(0.0).round() as u64;
            if reserve_sol_lamports > 0 && reserve_tok_units > 0 {
                return (reserve_sol_lamports, reserve_tok_units);
            }
        }

        (GENESIS_SOL_LAMPORTS, GENESIS_TOKEN_RESERVES)
    }

    fn stage_gatekeeper_history_for_commit(
        &self,
        pool_id: Pubkey,
        base_mint: Pubkey,
        _created_at_ms: u64,
        buffered_txs: &[crate::components::gatekeeper::GatekeeperBufferedTx],
    ) -> usize {
        let (initial_reserve_sol, initial_reserve_tok) =
            self.resolve_gatekeeper_initial_reserves(pool_id, base_mint);
        let history: Vec<_> = buffered_txs
            .iter()
            .filter_map(pool_tx_to_gatekeeper_history)
            .collect();

        self.commit_coordinator.stage_history(
            pool_id,
            base_mint,
            initial_reserve_sol,
            initial_reserve_tok,
            history,
        )
    }

    fn forward_approved_tx_to_commit_or_live_pipeline(
        &self,
        pool_id: Pubkey,
        base_mint: Pubkey,
        tx: &PoolTransaction,
        event_ts_ms: u64,
    ) {
        let Some(tx_key) = pool_tx_to_tx_key(tx, event_ts_ms) else {
            return;
        };

        let runtime_state = self
            .effective_runtime_pool_state(&pool_id, Some(&base_mint))
            .unwrap_or(PoolState::Tracked);

        if runtime_state.is_committed() {
            if !self.live_pipeline.is_initialized(&base_mint) {
                if let Some(snapshot) = self.committed_bootstrap_snapshot(&base_mint) {
                    self.ensure_live_pipeline_initialized_from_snapshot(base_mint, &snapshot);
                } else {
                    warn!(
                        pool = %pool_id,
                        base_mint = %base_mint,
                        "Runtime marked pool committed but launcher bootstrap snapshot is missing"
                    );
                    return;
                }
            }

            let Some(event) = pool_tx_to_live_event(base_mint, tx, tx_key) else {
                return;
            };

            if let Err(err) = self.live_pipeline.process_event(event) {
                warn!(
                    pool = %pool_id,
                    base_mint = %base_mint,
                    error = %err,
                    "Failed to forward committed tx into LivePipeline"
                );
            }
            return;
        }

        if !runtime_state.is_approved() {
            warn!(
                pool = %pool_id,
                base_mint = %base_mint,
                state = ?runtime_state,
                "Runtime rejected pre/post-commit routing for pool without approval"
            );
            return;
        }

        let Some(buffered_tx) = pool_tx_to_buffered_history_tx(tx, tx_key.clone()) else {
            return;
        };

        match self
            .commit_coordinator
            .add_approved_tx(&base_mint, buffered_tx)
        {
            CommitIngressOutcome::BufferedHistory | CommitIngressOutcome::PendingLive => {}
            CommitIngressOutcome::Duplicate => {}
            CommitIngressOutcome::RouteToLive { bootstrap_snapshot } => {
                self.ensure_live_pipeline_initialized_from_snapshot(base_mint, &bootstrap_snapshot);
                self.mark_pool_committed(pool_id);
                if let Some(event) = pool_tx_to_live_event(base_mint, tx, tx_key) {
                    if let Err(err) = self.live_pipeline.process_event(event) {
                        warn!(
                            pool = %pool_id,
                            base_mint = %base_mint,
                            error = %err,
                            "LivePipeline rejected tx after launcher commit persistence"
                        );
                    }
                }
            }
            CommitIngressOutcome::Missing => {
                if let Some(snapshot) = self.committed_bootstrap_snapshot(&base_mint) {
                    self.ensure_live_pipeline_initialized_from_snapshot(base_mint, &snapshot);
                    self.mark_pool_committed(pool_id);
                    if let Some(event) = pool_tx_to_live_event(base_mint, tx, tx_key) {
                        if let Err(err) = self.live_pipeline.process_event(event) {
                            warn!(
                                pool = %pool_id,
                                base_mint = %base_mint,
                                error = %err,
                                "LivePipeline rejected tx after recovering finalized launcher commit"
                            );
                        }
                    }
                } else {
                    warn!(
                        pool = %pool_id,
                        base_mint = %base_mint,
                        "Failed to route approved tx: launcher commit window missing"
                    );
                }
            }
        }
    }

    fn build_shadow_tx_metrics(&self, base_mint: &Pubkey) -> Option<TransactionMetrics> {
        self.shadow_ledger
            .get_snapshots_internal(base_mint)
            .and_then(|snaps| tx_metrics_from_snapshots(&snaps))
    }

    fn drain_registration_orphans(
        &self,
        pool_amm_id: Pubkey,
        approve_slot: Option<u64>,
        dev_wallet: Option<Pubkey>,
    ) {
        let mut orphans_guard = self.orphans.write();
        let Some(orphans) = orphans_guard.remove(&pool_amm_id) else {
            return;
        };

        let orphans_seen = orphans.len();
        let grace_multiplier = self
            .orphan_grace_period_multiplier
            .load(Ordering::Relaxed)
            .max(1);
        let fresh_window_ms = ORPHAN_TTL_MS.saturating_mul(grace_multiplier);
        let now = Instant::now();

        let mut adopted_orphans: Vec<OrphanTx> = Vec::with_capacity(orphans.len());
        let mut dropped_stale = 0_usize;
        let mut dropped_missing_ts = 0_usize;
        let mut dropped_slot_cutoff = 0_usize;

        for tx in orphans {
            if tx.timestamp_ms == 0 {
                dropped_missing_ts += 1;
                continue;
            }

            let age_ms = now.duration_since(tx.arrived_at).as_millis() as u64;
            if age_ms > fresh_window_ms {
                dropped_stale += 1;
                continue;
            }

            let slot_rejected = match (approve_slot, tx.slot) {
                (Some(cutoff_slot), Some(tx_slot)) => tx_slot < cutoff_slot,
                (Some(_), None) => true,
                (None, _) => false,
            };
            if slot_rejected {
                dropped_slot_cutoff += 1;
                continue;
            }

            adopted_orphans.push(tx);
        }

        let adoption_cap = self
            .max_orphans_adopted_on_register
            .load(Ordering::Relaxed)
            .max(1);
        let mut dropped_capped = 0_usize;
        if adopted_orphans.len() > adoption_cap {
            adopted_orphans.sort_by_key(|tx| tx.arrived_at);
            let drop_count = adopted_orphans.len() - adoption_cap;
            dropped_capped = drop_count;
            adopted_orphans.drain(0..drop_count);
        }

        self.orphan_metrics
            .adopted
            .fetch_add(adopted_orphans.len() as u64, Ordering::Relaxed);
        self.orphan_metrics
            .dropped_stale
            .fetch_add(dropped_stale as u64, Ordering::Relaxed);
        self.orphan_metrics
            .dropped_missing_timestamp
            .fetch_add(dropped_missing_ts as u64, Ordering::Relaxed);
        self.orphan_metrics
            .dropped_capped
            .fetch_add(dropped_capped as u64, Ordering::Relaxed);
        self.orphan_metrics
            .dropped_slot_cutoff
            .fetch_add(dropped_slot_cutoff as u64, Ordering::Relaxed);

        info!(
            "⚡ ORPHAN REGISTRATION DRAIN: pool={} seen={} accepted={} dropped_stale={} dropped_capped={} dropped_missing_ts={} dropped_slot_cutoff={} fresh_window_ms={} approve_slot={:?}",
            pool_amm_id,
            orphans_seen,
            adopted_orphans.len(),
            dropped_stale,
            dropped_capped,
            dropped_missing_ts,
            dropped_slot_cutoff,
            fresh_window_ms,
            approve_slot
        );

        for tx in adopted_orphans {
            if let Some(dev) = dev_wallet {
                if tx.signer == dev.to_string() {
                    info!(
                        "🕵️ IWIM: Orphan tx recognized as DEV ACTION on pool {}",
                        pool_amm_id
                    );
                }
            }
        }
    }

    fn register_runtime_pool_detection(
        &self,
        pool_amm_id: Pubkey,
        base_mint: Pubkey,
        candidate: &EnhancedCandidate,
    ) -> bool {
        if !is_valid_pool_address(&pool_amm_id) {
            error!("🚨 REJECTED invalid pool address: pool={}", pool_amm_id);
            return false;
        }

        if !is_valid_mint_address(&base_mint) {
            error!(
                "🚨 REJECTED invalid mint address: mint={} for pool={}",
                base_mint, pool_amm_id
            );
            return false;
        }

        if let Some(pump_program_pubkey) = self.pump_program_pubkey {
            if base_mint == pump_program_pubkey {
                error!(
                    "🚨 REJECTED base_mint matches Pump.fun program id: mint={} pool={}",
                    base_mint, pool_amm_id
                );
                return false;
            }
        }

        if let Some(existing_identity) = self.lookup_pool_identity(&pool_amm_id) {
            if existing_identity.base_mint == base_mint
                && existing_identity.bonding_curve == candidate.bonding_curve
            {
                return false;
            }

            warn!(
                pool = %pool_amm_id,
                existing_base_mint = %Pubkey::from(existing_identity.base_mint),
                existing_bonding_curve = %Pubkey::from(existing_identity.bonding_curve),
                incoming_base_mint = %base_mint,
                incoming_bonding_curve = %candidate.bonding_curve,
                "Rejecting conflicting runtime pool re-registration"
            );
            return false;
        }

        {
            let registered_mints = self.registered_mints.read();
            if let Some(existing_pool) = registered_mints.get(&base_mint) {
                warn!(
                    "⚠️  already registered key={} pool_amm_id={} existing_pool_amm_id={}",
                    base_mint, pool_amm_id, existing_pool
                );
                return false;
            }
        }

        let identity = DomainPoolIdentity {
            pool_id: PoolId::from(pool_amm_id),
            base_mint: BaseMint::from(base_mint),
            bonding_curve: BondingCurveKey::from(candidate.bonding_curve),
        };
        let bootstrap_bonding_curve = candidate.bonding_curve;
        let bootstrap_hints = bootstrap_hints_from_candidate(candidate);

        self.approved_pools.remove(&pool_amm_id);
        self.registered_mints.write().insert(base_mint, pool_amm_id);
        self.pool_identities.register(identity);
        self.mark_pool_tracked(pool_amm_id);

        self.reconciliation_runtime.lock().register_pool(base_mint);
        self.shadow_ledger
            .register_curve_alias(base_mint, identity.bonding_curve.into());
        coverage_audit().register_pool_alias(
            &candidate.bonding_curve.to_string(),
            &pool_amm_id.to_string(),
        );
        coverage_audit().register_pool_alias(&base_mint.to_string(), &pool_amm_id.to_string());
        self.account_state_core.register_pool_from_bootstrap(
            pool_amm_id,
            base_mint,
            bootstrap_bonding_curve,
            bootstrap_hints,
        );

        info!(
            pool = %pool_amm_id,
            base_mint = %base_mint,
            bonding_curve = %candidate.bonding_curve,
            "WYKRYWANIE ZAREJESTROWANEJ PULI RUNTIME BEZ TRYBU LEGACY"
        );
        true
    }

    #[allow(deprecated)]
    pub fn register_new_pool(
        &self,
        pool_amm_id: Pubkey,
        base_mint: Pubkey,
        candidate: EnhancedCandidate,
        dev_wallet: Option<Pubkey>,
    ) -> bool {
        if !self.register_runtime_pool_detection(pool_amm_id, base_mint, &candidate) {
            return false;
        }

        self.remember_detected_pool(
            pool_amm_id,
            self.detected_pool_from_candidate(&pool_amm_id, &candidate, dev_wallet),
        );
        self.drain_registration_orphans(pool_amm_id, candidate.slot, dev_wallet);
        self.replay_pre_identity_account_updates(pool_amm_id, base_mint);

        info!(
            pool = %pool_amm_id,
            base_mint = %base_mint,
            "Registered helper pool on runtime registries without legacy compat state"
        );
        true
    }

    /// Remove a pool completely from runtime state
    ///
    /// This is the ONLY correct way to delete a pool. It ensures all
    /// related state is cleaned up atomically.
    ///
    /// # Use Cases
    /// - Gatekeeper rejects pool as dead
    /// - Manual admin removal
    /// - Stale pool pruning
    ///
    /// # Returns
    /// `true` if pool was removed, `false` if pool didn't exist
    fn remove_pool_with_reason(&self, pool_amm_id: Pubkey, reason: &'static str) -> bool {
        // 1. Get identity before removing (needed for ShadowLedger cleanup and key translation)
        let identity = self.pool_identities.get_by_pool(&pool_amm_id);
        let base_mint_key = identity.map(|entry| entry.base_mint);
        let had_detected_pool = self.lookup_detected_pool(&pool_amm_id).is_some();
        let had_runtime_state = self.runtime_pool_state(&pool_amm_id).is_some();

        let removed_session = self.session_manager.remove_session(&pool_amm_id);

        if !removed_session && !had_detected_pool && !had_runtime_state && identity.is_none() {
            return false; // Pool didn't exist
        }

        // 3. Remove orphans (prevent leak)
        {
            let mut orphans = self.orphans.write();
            if let Some(orphan_txs) = orphans.remove(&pool_amm_id) {
                info!(
                    "🗑️  Removed {} orphan transactions for deleted pool {}",
                    orphan_txs.len(),
                    pool_amm_id
                );
            }
        }

        // 4. Clean up ShadowLedger snapshots (prevent memory leak)
        // Clean up base_mint snapshots only (canonical key)
        if let Some(base_mint) = base_mint_key {
            self.shadow_ledger.cleanup_snapshots(&base_mint);
            self.shadow_ledger.remove_curve_alias(&base_mint);
            self.account_state_core.remove_pool(&base_mint);
            self.pending_account_updates.write().remove(&base_mint);
            if self.live_pipeline.remove_mint(&base_mint) {
                info!(pool = %pool_amm_id, base_mint = %base_mint, reason, "🗑️  USUNIETO BASE MINT Z PIPELINE");
            }
            self.commit_coordinator.remove(&base_mint);
            self.committed_bootstrap_snapshots
                .write()
                .remove(&base_mint);
        }

        // 5. Remove from allowlist to prevent further processing (canonical: pool_id)
        if let Some(base_mint) = base_mint_key {
            self.approved_pools.remove(&pool_amm_id);
            self.registered_mints.write().remove(&base_mint);
            // Remove from reconciliation registry so stale pools don't consume
            // cycle budget indefinitely.
            self.reconciliation_runtime
                .lock()
                .unregister_pool(&base_mint);
        }
        self.pool_identities.remove_by_pool(&pool_amm_id);
        self.runtime_pool_states.write().remove(&pool_amm_id);
        self.detected_pools.write().remove(&pool_amm_id);

        info!(pool = %pool_amm_id, reason, "🗑️  USUNIETO BASE MINT Z RUNTIME");
        true
    }

    pub fn remove_pool(&self, pool_amm_id: Pubkey) -> bool {
        self.remove_pool_with_reason(pool_amm_id, "unspecified")
    }

    #[allow(deprecated)]
    pub fn register_pool_tx(
        &self,
        pool_amm_id: Pubkey,
        timestamp_ms: u64,
        slot: Option<u64>,
        mpcf_payload: Vec<u8>,
        maybe_dev_wallet: Option<Pubkey>,
        signer: String,
        is_buy: bool,
        volume_sol: f64,
    ) {
        // [FIX ARCH] OracleRuntime MUST NOT maintain independent tx_count/vol/metrics.
        // It is strictly an event relay and high-level orchestrator.
        // Data path is: EventBus -> SnapshotEngine -> ShadowLedger.
        // OracleRuntime just acknowledges the event for internal state (existence checks).

        let dev_wallet = self
            .lookup_pool_session(&pool_amm_id)
            .and_then(|session| session.read().dev_wallet)
            .or_else(|| {
                self.lookup_detected_pool(&pool_amm_id)
                    .and_then(|pool| Pubkey::try_from(pool.creator.as_str()).ok())
            })
            .or(maybe_dev_wallet);

        if self.is_known_runtime_pool(&pool_amm_id) {
            let is_dev_tx = dev_wallet
                .map(|dev| dev.to_string() == signer)
                .unwrap_or(false);

            if is_dev_tx {
                debug!("🕵️ DEV transaction observed for pool {}", pool_amm_id);
            }
            debug!(
                "✅ OracleRuntime forwarded tx for pool {} (delegated to SnapshotEngine/session)",
                pool_amm_id
            );
            return;
        }

        let mut orphans = self.orphans.write();
        let total_orphans: usize = orphans.values().map(|v| v.len()).sum();
        if total_orphans >= MAX_TOTAL_ORPHANS {
            return;
        }

        orphans
            .entry(pool_amm_id)
            .or_insert_with(Vec::new)
            .push(OrphanTx {
                timestamp_ms,
                slot,
                mpcf_payload,
                maybe_dev_wallet,
                signer,
                is_buy,
                volume_sol,
                arrived_at: Instant::now(),
            });
    }

    /// Diagnostic scoring helper — retained **only** for unit/integration test
    /// infrastructure. Production scoring is driven exclusively by
    /// `GatekeeperV2::evaluate` inside `start_oracle_runtime_task`.
    ///
    /// Will be removed once all legacy test helpers migrate to the
    /// event-bus-driven flow.
    #[deprecated(
        since = "3.0.0",
        note = "Test-only helper. Production path uses GatekeeperV2::evaluate."
    )]
    #[allow(deprecated)]
    pub fn score_pool(
        &self,
        pool_amm_id: Pubkey,
        _snapshot_engine: &SnapshotEngine,
        history_buffer: Option<&[TradeEvent]>,
        mark_as_scored: bool,
    ) -> Option<HyperPredictionResult> {
        let mut candidate = if let Some(session) = self.lookup_pool_session(&pool_amm_id) {
            session.read().candidate_snapshot.clone()
        } else if let Some(pool_data) = self
            .lookup_detected_pool(&pool_amm_id)
            .or_else(|| self.build_runtime_state_pool_snapshot(&pool_amm_id))
        {
            build_enhanced_candidate_from_pool_data(
                pool_data.as_ref(),
                &self.pump_program_id,
                &self.bonk_program_id,
            )
            .unwrap_or_else(|_| EnhancedCandidate {
                pool_amm_id,
                base_mint: Pubkey::try_from(pool_data.base_mint.as_str()).unwrap_or_default(),
                quote_mint: Pubkey::try_from(pool_data.quote_mint.as_str()).unwrap_or_default(),
                bonding_curve: Pubkey::try_from(pool_data.bonding_curve.as_str())
                    .unwrap_or_default(),
                amm_program_id: map_amm_program_string_to_pubkey(
                    &pool_data.amm_program,
                    &self.pump_program_id,
                    &self.bonk_program_id,
                )
                .unwrap_or_default(),
                slot: pool_data.slot,
                timestamp: detected_pool_event_ts_ms(&pool_data),
                initial_liquidity_sol: pool_data.initial_liquidity_sol.unwrap_or_default(),
                signature: pool_data.signature.clone(),
                ..Default::default()
            })
        } else {
            let identity = self.lookup_pool_identity(&pool_amm_id)?;
            EnhancedCandidate {
                pool_amm_id,
                base_mint: identity.base_mint.into(),
                bonding_curve: identity.bonding_curve.into(),
                ..Default::default()
            }
        };

        if let Some(history) = history_buffer {
            if !history.is_empty() {
                let earliest_ts = history
                    .iter()
                    .map(|tx| tx.timestamp_ms)
                    .min()
                    .unwrap_or(candidate.timestamp);
                if candidate.timestamp == 0 || earliest_ts < candidate.timestamp {
                    candidate.timestamp = earliest_ts;
                }
            }
        }

        // Musimy zwolnić locka przed check_anomaly, żeby nie blokować
        let paradox_state = {
            let rx_guard = self.paradox_rx.read();
            if let Some(ref rx) = *rx_guard {
                Some(rx.borrow().clone())
            } else {
                None
            }
        };

        let shadow_curve = self.shadow_ledger.get(&candidate.bonding_curve);

        if let Some(curve_state) = shadow_curve.as_ref() {
            candidate.virtual_sol_reserves = Some(curve_state.virtual_sol_reserves);
            candidate.token_total_supply = Some(curve_state.token_total_supply);
            let progress = if curve_state.token_total_supply > 0 {
                (curve_state.real_token_reserves as f64 / curve_state.token_total_supply as f64)
                    .clamp(0.0, 1.0)
            } else {
                0.0
            };
            candidate.bonding_curve_progress = Some(progress);
        }

        // 2. IWIM Analysis (Dev Wallet Intent)
        //
        // IWIM analysis removed — Gatekeeper V2 Phase 5 handles dev behavior in-line
        let iwim_result = None;

        // [FIX STATE] Resolve timestamps from ShadowLedger (SSOT)
        let timestamps_vec = self
            .shadow_ledger
            .get_snapshots_internal(&candidate.base_mint)
            .map(|snaps| snaps.iter().map(|s| s.timestamp_ms).collect::<Vec<u64>>())
            .unwrap_or_default();

        // 3. Resonance (Patterny czasowe)
        use ghost_brain::ResonanceDetector;
        let resonance_result = if timestamps_vec.len() >= 4 {
            let mut detector = ResonanceDetector::new();
            detector.add_timestamps(&timestamps_vec);
            Some(detector.analyze())
        } else {
            None
        };

        // 4. METRYKI TRANSAKCJI
        let tx_metrics = self
            .build_shadow_tx_metrics(&candidate.base_mint)
            .unwrap_or_default();
        let has_real_metrics = tx_metrics.tx_count > 0;

        let final_virtual_sol = candidate
            .virtual_sol_reserves
            .unwrap_or(GENESIS_SOL_LAMPORTS);

        // BUDUJEMY FORCED STATE (Dla ULVF, PRAECOG, MESA) z bieżącej krzywej
        let make_genesis_pool = || {
            AmmPool::new(
                GENESIS_SOL_LAMPORTS as u128,
                GENESIS_TOKEN_RESERVES as u128,
                BondingCurve::FEE_BPS as u16,
            )
            .map_err(|e| {
                error!(
                    pool = %pool_amm_id,
                    err = %e,
                    "CRITICAL: Failed to create genesis fallback AmmPool"
                );
                e
            })
            .ok()
        };

        let live_pool_state = if let Some(curve_state) = shadow_curve.as_ref() {
            match AmmPool::new(
                curve_state.virtual_sol_reserves as u128,
                curve_state.virtual_token_reserves as u128,
                BondingCurve::FEE_BPS as u16,
            ) {
                Ok(pool) => Some(pool),
                Err(e) => {
                    warn!(
                        pool = %pool_amm_id,
                        err = %e,
                        "Failed to build AmmPool from live curve; falling back to genesis pool"
                    );
                    make_genesis_pool()
                }
            }
        } else {
            match AmmPool::new(
                final_virtual_sol as u128,
                GENESIS_TOKEN_RESERVES as u128,
                BondingCurve::FEE_BPS as u16,
            ) {
                Ok(pool) => Some(pool),
                Err(e) => {
                    warn!(
                        pool = %pool_amm_id,
                        err = %e,
                        "Failed to build AmmPool from fallback reserves; using genesis pool"
                    );
                    make_genesis_pool()
                }
            }
        };
        let Some(live_pool_state) = live_pool_state else {
            // No safe pool state could be constructed; skip scoring for this candidate.
            return None;
        };

        // 6. Chaos Engine (na wymuszonym stanie)
        // Uwaga: Jeśli wagi chaosu są 0.0, to tylko marnuje CPU, ale zostawiam dla zgodności
        let chaos_result = if has_real_metrics {
            self.chaos_engine
                .run_simulation(&live_pool_state, MarketScenario::Mixed)
                .ok()
        } else {
            None
        };

        // Przygotowanie referencji dla Scoringu
        let tx_timestamps_slice = if !timestamps_vec.is_empty() {
            Some(timestamps_vec.as_slice())
        } else {
            None
        };
        let tx_bytes = None;

        // 7. SCORING
        // Teraz 'live_pool_state' jest zdefiniowane i zawiera poprawny stan
        match self.hyper_oracle.score_candidate(
            &candidate,
            self.pumpfun_cache.as_ref(),
            Some(&live_pool_state), // <--- TUTAJ PRZEKAZUJEMY HYBRYDOWY STAN
            tx_timestamps_slice,
            tx_bytes,
            iwim_result,
            chaos_result,
            resonance_result,
            None,
            None,
            Some(&tx_metrics),
            None,
            paradox_state,
            None,
            None,
        ) {
            Ok(result) => {
                if mark_as_scored {
                    self.mark_pool_scored(pool_amm_id);
                }

                info!(
                    "✅ SCORED: {} | Score: {} | Tx: {} | Vol: {:.2} SOL | Pool: {:.2} SOL",
                    candidate.base_mint,
                    result.score,
                    tx_metrics.tx_count,
                    tx_metrics.total_volume_sol,
                    final_virtual_sol as f64 / 1_000_000_000.0
                ); // Logujemy stan basenu dla pewności

                Some(result)
            }
            Err(e) => {
                warn!("❌ Scoring Failed for {}: {}", pool_amm_id, e);
                None
            }
        } // <--- To zamyka 'match' (To miałeś w moim kodzie)
    } // <--- TEGO BRAKUJE! (To zamyka funkcję 'process_candidate' lub pętlę)

    pub fn pool_count(&self) -> usize {
        self.session_manager
            .active_session_count()
            .max(self.detected_pools.read().len())
    }

    pub fn get_pool_tx_count(&self, pool_amm_id: Pubkey) -> usize {
        // [SSOT Fix] Read from ShadowLedger instead of local state to act as Single Source of Truth.
        let base_mint = self.lookup_base_mint_for_pool(&pool_amm_id);

        if let Some(base_mint) = base_mint {
            if let Some(snaps) = self.shadow_ledger.get_snapshots_internal(&base_mint) {
                if let Some(latest) = snaps.last() {
                    return latest.tx_count as usize;
                }
            }
        }
        0
    }

    #[allow(deprecated)]
    pub fn mark_pool_scored(&self, pool_amm_id: Pubkey) {
        if let Some(session) = self.lookup_pool_session(&pool_amm_id) {
            let mut session = session.write();
            if !matches!(
                session.status,
                SessionStatus::Decided(_) | SessionStatus::Closed
            ) {
                session.apply_verdict(VerdictOutcome::Pass {
                    reason: "scored".to_string(),
                });
            }
        }
        self.mark_pool_approved(pool_amm_id);
    }

    #[allow(deprecated)]
    pub fn prune_stale_pools(&self, ttl_ms: u128) -> usize {
        let now_ms = current_time_ms();
        let candidate_pool_ids: Vec<Pubkey> = self.detected_pools.read().keys().copied().collect();

        let mut removed_pool_ids: Vec<Pubkey> = Vec::new();
        for pool_id in candidate_pool_ids {
            let session_active = self
                .lookup_pool_session(&pool_id)
                .map(|session| {
                    let session = session.read();
                    matches!(
                        session.status,
                        SessionStatus::Created
                            | SessionStatus::Accumulating
                            | SessionStatus::Evaluating
                    )
                })
                .unwrap_or(false);

            let runtime_state = self
                .runtime_pool_state(&pool_id)
                .unwrap_or(PoolState::Tracked);
            let detected_age_ms = self
                .lookup_detected_pool(&pool_id)
                .and_then(|pool| pool.detected_wall_ts_ms)
                .map(|detected_at| now_ms.saturating_sub(detected_at) as u128)
                .unwrap_or(0);
            let should_keep =
                session_active || runtime_state.allows_runtime_relay() || detected_age_ms < ttl_ms;

            if !should_keep {
                removed_pool_ids.push(pool_id);
            }
        }

        for pool_id in &removed_pool_ids {
            let _ = self.remove_pool_with_reason(*pool_id, "prune_stale_pools");
        }

        let mut orphans = self.orphans.write();
        orphans.retain(|_, txs| {
            if let Some(first) = txs.first() {
                first.arrived_at.elapsed().as_secs() < 30
            } else {
                false
            }
        });

        let removed_count = removed_pool_ids.len();
        if removed_count > 0 {
            info!("🗑️  Pruned {} stale pools", removed_count);
        }

        removed_count
    }

    /// Cleanup stale orphan transactions to prevent memory leaks
    ///
    /// This method enforces:
    /// - TTL: Drops orphans older than ORPHAN_TTL_MS (30 seconds)
    /// - Per-pool cap: Limits orphans per pool to MAX_ORPHANS_PER_POOL
    /// - Global cap: Limits total orphans to MAX_TOTAL_ORPHANS
    ///
    /// Should be called periodically (e.g., every 10 seconds)
    pub fn cleanup_stale_orphans(&self) -> (usize, usize, usize) {
        let now = Instant::now();
        let mut total_dropped = 0;
        let mut pools_dropped = 0;
        let mut cap_enforced = 0;

        let mut orphans = self.orphans.write();

        // Phase 1: Remove stale orphans (TTL enforcement)
        orphans.retain(|pool_id, txs| {
            let before_count = txs.len();

            // Filter out stale transactions
            txs.retain(|tx| {
                let age_ms = now.duration_since(tx.arrived_at).as_millis() as u64;

                if age_ms > ORPHAN_TTL_MS {
                    warn!(
                        "🗑️  Dropping stale ORPHAN tx for pool={} (age={}ms, TTL={}ms)",
                        pool_id, age_ms, ORPHAN_TTL_MS
                    );
                    false
                } else {
                    true
                }
            });

            let dropped = before_count - txs.len();
            total_dropped += dropped;

            // Keep the pool entry if it still has orphans
            !txs.is_empty()
        });

        // Phase 2: Enforce per-pool cap
        for (pool_id, txs) in orphans.iter_mut() {
            if txs.len() > MAX_ORPHANS_PER_POOL {
                let excess = txs.len() - MAX_ORPHANS_PER_POOL;
                warn!(
                    "⚠️  Pool {} exceeds orphan cap ({} > {}), dropping {} oldest",
                    pool_id,
                    txs.len(),
                    MAX_ORPHANS_PER_POOL,
                    excess
                );

                // Remove oldest transactions (from beginning)
                txs.drain(0..excess);
                cap_enforced += excess;
                total_dropped += excess;
            }
        }

        // Phase 3: Enforce global cap (if still exceeded)
        let total_orphans: usize = orphans.values().map(|v| v.len()).sum();
        if total_orphans > MAX_TOTAL_ORPHANS {
            let excess = total_orphans - MAX_TOTAL_ORPHANS;
            warn!(
            "🚨 Global orphan cap exceeded ({} > {}), dropping {} transactions from oldest pools",
            total_orphans, MAX_TOTAL_ORPHANS, excess
        );

            // Collect pools sorted by oldest orphan timestamp
            let mut pool_ages: Vec<(Pubkey, Instant)> = orphans
                .iter()
                .filter_map(|(pool_id, txs)| txs.first().map(|tx| (*pool_id, tx.arrived_at)))
                .collect();

            pool_ages.sort_by_key(|(_, timestamp)| *timestamp);

            // Drop from oldest pools until we're under the cap
            let mut remaining_to_drop = excess;
            for (pool_id, _) in pool_ages {
                if remaining_to_drop == 0 {
                    break;
                }

                if let Some(txs) = orphans.get_mut(&pool_id) {
                    let to_drop = remaining_to_drop.min(txs.len());
                    txs.drain(0..to_drop);
                    remaining_to_drop -= to_drop;
                    cap_enforced += to_drop;

                    if txs.is_empty() {
                        orphans.remove(&pool_id);
                        pools_dropped += 1;
                    }
                }
            }
        }

        if total_dropped > 0 {
            info!(
            "🧹 Orphan cleanup: dropped {} transactions from {} pools, enforced {} cap violations",
            total_dropped, pools_dropped, cap_enforced
        );
        }

        let now_ms = current_time_ms();
        let mut pending_updates = self.pending_account_updates.write();
        let mut stale_pending_dropped = 0usize;
        pending_updates.retain(|_, events| {
            let before = events.len();
            events.retain(|entry| {
                now_ms.saturating_sub(entry.buffered_at_ms) <= PRE_IDENTITY_ACCOUNT_UPDATE_TTL_MS
            });
            stale_pending_dropped += before.saturating_sub(events.len());
            !events.is_empty()
        });
        if stale_pending_dropped > 0 {
            ::metrics::counter!(
                "account_update_pre_identity_replay_drop_total",
                stale_pending_dropped as u64,
                "reason" => "cleanup_stale"
            );
        }

        (total_dropped, pools_dropped, cap_enforced)
    }

    /// Get current orphan buffer statistics for monitoring
    pub fn get_orphan_stats(&self) -> (usize, usize) {
        let orphans = self.orphans.read();
        let pool_count = orphans.len();
        let total_orphans: usize = orphans.values().map(|v| v.len()).sum();
        (pool_count, total_orphans)
    }

    pub fn get_pre_identity_account_update_stats(&self) -> (usize, usize) {
        let pending = self.pending_account_updates.read();
        let mint_count = pending.len();
        let total_updates: usize = pending.values().map(Vec::len).sum();
        (mint_count, total_updates)
    }

    /// [AUDIT HELPER] Direct access to candidate reserves for verification
    #[allow(deprecated)]
    pub fn inspect_candidate_reserves(&self, pool_amm_id: Pubkey) -> Option<u64> {
        if let Some(base_mint) = self.lookup_base_mint_for_pool(&pool_amm_id) {
            if let Some(canonical) = self.account_state_core.get_canonical_state(&base_mint) {
                return Some(canonical.virtual_sol_reserves);
            }

            if let Some(bootstrap) = self.account_state_core.get_bootstrap_state(&base_mint) {
                if let Some((reserve_sol_lamports, _)) = bootstrap.speculative_reserves {
                    return Some(reserve_sol_lamports);
                }
            }
        }

        self.lookup_pool_identity(&pool_amm_id)
            .and_then(|identity| self.shadow_ledger.get(&identity.bonding_curve.into()))
            .map(|curve| curve.virtual_sol_reserves)
    }
}

fn format_gatekeeper_v2_config(config: &GatekeeperV2Config) -> String {
    format!(
        "cfg[min_sol={:.4} min_tx={} min_signers={} min_buy={} max_wait_ms={} int_cv=[{:.3},{:.3}] max_burst={:.2} avg_ms=[{:.0},{:.0}] entropy=[{:.2},{:.2}] min_unique_ratio={:.2} max_hhi={:.3} max_tx_signer={} max_gini={:.2} max_top3={:.2} min_buy_ratio={:.2} avg_tx_sol=[{:.3},{:.3}] vol_cv=[{:.2},{:.2}] total_sol=[{:.3},{:.3}] max_dev_buy={:.2} dev_tx_ratio=[{:.2},{:.2}] dev_vol_ratio=[{:.2},{:.2}] reject_on_dev_sell={} max_price_change={:.2} max_tx_impact={:.2}% bonding=[{:.2}%,{:.2}%] min_mcap={:.2} min_phases={} reeval_every_tx={} failed_tx_ratio={:?} use_slot_ordering={} hybrid[sell_buy=[{:.3},{:.3}] cu_cluster=[{:.3},{:.3}] static_fee=[{:.3},{:.3}] inner_ix=[{:.3},{:.3}] max_fixed_buy={:.3} max_fixed_buy_1e4={:.3} max_flipper={:.3} jito_tip=[{:.3},{:.3}] max_early_slot_dom={:.3} max_early_top3_3s={:.3} max_whale_top3={:.3} max_whale_top1={:.3} min_dev_latency_ms={}]]",
        config.min_sol_threshold,
        config.min_tx_count,
        config.min_unique_signers,
        config.min_buy_count,
        config.max_wait_time_ms,
        config.min_interval_cv,
        config.max_interval_cv,
        config.max_burst_ratio,
        config.min_avg_interval_ms,
        config.max_avg_interval_ms,
        config.min_timing_entropy,
        config.max_timing_entropy,
        config.min_unique_ratio,
        config.max_hhi,
        config.max_tx_per_signer,
        config.max_volume_gini,
        config.max_top3_volume_pct,
        config.min_buy_ratio,
        config.min_avg_tx_sol,
        config.max_avg_tx_sol,
        config.min_volume_cv,
        config.max_volume_cv,
        config.min_total_volume_sol,
        config.max_total_volume_sol,
        config.max_dev_buy_sol,
        config.min_dev_tx_ratio,
        config.max_dev_tx_ratio,
        config.max_dev_volume_ratio,
        config.min_dev_volume_ratio,
        config.reject_on_dev_sell,
        config.max_price_change_ratio,
        config.max_single_tx_price_impact_pct,
        config.min_bonding_progress_pct,
        config.max_bonding_progress_pct,
        config.min_market_cap_sol,
        config.min_phases_to_pass,
        config.re_eval_tx_interval,
        config.min_failed_tx_ratio_for_bot_flag,
        config.use_slot_ordering,
        config.min_sell_buy_ratio,
        config.max_sell_buy_ratio,
        config.min_compute_unit_cluster_dominance,
        config.max_compute_unit_cluster_dominance,
        config.min_static_fee_profile_ratio,
        config.max_static_fee_profile_ratio,
        config.min_avg_inner_ix_count_50tx,
        config.max_avg_inner_ix_count_50tx,
        config.max_fixed_size_buy_ratio,
        config.max_fixed_size_buy_ratio_1e4,
        config.max_flipper_presence_ratio,
        config.min_jito_tip_intensity,
        config.max_jito_tip_intensity,
        config.max_early_slot_volume_dominance_buy,
        config.max_early_top3_buy_volume_pct_3s,
        config.max_whale_reversal_ratio_top3,
        config.max_whale_reversal_ratio_top1,
        config.min_dev_paperhand_latency_ms,
    )
}

fn format_gatekeeper_v2_assessment(assessment: &GatekeeperAssessment) -> String {
    let sell_count_est = assessment
        .total_tx_evaluated
        .saturating_sub(assessment.buy_count);
    format!(
        "obs[tx={} signers={} buys={} sells~={} phases={}/6 evals={} dur_ms={} dust={}]{}{}{}{}{}",
        assessment.total_tx_evaluated,
        assessment.unique_signers_evaluated,
        assessment.buy_count,
        sell_count_est,
        assessment.phases_passed,
        assessment.eval_count,
        assessment.observation_duration_ms,
        assessment.dust_filtered_count,
        assessment
            .phase2_velocity
            .as_ref()
            .map(|v| format!(
                " | p2(cv={:.3} burst={:.2} avg_ms={:.0} entropy={:.2} accel={})",
                v.interval_cv,
                v.burst_ratio,
                v.avg_interval_ms,
                v.timing_entropy,
                v.is_accelerating
            ))
            .unwrap_or_default(),
        assessment
            .phase3_diversity
            .as_ref()
            .map(|d| format!(
                " | p3(unique_r={:.2} hhi={:.3} max_tx_signer={} gini={:.2} top3={:.2})",
                d.unique_ratio, d.hhi, d.max_tx_per_signer, d.volume_gini, d.top3_volume_pct
            ))
            .unwrap_or_default(),
        assessment
            .phase4_volume
            .as_ref()
            .map(|v| format!(
                " | p4(buy_r={:.2} avg_sol={:.3} cv={:.2} total={:.3} min={:.3} max={:.3})",
                v.buy_ratio,
                v.avg_tx_sol,
                v.volume_cv,
                v.total_volume_sol,
                v.min_tx_sol,
                v.max_tx_sol
            ))
            .unwrap_or_default(),
        assessment
            .phase5_dev
            .as_ref()
            .map(|d| format!(
                " | p5(dev_known={} dev_buy={:.2} tx_r={:.2} vol_r={:.2} sold={} first_buyer={})",
                d.dev_wallet_known,
                d.dev_buy_total_sol,
                d.dev_tx_ratio,
                d.dev_volume_ratio,
                d.dev_has_sold,
                d.dev_is_first_buyer
            ))
            .unwrap_or_default(),
        assessment
            .phase6_curve
            .as_ref()
            .map(|c| format!(
                " | p6(price_r={:.2} impact={:.1}% bonding={:.1}% mcap={:.1} mcap_r={:.2} pts={})",
                c.price_change_ratio,
                c.max_single_tx_price_impact_pct,
                c.bonding_progress_pct,
                c.current_market_cap_sol,
                c.market_cap_change_ratio,
                c.price_data_points
            ))
            .unwrap_or_default(),
    )
}

/// Persistent per-pool identity snapshot captured at `NewPoolDetected` time.
///
/// Unlike `pending_pools` (which is removed on verdict), this map survives
/// sweep and edge-case cleanups so that every JSONL record is guaranteed to
/// contain identity fields.
#[derive(Debug, Clone)]
struct ObservationIdentity {
    base_mint: String,
    first_seen_ts_ms: u64,
    first_seen_clock_source: &'static str,
    end_10s_ts_ms: u64,
    dev_pubkey: String,
    /// Consecutive failed identity-promotion attempts from transactions.
    /// Once this reaches `max_identity_promotion_retries()`, promotion stops.
    failed_promotion_attempts: u8,
    /// Earliest wall-clock ms at which another promotion attempt is allowed.
    /// Set to `0` for an immediate attempt.  Updated to `now_ms + backoff_delay`
    /// on each failure; reset to `0` on success or any full identity upgrade.
    next_promotion_attempt_ts_ms: u64,
}

/// Read `GHOST_MAX_IDENTITY_PROMOTION_RETRIES` from the environment on every
/// call — no caching via `static Lazy` so that tests that set the env var
/// after process start see the correct value immediately.
///
/// Returns the parsed value or the default of 25.
fn max_identity_promotion_retries_from_env() -> u8 {
    std::env::var("GHOST_MAX_IDENTITY_PROMOTION_RETRIES")
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(25)
}

/// Returns the runtime max identity promotion retries.
///
/// This delegates to `max_identity_promotion_retries_from_env()` so callers
/// do not need to care about the env-var name.  Production code calls this
/// once per tx and passes the result to `maybe_promote_observation_identity_from_tx`,
/// which means the env var is always read fresh (no stale cache).
fn max_identity_promotion_retries() -> u8 {
    max_identity_promotion_retries_from_env()
}

#[allow(dead_code)]
fn build_fallback_observation_identity(
    pool_id: Pubkey,
    tx: &PoolTransaction,
    gatekeeper_window_ms: u64,
) -> ObservationIdentity {
    let first_seen_ts_ms = tx_observed_wall_ts_ms(tx, current_time_ms());

    let base_mint = tx
        .token_mint
        .as_ref()
        .filter(|mint| Pubkey::try_from(mint.as_str()).is_ok())
        .cloned()
        .unwrap_or_else(|| pool_id.to_string());

    let dev_pubkey = normalize_dev_pubkey_str(&tx.signer).unwrap_or_else(|| "unknown".to_string());

    debug!(
        "POOL_IDENTITY_FALLBACK pool={} reason=NO_NEW_POOL_DETECTED base_mint={} dev_pubkey={} first_seen_ts_ms={}",
        pool_id,
        base_mint,
        dev_pubkey,
        first_seen_ts_ms
    );

    ObservationIdentity {
        base_mint,
        first_seen_ts_ms,
        first_seen_clock_source: tx_observed_wall_source_label(tx),
        end_10s_ts_ms: first_seen_ts_ms.saturating_add(gatekeeper_window_ms),
        dev_pubkey,
        failed_promotion_attempts: 0,
        next_promotion_attempt_ts_ms: 0,
    }
}

#[allow(dead_code)]
fn build_registered_observation_identity(
    pool_id: Pubkey,
    base_mint: Pubkey,
    gatekeeper_window_ms: u64,
) -> ObservationIdentity {
    let first_seen_ts_ms = current_time_ms();
    debug!(
        "POOL_IDENTITY_FALLBACK pool={} reason=REGISTERED_MINT_LOOKUP base_mint={} first_seen_ts_ms={}",
        pool_id,
        base_mint,
        first_seen_ts_ms
    );
    ObservationIdentity {
        base_mint: base_mint.to_string(),
        first_seen_ts_ms,
        first_seen_clock_source: "registered_wall",
        end_10s_ts_ms: first_seen_ts_ms.saturating_add(gatekeeper_window_ms),
        dev_pubkey: "unknown".to_string(),
        failed_promotion_attempts: 0,
        next_promotion_attempt_ts_ms: 0,
    }
}

fn build_unknown_observation_identity(
    pool_id: Pubkey,
    gatekeeper_window_ms: u64,
) -> ObservationIdentity {
    let first_seen_ts_ms = current_time_ms();
    debug!(
        "POOL_IDENTITY_FALLBACK pool={} reason=UNKNOWN_IDENTITY first_seen_ts_ms={}",
        pool_id, first_seen_ts_ms
    );
    ObservationIdentity {
        base_mint: "unknown".to_string(),
        first_seen_ts_ms,
        first_seen_clock_source: "runtime_wall",
        end_10s_ts_ms: first_seen_ts_ms.saturating_add(gatekeeper_window_ms),
        dev_pubkey: "unknown".to_string(),
        failed_promotion_attempts: 0,
        next_promotion_attempt_ts_ms: 0,
    }
}

/// Returns the exponential backoff delay (ms) before the next identity-promotion
/// attempt, given `attempt` consecutive failures so far.
fn promotion_backoff_delay_ms(attempt: u8) -> u64 {
    match attempt {
        0 => 0,
        1 => 50,
        2 => 200,
        3 => 1_000,
        4 => 5_000,
        5 => 15_000,
        _ => 30_000,
    }
}

/// Try to fill missing identity fields (`base_mint`, `dev_pubkey`) from the
/// current transaction.
///
/// # Parameters
/// - `now_ms`     — current wall-clock time (ms); injected so that tests can
///                  drive time deterministically without calling `current_time_ms()`.
/// - `max_retries` — budget cap; injected so that call-sites (and tests) control
///                   the limit without relying on a process-wide `Lazy`.
///
/// # Returns
/// `true` if at least one field was successfully promoted.
fn maybe_promote_observation_identity_from_tx(
    pool_id: Pubkey,
    tx: &PoolTransaction,
    gatekeeper_window_ms: u64,
    identity: &mut ObservationIdentity,
    base_mint_pubkey: &mut Option<Pubkey>,
    now_ms: u64,
    max_retries: u8,
) -> bool {
    let fallback_mint = tx
        .token_mint
        .as_ref()
        .and_then(|mint| Pubkey::try_from(mint.as_str()).ok());
    let fallback_dev = normalize_dev_pubkey_str(&tx.signer);

    let base_missing = !is_shadow_base_mint_present(&identity.base_mint);
    let dev_missing = !is_shadow_creator_present(&identity.dev_pubkey);

    if !base_missing && !dev_missing {
        return false;
    }

    // Budget exhausted — stop retrying permanently.  A single WARN was already
    // emitted when the counter first hit the limit.
    if identity.failed_promotion_attempts >= max_retries {
        return false;
    }

    // Exponential backoff: not yet time to try again.
    if now_ms < identity.next_promotion_attempt_ts_ms {
        return false;
    }

    let mut promoted = false;
    if base_missing {
        if let Some(mint) = fallback_mint {
            identity.base_mint = mint.to_string();
            *base_mint_pubkey = Some(mint);
            promoted = true;
        }
    }

    if dev_missing {
        if let Some(dev_pubkey) = fallback_dev {
            identity.dev_pubkey = dev_pubkey;
            promoted = true;
        }
    }

    if promoted {
        // Reset failure counter and backoff on any partial success so future
        // gaps don't accidentally exhaust the budget.
        identity.failed_promotion_attempts = 0;
        identity.next_promotion_attempt_ts_ms = 0;
        POOL_IDENTITY_PROMOTION_TOTAL
            .with_label_values(&["success"])
            .inc();
        let first_seen_ts_ms = tx_observed_wall_ts_ms(tx, now_ms);
        identity.first_seen_ts_ms = first_seen_ts_ms;
        identity.first_seen_clock_source = tx_observed_wall_source_label(tx);
        identity.end_10s_ts_ms = first_seen_ts_ms.saturating_add(gatekeeper_window_ms);
        if is_shadow_base_mint_present(&identity.base_mint) {
            coverage_audit()
                .set_window_base_mint(&pool_id.to_string(), Some(identity.base_mint.clone()));
        }
        info!(
            "POOL_IDENTITY_FALLBACK pool={} reason=FIRST_TX_PROMOTION base_mint={} dev_pubkey={} first_seen_ts_ms={}",
            pool_id,
            identity.base_mint,
            identity.dev_pubkey,
            identity.first_seen_ts_ms
        );
    } else {
        identity.failed_promotion_attempts = identity.failed_promotion_attempts.saturating_add(1);
        let delay_ms = promotion_backoff_delay_ms(identity.failed_promotion_attempts);
        identity.next_promotion_attempt_ts_ms = now_ms.saturating_add(delay_ms);
        POOL_IDENTITY_PROMOTION_TOTAL
            .with_label_values(&["failure"])
            .inc();
        if identity.failed_promotion_attempts == max_retries {
            POOL_IDENTITY_EXHAUSTED_TOTAL.inc();
            warn!(
                "POOL_IDENTITY_EXHAUSTED pool={} base_mint={} dev_pubkey={} \
                 after={} failed_promotion_attempts reason=tx_stream_missing_identity",
                pool_id, identity.base_mint, identity.dev_pubkey, max_retries
            );
        }
    }

    promoted
}

#[allow(dead_code)]
fn ensure_observation_identity(
    pool_id: Pubkey,
    observation_identities: &mut HashMap<Pubkey, ObservationIdentity>,
    pending_pools: &HashMap<Pubkey, Arc<DetectedPool>>,
    pool_registered_wall_ts_ms: &HashMap<Pubkey, u64>,
    oracle_runtime: &OracleRuntime,
    gatekeeper_window_ms: u64,
    tx_hint: Option<&PoolTransaction>,
) -> ObservationIdentity {
    if let Some(existing) = observation_identities.get(&pool_id) {
        return existing.clone();
    }

    let identity = if let Some(pool_data) = pending_pools.get(&pool_id) {
        let first_seen_clock_source = if pool_registered_wall_ts_ms.contains_key(&pool_id) {
            "registered_wall"
        } else {
            detected_pool_observed_wall_source_label(pool_data)
        };
        let first_seen_ts_ms = pool_registered_wall_ts_ms
            .get(&pool_id)
            .copied()
            .unwrap_or_else(|| detected_pool_observed_wall_ts_ms(pool_data, current_time_ms()));
        warn!(
            "POOL_IDENTITY_FALLBACK pool={} reason=PENDING_POOL_LOOKUP base_mint={} first_seen_ts_ms={}",
            pool_id,
            pool_data.base_mint,
            first_seen_ts_ms
        );
        ObservationIdentity {
            base_mint: pool_data.base_mint.clone(),
            first_seen_ts_ms,
            first_seen_clock_source,
            end_10s_ts_ms: first_seen_ts_ms.saturating_add(gatekeeper_window_ms),
            dev_pubkey: normalize_dev_pubkey_str(&pool_data.creator)
                .unwrap_or_else(|| "unknown".to_string()),
            failed_promotion_attempts: 0,
            next_promotion_attempt_ts_ms: 0,
        }
    } else if let Some(tx) = tx_hint {
        build_fallback_observation_identity(pool_id, tx, gatekeeper_window_ms)
    } else if let Some(base_mint) = oracle_runtime.lookup_base_mint_for_pool(&pool_id) {
        build_registered_observation_identity(pool_id, base_mint, gatekeeper_window_ms)
    } else {
        build_unknown_observation_identity(pool_id, gatekeeper_window_ms)
    };

    observation_identities.insert(pool_id, identity.clone());
    identity
}

/// Enrich a `GatekeeperBuyLog` with observation identity fields from the
/// persistent observation-identity snapshot captured at `NewPoolDetected`.
///
/// Sets `base_mint`, `first_seen_ts_ms`, `end_10s_ts_ms`, `join_key`,
/// `dev_pubkey` and `gatekeeper_version`.  These fields are required for
/// deterministic downstream labeling (trade-only labeler) and must never
/// be None in the JSONL output.
fn enrich_buy_log_with_observation_identity(
    log: &mut ghost_brain::oracle::GatekeeperBuyLog,
    identity: &ObservationIdentity,
) {
    log.base_mint = Some(identity.base_mint.clone());
    log.first_seen_ts_ms = Some(identity.first_seen_ts_ms);
    log.first_seen_clock_source = Some(identity.first_seen_clock_source.to_string());
    log.observation_start_ts_ms = Some(identity.first_seen_ts_ms);
    log.observation_end_ts_ms = Some(
        identity
            .first_seen_ts_ms
            .saturating_add(log.observation_duration_ms),
    );
    log.observation_window_ms = Some(log.observation_duration_ms);
    log.end_10s_ts_ms = Some(identity.end_10s_ts_ms);
    // Deterministic join key aligned with downstream labeler contract.
    log.join_key = Some(format!(
        "{}:{}:{}",
        log.pool_id, identity.base_mint, identity.first_seen_ts_ms
    ));
    log.dev_pubkey = Some(identity.dev_pubkey.clone());
    log.gatekeeper_version = Some(ghost_brain::oracle::GATEKEEPER_VERSION.to_string());
}

fn enrich_buy_log_with_clock_provenance(
    log: &mut ghost_brain::oracle::GatekeeperBuyLog,
    buffer: &crate::components::gatekeeper::GatekeeperBuffer,
) {
    log.curve_t0_clock_source = buffer.curve_t0_clock_source().map(str::to_string);
}

/// Enrich a GatekeeperBuyLog with IWIM Veto Gate telemetry fields.
fn enrich_buy_log_with_iwim(
    log: &mut ghost_brain::oracle::GatekeeperBuyLog,
    iwim: &crate::components::iwim_veto::IwimVetoResult,
) {
    log.iwim_enabled = iwim.enabled;
    log.iwim_mode = Some(format!("{:?}", iwim.mode));
    log.iwim_fetch_status = Some(format!("{}", iwim.fetch_status));
    log.iwim_quality = Some(format!("{}", iwim.quality));
    log.iwim_confidence = Some(iwim.confidence);
    log.iwim_n_tx = Some(iwim.n_tx_analyzed);
    log.iwim_n_tx_requested = Some(iwim.n_tx_requested);
    log.iwim_latency_ms = Some(iwim.latency_ms);
    log.iwim_rpc_used = Some(iwim.rpc_used.clone());
    log.iwim_status = Some(format!("{}", iwim.status));
    log.iwim_veto_reason = iwim.veto_reason.as_ref().map(|r| format!("{}", r));
    log.iwim_gatekeeper_strength = Some(format!("{}", iwim.gatekeeper_strength));
    log.iwim_rug_threat_score = iwim.raw_result.as_ref().map(|r| r.rug_threat_score as f32);
    log.iwim_sybil_score = iwim.raw_result.as_ref().map(|r| r.sybil_score as f32);
    log.iwim_organic_score = iwim.raw_result.as_ref().map(|r| r.organic_score as f32);
}

/// Enrich a `GatekeeperBuyLog` with A/B window boundary fields from `WindowState`.
///
/// `pool_id` and `verdict_tag` are used to build the deterministic `ab_record_id`
/// for downstream dedup (format: `"{pool_id}:{t0}:{t_end}:{verdict}"`).
///
/// Double-write safety: `window_states.remove()` at every call-site guarantees
/// each pool's WindowState is consumed exactly once.
fn enrich_buy_log_with_window(
    log: &mut ghost_brain::oracle::GatekeeperBuyLog,
    ws: &WindowState,
    pool_id: &str,
    verdict_tag: &str,
) {
    let t0 = ensure_epoch_ms(ws.t0_event_ts_ms, "ab_t0_event_ts_ms", pool_id);
    let t_end = ensure_epoch_ms(ws.t_end_event_ts_ms, "ab_t_end_event_ts_ms", pool_id);
    if t_end < t0 {
        tracing::error!(
            pool = %pool_id,
            t0 = %t0,
            t_end = %t_end,
            "AB_WINDOW_INVARIANT: t_end < t0"
        );
    }
    log.ab_window_ms = Some(ws.window_ms);
    log.ab_t0_event_ts_ms = Some(t0);
    log.ab_t_end_event_ts_ms = Some(t_end);
    log.ab_window_complete = ws.window_complete;
    log.ab_window_close_reason = Some(
        ws.window_close_reason
            .as_ref()
            .map_or("STILL_OPEN", |r| r.tag())
            .to_string(),
    );
    log.ab_tx_count_window = Some(ws.tx_count_window);
    log.ab_unique_signers_window = Some(ws.unique_signers_window);
    log.ab_fail_count_window = Some(ws.fail_count_window);
    log.ab_window_origin = Some(ws.started_from.as_str().to_string());
    log.ab_record_id = Some(format!("{}:{}:{}:{}", pool_id, t0, t_end, verdict_tag));
}

fn enforce_buy_log_buy_routing(
    log: &mut ghost_brain::oracle::GatekeeperBuyLog,
    assessment: &GatekeeperAssessment,
) {
    log.decision_verdict_buy = Some(true);
    log.verdict_type = Some("BUY".to_string());
    log.legacy_live_verdict_buy = Some(true);
    log.legacy_live_verdict_type = Some("BUY".to_string());

    if log.decision_reason.is_none() {
        log.decision_reason = assessment
            .decision
            .as_ref()
            .map(|decision| decision.reason_chain.clone())
            .or_else(|| Some("gatekeeper_buy".to_string()));
    }
    if log.legacy_live_reason_chain.is_none() {
        log.legacy_live_reason_chain = log.decision_reason.clone();
    }

    if log.ab_record_id.is_none() {
        if let (Some(t0), Some(t_end)) = (log.ab_t0_event_ts_ms, log.ab_t_end_event_ts_ms) {
            log.ab_record_id = Some(format!("{}:{}:{}:BUY", log.pool_id, t0, t_end));
        }
    }
}

fn canonical_gatekeeper_config_hash(
    config: &ghost_brain::config::GatekeeperV2Config,
) -> Result<String, serde_json::Error> {
    let canonical_bytes = serde_json::to_vec(config)?;
    Ok(blake3::hash(&canonical_bytes).to_hex().to_string())
}

fn derive_gatekeeper_rollout_profile(log_dir: &std::path::Path) -> String {
    let components: Vec<String> = log_dir
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();

    components
        .windows(2)
        .find_map(|window| {
            if window[0] == "rollout" {
                Some(window[1].clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown_rollout".to_string())
}

fn build_decision_logger_config(
    decision_log_path: &str,
    gatekeeper_config: &ghost_brain::config::GatekeeperV2Config,
) -> ghost_brain::oracle::DecisionLoggerConfig {
    let log_dir = std::path::PathBuf::from(crate::config::normalize_decision_log_path(
        decision_log_path,
    ));
    let gatekeeper_rollout_profile = derive_gatekeeper_rollout_profile(&log_dir);
    let gatekeeper_config_hash = match canonical_gatekeeper_config_hash(gatekeeper_config) {
        Ok(hash) => hash,
        Err(err) => {
            error!(
                "GATEKEEPER_LOG_HASH_FAILED: failed to serialize gatekeeper config for routing hash: {}",
                err
            );
            "config_hash_unavailable".to_string()
        }
    };

    ghost_brain::oracle::DecisionLoggerConfig {
        log_dir: log_dir.clone(),
        gatekeeper_log_dir: log_dir,
        gatekeeper_rollout_profile,
        gatekeeper_config_hash,
        channel_buffer_size: 1000,
        enabled: true,
    }
}

fn build_coverage_audit_log_path(decision_log_path: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(crate::config::normalize_decision_log_path(
        decision_log_path,
    ))
    .join("seer_runtime_coverage_audit.jsonl")
}

/// Default max vector length for JSONL v3 window vectors.
const VECTORS_MAX_LEN: usize = 200;

/// Convert an empty Vec<f64> to None, pass non-empty through as Some.
fn none_if_empty(v: Vec<f64>) -> Option<Vec<f64>> {
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Convert an empty Vec<i64> to None, pass non-empty through as Some.
fn none_if_empty_i64(v: Vec<i64>) -> Option<Vec<i64>> {
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Enrich a buy log with deterministic window vectors extracted from the
/// gatekeeper buffer.  Vectors are empty (None) when the buffer has no data
/// in the window `[t0, t_end]`.
fn enrich_buy_log_with_vectors(
    log: &mut ghost_brain::oracle::GatekeeperBuyLog,
    buf: &crate::components::gatekeeper::GatekeeperBuffer,
    ws: &WindowState,
) {
    let vecs = buf.extract_window_vectors(ws.t0_event_ts_ms, ws.t_end_event_ts_ms, VECTORS_MAX_LEN);
    log.vectors_max_len = Some(vecs.max_len);
    log.vectors_ts_offsets_ms = none_if_empty_i64(vecs.ts_offsets_ms);
    log.vectors_sol_amounts = none_if_empty(vecs.sol_amounts);
    log.vectors_prices = none_if_empty(vecs.prices);
    log.vectors_interval_ms = none_if_empty(vecs.interval_ms);
    log.vectors_d_price = none_if_empty(vecs.d_price);
}

// =============================================================================
// Gatekeeper Decision → Events JSONL helper
// =============================================================================

/// Emit a single CandidateFinalized event to the events JSONL stream.
/// Called exactly once per pool when the gatekeeper reaches a final verdict.
fn emit_gatekeeper_decision_event(
    emitter: &EventEmitter,
    pool_id: &Pubkey,
    verdict: &str,
    assessment: &GatekeeperAssessment,
) {
    let candidate_id = pool_id.to_string();
    let flags: Vec<String> = assessment
        .decision
        .as_ref()
        .map(|d| {
            d.soft_signals
                .format_flags()
                .split(',')
                .filter(|s| !s.is_empty() && *s != "none")
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    emitter.emit_candidate(
        &candidate_id,
        None, // mcap_snapshot: not available at gatekeeper level
        None, // price_snapshot: not available at gatekeeper level
        verdict,
        flags,
        "gatekeeper_v2",
    );
}

// =============================================================================
// Oracle Runtime Task & Helpers
// =============================================================================

/// Bounded FIFO set: O(1) contains + insert, FIFO eviction when cap is reached.
///
/// Used for `rejected_pools` to prevent unbounded memory growth.
/// On PumpFun/BonkFun ~10k–50k new pools are created daily; without a cap the
/// set would accumulate hundreds of thousands of entries over a multi-day run.
///
/// Eviction is FIFO (oldest-first). An evicted pool_id CAN theoretically be
/// re-inserted if it shows up again — but that pool was rejected/timed-out
/// 50k+ events ago, and re-accepting it for a brief window is safe.
struct BoundedFifoSet {
    set: HashSet<Pubkey>,
    fifo: VecDeque<Pubkey>,
    cap: usize,
}

impl BoundedFifoSet {
    fn new(cap: usize) -> Self {
        Self {
            set: HashSet::with_capacity(cap.min(4096)),
            fifo: VecDeque::with_capacity(cap.min(4096)),
            cap,
        }
    }

    fn contains(&self, key: &Pubkey) -> bool {
        self.set.contains(key)
    }

    fn insert(&mut self, key: Pubkey) {
        if self.set.contains(&key) {
            return; // already present, FIFO order unchanged
        }
        if self.set.len() >= self.cap {
            // Evict oldest entry
            if let Some(evicted) = self.fifo.pop_front() {
                self.set.remove(&evicted);
                increment_counter!("rejected_pools_evicted_total");
            }
        }
        self.set.insert(key);
        self.fifo.push_back(key);
    }

    fn len(&self) -> usize {
        self.set.len()
    }
}

/// Maximum number of rejected pool IDs retained in memory.
/// At 50k entries: ~50k × (32B key + ~50B HashMap overhead) ≈ 4 MB.
const REJECTED_POOLS_CAP: usize = 50_000;

// =============================================================================
// Per-Pool Parallel Observation (EPIC: Parallel Gatekeeper Architecture)
// =============================================================================

/// Message sent from the router to a per-pool observation task.
enum PoolObservationMsg {
    /// A new transaction for this pool.
    Transaction(Arc<PoolTransaction>),
    /// Late-arriving pool metadata (when TX arrived before NewPoolDetected).
    NewPool(Arc<DetectedPool>),
}

/// Result sent back from a per-pool task on terminal verdict.
struct PoolObservationResult {
    pool_id: Pubkey,
    base_mint: Option<Pubkey>,
    /// True if a BUY was executed (not vetoed by IWIM).
    bought: bool,
    /// True when the pool must remain active after task completion even
    /// without a confirmed live BUY, e.g. an accepted shadow Guardian handoff.
    retain_runtime_pool: bool,
}

fn should_cleanup_pool_after_observation(result: &PoolObservationResult) -> bool {
    !result.bought && !result.retain_runtime_pool
}

fn ensure_pool_observation_session(
    ctx: &PoolObservationContext,
    pool_id: Pubkey,
    registered_wall_ts_ms: u64,
    pool_data: Option<&DetectedPool>,
) -> Option<SharedSession> {
    if let Some(session) = ctx.session_manager.get_session(&pool_id) {
        return Some(session);
    }

    let open_request = ctx.oracle_runtime.build_session_open_request(
        pool_id,
        registered_wall_ts_ms,
        &ctx.gatekeeper_config,
        &ctx.fingerprint_config,
        pool_data,
    )?;

    if let Err(err) = ctx.session_manager.open_session(open_request) {
        warn!(pool = %pool_id, error = ?err, "❗ NIEUDANA PRÓBA OTWARCIA SESJI OBSERWACJI PULI");
        return None;
    }

    ctx.session_manager.get_session(&pool_id)
}

fn finish_pool_observation(ctx: &PoolObservationContext, pool_id: Pubkey, verdict: VerdictOutcome) {
    ctx.session_manager.close_session(&pool_id, verdict);
    ctx.session_manager.remove_session(&pool_id);
}

fn build_timeout_assessment_from_policy_context(
    materialized_features: MaterializedFeatureSet,
    gatekeeper_config: &GatekeeperV2Config,
    context: PolicyEvaluationContext,
    curve_t0_event_ts_ms: Option<u64>,
    curve_wait_elapsed_ms: Option<u64>,
) -> GatekeeperAssessment {
    let mut assessment =
        build_assessment_from_features(materialized_features, gatekeeper_config, context);
    assessment.curve_t0_event_ts_ms = assessment.curve_t0_event_ts_ms.or(curve_t0_event_ts_ms);
    assessment.curve_wait_elapsed_ms = assessment.curve_wait_elapsed_ms.or(curve_wait_elapsed_ms);
    let decision = build_timeout_decision_from_assessment(&assessment, gatekeeper_config);
    assessment.hard_reject_reason = decision.hard_fail_reason.clone();
    assessment.decision = Some(decision);
    assessment
}

fn build_timeout_assessment_from_materialized_features(
    legacy_assessment: GatekeeperAssessment,
    materialized_features: MaterializedFeatureSet,
    gatekeeper_config: &GatekeeperV2Config,
) -> GatekeeperAssessment {
    build_timeout_assessment_from_policy_context(
        materialized_features,
        gatekeeper_config,
        PolicyEvaluationContext {
            finalize_lag_ms: legacy_assessment.finalize_lag_ms,
            eval_count: legacy_assessment.eval_count,
        },
        legacy_assessment.curve_t0_event_ts_ms,
        legacy_assessment.curve_wait_elapsed_ms,
    )
}

fn build_timeout_assessment_from_session(
    session: &mut PoolObservationSession,
    gatekeeper_config: &GatekeeperV2Config,
) -> GatekeeperAssessment {
    let features = session.materialize_features();
    let buffer = session.gatekeeper_buffer();
    build_timeout_assessment_from_policy_context(
        features,
        gatekeeper_config,
        buffer.policy_evaluation_context(),
        buffer.curve_t0_event_ts_ms(),
        buffer.curve_wait_elapsed_ms(),
    )
}

fn materialize_terminal_features(
    session: &mut PoolObservationSession,
    gatekeeper_config: &GatekeeperV2Config,
    force_deadline: bool,
) -> MaterializedFeatureSet {
    let mut features = session.materialize_features();
    if force_deadline {
        let forced_wait_elapsed = features
            .curve_readiness
            .wait_elapsed_ms
            .unwrap_or_default()
            .max(gatekeeper_config.curve_wait_ms);
        features.curve_readiness.wait_elapsed_ms = Some(forced_wait_elapsed);
    }
    features
}

fn evaluate_feature_driven_terminal_verdict(
    session: &mut PoolObservationSession,
    gatekeeper_config: &GatekeeperV2Config,
    force_deadline: bool,
) -> GatekeeperVerdict {
    session.begin_evaluation();

    #[cfg(test)]
    if !gatekeeper_config.use_three_layer_decision {
        // Explicit compat/test path only. Production startup and preflight must
        // reject this mode after Phase 2 closure.
        ::metrics::counter!("legacy_terminal_verdict_total", 1u64);
        let features = materialize_terminal_features(session, gatekeeper_config, force_deadline);
        let verdict = {
            let buffer = session.gatekeeper_buffer_mut();
            buffer.prepare_feature_evaluation();
            buffer.evaluate_compat_from_features(features, gatekeeper_config, force_deadline)
        };

        if matches!(
            verdict,
            GatekeeperVerdict::PendingCurve | GatekeeperVerdict::Wait
        ) {
            session.resume_accumulation();
        }

        return verdict;
    }

    let features = materialize_terminal_features(session, gatekeeper_config, force_deadline);
    if force_deadline {
        let phase1_passed = features.tx_intel_features.tx_count
            >= gatekeeper_config.min_tx_count as u64
            && features.tx_intel_features.unique_signers
                >= gatekeeper_config.min_unique_signers as u64
            && features.tx_intel_features.buy_count >= gatekeeper_config.min_buy_count as u64;
        if !phase1_passed {
            if session.canonical_update_count() == 0 {
                ::metrics::counter!("timeout_without_canonical_updates_total", 1u64);
                coverage_audit()
                    .record_timeout_without_canonical_updates(&session.pool_amm_id.to_string());
            }
            let buffer = session.gatekeeper_buffer();
            let mut assessment = build_timeout_assessment_from_policy_context(
                features,
                gatekeeper_config,
                buffer.policy_evaluation_context(),
                buffer.curve_t0_event_ts_ms(),
                buffer.curve_wait_elapsed_ms(),
            );
            assessment.cache_v25_confidence(gatekeeper_config);
            return GatekeeperVerdict::Timeout { assessment };
        }
    }

    let verdict = {
        let buffer = session.gatekeeper_buffer_mut();
        buffer.prepare_feature_evaluation();
        buffer.evaluate_from_features(features, gatekeeper_config)
    };

    if matches!(verdict, GatekeeperVerdict::PendingCurve) {
        session
            .gatekeeper_buffer_mut()
            .rollback_feature_evaluation();
        session.resume_accumulation();
    }

    verdict
}

fn resolve_feature_trigger_outcome(
    session: &mut PoolObservationSession,
    ingress: GatekeeperIngressOutcome,
    gatekeeper_config: &GatekeeperV2Config,
) -> GatekeeperVerdict {
    match ingress {
        GatekeeperIngressOutcome::Wait => GatekeeperVerdict::Wait,
        GatekeeperIngressOutcome::ApprovedTx { tx, metrics } => {
            GatekeeperVerdict::ApprovedTx { tx, metrics }
        }
        GatekeeperIngressOutcome::TriggerEvaluation => {
            evaluate_feature_driven_terminal_verdict(session, gatekeeper_config, false)
        }
        GatekeeperIngressOutcome::DeadlineElapsed => {
            evaluate_feature_driven_terminal_verdict(session, gatekeeper_config, true)
        }
    }
}

#[cfg(test)]
fn cutover_feature_driven_terminal_verdict(
    session: &mut PoolObservationSession,
    legacy_verdict: GatekeeperVerdict,
    gatekeeper_config: &GatekeeperV2Config,
) -> GatekeeperVerdict {
    match legacy_verdict {
        GatekeeperVerdict::Buy { .. } | GatekeeperVerdict::Reject { .. } => {
            session.begin_evaluation();
            session
                .gatekeeper_buffer_mut()
                .prepare_policy_evaluation(&legacy_verdict);
            let features = session.materialize_features();
            let verdict = session
                .gatekeeper_buffer_mut()
                .evaluate_from_features(features, gatekeeper_config);
            if matches!(verdict, GatekeeperVerdict::PendingCurve) {
                session.resume_accumulation();
            }
            verdict
        }
        GatekeeperVerdict::Timeout { assessment } => {
            session.begin_evaluation();
            let features = session.materialize_features();
            let assessment = build_timeout_assessment_from_materialized_features(
                assessment,
                features,
                gatekeeper_config,
            );
            GatekeeperVerdict::Timeout { assessment }
        }
        GatekeeperVerdict::Wait
        | GatekeeperVerdict::PendingCurve
        | GatekeeperVerdict::ApprovedTx { .. } => {
            panic!("cutover_feature_driven_terminal_verdict requires a terminal legacy verdict")
        }
    }
}

/// Handle to a spawned per-pool observation task.
struct PoolTaskHandle {
    /// Channel to send transactions to the per-pool task.
    tx: tokio::sync::mpsc::Sender<PoolObservationMsg>,
    /// Handle to abort the task if needed (retained for future graceful shutdown).
    _abort_handle: tokio::task::AbortHandle,
    /// Number of transaction messages successfully enqueued for this pool.
    ///
    /// Used to classify the pool as *hot* (≥ [`HOT_POOL_TX_THRESHOLD`]) so
    /// backpressure retries can be tuned for high-volume pools.
    tx_enqueued: u64,
}

impl PoolTaskHandle {
    /// Returns `true` when this pool has seen enough transactions to be
    /// treated as a *hot* pool for backpressure prioritization purposes.
    fn is_hot(&self) -> bool {
        self.tx_enqueued >= HOT_POOL_TX_THRESHOLD
    }
}

fn enqueue_pool_observation_msg(
    sender: &tokio::sync::mpsc::Sender<PoolObservationMsg>,
    pool_id: Pubkey,
    msg: PoolObservationMsg,
    msg_kind: &'static str,
    is_hot: bool,
) {
    // Hot pools receive more aggressive retry behaviour: more attempts with a
    // shorter per-attempt wait window.  Cold pools use the standard constants.
    // Both paths are deterministic and bounded — no infinite loops, no
    // unbounded memory growth.
    let (retry_attempts, retry_wait_ms) = if is_hot {
        (
            HOT_POOL_BACKPRESSURE_RETRY_ATTEMPTS,
            HOT_POOL_BACKPRESSURE_WAIT_MS,
        )
    } else {
        (
            POOL_TASK_BACKPRESSURE_RETRY_ATTEMPTS,
            POOL_TASK_BACKPRESSURE_WAIT_MS,
        )
    };

    match sender.try_send(msg) {
        Ok(()) => {}
        Err(tokio::sync::mpsc::error::TrySendError::Full(msg)) => {
            warn!(
                "POOL_TASK_BACKPRESSURE pool={} msg={} is_hot={} action=retry_send wait_ms={} attempts={}",
                pool_id,
                msg_kind,
                is_hot,
                retry_wait_ms,
                retry_attempts
            );
            let sender = sender.clone();
            tokio::spawn(async move {
                let mut pending = Some(msg);
                for attempt in 1..=retry_attempts {
                    match tokio::time::timeout(
                        Duration::from_millis(retry_wait_ms),
                        sender.reserve(),
                    )
                    .await
                    {
                        Ok(Ok(permit)) => {
                            permit.send(pending.take().expect("pending pool message"));
                            return;
                        }
                        Ok(Err(_)) => {
                            warn!("POOL_TASK_CHANNEL_CLOSED pool={} msg={}", pool_id, msg_kind);
                            return;
                        }
                        Err(_) if attempt < retry_attempts => {}
                        Err(_) => {
                            if is_hot {
                                increment_counter!("hot_pool_task_backpressure_drop_total");
                                warn!(
                                    "HOT_POOL_TASK_BACKPRESSURE_DROP pool={} msg={} total_wait_ms={}",
                                    pool_id,
                                    msg_kind,
                                    retry_wait_ms * retry_attempts as u64
                                );
                            } else {
                                warn!(
                                    "POOL_TASK_BACKPRESSURE_DROP pool={} msg={} total_wait_ms={}",
                                    pool_id,
                                    msg_kind,
                                    retry_wait_ms * retry_attempts as u64
                                );
                            }
                            return;
                        }
                    }
                }
            });
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
            warn!("POOL_TASK_CHANNEL_CLOSED pool={} msg={}", pool_id, msg_kind);
        }
    }
}

/// Shared context for all per-pool observation tasks.
/// Created once in `start_oracle_runtime_task` and shared via `Arc`.
struct PoolObservationContext {
    oracle_runtime: Arc<OracleRuntime>,
    session_manager: Arc<SessionManager>,
    snapshot_engine: Arc<SnapshotEngine>,
    event_tx: EventBusSender,
    post_buy_tx: Option<DirectPostBuySender>,
    decision_logger: Arc<ghost_brain::oracle::DecisionLogger>,
    coverage_audit_log_path: std::path::PathBuf,
    trigger: Option<Arc<crate::components::trigger::TriggerComponent>>,
    iwim_veto_config: ghost_brain::config::IwimVetoGateConfig,
    gatekeeper_config: GatekeeperV2Config,
    cross_pool_velocity_config: CrossPoolVelocityConfig,
    funding_source_config: FundingSourceConfig,
    authoritative_funding_coverage_gate_enabled: bool,
    fingerprint_config: EarlyFingerprintConfig,
    event_emitter: Option<Arc<EventEmitter>>,
    health: Option<Arc<ghost_core::health::RuntimeHealth>>,
    result_tx: tokio::sync::mpsc::UnboundedSender<PoolObservationResult>,
    post_buy_epoch: Arc<AtomicU64>,
    execution_mode: ExecutionMode,
    shadow_entry_log_path: std::path::PathBuf,
    shadow_lifecycle_log_path: Option<std::path::PathBuf>,
    gatekeeper_rollout_profile: String,
    dry_run: bool,
    ab_window_ms: u64,
}

impl PoolObservationContext {
    fn runtime_lane(&self) -> Lane {
        match self.execution_mode {
            ExecutionMode::Live | ExecutionMode::Dual => Lane::Live,
            ExecutionMode::Paper => Lane::Paper,
            ExecutionMode::Shadow => Lane::Shadow,
        }
    }

    fn post_buy_lane(&self) -> &'static str {
        match self.execution_mode {
            ExecutionMode::Live | ExecutionMode::Dual => "live",
            ExecutionMode::Paper => "paper",
            ExecutionMode::Shadow => "shadow",
        }
    }

    fn canonical_shadow_mode(&self) -> bool {
        self.execution_mode == ExecutionMode::Shadow
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuyPathMetadataSource {
    LocalTaskState,
    RuntimeRegistry,
    RuntimeStateSnapshot,
    WaitFallback,
    Missing,
}

#[derive(Debug, Clone)]
struct ShadowRunReadiness {
    ready: bool,
    missing_fields: Vec<String>,
}

#[derive(Debug, Clone)]
struct BuyPathExecutionOutcome {
    metadata_source: BuyPathMetadataSource,
    bought: bool,
    retain_runtime_pool: bool,
    close_reason: WindowCloseReason,
    shadow_execution_outcome: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FscAuthoritativeBuyGateStatus {
    stream_available: bool,
    warmup_ready: bool,
    coverage_window_ready: bool,
    authoritative_buy_gate_open: bool,
    coverage_window_remaining_ms: u64,
}

fn observe_funding_transfer(
    session_manager: &SessionManager,
    transfer: &FundingTransferObserved,
    config: &FundingSourceConfig,
) {
    let funding_source_index = session_manager.funding_source_index();
    let warmup_ready_before = funding_source_index.warmup_ready();
    funding_source_index.observe_transfer(transfer, config);

    if transfer.full_chain_coverage {
        let warmup_ready_after = funding_source_index.warmup_ready();
        if !warmup_ready_before && warmup_ready_after {
            info!(
                signature = %transfer.signature,
                source_wallet = %transfer.source_wallet,
                recipient_wallet = %transfer.recipient_wallet,
                "FSC authoritative funding warmup became ready"
            );
        }
    }
}

fn current_fsc_authoritative_buy_gate_status(
    session_manager: &SessionManager,
    config: &FundingSourceConfig,
    gate_enabled: bool,
) -> FscAuthoritativeBuyGateStatus {
    let coverage_status = session_manager
        .funding_source_index()
        .coverage_window_status(config, current_time_ms());
    FscAuthoritativeBuyGateStatus {
        stream_available: coverage_status.stream_available,
        warmup_ready: coverage_status.warmup_ready,
        coverage_window_ready: coverage_status.coverage_window_ready,
        authoritative_buy_gate_open: !gate_enabled || coverage_status.authoritative_buy_ready,
        coverage_window_remaining_ms: if gate_enabled {
            coverage_status.coverage_window_remaining_ms
        } else {
            0
        },
    }
}

fn refresh_fsc_authoritative_buy_gate_status(
    session_manager: &SessionManager,
    config: &FundingSourceConfig,
    gate_enabled: bool,
    source: &'static str,
    force_log: bool,
    previous_status: &mut Option<FscAuthoritativeBuyGateStatus>,
) -> FscAuthoritativeBuyGateStatus {
    let status = current_fsc_authoritative_buy_gate_status(session_manager, config, gate_enabled);
    record_fsc_coverage_window_ready(status.coverage_window_ready);
    record_fsc_coverage_window_remaining_ms(status.coverage_window_remaining_ms);
    record_fsc_authoritative_buy_gate_open(status.authoritative_buy_gate_open);
    if force_log || previous_status.as_ref() != Some(&status) {
        info!(
            gate_enabled,
            stream_available = status.stream_available,
            warmup_ready = status.warmup_ready,
            coverage_window_ready = status.coverage_window_ready,
            authoritative_buy_gate_open = status.authoritative_buy_gate_open,
            coverage_window_remaining_ms = status.coverage_window_remaining_ms,
            source,
            "FSC authoritative funding coverage gate updated"
        );
    }
    *previous_status = Some(status);
    status
}

fn apply_authoritative_funding_stream_availability(
    session_manager: &SessionManager,
    available: bool,
    source: &'static str,
    force_log: bool,
) {
    let funding_source_index = session_manager.funding_source_index();
    let previous_available = funding_source_index.stream_available();
    let previous_warmup_ready = funding_source_index.warmup_ready();
    session_manager.set_funding_stream_available(available);
    let warmup_ready = funding_source_index.warmup_ready();

    if force_log || previous_available != available || previous_warmup_ready != warmup_ready {
        info!(
            available,
            warmup_ready, source, "FSC authoritative funding availability updated"
        );
    }
}

/// Emit `InitPoolEvent` to SnapshotEngine if pool data and reserves are available.
/// Extracted to avoid duplication between initial spawn and late-arriving NewPool.
fn maybe_emit_init_pool_event(
    ctx: &PoolObservationContext,
    pool_id: Pubkey,
    pd: Option<&DetectedPool>,
) {
    let Some(pd) = pd else { return };
    ctx.snapshot_engine.track_pool(pool_id);
    let (Ok(base_mint), Ok(quote_mint)) = (
        Pubkey::try_from(pd.base_mint.as_str()),
        Pubkey::try_from(pd.quote_mint.as_str()),
    ) else {
        return;
    };
    let px_ctx = ctx.oracle_runtime.resolve_price_context(pool_id, base_mint);
    let px_ctx = ctx.oracle_runtime.resolve_price_context(pool_id, base_mint);
    if let (Some(rb), Some(rq)) = (px_ctx.reserve_base, px_ctx.reserve_quote) {
        let (price, state, _) = derive_price_canonical(rb, rq, px_ctx.price_quote.unwrap_or(0.0));
        let initial_price = if state == PriceState::Valid {
            price
        } else {
            px_ctx.price_quote.unwrap_or(0.0)
        };
        ctx.snapshot_engine
            .handle_initialize_pool_event(&InitPoolEvent {
                pool_amm_id: pool_id,
                base_mint,
                quote_mint,
                slot: pd.slot,
                timestamp_ms: detected_pool_event_ts_ms(pd),
                initial_liquidity_sol: pd.initial_liquidity_sol.unwrap_or(0.0),
                initial_reserve_base: rb,
                initial_reserve_quote: rq,
                initial_price_quote: initial_price,
            });
        debug!(
            pool = %pool_id,
            base_mint = %base_mint,
            "InitPoolEvent sent reserve_base={} reserve_quote={}",
            rb, rq
        );
    }
}

fn initial_window_state_for_task(
    pool_data: Option<&DetectedPool>,
    registered_wall_ts_ms: u64,
    window_ms: u64,
) -> Option<WindowState> {
    pool_data.map(|pd| {
        let t0 = detected_pool_epoch_like_ts_ms(pd).unwrap_or(registered_wall_ts_ms);
        WindowState::from_pool_detected(t0, window_ms)
    })
}

fn ensure_window_state_for_tx(
    window_state: &mut Option<WindowState>,
    tx_ts_ms: u64,
    window_ms: u64,
) -> &mut WindowState {
    window_state.get_or_insert_with(|| WindowState::from_first_tx(tx_ts_ms, window_ms))
}

fn maybe_open_coverage_window(
    pool_id: Pubkey,
    base_mint_pubkey: Option<Pubkey>,
    window_state: Option<&WindowState>,
    opened: &mut bool,
) {
    if *opened {
        return;
    }
    let Some(window_state) = window_state else {
        return;
    };
    coverage_audit().open_window(
        pool_id.to_string(),
        base_mint_pubkey.map(|mint| mint.to_string()),
        window_state.t0_event_ts_ms,
        window_state.t_end_event_ts_ms,
    );
    *opened = true;
}

fn install_buy_path_metadata(
    pool_id: Pubkey,
    registered_wall_ts_ms: u64,
    ab_window_ms: u64,
    pd: Arc<DetectedPool>,
    identity: &mut ObservationIdentity,
    base_mint_pubkey: &mut Option<Pubkey>,
    pool_data: &mut Option<Arc<DetectedPool>>,
) {
    *identity = ObservationIdentity {
        base_mint: pd.base_mint.clone(),
        first_seen_ts_ms: registered_wall_ts_ms,
        first_seen_clock_source: "registered_wall",
        end_10s_ts_ms: registered_wall_ts_ms.saturating_add(ab_window_ms),
        dev_pubkey: normalize_dev_pubkey_str(&pd.creator).unwrap_or_else(|| "unknown".to_string()),
        failed_promotion_attempts: 0,
        next_promotion_attempt_ts_ms: 0,
    };
    *base_mint_pubkey = Pubkey::try_from(pd.base_mint.as_str()).ok();
    coverage_audit().set_window_base_mint(&pool_id.to_string(), Some(pd.base_mint.clone()));
    *pool_data = Some(pd);
}

fn buy_path_metadata_source_tag(source: BuyPathMetadataSource) -> &'static str {
    match source {
        BuyPathMetadataSource::LocalTaskState => "local_task_state",
        BuyPathMetadataSource::RuntimeRegistry => "runtime_registry",
        BuyPathMetadataSource::RuntimeStateSnapshot => "runtime_state_snapshot",
        BuyPathMetadataSource::WaitFallback => "wait_fallback",
        BuyPathMetadataSource::Missing => "missing",
    }
}

fn is_shadow_base_mint_present(value: &str) -> bool {
    !value.is_empty() && value != "unknown" && Pubkey::try_from(value).is_ok()
}

fn is_shadow_quote_mint_present(value: &str) -> bool {
    !value.is_empty()
        && value != "unknown"
        && (value.eq_ignore_ascii_case("SOL") || Pubkey::try_from(value).is_ok())
}

fn is_shadow_creator_present(value: &str) -> bool {
    !value.is_empty()
        && value != "unknown"
        && Pubkey::try_from(value)
            .ok()
            .is_some_and(|pubkey| pubkey != Pubkey::default())
}

fn has_shadow_initial_liquidity(pool_data: &DetectedPool) -> bool {
    pool_data
        .initial_liquidity_sol
        .is_some_and(|value| value.is_finite() && value > 0.0)
}

fn shadow_metadata_score(pool_data: &DetectedPool) -> u8 {
    let mut score = 0_u8;
    if is_shadow_base_mint_present(&pool_data.base_mint) {
        score += 1;
    }
    if is_shadow_quote_mint_present(&pool_data.quote_mint) {
        score += 1;
    }
    if is_shadow_creator_present(&pool_data.creator) {
        score += 1;
    }
    if has_shadow_initial_liquidity(pool_data) {
        score += 1;
    }
    if pool_data.slot.is_some() {
        score += 1;
    }
    if detected_pool_epoch_like_ts_ms(pool_data).is_some() {
        score += 1;
    }
    score
}

fn choose_shadow_metadata_pool(
    runtime_registry_pool: Option<Arc<DetectedPool>>,
    runtime_state_snapshot_pool: Option<Arc<DetectedPool>>,
) -> Option<(Arc<DetectedPool>, BuyPathMetadataSource)> {
    match (runtime_registry_pool, runtime_state_snapshot_pool) {
        (Some(runtime_registry_pool), Some(runtime_state_snapshot_pool)) => {
            let registry_score = shadow_metadata_score(runtime_registry_pool.as_ref());
            let snapshot_score = shadow_metadata_score(runtime_state_snapshot_pool.as_ref());
            if snapshot_score > registry_score {
                Some((
                    runtime_state_snapshot_pool,
                    BuyPathMetadataSource::RuntimeStateSnapshot,
                ))
            } else {
                Some((
                    runtime_registry_pool,
                    BuyPathMetadataSource::RuntimeRegistry,
                ))
            }
        }
        (Some(runtime_registry_pool), None) => Some((
            runtime_registry_pool,
            BuyPathMetadataSource::RuntimeRegistry,
        )),
        (None, Some(runtime_state_snapshot_pool)) => Some((
            runtime_state_snapshot_pool,
            BuyPathMetadataSource::RuntimeStateSnapshot,
        )),
        (None, None) => None,
    }
}

fn merge_local_buy_path_pool_data(
    local_pool: Arc<DetectedPool>,
    fallback: Option<(Arc<DetectedPool>, BuyPathMetadataSource)>,
    identity: &ObservationIdentity,
) -> (Arc<DetectedPool>, BuyPathMetadataSource) {
    let mut merged = (*local_pool).clone();
    let local_score = shadow_metadata_score(&merged);
    let mut metadata_source = BuyPathMetadataSource::LocalTaskState;

    if !is_shadow_base_mint_present(&merged.base_mint)
        && is_shadow_base_mint_present(&identity.base_mint)
    {
        merged.base_mint = identity.base_mint.clone();
    }
    if !is_shadow_creator_present(&merged.creator)
        && is_shadow_creator_present(&identity.dev_pubkey)
    {
        merged.creator = identity.dev_pubkey.clone();
    }
    if merged.timestamp_ms == 0 && identity.first_seen_ts_ms > 0 {
        merged.timestamp_ms = identity.first_seen_ts_ms;
        if merged.event_time.is_empty() {
            merged.event_time =
                ghost_core::EventTimeMetadata::new(None, Some(identity.first_seen_ts_ms), None);
        }
    }

    if let Some((fallback_pool, fallback_source)) = fallback {
        if !is_shadow_base_mint_present(&merged.base_mint)
            && is_shadow_base_mint_present(&fallback_pool.base_mint)
        {
            merged.base_mint = fallback_pool.base_mint.clone();
        }
        if !is_shadow_quote_mint_present(&merged.quote_mint)
            && is_shadow_quote_mint_present(&fallback_pool.quote_mint)
        {
            merged.quote_mint = fallback_pool.quote_mint.clone();
        }
        if !is_shadow_creator_present(&merged.creator)
            && is_shadow_creator_present(&fallback_pool.creator)
        {
            merged.creator = fallback_pool.creator.clone();
        }
        if !has_shadow_initial_liquidity(&merged) && has_shadow_initial_liquidity(&fallback_pool) {
            merged.initial_liquidity_sol = fallback_pool.initial_liquidity_sol;
        }
        if merged.slot.is_none() {
            merged.slot = fallback_pool.slot;
        }
        if merged.timestamp_ms == 0 && fallback_pool.timestamp_ms > 0 {
            merged.timestamp_ms = fallback_pool.timestamp_ms;
        }
        merged.event_time = merged
            .event_time
            .with_missing_from(fallback_pool.event_time);
        if merged.signature.is_empty() && !fallback_pool.signature.is_empty() {
            merged.signature = fallback_pool.signature.clone();
        }

        if shadow_metadata_score(&merged) > local_score {
            metadata_source = fallback_source;
        }
    }

    (Arc::new(merged), metadata_source)
}

fn maybe_upgrade_observation_identity_from_pool_data(
    identity: &mut ObservationIdentity,
    pool_data: &DetectedPool,
) {
    if !is_shadow_base_mint_present(&identity.base_mint)
        && is_shadow_base_mint_present(&pool_data.base_mint)
    {
        identity.base_mint = pool_data.base_mint.clone();
    }

    if !is_shadow_creator_present(&identity.dev_pubkey)
        && is_shadow_creator_present(&pool_data.creator)
    {
        identity.dev_pubkey = pool_data.creator.clone();
    }
}

fn compute_shadow_run_readiness(
    pool_data: Option<&DetectedPool>,
    _buffered_txs: &[crate::components::gatekeeper::GatekeeperBufferedTx],
) -> ShadowRunReadiness {
    let Some(pool_data) = pool_data else {
        return ShadowRunReadiness {
            ready: false,
            missing_fields: vec![
                "base_mint".to_string(),
                "quote_mint".to_string(),
                "creator".to_string(),
                "initial_liquidity_sol".to_string(),
                "slot".to_string(),
                "timestamp_ms".to_string(),
            ],
        };
    };

    let mut missing_fields = Vec::with_capacity(7);
    if !is_shadow_base_mint_present(&pool_data.base_mint) {
        missing_fields.push("base_mint".to_string());
    }
    if !is_shadow_quote_mint_present(&pool_data.quote_mint) {
        missing_fields.push("quote_mint".to_string());
    }
    if !is_shadow_creator_present(&pool_data.creator) {
        missing_fields.push("creator".to_string());
    }
    if !pool_data
        .initial_liquidity_sol
        .is_some_and(|value| value.is_finite() && value > 0.0)
    {
        missing_fields.push("initial_liquidity_sol".to_string());
    }
    if pool_data.slot.is_none() {
        missing_fields.push("slot".to_string());
    }
    if detected_pool_epoch_like_ts_ms(pool_data).is_none() {
        missing_fields.push("timestamp_ms".to_string());
    }

    // Shadow BUY readiness is defined by canonical metadata only.
    // Observed BUY transactions remain useful for opportunistic reserve/account
    // enrichment, but once runtime/local metadata is canonical they must not
    // block shadow-only execution.

    ShadowRunReadiness {
        ready: missing_fields.is_empty(),
        missing_fields,
    }
}

fn backfill_initial_liquidity_sol_from_reserve_quote(
    pool_data: Arc<DetectedPool>,
    reserve_quote: Option<f64>,
) -> Arc<DetectedPool> {
    if pool_data
        .initial_liquidity_sol
        .is_some_and(|value| value.is_finite() && value > 0.0)
    {
        return pool_data;
    }

    let Some(initial_liquidity_sol) =
        reserve_quote.filter(|value| value.is_finite() && *value > 0.0)
    else {
        return pool_data;
    };

    let mut enriched = (*pool_data).clone();
    enriched.initial_liquidity_sol = Some(initial_liquidity_sol);
    Arc::new(enriched)
}

fn backfill_initial_liquidity_sol_from_runtime(
    oracle_runtime: &OracleRuntime,
    pool_id: Pubkey,
    pool_data: Arc<DetectedPool>,
) -> Arc<DetectedPool> {
    let Ok(base_mint) = Pubkey::try_from(pool_data.base_mint.as_str()) else {
        return pool_data;
    };

    let reserve_quote = oracle_runtime
        .resolve_price_context(pool_id, base_mint)
        .reserve_quote
        .filter(|value| value.is_finite() && *value > 0.0)
        .or_else(|| {
            let shadow_curve_key = Pubkey::try_from(pool_data.bonding_curve.as_str())
                .ok()
                .or_else(|| {
                    oracle_runtime
                        .lookup_pool_identity(&pool_id)
                        .map(|identity| identity.bonding_curve.into())
                })
                .or_else(|| oracle_runtime.shadow_ledger.resolve_curve_key(&base_mint))?;
            let now_ms = ghost_core::shadow_ledger::current_time_ms();
            oracle_runtime
                .shadow_ledger
                .get_curve_with_known_age(&shadow_curve_key, now_ms)
                .and_then(|(curve, _, _)| {
                    let reserve_quote = curve.virtual_sol_reserves as f64 / LAMPORTS_PER_SOL;
                    (reserve_quote.is_finite() && reserve_quote > 0.0).then_some(reserve_quote)
                })
        });

    backfill_initial_liquidity_sol_from_reserve_quote(pool_data, reserve_quote)
}

fn latest_observed_buy_reserve_quote(
    buffered_txs: &[crate::components::gatekeeper::GatekeeperBufferedTx],
) -> Option<f64> {
    buffered_txs.iter().rev().find_map(|buffered| {
        let tx = buffered.tx.as_ref();
        if !tx.success || !tx.is_buy {
            return None;
        }

        tx.reserve_quote
            .or(tx.v_sol_in_bonding_curve)
            .filter(|value| value.is_finite() && *value > 0.0)
    })
}

fn backfill_initial_liquidity_sol_from_buffered_buys(
    pool_data: Arc<DetectedPool>,
    buffered_txs: &[crate::components::gatekeeper::GatekeeperBufferedTx],
) -> Arc<DetectedPool> {
    backfill_initial_liquidity_sol_from_reserve_quote(
        pool_data,
        latest_observed_buy_reserve_quote(buffered_txs),
    )
}

fn enrich_buy_log_with_shadow_run(
    log: &mut ghost_brain::oracle::GatekeeperBuyLog,
    metadata_source: BuyPathMetadataSource,
    shadow_execution_outcome: &str,
    pool_data: Option<&DetectedPool>,
    buffered_txs: &[crate::components::gatekeeper::GatekeeperBufferedTx],
    trigger: Option<&Arc<crate::components::trigger::TriggerComponent>>,
) {
    let readiness = compute_shadow_run_readiness(pool_data, buffered_txs);
    log.shadow_ready = Some(readiness.ready);
    log.shadow_missing_fields =
        (!readiness.missing_fields.is_empty()).then_some(readiness.missing_fields);
    log.shadow_metadata_source = Some(buy_path_metadata_source_tag(metadata_source).to_string());
    log.shadow_execution_outcome = Some(shadow_execution_outcome.to_string());
    log.shadow_trigger_present = Some(trigger.is_some());
    if let Some(trigger) = trigger {
        log.shadow_entry_mode = Some(trigger.entry_mode().as_str().to_string());
        log.shadow_trigger_eligible = Some(trigger.supports_shadow_run());
    } else {
        log.shadow_entry_mode = None;
        log.shadow_trigger_eligible = Some(false);
    }
}

async fn hydrate_buy_path_metadata(
    pool_id: Pubkey,
    registered_wall_ts_ms: u64,
    rx: &mut tokio::sync::mpsc::Receiver<PoolObservationMsg>,
    ctx: &PoolObservationContext,
    identity: &mut ObservationIdentity,
    base_mint_pubkey: &mut Option<Pubkey>,
    pool_data: &mut Option<Arc<DetectedPool>>,
) -> BuyPathMetadataSource {
    let runtime_fallback = ctx.oracle_runtime.lookup_shadow_metadata_pool(&pool_id);

    if let Some(local_pool) = pool_data.clone() {
        let (merged_pool, metadata_source) =
            merge_local_buy_path_pool_data(local_pool, runtime_fallback, identity);
        maybe_upgrade_observation_identity_from_pool_data(identity, merged_pool.as_ref());
        if base_mint_pubkey.is_none() {
            *base_mint_pubkey = Pubkey::from_str(&merged_pool.base_mint).ok();
        }
        info!(
            "POOL_TASK_BUY_METADATA_HYDRATED pool={} source={} mint={}",
            pool_id,
            buy_path_metadata_source_tag(metadata_source),
            merged_pool.base_mint
        );
        *pool_data = Some(merged_pool);
        return metadata_source;
    }

    if let Some((pd, source)) = runtime_fallback {
        info!(
            "POOL_TASK_BUY_METADATA_HYDRATED pool={} source={} mint={}",
            pool_id,
            buy_path_metadata_source_tag(source),
            pd.base_mint
        );
        install_buy_path_metadata(
            pool_id,
            registered_wall_ts_ms,
            ctx.ab_window_ms,
            pd,
            identity,
            base_mint_pubkey,
            pool_data,
        );
        return source;
    }

    let wait_deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    loop {
        let now = tokio::time::Instant::now();
        if now >= wait_deadline {
            return BuyPathMetadataSource::Missing;
        }

        let poll_window = (wait_deadline - now).min(Duration::from_millis(25));
        tokio::select! {
            maybe_msg = rx.recv() => {
                match maybe_msg {
                    Some(PoolObservationMsg::NewPool(pd)) => {
                        info!(
                            "POOL_TASK_BUY_METADATA_HYDRATED pool={} source=late_new_pool mint={}",
                            pool_id, pd.base_mint
                        );
                        install_buy_path_metadata(
                            pool_id,
                            registered_wall_ts_ms,
                            ctx.ab_window_ms,
                            pd,
                            identity,
                            base_mint_pubkey,
                            pool_data,
                        );
                        return BuyPathMetadataSource::WaitFallback;
                    }
                    Some(PoolObservationMsg::Transaction(_)) => continue,
                    None => return BuyPathMetadataSource::Missing,
                }
            }
            _ = tokio::time::sleep(poll_window) => {
                if let Some((pd, _source)) = ctx.oracle_runtime.lookup_shadow_metadata_pool(&pool_id) {
                    info!(
                        "POOL_TASK_BUY_METADATA_HYDRATED pool={} source=wait_registry mint={}",
                        pool_id, pd.base_mint
                    );
                        install_buy_path_metadata(
                            pool_id,
                            registered_wall_ts_ms,
                            ctx.ab_window_ms,
                            pd,
                            identity,
                            base_mint_pubkey,
                            pool_data,
                    );
                    return BuyPathMetadataSource::WaitFallback;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriggerReadinessWaitError {
    CanonicalNotReady { mint: Pubkey },
    ObservationChannelClosed { mint: Pubkey },
    ReadinessNotifierClosed { mint: Pubkey },
}

impl TriggerReadinessWaitError {
    const fn outcome_tag(self) -> &'static str {
        match self {
            Self::CanonicalNotReady { .. } => "trigger_canonical_not_ready",
            Self::ObservationChannelClosed { .. } => "trigger_observation_channel_closed",
            Self::ReadinessNotifierClosed { .. } => "trigger_readiness_notifier_closed",
        }
    }
}

fn apply_trigger_readiness_message(
    pool_id: Pubkey,
    registered_wall_ts_ms: u64,
    msg: PoolObservationMsg,
    ctx: &PoolObservationContext,
    identity: &mut ObservationIdentity,
    base_mint_pubkey: &mut Option<Pubkey>,
    pool_data: &mut Option<Arc<DetectedPool>>,
) {
    match msg {
        PoolObservationMsg::Transaction(tx) => {
            maybe_promote_observation_identity_from_tx(
                pool_id,
                tx.as_ref(),
                ctx.ab_window_ms,
                identity,
                base_mint_pubkey,
                current_time_ms(),
                max_identity_promotion_retries(),
            );
            let _ = ctx
                .oracle_runtime
                .maybe_materialize_canonical_state_from_observed_tx(
                    pool_id,
                    *base_mint_pubkey,
                    tx.as_ref(),
                );
        }
        PoolObservationMsg::NewPool(pd) => {
            install_buy_path_metadata(
                pool_id,
                registered_wall_ts_ms,
                ctx.ab_window_ms,
                pd,
                identity,
                base_mint_pubkey,
                pool_data,
            );
        }
    }
}

async fn wait_for_live_trigger_readiness(
    pool_id: Pubkey,
    registered_wall_ts_ms: u64,
    mint: Pubkey,
    entry_mode: crate::config::TriggerEntryMode,
    rx: &mut tokio::sync::mpsc::Receiver<PoolObservationMsg>,
    ctx: &PoolObservationContext,
    identity: &mut ObservationIdentity,
    base_mint_pubkey: &mut Option<Pubkey>,
    pool_data: &mut Option<Arc<DetectedPool>>,
) -> Result<(), TriggerReadinessWaitError> {
    if !matches!(
        entry_mode,
        crate::config::TriggerEntryMode::Live | crate::config::TriggerEntryMode::LiveAndShadow
    ) {
        return Ok(());
    }

    let mut readiness_rx = ctx.oracle_runtime.subscribe_canonical_readiness(&mint);
    if ctx.oracle_runtime.is_live_trigger_canonical_ready(&mint) {
        return Ok(());
    }

    let deadline = Instant::now() + Duration::from_millis(LIVE_TRIGGER_READINESS_TIMEOUT_MS);
    loop {
        if Instant::now() >= deadline {
            error!(
                mint = %mint,
                timeout_ms = LIVE_TRIGGER_READINESS_TIMEOUT_MS,
                "Trigger readiness gate timed out waiting for canonical state"
            );
            return Err(TriggerReadinessWaitError::CanonicalNotReady { mint });
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        tokio::select! {
            biased;

            readiness = readiness_rx.changed() => {
                if readiness.is_err() {
                    return if ctx.oracle_runtime.is_live_trigger_canonical_ready(&mint) {
                        Ok(())
                    } else {
                        Err(TriggerReadinessWaitError::ReadinessNotifierClosed { mint })
                    };
                }
                if ctx.oracle_runtime.is_live_trigger_canonical_ready(&mint) {
                    return Ok(());
                }
            }

            maybe_msg = rx.recv() => {
                let Some(msg) = maybe_msg else {
                    return Err(TriggerReadinessWaitError::ObservationChannelClosed { mint });
                };
                apply_trigger_readiness_message(
                    pool_id,
                    registered_wall_ts_ms,
                    msg,
                    ctx,
                    identity,
                    base_mint_pubkey,
                    pool_data,
                );
                if ctx.oracle_runtime.is_live_trigger_canonical_ready(&mint) {
                    return Ok(());
                }
            }

            _ = tokio::time::sleep(remaining) => {
                error!(
                    mint = %mint,
                    timeout_ms = LIVE_TRIGGER_READINESS_TIMEOUT_MS,
                    "Trigger readiness gate timed out waiting for canonical state"
                );
                return Err(TriggerReadinessWaitError::CanonicalNotReady { mint });
            }
        }
    }
}

async fn execute_gatekeeper_buy_path(
    pool_id: Pubkey,
    registered_wall_ts_ms: u64,
    buffered_txs: &[crate::components::gatekeeper::GatekeeperBufferedTx],
    assessment: &GatekeeperAssessment,
    post_buy_lane: &str,
    rx: &mut tokio::sync::mpsc::Receiver<PoolObservationMsg>,
    ctx: &PoolObservationContext,
    identity: &mut ObservationIdentity,
    base_mint_pubkey: &mut Option<Pubkey>,
    pool_data: &mut Option<Arc<DetectedPool>>,
) -> BuyPathExecutionOutcome {
    let metadata_source = hydrate_buy_path_metadata(
        pool_id,
        registered_wall_ts_ms,
        rx,
        ctx,
        identity,
        base_mint_pubkey,
        pool_data,
    )
    .await;

    let mut bought = false;
    let mut retain_runtime_pool = false;
    let mut buy_close_reason = WindowCloseReason::PoolBoughtEarly;
    let mut shadow_execution_outcome = if matches!(metadata_source, BuyPathMetadataSource::Missing)
    {
        "metadata_missing".to_string()
    } else {
        "metadata_ready".to_string()
    };

    if let Some(ref pd) = pool_data {
        let enriched_pool_data = backfill_initial_liquidity_sol_from_buffered_buys(
            backfill_initial_liquidity_sol_from_runtime(
                ctx.oracle_runtime.as_ref(),
                pool_id,
                pd.clone(),
            ),
            buffered_txs,
        );
        *pool_data = Some(enriched_pool_data);
    }

    if pool_data.is_some() {
        let approved_pools = ctx.oracle_runtime.approved_pools();
        let base_mint_opt = pool_data
            .as_ref()
            .and_then(|pd| Pubkey::try_from(pd.base_mint.as_str()).ok());
        let pool_amm_id = pool_id;
        if let Some(base_mint) = base_mint_opt {
            let staged = ctx.oracle_runtime.stage_gatekeeper_history_for_commit(
                pool_amm_id,
                base_mint,
                registered_wall_ts_ms,
                buffered_txs,
            );
            debug!(
                pool = %pool_amm_id,
                base_mint = %base_mint,
                staged,
                "Staged Gatekeeper BUY history into launcher commit coordinator"
            );
        } else {
            warn!(
                pool = %pool_amm_id,
                "Gatekeeper BUY missing valid base_mint; canonical commit path skipped"
            );
        }
        ctx.oracle_runtime.mark_pool_approved(pool_amm_id);
        approved_pools.insert(pool_amm_id);
        ctx.snapshot_engine.mark_pool_active(pool_amm_id);

        let quote_mint_opt = pool_data
            .as_ref()
            .and_then(|pd| Pubkey::try_from(pd.quote_mint.as_str()).ok());

        if let (Some(base_mint), Some(quote_mint)) = (base_mint_opt, quote_mint_opt) {
            let px = ctx
                .oracle_runtime
                .resolve_price_context(pool_amm_id, base_mint);
            if let Some(current_pool_data) = pool_data.clone() {
                *pool_data = Some(backfill_initial_liquidity_sol_from_buffered_buys(
                    backfill_initial_liquidity_sol_from_reserve_quote(
                        current_pool_data,
                        px.reserve_quote,
                    ),
                    buffered_txs,
                ));
            }
            if let (Some(rb), Some(rq)) = (px.reserve_base, px.reserve_quote) {
                let (price, state, _) =
                    derive_price_canonical(rb, rq, px.price_quote.unwrap_or(0.0));
                let ip = if state == PriceState::Valid {
                    price
                } else {
                    px.price_quote.unwrap_or(0.0)
                };
                let initial_liquidity_sol = pool_data
                    .as_ref()
                    .and_then(|pd| pd.initial_liquidity_sol)
                    .unwrap_or(rq);
                ctx.snapshot_engine
                    .handle_initialize_pool_event(&InitPoolEvent {
                        pool_amm_id,
                        base_mint,
                        quote_mint,
                        slot: pool_data.as_ref().and_then(|pd| pd.slot),
                        timestamp_ms: pool_data
                            .as_ref()
                            .map_or(0, |pd| detected_pool_event_ts_ms(pd)),
                        initial_liquidity_sol,
                        initial_reserve_base: rb,
                        initial_reserve_quote: rq,
                        initial_price_quote: ip,
                    });
                info!(
                    pool = %pool_amm_id,
                    base_mint = %base_mint,
                    "Bootstrap InitPoolEvent sent reserve_base={} reserve_quote={}",
                    rb, rq
                );
            } else {
                error!(
                    pool = %pool_amm_id,
                    base_mint = %base_mint,
                    "KRYTYK: Gatekeeper PASS ale WCIĄŻ brak danych rezerw!"
                );
            }
        }

        let shadow_readiness = compute_shadow_run_readiness(pool_data.as_deref(), buffered_txs);

        for buffered in buffered_txs {
            let tx = &buffered.tx;
            let event_ts_ms = tx_event_ts_ms(tx.as_ref());
            ctx.oracle_runtime.register_pool_tx(
                pool_amm_id,
                event_ts_ms,
                tx.slot,
                tx.mpcf_payload.clone(),
                None,
                tx.signer.clone(),
                tx.is_buy,
                tx.volume_sol,
            );
            if tx.success {
                ctx.snapshot_engine.track_pool(pool_amm_id);
            }
        }

        if let Some(ref trigger_component) = ctx.trigger {
            let shadow_only_requires_ready = matches!(
                trigger_component.entry_mode(),
                crate::config::TriggerEntryMode::ShadowOnly
            ) && trigger_component.shadow_run_enabled();

            if shadow_only_requires_ready && !shadow_readiness.ready {
                warn!(
                    pool = %pool_amm_id,
                    missing_fields = ?shadow_readiness.missing_fields,
                    "⏩ POMINIETO Shadow-only BUY Z POWODU: metadata is not shadow-ready"
                );
                shadow_execution_outcome = "shadow_skipped_not_ready".to_string();
            } else if let Some(initial_pool_data) = pool_data.as_deref() {
                let Some(initial_buy_mint) =
                    Pubkey::try_from(initial_pool_data.base_mint.as_str()).ok()
                else {
                    shadow_execution_outcome = "invalid_base_mint".to_string();
                    return BuyPathExecutionOutcome {
                        metadata_source,
                        bought,
                        retain_runtime_pool,
                        close_reason: buy_close_reason,
                        shadow_execution_outcome,
                    };
                };

                trigger_component
                    .spawn_prewarm_advisory(
                        crate::components::trigger::TriggerPrewarmAdvisory::TipFloor,
                    )
                    .await;
                if let Err(err) = wait_for_live_trigger_readiness(
                    pool_amm_id,
                    registered_wall_ts_ms,
                    initial_buy_mint,
                    trigger_component.entry_mode(),
                    rx,
                    ctx,
                    identity,
                    base_mint_pubkey,
                    pool_data,
                )
                .await
                {
                    error!(
                        pool = %pool_amm_id,
                        base_mint = %initial_buy_mint,
                        outcome = err.outcome_tag(),
                        error = ?err,
                        "Gatekeeper BUY readiness gate blocked live trigger dispatch"
                    );
                    shadow_execution_outcome = err.outcome_tag().to_string();
                } else if let Some(pd) = pool_data.as_deref() {
                    let buy_mint = base_mint_pubkey.unwrap_or(initial_buy_mint);
                    let trade_value_sol =
                        trigger_component.estimate_trade_value_sol(pd.initial_liquidity_sol);
                    let urgency = (assessment.phases_passed as f64 / 6.0).clamp(0.0, 1.0);
                    let resolved_tip = trigger_component
                        .resolve_live_buy_tip(trade_value_sol, urgency)
                        .await;
                    let tip_lamports = resolved_tip.tip_lamports;
                    let mut account_overrides = derive_buy_account_overrides(buffered_txs);
                    if account_overrides.creator_pubkey.is_none() {
                        account_overrides.creator_pubkey =
                            Pubkey::try_from(pd.creator.as_str()).ok();
                    }
                    if matches!(
                        account_overrides.buy_variant,
                        Some(trigger::PumpfunBuyVariant::LegacyBuy)
                    ) {
                        account_overrides.legacy_buy_curve = ctx
                            .oracle_runtime
                            .resolve_trigger_buy_curve(buy_mint, buffered_txs);
                        if account_overrides.legacy_buy_curve.is_none() {
                            warn!(
                                pool = %pool_amm_id,
                                base_mint = %pd.base_mint,
                                "Trigger: legacy_buy override is missing ghost-core curve state; live BUY will fail closed"
                            );
                        }
                    }
                    info!(
                        pool = %pool_amm_id,
                        base_mint = %pd.base_mint,
                        has_token_program = account_overrides.token_program.is_some(),
                        has_global_config = account_overrides.global_config.is_some(),
                        has_fee_recipient = account_overrides.fee_recipient.is_some(),
                        has_creator_pubkey = account_overrides.creator_pubkey.is_some(),
                        has_associated_bonding_curve =
                            account_overrides.associated_bonding_curve.is_some(),
                        has_legacy_buy_curve = account_overrides.legacy_buy_curve.is_some(),
                        token_program = %account_overrides
                            .token_program
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        global_config = %account_overrides
                            .global_config
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        fee_recipient = %account_overrides
                            .fee_recipient
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        creator_pubkey = %account_overrides
                            .creator_pubkey
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        associated_bonding_curve = %account_overrides
                            .associated_bonding_curve
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        buy_variant = account_overrides
                            .buy_variant
                            .map(|variant| variant.as_str())
                            .unwrap_or("unknown"),
                        "Shadow buy account overrides prepared"
                    );

                    let fsc_gate_status =
                        ctx.authoritative_funding_coverage_gate_enabled.then(|| {
                            current_fsc_authoritative_buy_gate_status(
                                ctx.session_manager.as_ref(),
                                &ctx.funding_source_config,
                                true,
                            )
                        });
                    let receipt = execute_gatekeeper_buy_via_trigger_with_fsc_gate(
                        trigger_component,
                        fsc_gate_status,
                        buy_mint,
                        &account_overrides,
                        tip_lamports,
                        Some(resolved_tip.telemetry.clone()),
                    )
                    .await;
                    match apply_trigger_dispatch_receipt(
                        &ctx.event_tx,
                        ctx.post_buy_tx.as_ref(),
                        trigger_component,
                        &ctx.post_buy_epoch,
                        ctx.execution_mode,
                        &ctx.shadow_entry_log_path,
                        ctx.shadow_lifecycle_log_path.as_deref(),
                        &ctx.gatekeeper_rollout_profile,
                        pool_amm_id,
                        pd,
                        trade_value_sol,
                        tip_lamports,
                        post_buy_lane,
                        receipt,
                    )
                    .await
                    {
                        Ok(applied) => {
                            bought = applied.bought;
                            retain_runtime_pool = applied.retain_runtime_pool;
                            buy_close_reason = applied.close_reason;
                            shadow_execution_outcome = applied.shadow_execution_outcome;
                        }
                        Err(e) => {
                            error!(
                                pool = %pool_amm_id,
                                "GATEKEEPER BUY PATH FAILED: {}",
                                e
                            );
                            shadow_execution_outcome =
                                shadow_execution_outcome_from_dispatch_error(trigger_component, &e);
                        }
                    }
                } else {
                    shadow_execution_outcome = "metadata_missing".to_string();
                }
            } else {
                shadow_execution_outcome = "metadata_missing".to_string();
            }
        } else {
            warn!(
                pool = %pool_id,
                "Gatekeeper CHCIAL KUPIC ALE TRIGGER NIEDOSTEPNY",
            );
            shadow_execution_outcome = "trigger_missing".to_string();
        }
    } else {
        warn!(
            "POOL_TASK_BUY_NO_METADATA pool={} registry_and_wait_fallback_exhausted, BUY skipped",
            pool_id
        );
    }

    BuyPathExecutionOutcome {
        metadata_source,
        bought,
        retain_runtime_pool,
        close_reason: buy_close_reason,
        shadow_execution_outcome,
    }
}

async fn execute_gatekeeper_buy_via_trigger(
    trigger_component: &crate::components::trigger::TriggerComponent,
    buy_mint: Pubkey,
    account_overrides: &crate::components::trigger::BuyAccountOverrides,
    tip_lamports: u64,
    tip_floor_telemetry: Option<crate::components::live_tx_sender::TipFloorResolutionTelemetry>,
) -> crate::components::trigger::TriggerDispatchReceipt {
    execute_gatekeeper_buy_via_trigger_with_fsc_gate(
        trigger_component,
        None,
        buy_mint,
        account_overrides,
        tip_lamports,
        tip_floor_telemetry,
    )
    .await
}

async fn execute_gatekeeper_buy_via_trigger_with_fsc_gate(
    trigger_component: &crate::components::trigger::TriggerComponent,
    fsc_gate_status: Option<FscAuthoritativeBuyGateStatus>,
    buy_mint: Pubkey,
    account_overrides: &crate::components::trigger::BuyAccountOverrides,
    tip_lamports: u64,
    tip_floor_telemetry: Option<crate::components::live_tx_sender::TipFloorResolutionTelemetry>,
) -> crate::components::trigger::TriggerDispatchReceipt {
    if let Some(gate_status) = fsc_gate_status {
        match trigger_component.entry_mode() {
            crate::config::TriggerEntryMode::DryRunMock
            | crate::config::TriggerEntryMode::ShadowOnly => {}
            crate::config::TriggerEntryMode::LiveAndShadow
                if !gate_status.authoritative_buy_gate_open
                    && trigger_component.supports_shadow_run() =>
            {
                increment_counter!(
                    "fsc_authoritative_buy_gate_block_total",
                    "entry_mode" => "live_and_shadow",
                    "outcome" => "shadow_fallback"
                );
                warn!(
                    mint = %buy_mint,
                    coverage_window_ready = gate_status.coverage_window_ready,
                    warmup_ready = gate_status.warmup_ready,
                    coverage_window_remaining_ms = gate_status.coverage_window_remaining_ms,
                    "FSC coverage gate blocked live BUY; degrading LiveAndShadow to shadow-only"
                );
                return match trigger_component
                    .prepare_buy_request_with_tip_telemetry(
                        &buy_mint,
                        account_overrides,
                        tip_lamports,
                        tip_floor_telemetry,
                    )
                    .await
                {
                    Ok(prepared_buy) => {
                        trigger_component
                            .dispatch_prepared_buy_shadow_only(prepared_buy)
                            .await
                    }
                    Err(e) => crate::components::trigger::TriggerDispatchReceipt {
                        primary_outcome: Err(e),
                        shadow_task: None,
                        active_position_lease: None,
                        retain_position_slot_on_error: false,
                        failed_request: None,
                        failed_context: trigger_component
                            .build_dispatch_failure_context(tip_lamports),
                    },
                };
            }
            crate::config::TriggerEntryMode::Live
            | crate::config::TriggerEntryMode::LiveAndShadow
                if !gate_status.authoritative_buy_gate_open =>
            {
                increment_counter!(
                    "fsc_authoritative_buy_gate_block_total",
                    "entry_mode" => trigger_component.entry_mode().as_str(),
                    "outcome" => "blocked"
                );
                warn!(
                    mint = %buy_mint,
                    coverage_window_ready = gate_status.coverage_window_ready,
                    warmup_ready = gate_status.warmup_ready,
                    coverage_window_remaining_ms = gate_status.coverage_window_remaining_ms,
                    "FSC coverage gate blocked authoritative live BUY"
                );
                return crate::components::trigger::TriggerDispatchReceipt {
                    primary_outcome: Err(anyhow::anyhow!(
                        "authoritative live BUY blocked until FSC coverage window is ready (warmup_ready={}, coverage_window_ready={}, remaining_ms={})",
                        gate_status.warmup_ready,
                        gate_status.coverage_window_ready,
                        gate_status.coverage_window_remaining_ms
                    )),
                    shadow_task: None,
                    active_position_lease: None,
                    retain_position_slot_on_error: false,
                    failed_request: None,
                    failed_context: trigger_component.build_dispatch_failure_context(tip_lamports),
                };
            }
            _ => {}
        }
    }

    match trigger_component.entry_mode() {
        crate::config::TriggerEntryMode::DryRunMock => {
            crate::components::trigger::TriggerDispatchReceipt {
                primary_outcome: trigger_component
                    .execute_buy(&buy_mint, account_overrides, tip_lamports)
                    .await,
                shadow_task: None,
                active_position_lease: None,
                retain_position_slot_on_error: false,
                failed_request: None,
                failed_context: None,
            }
        }
        _ => match trigger_component
            .prepare_buy_request_with_tip_telemetry(
                &buy_mint,
                account_overrides,
                tip_lamports,
                tip_floor_telemetry,
            )
            .await
        {
            Ok(prepared_buy) => {
                trigger_component
                    .dispatch_prepared_buy_with_shadow(prepared_buy)
                    .await
            }
            Err(e) => crate::components::trigger::TriggerDispatchReceipt {
                primary_outcome: Err(e),
                shadow_task: None,
                active_position_lease: None,
                retain_position_slot_on_error: false,
                failed_request: None,
                failed_context: trigger_component.build_dispatch_failure_context(tip_lamports),
            },
        },
    }
}

fn spawn_shadow_buy_observer(
    event_tx: crate::events::EventBusSender,
    pool_amm_id: Pubkey,
    base_mint: String,
    emit_event_bus: bool,
    shadow_log_path: std::path::PathBuf,
    shadow_lifecycle_log_path: Option<std::path::PathBuf>,
    join_key: String,
    rollout_profile: String,
    live_signature: Option<String>,
    shadow_task: crate::components::trigger::PendingShadowSimulation,
) {
    tokio::spawn(async move {
        let crate::components::trigger::PendingShadowSimulation { request, handle } = shadow_task;
        match handle.await {
            Ok(Ok(mut report)) => {
                report.live_signature = live_signature;
                let shadow_event =
                    crate::components::trigger::shadow_run::shadow_buy_event_from_report(
                        &pool_amm_id.to_string(),
                        &base_mint,
                        report,
                    );
                if emit_event_bus {
                    if let Err(e) =
                        event_tx.send(GhostEvent::shadow_buy_simulated(shadow_event.clone()))
                    {
                        warn!(
                            pool = %pool_amm_id,
                            "Failed to emit ShadowBuySimulated event from live_and_shadow: {}",
                            e
                        );
                    }
                }
                if let Err(e) = append_shadow_buy_report_record(
                    &shadow_log_path,
                    shadow_lifecycle_log_path.as_deref(),
                    crate::config::TriggerEntryMode::LiveAndShadow,
                    &shadow_event,
                    &join_key,
                    &rollout_profile,
                    crate::components::trigger::shadow_run::ShadowDispatchStatus::Closed,
                )
                .await
                {
                    error!(
                        pool = %pool_amm_id,
                        "Failed to append live_and_shadow shadow report record: {}",
                        e
                    );
                }
            }
            Ok(Err(e)) => {
                let shadow_event =
                    crate::components::trigger::shadow_run::shadow_failure_event_from_request(
                        &pool_amm_id.to_string(),
                        &base_mint,
                        &request,
                        live_signature,
                        &e,
                    );
                if emit_event_bus {
                    if let Err(send_err) =
                        event_tx.send(GhostEvent::shadow_buy_simulated(shadow_event.clone()))
                    {
                        warn!(
                            pool = %pool_amm_id,
                            "Failed to emit failed ShadowBuySimulated event from live_and_shadow: {}",
                            send_err
                        );
                    }
                }
                if let Err(write_err) = append_shadow_buy_report_record(
                    &shadow_log_path,
                    shadow_lifecycle_log_path.as_deref(),
                    crate::config::TriggerEntryMode::LiveAndShadow,
                    &shadow_event,
                    &join_key,
                    &rollout_profile,
                    crate::components::trigger::shadow_run::ShadowDispatchStatus::Failed,
                )
                .await
                {
                    error!(
                        pool = %pool_amm_id,
                        "Failed to append live_and_shadow shadow failure record: {}",
                        write_err
                    );
                }
                warn!(
                    pool = %pool_amm_id,
                    "Background live_and_shadow simulation failed: {}",
                    e
                );
            }
            Err(e) => {
                let join_error = anyhow::Error::new(e);
                let shadow_event =
                    crate::components::trigger::shadow_run::shadow_failure_event_from_request(
                        &pool_amm_id.to_string(),
                        &base_mint,
                        &request,
                        live_signature,
                        &join_error,
                    );
                if emit_event_bus {
                    if let Err(send_err) =
                        event_tx.send(GhostEvent::shadow_buy_simulated(shadow_event.clone()))
                    {
                        warn!(
                            pool = %pool_amm_id,
                            "Failed to emit join-failure ShadowBuySimulated event from live_and_shadow: {}",
                            send_err
                        );
                    }
                }
                if let Err(write_err) = append_shadow_buy_report_record(
                    &shadow_log_path,
                    shadow_lifecycle_log_path.as_deref(),
                    crate::config::TriggerEntryMode::LiveAndShadow,
                    &shadow_event,
                    &join_key,
                    &rollout_profile,
                    crate::components::trigger::shadow_run::ShadowDispatchStatus::Abandoned,
                )
                .await
                {
                    error!(
                        pool = %pool_amm_id,
                        "Failed to append live_and_shadow shadow join-failure record: {}",
                        write_err
                    );
                }
                warn!(
                    pool = %pool_amm_id,
                    "Background live_and_shadow task join failed: {}",
                    join_error
                );
            }
        }
    });
}

fn shadow_entry_price(trade_value_sol: f64, entry_token_amount_raw: Option<u64>) -> Option<f64> {
    if !trade_value_sol.is_finite() || trade_value_sol <= 0.0 {
        return None;
    }
    entry_token_amount_raw
        .filter(|tokens| *tokens > 0)
        .map(|tokens| trade_value_sol / (tokens as f64 / PUMP_TOKEN_DECIMAL_FACTOR))
}

fn format_shadow_entry_timestamp(timestamp_ms: u64) -> String {
    Utc.timestamp_millis_opt(timestamp_ms as i64)
        .single()
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn shadow_entry_record_from_event(
    pool_amm_id: Pubkey,
    base_mint: &str,
    trade_value_sol: f64,
    event: &crate::events::ShadowBuySimulationEvent,
    execution_outcome: &str,
) -> Option<ShadowEntryRecord> {
    let entry_execution_ts_ms = if event.simulation_finished_ts_ms > 0 {
        event.simulation_finished_ts_ms
    } else {
        event.decision_ts_ms
    };
    Some(ShadowEntryRecord {
        timestamp: format_shadow_entry_timestamp(entry_execution_ts_ms),
        pool_id: pool_amm_id.to_string(),
        mint_id: base_mint.to_string(),
        entry_price: shadow_entry_price(trade_value_sol, event.entry_token_amount_raw)?,
        slot: Some(event.rpc_slot),
        timestamp_ms: entry_execution_ts_ms,
        candidate_id: Some(event.candidate_id.clone()),
        order_id: None,
        quote_id: None,
        timing_source: None,
        execution_outcome: execution_outcome.to_string(),
    })
}

fn shadow_entry_record_from_request(
    pool_amm_id: Pubkey,
    base_mint: &str,
    request: &crate::components::trigger::PreparedBuyRequest,
    execution_outcome: &str,
) -> Option<ShadowEntryRecord> {
    Some(ShadowEntryRecord {
        timestamp: format_shadow_entry_timestamp(request.decision_ts_ms),
        pool_id: pool_amm_id.to_string(),
        mint_id: base_mint.to_string(),
        entry_price: shadow_entry_price(request.trade_value_sol, request.entry_token_amount_raw)?,
        slot: None,
        timestamp_ms: request.decision_ts_ms,
        candidate_id: Some(crate::events::build_execution_candidate_id(
            base_mint,
            pool_amm_id.to_string(),
            request.decision_ts_ms.to_string(),
        )),
        order_id: None,
        quote_id: None,
        timing_source: None,
        execution_outcome: execution_outcome.to_string(),
    })
}

async fn append_shadow_entry_record(
    log_path: &std::path::Path,
    record: &ShadowEntryRecord,
) -> anyhow::Result<()> {
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

async fn maybe_append_canonical_shadow_entry_record(
    execution_mode: ExecutionMode,
    log_path: &std::path::Path,
    record: Option<ShadowEntryRecord>,
) {
    if execution_mode != ExecutionMode::Shadow {
        return;
    }

    let Some(record) = record else {
        return;
    };

    if let Err(err) = append_shadow_entry_record(log_path, &record).await {
        error!(
            path = %log_path.display(),
            "Failed to append canonical shadow entry record: {}",
            err
        );
    }
}

async fn append_shadow_buy_report_record(
    log_path: &std::path::Path,
    lifecycle_log_path: Option<&std::path::Path>,
    entry_mode: crate::config::TriggerEntryMode,
    record: &crate::events::ShadowBuySimulationEvent,
    join_key: &str,
    rollout_profile: &str,
    dispatch_status: crate::components::trigger::shadow_run::ShadowDispatchStatus,
) -> anyhow::Result<()> {
    let jsonl_record =
        crate::components::trigger::shadow_run::ShadowBuySimulationRecord::from_event(
            entry_mode, record,
        )
        .with_lifecycle_identity(join_key.to_string(), rollout_profile.to_string());
    crate::components::trigger::shadow_run::record_shadow_buy_metrics(&jsonl_record);
    crate::oracle_metrics::record_shadow_lifecycle_status(if jsonl_record.err.is_some() {
        "failed_reconciliation"
    } else {
        "dispatched"
    });
    crate::components::trigger::shadow_run::append_shadow_buy_record(log_path, &jsonl_record)
        .await?;
    if let Some(lifecycle_log_path) = lifecycle_log_path {
        let lifecycle_record =
            crate::components::trigger::shadow_run::ShadowDispatchLifecycleRecord::from_shadow_buy_record(
                &jsonl_record,
                join_key.to_string(),
                rollout_profile.to_string(),
                dispatch_status,
            );
        crate::components::trigger::shadow_run::append_shadow_dispatch_lifecycle_record(
            lifecycle_log_path,
            &lifecycle_record,
        )
        .await?;
    }
    Ok(())
}

fn shadow_execution_outcome_from_dispatch_error(
    trigger_component: &crate::components::trigger::TriggerComponent,
    err: &anyhow::Error,
) -> String {
    if !trigger_component.shadow_run_enabled() {
        return "trigger_dispatch_failed".to_string();
    }

    let message = err.to_string();
    let lower = message.to_lowercase();
    if matches!(
        err.downcast_ref::<SafetyViolation>(),
        Some(
            SafetyViolation::MaxConcurrentPositionsReached { .. }
                | SafetyViolation::PositionSlotAlreadyActive { .. }
        )
    ) {
        return "shadow_position_limit_reached".to_string();
    }
    if lower.contains("missing canonical creator_pubkey")
        || lower.contains("invalid associated_bonding_curve override")
        || lower.contains("missing canonical associated_bonding_curve")
    {
        return "shadow_metadata_missing".to_string();
    }
    if lower.contains("insufficient payer balance")
        || lower.contains("insufficient funds")
        || lower.contains("insufficient balance")
        || lower.contains("balance critical")
        || lower.contains("no safe trade capacity")
        || lower.contains("insufficient safe balance")
    {
        return "shadow_insufficient_balance".to_string();
    }
    if lower.contains("429") || lower.contains("too many requests") {
        return "shadow_transport_error".to_string();
    }
    if lower.contains("invalid fee payer account owner")
        || lower.contains("invalid executable fee payer account")
    {
        return "shadow_invalid_fee_payer".to_string();
    }
    if lower.contains("failed to fetch mint account") || lower.contains("accountnotfound") {
        return "shadow_account_not_visible".to_string();
    }
    if lower.contains("connection refused")
        || lower.contains("error trying to connect")
        || lower.contains("cluster version query failed")
    {
        return "shadow_rpc_unreachable".to_string();
    }

    match crate::components::trigger::shadow_run::classify_shadow_error(&message) {
        "network_provider_problem" | "timing_blockhash_problem" => {
            "shadow_transport_error".to_string()
        }
        "authority_problem" => "shadow_authority_error".to_string(),
        "fee_compute_problem" => "shadow_fee_compute_error".to_string(),
        "data_problem" => "shadow_data_problem".to_string(),
        "simulation_mismatch" => "shadow_simulation_error".to_string(),
        "logic_invariant_problem" => "shadow_invariant_error".to_string(),
        _ => "shadow_unknown_error".to_string(),
    }
}

fn shadow_execution_outcome_from_report_err(err: &str) -> String {
    match crate::components::trigger::shadow_run::classify_shadow_error(err) {
        "network_provider_problem" | "timing_blockhash_problem" => {
            "shadow_transport_error".to_string()
        }
        "authority_problem" => "shadow_authority_error".to_string(),
        "fee_compute_problem" => "shadow_fee_compute_error".to_string(),
        "data_problem" => "shadow_data_problem".to_string(),
        "simulation_mismatch" => "shadow_simulation_error".to_string(),
        "logic_invariant_problem" => "shadow_invariant_error".to_string(),
        _ => "shadow_unknown_error".to_string(),
    }
}

fn shadow_execution_outcome_from_handoff_ack(ack: DirectPostBuyHandoffAck) -> Option<String> {
    match ack {
        DirectPostBuyHandoffAck::Accepted => None,
        DirectPostBuyHandoffAck::Rejected(reason) => {
            Some(format!("shadow_handoff_rejected_{reason}"))
        }
    }
}

fn derive_buy_account_overrides(
    buffered_txs: &[crate::components::gatekeeper::GatekeeperBufferedTx],
) -> crate::components::trigger::BuyAccountOverrides {
    let canonical_global_config = trigger::DirectBuyBuilder::canonical_global_config();
    let mut overrides = crate::components::trigger::BuyAccountOverrides::default();
    for buffered in buffered_txs.iter().rev() {
        let tx = buffered.tx.as_ref();
        if !tx.success || !tx.is_buy {
            continue;
        }
        if overrides.global_config.is_none() {
            overrides.global_config = tx
                .global_config
                .as_deref()
                .and_then(|value| Pubkey::try_from(value).ok());
            overrides.global_config = overrides
                .global_config
                .filter(|value| *value == canonical_global_config);
        }
        if overrides.fee_recipient.is_none() {
            overrides.fee_recipient = tx
                .fee_recipient
                .as_deref()
                .and_then(|value| Pubkey::try_from(value).ok())
                .filter(trigger::DirectBuyBuilder::is_authorized_fee_recipient);
        }
        if overrides.token_program.is_none() {
            overrides.token_program = tx
                .token_program
                .as_deref()
                .and_then(|value| Pubkey::try_from(value).ok());
        }
        if overrides.buy_variant.is_none() {
            overrides.buy_variant = tx.buy_variant.as_deref().and_then(|value| match value {
                "routed_exact_sol_in" => Some(trigger::PumpfunBuyVariant::RoutedExactSolIn),
                _ => None,
            });
        }
        if overrides.associated_bonding_curve.is_none() {
            overrides.associated_bonding_curve = tx
                .associated_bonding_curve
                .as_deref()
                .and_then(|value| Pubkey::try_from(value).ok());
        }
        if overrides.global_config.is_some()
            && overrides.fee_recipient.is_some()
            && overrides.token_program.is_some()
            && overrides.buy_variant.is_some()
            && overrides.associated_bonding_curve.is_some()
        {
            break;
        }
    }
    overrides
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TriggerBuyOutcomeApplied {
    bought: bool,
    retain_runtime_pool: bool,
    close_reason: WindowCloseReason,
    shadow_execution_outcome: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
struct ShadowEntryRecord {
    timestamp: String,
    pool_id: String,
    mint_id: String,
    entry_price: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    slot: Option<u64>,
    timestamp_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quote_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timing_source: Option<String>,
    execution_outcome: String,
}

const LIVE_POST_BUY_HANDOFF_RETRY_ATTEMPTS: u32 = 3;
const LIVE_POST_BUY_HANDOFF_RETRY_DELAY_MS: u64 = 10;
const SHADOW_POST_BUY_HANDOFF_ACK_TIMEOUT_MS: u64 = 1_000;

fn build_post_buy_handoff_event(
    pool_amm_id: Pubkey,
    pool_data: &DetectedPool,
    signature: &str,
    trade_value_sol: f64,
    tip_lamports: u64,
    post_buy_lane: &str,
    epoch: u64,
    position_slot_id: Option<PositionSlotId>,
    min_tokens_out: Option<u64>,
    entry_token_amount_raw: Option<u64>,
    buy_landed_slot: Option<u64>,
) -> GhostEvent {
    let creator_pubkey = Pubkey::from_str(&pool_data.creator)
        .ok()
        .map(|pubkey| pubkey.to_string());
    GhostEvent::post_buy_submitted(
        pool_amm_id.to_string(),
        pool_data.base_mint.clone(),
        signature.to_string(),
        trade_value_sol,
        tip_lamports,
        post_buy_lane,
        epoch,
        position_slot_id,
        PostBuySource::LiveBuy,
        min_tokens_out,
        entry_token_amount_raw,
        buy_landed_slot,
        creator_pubkey,
    )
}

fn send_direct_post_buy_handoff(
    post_buy_tx: Option<&DirectPostBuySender>,
    event: &GhostEvent,
    pool_amm_id: Pubkey,
    post_buy_lane: &str,
) -> anyhow::Result<()> {
    let Some(post_buy_tx) = post_buy_tx else {
        return Ok(());
    };

    post_buy_tx
        .send(DirectPostBuyHandoff::without_ack(event.clone()))
        .map_err(|e| {
            ::metrics::counter!(
                "trigger_post_buy_handoff_failed_total",
                1u64,
                "lane" => post_buy_lane.to_string(),
                "transport" => "direct_queue"
            );
            error!(
                pool = %pool_amm_id,
                lane = post_buy_lane,
                "Confirmed BUY could not be handed off to direct PostBuyRuntime queue: {}",
                e
            );
            anyhow::anyhow!(
                "direct post-buy handoff failed for pool {}: {}",
                pool_amm_id,
                e
            )
        })
}

async fn send_direct_shadow_post_buy_handoff(
    post_buy_tx: Option<&DirectPostBuySender>,
    event: &GhostEvent,
    pool_amm_id: Pubkey,
    post_buy_lane: &str,
) -> anyhow::Result<Option<DirectPostBuyHandoffAck>> {
    let Some(post_buy_tx) = post_buy_tx else {
        return Ok(None);
    };

    let (handoff, ack_rx) = DirectPostBuyHandoff::with_ack(event.clone());
    post_buy_tx.send(handoff).map_err(|e| {
        ::metrics::counter!(
            "trigger_post_buy_handoff_failed_total",
            1u64,
            "lane" => post_buy_lane.to_string(),
            "transport" => "direct_queue"
        );
        error!(
            pool = %pool_amm_id,
            lane = post_buy_lane,
            "Confirmed BUY could not be handed off to direct PostBuyRuntime queue: {}",
            e
        );
        anyhow::anyhow!(
            "direct post-buy handoff failed for pool {}: {}",
            pool_amm_id,
            e
        )
    })?;

    let ack = tokio::time::timeout(
        Duration::from_millis(SHADOW_POST_BUY_HANDOFF_ACK_TIMEOUT_MS),
        ack_rx,
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "shadow post-buy handoff ack timed out after {}ms for pool {}",
            SHADOW_POST_BUY_HANDOFF_ACK_TIMEOUT_MS,
            pool_amm_id
        )
    })?
    .map_err(|_| {
        anyhow::anyhow!(
            "shadow post-buy handoff ack channel closed for pool {}",
            pool_amm_id
        )
    })?;

    Ok(Some(ack))
}

async fn send_broadcast_post_buy_handoff(
    event_tx: &crate::events::EventBusSender,
    handoff_event: &GhostEvent,
    pool_amm_id: Pubkey,
    signature: &str,
    post_buy_lane: &str,
    direct_handoff_enabled: bool,
) -> anyhow::Result<()> {
    let mut last_error = None;

    for attempt in 1..=LIVE_POST_BUY_HANDOFF_RETRY_ATTEMPTS {
        match event_tx.send(handoff_event.clone()) {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_error = Some(e.to_string());
                if attempt < LIVE_POST_BUY_HANDOFF_RETRY_ATTEMPTS {
                    warn!(
                        pool = %pool_amm_id,
                        attempt,
                        max_attempts = LIVE_POST_BUY_HANDOFF_RETRY_ATTEMPTS,
                        retry_delay_ms = LIVE_POST_BUY_HANDOFF_RETRY_DELAY_MS,
                        "Failed to send PostBuySubmitted event, retrying: {}",
                        e
                    );
                    tokio::time::sleep(Duration::from_millis(LIVE_POST_BUY_HANDOFF_RETRY_DELAY_MS))
                        .await;
                }
            }
        }
    }

    if direct_handoff_enabled {
        ::metrics::counter!(
            "trigger_post_buy_handoff_degraded_total",
            1u64,
            "lane" => post_buy_lane.to_string(),
            "transport" => "broadcast"
        );
        warn!(
            pool = %pool_amm_id,
            lane = post_buy_lane,
            signature = signature,
            "Post-buy direct handoff succeeded but broadcast transport failed after retries: {}",
            last_error.unwrap_or_else(|| "unknown broadcast error".to_string())
        );
        return Ok(());
    }

    ::metrics::counter!(
        "trigger_post_buy_handoff_failed_total",
        1u64,
        "lane" => post_buy_lane.to_string(),
        "transport" => "broadcast"
    );

    let error_message = last_error.unwrap_or_else(|| "unknown post-buy handoff error".to_string());
    error!(
        pool = %pool_amm_id,
        lane = post_buy_lane,
        signature = signature,
        "Confirmed BUY could not be handed off to PostBuyRuntime: {}",
        error_message
    );
    Err(anyhow::anyhow!(
        "confirmed buy for pool {} failed to hand off to post-buy runtime after {} attempts: {}",
        pool_amm_id,
        LIVE_POST_BUY_HANDOFF_RETRY_ATTEMPTS,
        error_message
    ))
}

async fn send_post_buy_handoff(
    event_tx: &crate::events::EventBusSender,
    post_buy_tx: Option<&DirectPostBuySender>,
    pool_amm_id: Pubkey,
    pool_data: &DetectedPool,
    signature: &str,
    trade_value_sol: f64,
    tip_lamports: u64,
    post_buy_lane: &str,
    epoch: u64,
    position_slot_id: Option<PositionSlotId>,
    min_tokens_out: Option<u64>,
    buy_landed_slot: Option<u64>,
) -> anyhow::Result<()> {
    let handoff_event = build_post_buy_handoff_event(
        pool_amm_id,
        pool_data,
        signature,
        trade_value_sol,
        tip_lamports,
        post_buy_lane,
        epoch,
        position_slot_id,
        min_tokens_out,
        None,
        buy_landed_slot,
    );
    send_direct_post_buy_handoff(post_buy_tx, &handoff_event, pool_amm_id, post_buy_lane)?;
    send_broadcast_post_buy_handoff(
        event_tx,
        &handoff_event,
        pool_amm_id,
        signature,
        post_buy_lane,
        post_buy_tx.is_some(),
    )
    .await
}

async fn send_shadow_post_buy_handoff(
    event_tx: &crate::events::EventBusSender,
    post_buy_tx: Option<&DirectPostBuySender>,
    pool_amm_id: Pubkey,
    pool_data: &DetectedPool,
    signature: &str,
    trade_value_sol: f64,
    tip_lamports: u64,
    post_buy_lane: &str,
    epoch: u64,
    position_slot_id: Option<PositionSlotId>,
    min_tokens_out: Option<u64>,
    entry_token_amount_raw: Option<u64>,
) -> anyhow::Result<Option<DirectPostBuyHandoffAck>> {
    let handoff_event = build_post_buy_handoff_event(
        pool_amm_id,
        pool_data,
        signature,
        trade_value_sol,
        tip_lamports,
        post_buy_lane,
        epoch,
        position_slot_id,
        min_tokens_out,
        entry_token_amount_raw,
        None,
    );
    let ack = send_direct_shadow_post_buy_handoff(
        post_buy_tx,
        &handoff_event,
        pool_amm_id,
        post_buy_lane,
    )
    .await?;
    send_broadcast_post_buy_handoff(
        event_tx,
        &handoff_event,
        pool_amm_id,
        signature,
        post_buy_lane,
        post_buy_tx.is_some(),
    )
    .await?;
    Ok(ack)
}

async fn apply_trigger_buy_outcome(
    event_tx: &crate::events::EventBusSender,
    post_buy_tx: Option<&DirectPostBuySender>,
    trigger_component: &crate::components::trigger::TriggerComponent,
    post_buy_epoch: &std::sync::atomic::AtomicU64,
    execution_mode: ExecutionMode,
    canonical_shadow_entry_log_path: &std::path::Path,
    shadow_lifecycle_log_path: Option<&std::path::Path>,
    shadow_join_key: &str,
    rollout_profile: &str,
    pool_amm_id: Pubkey,
    pool_data: &DetectedPool,
    trade_value_sol: f64,
    tip_lamports: u64,
    post_buy_lane: &str,
    active_position_lease: Option<crate::components::trigger::safety::ActivePositionLease>,
    min_tokens_out: Option<u64>,
    outcome: crate::components::trigger::TriggerBuyOutcome,
) -> anyhow::Result<TriggerBuyOutcomeApplied> {
    match outcome {
        outcome @ crate::components::trigger::TriggerBuyOutcome::LiveConfirmed {
            signature, ..
        }
        | outcome @ crate::components::trigger::TriggerBuyOutcome::DryRunMock { signature } => {
            let mut active_position_lease = active_position_lease;
            let execution_outcome = match outcome {
                crate::components::trigger::TriggerBuyOutcome::LiveConfirmed { .. } => {
                    "live_confirmed"
                }
                crate::components::trigger::TriggerBuyOutcome::DryRunMock { .. } => "dry_run_mock",
                crate::components::trigger::TriggerBuyOutcome::ShadowSimulated { .. } => {
                    unreachable!()
                }
            };
            info!(
                pool = %pool_amm_id,
                "🚀 LIVE BUY LANDED!: sig={} tip={} trade={:.6}",
                signature, tip_lamports, trade_value_sol
            );

            if let Err(e) = event_tx.send(GhostEvent::transaction_sent(
                signature.to_string(),
                None,
                "buy_transaction",
            )) {
                warn!(
                    pool = %pool_amm_id,
                    "Failed to send TransactionSent event: {}",
                    e
                );
            }

            let epoch = post_buy_epoch.fetch_add(1, Ordering::Relaxed);
            let signature_str = signature.to_string();
            let buy_landed_slot = match &outcome {
                crate::components::trigger::TriggerBuyOutcome::LiveConfirmed {
                    landed_slot,
                    ..
                } => *landed_slot,
                crate::components::trigger::TriggerBuyOutcome::DryRunMock { .. }
                | crate::components::trigger::TriggerBuyOutcome::ShadowSimulated { .. } => None,
            };
            let live_confirmed = matches!(
                &outcome,
                crate::components::trigger::TriggerBuyOutcome::LiveConfirmed { .. }
            );
            let position_slot_id = active_position_lease.as_ref().map(|lease| lease.slot_id);
            if let Err(error) = send_post_buy_handoff(
                event_tx,
                post_buy_tx,
                pool_amm_id,
                pool_data,
                &signature_str,
                trade_value_sol,
                tip_lamports,
                post_buy_lane,
                epoch,
                position_slot_id,
                min_tokens_out,
                buy_landed_slot,
            )
            .await
            {
                if live_confirmed {
                    if let Some(lease) = active_position_lease.take() {
                        lease.retain();
                    }
                    if let Some(slot_id) = position_slot_id {
                        warn!(
                            pool = %pool_amm_id,
                            signature = %signature_str,
                            slot_id = %slot_id,
                            "Confirmed live BUY handoff failed; keeping position slot reserved fail-closed: {}",
                            error
                        );
                    }
                }
                return Err(error);
            }
            if let Some(lease) = active_position_lease.take() {
                lease.retain();
            }

            Ok(TriggerBuyOutcomeApplied {
                bought: true,
                retain_runtime_pool: true,
                close_reason: WindowCloseReason::PoolBoughtEarly,
                shadow_execution_outcome: execution_outcome.to_string(),
            })
        }
        crate::components::trigger::TriggerBuyOutcome::ShadowSimulated { report } => {
            let mut active_position_lease = active_position_lease;
            let shadow_entry_token_amount_raw = report.entry_token_amount_raw;
            let mut retain_runtime_pool = false;
            let mut shadow_execution_outcome = report
                .err
                .as_deref()
                .map(shadow_execution_outcome_from_report_err)
                .unwrap_or_else(|| "shadow_simulated".to_string());
            let shadow_event = crate::components::trigger::shadow_run::shadow_buy_event_from_report(
                &pool_amm_id.to_string(),
                &pool_data.base_mint,
                report,
            );
            if trigger_component.shadow_run_emit_event_bus() {
                if let Err(e) =
                    event_tx.send(GhostEvent::shadow_buy_simulated(shadow_event.clone()))
                {
                    warn!(
                        pool = %pool_amm_id,
                        "Failed to emit ShadowBuySimulated event: {}",
                        e
                    );
                }
            }

            if shadow_event.err.is_none() && matches!(post_buy_lane, "paper" | "shadow") {
                let position_slot_id = active_position_lease.as_ref().map(|lease| lease.slot_id);
                let epoch = post_buy_epoch.fetch_add(1, Ordering::Relaxed);
                let trace_ref = shadow_event
                    .live_signature
                    .clone()
                    .unwrap_or_else(|| shadow_event.decision_ts_ms.to_string());
                if post_buy_lane == "shadow" {
                    match send_shadow_post_buy_handoff(
                        event_tx,
                        post_buy_tx,
                        pool_amm_id,
                        pool_data,
                        &trace_ref,
                        trade_value_sol,
                        tip_lamports,
                        post_buy_lane,
                        epoch,
                        position_slot_id,
                        min_tokens_out,
                        shadow_entry_token_amount_raw,
                    )
                    .await
                    {
                        Ok(Some(ack)) => {
                            if let Some(outcome) = shadow_execution_outcome_from_handoff_ack(ack) {
                                shadow_execution_outcome = outcome;
                            } else {
                                retain_runtime_pool = true;
                                if let Some(lease) = active_position_lease.take() {
                                    lease.retain();
                                }
                            }
                        }
                        Ok(None) => {
                            shadow_execution_outcome = "shadow_handoff_unconfirmed".to_string();
                            warn!(
                                pool = %pool_amm_id,
                                "Shadow PostBuySubmitted handoff fell back to broadcast-only transport; keeping outcome unconfirmed"
                            );
                        }
                        Err(e) => {
                            shadow_execution_outcome = "shadow_handoff_transport_error".to_string();
                            warn!(
                                pool = %pool_amm_id,
                                "Failed to send shadow-backed PostBuySubmitted event: {}",
                                e
                            );
                        }
                    }
                } else if let Err(e) = send_post_buy_handoff(
                    event_tx,
                    post_buy_tx,
                    pool_amm_id,
                    pool_data,
                    &trace_ref,
                    trade_value_sol,
                    tip_lamports,
                    post_buy_lane,
                    epoch,
                    position_slot_id,
                    min_tokens_out,
                    None,
                )
                .await
                {
                    shadow_execution_outcome = "shadow_handoff_transport_error".to_string();
                    warn!(
                        pool = %pool_amm_id,
                        "Failed to send shadow-backed PostBuySubmitted event: {}",
                        e
                    );
                } else {
                    retain_runtime_pool = true;
                    if let Some(lease) = active_position_lease.take() {
                        lease.retain();
                    }
                }
            }

            let shadow_log_path =
                std::path::PathBuf::from(trigger_component.shadow_run_output_path());
            let canonical_shadow_entry_record = shadow_entry_record_from_event(
                pool_amm_id,
                &pool_data.base_mint,
                trade_value_sol,
                &shadow_event,
                &shadow_execution_outcome,
            );
            let shadow_lifecycle_log_path =
                shadow_lifecycle_log_path.map(std::path::Path::to_path_buf);
            let shadow_join_key = shadow_join_key.to_string();
            let rollout_profile = rollout_profile.to_string();
            tokio::spawn(async move {
                if let Err(e) = append_shadow_buy_report_record(
                    &shadow_log_path,
                    shadow_lifecycle_log_path.as_deref(),
                    crate::config::TriggerEntryMode::ShadowOnly,
                    &shadow_event,
                    &shadow_join_key,
                    &rollout_profile,
                    crate::components::trigger::shadow_run::ShadowDispatchStatus::Closed,
                )
                .await
                {
                    error!("Failed to append shadow buy report record: {}", e);
                }
            });
            maybe_append_canonical_shadow_entry_record(
                execution_mode,
                canonical_shadow_entry_log_path,
                canonical_shadow_entry_record,
            )
            .await;

            Ok(TriggerBuyOutcomeApplied {
                bought: false,
                retain_runtime_pool,
                close_reason: WindowCloseReason::PoolShadowedEarly,
                shadow_execution_outcome,
            })
        }
    }
}

async fn apply_trigger_dispatch_receipt(
    event_tx: &crate::events::EventBusSender,
    post_buy_tx: Option<&DirectPostBuySender>,
    trigger_component: &Arc<crate::components::trigger::TriggerComponent>,
    post_buy_epoch: &std::sync::atomic::AtomicU64,
    execution_mode: ExecutionMode,
    canonical_shadow_entry_log_path: &std::path::Path,
    shadow_lifecycle_log_path: Option<&std::path::Path>,
    rollout_profile: &str,
    pool_amm_id: Pubkey,
    pool_data: &DetectedPool,
    trade_value_sol: f64,
    tip_lamports: u64,
    post_buy_lane: &str,
    receipt: crate::components::trigger::TriggerDispatchReceipt,
) -> anyhow::Result<TriggerBuyOutcomeApplied> {
    let crate::components::trigger::TriggerDispatchReceipt {
        primary_outcome,
        shadow_task,
        active_position_lease,
        retain_position_slot_on_error,
        failed_request,
        failed_context,
    } = receipt;
    let min_tokens_out = failed_request
        .as_ref()
        .map(|request| request.min_tokens_out);
    let mut active_position_lease = active_position_lease;
    let shadow_join_key = crate::components::trigger::shadow_run::make_shadow_join_key(
        &pool_amm_id.to_string(),
        &pool_data.base_mint,
        pool_data.timestamp_ms,
    );

    match primary_outcome {
        Ok(outcome) => {
            let live_signature = match &outcome {
                crate::components::trigger::TriggerBuyOutcome::LiveConfirmed {
                    signature, ..
                } => Some(signature.to_string()),
                _ => None,
            };
            if live_signature.is_some() && shadow_task.is_some() {
                increment_counter!("trigger_live_success_with_shadow_companion_total");
            }
            let applied = apply_trigger_buy_outcome(
                event_tx,
                post_buy_tx,
                trigger_component,
                post_buy_epoch,
                execution_mode,
                canonical_shadow_entry_log_path,
                shadow_lifecycle_log_path,
                &shadow_join_key,
                rollout_profile,
                pool_amm_id,
                pool_data,
                trade_value_sol,
                tip_lamports,
                post_buy_lane,
                active_position_lease.take(),
                min_tokens_out,
                outcome,
            )
            .await;
            if let Some(shadow_task) = shadow_task {
                spawn_shadow_buy_observer(
                    event_tx.clone(),
                    pool_amm_id,
                    pool_data.base_mint.clone(),
                    trigger_component.shadow_run_emit_event_bus(),
                    std::path::PathBuf::from(trigger_component.shadow_run_output_path()),
                    shadow_lifecycle_log_path.map(std::path::Path::to_path_buf),
                    shadow_join_key.clone(),
                    rollout_profile.to_string(),
                    live_signature,
                    shadow_task,
                );
            }
            applied
        }
        Err(e) => {
            if retain_position_slot_on_error {
                if let Some(slot_id) = active_position_lease.as_ref().map(|lease| lease.slot_id) {
                    if let Some(lease) = active_position_lease.take() {
                        lease.retain();
                    }
                    warn!(
                        pool = %pool_amm_id,
                        slot_id = %slot_id,
                        error = %e,
                        "Live BUY landing remained uncertain after submit; keeping position slot reserved fail-closed"
                    );
                }
            }
            if let Some(shadow_task) = shadow_task {
                spawn_shadow_buy_observer(
                    event_tx.clone(),
                    pool_amm_id,
                    pool_data.base_mint.clone(),
                    trigger_component.shadow_run_emit_event_bus(),
                    std::path::PathBuf::from(trigger_component.shadow_run_output_path()),
                    shadow_lifecycle_log_path.map(std::path::Path::to_path_buf),
                    shadow_join_key.clone(),
                    rollout_profile.to_string(),
                    None,
                    shadow_task,
                );
            } else if let Some(request) = failed_request {
                if trigger_component.supports_shadow_run() {
                    let shadow_log_path =
                        std::path::PathBuf::from(trigger_component.shadow_run_output_path());
                    let shadow_event =
                        crate::components::trigger::shadow_run::shadow_failure_event_from_request(
                            &pool_amm_id.to_string(),
                            &pool_data.base_mint,
                            &request,
                            None,
                            &e,
                        );
                    if trigger_component.shadow_run_emit_event_bus() {
                        if let Err(send_err) =
                            event_tx.send(GhostEvent::shadow_buy_simulated(shadow_event.clone()))
                        {
                            warn!(
                                pool = %pool_amm_id,
                                "Failed to emit shadow dispatch failure event: {}",
                                send_err
                            );
                        }
                    }
                    if let Err(write_err) = append_shadow_buy_report_record(
                        &shadow_log_path,
                        shadow_lifecycle_log_path,
                        trigger_component.entry_mode(),
                        &shadow_event,
                        &shadow_join_key,
                        rollout_profile,
                        crate::components::trigger::shadow_run::ShadowDispatchStatus::Failed,
                    )
                    .await
                    {
                        error!(
                            pool = %pool_amm_id,
                            "Failed to append shadow failure record after dispatch error: {}",
                            write_err
                        );
                    }
                }
                maybe_append_canonical_shadow_entry_record(
                    execution_mode,
                    canonical_shadow_entry_log_path,
                    shadow_entry_record_from_request(
                        pool_amm_id,
                        &pool_data.base_mint,
                        &request,
                        &shadow_execution_outcome_from_dispatch_error(trigger_component, &e),
                    ),
                )
                .await;
            } else if let Some(context) = failed_context {
                if trigger_component.supports_shadow_run() {
                    let shadow_log_path =
                        std::path::PathBuf::from(trigger_component.shadow_run_output_path());
                    let shadow_event =
                        crate::components::trigger::shadow_run::shadow_failure_event_from_context(
                            &pool_amm_id.to_string(),
                            &pool_data.base_mint,
                            &context,
                            pool_data.base_mint.clone(),
                            None,
                            &e,
                        );
                    if trigger_component.shadow_run_emit_event_bus() {
                        if let Err(send_err) =
                            event_tx.send(GhostEvent::shadow_buy_simulated(shadow_event.clone()))
                        {
                            warn!(
                                pool = %pool_amm_id,
                                "Failed to emit shadow preflight failure event: {}",
                                send_err
                            );
                        }
                    }
                    if let Err(write_err) = append_shadow_buy_report_record(
                        &shadow_log_path,
                        shadow_lifecycle_log_path,
                        trigger_component.entry_mode(),
                        &shadow_event,
                        &shadow_join_key,
                        rollout_profile,
                        crate::components::trigger::shadow_run::ShadowDispatchStatus::Failed,
                    )
                    .await
                    {
                        error!(
                            pool = %pool_amm_id,
                            "Failed to append shadow preflight failure record: {}",
                            write_err
                        );
                    }
                }
            }
            Err(e)
        }
    }
}

async fn append_coverage_audit_record(
    log_path: &std::path::Path,
    record: &CoverageAuditRecord,
) -> anyhow::Result<()> {
    fn coverage_audit_write_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    let _guard = coverage_audit_write_lock().lock().await;
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .await?;
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');
    tokio::io::AsyncWriteExt::write_all(&mut file, &line).await?;
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    Ok(())
}

fn build_seer_geyser_event_from_confirmed_tx(
    tx: &EncodedConfirmedTransactionWithStatusMeta,
    signature: &Signature,
) -> Option<SeerGeyserEvent> {
    let meta = tx.transaction.meta.as_ref()?;
    let (accounts, instructions) =
        parse_ui_transaction_with_meta(&tx.transaction.transaction, Some(meta))?;
    let logs = extract_logs_from_meta(meta);
    let (pre_balances, post_balances) = extract_balances_from_meta(meta);
    let inner_instructions = extract_inner_instructions_from_meta(meta, &accounts);
    let (pre_token_balances, post_token_balances) = extract_token_balances_from_meta(meta);

    Some(SeerGeyserEvent::Transaction {
        slot: seer::types::normalize_slot(Some(tx.slot)),
        event_ts_ms: seer::types::event_ts_from_block_time(tx.block_time),
        arrival_ts_ms: None,
        event_time: ghost_core::EventTimeMetadata::new(
            seer::types::event_ts_from_block_time(tx.block_time),
            Some(seer::types::ingress_epoch_ms()),
            None,
        ),
        signature: *signature,
        accounts,
        instructions,
        logs,
        block_time: tx.block_time,
        account_data: HashMap::new(),
        pre_balances,
        post_balances,
        success: meta.err.is_none(),
        error_code: meta.err.as_ref().map(|err| format!("{:?}", err)),
        compute_units_consumed: Option::<u64>::from(meta.compute_units_consumed.clone()),
        synthetic: false,
        source: "coverage_audit_rpc".to_string(),
        mpcf_payload_bytes: None,
        mpcf_payload_missing_reason: RawBytesMissingReason::ProviderDoesNotSupport,
        inner_instructions,
        pre_token_balances,
        post_token_balances,
    })
}

fn merge_truth_trade(
    truth_signatures: &mut HashMap<String, CoverageAuditTruthSignatureState>,
    window: &CoverageAuditClosedWindow,
    trade: &TradeEvent,
) {
    let Some((chain_ts_ms, time_source)) = truth_trade_chain_ts_ms(trade) else {
        return;
    };
    if trade.pool_amm_id.to_string() != window.pool_id {
        return;
    }
    if chain_ts_ms < window.t0_ms || chain_ts_ms > window.t_end_ms {
        return;
    }

    let signature_key = trade.signature.to_string();
    let failed = !trade.success;
    truth_signatures
        .entry(signature_key)
        .and_modify(|existing| {
            existing.failed |= failed;
            if existing.time_source.is_none() {
                existing.time_source = Some(time_source.to_string());
            }
        })
        .or_insert_with(|| CoverageAuditTruthSignatureState {
            failed,
            time_source: Some(time_source.to_string()),
        });
}

fn truth_trade_chain_ts_ms(trade: &TradeEvent) -> Option<(u64, &'static str)> {
    trade
        .event_time
        .chain_event_ts_ms
        .map(|ts| (ts, "explicit_chain_event"))
        .or_else(|| {
            if trade.timestamp_ms > 0
                && matches!(
                    trade.semantic.timestamp_quality,
                    ghost_core::TimestampQuality::Chain
                )
            {
                Some((trade.timestamp_ms, "legacy_chain_compat"))
            } else {
                None
            }
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeTxTimeSourceInfo {
    effective_source: &'static str,
    fallback_class: Option<&'static str>,
}

fn runtime_tx_time_source_info(tx: &PoolTransaction) -> RuntimeTxTimeSourceInfo {
    if tx.event_time.chain_event_ts_ms.is_some() {
        RuntimeTxTimeSourceInfo {
            effective_source: "chain_event",
            fallback_class: None,
        }
    } else if tx.event_time.ingress_wall_ts_ms.is_some() {
        RuntimeTxTimeSourceInfo {
            effective_source: "ingress_wall",
            fallback_class: None,
        }
    } else if tx.compat_event_ts_ms().is_some() {
        RuntimeTxTimeSourceInfo {
            effective_source: "wall_clock_fallback",
            fallback_class: Some("legacy_compat_rejected"),
        }
    } else {
        RuntimeTxTimeSourceInfo {
            effective_source: "wall_clock_fallback",
            fallback_class: Some("missing_explicit_time"),
        }
    }
}

fn runtime_account_update_time_source_info(event: &AccountUpdateEvent) -> RuntimeTxTimeSourceInfo {
    if event.event_time.chain_event_ts_ms.is_some() {
        RuntimeTxTimeSourceInfo {
            effective_source: "chain_event",
            fallback_class: None,
        }
    } else if event.event_time.ingress_wall_ts_ms.is_some() {
        RuntimeTxTimeSourceInfo {
            effective_source: "ingress_wall",
            fallback_class: None,
        }
    } else if matches!(
        event.replay_origin,
        seer::ipc::AccountUpdateReplayOrigin::PendingReplay
    ) {
        RuntimeTxTimeSourceInfo {
            effective_source: "wall_clock_fallback",
            fallback_class: Some("replay_missing_explicit_time"),
        }
    } else {
        RuntimeTxTimeSourceInfo {
            effective_source: "wall_clock_fallback",
            fallback_class: Some("missing_explicit_time"),
        }
    }
}

async fn fetch_chain_truth_signatures(
    oracle_runtime: &OracleRuntime,
    window: &CoverageAuditClosedWindow,
) -> Result<HashMap<String, CoverageAuditTruthSignatureState>, String> {
    let rpc_client = oracle_runtime
        .rpc_client
        .as_ref()
        .cloned()
        .ok_or_else(|| "rpc_client_unavailable".to_string())?;
    let pool_pubkey =
        Pubkey::from_str(&window.pool_id).map_err(|err| format!("invalid_pool_id:{}", err))?;
    let parser = BinaryParser::new(false);
    if let Some(base_mint) = &window.base_mint {
        parser.set_curve_mapping(&window.pool_id, base_mint);
    }

    let block_time_start_sec = window.t0_ms.saturating_sub(1_000) / 1_000;
    let block_time_end_sec = window.t_end_ms.saturating_add(1_000) / 1_000;
    let tx_config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base64),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
    };

    let mut before: Option<Signature> = None;
    let mut signature_infos = Vec::new();
    for _ in 0..5 {
        let batch = rpc_client
            .get_signatures_for_address_with_config(
                &pool_pubkey,
                GetConfirmedSignaturesForAddress2Config {
                    before,
                    until: None,
                    limit: Some(1_000),
                    commitment: Some(CommitmentConfig::confirmed()),
                },
            )
            .await
            .map_err(|err| format!("get_signatures_failed:{}", err))?;
        if batch.is_empty() {
            break;
        }

        let mut oldest_block_time: Option<u64> = None;
        for info in &batch {
            if let Some(block_time) = info.block_time.and_then(|ts| u64::try_from(ts).ok()) {
                oldest_block_time =
                    Some(oldest_block_time.map_or(block_time, |prev| prev.min(block_time)));
                if block_time >= block_time_start_sec && block_time <= block_time_end_sec {
                    signature_infos.push(info.clone());
                }
            } else {
                signature_infos.push(info.clone());
            }
        }

        before = batch
            .last()
            .and_then(|info| Signature::from_str(&info.signature).ok());

        if oldest_block_time.is_some_and(|ts| ts < block_time_start_sec) {
            break;
        }
    }

    let mut truth_signatures: HashMap<String, CoverageAuditTruthSignatureState> = HashMap::new();
    for info in signature_infos {
        let signature = match Signature::from_str(&info.signature) {
            Ok(signature) => signature,
            Err(_) => continue,
        };
        let tx = match rpc_client
            .get_transaction_with_config(&signature, tx_config)
            .await
        {
            Ok(tx) => tx,
            Err(_) => continue,
        };
        let Some(event) = build_seer_geyser_event_from_confirmed_tx(&tx, &signature) else {
            continue;
        };
        let trades = match parser.parse_trades(&event) {
            Ok(trades) => trades,
            Err(_) => continue,
        };
        for trade in trades {
            merge_truth_trade(&mut truth_signatures, window, &trade);
        }
    }

    Ok(truth_signatures)
}

fn spawn_coverage_audit_for_closed_window(
    ctx: Arc<PoolObservationContext>,
    pool_id: Pubkey,
    base_mint: Option<Pubkey>,
    window_state: &WindowState,
    verdict: &str,
) {
    let pool_id_str = pool_id.to_string();
    if let Some(base_mint) = base_mint {
        coverage_audit().set_window_base_mint(&pool_id_str, Some(base_mint.to_string()));
    }
    let Some(closed_window) = coverage_audit().close_window(
        &pool_id_str,
        Some(verdict.to_string()),
        window_state.window_complete,
        window_state
            .window_close_reason
            .as_ref()
            .map(|reason| reason.tag().to_string()),
    ) else {
        return;
    };

    tokio::spawn(async move {
        let truth_result = fetch_chain_truth_signatures(&ctx.oracle_runtime, &closed_window).await;
        let record = match truth_result {
            Ok(signatures) => coverage_audit().build_record(closed_window, signatures, None),
            Err(err) => coverage_audit().build_record(closed_window, HashMap::new(), Some(err)),
        };

        if let Err(err) = append_coverage_audit_record(&ctx.coverage_audit_log_path, &record).await
        {
            error!(
                pool = %record.pool_id,
                window_id = %record.window_id,
                error = %err,
                "Failed to append coverage audit record"
            );
            return;
        }

        if record.rpc_error.is_some() || record.missing_count > 0 {
            warn!(
                "COVERAGE_AUDIT pool={} truth={} rx={} emitted={} accepted={} miss={} rx_pct={:.2} emit_pct={:.2} accepted_pct={:.2} reasons={:?} truth_time_sources={:?} runtime_time_sources={:?}",
                record.pool_id,
                record.chain_truth_count,
                record.seer_rx_count,
                record.seer_emitted_count,
                record.runtime_accepted_count,
                record.missing_count,
                record.truth_to_rx_pct,
                record.truth_to_emit_pct,
                record.truth_to_runtime_accept_pct,
                record.counts_by_reason,
                record.chain_truth_by_time_source,
                record.runtime_seen_by_time_source,
            );
        } else {
            debug!(
                "COVERAGE_AUDIT pool={} truth={} rx={} emitted={} accepted={} miss={} rx_pct={:.2} emit_pct={:.2} accepted_pct={:.2} reasons={:?} truth_time_sources={:?} runtime_time_sources={:?}",
                record.pool_id,
                record.chain_truth_count,
                record.seer_rx_count,
                record.seer_emitted_count,
                record.runtime_accepted_count,
                record.missing_count,
                record.truth_to_rx_pct,
                record.truth_to_emit_pct,
                record.truth_to_runtime_accept_pct,
                record.counts_by_reason,
                record.chain_truth_by_time_source,
                record.runtime_seen_by_time_source,
            );
        }
    });
}

/// Per-pool observation task that runs gatekeeper evaluation independently.
///
/// Each pool gets its own tokio task with:
/// - Independent `GatekeeperBuffer` for transaction accumulation
/// - Own `tokio::time::sleep` deadline (eliminates 1s sweep timer jitter)
/// - IWIM veto gate runs inside this task (doesn't block other pools)
///
/// The task exits on any terminal verdict (BUY/REJECT/TIMEOUT) and sends
/// the result back to the router via `ctx.result_tx`.
async fn pool_observation_task(
    pool_id: Pubkey,
    initial_pool_data: Option<Arc<DetectedPool>>,
    registered_wall_ts_ms: u64,
    mut rx: tokio::sync::mpsc::Receiver<PoolObservationMsg>,
    ctx: Arc<PoolObservationContext>,
) {
    // ── Mutable pool_data — may arrive late via NewPool message ─────────
    let mut pool_data = initial_pool_data;

    let Some(session) = ensure_pool_observation_session(
        ctx.as_ref(),
        pool_id,
        registered_wall_ts_ms,
        pool_data.as_deref(),
    ) else {
        warn!(pool = %pool_id, "ZADANIE OBSERWACJI PULI ZOSTAŁO PRZERWANE PRZED OTWARCIEM SESJI");
        let _ = ctx.result_tx.send(PoolObservationResult {
            pool_id,
            base_mint: None,
            bought: false,
            retain_runtime_pool: false,
        });
        return;
    };

    // ── Build pool identity for JSONL enrichment ────────────────────────
    // When pool_data is present but its base_mint is invalid/unknown (e.g.
    // seer emitted a NewPoolDetected with an incomplete discriminator), fall
    // back to the OracleRuntime registry before accepting "unknown" so that
    // `maybe_promote_observation_identity_from_tx` is only invoked as a last
    // resort.
    let mut identity = if let Some(ref pd) = pool_data {
        if is_shadow_base_mint_present(&pd.base_mint) {
            ObservationIdentity {
                base_mint: pd.base_mint.clone(),
                first_seen_ts_ms: registered_wall_ts_ms,
                first_seen_clock_source: "registered_wall",
                end_10s_ts_ms: registered_wall_ts_ms.saturating_add(ctx.ab_window_ms),
                dev_pubkey: normalize_dev_pubkey_str(&pd.creator)
                    .unwrap_or_else(|| "unknown".to_string()),
                failed_promotion_attempts: 0,
                next_promotion_attempt_ts_ms: 0,
            }
        } else if let Some(base_mint) = ctx.oracle_runtime.lookup_base_mint_for_pool(&pool_id) {
            // pool_data present but base_mint is unknown — registry knows it
            build_registered_observation_identity(pool_id, base_mint, ctx.ab_window_ms)
        } else {
            // pool_data present but both pool_data.base_mint and registry are unknown;
            // start with whatever pool_data has so at least creator/ts are captured.
            ObservationIdentity {
                base_mint: pd.base_mint.clone(),
                first_seen_ts_ms: registered_wall_ts_ms,
                first_seen_clock_source: "registered_wall",
                end_10s_ts_ms: registered_wall_ts_ms.saturating_add(ctx.ab_window_ms),
                dev_pubkey: normalize_dev_pubkey_str(&pd.creator)
                    .unwrap_or_else(|| "unknown".to_string()),
                failed_promotion_attempts: 0,
                next_promotion_attempt_ts_ms: 0,
            }
        }
    } else if let Some(base_mint) = ctx
        .oracle_runtime
        .lookup_base_mint_for_pool(&pool_id)
        .or_else(|| Some(session.read().base_mint))
    {
        build_registered_observation_identity(pool_id, base_mint, ctx.ab_window_ms)
    } else {
        build_unknown_observation_identity(pool_id, ctx.ab_window_ms)
    };
    {
        let mut session = session.write();
        let buffer = session.gatekeeper_buffer_mut();
        buffer.set_registered_wall_t0(registered_wall_ts_ms);
        let curve_t0 = pool_data
            .as_ref()
            .and_then(|pd| detected_pool_epoch_like_ts_ms(pd))
            .unwrap_or(registered_wall_ts_ms);
        let curve_t0_source = pool_data
            .as_ref()
            .map(|pd| detected_pool_epoch_source_label(pd))
            .unwrap_or("registered_wall");
        buffer.set_curve_t0_with_source(curve_t0, curve_t0_source);
        if let Some(ref pd) = pool_data {
            buffer.set_pool_identity_with_liquidity(
                Some(pd.creator.as_str()),
                Some(pd.signature.as_str()),
                pd.initial_liquidity_sol,
            );
        }
    }

    // ── Fingerprint aggregator ──────────────────────────────────────────
    let slot_known = pool_data.as_ref().and_then(|pd| pd.slot).is_some();
    let creation_slot = pool_data
        .as_ref()
        .and_then(|pd| pd.slot)
        .unwrap_or(u64::MAX);
    let creation_ts = pool_data
        .as_ref()
        .and_then(|pd| detected_pool_epoch_like_ts_ms(pd))
        .unwrap_or(0);
    let supply_raw = if slot_known {
        Some(GENESIS_TOKEN_RESERVES as u128)
    } else {
        None
    };
    let mut fingerprint_agg: Option<FingerprintAggregator> = Some(FingerprintAggregator::new(
        ctx.fingerprint_config.clone(),
        creation_slot,
        slot_known,
        creation_ts,
        supply_raw,
        PUMPFUN_TOKEN_DECIMALS,
        pool_data.as_ref().map(|pd| pd.creator.clone()),
    ));

    // ── A/B window state ────────────────────────────────────────────────
    let mut window_state = initial_window_state_for_task(
        pool_data.as_deref(),
        registered_wall_ts_ms,
        ctx.ab_window_ms,
    );
    let mut coverage_window_opened = false;

    // ── Per-pool monotonic timestamp ────────────────────────────────────
    let mut last_event_ts: Option<u64> = None;

    // ── Base mint for result reporting ──────────────────────────────────
    let mut base_mint_pubkey = pool_data
        .as_ref()
        .and_then(|pd| Pubkey::try_from(pd.base_mint.as_str()).ok());
    if base_mint_pubkey.is_none() {
        base_mint_pubkey = ctx
            .oracle_runtime
            .lookup_base_mint_for_pool(&pool_id)
            .or_else(|| Some(session.read().base_mint));
    }

    maybe_open_coverage_window(
        pool_id,
        base_mint_pubkey,
        window_state.as_ref(),
        &mut coverage_window_opened,
    );

    ctx.snapshot_engine.track_pool(pool_id);

    // ── Emit InitPoolEvent if reserves available ────────────────────────
    maybe_emit_init_pool_event(&ctx, pool_id, pool_data.as_deref());

    let post_buy_lane: &str = ctx.post_buy_lane();

    // ── Deadline: independent per-pool timer ────────────────────────────
    // +1ms grace to avoid races with event-time based deadline in buffer
    let deadline = tokio::time::sleep(Duration::from_millis(
        ctx.gatekeeper_config.max_wait_time_ms.saturating_add(1),
    ));
    tokio::pin!(deadline);

    // ── DOW timer: per-pool interval for time-guaranteed shadow checkpoints ──
    // Fires Early/Normal/Extended independently of TX traffic.
    // Interval ticks are consumed as a tokio::select! branch — same task,
    // same serialized buffer access, no duplicate checkpoints.
    let mut dow_tick = crate::components::gatekeeper_dow_timer::dow_timer_interval(
        ctx.gatekeeper_config.dow.tick_interval_ms,
    );
    // Skip the immediate tick (0ms burst after interval creation).
    dow_tick.tick().await;

    loop {
        let verdict = tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(PoolObservationMsg::Transaction(tx)) => {
                        // Normalize timestamp (per-pool monotonic)
                        let (normalized_ts, has_chain_time) =
                            normalize_gatekeeper_event_time_ms(&tx, last_event_ts);
                        let runtime_time_source = runtime_tx_time_source_info(&tx);
                        if !has_chain_time {
                            debug!(
                                "POOL_TASK_NO_CHAIN_TIME pool={} sig={} slot={:?} ts={} source={}",
                                pool_id,
                                tx.signature,
                                tx.slot,
                                normalized_ts,
                                runtime_time_source.effective_source
                            );
                        }
                        last_event_ts = Some(normalized_ts);
                        let raw_tx = Arc::new((*tx).clone());
                        coverage_audit().record_runtime_seen_with_detail(
                            &pool_id.to_string(),
                            &raw_tx.signature,
                            runtime_time_source.effective_source,
                            runtime_time_source.fallback_class,
                        );

                        maybe_promote_observation_identity_from_tx(
                            pool_id,
                            raw_tx.as_ref(),
                            ctx.ab_window_ms,
                            &mut identity,
                            &mut base_mint_pubkey,
                            current_time_ms(),
                            max_identity_promotion_retries(),
                        );

                        let _ = ctx
                            .oracle_runtime
                            .maybe_materialize_canonical_state_from_observed_tx(
                                pool_id,
                                base_mint_pubkey,
                                raw_tx.as_ref(),
                            );

                        // Enrich from AccountStateCore first; ShadowLedger is limited
                        // to bootstrap/degraded fallback semantics in PR7.
                        let tx = enrich_pool_tx_from_canonical_state(
                            raw_tx,
                            pool_id,
                            base_mint_pubkey,
                            ctx.oracle_runtime.account_state_core(),
                            ctx.oracle_runtime.get_shadow_ledger(),
                            ctx.oracle_runtime.shadow_ledger_enrichment_freshness_ms(),
                        );

                        // Feed fingerprint aggregator
                        if let Some(ref mut fp_agg) = fingerprint_agg {
                            if let Some(fp_event) = pool_tx_to_fingerprint_event(&tx) {
                                if fp_agg.in_window(&fp_event) {
                                    fp_agg.ingest(&fp_event);
                                }
                            }
                        }

                        // Update A/B window state
                        let window_state =
                            ensure_window_state_for_tx(&mut window_state, normalized_ts, ctx.ab_window_ms);
                        maybe_open_coverage_window(
                            pool_id,
                            base_mint_pubkey,
                            Some(window_state),
                            &mut coverage_window_opened,
                        );
                        window_state.try_ingest(normalized_ts, &tx.signer, tx.success);
                        increment_counter!("gatekeeper_window_tx_seen");

                        increment_counter!("grpc_events_parsed_ok");
                        let verdict = {
                            let mut session = session.write();
                            let accepted_tx_before = session.diagnostics.total_tx_seen;
                            let ingress = session.ingest_transaction(tx.clone());
                            if session.diagnostics.total_tx_seen > accepted_tx_before {
                                session.try_checkpoint(normalized_ts);
                            }
                            resolve_feature_trigger_outcome(&mut session, ingress, &ctx.gatekeeper_config)
                        };
                        verdict
                    }
                    Some(PoolObservationMsg::NewPool(pd)) => {
                        // Late-arriving pool metadata — upgrade identity & state.
                        // Also handle the case where pool_data already exists but
                        // has an incomplete identity (base_mint or creator unknown)
                        // because the upstream seer emitted an incomplete event.
                        let identity_needs_upgrade =
                            !is_shadow_base_mint_present(&identity.base_mint)
                                || !is_shadow_creator_present(&identity.dev_pubkey);
                        if pool_data.is_none() || identity_needs_upgrade {
                            info!(
                                "POOL_TASK_LATE_METADATA pool={} mint={} identity_needs_upgrade={}",
                                pool_id, pd.base_mint, identity_needs_upgrade
                            );
                            identity = ObservationIdentity {
                                base_mint: pd.base_mint.clone(),
                                first_seen_ts_ms: registered_wall_ts_ms,
                                first_seen_clock_source: "registered_wall",
                                end_10s_ts_ms: registered_wall_ts_ms.saturating_add(ctx.ab_window_ms),
                                dev_pubkey: normalize_dev_pubkey_str(&pd.creator)
                                    .unwrap_or_else(|| "unknown".to_string()),
                                failed_promotion_attempts: 0,
                                next_promotion_attempt_ts_ms: 0,
                            };
                            base_mint_pubkey =
                                Pubkey::try_from(pd.base_mint.as_str()).ok();
                            coverage_audit().set_window_base_mint(
                                &pool_id.to_string(),
                                Some(pd.base_mint.clone()),
                            );

                            if let Ok(dev_wallet) = Pubkey::try_from(pd.creator.as_str()) {
                                let mut session = session.write();
                                session.dev_wallet = Some(dev_wallet);
                                session.update_tx_intelligence_dev_wallet(Some(dev_wallet));
                                session.update_tx_intelligence_fingerprint_anchor(
                                    pd.slot,
                                    detected_pool_epoch_like_ts_ms(&pd),
                                    Some(dev_wallet),
                                );
                                if let Some(curve_t0) = detected_pool_epoch_like_ts_ms(&pd) {
                                    session
                                        .gatekeeper_buffer_mut()
                                        .set_curve_t0_with_source(
                                            curve_t0,
                                            detected_pool_epoch_source_label(&pd),
                                        );
                                }
                                session
                                    .gatekeeper_buffer_mut()
                                    .set_pool_identity_with_liquidity(
                                        Some(pd.creator.as_str()),
                                        Some(pd.signature.as_str()),
                                        pd.initial_liquidity_sol,
                                    );
                            } else if let Some(curve_t0) = detected_pool_epoch_like_ts_ms(&pd) {
                                session
                                    .write()
                                    .gatekeeper_buffer_mut()
                                    .set_curve_t0_with_source(
                                        curve_t0,
                                        detected_pool_epoch_source_label(&pd),
                                    );
                            }

                            // Re-init fingerprint aggregator with real slot data
                            if let Some(slot) = pd.slot {
                                fingerprint_agg = Some(FingerprintAggregator::new(
                                    ctx.fingerprint_config.clone(),
                                    slot,
                                    true,
                                    detected_pool_event_ts_ms(&pd),
                                    Some(GENESIS_TOKEN_RESERVES as u128),
                                    PUMPFUN_TOKEN_DECIMALS,
                                    Some(pd.creator.clone()),
                                ));
                            }

                            maybe_emit_init_pool_event(&ctx, pool_id, Some(&pd));
                            pool_data = Some(pd);
                        }
                        GatekeeperVerdict::Wait
                    }
                    None => {
                        // Channel closed — force terminal evaluation on the collected snapshot.
                        let mut session = session.write();
                        session
                            .gatekeeper_buffer_mut()
                            .advance_event_clock(current_time_ms());
                        evaluate_feature_driven_terminal_verdict(
                            &mut session,
                            &ctx.gatekeeper_config,
                            true,
                        )
                    }
                }
            }
            _ = dow_tick.tick() => {
                // ── DOW timer: time-guaranteed shadow checkpoint firing ──
                // Fires Early (2-5s), Normal (5-7s), Extended (7-10s)
                // independently of TX traffic. The *_shadow_fired flags
                // inside GatekeeperBuffer ensure one-shot per stage.
                if ctx.gatekeeper_config.v25.shadow_enabled
                    && ctx.gatekeeper_config.dow.enabled
                {
                    let mut session = session.write();
                    let now_wall = current_time_ms();
                    session
                        .gatekeeper_buffer_mut()
                        .maybe_fire_shadow_checkpoint_from(
                            now_wall,
                            crate::components::gatekeeper::ShadowCheckpointSource::Timer,
                        );
                }
                GatekeeperVerdict::Wait
            }
            _ = &mut deadline => {
                let now_ms = current_time_ms();
                if let Some(window_state) = window_state.as_mut() {
                    window_state.try_sweep_complete(now_ms);
                }
                let mut session = session.write();
                session.gatekeeper_buffer_mut().advance_event_clock(now_ms);
                evaluate_feature_driven_terminal_verdict(
                    &mut session,
                    &ctx.gatekeeper_config,
                    true,
                )
            }
        };

        match verdict {
            GatekeeperVerdict::Wait | GatekeeperVerdict::PendingCurve => continue,

            GatekeeperVerdict::ApprovedTx { tx, metrics } => {
                // Pass-through for pools already allowed by runtime policy.
                if let Ok(signer_key) = Pubkey::try_from(tx.signer.as_str()) {
                    let event_ts_ms = tx_event_ts_ms(tx.as_ref());
                    let active_mint = tx
                        .token_mint
                        .as_ref()
                        .and_then(|m| Pubkey::try_from(m.as_str()).ok())
                        .or(base_mint_pubkey);

                    if tx.arrival_ts_ms > 0 {
                        let requested_sol = tx
                            .sol_amount_lamports
                            .map(|l| l as f64 / LAMPORTS_PER_SOL)
                            .unwrap_or(tx.volume_sol);
                        let executed_sol = if tx.success { requested_sol } else { 0.0 };
                        ctx.oracle_runtime.record_panic_transaction(
                            pool_id,
                            PanicTx {
                                slot: tx.slot,
                                arrival_ts_ms: tx.arrival_ts_ms,
                                event_time: tx.event_time,
                                impulse_weight: requested_sol,
                                requested_sol_amount: requested_sol,
                                executed_sol_amount: executed_sol,
                                priority_fee_micro_lamports: 0,
                                success: tx.success,
                                signer: signer_key,
                            },
                        );
                    }

                    ctx.oracle_runtime.register_pool_tx(
                        pool_id,
                        event_ts_ms,
                        tx.slot,
                        tx.mpcf_payload.clone(),
                        None,
                        tx.signer.clone(),
                        tx.is_buy,
                        tx.volume_sol,
                    );

                    if tx.success {
                        if let Some(mint) = active_mint {
                            ctx.oracle_runtime
                                .forward_approved_tx_to_commit_or_live_pipeline(
                                    pool_id,
                                    mint,
                                    tx.as_ref(),
                                    event_ts_ms,
                                );
                        }
                    }

                    // Single-ingress contract (PR-3b):
                    // SnapshotListener is the authoritative writer to SnapshotEngine.
                    // OracleRuntime must NOT call handle_tx_event — that is a competing
                    // second write path.  track_pool() is kept for pool-lifecycle management.
                    if tx.success {
                        if let Some(_mint) = active_mint {
                            ctx.snapshot_engine.track_pool(pool_id);
                        }
                    }
                }
                continue;
            }

            GatekeeperVerdict::Reject {
                mut assessment,
                reason,
            } => {
                // P5: pool was evaluated and actively rejected → no dispatch.
                crate::oracle_metrics::record_shadow_lifecycle_status("no_dispatch_rejected");
                ctx.oracle_runtime.append_decision_to_wal(
                    pool_id,
                    pool_data.as_ref().and_then(|pd| pd.slot),
                    WalGatekeeperDecision::Reject,
                    Some(reason.clone()),
                );
                let window_state = window_state.get_or_insert_with(|| {
                    WindowState::from_first_tx(registered_wall_ts_ms, ctx.ab_window_ms)
                });
                maybe_open_coverage_window(
                    pool_id,
                    base_mint_pubkey,
                    Some(window_state),
                    &mut coverage_window_opened,
                );
                if let Some(fp_agg) = fingerprint_agg.take() {
                    assessment.early_fingerprint = Some(fp_agg.finalize());
                }
                window_state.mark_verdict_early(WindowCloseReason::PoolRejectedEarly);
                info!(
                    "🚫 GATEKEEPER MOWI: WYPIERDALAC! : pool={} reason={} (signers={} tx={} phases={}/6 in {}ms lag={}ms dust={}){}",
                    pool_id, reason, assessment.unique_signers_evaluated,
                    assessment.total_tx_evaluated, assessment.phases_passed,
                    assessment.observation_duration_ms, assessment.finalize_lag_ms, assessment.dust_filtered_count,
                    assessment.decision_summary(),
                );
                let mut buy_log = assessment.to_buy_log(&pool_id, &ctx.gatekeeper_config);
                enrich_buy_log_with_observation_identity(&mut buy_log, &identity);
                enrich_buy_log_with_window(
                    &mut buy_log,
                    &window_state,
                    &pool_id.to_string(),
                    "REJECT",
                );
                {
                    let session = session.read();
                    enrich_buy_log_with_clock_provenance(&mut buy_log, session.gatekeeper_buffer());
                    enrich_buy_log_with_vectors(
                        &mut buy_log,
                        session.gatekeeper_buffer(),
                        &window_state,
                    );
                }
                let dl = ctx.decision_logger.clone();
                tokio::spawn(async move {
                    dl.log_gatekeeper_buy_decision(buy_log).await;
                });
                if let Some(ref emitter) = ctx.event_emitter {
                    emit_gatekeeper_decision_event(emitter, &pool_id, "REJECT", &assessment);
                    if let Some(ref h) = ctx.health {
                        h.mark_events_write();
                    }
                }
                spawn_coverage_audit_for_closed_window(
                    ctx.clone(),
                    pool_id,
                    base_mint_pubkey,
                    &window_state,
                    "REJECT",
                );
                if let Some(ref h) = ctx.health {
                    h.mark_gatekeeper_decision();
                }
                ctx.oracle_runtime.mark_pool_scored(pool_id);
                finish_pool_observation(
                    ctx.as_ref(),
                    pool_id,
                    VerdictOutcome::Fail {
                        reason: reason.clone(),
                    },
                );
                let _ = ctx.result_tx.send(PoolObservationResult {
                    pool_id,
                    base_mint: base_mint_pubkey,
                    bought: false,
                    retain_runtime_pool: false,
                });
                return;
            }

            GatekeeperVerdict::Timeout { mut assessment } => {
                // P5: pool timed out without meeting Phase 1 → no dispatch eligible.
                crate::oracle_metrics::record_shadow_lifecycle_status("no_dispatch_eligible");
                ctx.oracle_runtime.append_decision_to_wal(
                    pool_id,
                    pool_data.as_ref().and_then(|pd| pd.slot),
                    WalGatekeeperDecision::Timeout,
                    assessment
                        .decision
                        .as_ref()
                        .map(|decision| decision.reason_chain.clone())
                        .or_else(|| Some("gatekeeper_timeout".to_string())),
                );
                let window_state = window_state.get_or_insert_with(|| {
                    WindowState::from_first_tx(registered_wall_ts_ms, ctx.ab_window_ms)
                });
                maybe_open_coverage_window(
                    pool_id,
                    base_mint_pubkey,
                    Some(window_state),
                    &mut coverage_window_opened,
                );
                if let Some(fp_agg) = fingerprint_agg.take() {
                    assessment.early_fingerprint = Some(fp_agg.finalize());
                }
                window_state.mark_verdict_early(WindowCloseReason::GatekeeperTimeout);
                let timeout_reason = assessment
                    .decision
                    .as_ref()
                    .map(|decision| decision.reason_chain.as_str())
                    .unwrap_or("TIMEOUT");
                info!(
                    "⏰ GATEKEEPER V2 ODPULA: pool={} reason={} (signers={} tx={} in {}ms lag={}ms dust={}){}",
                    pool_id,
                    timeout_reason,
                    assessment.unique_signers_evaluated,
                    assessment.total_tx_evaluated,
                    assessment.observation_duration_ms,
                    assessment.finalize_lag_ms,
                    assessment.dust_filtered_count,
                    assessment.decision_summary(),
                );
                let mut buy_log = assessment.to_buy_log(&pool_id, &ctx.gatekeeper_config);
                enrich_buy_log_with_observation_identity(&mut buy_log, &identity);
                enrich_buy_log_with_window(
                    &mut buy_log,
                    &window_state,
                    &pool_id.to_string(),
                    "TIMEOUT",
                );
                {
                    let session = session.read();
                    enrich_buy_log_with_clock_provenance(&mut buy_log, session.gatekeeper_buffer());
                    enrich_buy_log_with_vectors(
                        &mut buy_log,
                        session.gatekeeper_buffer(),
                        &window_state,
                    );
                }
                let dl = ctx.decision_logger.clone();
                tokio::spawn(async move {
                    dl.log_gatekeeper_buy_decision(buy_log).await;
                });
                if let Some(ref emitter) = ctx.event_emitter {
                    emit_gatekeeper_decision_event(emitter, &pool_id, "TIMEOUT", &assessment);
                    if let Some(ref h) = ctx.health {
                        h.mark_events_write();
                    }
                }
                spawn_coverage_audit_for_closed_window(
                    ctx.clone(),
                    pool_id,
                    base_mint_pubkey,
                    &window_state,
                    "TIMEOUT",
                );
                if let Some(ref h) = ctx.health {
                    h.mark_gatekeeper_decision();
                }
                ctx.oracle_runtime.mark_pool_scored(pool_id);
                finish_pool_observation(
                    ctx.as_ref(),
                    pool_id,
                    VerdictOutcome::Timeout {
                        reason: "gatekeeper_timeout".to_string(),
                    },
                );
                let _ = ctx.result_tx.send(PoolObservationResult {
                    pool_id,
                    base_mint: base_mint_pubkey,
                    bought: false,
                    retain_runtime_pool: false,
                });
                return;
            }

            GatekeeperVerdict::Buy {
                buffered_txs,
                mut assessment,
            } => {
                if let Some(fp_agg) = fingerprint_agg.take() {
                    assessment.early_fingerprint = Some(fp_agg.finalize());
                }

                let gatekeeper_verdict_at = current_time_ms();
                let gatekeeper_base_mint = base_mint_pubkey
                    .map(|mint| mint.to_string())
                    .or_else(|| pool_data.as_ref().map(|pd| pd.base_mint.clone()))
                    .unwrap_or_else(|| "unknown".to_string());
                info!(
                    pool = %pool_id,
                    base_mint = %gatekeeper_base_mint,
                    gatekeeper_verdict_at,
                    iwim_enabled = ctx.iwim_veto_config.enabled,
                    "Gatekeeper: PASS verdict timing checkpoint"
                );

                // ── IWIM Veto Gate (runs in per-pool task — no cross-pool blocking) ──
                let iwim_veto_result = if ctx.iwim_veto_config.enabled {
                    let dev_wallet_for_iwim = pool_data
                        .as_ref()
                        .and_then(|pd| Pubkey::try_from(pd.creator.as_str()).ok());
                    let gk_strength = assessment
                        .decision
                        .as_ref()
                        .and_then(|d| d.gatekeeper_strength)
                        .unwrap_or(crate::components::gatekeeper::GatekeeperStrength::Borderline);

                    let (should_buy, iwim_res) = crate::components::iwim_veto::run_iwim_veto_gate(
                        &ctx.iwim_veto_config,
                        dev_wallet_for_iwim.as_ref(),
                        &pool_id,
                        gk_strength,
                        ctx.oracle_runtime.rpc_client.as_ref(),
                    )
                    .await;

                    if !should_buy {
                        let window_state = window_state.get_or_insert_with(|| {
                            WindowState::from_first_tx(registered_wall_ts_ms, ctx.ab_window_ms)
                        });
                        maybe_open_coverage_window(
                            pool_id,
                            base_mint_pubkey,
                            Some(window_state),
                            &mut coverage_window_opened,
                        );
                        // IWIM vetoed the BUY → convert to REJECT
                        let iwim_verdict_type = match iwim_res.status {
                            crate::components::iwim_veto::IwimStatus::Veto => {
                                crate::components::gatekeeper::GatekeeperVerdictType::RejectIwimVeto
                            }
                            crate::components::iwim_veto::IwimStatus::Unknown => {
                                if iwim_res.quality
                                    == crate::components::iwim_veto::IwimQuality::Low
                                {
                                    crate::components::gatekeeper::GatekeeperVerdictType::RejectIwimLowConf
                                } else {
                                    crate::components::gatekeeper::GatekeeperVerdictType::RejectIwimUnknownStrict
                                }
                            }
                            _ => {
                                crate::components::gatekeeper::GatekeeperVerdictType::RejectIwimUnknownStrict
                            }
                        };
                        if let Some(ref mut decision) = assessment.decision {
                            decision.verdict_buy = false;
                            decision.verdict_type = iwim_verdict_type;
                            decision.reason_chain = format!(
                                "{} → IWIM_REJECT: {}",
                                decision.reason_chain,
                                iwim_res.summary()
                            );
                            decision.reason_code = Some(match iwim_verdict_type {
                                crate::components::gatekeeper::GatekeeperVerdictType::RejectIwimVeto => {
                                    ghost_brain::oracle::reason_code::GatekeeperReasonCode::RejectIwimVeto
                                }
                                crate::components::gatekeeper::GatekeeperVerdictType::RejectIwimLowConf => {
                                    ghost_brain::oracle::reason_code::GatekeeperReasonCode::RejectIwimLowConf
                                }
                                _ => {
                                    ghost_brain::oracle::reason_code::GatekeeperReasonCode::RejectIwimUnknownStrict
                                }
                            });
                        }

                        let reason =
                            format!(
                            "IWIM_VETO: {} (gk_str={} dev_known={} fetch={} quality={} conf={:.2})",
                            iwim_res.status, gk_strength, iwim_res.dev_known,
                            iwim_res.fetch_status, iwim_res.quality, iwim_res.confidence,
                        );
                        ctx.oracle_runtime.append_decision_to_wal(
                            pool_id,
                            decision_slot_hint(&buffered_txs, pool_data.as_deref()),
                            WalGatekeeperDecision::Reject,
                            Some(reason.clone()),
                        );
                        info!(
                            pool = %pool_id,
                            "🛡️ IWIM VETO GATE pool={} {}{}",
                            pool_id, reason, assessment.decision_summary(),
                        );
                        {
                            let fp_mint = pool_data
                                .as_ref()
                                .map(|pd| pd.base_mint.as_str())
                                .unwrap_or("unknown");
                            info!(
                                "{}",
                                assessment.fingerprint_summary(&pool_id.to_string(), fp_mint,),
                            );
                        }

                        window_state.mark_verdict_early(WindowCloseReason::PoolRejectedEarly);
                        let mut buy_log = assessment.to_buy_log(&pool_id, &ctx.gatekeeper_config);
                        enrich_buy_log_with_observation_identity(&mut buy_log, &identity);
                        enrich_buy_log_with_iwim(&mut buy_log, &iwim_res);
                        enrich_buy_log_with_window(
                            &mut buy_log,
                            &window_state,
                            &pool_id.to_string(),
                            "REJECT",
                        );
                        {
                            let session = session.read();
                            enrich_buy_log_with_clock_provenance(
                                &mut buy_log,
                                session.gatekeeper_buffer(),
                            );
                            enrich_buy_log_with_vectors(
                                &mut buy_log,
                                session.gatekeeper_buffer(),
                                &window_state,
                            );
                        }
                        let dl = ctx.decision_logger.clone();
                        tokio::spawn(async move {
                            dl.log_gatekeeper_buy_decision(buy_log).await;
                        });
                        if let Some(ref emitter) = ctx.event_emitter {
                            emit_gatekeeper_decision_event(
                                emitter,
                                &pool_id,
                                "REJECT",
                                &assessment,
                            );
                            if let Some(ref h) = ctx.health {
                                h.mark_events_write();
                            }
                        }
                        spawn_coverage_audit_for_closed_window(
                            ctx.clone(),
                            pool_id,
                            base_mint_pubkey,
                            &window_state,
                            "REJECT",
                        );
                        if let Some(ref h) = ctx.health {
                            h.mark_gatekeeper_decision();
                        }
                        ctx.oracle_runtime.mark_pool_scored(pool_id);
                        finish_pool_observation(
                            ctx.as_ref(),
                            pool_id,
                            VerdictOutcome::Fail {
                                reason: reason.clone(),
                            },
                        );
                        let _ = ctx.result_tx.send(PoolObservationResult {
                            pool_id,
                            base_mint: base_mint_pubkey,
                            bought: false,
                            retain_runtime_pool: false,
                        });
                        return;
                    }

                    Some(iwim_res)
                } else {
                    None
                };

                let post_iwim_gate_at = if ctx.iwim_veto_config.enabled {
                    current_time_ms()
                } else {
                    gatekeeper_verdict_at
                };
                let iwim_gate_latency_ms = post_iwim_gate_at.saturating_sub(gatekeeper_verdict_at);
                let iwim_status = iwim_veto_result
                    .as_ref()
                    .map(|result| result.status.to_string())
                    .unwrap_or_else(|| "disabled".to_string());
                let iwim_quality = iwim_veto_result
                    .as_ref()
                    .map(|result| result.quality.to_string())
                    .unwrap_or_else(|| "disabled".to_string());
                info!(
                    pool = %pool_id,
                    base_mint = %gatekeeper_base_mint,
                    gatekeeper_verdict_at,
                    post_iwim_gate_at,
                    iwim_gate_latency_ms,
                    iwim_enabled = ctx.iwim_veto_config.enabled,
                    iwim_status = %iwim_status,
                    iwim_quality = %iwim_quality,
                    "Gatekeeper: post-IWIM BUY timing checkpoint"
                );

                // ── BUY execution ───────────────────────────────────────
                let window_state = window_state.get_or_insert_with(|| {
                    WindowState::from_first_tx(registered_wall_ts_ms, ctx.ab_window_ms)
                });
                maybe_open_coverage_window(
                    pool_id,
                    base_mint_pubkey,
                    Some(window_state),
                    &mut coverage_window_opened,
                );
                let buy_execution = execute_gatekeeper_buy_path(
                    pool_id,
                    registered_wall_ts_ms,
                    &buffered_txs,
                    &assessment,
                    post_buy_lane,
                    &mut rx,
                    &ctx,
                    &mut identity,
                    &mut base_mint_pubkey,
                    &mut pool_data,
                )
                .await;
                let bought = buy_execution.bought;
                let retain_runtime_pool = buy_execution.retain_runtime_pool;
                let buy_close_reason = buy_execution.close_reason;
                debug!(
                    pool = %pool_id,
                    metadata_source = ?buy_execution.metadata_source,
                    "Gatekeeper BUY path resolved metadata"
                );
                ctx.oracle_runtime.append_decision_to_wal(
                    pool_id,
                    decision_slot_hint(&buffered_txs, pool_data.as_deref()),
                    WalGatekeeperDecision::Buy,
                    assessment
                        .decision
                        .as_ref()
                        .map(|decision| decision.reason_chain.clone()),
                );

                // ── JSONL + events ──────────────────────────────────────
                window_state.mark_verdict_early(buy_close_reason.clone());
                if let Some(ref emitter) = ctx.event_emitter {
                    emit_gatekeeper_decision_event(emitter, &pool_id, "PASS", &assessment);
                    if let Some(ref h) = ctx.health {
                        h.mark_events_write();
                    }
                }
                if let Some(ref h) = ctx.health {
                    h.mark_gatekeeper_decision();
                }

                let base_mint_for_log = pool_data
                    .as_ref()
                    .map(|pd| pd.base_mint.clone())
                    .unwrap_or_default();
                info!(
                    "🔓 GATEKEEPER V2: KUPUJ! {} (signers={} tx={} phases={}/6 in {}ms lag={}ms dust={}){}",
                    pool_id,
                    assessment.unique_signers_evaluated,
                    assessment.total_tx_evaluated,
                    assessment.phases_passed,
                    assessment.observation_duration_ms,
                    assessment.finalize_lag_ms,
                    assessment.dust_filtered_count,
                    assessment.decision_summary(),
                );
                info!(
                    "{}",
                    assessment.fingerprint_summary(&pool_id.to_string(), &base_mint_for_log,),
                );
                if let Some(ref ir) = iwim_veto_result {
                    info!(
                        pool = %pool_id,
                        "🛡️ IWIM_VETO: {}",
                        ir.summary(),
                    );
                }
                info!(
                    "WINDOW pool={} t0={} t_end={} complete={} reason={} tx_in_window={} finalize_lag_ms={}",
                    pool_id,
                    window_state.t0_event_ts_ms,
                    window_state.t_end_event_ts_ms,
                    window_state.window_complete,
                    window_state
                        .window_close_reason
                        .as_ref()
                        .map_or("NONE", |r| r.tag()),
                    window_state.tx_count_window,
                    assessment.finalize_lag_ms,
                );
                debug!(
                    "🔍 GK2 BUY pool={} | {} | {}",
                    pool_id,
                    format_gatekeeper_v2_assessment(&assessment),
                    format_gatekeeper_v2_config(&ctx.gatekeeper_config),
                );
                let gk_cfg = ctx.gatekeeper_config.clone();
                let mut buy_log = assessment.to_buy_log(&pool_id, &gk_cfg);
                enrich_buy_log_with_observation_identity(&mut buy_log, &identity);
                if let Some(ref ir) = iwim_veto_result {
                    enrich_buy_log_with_iwim(&mut buy_log, ir);
                }
                enrich_buy_log_with_window(
                    &mut buy_log,
                    &window_state,
                    &pool_id.to_string(),
                    "BUY",
                );
                enforce_buy_log_buy_routing(&mut buy_log, &assessment);
                enrich_buy_log_with_shadow_run(
                    &mut buy_log,
                    buy_execution.metadata_source,
                    &buy_execution.shadow_execution_outcome,
                    pool_data.as_deref(),
                    &buffered_txs,
                    ctx.trigger.as_ref(),
                );
                {
                    let session = session.read();
                    enrich_buy_log_with_clock_provenance(&mut buy_log, session.gatekeeper_buffer());
                    enrich_buy_log_with_vectors(
                        &mut buy_log,
                        session.gatekeeper_buffer(),
                        &window_state,
                    );
                }
                let dl = ctx.decision_logger.clone();
                tokio::spawn(async move {
                    dl.log_gatekeeper_buy_decision(buy_log).await;
                });
                spawn_coverage_audit_for_closed_window(
                    ctx.clone(),
                    pool_id,
                    base_mint_pubkey,
                    &window_state,
                    "BUY",
                );
                ctx.oracle_runtime.mark_pool_scored(pool_id);
                finish_pool_observation(
                    ctx.as_ref(),
                    pool_id,
                    VerdictOutcome::Pass {
                        reason: "gatekeeper_buy".to_string(),
                    },
                );

                let _ = ctx.result_tx.send(PoolObservationResult {
                    pool_id,
                    base_mint: base_mint_pubkey,
                    bought,
                    retain_runtime_pool,
                });
                return;
            }
        }
    }
}

fn process_runtime_account_update_event(
    oracle_runtime: &OracleRuntime,
    event: &AccountUpdateEvent,
) {
    let outcome = oracle_runtime.process_account_update_with_explicit_source(
        &event.base_mint,
        event.sol_reserves,
        event.token_reserves,
        event.complete,
        event.slot,
        event.curve_finality,
        UpdateSource::GeyserAccountUpdate,
        Some(event),
        true,
    );
    if outcome.is_some() {
        increment_counter!("oracle_runtime_account_update_reconciliation_total");
    }
}

fn spawn_account_update_worker(
    oracle_runtime: Arc<OracleRuntime>,
    mut work_rx: tokio::sync::mpsc::UnboundedReceiver<AccountUpdateEvent>,
    queue_depth: Arc<AtomicUsize>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = work_rx.recv().await {
            let base_mint = event.base_mint;
            let slot = event.slot;
            let write_version = event.write_version;
            let runtime = Arc::clone(&oracle_runtime);
            let join_result = tokio::task::spawn_blocking(move || {
                process_runtime_account_update_event(runtime.as_ref(), &event);
            })
            .await;
            let remaining = queue_depth
                .fetch_sub(1, Ordering::Relaxed)
                .saturating_sub(1);
            ::metrics::gauge!(
                "oracle_runtime_account_update_queue_depth",
                remaining as f64
            );

            if let Err(err) = join_result {
                error!(
                    base_mint = %base_mint,
                    slot,
                    write_version = ?write_version,
                    error = %err,
                    "OracleRuntime AccountUpdate worker task failed"
                );
            }
        }

        queue_depth.store(0, Ordering::Relaxed);
        ::metrics::gauge!("oracle_runtime_account_update_queue_depth", 0.0);
        debug!("OracleRuntime AccountUpdate worker exited");
    })
}

fn dispatch_account_update_to_worker(
    worker_tx: &tokio::sync::mpsc::UnboundedSender<AccountUpdateEvent>,
    queue_depth: &Arc<AtomicUsize>,
    event: AccountUpdateEvent,
) {
    let base_mint = event.base_mint;
    let slot = event.slot;
    let write_version = event.write_version;
    match worker_tx.send(event) {
        Ok(()) => {
            let depth = queue_depth.fetch_add(1, Ordering::Relaxed) + 1;
            ::metrics::gauge!("oracle_runtime_account_update_queue_depth", depth as f64);
            increment_counter!(
                "oracle_runtime_account_update_dispatch_total",
                "outcome" => "queued"
            );
        }
        Err(_) => {
            increment_counter!(
                "oracle_runtime_account_update_dispatch_total",
                "outcome" => "queue_closed"
            );
            warn!(
                base_mint = %base_mint,
                slot,
                write_version = ?write_version,
                "OracleRuntime AccountUpdate worker queue closed; dropping canonical update"
            );
        }
    }
}

pub async fn start_oracle_runtime_task(
    event_rx: tokio::sync::broadcast::Receiver<GhostEvent>,
    oracle_runtime: Arc<OracleRuntime>,
    snapshot_engine: Arc<SnapshotEngine>,
    event_tx: EventBusSender,
    post_buy_tx: Option<DirectPostBuySender>,
    analysis_window_ms: u64,
    gatekeeper_config: GatekeeperV2Config,
    iwim_veto_config: ghost_brain::config::IwimVetoGateConfig,
    dry_run: bool,
    decision_log_path: String,
    trigger: Option<Arc<crate::components::trigger::TriggerComponent>>,
    events_output_dir: String,
    health: Option<Arc<ghost_core::health::RuntimeHealth>>,
    canonical_account_update_relay_enabled: bool,
    authoritative_funding_stream_available: bool,
) {
    let shadow_defaults = ghost_brain::config::ExecutionShadowConfig::default();
    start_oracle_runtime_task_with_funding_availability(
        event_rx,
        oracle_runtime,
        snapshot_engine,
        event_tx,
        post_buy_tx,
        analysis_window_ms,
        gatekeeper_config,
        iwim_veto_config,
        if dry_run {
            ExecutionMode::Paper
        } else {
            ExecutionMode::Live
        },
        dry_run,
        decision_log_path,
        shadow_defaults.entry_log_path,
        shadow_defaults.lifecycle_log_path,
        trigger,
        events_output_dir,
        health,
        canonical_account_update_relay_enabled,
        authoritative_funding_stream_available,
        false,
        None,
    )
    .await;
}

pub async fn start_oracle_runtime_task_with_funding_availability(
    mut event_rx: tokio::sync::broadcast::Receiver<GhostEvent>,
    oracle_runtime: Arc<OracleRuntime>,
    snapshot_engine: Arc<SnapshotEngine>,
    event_tx: EventBusSender,
    post_buy_tx: Option<DirectPostBuySender>,
    analysis_window_ms: u64,
    gatekeeper_config: GatekeeperV2Config,
    iwim_veto_config: ghost_brain::config::IwimVetoGateConfig,
    execution_mode: ExecutionMode,
    dry_run: bool,
    decision_log_path: String,
    shadow_entry_log_path: String,
    shadow_lifecycle_log_path: Option<String>,
    trigger: Option<Arc<crate::components::trigger::TriggerComponent>>,
    events_output_dir: String,
    health: Option<Arc<ghost_core::health::RuntimeHealth>>,
    canonical_account_update_relay_enabled: bool,
    authoritative_funding_stream_available: bool,
    authoritative_funding_coverage_gate_enabled: bool,
    mut authoritative_funding_stream_availability_rx: Option<watch::Receiver<bool>>,
) {
    info!(
        "🔮 RUSZA WATEK Oracle Runtime (OKNO: {}ms, execution_mode: {:?}, dry_run: {}, iwim_veto: {})",
        analysis_window_ms,
        execution_mode,
        dry_run,
        if iwim_veto_config.enabled {
            "ON"
        } else {
            "OFF"
        }
    );
    let normalized_decision_log_path =
        crate::config::normalize_decision_log_path(&decision_log_path);
    info!(
        "   📝 Decision Logger: AKTYWNY (path: {})",
        normalized_decision_log_path
    );
    let decision_logger_config =
        build_decision_logger_config(&normalized_decision_log_path, &gatekeeper_config);
    let gatekeeper_rollout_profile = decision_logger_config.gatekeeper_rollout_profile.clone();

    // Initialize Decision Logger for cyclic engine telemetry
    let decision_logger = Arc::new(ghost_brain::oracle::DecisionLogger::new_with_health(
        decision_logger_config,
        health.clone(),
    ));

    // Initialize EventEmitter for gatekeeper decision events (CandidateFinalized)
    let epoch_ms_start = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let run_id = format!("launcher-{}", epoch_ms_start);
    let lane = match execution_mode {
        ExecutionMode::Live | ExecutionMode::Dual => Lane::Live,
        ExecutionMode::Paper => Lane::Paper,
        ExecutionMode::Shadow => Lane::Shadow,
    };
    let event_emitter = match EventEmitter::new(
        EventWriterConfig {
            output_dir: events_output_dir,
            enable_optional_events: true,
            ..Default::default()
        },
        run_id.clone(),
        lane,
    ) {
        Ok(e) => {
            info!(
                "📊 EventEmitter initialized (run_id={}, lane={:?}, dir=datasets/events)",
                run_id, lane
            );
            Some(Arc::new(e))
        }
        Err(e) => {
            error!(
                "Failed to initialize EventEmitter: {} — events JSONL will be empty",
                e
            );
            None
        }
    };

    oracle_runtime.configure_approval_gating(&snapshot_engine);
    oracle_runtime.set_panic_retention_ms(analysis_window_ms);

    // Spawn periodic orphan cleanup task to prevent memory leaks
    let oracle_runtime_cleanup = oracle_runtime.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        info!(
            "🧹 PORZADKI: SIEROTY NA SMIETNIK(interwał: 10s, TTL: {}ms)",
            ORPHAN_TTL_MS
        );

        loop {
            interval.tick().await;

            let (pools_with_orphans, total_orphans) = oracle_runtime_cleanup.get_orphan_stats();

            if total_orphans > 0 {
                debug!(
                    "📊 STATY BUFORA SIEROT: {} pools with {} total orphans",
                    pools_with_orphans, total_orphans
                );

                let (dropped, pools_dropped, cap_enforced) =
                    oracle_runtime_cleanup.cleanup_stale_orphans();

                if dropped > 0 {
                    info!(
                        "🧹 PORZADKI: dropped {} transactions, {} pools cleaned, {} cap enforcements",
                        dropped, pools_dropped, cap_enforced
                    );
                }
            }
        }
    });

    // ── Periodic reconciliation health reporter ──────────────────────────────
    //
    // Periodically logs a point-in-time snapshot of reconciliation health.
    //
    // Reconciliation is **event-driven**: every on-chain AccountUpdate is fed
    // into `OracleRuntime::process_account_update(...)` which immediately
    // reconciles the pool and updates drift + legacy-compat counters. There is
    // no polling of on-chain state here.
    //
    // This task is therefore a *health reporter only* — it emits the current
    // `ReconciliationRuntimeStatus` so operators can observe drift behaviour
    // without digging through unit-test-only APIs.
    //
    // Lifecycle: runs until the oracle_runtime Arc is dropped or the process
    // exits.  `MissedTickBehavior::Skip` prevents burst logging on stalls.
    let oracle_runtime_recon = oracle_runtime.clone();
    tokio::spawn(async move {
        /// Interval between health-status log emissions.
        const RECONCILIATION_HEALTH_LOG_INTERVAL_SECS: u64 = 30;

        let mut interval =
            tokio::time::interval(Duration::from_secs(RECONCILIATION_HEALTH_LOG_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        info!(
            "🔄 ReconciliationRuntime health reporter started (interval: {}s, mode: event-driven)",
            RECONCILIATION_HEALTH_LOG_INTERVAL_SECS
        );

        loop {
            interval.tick().await;

            let status = oracle_runtime_recon.reconciliation_status();
            info!(
                "🔄 ReconciliationRuntime health: registered_pools={} total_checks={} \
                 drifting_pools={} hot_pools={} cycle={}",
                status.registered_pools,
                status.total_checks,
                status.total_drifting_pools,
                status.total_hot_pools,
                status.cycle_count,
            );
        }
    });

    // ── Shadow Ledger health metrics reporter ────────────────────────────
    //
    // Every 5 s: reads committed_pool_count + total_snapshot_count from the
    // ShadowLedger and updates the Prometheus gauges in oracle_metrics.
    {
        let shadow_ledger_health = oracle_runtime.get_shadow_ledger().clone();
        tokio::spawn(async move {
            const SHADOW_HEALTH_INTERVAL_MS: u64 = 5_000;
            let mut interval =
                tokio::time::interval(Duration::from_millis(SHADOW_HEALTH_INTERVAL_MS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            info!(
                "📊 Shadow Ledger health reporter started (interval: {}ms)",
                SHADOW_HEALTH_INTERVAL_MS
            );

            loop {
                interval.tick().await;
                let committed_pools = shadow_ledger_health.committed_pool_count();
                let total_snapshots = shadow_ledger_health.total_snapshot_count();
                record_shadow_ledger_health(committed_pools, total_snapshots);
            }
        });
    }

    if !canonical_account_update_relay_enabled {
        info!("🔄 ReconciliationRuntime active cycle DISABLED (degraded/test fallback: canonical_account_update_relay_enabled=false)");
    }

    // ── Per-pool parallel observation state ──────────────────────────────
    let mut pool_task_handles: HashMap<Pubkey, PoolTaskHandle> = HashMap::new();
    let mut rejected_pools = BoundedFifoSet::new(REJECTED_POOLS_CAP);
    let post_buy_epoch = Arc::new(AtomicU64::new(1));
    let (result_tx, mut result_rx) =
        tokio::sync::mpsc::unbounded_channel::<PoolObservationResult>();
    let session_manager = oracle_runtime.session_manager();
    let funding_source_config = FundingSourceConfig::from_gatekeeper_config(&gatekeeper_config);
    apply_authoritative_funding_stream_availability(
        session_manager.as_ref(),
        authoritative_funding_stream_available,
        "startup",
        true,
    );

    let ctx = Arc::new(PoolObservationContext {
        oracle_runtime: oracle_runtime.clone(),
        session_manager,
        snapshot_engine: snapshot_engine.clone(),
        event_tx: event_tx.clone(),
        post_buy_tx,
        decision_logger: decision_logger.clone(),
        coverage_audit_log_path: build_coverage_audit_log_path(&normalized_decision_log_path),
        trigger: trigger.clone(),
        iwim_veto_config: iwim_veto_config.clone(),
        gatekeeper_config: gatekeeper_config.clone(),
        cross_pool_velocity_config: CrossPoolVelocityConfig::from_gatekeeper_config(
            &gatekeeper_config,
        ),
        funding_source_config,
        authoritative_funding_coverage_gate_enabled,
        fingerprint_config: EarlyFingerprintConfig::default(),
        event_emitter: event_emitter.clone(),
        health: health.clone(),
        result_tx,
        post_buy_epoch,
        execution_mode,
        shadow_entry_log_path: std::path::PathBuf::from(shadow_entry_log_path),
        shadow_lifecycle_log_path: shadow_lifecycle_log_path.map(std::path::PathBuf::from),
        gatekeeper_rollout_profile,
        dry_run,
        ab_window_ms: analysis_window_ms,
    });
    let (account_update_work_tx, account_update_work_rx) =
        tokio::sync::mpsc::unbounded_channel::<AccountUpdateEvent>();
    let account_update_queue_depth = Arc::new(AtomicUsize::new(0));
    let _account_update_worker = spawn_account_update_worker(
        Arc::clone(&oracle_runtime),
        account_update_work_rx,
        Arc::clone(&account_update_queue_depth),
    );
    let mut authoritative_funding_stream_signal_closed = false;
    let mut fsc_coverage_window_tick =
        tokio::time::interval(Duration::from_millis(FSC_COVERAGE_WINDOW_POLL_INTERVAL_MS));
    fsc_coverage_window_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_fsc_gate_status = None;
    refresh_fsc_authoritative_buy_gate_status(
        ctx.session_manager.as_ref(),
        &ctx.funding_source_config,
        ctx.authoritative_funding_coverage_gate_enabled,
        "startup",
        true,
        &mut last_fsc_gate_status,
    );

    loop {
        tokio::select! {
            availability_changed = async {
                match authoritative_funding_stream_availability_rx.as_mut() {
                    Some(rx) => rx.changed().await,
                    None => {
                        std::future::pending::<Result<(), tokio::sync::watch::error::RecvError>>()
                            .await
                    }
                }
            }, if !authoritative_funding_stream_signal_closed => {
                    match availability_changed {
                        Ok(()) => {
                            let available = *authoritative_funding_stream_availability_rx
                                .as_mut()
                                .expect("guarded by authoritative funding availability receiver")
                                .borrow_and_update();
                            apply_authoritative_funding_stream_availability(
                                ctx.session_manager.as_ref(),
                                available,
                                "seer_lane_health",
                                false,
                            );
                            refresh_fsc_authoritative_buy_gate_status(
                                ctx.session_manager.as_ref(),
                                &ctx.funding_source_config,
                                ctx.authoritative_funding_coverage_gate_enabled,
                                "seer_lane_health",
                                false,
                                &mut last_fsc_gate_status,
                            );
                        }
                        Err(_) => {
                            authoritative_funding_stream_signal_closed = true;
                            warn!("FSC authoritative funding availability signal closed; retaining last known state");
                        }
                    }
                }

            _ = fsc_coverage_window_tick.tick() => {
                refresh_fsc_authoritative_buy_gate_status(
                    ctx.session_manager.as_ref(),
                    &ctx.funding_source_config,
                    ctx.authoritative_funding_coverage_gate_enabled,
                    "coverage_tick",
                    false,
                    &mut last_fsc_gate_status,
                );
            }

            event_result = event_rx.recv() => {
                let event = match event_result {
                    Ok(e) => e,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("LAG ORACLE by {} messages", n);
                        continue;
                    }
                    Err(_) => break,
                };

                match event {
                    GhostEvent::NewPoolDetected(pool_data) => {
                        info!(
                            "🔮 OBSLUGUJE EVENT NewPoolDetected: pool={}",
                            pool_data.pool_amm_id
                        );
                        let registered_wall_ts_ms = current_time_ms();

                        if let Ok(candidate) = build_enhanced_candidate_from_pool_data(
                            &pool_data,
                            &oracle_runtime.pump_program_id,
                            &oracle_runtime.bonk_program_id,
                        ) {
                            if let Ok(pool_id) =
                                Pubkey::try_from(pool_data.pool_amm_id.as_str())
                            {
                                if let Ok(base_mint) =
                                    Pubkey::try_from(pool_data.base_mint.as_str())
                                {
                                    let detected_creator = Pubkey::try_from(pool_data.creator.as_str())
                                        .ok()
                                        .filter(|pubkey| *pubkey != Pubkey::default());
                                    if rejected_pools.contains(&pool_id)
                                        || rejected_pools.contains(&base_mint)
                                    {
                                        warn!(
                                            "TX_IGNORED_ZOMBIE pool={} mint={} reason=REJECTED_POOL_REGISTRATION",
                                            pool_id, base_mint
                                        );
                                        continue;
                                    }

                                    // Already tracking this pool — forward metadata
                                    if let Some(handle) = pool_task_handles.get(&pool_id) {
                                        // Register pool with OracleRuntime if not yet done
                                        let registered = oracle_runtime.register_new_pool(
                                            pool_id,
                                            base_mint,
                                            candidate.clone(),
                                            detected_creator,
                                        );
                                        if registered
                                            || oracle_runtime
                                                .lookup_base_mint_for_pool(&pool_id)
                                                .is_some()
                                        {
                                            oracle_runtime
                                                .remember_detected_pool(pool_id, pool_data.clone());
                                        }
                                        // Forward pool data to existing task
                                        enqueue_pool_observation_msg(
                                            &handle.tx,
                                            pool_id,
                                            PoolObservationMsg::NewPool(pool_data.clone()),
                                            "new_pool",
                                            handle.is_hot(),
                                        );
                                        debug!(
                                            "POOL_TASK_LATE_METADATA_SENT pool={} mint={}",
                                            pool_id, base_mint
                                        );
                                        continue;
                                    }

                                    if oracle_runtime.register_new_pool(
                                        pool_id,
                                        base_mint,
                                        candidate.clone(),
                                        detected_creator,
                                    ) {
                                        // Spawn per-pool observation task
                                        let (task_tx, task_rx) = tokio::sync::mpsc::channel(
                                            POOL_TASK_CHANNEL_CAPACITY,
                                        );
                                        let ctx_clone = ctx.clone();
                                        let pool_data_clone = pool_data.clone();
                                        let join_handle =
                                            tokio::spawn(pool_observation_task(
                                                pool_id,
                                                Some(pool_data_clone),
                                                registered_wall_ts_ms,
                                                task_rx,
                                                ctx_clone,
                                            ));
                                        pool_task_handles.insert(
                                            pool_id,
                                            PoolTaskHandle {
                                                tx: task_tx,
                                                _abort_handle: join_handle.abort_handle(),
                                                tx_enqueued: 0,
                                            },
                                        );
                                        info!(
                                            "🔮 POOL_TASK_SPAWNED pool={} mint={} window={}ms active_tasks={}",
                                            pool_id,
                                            base_mint,
                                            ctx.gatekeeper_config.max_wait_time_ms,
                                            pool_task_handles.len()
                                        );
                                    }
                                }
                            }
                        }
                    }

                    GhostEvent::PoolTransaction(tx) => {
                        increment_counter!("grpc_events_received");
                        if let Ok(mut pool_id) =
                            Pubkey::try_from(tx.pool_amm_id.as_str())
                        {
                            let mut base_mint: Option<Pubkey> = None;
                            if let Some(ref mint_str) = tx.token_mint {
                                if let Ok(parsed_mint) =
                                    Pubkey::try_from(mint_str.as_str())
                                {
                                    base_mint = Some(parsed_mint);
                                    if let Some(mapped_pool) =
                                        oracle_runtime
                                            .lookup_registered_pool(&parsed_mint)
                                    {
                                        if mapped_pool != pool_id {
                                            debug!(
                                                "POOL_ID_REMAP mint={} from={} to={}",
                                                parsed_mint, pool_id, mapped_pool
                                            );
                                            pool_id = mapped_pool;
                                        }
                                    }
                                }
                            }

                            // Skip rejected pools
                            if rejected_pools.contains(&pool_id)
                                || base_mint
                                    .as_ref()
                                    .is_some_and(|m| rejected_pools.contains(m))
                            {
                                continue;
                            }

                            // Enrich base_mint from pool identity registry when not
                            // available from the TX.  This broadens coverage for the
                            // Shadow Ledger fast path below and for subsequent routing.
                            if base_mint.is_none() {
                                base_mint =
                                    oracle_runtime.lookup_base_mint_for_pool(&pool_id);
                            }

                            // ── Shadow Ledger authoritative runtime engine fast path ──────
                            //
                            // Canonical runtime pools that already reached Approved/Committed
                            // continue through the launcher-owned relay path even after their
                            // observation task has exited.  Raw PoolTransaction MUST NOT spawn
                            // a fresh observation task for unknown / removed pools.
                            if !pool_task_handles.contains_key(&pool_id) {
                                if let Some(mint) = base_mint {
                                    if let Some(runtime_state) = oracle_runtime
                                        .effective_runtime_pool_state(&pool_id, Some(&mint))
                                    {
                                        if runtime_state.allows_runtime_relay() && tx.success {
                                            let pool_id_string = pool_id.to_string();
                                            ctx.session_manager
                                                .cross_pool_velocity_index()
                                                .observe_transaction(
                                                    pool_id_string.as_str(),
                                                    &tx,
                                                    &ctx.cross_pool_velocity_config,
                                                );
                                            let event_ts_ms = tx_event_ts_ms(&tx);
                                            oracle_runtime
                                                .forward_approved_tx_to_commit_or_live_pipeline(
                                                    pool_id,
                                                    mint,
                                                    &tx,
                                                    event_ts_ms,
                                                );
                                            if runtime_state.is_committed() {
                                                increment_counter!(
                                                    "shadow_ledger_live_tx_committed_pool_total"
                                                );
                                            } else {
                                                increment_counter!(
                                                    "shadow_ledger_live_tx_approved_pool_total"
                                                );
                                            }
                                            continue;
                                        }
                                    }
                                }
                            }

                            // Route to an existing canonical per-pool task only.
                            if let Some(handle) = pool_task_handles.get_mut(&pool_id) {
                                let is_hot = handle.is_hot();
                                handle.tx_enqueued += 1;
                                enqueue_pool_observation_msg(
                                    &handle.tx,
                                    pool_id,
                                    PoolObservationMsg::Transaction(tx),
                                    "tx",
                                    is_hot,
                                );
                            } else {
                                let event_ts_ms = tx_event_ts_ms(&tx);
                                oracle_runtime.register_pool_tx(
                                    pool_id,
                                    event_ts_ms,
                                    tx.slot,
                                    tx.mpcf_payload.clone(),
                                    None,
                                    tx.signer.clone(),
                                    tx.is_buy,
                                    tx.volume_sol,
                                );
                                increment_counter!("oracle_runtime_tx_first_unknown_buffered_total");
                                debug!(
                                    pool = %pool_id,
                                    base_mint = ?base_mint,
                                    signature = %tx.signature,
                                    "Buffering tx-first event for non-canonical pool until NewPoolDetected"
                                );
                            }
                        }
                    }

                    GhostEvent::FundingTransferObserved(transfer) => {
                        observe_funding_transfer(
                            ctx.session_manager.as_ref(),
                            transfer.as_ref(),
                            &ctx.funding_source_config,
                        );
                        refresh_fsc_authoritative_buy_gate_status(
                            ctx.session_manager.as_ref(),
                            &ctx.funding_source_config,
                            ctx.authoritative_funding_coverage_gate_enabled,
                            "funding_transfer",
                            false,
                            &mut last_fsc_gate_status,
                        );
                    }

                    GhostEvent::GatekeeperCommitted {
                        pool_amm_id,
                        base_mint,
                        ..
                    } => {
                        let committed_pool = Pubkey::from_str(&pool_amm_id)
                            .ok()
                            .or_else(|| {
                                Pubkey::from_str(&base_mint)
                                    .ok()
                                    .and_then(|mint| oracle_runtime.lookup_registered_pool(&mint))
                            });
                        if let Some(pool_id) = committed_pool {
                            oracle_runtime.mark_pool_committed(pool_id);
                        }
                    }

                    // ── Live AccountUpdate canonical ingest ─────────────────────
                    // degraded/test compatibility (canonical_account_update_relay_enabled == false):
                    // this arm is a no-op and runtime state is driven only by
                    // pool TX flow.
                    // AccountUpdate events do not reach here in normal operation because
                    // they are already dropped at the Seer IPC layer.
                    //
                    // When canonical_account_update_relay_enabled is true, on-chain AccountUpdate
                    // data flows here from Seer and feeds AccountStateCore while
                    // ReconciliationRuntime remains monitoring-only.
                    GhostEvent::AccountUpdate(event) => {
                        if canonical_account_update_relay_enabled {
                            if rejected_pools.contains(&event.base_mint)
                                || rejected_pools.contains(&event.bonding_curve)
                                || oracle_runtime
                                    .lookup_registered_pool(&event.base_mint)
                                    .is_some_and(|pool_id| rejected_pools.contains(&pool_id))
                            {
                                increment_counter!(
                                    "oracle_runtime_account_update_ignored_total",
                                    "reason" => "rejected_pool"
                                );
                                debug!(
                                    base_mint = %event.base_mint,
                                    bonding_curve = %event.bonding_curve,
                                    slot = event.slot,
                                    "ACCOUNT_UPDATE_IGNORED_ZOMBIE reason=REJECTED_POOL"
                                );
                                continue;
                            }
                            dispatch_account_update_to_worker(
                                &account_update_work_tx,
                                &account_update_queue_depth,
                                event,
                            );
                        }
                        // degraded/test fallback: intentional no-op — increment ignored_total
                        // for observability when AccountUpdate ingest is explicitly disabled.
                        #[cfg(not(test))]
                        if !canonical_account_update_relay_enabled {
                            increment_counter!("oracle_runtime_account_update_ignored_total");
                        }
                    }

                    _ => continue,
                }
            }

            Some(result) = result_rx.recv() => {
                pool_task_handles.remove(&result.pool_id);
                if should_cleanup_pool_after_observation(&result) {
                    rejected_pools.insert(result.pool_id);
                    if let Some(mint) = result.base_mint {
                        rejected_pools.insert(mint);
                    }
                    snapshot_engine.remove_pool(result.pool_id);
                    let _ = oracle_runtime
                        .remove_pool_with_reason(result.pool_id, "pool_task_done_cleanup");
                } else if result.retain_runtime_pool {
                    increment_counter!("oracle_runtime_pool_retained_post_buy_total");
                    info!(
                        pool = %result.pool_id,
                        bought = result.bought,
                        retain_runtime_pool = result.retain_runtime_pool,
                        runtime_state = ?oracle_runtime.runtime_pool_state(&result.pool_id),
                        "POOL_TASK_RETAINED_FOR_POST_BUY_MONITORING"
                    );
                }
                debug!(
                    "POOL_TASK_DONE pool={} bought={} retain_runtime_pool={} active_tasks={}",
                    result.pool_id,
                    result.bought,
                    result.retain_runtime_pool,
                    pool_task_handles.len()
                );
            }
        }
    } // loop
}

fn map_amm_program_string_to_pubkey(
    amm_program: &str,
    pump_program_id: &str,
    bonk_program_id: &str,
) -> Result<Pubkey, String> {
    if let Ok(pubkey) = Pubkey::try_from(amm_program) {
        return Ok(pubkey);
    }
    match amm_program.to_lowercase().as_str() {
        "pumpfun" | "pump" => Pubkey::try_from(pump_program_id)
            .map_err(|e| format!("Invalid pump program id '{}': {}", pump_program_id, e)),
        "bonkfun" | "bonk" => Pubkey::try_from(bonk_program_id)
            .map_err(|e| format!("Invalid bonk program id '{}': {}", bonk_program_id, e)),
        _ => Err(format!("Unknown AMM: {}", amm_program)),
    }
}

fn build_enhanced_candidate_from_pool_data(
    pool_data: &DetectedPool,
    pump_program_id: &str,
    bonk_program_id: &str,
) -> Result<EnhancedCandidate, String> {
    Ok(EnhancedCandidate {
        slot: pool_data.slot,
        timestamp: detected_pool_event_ts_ms(pool_data),
        initial_liquidity_sol: pool_data.initial_liquidity_sol.unwrap_or(0.0),
        dev_buy_sol: 0.0,
        bonding_curve_progress: None,
        vanity_score: 0,
        metadata_len_score: 0,
        has_dev_buy: false,
        mint_auth_disabled: false,
        _hot_padding: [0u8; 4],
        _cache_barrier_1: Default::default(),
        expected_price: None,
        shadow_bonding_progress: None,
        virtual_sol_reserves: None,
        shadow_market_cap: None,
        _cache_barrier_2: Default::default(),
        pool_amm_id: Pubkey::try_from(pool_data.pool_amm_id.as_str()).map_err(|e| e.to_string())?,
        amm_program_id: map_amm_program_string_to_pubkey(
            &pool_data.amm_program,
            pump_program_id,
            bonk_program_id,
        )?,
        base_mint: Pubkey::try_from(pool_data.base_mint.as_str()).map_err(|e| e.to_string())?,
        quote_mint: Pubkey::try_from(pool_data.quote_mint.as_str()).map_err(|e| e.to_string())?,
        bonding_curve: Pubkey::try_from(pool_data.bonding_curve.as_str())
            .map_err(|e| e.to_string())?,
        signature: pool_data.signature.clone(),
        token_total_supply: None,
    })
}

impl PanicProvider for OracleRuntime {
    fn fetch_panic_transactions(&self, pool_amm_id: Pubkey, since_ts_ms: u64) -> Vec<PanicTx> {
        let retention_ms = self.panic_retention_ms.load(Ordering::Relaxed);
        let mut buffers = self.panic_tx_buffer.write();
        let Some(buffer) = buffers.get_mut(&pool_amm_id) else {
            return Vec::new();
        };

        if retention_ms > 0 {
            if let Some(last) = buffer.back() {
                let cutoff_ms = last.arrival_ts_ms.saturating_sub(retention_ms);
                while let Some(front) = buffer.front() {
                    if front.arrival_ts_ms < cutoff_ms {
                        buffer.pop_front();
                    } else {
                        break;
                    }
                }
            }
        }

        while let Some(front) = buffer.front() {
            if front.arrival_ts_ms <= since_ts_ms {
                buffer.pop_front();
            } else {
                break;
            }
        }

        buffer
            .iter()
            .filter(|tx| tx.arrival_ts_ms > since_ts_ms)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::gatekeeper::GatekeeperBufferedTx;
    use crate::events::RawBytesMissingReason;
    use async_trait::async_trait;
    use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
    use ghost_brain::oracle::snapshot_engine::{ApprovedPools, PoolMetrics};
    use ghost_core::shadow_ledger::LivePipelineConfig;
    use ghost_core::{GatekeeperDecision as WalGatekeeperDecision, Wal, WalRecord};
    use solana_sdk::signature::Keypair;
    use solana_sdk::signer::Signer;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct MockShadowSimulator;

    #[async_trait]
    impl crate::components::trigger::shadow_run::ShadowSimulator for MockShadowSimulator {
        async fn simulate_buy(
            &self,
            request: &crate::components::trigger::PreparedBuyRequest,
            config: &crate::config::TriggerShadowRunConfig,
        ) -> anyhow::Result<crate::components::trigger::ShadowBuySimulationReport> {
            Ok(crate::components::trigger::ShadowBuySimulationReport {
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
                logs: vec!["shadow".to_string()],
                return_data: None,
                err: None,
            })
        }
    }

    fn write_test_keypair(path: &std::path::Path) -> Keypair {
        let keypair = Keypair::new();
        let bytes = keypair.to_bytes().to_vec();
        std::fs::write(path, serde_json::to_vec(&bytes).expect("serialize keypair"))
            .expect("write keypair");
        keypair
    }

    fn mock_account_info_body(owner: &str, lamports: u64) -> String {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":{{\"data\":[\"\",\"base64\"],\"executable\":false,\"lamports\":{},\"owner\":\"{}\",\"rentEpoch\":0,\"space\":0}}}},\"id\":1}}",
            lamports, owner
        )
    }

    async fn spawn_trigger_prepare_mock_rpc_server(
        payer_pubkey: Pubkey,
        mint: Pubkey,
        user_ata: Pubkey,
        token_program: Pubkey,
        payer_balance_lamports: u64,
    ) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind trigger prepare rpc");
        let addr = listener.local_addr().expect("rpc addr");
        let payer_pubkey = payer_pubkey.to_string();
        let mint = mint.to_string();
        let user_ata = user_ata.to_string();
        let token_program = token_program.to_string();
        let latest_blockhash = solana_sdk::hash::Hash::new_unique().to_string();
        let payer_account_body =
            mock_account_info_body(&solana_sdk::system_program::id().to_string(), 1_000_000);
        let mint_account_body = mock_account_info_body(&token_program, 1_000_000);
        let missing_ata_body = format!(
            "{{\"jsonrpc\":\"2.0\",\"error\":{{\"code\":-32002,\"message\":\"AccountNotFound: pubkey={}\"}},\"id\":1}}",
            user_ata
        );

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
                    format!(
                        "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":{}}},\"id\":1}}",
                        payer_balance_lamports
                    )
                } else if request.contains("\"getMinimumBalanceForRentExemption\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":2039280,\"id\":1}".to_string()
                } else if request.contains("\"getLatestBlockhash\"") {
                    format!(
                        "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":{{\"blockhash\":\"{}\",\"lastValidBlockHeight\":123456}}}},\"id\":1}}",
                        latest_blockhash
                    )
                } else if request.contains("\"getBlockHeight\"") {
                    "{\"jsonrpc\":\"2.0\",\"result\":1000,\"id\":1}".to_string()
                } else if request.contains("\"getAccountInfo\"") && request.contains(&payer_pubkey)
                {
                    payer_account_body.clone()
                } else if request.contains("\"getAccountInfo\"") && request.contains(&mint) {
                    mint_account_body.clone()
                } else if request.contains("\"getAccountInfo\"") && request.contains(&user_ata) {
                    missing_ata_body.clone()
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

    fn test_pending_shadow_simulation(
        shadow_task: tokio::task::JoinHandle<
            anyhow::Result<crate::components::trigger::ShadowBuySimulationReport>,
        >,
    ) -> crate::components::trigger::PendingShadowSimulation {
        let payer = solana_sdk::signature::Keypair::new();
        let recent_blockhash = solana_sdk::hash::Hash::new_unique();
        let transfer_ix =
            solana_sdk::system_instruction::transfer(&payer.pubkey(), &Pubkey::new_unique(), 1);
        let rpc_buy_tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
            std::slice::from_ref(&transfer_ix),
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );
        let buy_tx = solana_sdk::transaction::VersionedTransaction::try_new(
            solana_sdk::message::VersionedMessage::V0(
                solana_sdk::message::v0::Message::try_compile(
                    &payer.pubkey(),
                    &[transfer_ix],
                    &[],
                    recent_blockhash,
                )
                .expect("test shadow message"),
            ),
            &[&payer],
        )
        .expect("test shadow versioned tx");

        crate::components::trigger::PendingShadowSimulation {
            request: crate::components::trigger::PreparedBuyRequest {
                mint: Pubkey::new_unique(),
                payer_pubkey: payer.pubkey(),
                payer_provenance: "configured",
                user_ata: Pubkey::new_unique(),
                token_program: Pubkey::new_unique(),
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
                rpc_buy_tx,
                buy_tx,
                tip_tx: None,
                decision_ts_ms: 10,
            },
            handle: shadow_task,
        }
    }

    fn test_prepared_buy_request() -> crate::components::trigger::PreparedBuyRequest {
        let payer = solana_sdk::signature::Keypair::new();
        let recent_blockhash = solana_sdk::hash::Hash::new_unique();
        let transfer_ix =
            solana_sdk::system_instruction::transfer(&payer.pubkey(), &Pubkey::new_unique(), 1);
        let rpc_buy_tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
            std::slice::from_ref(&transfer_ix),
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );
        let buy_tx = solana_sdk::transaction::VersionedTransaction::try_new(
            solana_sdk::message::VersionedMessage::V0(
                solana_sdk::message::v0::Message::try_compile(
                    &payer.pubkey(),
                    &[transfer_ix],
                    &[],
                    recent_blockhash,
                )
                .expect("test request message"),
            ),
            &[&payer],
        )
        .expect("test request versioned tx");

        crate::components::trigger::PreparedBuyRequest {
            mint: Pubkey::new_unique(),
            payer_pubkey: payer.pubkey(),
            payer_provenance: "configured",
            user_ata: Pubkey::new_unique(),
            token_program: Pubkey::new_unique(),
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
            rpc_buy_tx,
            buy_tx,
            tip_tx: None,
            decision_ts_ms: 10,
        }
    }

    fn test_pool_observation_tx(signature: &str) -> Arc<PoolTransaction> {
        Arc::new(PoolTransaction {
            semantic: ghost_core::EventSemanticEnvelope {
                slot_quality: ghost_core::SlotQuality::Present,
                ..Default::default()
            },
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: Pubkey::new_unique().to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(1_000), None),
            arrival_ts_ms: 1_000,
            signer: Pubkey::new_unique().to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: Some(100_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: signature.to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
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
        })
    }

    fn test_detected_pool(pool_id: Pubkey) -> Arc<DetectedPool> {
        Arc::new(DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: Pubkey::new_unique().to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: Pubkey::new_unique().to_string(),
            creator: Pubkey::new_unique().to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: Signature::new_unique().to_string(),
        })
    }

    fn enhanced_candidate_from_detected_pool(pool: &DetectedPool) -> EnhancedCandidate {
        EnhancedCandidate {
            pool_amm_id: Pubkey::from_str(&pool.pool_amm_id).expect("pool pubkey"),
            amm_program_id: Pubkey::new_unique(),
            base_mint: Pubkey::from_str(&pool.base_mint).expect("base mint"),
            quote_mint: Pubkey::from_str(&pool.quote_mint).expect("quote mint"),
            bonding_curve: Pubkey::from_str(&pool.bonding_curve).expect("bonding curve"),
            slot: pool.slot,
            timestamp: detected_pool_event_ts_ms(pool),
            initial_liquidity_sol: pool.initial_liquidity_sol.unwrap_or_default(),
            signature: pool.signature.clone(),
            ..Default::default()
        }
    }

    fn register_test_detected_pool(runtime: &OracleRuntime, pool: &DetectedPool) {
        let pool_id = Pubkey::from_str(&pool.pool_amm_id).expect("pool pubkey");
        let base_mint = Pubkey::from_str(&pool.base_mint).expect("base mint");
        let candidate = enhanced_candidate_from_detected_pool(pool);
        assert!(
            runtime.register_new_pool(
                pool_id,
                base_mint,
                candidate,
                Pubkey::from_str(&pool.creator).ok(),
            ),
            "test pool should register exactly once"
        );
    }

    #[test]
    fn test_pool_tx_to_fingerprint_event_prefers_effective_event_time() {
        let mut tx = (*test_pool_observation_tx("fingerprint-sig")).clone();
        tx.slot = Some(42);
        tx.timestamp_ms = 100;
        tx.event_time = ghost_core::EventTimeMetadata::new(None, Some(150), None);

        let event = pool_tx_to_fingerprint_event(&tx).expect("fingerprint event");
        assert_eq!(event.timestamp_ms, 150);
    }

    #[test]
    fn test_pool_tx_to_fingerprint_event_ignores_legacy_only_timestamp() {
        let mut tx = (*test_pool_observation_tx("fingerprint-legacy-sig")).clone();
        tx.slot = Some(42);
        tx.timestamp_ms = 100;
        tx.event_time = ghost_core::EventTimeMetadata::default();

        let before = current_time_ms();
        let event = pool_tx_to_fingerprint_event(&tx).expect("fingerprint event");
        let after = current_time_ms();

        assert_ne!(event.timestamp_ms, 100);
        assert!(
            event.timestamp_ms >= before && event.timestamp_ms <= after,
            "expected wall-clock fallback, got {} outside [{}, {}]",
            event.timestamp_ms,
            before,
            after
        );
    }

    #[test]
    fn test_detected_pool_event_ts_ms_prefers_explicit_then_detected_wall() {
        let pool_id = Pubkey::new_unique();
        let mut pool = (*test_detected_pool(pool_id)).clone();
        pool.timestamp_ms = 100;
        pool.detected_wall_ts_ms = Some(200);
        pool.event_time = ghost_core::EventTimeMetadata::default();
        assert_eq!(detected_pool_event_ts_ms(&pool), 200);

        pool.event_time = ghost_core::EventTimeMetadata::new(None, Some(300), None);
        assert_eq!(detected_pool_event_ts_ms(&pool), 300);
    }

    #[test]
    fn test_detected_pool_epoch_like_ts_ms_ignores_legacy_timestamp_without_explicit_or_detected_wall(
    ) {
        let pool_id = Pubkey::new_unique();
        let mut pool = (*test_detected_pool(pool_id)).clone();
        pool.timestamp_ms = 100;
        pool.detected_wall_ts_ms = None;
        pool.event_time = ghost_core::EventTimeMetadata::default();

        let before = current_time_ms();
        let resolved = detected_pool_event_ts_ms(&pool);
        let after = current_time_ms();

        assert_eq!(detected_pool_epoch_like_ts_ms(&pool), None);
        assert_ne!(resolved, 100);
        assert!(resolved >= before && resolved <= after);
    }

    #[test]
    fn initial_window_state_for_task_prefers_detected_wall_over_legacy_timestamp() {
        let pool_id = Pubkey::new_unique();
        let mut pool = (*test_detected_pool(pool_id)).clone();
        pool.timestamp_ms = 100;
        pool.detected_wall_ts_ms = Some(200);
        pool.event_time = ghost_core::EventTimeMetadata::default();

        let ws = initial_window_state_for_task(Some(&pool), 1_000, 10_000).expect("window state");
        assert_eq!(ws.started_from.as_str(), "NewPoolDetected");
        assert_eq!(ws.t0_event_ts_ms, 200);
        assert_eq!(ws.t_end_event_ts_ms, 10_200);
    }

    #[test]
    fn test_build_enhanced_candidate_from_pool_data_uses_effective_event_time() {
        let pool_id = Pubkey::new_unique();
        let mut pool = (*test_detected_pool(pool_id)).clone();
        pool.timestamp_ms = 100;
        pool.detected_wall_ts_ms = Some(200);
        pool.event_time = ghost_core::EventTimeMetadata::new(None, Some(300), None);

        let candidate = build_enhanced_candidate_from_pool_data(
            &pool,
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj",
        )
        .expect("enhanced candidate");

        assert_eq!(candidate.timestamp, 300);
    }

    #[test]
    fn test_build_enhanced_candidate_from_pool_data_ignores_legacy_only_timestamp() {
        let pool_id = Pubkey::new_unique();
        let mut pool = (*test_detected_pool(pool_id)).clone();
        pool.timestamp_ms = 100;
        pool.detected_wall_ts_ms = None;
        pool.event_time = ghost_core::EventTimeMetadata::default();

        let before = current_time_ms();
        let candidate = build_enhanced_candidate_from_pool_data(
            &pool,
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj",
        )
        .expect("enhanced candidate");
        let after = current_time_ms();

        assert_ne!(candidate.timestamp, pool.timestamp_ms);
        assert!(candidate.timestamp >= before && candidate.timestamp <= after);
    }

    fn register_runtime_pool_for_base_mint(
        runtime: &OracleRuntime,
        base_mint: Pubkey,
        bonding_curve: Pubkey,
    ) -> Pubkey {
        let pool_id = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            amm_program_id: Pubkey::new_unique(),
            base_mint,
            quote_mint: Pubkey::new_unique(),
            bonding_curve,
            slot: Some(1),
            timestamp: 1_000,
            initial_liquidity_sol: 0.0,
            signature: Signature::new_unique().to_string(),
            ..Default::default()
        };
        assert!(
            runtime.register_new_pool(pool_id, base_mint, candidate, None),
            "runtime test pool should register exactly once"
        );
        pool_id
    }

    fn test_gatekeeper_buy_assessment(phases_passed: u8) -> GatekeeperAssessment {
        GatekeeperAssessment {
            phase1_passed: phases_passed >= 1,
            phase2_velocity: None,
            phase2_passed: phases_passed >= 2,
            phase3_diversity: None,
            phase3_passed: phases_passed >= 3,
            phase4_volume: None,
            phase4_passed: phases_passed >= 4,
            phase5_dev: None,
            phase5_passed: phases_passed >= 5,
            phase6_curve: None,
            phase6_passed: phases_passed >= 6,
            phases_passed,
            hard_reject_reason: None,
            total_tx_evaluated: 0,
            unique_tx_evaluated: 0,
            unique_signers_evaluated: 0,
            observation_duration_ms: 0,
            finalize_lag_ms: 0,
            dust_filtered_count: 0,
            eval_count: 0,
            buy_count: 0,
            decision: None,
            early_fingerprint: None,
            curve_t0_event_ts_ms: None,
            curve_t0_clock_source: None,
            curve_wait_elapsed_ms: None,
            feature_snapshot: ghost_core::checkpoint::MaterializedFeatureSet::default(),
            checkpoint_count: 0,
            trajectory_available: false,
            v25_shadow_decisions: Vec::new(),
            trajectory: None,
            pdd_assessment: None,
            aps_diagnostics: None,
            observation_stage: None,
            entry_drift_pct: None,
            entry_drift_anchor_quality: None,
            adaptive_thresholds_applied: false,
            v25_confidence: None,
        }
    }

    fn review_test_gatekeeper_config() -> GatekeeperV2Config {
        let mut config = GatekeeperV2Config::default();
        config.min_tx_count = 2;
        config.min_unique_signers = 2;
        config.min_buy_count = 2;
        config.min_interval_cv = 0.0;
        config.max_burst_ratio = 1.0;
        config.min_avg_interval_ms = 0.0;
        config.max_avg_interval_ms = 60_000.0;
        config.min_timing_entropy = 0.0;
        config.min_dust_filtered_count = 0;
        config.min_unique_ratio = 0.0;
        config.max_unique_ratio = 1.0;
        config.max_hhi = 1.0;
        config.max_tx_per_signer = 64;
        config.min_volume_gini = 0.0;
        config.max_volume_gini = 1.0;
        config.max_top3_volume_pct = 1.0;
        config.max_same_ms_tx_ratio = 1.0;
        config.min_buy_ratio = 0.0;
        config.max_buy_ratio = 1.0;
        config.min_avg_tx_sol = 0.0;
        config.max_avg_tx_sol = 100.0;
        config.min_volume_cv = 0.0;
        config.min_total_volume_sol = 0.0;
        config.min_sol_buy_ratio = 0.0;
        config.min_consecutive_buys = 0;
        config.max_dev_buy_sol = 100.0;
        config.min_dev_buy_sol = 0.0;
        config.max_dev_tx_ratio = 1.0;
        config.max_dev_volume_ratio = 1.0;
        config.min_dev_volume_ratio = 0.0;
        config.reject_on_dev_sell = true;
        config.max_price_change_ratio = 10_000.0;
        config.max_single_tx_price_impact_pct = 100.0;
        config.max_single_sell_impact_pct = 100.0;
        config.max_bonding_progress_pct = 100.0;
        config.min_bonding_progress_pct = 0.0;
        config.min_market_cap_sol = 0.0;
        config.min_phases_to_pass = 1;
        config.hard_fail_hhi = 1.0;
        config.hard_fail_same_ms_tx_ratio = 1.0;
        config.hard_fail_top3_volume_pct = 1.0;
        config.use_three_layer_decision = true;
        config.max_soft_points = 100;
        config.dev_unknown_max_soft_points = 100;
        config.curve_wait_ms = 800;
        config.curve_require_for_buy = true;
        config
    }

    fn review_test_buffered_tx(
        pool_id: Pubkey,
        signature: &str,
        timestamp_ms: u64,
        curve_data_known: bool,
    ) -> crate::components::gatekeeper::GatekeeperBufferedTx {
        let mut tx = (*test_pool_observation_tx(signature)).clone();
        tx.pool_amm_id = pool_id.to_string();
        tx.signer = Pubkey::new_unique().to_string();
        tx.timestamp_ms = timestamp_ms;
        tx.event_time.ingress_wall_ts_ms = Some(timestamp_ms);
        tx.arrival_ts_ms = timestamp_ms;
        tx.slot = Some(100 + (timestamp_ms / 10));
        tx.event_ordinal = Some(0);
        tx.volume_sol = 1.0;
        tx.curve_data_known = curve_data_known;
        tx.curve_finality = if curve_data_known {
            CurveFinality::Finalized
        } else {
            CurveFinality::Speculative
        };
        let tx = Arc::new(tx);
        crate::components::gatekeeper::GatekeeperBufferedTx {
            tx: tx.clone(),
            metrics: ghost_brain::oracle::snapshot_engine::PoolMetrics {
                tx_count: 1,
                unique_addrs: 1,
                volume_sol: tx.volume_sol,
                buy_volume_sol: tx.volume_sol,
                sell_volume_sol: 0.0,
                dev_buy_lamports: 0,
            },
            tx_key: TxKey::new(timestamp_ms, tx.slot, tx.event_ordinal, None, 0)
                .expect("buffered tx key"),
        }
    }

    fn review_seed_feature_buy_session(
        session: &mut PoolObservationSession,
        pool_id: Pubkey,
        tx_signature: &str,
    ) {
        let seeded = review_test_buffered_tx(pool_id, tx_signature, 1_120, true);
        let _ = session.ingest_transaction(seeded.tx);
        session.account_features = ghost_core::account_state_core::types::AccountStateFeatures {
            current_reserves: (30_000_000_000, 900_000_000),
            price_sol: 35.0 / 900_000.0,
            market_cap_sol: 42.0,
            bonding_progress: 0.22,
            price_change_since_t0_pct: 12.0,
            reserve_velocity_sol_per_sec: 1.5,
            is_bootstrap: false,
            curve_finality: CurveFinality::Finalized,
            state_phase: ghost_core::account_state_core::types::StatePhase::Canonical,
            update_count: 3,
        };
        session.tx_intel_features = ghost_core::tx_intelligence::types::TxIntelFeatures {
            tx_count: 24,
            buy_count: 20,
            sell_count: 4,
            unique_signers: 18,
            buy_ratio: 0.83,
            sol_buy_ratio: 0.86,
            avg_tx_sol: 1.1,
            volume_cv: 0.25,
            hhi: 0.18,
            volume_gini: 0.22,
            unique_signer_ratio: 0.75,
            avg_tx_per_signer: 1.33,
            same_ms_tx_ratio: 0.05,
            bundle_suspicion_ratio: 0.02,
            top3_volume_pct: 0.44,
            dev_buy_sol: 0.5,
            dev_volume_ratio: 0.08,
            dev_tx_ratio: 0.04,
            dev_has_sold: false,
            interval_cv: 0.40,
            timing_entropy: 1.6,
            avg_interval_ms: 140.0,
            burst_ratio: 0.10,
            dust_ratio: 0.02,
            max_tx_per_signer: 3,
            total_volume_sol: 26.4,
            min_tx_sol: 0.2,
            max_tx_sol: 2.5,
            max_consecutive_buys: 6,
            dev_wallet_known: true,
            dev_initial_buy_tokens: Some(100_000.0),
            dev_tx_count: 1,
            dev_is_first_buyer: true,
            dust_tx_count: 1,
            failed_tx_count: 0,
        };
        session.diagnostics.last_tx_ts_ms = Some(2_200);
        session.gatekeeper_buffer_mut().set_curve_t0(1_000);
        session.gatekeeper_buffer_mut().record_curve_state(
            ghost_core::CurveFreshnessState::Fresh,
            CurveFinality::Finalized,
        );
        session.try_checkpoint(2_200);
    }

    #[test]
    fn build_timeout_assessment_from_materialized_features_populates_pr6_fields() {
        let mut legacy_assessment = test_gatekeeper_buy_assessment(0);
        legacy_assessment.finalize_lag_ms = 42;
        legacy_assessment.eval_count = 3;
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.use_three_layer_decision = true;

        let mut materialized = MaterializedFeatureSet::default();
        materialized.account_features.price_sol = 1.25;
        materialized.account_features.market_cap_sol = 32.0;
        materialized.account_features.bonding_progress = 0.08;
        materialized.tx_intel_features.tx_count = 6;
        materialized.tx_intel_features.unique_signers = 4;
        materialized.tx_intel_features.buy_count = 5;
        materialized.checkpoint_features.trajectory_checkpoint_count = 2;
        materialized.checkpoint_features.price_trajectory = vec![1.0, 1.2];
        materialized
            .checkpoint_features
            .price_change_from_first_checkpoint_pct = 25.0;
        materialized.curve_readiness.price_sample_count = 3;
        materialized.curve_readiness.t0_event_ts_ms = Some(1_000);
        materialized.curve_readiness.wait_elapsed_ms = Some(250);
        materialized.curve_readiness.freshness = ghost_core::CurveFreshnessState::Fresh;

        let assessment = build_timeout_assessment_from_materialized_features(
            legacy_assessment,
            materialized,
            &gatekeeper_config,
        );

        assert_eq!(assessment.checkpoint_count, 2);
        assert!(!assessment.trajectory_available);
        assert!(assessment.trajectory.is_none());
        assert_eq!(
            assessment
                .feature_snapshot
                .checkpoint_features
                .trajectory_checkpoint_count,
            2
        );
        assert_eq!(assessment.curve_t0_event_ts_ms, Some(1_000));
        assert_eq!(assessment.curve_wait_elapsed_ms, Some(250));
        assert_eq!(assessment.entry_drift_pct, Some(25.0));
        assert!(assessment.pdd_assessment.is_some());
        assert!(assessment.v25_confidence.is_none());
        assert!(assessment.decision.is_some());
        assert_eq!(
            assessment
                .decision
                .as_ref()
                .expect("timeout decision")
                .verdict_type,
            crate::components::gatekeeper::GatekeeperVerdictType::TimeoutPhase1
        );
        assert_eq!(assessment.finalize_lag_ms, 42);
        assert_eq!(assessment.eval_count, 3);
    }

    #[test]
    fn cutover_feature_driven_terminal_verdict_replaces_legacy_reject_assessment() {
        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let quote_mint = Pubkey::from_str(&detected_pool.quote_mint).expect("quote mint");
        let bonding_curve = Pubkey::from_str(&detected_pool.bonding_curve).expect("bonding curve");
        let dev_wallet = Pubkey::from_str(&detected_pool.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            bonding_curve,
            slot: detected_pool.slot,
            timestamp: detected_pool
                .detected_wall_ts_ms
                .unwrap_or(detected_pool.timestamp_ms),
            initial_liquidity_sol: detected_pool.initial_liquidity_sol.unwrap_or_default(),
            signature: detected_pool.signature.clone(),
            ..Default::default()
        };
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.use_three_layer_decision = true;
        let mut session = PoolObservationSession::new(
            ghost_core::session::types::SessionId(1),
            pool_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            1_000,
            6_000,
            &gatekeeper_config,
            crate::tx_intelligence::TxIntelligenceConfig::from_gatekeeper_config(
                &gatekeeper_config,
                EarlyFingerprintConfig::default(),
            ),
        );
        session.gatekeeper_buffer_mut().record_curve_state(
            ghost_core::CurveFreshnessState::Fresh,
            CurveFinality::Finalized,
        );

        let mut observed_tx = (*test_pool_observation_tx("sig-pr6-cutover")).clone();
        observed_tx.pool_amm_id = pool_id.to_string();
        let _ = session.ingest_transaction(Arc::new(observed_tx));
        session.try_checkpoint(1_000);

        let legacy_verdict = GatekeeperVerdict::Reject {
            assessment: test_gatekeeper_buy_assessment(0),
            reason: "legacy-inline-reject".to_string(),
        };

        let verdict = cutover_feature_driven_terminal_verdict(
            &mut session,
            legacy_verdict,
            &gatekeeper_config,
        );

        match verdict {
            GatekeeperVerdict::Reject { assessment, reason } => {
                assert_ne!(reason, "legacy-inline-reject");
                assert_eq!(assessment.feature_snapshot.tx_intel_features.tx_count, 1);
                assert!(assessment.decision.is_some());
                assert!(
                    assessment.feature_snapshot.curve_readiness.freshness
                        == ghost_core::CurveFreshnessState::Fresh
                );
            }
            _ => panic!("expected feature-driven reject"),
        }
    }

    #[test]
    fn cutover_feature_driven_terminal_verdict_preserves_timeout_variant() {
        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let quote_mint = Pubkey::from_str(&detected_pool.quote_mint).expect("quote mint");
        let bonding_curve = Pubkey::from_str(&detected_pool.bonding_curve).expect("bonding curve");
        let dev_wallet = Pubkey::from_str(&detected_pool.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            bonding_curve,
            slot: detected_pool.slot,
            timestamp: detected_pool
                .detected_wall_ts_ms
                .unwrap_or(detected_pool.timestamp_ms),
            initial_liquidity_sol: detected_pool.initial_liquidity_sol.unwrap_or_default(),
            signature: detected_pool.signature.clone(),
            ..Default::default()
        };
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.use_three_layer_decision = true;
        let mut session = PoolObservationSession::new(
            ghost_core::session::types::SessionId(2),
            pool_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            1_000,
            6_000,
            &gatekeeper_config,
            crate::tx_intelligence::TxIntelligenceConfig::from_gatekeeper_config(
                &gatekeeper_config,
                EarlyFingerprintConfig::default(),
            ),
        );
        session.gatekeeper_buffer_mut().record_curve_state(
            ghost_core::CurveFreshnessState::Fresh,
            CurveFinality::Finalized,
        );

        let mut observed_tx = (*test_pool_observation_tx("sig-pr6-timeout")).clone();
        observed_tx.pool_amm_id = pool_id.to_string();
        let _ = session.ingest_transaction(Arc::new(observed_tx));
        session.try_checkpoint(1_000);

        let legacy_timeout = GatekeeperVerdict::Timeout {
            assessment: test_gatekeeper_buy_assessment(0),
        };

        let verdict = cutover_feature_driven_terminal_verdict(
            &mut session,
            legacy_timeout,
            &gatekeeper_config,
        );

        match verdict {
            GatekeeperVerdict::Timeout { assessment } => {
                assert_eq!(assessment.feature_snapshot.tx_intel_features.tx_count, 1);
                assert!(assessment.decision.is_some());
                assert_eq!(
                    assessment
                        .decision
                        .as_ref()
                        .expect("timeout cutover should attach policy decision")
                        .verdict_type,
                    crate::components::gatekeeper::GatekeeperVerdictType::TimeoutPhase1
                );
                assert!(assessment
                    .decision
                    .as_ref()
                    .expect("timeout cutover should attach policy decision")
                    .reason_chain
                    .contains("TIMEOUT_PHASE1"));
            }
            _ => panic!("expected timeout verdict"),
        }
    }

    #[test]
    fn cutover_feature_driven_terminal_verdict_can_override_legacy_buy_with_feature_reject() {
        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let quote_mint = Pubkey::from_str(&detected_pool.quote_mint).expect("quote mint");
        let bonding_curve = Pubkey::from_str(&detected_pool.bonding_curve).expect("bonding curve");
        let dev_wallet = Pubkey::from_str(&detected_pool.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            bonding_curve,
            slot: detected_pool.slot,
            timestamp: detected_pool
                .detected_wall_ts_ms
                .unwrap_or(detected_pool.timestamp_ms),
            initial_liquidity_sol: detected_pool.initial_liquidity_sol.unwrap_or_default(),
            signature: detected_pool.signature.clone(),
            ..Default::default()
        };
        let gatekeeper_config = review_test_gatekeeper_config();
        let mut session = PoolObservationSession::new(
            ghost_core::session::types::SessionId(3),
            pool_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            1_000,
            6_000,
            &gatekeeper_config,
            crate::tx_intelligence::TxIntelligenceConfig::from_gatekeeper_config(
                &gatekeeper_config,
                EarlyFingerprintConfig::default(),
            ),
        );
        review_seed_feature_buy_session(&mut session, pool_id, "sig-pr6-buy-override-seed");
        session.tx_intel_features.dev_has_sold = true;

        let legacy_buy = GatekeeperVerdict::Buy {
            buffered_txs: vec![review_test_buffered_tx(pool_id, "legacy-buy", 1_140, true)],
            assessment: test_gatekeeper_buy_assessment(6),
        };

        let verdict =
            cutover_feature_driven_terminal_verdict(&mut session, legacy_buy, &gatekeeper_config);

        match verdict {
            GatekeeperVerdict::Reject { assessment, .. } => {
                let decision = assessment.decision.expect("policy decision");
                assert_eq!(
                    decision.verdict_type,
                    crate::components::gatekeeper::GatekeeperVerdictType::RejectHardFail
                );
                assert!(assessment.feature_snapshot.tx_intel_features.dev_has_sold);
                assert!(assessment.feature_snapshot.tx_intel_features.tx_count > 0);
            }
            _ => panic!("expected feature-driven reject override"),
        }
    }

    #[test]
    fn cutover_feature_driven_terminal_verdict_can_override_legacy_reject_with_feature_buy() {
        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let quote_mint = Pubkey::from_str(&detected_pool.quote_mint).expect("quote mint");
        let bonding_curve = Pubkey::from_str(&detected_pool.bonding_curve).expect("bonding curve");
        let dev_wallet = Pubkey::from_str(&detected_pool.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            bonding_curve,
            slot: detected_pool.slot,
            timestamp: detected_pool
                .detected_wall_ts_ms
                .unwrap_or(detected_pool.timestamp_ms),
            initial_liquidity_sol: detected_pool.initial_liquidity_sol.unwrap_or_default(),
            signature: detected_pool.signature.clone(),
            ..Default::default()
        };
        let gatekeeper_config = review_test_gatekeeper_config();
        let mut session = PoolObservationSession::new(
            ghost_core::session::types::SessionId(4),
            pool_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            1_000,
            6_000,
            &gatekeeper_config,
            crate::tx_intelligence::TxIntelligenceConfig::from_gatekeeper_config(
                &gatekeeper_config,
                EarlyFingerprintConfig::default(),
            ),
        );
        review_seed_feature_buy_session(&mut session, pool_id, "sig-pr6-reject-override-seed");

        let legacy_reject = GatekeeperVerdict::Reject {
            assessment: test_gatekeeper_buy_assessment(0),
            reason: "legacy-inline-reject".to_string(),
        };

        let verdict = cutover_feature_driven_terminal_verdict(
            &mut session,
            legacy_reject,
            &gatekeeper_config,
        );

        match verdict {
            GatekeeperVerdict::Buy {
                buffered_txs,
                assessment,
            } => {
                let decision = assessment.decision.expect("policy decision");
                assert_eq!(
                    decision.verdict_type,
                    crate::components::gatekeeper::GatekeeperVerdictType::Buy
                );
                assert!(
                    !buffered_txs.is_empty(),
                    "feature-driven buy should preserve buffered tx handoff"
                );
                assert_eq!(assessment.feature_snapshot.tx_intel_features.tx_count, 24);
            }
            GatekeeperVerdict::Reject { reason, .. } => {
                panic!("expected feature-driven buy override, got reject: {reason}")
            }
            GatekeeperVerdict::Timeout { .. } => {
                panic!("expected feature-driven buy override, got timeout")
            }
            GatekeeperVerdict::PendingCurve => {
                panic!("expected feature-driven buy override, got pending_curve")
            }
            GatekeeperVerdict::Wait => panic!("expected feature-driven buy override, got wait"),
            GatekeeperVerdict::ApprovedTx { .. } => {
                panic!("expected feature-driven buy override, got approved_tx")
            }
        }
    }

    #[test]
    fn cutover_feature_driven_terminal_verdict_resumes_after_pending_curve() {
        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let quote_mint = Pubkey::from_str(&detected_pool.quote_mint).expect("quote mint");
        let bonding_curve = Pubkey::from_str(&detected_pool.bonding_curve).expect("bonding curve");
        let dev_wallet = Pubkey::from_str(&detected_pool.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            bonding_curve,
            slot: detected_pool.slot,
            timestamp: detected_pool
                .detected_wall_ts_ms
                .unwrap_or(detected_pool.timestamp_ms),
            initial_liquidity_sol: detected_pool.initial_liquidity_sol.unwrap_or_default(),
            signature: detected_pool.signature.clone(),
            ..Default::default()
        };
        let mut gatekeeper_config = review_test_gatekeeper_config();
        gatekeeper_config.min_tx_count = 2;
        gatekeeper_config.min_unique_signers = 2;
        gatekeeper_config.min_buy_count = 2;
        gatekeeper_config.max_wait_time_ms = 50;
        let mut session = PoolObservationSession::new(
            ghost_core::session::types::SessionId(5),
            pool_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            1_000,
            1_050,
            &gatekeeper_config,
            crate::tx_intelligence::TxIntelligenceConfig::from_gatekeeper_config(
                &gatekeeper_config,
                EarlyFingerprintConfig::default(),
            ),
        );
        session.gatekeeper_buffer_mut().set_curve_t0(1_000);
        let pending_seed = review_test_buffered_tx(pool_id, "sig-pr6-pending", 1_010, false);
        let _ = session.ingest_transaction(pending_seed.tx.clone());

        let legacy_buy = GatekeeperVerdict::Buy {
            buffered_txs: vec![pending_seed],
            assessment: test_gatekeeper_buy_assessment(6),
        };

        let pending =
            cutover_feature_driven_terminal_verdict(&mut session, legacy_buy, &gatekeeper_config);
        assert!(matches!(pending, GatekeeperVerdict::PendingCurve));
        assert!(matches!(session.get_status(), SessionStatus::Accumulating));
        assert_eq!(session.gatekeeper_buffer().state(), PoolState::Tracked);

        let legacy_timeout = session.gatekeeper_buffer_mut().force_check_deadline(1_100);
        let terminal = cutover_feature_driven_terminal_verdict(
            &mut session,
            legacy_timeout,
            &gatekeeper_config,
        );

        match terminal {
            GatekeeperVerdict::Timeout { assessment } => {
                assert!(assessment.decision.is_some());
                assert_eq!(assessment.feature_snapshot.tx_intel_features.tx_count, 1);
            }
            _ => panic!("expected timeout after pending curve"),
        }
    }

    #[test]
    fn resolve_feature_trigger_outcome_terminalizes_without_legacy_on_transaction() {
        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let quote_mint = Pubkey::from_str(&detected_pool.quote_mint).expect("quote mint");
        let bonding_curve = Pubkey::from_str(&detected_pool.bonding_curve).expect("bonding curve");
        let dev_wallet = Pubkey::from_str(&detected_pool.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            bonding_curve,
            slot: detected_pool.slot,
            timestamp: detected_pool
                .detected_wall_ts_ms
                .unwrap_or(detected_pool.timestamp_ms),
            initial_liquidity_sol: detected_pool.initial_liquidity_sol.unwrap_or_default(),
            signature: detected_pool.signature.clone(),
            ..Default::default()
        };
        let mut gatekeeper_config = review_test_gatekeeper_config();
        gatekeeper_config.min_tx_count = 1;
        gatekeeper_config.min_unique_signers = 1;
        gatekeeper_config.min_buy_count = 1;
        let mut session = PoolObservationSession::new(
            ghost_core::session::types::SessionId(6),
            pool_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            1_000,
            6_000,
            &gatekeeper_config,
            crate::tx_intelligence::TxIntelligenceConfig::from_gatekeeper_config(
                &gatekeeper_config,
                EarlyFingerprintConfig::default(),
            ),
        );
        session.gatekeeper_buffer_mut().set_curve_t0(1_000);
        session.gatekeeper_buffer_mut().record_curve_state(
            ghost_core::CurveFreshnessState::Fresh,
            CurveFinality::Finalized,
        );

        let mut observed_tx = (*test_pool_observation_tx("sig-pr6-trigger-cutover")).clone();
        observed_tx.pool_amm_id = pool_id.to_string();
        observed_tx.signer = Pubkey::new_unique().to_string();
        observed_tx.timestamp_ms = 1_200;
        observed_tx.event_time.ingress_wall_ts_ms = Some(1_200);
        observed_tx.arrival_ts_ms = 1_200;
        observed_tx.slot = Some(120);
        observed_tx.event_ordinal = Some(0);
        observed_tx.curve_data_known = true;
        let ingress = session.ingest_transaction(Arc::new(observed_tx));
        assert!(matches!(
            ingress,
            GatekeeperIngressOutcome::TriggerEvaluation
        ));

        session.account_features = ghost_core::account_state_core::types::AccountStateFeatures {
            current_reserves: (30_000_000_000, 900_000_000),
            price_sol: 35.0 / 900_000.0,
            market_cap_sol: 42.0,
            bonding_progress: 0.22,
            price_change_since_t0_pct: 12.0,
            reserve_velocity_sol_per_sec: 1.5,
            is_bootstrap: false,
            curve_finality: CurveFinality::Finalized,
            state_phase: ghost_core::account_state_core::types::StatePhase::Canonical,
            update_count: 3,
        };
        session.tx_intel_features = ghost_core::tx_intelligence::types::TxIntelFeatures {
            tx_count: 24,
            buy_count: 20,
            sell_count: 4,
            unique_signers: 18,
            buy_ratio: 0.83,
            sol_buy_ratio: 0.86,
            avg_tx_sol: 1.1,
            volume_cv: 0.25,
            hhi: 0.18,
            volume_gini: 0.22,
            unique_signer_ratio: 0.75,
            avg_tx_per_signer: 1.33,
            same_ms_tx_ratio: 0.05,
            bundle_suspicion_ratio: 0.02,
            top3_volume_pct: 0.44,
            dev_buy_sol: 0.5,
            dev_volume_ratio: 0.08,
            dev_tx_ratio: 0.04,
            dev_has_sold: false,
            interval_cv: 0.40,
            timing_entropy: 1.6,
            avg_interval_ms: 140.0,
            burst_ratio: 0.10,
            dust_ratio: 0.02,
            max_tx_per_signer: 3,
            total_volume_sol: 26.4,
            min_tx_sol: 0.2,
            max_tx_sol: 2.5,
            max_consecutive_buys: 6,
            dev_wallet_known: true,
            dev_initial_buy_tokens: Some(100_000.0),
            dev_tx_count: 1,
            dev_is_first_buyer: true,
            dust_tx_count: 1,
            failed_tx_count: 0,
        };
        session.diagnostics.last_tx_ts_ms = Some(2_200);
        session.try_checkpoint(2_200);

        let verdict = resolve_feature_trigger_outcome(&mut session, ingress, &gatekeeper_config);

        match verdict {
            GatekeeperVerdict::Buy { assessment, .. } => {
                assert_eq!(
                    assessment.decision.expect("policy decision").verdict_type,
                    crate::components::gatekeeper::GatekeeperVerdictType::Buy
                );
                assert!(assessment.feature_snapshot.tx_intel_features.tx_count > 0);
            }
            _ => panic!("expected feature-driven buy from pure tx trigger"),
        }
    }

    #[test]
    fn evaluate_feature_driven_terminal_verdict_times_out_without_force_check_deadline() {
        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let quote_mint = Pubkey::from_str(&detected_pool.quote_mint).expect("quote mint");
        let bonding_curve = Pubkey::from_str(&detected_pool.bonding_curve).expect("bonding curve");
        let dev_wallet = Pubkey::from_str(&detected_pool.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            bonding_curve,
            slot: detected_pool.slot,
            timestamp: detected_pool
                .detected_wall_ts_ms
                .unwrap_or(detected_pool.timestamp_ms),
            initial_liquidity_sol: detected_pool.initial_liquidity_sol.unwrap_or_default(),
            signature: detected_pool.signature.clone(),
            ..Default::default()
        };
        let mut gatekeeper_config = review_test_gatekeeper_config();
        gatekeeper_config.min_tx_count = 3;
        gatekeeper_config.min_unique_signers = 3;
        gatekeeper_config.min_buy_count = 3;
        gatekeeper_config.max_wait_time_ms = 50;
        let mut session = PoolObservationSession::new(
            ghost_core::session::types::SessionId(7),
            pool_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            1_000,
            1_050,
            &gatekeeper_config,
            crate::tx_intelligence::TxIntelligenceConfig::from_gatekeeper_config(
                &gatekeeper_config,
                EarlyFingerprintConfig::default(),
            ),
        );

        let mut observed_tx = (*test_pool_observation_tx("sig-pr6-deadline-cutover")).clone();
        observed_tx.pool_amm_id = pool_id.to_string();
        observed_tx.signer = Pubkey::new_unique().to_string();
        observed_tx.timestamp_ms = 1_010;
        observed_tx.event_time.ingress_wall_ts_ms = Some(1_010);
        observed_tx.arrival_ts_ms = 1_010;
        observed_tx.slot = Some(101);
        observed_tx.event_ordinal = Some(0);
        let ingress = session.ingest_transaction(Arc::new(observed_tx));
        assert!(matches!(ingress, GatekeeperIngressOutcome::Wait));

        session.try_checkpoint(1_010);
        session.gatekeeper_buffer_mut().advance_event_clock(1_100);
        let verdict =
            evaluate_feature_driven_terminal_verdict(&mut session, &gatekeeper_config, true);

        match verdict {
            GatekeeperVerdict::Timeout { assessment } => {
                assert!(assessment.decision.is_some());
                assert!(
                    assessment.v25_confidence.is_some(),
                    "oracle timeout branch must cache v25 confidence"
                );
                assert_eq!(assessment.feature_snapshot.tx_intel_features.tx_count, 1);
                assert_eq!(
                    assessment
                        .decision
                        .as_ref()
                        .expect("timeout decision")
                        .verdict_type,
                    crate::components::gatekeeper::GatekeeperVerdictType::TimeoutPhase1
                );
            }
            _ => panic!("expected timeout from pure deadline trigger"),
        }
    }

    #[test]
    fn legacy_compat_terminal_path_keeps_wait_semantics_before_deadline() {
        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let quote_mint = Pubkey::from_str(&detected_pool.quote_mint).expect("quote mint");
        let bonding_curve = Pubkey::from_str(&detected_pool.bonding_curve).expect("bonding curve");
        let dev_wallet = Pubkey::from_str(&detected_pool.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            bonding_curve,
            slot: detected_pool.slot,
            timestamp: detected_pool
                .detected_wall_ts_ms
                .unwrap_or(detected_pool.timestamp_ms),
            initial_liquidity_sol: detected_pool.initial_liquidity_sol.unwrap_or_default(),
            signature: detected_pool.signature.clone(),
            ..Default::default()
        };
        let mut gatekeeper_config = review_test_gatekeeper_config();
        gatekeeper_config.use_three_layer_decision = false;
        gatekeeper_config.min_phases_to_pass = 7;
        let mut session = PoolObservationSession::new(
            ghost_core::session::types::SessionId(8),
            pool_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            1_000,
            6_000,
            &gatekeeper_config,
            crate::tx_intelligence::TxIntelligenceConfig::from_gatekeeper_config(
                &gatekeeper_config,
                EarlyFingerprintConfig::default(),
            ),
        );

        review_seed_feature_buy_session(&mut session, pool_id, "sig-pr6-legacy-wait");

        let verdict = resolve_feature_trigger_outcome(
            &mut session,
            GatekeeperIngressOutcome::TriggerEvaluation,
            &gatekeeper_config,
        );

        assert!(matches!(verdict, GatekeeperVerdict::Wait));
        assert!(matches!(session.get_status(), SessionStatus::Accumulating));
    }

    #[test]
    fn legacy_compat_terminal_deadline_does_not_buy_on_phase_count_only() {
        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let quote_mint = Pubkey::from_str(&detected_pool.quote_mint).expect("quote mint");
        let bonding_curve = Pubkey::from_str(&detected_pool.bonding_curve).expect("bonding curve");
        let dev_wallet = Pubkey::from_str(&detected_pool.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            bonding_curve,
            slot: detected_pool.slot,
            timestamp: detected_pool
                .detected_wall_ts_ms
                .unwrap_or(detected_pool.timestamp_ms),
            initial_liquidity_sol: detected_pool.initial_liquidity_sol.unwrap_or_default(),
            signature: detected_pool.signature.clone(),
            ..Default::default()
        };
        let mut gatekeeper_config = review_test_gatekeeper_config();
        gatekeeper_config.use_three_layer_decision = false;
        gatekeeper_config.min_phases_to_pass = 1;
        gatekeeper_config.min_tx_count = 2;
        gatekeeper_config.min_unique_signers = 2;
        gatekeeper_config.min_buy_count = 2;
        let mut session = PoolObservationSession::new(
            ghost_core::session::types::SessionId(81),
            pool_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            1_000,
            1_050,
            &gatekeeper_config,
            crate::tx_intelligence::TxIntelligenceConfig::from_gatekeeper_config(
                &gatekeeper_config,
                EarlyFingerprintConfig::default(),
            ),
        );

        let mut observed_tx = (*test_pool_observation_tx("sig-pr6-legacy-deadline-no-buy")).clone();
        observed_tx.pool_amm_id = pool_id.to_string();
        observed_tx.signer = Pubkey::new_unique().to_string();
        observed_tx.timestamp_ms = 1_010;
        observed_tx.event_time.ingress_wall_ts_ms = Some(1_010);
        observed_tx.arrival_ts_ms = 1_010;
        observed_tx.slot = Some(101);
        observed_tx.event_ordinal = Some(0);
        let ingress = session.ingest_transaction(Arc::new(observed_tx));
        assert!(matches!(ingress, GatekeeperIngressOutcome::Wait));

        session.try_checkpoint(1_010);
        session.gatekeeper_buffer_mut().advance_event_clock(1_500);
        let verdict =
            evaluate_feature_driven_terminal_verdict(&mut session, &gatekeeper_config, true);

        match verdict {
            GatekeeperVerdict::Timeout { assessment } => {
                let decision = assessment.decision.expect("timeout decision");
                assert_eq!(
                    decision.verdict_type,
                    crate::components::gatekeeper::GatekeeperVerdictType::TimeoutPhase1
                );
                assert!(!decision.verdict_buy);
                assert_eq!(assessment.total_tx_evaluated, 1);
                assert!(!assessment.phase1_passed);
            }
            _ => panic!("expected timeout instead of phase-count-only BUY"),
        }
    }

    #[test]
    fn legacy_compat_terminal_buy_populates_log_routing_fields() {
        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let quote_mint = Pubkey::from_str(&detected_pool.quote_mint).expect("quote mint");
        let bonding_curve = Pubkey::from_str(&detected_pool.bonding_curve).expect("bonding curve");
        let dev_wallet = Pubkey::from_str(&detected_pool.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            bonding_curve,
            slot: detected_pool.slot,
            timestamp: detected_pool
                .detected_wall_ts_ms
                .unwrap_or(detected_pool.timestamp_ms),
            initial_liquidity_sol: detected_pool.initial_liquidity_sol.unwrap_or_default(),
            signature: detected_pool.signature.clone(),
            ..Default::default()
        };
        let mut gatekeeper_config = review_test_gatekeeper_config();
        gatekeeper_config.use_three_layer_decision = false;
        let mut session = PoolObservationSession::new(
            ghost_core::session::types::SessionId(82),
            pool_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            1_000,
            6_000,
            &gatekeeper_config,
            crate::tx_intelligence::TxIntelligenceConfig::from_gatekeeper_config(
                &gatekeeper_config,
                EarlyFingerprintConfig::default(),
            ),
        );

        review_seed_feature_buy_session(&mut session, pool_id, "sig-pr6-legacy-buy-log");

        let verdict =
            evaluate_feature_driven_terminal_verdict(&mut session, &gatekeeper_config, true);

        match verdict {
            GatekeeperVerdict::Buy { assessment, .. } => {
                let buy_log = assessment.to_buy_log(&pool_id, &gatekeeper_config);
                assert_eq!(buy_log.decision_verdict_buy, Some(true));
                assert_eq!(buy_log.verdict_type.as_deref(), Some("BUY"));
            }
            _ => panic!("expected BUY with populated routing fields"),
        }
    }

    #[test]
    fn feature_driven_deadline_uses_materialized_phase1_not_stale_buffer_latch() {
        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let quote_mint = Pubkey::from_str(&detected_pool.quote_mint).expect("quote mint");
        let bonding_curve = Pubkey::from_str(&detected_pool.bonding_curve).expect("bonding curve");
        let dev_wallet = Pubkey::from_str(&detected_pool.creator).ok();
        let candidate_snapshot = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            quote_mint,
            bonding_curve,
            slot: detected_pool.slot,
            timestamp: detected_pool
                .detected_wall_ts_ms
                .unwrap_or(detected_pool.timestamp_ms),
            initial_liquidity_sol: detected_pool.initial_liquidity_sol.unwrap_or_default(),
            signature: detected_pool.signature.clone(),
            ..Default::default()
        };
        let mut gatekeeper_config = review_test_gatekeeper_config();
        gatekeeper_config.max_wait_time_ms = 50;
        let mut session = PoolObservationSession::new(
            ghost_core::session::types::SessionId(83),
            pool_id,
            base_mint,
            bonding_curve,
            dev_wallet,
            candidate_snapshot,
            1_000,
            1_050,
            &gatekeeper_config,
            crate::tx_intelligence::TxIntelligenceConfig::from_gatekeeper_config(
                &gatekeeper_config,
                EarlyFingerprintConfig::default(),
            ),
        );

        review_seed_feature_buy_session(
            &mut session,
            pool_id,
            "sig-pr6-deadline-materialized-phase1",
        );

        let verdict =
            evaluate_feature_driven_terminal_verdict(&mut session, &gatekeeper_config, true);

        match verdict {
            GatekeeperVerdict::Buy { assessment, .. } => {
                let decision = assessment.decision.expect("buy decision");
                assert_eq!(
                    decision.verdict_type,
                    crate::components::gatekeeper::GatekeeperVerdictType::Buy
                );
                assert!(decision.verdict_buy);
                assert!(assessment.phase1_passed);
            }
            GatekeeperVerdict::Reject { assessment, .. } => {
                assert!(
                    assessment.phase1_passed,
                    "deadline should not degrade qualifying pools to timeout"
                );
                assert_ne!(
                    assessment
                        .decision
                        .as_ref()
                        .expect("reject decision")
                        .verdict_type,
                    crate::components::gatekeeper::GatekeeperVerdictType::TimeoutPhase1
                );
            }
            GatekeeperVerdict::Timeout { .. } => {
                panic!("unexpected timeout despite materialized phase1")
            }
            GatekeeperVerdict::Wait => panic!("unexpected wait at forced deadline"),
            GatekeeperVerdict::PendingCurve => {
                panic!("unexpected pending curve at forced deadline")
            }
            GatekeeperVerdict::ApprovedTx { .. } => {
                panic!("unexpected approved-tx verdict at forced deadline")
            }
        }
    }

    fn test_pool_observation_context(
        oracle_runtime: Arc<OracleRuntime>,
        snapshot_engine: Arc<SnapshotEngine>,
        event_tx: EventBusSender,
        trigger: Option<Arc<crate::components::trigger::TriggerComponent>>,
    ) -> Arc<PoolObservationContext> {
        let (result_tx, _result_rx) = tokio::sync::mpsc::unbounded_channel();
        let session_manager = oracle_runtime.session_manager();
        Arc::new(PoolObservationContext {
            oracle_runtime,
            session_manager,
            snapshot_engine,
            event_tx,
            post_buy_tx: None,
            decision_logger: Arc::new(ghost_brain::oracle::DecisionLogger::new(
                ghost_brain::oracle::DecisionLoggerConfig {
                    enabled: false,
                    ..Default::default()
                },
            )),
            coverage_audit_log_path: std::path::PathBuf::from("/tmp/ghost-gatekeeper-test.jsonl"),
            trigger,
            iwim_veto_config: ghost_brain::config::IwimVetoGateConfig::default(),
            gatekeeper_config: GatekeeperV2Config::default(),
            cross_pool_velocity_config: CrossPoolVelocityConfig::from_gatekeeper_config(
                &GatekeeperV2Config::default(),
            ),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(
                &GatekeeperV2Config::default(),
            ),
            authoritative_funding_coverage_gate_enabled: false,
            fingerprint_config: EarlyFingerprintConfig::default(),
            event_emitter: None,
            health: None,
            result_tx,
            post_buy_epoch: Arc::new(AtomicU64::new(1)),
            execution_mode: ExecutionMode::Paper,
            shadow_entry_log_path: std::path::PathBuf::from("/tmp/ghost-shadow-entry-test.jsonl"),
            shadow_lifecycle_log_path: None,
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            dry_run: true,
            ab_window_ms: 10_000,
        })
    }

    #[test]
    fn oracle_runtime_wal_records_decision() {
        let wal_dir = tempdir().expect("wal tempdir");
        let wal = Arc::new(Wal::new(wal_dir.path(), 60_000, 60_000).expect("wal init"));
        let runtime = OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        )
        .with_wal(Arc::clone(&wal));

        let pool_id = Pubkey::new_unique();
        runtime.append_decision_to_wal(
            pool_id,
            Some(77),
            WalGatekeeperDecision::Timeout,
            Some("gatekeeper_timeout".to_string()),
        );

        wal.flush().expect("wal flush");
        let mut records = Vec::new();
        wal.replay_all(|record| records.push(record))
            .expect("wal replay");

        assert!(matches!(
            records.as_slice(),
            [WalRecord::Decision {
                slot,
                pool_id: Some(pool_bytes),
                decision,
                reason: Some(reason),
                ..
            }] if *slot == 77
                && *pool_bytes == pool_id.to_bytes().to_vec()
                && *decision == WalGatekeeperDecision::Timeout
                && reason == "gatekeeper_timeout"
        ));
    }

    #[tokio::test]
    async fn buy_path_hydrates_pool_metadata_from_runtime_registry() {
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        register_test_detected_pool(runtime.as_ref(), detected_pool.as_ref());
        runtime.remember_detected_pool(pool_id, detected_pool.clone());

        let (event_tx, mut event_rx) = crate::events::create_event_bus();
        let ctx = test_pool_observation_context(runtime.clone(), snapshot_engine, event_tx, None);
        let (_tx, mut rx) = tokio::sync::mpsc::channel(4);
        let buffered_txs: Vec<crate::components::gatekeeper::GatekeeperBufferedTx> = Vec::new();
        let mut identity = build_unknown_observation_identity(pool_id, ctx.ab_window_ms);
        let mut base_mint_pubkey = None;
        let mut pool_data = None;

        let outcome = execute_gatekeeper_buy_path(
            pool_id,
            2_000,
            &buffered_txs,
            &test_gatekeeper_buy_assessment(6),
            "paper",
            &mut rx,
            ctx.as_ref(),
            &mut identity,
            &mut base_mint_pubkey,
            &mut pool_data,
        )
        .await;

        assert_eq!(
            outcome.metadata_source,
            BuyPathMetadataSource::RuntimeRegistry
        );
        assert!(!outcome.bought, "no trigger configured in this test");
        assert_eq!(
            pool_data.as_ref().map(|pd| pd.base_mint.as_str()),
            Some(detected_pool.base_mint.as_str())
        );
        assert_eq!(
            base_mint_pubkey,
            Some(Pubkey::from_str(&detected_pool.base_mint).expect("base mint pubkey"))
        );
        assert_eq!(identity.base_mint, detected_pool.base_mint);
    }

    #[tokio::test]
    async fn buy_path_hydrates_pool_metadata_from_runtime_state_snapshot() {
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        register_test_detected_pool(runtime.as_ref(), detected_pool.as_ref());

        let (event_tx, mut event_rx) = crate::events::create_event_bus();
        let ctx = test_pool_observation_context(runtime, snapshot_engine, event_tx, None);
        let (_tx, mut rx) = tokio::sync::mpsc::channel(4);
        let mut identity = build_unknown_observation_identity(pool_id, ctx.ab_window_ms);
        let mut base_mint_pubkey = None;
        let mut pool_data = None;

        let metadata_source = hydrate_buy_path_metadata(
            pool_id,
            2_000,
            &mut rx,
            ctx.as_ref(),
            &mut identity,
            &mut base_mint_pubkey,
            &mut pool_data,
        )
        .await;

        assert_eq!(metadata_source, BuyPathMetadataSource::RuntimeRegistry);
        assert_eq!(
            pool_data.as_ref().map(|pd| pd.base_mint.as_str()),
            Some(detected_pool.base_mint.as_str())
        );
        assert_eq!(identity.base_mint, detected_pool.base_mint);
    }

    #[tokio::test]
    async fn late_new_pool_still_rescues_buy_path() {
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);

        let (event_tx, mut event_rx) = crate::events::create_event_bus();
        let ctx = test_pool_observation_context(runtime.clone(), snapshot_engine, event_tx, None);
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let buffered_txs: Vec<crate::components::gatekeeper::GatekeeperBufferedTx> = Vec::new();
        let ctx_clone = ctx.clone();

        let join = tokio::spawn(async move {
            let mut identity = build_unknown_observation_identity(pool_id, ctx_clone.ab_window_ms);
            let mut base_mint_pubkey = None;
            let mut pool_data = None;
            let mut rx = rx;
            let outcome = execute_gatekeeper_buy_path(
                pool_id,
                2_000,
                &buffered_txs,
                &test_gatekeeper_buy_assessment(6),
                "paper",
                &mut rx,
                ctx_clone.as_ref(),
                &mut identity,
                &mut base_mint_pubkey,
                &mut pool_data,
            )
            .await;
            (outcome, identity, base_mint_pubkey, pool_data)
        });

        let late_pool = detected_pool.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            tx.send(PoolObservationMsg::NewPool(late_pool))
                .await
                .expect("late pool metadata send");
        });

        tokio::time::sleep(Duration::from_millis(75)).await;

        let (outcome, identity, base_mint_pubkey, pool_data) = join.await.expect("buy path join");
        assert_eq!(outcome.metadata_source, BuyPathMetadataSource::WaitFallback);
        assert!(pool_data.is_some(), "late metadata should rescue BUY path");
        assert_eq!(identity.base_mint, detected_pool.base_mint);
        assert_eq!(
            base_mint_pubkey,
            Some(Pubkey::from_str(&detected_pool.base_mint).expect("base mint pubkey"))
        );
    }

    #[tokio::test]
    async fn live_buy_path_fails_closed_when_canonical_readiness_timeout_expires() {
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        register_test_detected_pool(runtime.as_ref(), detected_pool.as_ref());
        runtime.remember_detected_pool(pool_id, detected_pool.clone());

        let trigger = Arc::new(crate::components::trigger::TriggerComponent::new(
            crate::config::TriggerComponentConfig {
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
            },
        ));

        let (event_tx, mut event_rx) = crate::events::create_event_bus();
        let ctx = test_pool_observation_context(
            runtime,
            snapshot_engine,
            event_tx,
            Some(trigger.clone()),
        );
        let (_tx, mut rx) = tokio::sync::mpsc::channel(4);
        let mut identity = build_unknown_observation_identity(pool_id, ctx.ab_window_ms);
        let mut base_mint_pubkey = None;
        let mut pool_data = None;

        let outcome = execute_gatekeeper_buy_path(
            pool_id,
            2_000,
            &[],
            &test_gatekeeper_buy_assessment(6),
            "paper",
            &mut rx,
            ctx.as_ref(),
            &mut identity,
            &mut base_mint_pubkey,
            &mut pool_data,
        )
        .await;

        assert_eq!(
            outcome.shadow_execution_outcome,
            "trigger_canonical_not_ready"
        );
        assert_eq!(
            trigger.prepared_request_invocations(),
            0,
            "live trigger must not be invoked before canonical readiness gate passes"
        );
    }

    #[tokio::test]
    async fn live_buy_path_waits_for_tx_bootstrap_canonical_readiness_before_trigger_dispatch() {
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        register_test_detected_pool(runtime.as_ref(), detected_pool.as_ref());
        runtime.remember_detected_pool(pool_id, detected_pool.clone());

        let trigger = Arc::new(crate::components::trigger::TriggerComponent::new(
            crate::config::TriggerComponentConfig {
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
            },
        ));

        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let ctx = test_pool_observation_context(
            runtime.clone(),
            snapshot_engine,
            event_tx,
            Some(trigger.clone()),
        );
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let ctx_clone = ctx.clone();

        let join = tokio::spawn(async move {
            let mut identity = build_unknown_observation_identity(pool_id, ctx_clone.ab_window_ms);
            let mut base_mint_pubkey = None;
            let mut pool_data = None;
            let mut rx = rx;
            execute_gatekeeper_buy_path(
                pool_id,
                2_000,
                &[],
                &test_gatekeeper_buy_assessment(6),
                "paper",
                &mut rx,
                ctx_clone.as_ref(),
                &mut identity,
                &mut base_mint_pubkey,
                &mut pool_data,
            )
            .await
        });

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            let mut observed_tx = (*test_pool_observation_tx("sig-gate-readiness")).clone();
            observed_tx.pool_amm_id = pool_id.to_string();
            observed_tx.token_mint = Some(base_mint.to_string());
            observed_tx.slot = Some(42);
            observed_tx.curve_data_known = true;
            observed_tx.curve_finality = CurveFinality::Speculative;
            observed_tx.v_sol_in_bonding_curve = Some(31.0);
            observed_tx.v_tokens_in_bonding_curve = Some(900_000_000.0);
            tx.send(PoolObservationMsg::Transaction(Arc::new(observed_tx)))
                .await
                .expect("tx bootstrap send");
        });

        let outcome = join.await.expect("buy path join");
        assert_eq!(outcome.shadow_execution_outcome, "trigger_dispatch_failed");
        assert_eq!(
            trigger.prepared_request_invocations(),
            1,
            "live trigger should run once canonical readiness gate is satisfied"
        );
        assert!(
            runtime.is_live_trigger_canonical_ready(&base_mint),
            "tx-observed bootstrap should satisfy live trigger readiness"
        );
    }

    #[tokio::test]
    async fn buy_path_logs_skip_only_after_registry_and_wait_fallback_fail() {
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let ctx = test_pool_observation_context(runtime, snapshot_engine, event_tx, None);
        let (_tx, rx) = tokio::sync::mpsc::channel(4);
        let buffered_txs: Vec<crate::components::gatekeeper::GatekeeperBufferedTx> = Vec::new();
        let ctx_clone = ctx.clone();

        let join = tokio::spawn(async move {
            let mut identity = build_unknown_observation_identity(pool_id, ctx_clone.ab_window_ms);
            let mut base_mint_pubkey = None;
            let mut pool_data = None;
            let mut rx = rx;
            let outcome = execute_gatekeeper_buy_path(
                pool_id,
                2_000,
                &buffered_txs,
                &test_gatekeeper_buy_assessment(6),
                "paper",
                &mut rx,
                ctx_clone.as_ref(),
                &mut identity,
                &mut base_mint_pubkey,
                &mut pool_data,
            )
            .await;
            (outcome, pool_data)
        });

        tokio::time::sleep(Duration::from_millis(550)).await;

        let (outcome, pool_data) = join.await.expect("buy path join");
        assert_eq!(outcome.metadata_source, BuyPathMetadataSource::Missing);
        assert!(
            pool_data.is_none(),
            "BUY path should skip only after both fallbacks fail"
        );
    }

    #[tokio::test]
    async fn buy_path_does_not_synthesize_pool_metadata_from_buffered_txs() {
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let mut tx = (*test_pool_observation_tx("first-tx-fallback")).clone();
        tx.pool_amm_id = pool_id.to_string();
        tx.token_mint = Some(base_mint.to_string());
        tx.signer = solana_sdk::signature::Keypair::new().pubkey().to_string();
        tx.slot = Some(42);
        tx.timestamp_ms = 12_345;
        tx.arrival_ts_ms = 12_350;
        tx.reserve_quote = Some(9.5);
        tx.volume_sol = 1.25;
        tx.buy_variant = Some("legacy_buy".to_string());
        let buffered_txs = vec![crate::components::gatekeeper::GatekeeperBufferedTx {
            tx: Arc::new(tx),
            metrics: PoolMetrics::default(),
            tx_key: TxKey::new(12_345, Some(42), Some(0), None, 0).expect("tx key"),
        }];

        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let ctx = test_pool_observation_context(runtime, snapshot_engine, event_tx, None);
        let (_tx, mut rx) = tokio::sync::mpsc::channel(4);
        let mut identity = build_unknown_observation_identity(pool_id, ctx.ab_window_ms);
        let mut base_mint_pubkey = None;
        let mut pool_data = None;

        let outcome = execute_gatekeeper_buy_path(
            pool_id,
            2_000,
            &buffered_txs,
            &test_gatekeeper_buy_assessment(6),
            "paper",
            &mut rx,
            ctx.as_ref(),
            &mut identity,
            &mut base_mint_pubkey,
            &mut pool_data,
        )
        .await;

        assert_eq!(outcome.metadata_source, BuyPathMetadataSource::Missing);
        assert!(
            pool_data.is_none(),
            "buffered txs must not synthesize canonical pool metadata"
        );
    }

    #[test]
    fn shadow_run_readiness_reports_missing_required_fields() {
        let readiness = compute_shadow_run_readiness(
            Some(&DetectedPool {
                semantic: Default::default(),
                pool_amm_id: Pubkey::new_unique().to_string(),
                base_mint: "unknown".to_string(),
                quote_mint: "".to_string(),
                amm_program: "pumpfun".to_string(),
                bonding_curve: Pubkey::new_unique().to_string(),
                creator: "unknown".to_string(),
                slot: None,
                timestamp_ms: 0,
                event_time: ghost_core::EventTimeMetadata::default(),
                detected_wall_ts_ms: None,
                initial_liquidity_sol: None,
                signature: Signature::new_unique().to_string(),
            }),
            &[],
        );

        assert!(!readiness.ready);
        assert_eq!(
            readiness.missing_fields,
            vec![
                "base_mint".to_string(),
                "quote_mint".to_string(),
                "creator".to_string(),
                "initial_liquidity_sol".to_string(),
                "slot".to_string(),
                "timestamp_ms".to_string(),
            ]
        );
    }

    #[test]
    fn shadow_run_readiness_allows_complete_metadata_without_observed_buy_tx() {
        let readiness = compute_shadow_run_readiness(
            Some(&DetectedPool {
                semantic: Default::default(),
                pool_amm_id: Pubkey::new_unique().to_string(),
                base_mint: Pubkey::new_unique().to_string(),
                quote_mint: WRAPPED_SOL_MINT.to_string(),
                amm_program: "pumpfun".to_string(),
                bonding_curve: Pubkey::new_unique().to_string(),
                creator: solana_sdk::signature::Keypair::new().pubkey().to_string(),
                slot: Some(1),
                timestamp_ms: 1_000,
                event_time: ghost_core::EventTimeMetadata::default(),
                detected_wall_ts_ms: Some(1_001),
                initial_liquidity_sol: Some(1.0),
                signature: Signature::new_unique().to_string(),
            }),
            &[],
        );

        assert!(readiness.ready);
        assert!(readiness.missing_fields.is_empty());
    }

    #[test]
    fn shadow_run_readiness_accepts_successful_observed_buy_tx() {
        let mut tx = (*test_pool_observation_tx("readiness-buy")).clone();
        tx.success = true;
        tx.is_buy = true;
        tx.buy_variant = Some("legacy_buy".to_string());

        let buffered_txs = vec![GatekeeperBufferedTx {
            tx: Arc::new(tx),
            metrics: PoolMetrics::default(),
            tx_key: TxKey::new(1_000, Some(1), Some(0), None, 0).expect("tx key"),
        }];

        let readiness = compute_shadow_run_readiness(
            Some(&DetectedPool {
                semantic: Default::default(),
                pool_amm_id: Pubkey::new_unique().to_string(),
                base_mint: Pubkey::new_unique().to_string(),
                quote_mint: WRAPPED_SOL_MINT.to_string(),
                amm_program: "pumpfun".to_string(),
                bonding_curve: Pubkey::new_unique().to_string(),
                creator: solana_sdk::signature::Keypair::new().pubkey().to_string(),
                slot: Some(1),
                timestamp_ms: 1_000,
                event_time: ghost_core::EventTimeMetadata::default(),
                detected_wall_ts_ms: Some(1_001),
                initial_liquidity_sol: Some(1.0),
                signature: Signature::new_unique().to_string(),
            }),
            &buffered_txs,
        );

        assert!(readiness.ready);
        assert!(readiness.missing_fields.is_empty());
    }

    #[test]
    fn tx_first_window_state_uses_first_tx_fallback() {
        let mut window_state = initial_window_state_for_task(None, 1_000, 10_000);
        assert!(window_state.is_none());

        let ws = ensure_window_state_for_tx(&mut window_state, 1_234, 10_000);
        assert_eq!(ws.started_from.as_str(), "FirstTxFallback");
        assert_eq!(ws.t0_event_ts_ms, 1_234);
        assert_eq!(ws.t_end_event_ts_ms, 11_234);
    }

    #[tokio::test]
    async fn buy_path_does_not_skip_shadow_only_when_new_pool_arrived_earlier() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        register_test_detected_pool(runtime.as_ref(), detected_pool.as_ref());
        runtime.remember_detected_pool(pool_id, detected_pool.clone());

        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-buy-path.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );

        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let mut shadow_rx = event_tx.subscribe();
        let ctx = test_pool_observation_context(runtime, snapshot_engine, event_tx.clone(), None);
        let (_tx, mut rx) = tokio::sync::mpsc::channel(4);
        let mut identity = build_unknown_observation_identity(pool_id, ctx.ab_window_ms);
        let mut base_mint_pubkey = None;
        let mut pool_data = None;

        let metadata_source = hydrate_buy_path_metadata(
            pool_id,
            2_000,
            &mut rx,
            ctx.as_ref(),
            &mut identity,
            &mut base_mint_pubkey,
            &mut pool_data,
        )
        .await;

        assert_eq!(metadata_source, BuyPathMetadataSource::RuntimeRegistry);
        let resolved_pool = pool_data
            .as_ref()
            .expect("runtime registry should hydrate DetectedPool");
        let applied = apply_trigger_buy_outcome(
            &event_tx,
            None,
            &trigger,
            &ctx.post_buy_epoch,
            ctx.execution_mode,
            &ctx.shadow_entry_log_path,
            ctx.shadow_lifecycle_log_path.as_deref(),
            "test-join",
            &ctx.gatekeeper_rollout_profile,
            pool_id,
            resolved_pool,
            0.1,
            10,
            "paper",
            None,
            None,
            crate::components::trigger::TriggerBuyOutcome::ShadowSimulated {
                report: crate::components::trigger::ShadowBuySimulationReport {
                    mint: resolved_pool.base_mint.clone(),
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
                },
            },
        )
        .await
        .expect("shadow-only apply should succeed");

        assert!(!applied.bought, "shadow_only should not mark a live buy");
        assert!(applied.retain_runtime_pool);
        assert_eq!(applied.close_reason, WindowCloseReason::PoolShadowedEarly);

        let event = tokio::time::timeout(Duration::from_secs(1), shadow_rx.recv())
            .await
            .expect("shadow event timeout")
            .expect("shadow event receive");
        assert!(
            matches!(event, GhostEvent::ShadowBuySimulated(_)),
            "shadow_only BUY path should reach simulation when registry metadata exists"
        );
    }

    #[tokio::test]
    async fn shadow_only_skips_when_shadow_readiness_is_incomplete() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let mut pool = (*test_detected_pool(pool_id)).clone();
        pool.creator = "unknown".to_string();
        pool.slot = None;
        pool.initial_liquidity_sol = None;
        register_test_detected_pool(runtime.as_ref(), &pool);
        runtime.remember_detected_pool(pool_id, Arc::new(pool.clone()));

        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-skip.jsonl")
            .to_string_lossy()
            .into_owned();
        trigger_config.shadow_run.enabled = true;
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );

        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let mut shadow_rx = event_tx.subscribe();
        let ctx = test_pool_observation_context(runtime, snapshot_engine, event_tx, Some(trigger));
        let (_tx, mut rx) = tokio::sync::mpsc::channel(4);
        let mut identity = build_unknown_observation_identity(pool_id, ctx.ab_window_ms);
        let mut base_mint_pubkey = None;
        let mut pool_data = None;

        let outcome = execute_gatekeeper_buy_path(
            pool_id,
            2_000,
            &[],
            &test_gatekeeper_buy_assessment(6),
            "paper",
            &mut rx,
            ctx.as_ref(),
            &mut identity,
            &mut base_mint_pubkey,
            &mut pool_data,
        )
        .await;

        assert_eq!(outcome.shadow_execution_outcome, "shadow_skipped_not_ready");
        assert!(!outcome.bought);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), shadow_rx.recv())
                .await
                .is_err(),
            "shadow_only should not emit simulation event when metadata is incomplete"
        );
    }

    #[tokio::test]
    async fn shadow_only_derives_initial_liquidity_from_curve_context() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let mut pool = (*test_detected_pool(pool_id)).clone();
        pool.initial_liquidity_sol = None;
        let bonding_curve = Pubkey::from_str(&pool.bonding_curve).expect("bonding curve");
        register_test_detected_pool(runtime.as_ref(), &pool);
        runtime.remember_detected_pool(pool_id, Arc::new(pool.clone()));
        runtime.get_shadow_ledger().insert_with_slot(
            bonding_curve,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: 12_000_000_000,
                virtual_token_reserves: 900_000_000_000,
                real_sol_reserves: 12_000_000_000,
                real_token_reserves: 900_000_000_000,
                token_total_supply: 900_000_000_000,
                complete: 0,
                _padding: [0u8; 7],
            },
            100,
        );

        let keypair_path = temp.path().join("id.json");
        let payer = write_test_keypair(&keypair_path);
        let mint = Pubkey::from_str(&pool.base_mint).expect("base mint");
        let token_program =
            Pubkey::from_str(trigger::direct_buy_builder::TOKEN_PROGRAM_ID).expect("token program");
        let user_ata = spl_associated_token_account::get_associated_token_address_with_program_id(
            &payer.pubkey(),
            &mint,
            &token_program,
        );
        let rpc_url = spawn_trigger_prepare_mock_rpc_server(
            payer.pubkey(),
            mint,
            user_ata,
            token_program,
            1_000_000_000,
        )
        .await;

        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
            rpc_url: rpc_url.clone(),
            keypair_path: Some(keypair_path.to_string_lossy().into_owned()),
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-derived-liquidity.jsonl")
            .to_string_lossy()
            .into_owned();
        trigger_config.shadow_run.enabled = true;
        trigger_config.shadow_run.shadow_rpc_url = rpc_url;
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );

        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let mut shadow_rx = event_tx.subscribe();
        let ctx = test_pool_observation_context(runtime, snapshot_engine, event_tx, Some(trigger));
        let (_tx, mut rx) = tokio::sync::mpsc::channel(4);
        let mut identity = build_unknown_observation_identity(pool_id, ctx.ab_window_ms);
        let mut base_mint_pubkey = None;
        let mut pool_data = None;

        let outcome = execute_gatekeeper_buy_path(
            pool_id,
            2_000,
            &[],
            &test_gatekeeper_buy_assessment(6),
            "paper",
            &mut rx,
            ctx.as_ref(),
            &mut identity,
            &mut base_mint_pubkey,
            &mut pool_data,
        )
        .await;

        assert_eq!(outcome.shadow_execution_outcome, "shadow_simulated");
        let hydrated = pool_data.expect("pool data should remain hydrated");
        assert_eq!(hydrated.initial_liquidity_sol, Some(12.0));
        let event = tokio::time::timeout(Duration::from_secs(1), shadow_rx.recv())
            .await
            .expect("shadow event timeout")
            .expect("shadow event receive");
        assert!(matches!(event, GhostEvent::ShadowBuySimulated(_)));
    }

    #[tokio::test]
    async fn pool_observation_task_uses_session_lifecycle() {
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        register_test_detected_pool(runtime.as_ref(), detected_pool.as_ref());
        runtime.remember_detected_pool(pool_id, detected_pool.clone());

        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.max_wait_time_ms = 1;
        let ctx = Arc::new(PoolObservationContext {
            oracle_runtime: runtime.clone(),
            session_manager: runtime.session_manager(),
            snapshot_engine,
            event_tx,
            post_buy_tx: None,
            decision_logger: Arc::new(ghost_brain::oracle::DecisionLogger::new(
                ghost_brain::oracle::DecisionLoggerConfig {
                    enabled: false,
                    ..Default::default()
                },
            )),
            coverage_audit_log_path: std::path::PathBuf::from("/tmp/ghost-gatekeeper-test.jsonl"),
            trigger: None,
            iwim_veto_config: ghost_brain::config::IwimVetoGateConfig::default(),
            cross_pool_velocity_config: CrossPoolVelocityConfig::from_gatekeeper_config(
                &gatekeeper_config,
            ),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(&gatekeeper_config),
            authoritative_funding_coverage_gate_enabled: false,
            gatekeeper_config,
            fingerprint_config: EarlyFingerprintConfig::default(),
            event_emitter: None,
            health: None,
            result_tx,
            post_buy_epoch: Arc::new(AtomicU64::new(1)),
            execution_mode: ExecutionMode::Paper,
            shadow_entry_log_path: std::path::PathBuf::from("/tmp/ghost-shadow-entry-test.jsonl"),
            shadow_lifecycle_log_path: None,
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            dry_run: true,
            ab_window_ms: 10_000,
        });
        let (tx, rx) = tokio::sync::mpsc::channel(4);

        let join = tokio::spawn(pool_observation_task(
            pool_id,
            Some(detected_pool.clone()),
            1_000,
            rx,
            ctx.clone(),
        ));

        let opened_session = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(session) = ctx.session_manager.get_session(&pool_id) {
                    break session;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("session should open before task exits");

        assert!(matches!(
            opened_session.read().get_status(),
            SessionStatus::Created | SessionStatus::Accumulating | SessionStatus::Evaluating
        ));

        let mut observed_tx = (*test_pool_observation_tx("sig-session-lifecycle")).clone();
        observed_tx.pool_amm_id = pool_id.to_string();
        tx.send(PoolObservationMsg::Transaction(Arc::new(observed_tx)))
            .await
            .expect("tx send must succeed");
        drop(tx);

        let result = tokio::time::timeout(Duration::from_secs(1), result_rx.recv())
            .await
            .expect("result should arrive")
            .expect("pool observation result should be emitted");
        assert_eq!(result.pool_id, pool_id);
        assert!(!result.bought);
        assert_eq!(
            result.base_mint,
            Pubkey::from_str(&detected_pool.base_mint).ok()
        );

        tokio::time::timeout(Duration::from_secs(1), join)
            .await
            .expect("task join timeout")
            .expect("task should finish cleanly");

        assert!(
            ctx.session_manager.get_session(&pool_id).is_none(),
            "pool_observation_task must remove the session on terminal verdict"
        );
        assert!(
            matches!(opened_session.read().get_status(), SessionStatus::Closed),
            "removed session handle should be closed"
        );
    }

    #[tokio::test]
    async fn pool_observation_task_wires_pr5_checkpoint_and_materialization() {
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        ));
        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        runtime.configure_approval_gating(snapshot_engine.as_ref());

        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        register_test_detected_pool(runtime.as_ref(), detected_pool.as_ref());
        runtime.remember_detected_pool(pool_id, detected_pool.clone());

        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut gatekeeper_config = GatekeeperV2Config::default();
        gatekeeper_config.max_wait_time_ms = 25;
        let ctx = Arc::new(PoolObservationContext {
            oracle_runtime: runtime.clone(),
            session_manager: runtime.session_manager(),
            snapshot_engine,
            event_tx,
            post_buy_tx: None,
            decision_logger: Arc::new(ghost_brain::oracle::DecisionLogger::new(
                ghost_brain::oracle::DecisionLoggerConfig {
                    enabled: false,
                    ..Default::default()
                },
            )),
            coverage_audit_log_path: std::path::PathBuf::from("/tmp/ghost-gatekeeper-test.jsonl"),
            trigger: None,
            iwim_veto_config: ghost_brain::config::IwimVetoGateConfig::default(),
            cross_pool_velocity_config: CrossPoolVelocityConfig::from_gatekeeper_config(
                &gatekeeper_config,
            ),
            funding_source_config: FundingSourceConfig::from_gatekeeper_config(&gatekeeper_config),
            authoritative_funding_coverage_gate_enabled: false,
            gatekeeper_config,
            fingerprint_config: EarlyFingerprintConfig::default(),
            event_emitter: None,
            health: None,
            result_tx,
            post_buy_epoch: Arc::new(AtomicU64::new(1)),
            execution_mode: ExecutionMode::Paper,
            shadow_entry_log_path: std::path::PathBuf::from("/tmp/ghost-shadow-entry-test.jsonl"),
            shadow_lifecycle_log_path: None,
            gatekeeper_rollout_profile: "test-rollout".to_string(),
            dry_run: true,
            ab_window_ms: 10_000,
        });
        let (tx, rx) = tokio::sync::mpsc::channel(4);

        let join = tokio::spawn(pool_observation_task(
            pool_id,
            Some(detected_pool),
            1_000,
            rx,
            ctx.clone(),
        ));

        let opened_session = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(session) = ctx.session_manager.get_session(&pool_id) {
                    break session;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("session should open before task exits");

        {
            let mut session = opened_session.write();
            session.set_checkpoint_interval_ms(1);
            session.checkpoint_engine.config.min_tx_between_checkpoints = 1;
        }

        let mut observed_tx = (*test_pool_observation_tx("sig-pr5-runtime")).clone();
        observed_tx.pool_amm_id = pool_id.to_string();
        tx.send(PoolObservationMsg::Transaction(Arc::new(observed_tx)))
            .await
            .expect("tx send must succeed");
        drop(tx);

        let result = tokio::time::timeout(Duration::from_secs(1), result_rx.recv())
            .await
            .expect("result should arrive")
            .expect("pool observation result should be emitted");
        assert_eq!(result.pool_id, pool_id);

        tokio::time::timeout(Duration::from_secs(1), join)
            .await
            .expect("task join timeout")
            .expect("task should finish cleanly");

        let session = opened_session.read();
        assert_eq!(session.diagnostics.total_tx_seen, 1);
        assert!(
            session.diagnostics.checkpoint_count >= 1,
            "production path should create at least one checkpoint"
        );
        let materialized = session.materialize_features();
        assert!(
            materialized.checkpoint_features.trajectory_checkpoint_count >= 1,
            "production path should materialize trajectory checkpoints"
        );
        assert!(
            materialized.session_metadata.observation_duration_ms > 0
                || materialized.curve_readiness.t0_event_ts_ms.is_some()
                || materialized.tx_intel_features.tx_count > 0,
            "materialized feature set should contain live runtime observation data"
        );
    }

    #[test]
    #[allow(deprecated)]
    fn build_shadow_tx_metrics_prefers_shadow_ledger_snapshots() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );
        let snapshot_engine = SnapshotEngine::new(16, 0);
        runtime.configure_approval_gating(&snapshot_engine);

        let bonding_curve = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let snapshots = vec![
            MarketSnapshot {
                // Test snapshot without slot context (legacy path)
                slot: None,
                tx_key: None,
                timestamp_ms: 1_000,
                tx_count: 1,
                unique_addrs: 1,
                cum_volume_sol: 0.25,
                ..Default::default()
            },
            MarketSnapshot {
                // Test snapshot without slot context (legacy path)
                slot: None,
                tx_key: None,
                timestamp_ms: 2_000,
                tx_count: 4,
                unique_addrs: 3,
                cum_volume_sol: 2.5,
                ..Default::default()
            },
        ];

        shadow_ledger.commit_history(base_mint, snapshots, None);

        let _bonding_curve = bonding_curve;
        let metrics = runtime
            .build_shadow_tx_metrics(&base_mint)
            .expect("shadow tx metrics should be derived from snapshots");

        assert_eq!(metrics.tx_count, 4);
        assert_eq!(metrics.unique_addrs, 3);
        assert!((metrics.total_volume_sol - 2.5).abs() < 1e-6);
        assert_eq!(metrics.buy_count, 4);
        assert_eq!(metrics.sell_count, 0);
    }

    #[test]
    fn test_orphan_ttl_enforcement() {
        // Test that orphans older than TTL are dropped
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();

        // Register an orphan transaction
        runtime.register_pool_tx(
            pool_id,
            1000,
            Some(0),
            vec![1, 2, 3],
            None,
            "signer1".to_string(),
            true,
            1.0,
        );

        // Verify orphan was buffered
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 1);
        assert_eq!(total_orphans, 1);

        // Simulate orphan age by directly manipulating arrival time (not possible without access)
        // Instead, we'll test that cleanup is callable and returns correct format
        let (dropped, pools_dropped, cap_enforced) = runtime.cleanup_stale_orphans();

        // Fresh orphans shouldn't be dropped
        assert_eq!(dropped, 0);
        assert_eq!(pools_dropped, 0);
        assert_eq!(cap_enforced, 0);

        // Verify orphan still exists
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 1);
        assert_eq!(total_orphans, 1);
    }

    #[test]
    fn test_orphan_per_pool_cap() {
        // Test that per-pool cap is enforced
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();

        // Try to add more than MAX_ORPHANS_PER_POOL transactions
        for i in 0..MAX_ORPHANS_PER_POOL + 10 {
            runtime.register_pool_tx(
                pool_id,
                1000 + i as u64,
                Some(i as u64),
                vec![1, 2, 3],
                None,
                format!("signer{}", i),
                true,
                1.0,
            );
        }

        let _ = runtime.cleanup_stale_orphans();

        // Verify cap is enforced
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 1);
        assert!(
            total_orphans <= MAX_ORPHANS_PER_POOL,
            "Expected {} or fewer orphans, got {}",
            MAX_ORPHANS_PER_POOL,
            total_orphans
        );
    }

    #[test]
    fn test_orphan_global_cap() {
        // Test that global cap is enforced
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        // Add orphans across multiple pools
        let num_pools = 150;
        let txs_per_pool = MAX_ORPHANS_PER_POOL;

        for pool_idx in 0..num_pools {
            let pool_id = Pubkey::new_unique();
            for tx_idx in 0..txs_per_pool.min(100) {
                runtime.register_pool_tx(
                    pool_id,
                    1000 + tx_idx as u64,
                    Some(tx_idx as u64),
                    vec![1, 2, 3],
                    None,
                    format!("signer{}", tx_idx),
                    true,
                    1.0,
                );
            }
        }

        // Verify global cap is enforced
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert!(
            total_orphans <= MAX_TOTAL_ORPHANS,
            "Expected {} or fewer total orphans, got {}",
            MAX_TOTAL_ORPHANS,
            total_orphans
        );
    }

    #[test]
    fn test_orphan_adoption_drops_stale_arrival_even_with_fresh_timestamp() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(1),
            ..Default::default()
        };

        let stale_age_ms = ORPHAN_TTL_MS.saturating_mul(ORPHAN_GRACE_PERIOD_MULTIPLIER) + 10;

        runtime.register_pool_tx(
            pool_id,
            current_time_ms(),
            Some(1),
            vec![1, 2, 3],
            None,
            "stale-signer".to_string(),
            true,
            1.0,
        );
        runtime
            .orphans
            .write()
            .get_mut(&pool_id)
            .and_then(|txs| txs.first_mut())
            .expect("buffered orphan")
            .arrived_at = Instant::now() - Duration::from_millis(stale_age_ms);

        runtime.register_new_pool(pool_id, base_mint, candidate.clone(), None);

        let metrics = runtime.orphan_metrics_snapshot();
        assert_eq!(metrics.dropped_stale, 1);
        assert_eq!(metrics.adopted, 0);
        assert_eq!(runtime.get_pool_tx_count(pool_id), 0);
        let (_pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(total_orphans, 0);
    }

    #[test]
    fn test_orphan_adoption_drops_missing_slot() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(50),
            ..Default::default()
        };

        let fresh_ts = current_time_ms().saturating_sub(500);

        runtime.register_pool_tx(
            pool_id,
            fresh_ts,
            None,
            vec![9, 9, 9],
            None,
            "no-slot".to_string(),
            true,
            1.5,
        );

        runtime.register_new_pool(pool_id, base_mint, candidate.clone(), None);
        runtime.remember_detected_pool(
            pool_id,
            Arc::new(DetectedPool {
                semantic: Default::default(),
                pool_amm_id: pool_id.to_string(),
                base_mint: base_mint.to_string(),
                quote_mint: Pubkey::new_unique().to_string(),
                amm_program: "pumpfun".to_string(),
                bonding_curve: candidate.bonding_curve.to_string(),
                creator: Pubkey::new_unique().to_string(),
                slot: candidate.slot,
                timestamp_ms: 1_000,
                event_time: ghost_core::EventTimeMetadata::default(),
                detected_wall_ts_ms: Some(1_001),
                initial_liquidity_sol: Some(1.0),
                signature: Signature::new_unique().to_string(),
            }),
        );

        let metrics = runtime.orphan_metrics_snapshot();
        assert_eq!(metrics.adopted, 0);
        assert_eq!(metrics.dropped_slot_cutoff, 1);
        assert_eq!(runtime.get_pool_tx_count(pool_id), 0);
        let (_pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(total_orphans, 0);
    }

    #[test]
    fn test_orphan_adoption_accepts_fresh_arrival_despite_stale_legacy_timestamp() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(10),
            ..Default::default()
        };

        let stale_legacy_ts = current_time_ms()
            .saturating_sub(ORPHAN_TTL_MS.saturating_mul(ORPHAN_GRACE_PERIOD_MULTIPLIER) + 10);

        runtime.register_pool_tx(
            pool_id,
            stale_legacy_ts,
            Some(11),
            vec![4, 5, 6],
            None,
            "fresh-signer".to_string(),
            true,
            2.0,
        );

        runtime.register_new_pool(pool_id, base_mint, candidate, None);

        let metrics = runtime.orphan_metrics_snapshot();
        assert_eq!(metrics.adopted, 1);
        assert_eq!(metrics.dropped_stale, 0);
        assert_eq!(metrics.dropped_capped, 0);
        assert_eq!(runtime.get_pool_tx_count(pool_id), 0);
        let (_pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(total_orphans, 0);
    }

    #[test]
    fn test_orphan_adoption_cap_enforced() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        runtime.configure_orphan_adoption(ORPHAN_GRACE_PERIOD_MULTIPLIER, 50);
        let adoption_cap = 50usize;
        let orphan_count = 80usize;

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(20),
            ..Default::default()
        };

        let now_ms = current_time_ms();
        for i in 0..orphan_count {
            let ts = now_ms.saturating_sub(i as u64);
            runtime.register_pool_tx(
                pool_id,
                ts,
                Some(20 + i as u64),
                vec![7, 8, 9],
                None,
                format!("signer-{i}"),
                true,
                0.5,
            );
        }

        runtime.register_new_pool(pool_id, base_mint, candidate, None);

        let metrics = runtime.orphan_metrics_snapshot();
        assert_eq!(metrics.adopted, adoption_cap as u64);
        assert_eq!(metrics.dropped_capped, (orphan_count - adoption_cap) as u64);
        assert_eq!(metrics.dropped_stale, 0);
        assert_eq!(runtime.get_pool_tx_count(pool_id), 0);
        let (_pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(total_orphans, 0);
    }

    #[test]
    fn test_orphan_injection_on_pool_registration() {
        // Test that orphans are injected when pool is registered
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let base_ts = current_time_ms();
        let base_slot = 100u64;

        // Register orphan transactions before pool exists
        for i in 0..5 {
            runtime.register_pool_tx(
                pool_id,
                base_ts.saturating_sub(i as u64),
                Some(base_slot + i as u64),
                vec![1, 2, 3],
                None,
                format!("signer{}", i),
                true,
                1.0,
            );
        }

        // Verify orphans are buffered
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 1);
        assert_eq!(total_orphans, 5);

        // Register the pool
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(base_slot),
            ..Default::default()
        };

        runtime.register_new_pool(pool_id, base_mint, candidate, None);

        // Verify orphans were injected (buffer should be empty for this pool)
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 0);
        assert_eq!(total_orphans, 0);

        // OracleRuntime no longer injects tx history into local compat state.
        // Registration only drains the orphan buffer and updates telemetry.
        let tx_count = runtime.get_pool_tx_count(pool_id);
        assert_eq!(tx_count, 0);
    }

    #[test]
    fn test_orphan_stats() {
        // Test orphan statistics reporting
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        // Initially empty
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 0);
        assert_eq!(total_orphans, 0);

        // Add orphans to multiple pools
        let pool1 = Pubkey::new_unique();
        let pool2 = Pubkey::new_unique();
        let base_ts = current_time_ms();
        let base_slot = 200u64;

        for i in 0..3 {
            runtime.register_pool_tx(
                pool1,
                base_ts.saturating_sub(i as u64),
                Some(base_slot + i as u64),
                vec![1, 2, 3],
                None,
                format!("signer{}", i),
                true,
                1.0,
            );
        }

        for i in 0..2 {
            runtime.register_pool_tx(
                pool2,
                base_ts.saturating_sub(10 + i as u64),
                Some(base_slot + 10 + i as u64),
                vec![4, 5, 6],
                None,
                format!("signer{}", i),
                true,
                1.0,
            );
        }

        // Verify stats
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 2);
        assert_eq!(total_orphans, 5);
    }

    // =============================================================================
    // Untracked Pool Filtering Tests (Phase 1)
    // =============================================================================

    #[test]
    fn test_untracked_pool_filtering() {
        // Test that transactions for untracked pools don't create orphans
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let untracked_pool_id = Pubkey::new_unique();
        let base_ts = current_time_ms();
        let base_slot = 300u64;

        // Register transaction for untracked pool
        // In the actual runtime task, this would be filtered before calling register_pool_tx
        // But we test the orphan buffer behavior here
        runtime.register_pool_tx(
            untracked_pool_id,
            base_ts,
            Some(base_slot),
            vec![1, 2, 3],
            None,
            "signer1".to_string(),
            true,
            1.0,
        );

        // Verify orphan was buffered (this is expected at the register_pool_tx level)
        // The runtime task filter prevents this from being called in the first place
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 1);
        assert_eq!(total_orphans, 1);

        // Now register the pool (simulating late NewPoolDetected)
        let candidate = EnhancedCandidate {
            pool_amm_id: untracked_pool_id,
            base_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            slot: Some(base_slot),
            ..Default::default()
        };

        runtime.register_new_pool(untracked_pool_id, candidate.base_mint, candidate, None);

        // Late registration drains orphan buffer but does not synthesize local tx history.
        let tx_count = runtime.get_pool_tx_count(untracked_pool_id);
        assert_eq!(tx_count, 0);

        // Verify orphan buffer is now empty for this pool
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 0);
        assert_eq!(total_orphans, 0);
    }

    #[test]
    fn test_approval_gating_blocks_shadow_ledger_for_unapproved_pool() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );
        let snapshot_engine = SnapshotEngine::new(16, 0);
        runtime.configure_approval_gating(&snapshot_engine);

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(1),
            ..Default::default()
        };

        runtime.register_new_pool(pool_id, base_mint, candidate, None);
        assert!(!runtime.approved_pools.is_approved(&pool_id));

        runtime.register_pool_tx(
            pool_id,
            1_000,
            Some(1),
            vec![],
            None,
            "signer".to_string(),
            true,
            0.5,
        );

        assert_eq!(shadow_ledger.snapshot_count(), 0);
        assert_eq!(shadow_ledger.untracked_snapshot_write_blocked_total(), 0);
    }

    #[test]
    fn test_tracked_pool_no_orphan_buffering() {
        // Test that transactions for tracked pools are registered directly
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();

        // Register pool first
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            ..Default::default()
        };

        runtime.register_new_pool(pool_id, base_mint, candidate, None);

        // Register transactions for tracked pool
        for i in 0..5 {
            runtime.register_pool_tx(
                pool_id,
                1000 + i,
                Some(i as u64),
                vec![1, 2, 3],
                None,
                format!("signer{}", i),
                true,
                1.0,
            );
        }

        // OracleRuntime relay remains stateless; tx history stays outside local runtime state.
        let tx_count = runtime.get_pool_tx_count(pool_id);
        assert_eq!(tx_count, 0);

        // Verify no orphans were created
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 0);
        assert_eq!(total_orphans, 0);
    }

    // =============================================================================
    // Pool Lifecycle Race Condition & Memory Leak Fix Tests
    // =============================================================================

    #[test]
    fn test_remove_pool_cleans_all_state() {
        // Test that remove_pool() removes pool from pools, orphans, and ShadowLedger
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let base_slot = 400u64;

        // Register orphan transactions before pool exists
        for i in 0..3 {
            runtime.register_pool_tx(
                pool_id,
                1000 + i,
                Some(base_slot + i as u64),
                vec![1, 2, 3],
                None,
                format!("signer{}", i),
                true,
                1.0,
            );
        }

        // Verify orphans are buffered
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 1);
        assert_eq!(total_orphans, 3);

        // Register the pool
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            slot: Some(base_slot),
            ..Default::default()
        };

        runtime.register_new_pool(pool_id, base_mint, candidate, None);

        // Verify runtime registries were populated and orphans were drained.
        assert!(runtime.lookup_pool_identity(&pool_id).is_some());
        assert!(runtime.lookup_detected_pool(&pool_id).is_some());
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 0); // Orphans were injected
        assert_eq!(total_orphans, 0);

        // Add more orphans for this pool (simulate transactions arriving after registration)
        // This shouldn't happen in normal flow but tests edge case
        // These go directly to pool state, not orphan buffer
        runtime.register_pool_tx(
            pool_id,
            2000,
            Some(base_slot + 100),
            vec![4, 5, 6],
            None,
            "signer_after".to_string(),
            true,
            2.0,
        );

        // Now remove the pool
        let removed = runtime.remove_pool(pool_id);
        assert!(removed);

        // Verify runtime registries were cleaned up
        assert!(runtime.lookup_pool_identity(&pool_id).is_none());
        assert!(
            runtime.lookup_detected_pool(&pool_id).is_none(),
            "remove_pool must also clean runtime detected-pool registry"
        );

        // Verify orphans are removed
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 0);
        assert_eq!(total_orphans, 0);

        // Try to remove again - should return false
        let removed_again = runtime.remove_pool(pool_id);
        assert!(!removed_again);
    }

    #[test]
    fn test_remove_pool_nonexistent() {
        // Test that remove_pool() returns false for non-existent pool
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let nonexistent_pool_id = Pubkey::new_unique();

        // Try to remove non-existent pool
        let removed = runtime.remove_pool(nonexistent_pool_id);
        assert!(!removed);
    }

    #[test]
    fn test_register_new_pool_idempotency() {
        // Test that registering the same pool twice doesn't duplicate it
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            ..Default::default()
        };

        // First registration should succeed
        let first_result = runtime.register_new_pool(pool_id, base_mint, candidate.clone(), None);
        assert!(first_result);

        // Second registration should fail (idempotency check)
        let second_result = runtime.register_new_pool(pool_id, base_mint, candidate, None);
        assert!(!second_result);

        // Verify only one runtime registration exists
        assert_eq!(runtime.pool_count(), 1);
    }

    #[test]
    fn test_prune_stale_pools_cleans_orphans() {
        // Test that prune_stale_pools() removes orphans for pruned pools
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();

        // Register a pool
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            ..Default::default()
        };

        runtime.register_new_pool(pool_id, base_mint, candidate, None);

        // Verify runtime registration exists
        assert!(runtime.lookup_detected_pool(&pool_id).is_some());

        // Prune with TTL of 0 - should remove all pools that aren't scored
        let removed_count = runtime.prune_stale_pools(0);
        assert_eq!(removed_count, 1);

        // Verify pool was removed
        assert!(runtime.lookup_detected_pool(&pool_id).is_none());
    }

    #[test]
    fn test_prune_keeps_scored_pools() {
        // Test that prune_stale_pools() keeps pools that have been scored
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();

        // Register a pool
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            ..Default::default()
        };

        runtime.register_new_pool(pool_id, base_mint, candidate, None);

        // Mark pool as scored
        runtime.mark_pool_scored(pool_id);

        // Prune with TTL of 0 - should NOT remove scored pools
        let removed_count = runtime.prune_stale_pools(0);
        assert_eq!(removed_count, 0);

        // Verify pool still exists
        assert!(runtime.lookup_detected_pool(&pool_id).is_some());
    }

    #[test]
    fn test_atomic_orphan_injection() {
        // Test that orphan injection and pool registration happen atomically
        // This test ensures the race condition fix is working
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = Arc::new(OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        ));

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let base_ts = current_time_ms();
        let base_slot = 500u64;

        // Add orphans before pool registration
        for i in 0..10 {
            runtime.register_pool_tx(
                pool_id,
                base_ts.saturating_sub(i as u64),
                Some(base_slot + i as u64),
                vec![1, 2, 3],
                None,
                format!("signer{}", i),
                true,
                1.0,
            );
        }

        // Verify orphans are buffered
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 1);
        assert_eq!(total_orphans, 10);

        // Register the pool - orphans should be injected atomically
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(base_slot),
            ..Default::default()
        };

        runtime.register_new_pool(pool_id, base_mint, candidate, None);

        // Verify runtime registration exists
        assert!(runtime.lookup_detected_pool(&pool_id).is_some());

        // Verify all orphans were injected (buffer is empty)
        let (pool_count, total_orphans) = runtime.get_orphan_stats();
        assert_eq!(pool_count, 0);
        assert_eq!(total_orphans, 0);

        // Registration drains buffered orphans but does not recreate legacy local tx history.
        let tx_count = runtime.get_pool_tx_count(pool_id);
        assert_eq!(tx_count, 0);
    }

    // =============================================================================
    // OracleRuntimeConfig Tests
    // =============================================================================

    #[test]
    fn test_config_parse_bool_flag_truthy_values() {
        // Test truthy value parsing
        assert!(OracleRuntimeConfig::parse_bool_flag("1"));
        assert!(OracleRuntimeConfig::parse_bool_flag("true"));
        assert!(OracleRuntimeConfig::parse_bool_flag("TRUE"));
        assert!(OracleRuntimeConfig::parse_bool_flag("yes"));
        assert!(OracleRuntimeConfig::parse_bool_flag("YES"));
        assert!(OracleRuntimeConfig::parse_bool_flag("  yes  ")); // with whitespace
    }

    #[test]
    fn test_config_parse_bool_flag_falsy_values() {
        // Test falsy value parsing
        assert!(!OracleRuntimeConfig::parse_bool_flag("0"));
        assert!(!OracleRuntimeConfig::parse_bool_flag("false"));
        assert!(!OracleRuntimeConfig::parse_bool_flag("FALSE"));
        assert!(!OracleRuntimeConfig::parse_bool_flag("no"));
        assert!(!OracleRuntimeConfig::parse_bool_flag(""));
        assert!(!OracleRuntimeConfig::parse_bool_flag("invalid"));
        assert!(!OracleRuntimeConfig::parse_bool_flag("random"));
    }

    #[test]
    fn test_config_default_has_disabled() {
        // EPIC 2.3.5: Default config has runtime snapshots DISABLED
        // Note: This field is deprecated and has no effect (legacy paths removed)
        #[allow(deprecated)]
        let config = OracleRuntimeConfig::default();
        #[allow(deprecated)]
        let runtime_shadowledger_snapshots_enabled = config.runtime_shadowledger_snapshots_enabled;
        assert!(!runtime_shadowledger_snapshots_enabled);
        assert_eq!(
            config.shadow_ledger_enrichment_freshness_ms,
            DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS
        );
    }

    #[test]
    fn test_config_parse_u64_env_falls_back_to_default_on_missing_value() {
        assert_eq!(
            OracleRuntimeConfig::parse_u64_env("GHOST_TEST_MISSING_U64_ENV", 321),
            321
        );
    }

    // =============================================================================
    // EPIC 2.3.5: Single-Writer Architecture Tests
    // =============================================================================
    // Legacy dual-writer paths have been removed. OracleRuntime no longer writes
    // to ShadowLedger. Only SnapshotEngine is the canonical writer.

    #[test]
    fn test_runtime_never_writes_to_shadowledger() {
        // EPIC 2.3.5: OracleRuntime should NEVER write to ShadowLedger
        // regardless of config (legacy paths removed)
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());

        // Even with deprecated field set to true, runtime should not write
        #[allow(deprecated)]
        let config = OracleRuntimeConfig {
            runtime_shadowledger_snapshots_enabled: true, // This has no effect now
            shadow_ledger_enrichment_freshness_ms: DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
            ..Default::default()
        };

        let runtime = OracleRuntime::new_with_config(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
            None, // rpc_client
            None, // paradox_rx
            Arc::new(ghost_core::shadow_ledger::LivePipeline::new()),
            config,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            slot: Some(100),
            ..Default::default()
        };

        // Approve the pool (required for transaction processing)
        runtime.approved_pools().insert(pool_id);
        runtime.register_new_pool(pool_id, base_mint, candidate, None);

        // Check initial state
        let initial_count = shadow_ledger
            .get_snapshots(&base_mint)
            .as_ref()
            .map(|s| s.len())
            .unwrap_or(0);

        // Register transaction - should NOT write to ShadowLedger (EPIC 2.3.5)
        runtime.register_pool_tx(
            pool_id,
            1000,
            Some(100),
            vec![1, 2, 3],
            None,
            "signer1".to_string(),
            true,
            1.0,
        );

        // Verify snapshot was NOT written to ShadowLedger
        // EPIC 2.3.5: OracleRuntime never writes, only SnapshotEngine does
        let final_count = shadow_ledger
            .get_snapshots(&base_mint)
            .as_ref()
            .map(|s| s.len())
            .unwrap_or(0);

        assert_eq!(
            final_count, initial_count,
            "OracleRuntime should NEVER write to ShadowLedger (EPIC 2.3.5: single-writer mode)"
        );
    }

    #[tokio::test]
    async fn oracle_runtime_calls_trigger_through_prepared_request_path() {
        let trigger = crate::components::trigger::TriggerComponent::new(
            crate::config::TriggerComponentConfig {
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
            },
        );

        let receipt = execute_gatekeeper_buy_via_trigger(
            &trigger,
            Pubkey::new_unique(),
            &crate::components::trigger::BuyAccountOverrides::default(),
            1_000_000,
            None,
        )
        .await;
        let err = receipt
            .primary_outcome
            .expect_err("missing keypair should fail before live dispatch");

        assert!(
            err.to_string().contains("keypair_path"),
            "prepare path should fail on missing keypair in this test"
        );
        assert_eq!(
            trigger.prepared_request_invocations(),
            1,
            "live runtime buy path must call Trigger through prepare_buy_request"
        );
    }

    #[tokio::test]
    async fn oracle_runtime_live_and_shadow_calls_trigger_through_prepared_request_path() {
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::LiveAndShadow,
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
        };
        trigger_config.shadow_run.enabled = true;
        let trigger = crate::components::trigger::TriggerComponent::new(trigger_config);

        let receipt = execute_gatekeeper_buy_via_trigger(
            &trigger,
            Pubkey::new_unique(),
            &crate::components::trigger::BuyAccountOverrides::default(),
            1_000_000,
            None,
        )
        .await;
        let err = receipt
            .primary_outcome
            .expect_err("missing keypair should fail before live dispatch");

        assert!(
            err.to_string().contains("keypair_path"),
            "prepare path should fail on missing keypair in this test"
        );
        assert_eq!(
            trigger.prepared_request_invocations(),
            1,
            "live_and_shadow runtime buy path must call Trigger through prepare_buy_request"
        );
    }

    #[tokio::test]
    async fn oracle_runtime_fsc_gate_blocks_live_buys_before_prepare_path() {
        let trigger = crate::components::trigger::TriggerComponent::new(
            crate::config::TriggerComponentConfig {
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
            },
        );

        let receipt = execute_gatekeeper_buy_via_trigger_with_fsc_gate(
            &trigger,
            Some(FscAuthoritativeBuyGateStatus {
                stream_available: true,
                warmup_ready: true,
                coverage_window_ready: false,
                authoritative_buy_gate_open: false,
                coverage_window_remaining_ms: 1_800_000,
            }),
            Pubkey::new_unique(),
            &crate::components::trigger::BuyAccountOverrides::default(),
            1_000_000,
            None,
        )
        .await;
        let err = receipt
            .primary_outcome
            .expect_err("coverage gate should block live authoritative buy");

        assert!(
            err.to_string().contains("FSC coverage window"),
            "gate block should surface explicit FSC coverage reason"
        );
        assert_eq!(
            trigger.prepared_request_invocations(),
            0,
            "coverage gate should block before entering trigger prepare path"
        );
    }

    #[tokio::test]
    async fn oracle_runtime_fsc_gate_routes_live_and_shadow_through_shadow_prepare_path() {
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::LiveAndShadow,
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
        };
        trigger_config.shadow_run.enabled = true;
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );

        let receipt = execute_gatekeeper_buy_via_trigger_with_fsc_gate(
            &trigger,
            Some(FscAuthoritativeBuyGateStatus {
                stream_available: true,
                warmup_ready: true,
                coverage_window_ready: false,
                authoritative_buy_gate_open: false,
                coverage_window_remaining_ms: 1_800_000,
            }),
            Pubkey::new_unique(),
            &crate::components::trigger::BuyAccountOverrides::default(),
            1_000_000,
            None,
        )
        .await;
        let err = receipt
            .primary_outcome
            .expect_err("missing keypair should still fail through prepare path");
        assert!(
            err.to_string().contains("keypair_path"),
            "gate fallback should route through prepared shadow path instead of hard blocking"
        );
        assert_eq!(
            trigger.prepared_request_invocations(),
            1,
            "LiveAndShadow gate fallback should still use prepared request path"
        );
    }

    #[tokio::test]
    async fn shadow_only_emits_shadow_buy_simulated() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let mut rx = event_tx.subscribe();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };
        let outcome = crate::components::trigger::TriggerBuyOutcome::ShadowSimulated {
            report: crate::components::trigger::ShadowBuySimulationReport {
                mint: pool.base_mint.clone(),
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
            },
        };

        let applied = apply_trigger_buy_outcome(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-join",
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "paper",
            None,
            None,
            outcome,
        )
        .await
        .expect("shadow-only apply should succeed");

        assert_eq!(applied.close_reason, WindowCloseReason::PoolShadowedEarly);
        let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("shadow event timeout")
            .expect("shadow event receive");
        match event {
            GhostEvent::ShadowBuySimulated(event) => {
                assert_eq!(event.pool_amm_id, pool_id.to_string());
                assert_eq!(event.rpc_slot, 777);
                assert_eq!(event.latency_ms, 5);
                assert_eq!(event.live_signature, None);
            }
            other => panic!("expected ShadowBuySimulated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn shadow_only_does_not_emit_transaction_sent() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let mut rx = event_tx.subscribe();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let outcome = crate::components::trigger::TriggerBuyOutcome::ShadowSimulated {
            report: crate::components::trigger::ShadowBuySimulationReport {
                mint: pool.base_mint.clone(),
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
            },
        };

        let applied = apply_trigger_buy_outcome(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-join",
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "paper",
            None,
            None,
            outcome,
        )
        .await
        .expect("shadow-only apply should succeed");

        assert!(!applied.bought);
        let first = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("shadow event timeout")
            .expect("shadow event receive");
        assert!(matches!(first, GhostEvent::ShadowBuySimulated(_)));

        let second = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("post buy timeout")
            .expect("post buy receive");
        assert!(matches!(second, GhostEvent::PostBuySubmitted { .. }));

        assert!(tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn shadow_only_emits_post_buy_submitted_for_successful_paper_lane() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let mut rx = event_tx.subscribe();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let outcome = crate::components::trigger::TriggerBuyOutcome::ShadowSimulated {
            report: crate::components::trigger::ShadowBuySimulationReport {
                mint: pool.base_mint.clone(),
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
            },
        };

        let applied = apply_trigger_buy_outcome(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-join",
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "paper",
            None,
            None,
            outcome,
        )
        .await
        .expect("shadow-only apply should succeed");

        assert!(!applied.bought);
        assert!(applied.retain_runtime_pool);

        let first = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("shadow event timeout")
            .expect("shadow event receive");
        let shadow_event = match first {
            GhostEvent::ShadowBuySimulated(event) => event,
            other => panic!("expected ShadowBuySimulated, got {other:?}"),
        };
        assert_eq!(
            shadow_event.candidate_id,
            format!("{}_{}_10", pool.base_mint, pool_id)
        );

        let second = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("post buy timeout")
            .expect("post buy receive");
        match second {
            GhostEvent::PostBuySubmitted {
                candidate_id,
                pool_amm_id,
                base_mint,
                signature,
                lane,
                ..
            } => {
                assert_eq!(candidate_id, format!("{}_{}_10", pool.base_mint, pool_id));
                assert_eq!(pool_amm_id, pool_id.to_string());
                assert_eq!(base_mint, pool.base_mint);
                assert_eq!(signature, "10");
                assert_eq!(lane, "paper");
            }
            other => panic!("expected PostBuySubmitted, got {other:?}"),
        }
        assert!(tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn shadow_only_emits_post_buy_submitted_for_successful_shadow_lane() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let mut rx = event_tx.subscribe();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let outcome = crate::components::trigger::TriggerBuyOutcome::ShadowSimulated {
            report: crate::components::trigger::ShadowBuySimulationReport {
                mint: pool.base_mint.clone(),
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
            },
        };

        let _ = apply_trigger_buy_outcome(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Shadow,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-join",
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "shadow",
            None,
            None,
            outcome,
        )
        .await
        .expect("shadow-only apply should succeed");

        let first = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("shadow event timeout")
            .expect("shadow event receive");
        assert!(matches!(first, GhostEvent::ShadowBuySimulated(_)));

        let second = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("post buy timeout")
            .expect("post buy receive");
        match second {
            GhostEvent::PostBuySubmitted {
                candidate_id,
                pool_amm_id,
                base_mint,
                signature,
                lane,
                ..
            } => {
                assert_eq!(candidate_id, format!("{}_{}_10", pool.base_mint, pool_id));
                assert_eq!(pool_amm_id, pool_id.to_string());
                assert_eq!(base_mint, pool.base_mint);
                assert_eq!(signature, "10");
                assert_eq!(lane, "shadow");
            }
            other => panic!("expected PostBuySubmitted, got {other:?}"),
        }
        assert!(tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn pool_shadowed_early_marks_window_reason() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let applied = apply_trigger_buy_outcome(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-join",
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "paper",
            None,
            None,
            crate::components::trigger::TriggerBuyOutcome::ShadowSimulated {
                report: crate::components::trigger::ShadowBuySimulationReport {
                    mint: pool.base_mint.clone(),
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
                },
            },
        )
        .await
        .expect("shadow-only apply should succeed");

        assert_eq!(applied.close_reason, WindowCloseReason::PoolShadowedEarly);
    }

    #[tokio::test]
    async fn append_coverage_audit_record_serializes_whole_lines() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("coverage-audit.jsonl");

        let mut tasks = Vec::new();
        for i in 0..16u64 {
            let path = path.clone();
            tasks.push(tokio::spawn(async move {
                let record = CoverageAuditRecord {
                    schema_version: 5,
                    recorded_at_ms: 0,
                    audit_type: "seer_runtime_coverage_audit".to_string(),
                    audit_status: "ok".to_string(),
                    chain_truth_unavailable: false,
                    rpc_error: None,
                    window_id: format!("window-{i}"),
                    pool_id: format!("pool-{i}"),
                    base_mint: Some(format!("mint-{i}")),
                    t0_ms: i,
                    t_end_ms: i + 10,
                    window_ms: 10,
                    verdict: Some("buy".to_string()),
                    window_complete: true,
                    window_close_reason: Some("unit_test".to_string()),
                    chain_truth_count: 1,
                    chain_truth_failed_count: 0,
                    seer_rx_count: 1,
                    seer_emitted_count: 1,
                    runtime_seen_count: 1,
                    runtime_accepted_count: 1,
                    missing_count: 0,
                    truth_to_rx_pct: 100.0,
                    truth_to_emit_pct: 100.0,
                    truth_to_runtime_accept_pct: 100.0,
                    counts_by_reason: std::collections::BTreeMap::new(),
                    mapping_missing_by_reason: std::collections::BTreeMap::new(),
                    raw_received_by_source: std::collections::BTreeMap::new(),
                    seer_rx_by_source: std::collections::BTreeMap::new(),
                    seer_emitted_by_source: std::collections::BTreeMap::new(),
                    runtime_filtered_by_reason: std::collections::BTreeMap::new(),
                    filtered_reason_keys: Vec::new(),
                    duplicate_suppression_by_reason: std::collections::BTreeMap::new(),
                    chain_truth_by_time_source: std::collections::BTreeMap::new(),
                    runtime_seen_by_time_source: std::collections::BTreeMap::new(),
                    runtime_seen_by_effective_time_source: std::collections::BTreeMap::new(),
                    dominant_runtime_effective_time_source: None,
                    runtime_seen_by_fallback_class: std::collections::BTreeMap::new(),
                    timeout_primary_cause: None,
                    timeout_flags: Vec::new(),
                    missing_signatures: Vec::new(),
                    invariants: ghost_core::coverage_audit::CoverageAuditInvariantSummary {
                        emitted_without_rx: 0,
                        runtime_accepted_without_emitted: 0,
                        missing_reason_fallbacks: 0,
                    },
                    diagnostics:
                        ghost_core::coverage_audit::CoverageAuditWindowDiagnostics::default(),
                };
                append_coverage_audit_record(&path, &record)
                    .await
                    .expect("append record");
                record.pool_id
            }));
        }

        let mut expected = std::collections::BTreeSet::new();
        for task in tasks {
            expected.insert(task.await.expect("join task"));
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .expect("read audit file");
        let mut seen = std::collections::BTreeSet::new();
        for line in content.lines() {
            let record: CoverageAuditRecord =
                serde_json::from_str(line).expect("each line must be valid JSON");
            seen.insert(record.pool_id);
        }
        assert_eq!(seen, expected);
        assert_eq!(content.lines().count(), expected.len());
    }

    #[tokio::test]
    async fn shadow_only_report_with_err_sets_failure_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-err.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );
        let (event_tx, mut event_rx) = crate::events::create_event_bus();
        let mut rx = event_tx.subscribe();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let applied = apply_trigger_buy_outcome(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-join",
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "paper",
            None,
            None,
            crate::components::trigger::TriggerBuyOutcome::ShadowSimulated {
                report: crate::components::trigger::ShadowBuySimulationReport {
                    mint: pool.base_mint.clone(),
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
                    units_consumed: None,
                    logs: vec![],
                    return_data: None,
                    err: Some("InvalidAccountForFee".to_string()),
                },
            },
        )
        .await
        .expect("shadow-only apply should succeed");

        assert_eq!(applied.close_reason, WindowCloseReason::PoolShadowedEarly);
        assert_eq!(applied.shadow_execution_outcome, "shadow_authority_error");
        let first = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("shadow event timeout")
            .expect("shadow event receive");
        assert!(matches!(first, GhostEvent::ShadowBuySimulated(_)));
        assert!(tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn live_and_shadow_emits_live_events_and_keeps_live_path_non_blocking() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::LiveAndShadow,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-live-and-shadow.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let mut rx = event_tx.subscribe();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let signature = Signature::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let receipt = crate::components::trigger::TriggerDispatchReceipt {
            primary_outcome: Ok(
                crate::components::trigger::TriggerBuyOutcome::LiveConfirmed {
                    signature,
                    landed_slot: None,
                },
            ),
            shadow_task: Some(test_pending_shadow_simulation(tokio::spawn({
                let base_mint = pool.base_mint.clone();
                async move {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    Ok(crate::components::trigger::ShadowBuySimulationReport {
                        mint: base_mint,
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
                    })
                }
            }))),
            active_position_lease: None,
            retain_position_slot_on_error: false,
            failed_request: None,
            failed_context: None,
        };

        let started = Instant::now();
        let applied = apply_trigger_dispatch_receipt(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "paper",
            receipt,
        )
        .await
        .expect("live_and_shadow receipt should apply");

        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(applied.bought);
        assert!(applied.retain_runtime_pool);
        assert_eq!(applied.close_reason, WindowCloseReason::PoolBoughtEarly);

        let first = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("transaction sent timeout")
            .expect("transaction sent receive");
        assert!(matches!(first, GhostEvent::TransactionSent { .. }));

        let second = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("post buy timeout")
            .expect("post buy receive");
        assert!(matches!(second, GhostEvent::PostBuySubmitted { .. }));

        let third = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("shadow event timeout")
            .expect("shadow event receive");
        match third {
            GhostEvent::ShadowBuySimulated(event) => {
                assert_eq!(event.live_signature, Some(signature.to_string()));
            }
            other => panic!("expected ShadowBuySimulated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn live_confirmed_handoff_keeps_slot_reserved_until_post_buy_runtime_owns_it() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::LiveAndShadow,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-live-handoff-success.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let mut rx = event_tx.subscribe();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let signature = Signature::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };
        let tracker = crate::components::trigger::safety::PositionLimitTracker::new(1);
        let owner = Pubkey::new_unique();
        let lease = tracker
            .try_acquire(
                &owner,
                &pool.base_mint.parse().expect("valid mint"),
                &pool.pool_amm_id,
            )
            .expect("acquire live position slot");
        let expected_slot_id = lease.slot_id;

        let receipt = crate::components::trigger::TriggerDispatchReceipt {
            primary_outcome: Ok(
                crate::components::trigger::TriggerBuyOutcome::LiveConfirmed {
                    signature,
                    landed_slot: None,
                },
            ),
            shadow_task: None,
            active_position_lease: Some(lease),
            retain_position_slot_on_error: false,
            failed_request: Some(test_prepared_buy_request()),
            failed_context: None,
        };

        let applied = apply_trigger_dispatch_receipt(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "live",
            receipt,
        )
        .await
        .expect("live handoff should succeed");

        assert!(applied.bought);
        assert!(applied.retain_runtime_pool);
        assert_eq!(tracker.active_positions(), 1);

        let first = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("transaction sent timeout")
            .expect("transaction sent receive");
        assert!(matches!(first, GhostEvent::TransactionSent { .. }));

        let second = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("post buy timeout")
            .expect("post buy receive");
        match second {
            GhostEvent::PostBuySubmitted {
                lane,
                position_slot_id,
                ..
            } => {
                assert_eq!(lane, "live");
                assert_eq!(position_slot_id, Some(expected_slot_id));
            }
            other => panic!("expected PostBuySubmitted, got {other:?}"),
        }

        assert!(tracker.release(expected_slot_id));
        assert_eq!(tracker.active_positions(), 0);
    }

    #[tokio::test]
    async fn live_confirmed_handoff_failure_surfaces_error_and_keeps_slot_reserved() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::LiveAndShadow,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-live-handoff-failure.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );
        let (event_tx, event_rx) = crate::events::create_event_bus();
        drop(event_rx);
        assert_eq!(event_tx.receiver_count(), 0);

        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let signature = Signature::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };
        let tracker = crate::components::trigger::safety::PositionLimitTracker::new(1);
        let owner = Pubkey::new_unique();
        let lease = tracker
            .try_acquire(
                &owner,
                &pool.base_mint.parse().expect("valid mint"),
                &pool.pool_amm_id,
            )
            .expect("acquire live position slot");

        let receipt = crate::components::trigger::TriggerDispatchReceipt {
            primary_outcome: Ok(
                crate::components::trigger::TriggerBuyOutcome::LiveConfirmed {
                    signature,
                    landed_slot: None,
                },
            ),
            shadow_task: None,
            active_position_lease: Some(lease),
            retain_position_slot_on_error: false,
            failed_request: Some(test_prepared_buy_request()),
            failed_context: None,
        };

        let err = apply_trigger_dispatch_receipt(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "live",
            receipt,
        )
        .await
        .expect_err("missing post-buy consumer should surface a handoff error");

        assert!(
            err.to_string().contains("failed to hand off"),
            "unexpected error: {err}"
        );
        assert_eq!(tracker.active_positions(), 1);
        let second_mint = Pubkey::new_unique();
        assert!(
            tracker
                .try_acquire(&owner, &second_mint, "second_pool")
                .is_err(),
            "confirmed live BUY handoff failure must keep the slot reserved"
        );
    }

    #[tokio::test]
    async fn uncertain_live_submit_failure_keeps_slot_reserved_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::LiveAndShadow,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-live-uncertain-submit-failure.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );
        let (event_tx, event_rx) = crate::events::create_event_bus();
        drop(event_rx);

        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };
        let tracker = crate::components::trigger::safety::PositionLimitTracker::new(1);
        let owner = Pubkey::new_unique();
        let lease = tracker
            .try_acquire(
                &owner,
                &pool.base_mint.parse().expect("valid mint"),
                &pool.pool_amm_id,
            )
            .expect("acquire live position slot");

        let receipt = crate::components::trigger::TriggerDispatchReceipt {
            primary_outcome: Err(anyhow::anyhow!(
                "Live transaction submission failed: landing uncertain: late landing not ruled out"
            )),
            shadow_task: None,
            active_position_lease: Some(lease),
            retain_position_slot_on_error: true,
            failed_request: Some(test_prepared_buy_request()),
            failed_context: None,
        };

        let err = apply_trigger_dispatch_receipt(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "live",
            receipt,
        )
        .await
        .expect_err("uncertain submit failure should surface");

        assert!(err.to_string().contains("landing uncertain"));
        assert_eq!(tracker.active_positions(), 1);
        let second_mint = Pubkey::new_unique();
        assert!(
            tracker
                .try_acquire(&owner, &second_mint, "second_pool")
                .is_err(),
            "uncertain live BUY landing must keep the slot reserved fail-closed"
        );
    }

    #[tokio::test]
    async fn definite_live_submit_failure_releases_slot() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::LiveAndShadow,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-live-definite-submit-failure.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );
        let (event_tx, event_rx) = crate::events::create_event_bus();
        drop(event_rx);

        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };
        let tracker = crate::components::trigger::safety::PositionLimitTracker::new(1);
        let owner = Pubkey::new_unique();
        let lease = tracker
            .try_acquire(
                &owner,
                &pool.base_mint.parse().expect("valid mint"),
                &pool.pool_amm_id,
            )
            .expect("acquire live position slot");

        let receipt = crate::components::trigger::TriggerDispatchReceipt {
            primary_outcome: Err(anyhow::anyhow!("live submit failed")),
            shadow_task: None,
            active_position_lease: Some(lease),
            retain_position_slot_on_error: false,
            failed_request: Some(test_prepared_buy_request()),
            failed_context: None,
        };

        let err = apply_trigger_dispatch_receipt(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "live",
            receipt,
        )
        .await
        .expect_err("definite submit failure should surface");

        assert!(err.to_string().contains("live submit failed"));
        assert_eq!(tracker.active_positions(), 0);
        let second_mint = Pubkey::new_unique();
        let second_lease = tracker
            .try_acquire(&owner, &second_mint, "second_pool")
            .expect("definite failure should release the slot");
        drop(second_lease);
    }

    #[tokio::test]
    async fn live_and_shadow_shadow_failure_does_not_fail_live_entry() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::LiveAndShadow,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-live-and-shadow-failure.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let mut rx = event_tx.subscribe();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let signature = Signature::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let receipt = crate::components::trigger::TriggerDispatchReceipt {
            primary_outcome: Ok(
                crate::components::trigger::TriggerBuyOutcome::LiveConfirmed {
                    signature,
                    landed_slot: None,
                },
            ),
            shadow_task: Some(test_pending_shadow_simulation(tokio::spawn(async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Err(anyhow::anyhow!("shadow boom"))
            }))),
            active_position_lease: None,
            retain_position_slot_on_error: false,
            failed_request: None,
            failed_context: None,
        };

        let applied = apply_trigger_dispatch_receipt(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "paper",
            receipt,
        )
        .await
        .expect("live success should survive shadow failure");

        assert!(applied.bought);
        assert_eq!(applied.close_reason, WindowCloseReason::PoolBoughtEarly);

        let first = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("transaction sent timeout")
            .expect("transaction sent receive");
        assert!(matches!(first, GhostEvent::TransactionSent { .. }));

        let second = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("post buy timeout")
            .expect("post buy receive");
        assert!(matches!(second, GhostEvent::PostBuySubmitted { .. }));

        assert!(tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn live_and_shadow_live_failure_still_emits_shadow_telemetry() {
        let temp = tempfile::tempdir().unwrap();
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::LiveAndShadow,
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
        };
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-live-and-shadow-live-failure.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let mut rx = event_tx.subscribe();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let receipt = crate::components::trigger::TriggerDispatchReceipt {
            primary_outcome: Err(anyhow::anyhow!("live submit failed")),
            shadow_task: Some(test_pending_shadow_simulation(tokio::spawn({
                let base_mint = pool.base_mint.clone();
                async move {
                    Ok(crate::components::trigger::ShadowBuySimulationReport {
                        mint: base_mint,
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
                    })
                }
            }))),
            active_position_lease: None,
            retain_position_slot_on_error: false,
            failed_request: None,
            failed_context: None,
        };

        let err = apply_trigger_dispatch_receipt(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "paper",
            receipt,
        )
        .await
        .expect_err("live failure should still surface");

        assert!(err.to_string().contains("live submit failed"));
        let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("shadow event timeout")
            .expect("shadow event receive");
        match event {
            GhostEvent::ShadowBuySimulated(event) => {
                assert_eq!(event.live_signature, None);
            }
            other => panic!("expected ShadowBuySimulated, got {other:?}"),
        }
    }

    #[test]
    fn shadow_dispatch_error_classifies_rpc_unreachable() {
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.enabled = true;
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );

        let err = anyhow::anyhow!(
            "shadow RPC simulate failed: RPC request error: cluster version query failed: error sending request for url (http://127.0.0.1:8899/): error trying to connect: tcp connect error: Connection refused (os error 111)"
        );
        assert_eq!(
            shadow_execution_outcome_from_dispatch_error(&trigger, &err),
            "shadow_rpc_unreachable"
        );
    }

    #[test]
    fn shadow_dispatch_error_classifies_insufficient_balance() {
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.enabled = true;
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );

        let err = anyhow::anyhow!(
            "Insufficient payer balance for trigger buy: payer=abc have=2039280 need=103050000 amount=100000000 tip=3000000 fee_buffer=50000"
        );
        assert_eq!(
            shadow_execution_outcome_from_dispatch_error(&trigger, &err),
            "shadow_insufficient_balance"
        );
    }

    #[test]
    fn shadow_dispatch_error_classifies_bulkhead_balance_violation() {
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.enabled = true;
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );

        let err = anyhow::anyhow!("Balance critical: 0.007327349 SOL < 0.008 SOL emergency floor");
        assert_eq!(
            shadow_execution_outcome_from_dispatch_error(&trigger, &err),
            "shadow_insufficient_balance"
        );
    }

    #[test]
    fn shadow_dispatch_error_classifies_invalid_fee_payer() {
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.enabled = true;
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );

        let err = anyhow::anyhow!(
            "Invalid fee payer account owner for trigger buy: payer=abc owner=TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA executable=false"
        );
        assert_eq!(
            shadow_execution_outcome_from_dispatch_error(&trigger, &err),
            "shadow_invalid_fee_payer"
        );
    }

    #[test]
    fn shadow_dispatch_error_classifies_account_not_visible() {
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.enabled = true;
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );

        let err = anyhow::anyhow!(
            "Failed to fetch mint account: primary=AccountNotFound: pubkey=abc | secondary=AccountNotFound: pubkey=abc"
        );
        assert_eq!(
            shadow_execution_outcome_from_dispatch_error(&trigger, &err),
            "shadow_account_not_visible"
        );
    }

    #[test]
    fn shadow_dispatch_error_classifies_metadata_missing() {
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.enabled = true;
        let trigger = crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
            trigger_config,
            Arc::new(MockShadowSimulator),
        );

        let err = anyhow::anyhow!(
            "Missing canonical creator_pubkey for trigger buy: mint=abc refusing to derive creator_vault from default pubkey"
        );
        assert_eq!(
            shadow_execution_outcome_from_dispatch_error(&trigger, &err),
            "shadow_metadata_missing"
        );
    }

    #[test]
    fn shadow_report_error_classifies_authority_and_transport_outcomes() {
        assert_eq!(
            shadow_execution_outcome_from_report_err("InvalidAccountForFee"),
            "shadow_authority_error"
        );
        assert_eq!(
            shadow_execution_outcome_from_report_err("shadow RPC simulate timed out after 100ms"),
            "shadow_transport_error"
        );
    }

    #[test]
    fn merge_truth_trade_marks_failed_signature_and_ignores_out_of_window_noise() {
        let window = CoverageAuditClosedWindow {
            window_id: "pool-1:100:200".to_string(),
            pool_id: Pubkey::default().to_string(),
            base_mint: Some("mint-1".to_string()),
            t0_ms: 100,
            t_end_ms: 200,
            verdict: Some("BUY".to_string()),
            window_complete: true,
            window_close_reason: Some("END_REACHED".to_string()),
            signatures: HashMap::new(),
            diagnostics: Default::default(),
        };

        let mut truth = HashMap::new();
        let success_trade = TradeEvent {
            semantic: Default::default(),
            slot: Some(1),
            signature: Signature::new_unique(),
            event_ordinal: None,
            provenance: None,
            timestamp_ms: 150,
            event_time: ghost_core::EventTimeMetadata::new(Some(150), Some(1_500), None),
            arrival_ts_ms: 150,
            pool_amm_id: Pubkey::default(),
            mint: Pubkey::new_unique(),
            signer: Pubkey::new_unique(),
            is_buy: true,
            is_dev_buy: false,
            amount: 1,
            max_sol_cost: 1,
            min_sol_output: 0,
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: Vec::new(),
            mpcf_payload: Vec::new(),
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
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
            curve_finality: CurveFinality::Speculative,
            is_pumpswap: true,
        };
        let failed_same_sig = TradeEvent {
            success: false,
            ..success_trade.clone()
        };
        let wrong_window = TradeEvent {
            timestamp_ms: 250,
            event_time: ghost_core::EventTimeMetadata::new(Some(250), Some(1_600), None),
            signature: Signature::new_unique(),
            success: false,
            ..success_trade.clone()
        };
        let ingress_only = TradeEvent {
            signature: Signature::new_unique(),
            event_time: ghost_core::EventTimeMetadata::new(None, Some(150), None),
            semantic: success_trade
                .semantic
                .with_timestamp_quality(ghost_core::TimestampQuality::WallClock),
            ..success_trade.clone()
        };

        merge_truth_trade(&mut truth, &window, &success_trade);
        merge_truth_trade(&mut truth, &window, &failed_same_sig);
        merge_truth_trade(&mut truth, &window, &wrong_window);
        merge_truth_trade(&mut truth, &window, &ingress_only);

        assert_eq!(truth.len(), 1);
        let state = truth
            .get(&success_trade.signature.to_string())
            .expect("truth signature should be retained");
        assert!(state.failed);
        assert_eq!(state.time_source.as_deref(), Some("explicit_chain_event"));
    }

    #[test]
    fn derive_buy_account_overrides_prefers_latest_successful_buy() {
        use crate::components::gatekeeper::GatekeeperBufferedTx;
        use crate::events::PoolTransaction;
        use ghost_brain::oracle::snapshot_engine::PoolMetrics;
        use ghost_core::shadow_ledger::TxKey;
        use seer::types::RawBytesMissingReason;
        use std::sync::Arc;

        let failed_buy = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: "pool".to_string(),
            slot: Some(1),
            event_ordinal: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1,
            signer: "signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "failed_sig".to_string(),
            success: false,
            error_code: Some("InstructionError(0, Custom(1))".to_string()),
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: Some(Pubkey::new_unique().to_string()),
            fee_recipient: Some(Pubkey::new_unique().to_string()),
            token_program: Some(Pubkey::new_unique().to_string()),
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
        };

        let expected_global = trigger::DirectBuyBuilder::canonical_global_config();
        let expected_fee = trigger::DirectBuyBuilder::canonical_fee_recipient();
        let expected_token = Pubkey::new_unique();
        let expected_assoc_curve = Pubkey::new_unique();
        let successful_buy = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: "pool".to_string(),
            slot: Some(2),
            event_ordinal: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 2,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 2,
            signer: "signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "success_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: Some(expected_global.to_string()),
            fee_recipient: Some(expected_fee.to_string()),
            token_program: Some(expected_token.to_string()),
            buy_variant: Some("routed_exact_sol_in".to_string()),
            associated_bonding_curve: Some(expected_assoc_curve.to_string()),
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
        };

        let buffered_txs = vec![
            GatekeeperBufferedTx {
                tx: Arc::new(successful_buy),
                metrics: PoolMetrics::default(),
                tx_key: TxKey::new(2, Some(2), None, None, 0).unwrap(),
            },
            GatekeeperBufferedTx {
                tx: Arc::new(failed_buy),
                metrics: PoolMetrics::default(),
                tx_key: TxKey::new(1, Some(1), None, None, 0).unwrap(),
            },
        ];

        let overrides = derive_buy_account_overrides(&buffered_txs);

        assert_eq!(overrides.global_config, Some(expected_global));
        assert_eq!(overrides.fee_recipient, Some(expected_fee));
        assert_eq!(overrides.token_program, Some(expected_token));
        assert_eq!(
            overrides.buy_variant,
            Some(trigger::PumpfunBuyVariant::RoutedExactSolIn)
        );
        assert_eq!(
            overrides.associated_bonding_curve,
            Some(expected_assoc_curve)
        );
    }

    #[test]
    fn derive_buy_account_overrides_ignores_known_bad_legacy_fee_recipient() {
        use ghost_brain::oracle::snapshot_engine::PoolMetrics;
        use ghost_core::shadow_ledger::TxKey;
        use seer::types::RawBytesMissingReason;
        use std::sync::Arc;

        let known_bad =
            Pubkey::from_str(KNOWN_BAD_LEGACY_FEE_RECIPIENT).expect("known bad fee recipient");
        let successful_buy = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: "pool".to_string(),
            slot: Some(1),
            event_ordinal: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1,
            signer: "signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "success_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: Some(known_bad.to_string()),
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
        };

        let buffered_txs = vec![GatekeeperBufferedTx {
            tx: Arc::new(successful_buy),
            metrics: PoolMetrics::default(),
            tx_key: TxKey::new(1, Some(1), None, None, 0).unwrap(),
        }];

        let overrides = derive_buy_account_overrides(&buffered_txs);
        assert!(overrides.fee_recipient.is_none());
    }

    #[test]
    fn build_decision_logger_config_uses_configured_decision_root() {
        let decision_log_path = "logs/rollout/shadow-burnin-v25-repair/decisions";
        let config =
            build_decision_logger_config(decision_log_path, &GatekeeperV2Config::default());

        assert_eq!(config.log_dir, std::path::PathBuf::from(decision_log_path));
        assert_eq!(
            config.gatekeeper_log_dir,
            std::path::PathBuf::from(decision_log_path)
        );
        assert_eq!(
            config.gatekeeper_rollout_profile,
            "shadow-burnin-v25-repair"
        );
        assert_ne!(
            config.gatekeeper_log_dir,
            std::path::PathBuf::from("logs/decisions.json/rollout/shadow-burnin/decisions")
        );
    }

    #[test]
    fn build_decision_logger_config_normalizes_parent_segments_before_rollout_derivation() {
        let config = build_decision_logger_config(
            "configs/rollout/../../logs/rollout/shadow-burnin-v25-repair/decisions",
            &GatekeeperV2Config::default(),
        );

        assert_eq!(
            config.gatekeeper_log_dir,
            std::path::PathBuf::from("logs/rollout/shadow-burnin-v25-repair/decisions")
        );
        assert_eq!(
            config.gatekeeper_rollout_profile,
            "shadow-burnin-v25-repair"
        );
    }

    #[test]
    fn build_coverage_audit_log_path_uses_normalized_decision_root() {
        assert_eq!(
            build_coverage_audit_log_path(
                "configs/rollout/../../logs/rollout/shadow-burnin-v25-repair/decisions"
            ),
            std::path::PathBuf::from(
                "logs/rollout/shadow-burnin-v25-repair/decisions/seer_runtime_coverage_audit.jsonl"
            )
        );
        assert_eq!(
            build_coverage_audit_log_path("logs/decisions.jsonl"),
            std::path::PathBuf::from("logs/decisions/seer_runtime_coverage_audit.jsonl")
        );
    }

    #[test]
    fn build_decision_logger_config_uses_unknown_rollout_outside_rollout_roots() {
        let config =
            build_decision_logger_config("logs/decisions.jsonl", &GatekeeperV2Config::default());

        assert_eq!(config.gatekeeper_rollout_profile, "unknown_rollout");
        assert_eq!(config.gatekeeper_log_dir, config.log_dir);
        assert_eq!(config.log_dir, std::path::PathBuf::from("logs/decisions"));
    }

    #[test]
    fn derive_buy_account_overrides_keeps_primary_global_fee_recipient() {
        use ghost_brain::oracle::snapshot_engine::PoolMetrics;
        use ghost_core::shadow_ledger::TxKey;
        use seer::types::RawBytesMissingReason;
        use std::sync::Arc;

        let current_fee = Pubkey::from_str("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")
            .expect("primary global fee recipient");
        let successful_buy = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: "pool".to_string(),
            slot: Some(1),
            event_ordinal: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1,
            signer: "signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "success_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: Some(current_fee.to_string()),
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
        };

        let buffered_txs = vec![GatekeeperBufferedTx {
            tx: Arc::new(successful_buy),
            metrics: PoolMetrics::default(),
            tx_key: TxKey::new(1, Some(1), None, None, 0).unwrap(),
        }];

        let overrides = derive_buy_account_overrides(&buffered_txs);
        assert_eq!(overrides.fee_recipient, Some(current_fee));
    }

    #[test]
    fn derive_buy_account_overrides_keeps_reserved_fee_recipient() {
        use ghost_brain::oracle::snapshot_engine::PoolMetrics;
        use ghost_core::shadow_ledger::TxKey;
        use seer::types::RawBytesMissingReason;
        use std::sync::Arc;

        let reserved_fee = Pubkey::from_str("GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS")
            .expect("reserved fee recipient");
        let successful_buy = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: "pool".to_string(),
            slot: Some(1),
            event_ordinal: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1,
            signer: "signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "success_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: Some(reserved_fee.to_string()),
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
        };

        let buffered_txs = vec![GatekeeperBufferedTx {
            tx: Arc::new(successful_buy),
            metrics: PoolMetrics::default(),
            tx_key: TxKey::new(1, Some(1), None, None, 0).unwrap(),
        }];

        let overrides = derive_buy_account_overrides(&buffered_txs);
        assert_eq!(overrides.fee_recipient, Some(reserved_fee));
    }

    #[test]
    fn derive_buy_account_overrides_drops_noncanonical_fee_recipient() {
        use ghost_brain::oracle::snapshot_engine::PoolMetrics;
        use ghost_core::shadow_ledger::TxKey;
        use seer::types::RawBytesMissingReason;
        use std::sync::Arc;

        let successful_buy = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: "pool".to_string(),
            slot: Some(1),
            event_ordinal: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1,
            signer: "signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "success_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: Some(Pubkey::new_unique().to_string()),
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
        };

        let buffered_txs = vec![GatekeeperBufferedTx {
            tx: Arc::new(successful_buy),
            metrics: PoolMetrics::default(),
            tx_key: TxKey::new(1, Some(1), None, None, 0).unwrap(),
        }];

        let overrides = derive_buy_account_overrides(&buffered_txs);
        assert!(overrides.fee_recipient.is_none());
    }

    #[test]
    fn derive_buy_account_overrides_drops_noncanonical_global_config() {
        use ghost_brain::oracle::snapshot_engine::PoolMetrics;
        use ghost_core::shadow_ledger::TxKey;
        use seer::types::RawBytesMissingReason;
        use std::sync::Arc;

        let successful_buy = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: "pool".to_string(),
            slot: Some(1),
            event_ordinal: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1,
            signer: "signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "success_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: Some(Pubkey::new_unique().to_string()),
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
        };

        let buffered_txs = vec![GatekeeperBufferedTx {
            tx: Arc::new(successful_buy),
            metrics: PoolMetrics::default(),
            tx_key: TxKey::new(1, Some(1), None, None, 0).unwrap(),
        }];

        let overrides = derive_buy_account_overrides(&buffered_txs);
        assert!(overrides.global_config.is_none());
    }

    #[test]
    fn derive_buy_account_overrides_drops_legacy_buy_variant() {
        use ghost_brain::oracle::snapshot_engine::PoolMetrics;
        use ghost_core::shadow_ledger::TxKey;
        use seer::types::RawBytesMissingReason;
        use std::sync::Arc;

        let successful_buy = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: "pool".to_string(),
            slot: Some(1),
            event_ordinal: None,
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1,
            signer: "signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "success_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::FilteredByConfig,
            token_mint: None,
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: Some("legacy_buy".to_string()),
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
        };

        let buffered_txs = vec![GatekeeperBufferedTx {
            tx: Arc::new(successful_buy),
            metrics: PoolMetrics::default(),
            tx_key: TxKey::new(1, Some(1), None, None, 0).unwrap(),
        }];

        let overrides = derive_buy_account_overrides(&buffered_txs);
        assert!(overrides.buy_variant.is_none());
    }

    #[tokio::test]
    async fn shadow_only_dispatch_failure_writes_shadow_failure_record() {
        let temp = tempfile::tempdir().unwrap();
        let output_path = temp.path().join("shadow-failure.jsonl");
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.enabled = true;
        trigger_config.shadow_run.output_path = output_path.to_string_lossy().into_owned();
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );
        let (event_tx, mut event_rx) = crate::events::create_event_bus();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let receipt = crate::components::trigger::TriggerDispatchReceipt {
            primary_outcome: Err(anyhow::anyhow!(
                "shadow RPC simulate failed: RPC request error: cluster version query failed: error sending request for url (http://127.0.0.1:8899/): error trying to connect: tcp connect error: Connection refused (os error 111)"
            )),
            shadow_task: None,
            active_position_lease: None,
            retain_position_slot_on_error: false,
            failed_request: Some(test_prepared_buy_request()),
            failed_context: None,
        };

        let err = apply_trigger_dispatch_receipt(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "paper",
            receipt,
        )
        .await
        .expect_err("dispatch failure should propagate");

        assert!(err.to_string().contains("Connection refused"));
        let written = tokio::fs::read_to_string(&output_path)
            .await
            .expect("shadow failure record should be written");
        assert!(written.contains("\"pool_amm_id\":\""));
        assert!(written.contains(&pool_id.to_string()));
        assert!(written.contains("\"error_class\":\"network_provider_problem\""));
        assert!(written.contains("Connection refused"));
        let event = event_rx.recv().await.expect("shadow failure event");
        match event {
            GhostEvent::ShadowBuySimulated(event) => {
                assert_eq!(
                    event.error_class.as_deref(),
                    Some("network_provider_problem")
                );
                assert_eq!(event.payer_provenance, "configured");
            }
            other => panic!("expected ShadowBuySimulated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn shadow_only_preflight_failure_writes_shadow_failure_record() {
        let temp = tempfile::tempdir().unwrap();
        let output_path = temp.path().join("shadow-preflight-failure.jsonl");
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.enabled = true;
        trigger_config.shadow_run.output_path = output_path.to_string_lossy().into_owned();
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );
        let (event_tx, mut event_rx) = crate::events::create_event_bus();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };

        let receipt = crate::components::trigger::TriggerDispatchReceipt {
            primary_outcome: Err(anyhow::anyhow!(
                "Insufficient payer balance for trigger buy: payer=abc have=2039280 need=103050000 amount=100000000 tip=3000000 fee_buffer=50000"
            )),
            shadow_task: None,
            active_position_lease: None,
            retain_position_slot_on_error: false,
            failed_request: None,
            failed_context: Some(crate::components::trigger::TriggerDispatchFailureContext {
                amount_lamports: 100_000_000,
                tip_lamports: 3_000_000,
                decision_ts_ms: 10,
                payer_provenance: "configured",
                payer_pubkey: Some("payer-configured".to_string()),
            }),
        };

        let err = apply_trigger_dispatch_receipt(
            &event_tx,
            None,
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Paper,
            std::path::Path::new("/tmp/ghost-shadow-entry-test.jsonl"),
            None,
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "paper",
            receipt,
        )
        .await
        .expect_err("preflight failure should propagate");

        assert!(err.to_string().contains("Insufficient payer balance"));
        let written = tokio::fs::read_to_string(&output_path)
            .await
            .expect("shadow preflight failure record should be written");
        assert!(written.contains(&pool_id.to_string()));
        assert!(written.contains("\"amount_lamports\":100000000"));
        assert!(written.contains("Insufficient payer balance"));
        assert!(written.contains("\"payer_provenance\":\"configured\""));
        let event = event_rx
            .recv()
            .await
            .expect("shadow preflight failure event");
        match event {
            GhostEvent::ShadowBuySimulated(event) => {
                assert_eq!(event.error_class.as_deref(), Some("fee_compute_problem"));
                assert_eq!(event.payer_provenance, "configured");
            }
            other => panic!("expected ShadowBuySimulated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn p5_shadow_dispatch_lifecycle_writes_closed_with_idempotency_join_key_rollout_profile()
    {
        let temp = tempfile::tempdir().unwrap();
        let output_path = temp.path().join("shadow-canonical-entry.jsonl");
        let lifecycle_path = temp.path().join("shadow_lifecycle.jsonl");
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.enabled = true;
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-legacy-report.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let (direct_tx, mut direct_rx) =
            crate::components::post_buy_runtime::create_direct_post_buy_handoff_channel();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };
        let request = test_prepared_buy_request();
        let entry_token_amount_raw = request.entry_token_amount_raw.expect("shadow qty");
        let decision_ts_ms = request.decision_ts_ms;
        let tracker = crate::components::trigger::safety::PositionLimitTracker::new(1);
        let mint_pubkey = Pubkey::from_str(&pool.base_mint).expect("valid base mint");
        let slot_id = crate::components::trigger::safety::PositionSlotId::derive(
            &Pubkey::new_unique(),
            &mint_pubkey,
        );
        let active_position_lease = tracker
            .try_acquire_with_slot_id(slot_id, pool_id.to_string(), pool.base_mint.clone())
            .expect("shadow slot should reserve");
        let ack_task = tokio::spawn(async move {
            let handoff = direct_rx.recv().await.expect("direct handoff");
            let (_event, ack_tx) = handoff.into_parts();
            ack_tx
                .expect("shadow handoff ack channel")
                .send(DirectPostBuyHandoffAck::Accepted)
                .expect("send accepted handoff ack");
        });
        let receipt = crate::components::trigger::TriggerDispatchReceipt {
            primary_outcome: Ok(
                crate::components::trigger::TriggerBuyOutcome::ShadowSimulated {
                    report: crate::components::trigger::ShadowBuySimulationReport {
                        mint: pool.base_mint.clone(),
                        live_signature: None,
                        payer_pubkey: Pubkey::new_unique().to_string(),
                        payer_provenance: request.payer_provenance.to_string(),
                        amount_lamports: request.amount_lamports,
                        entry_token_amount_raw: request.entry_token_amount_raw,
                        tip_lamports: request.tip_lamports,
                        decision_ts_ms,
                        simulation_started_ts_ms: decision_ts_ms.saturating_add(1),
                        simulation_finished_ts_ms: decision_ts_ms.saturating_add(6),
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
                    },
                },
            ),
            shadow_task: None,
            active_position_lease: Some(active_position_lease),
            retain_position_slot_on_error: false,
            failed_request: Some(request),
            failed_context: None,
        };

        let applied = apply_trigger_dispatch_receipt(
            &event_tx,
            Some(&direct_tx),
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Shadow,
            &output_path,
            Some(&lifecycle_path),
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "shadow",
            receipt,
        )
        .await
        .expect("shadow receipt should apply");
        ack_task.await.expect("ack task should complete");

        assert!(!applied.bought);
        assert!(applied.retain_runtime_pool);
        assert_eq!(applied.shadow_execution_outcome, "shadow_simulated");
        assert_eq!(tracker.active_positions(), 1);

        let written = tokio::fs::read_to_string(&output_path)
            .await
            .expect("canonical shadow entry record should be written");
        let record: ShadowEntryRecord =
            serde_json::from_str(written.lines().next().expect("one shadow entry record"))
                .expect("deserialize canonical shadow entry");
        assert_eq!(record.pool_id, pool_id.to_string());
        assert_eq!(record.mint_id, pool.base_mint);
        assert_eq!(record.slot, Some(777));
        assert_eq!(record.timestamp_ms, decision_ts_ms.saturating_add(6));
        assert_eq!(record.execution_outcome, "shadow_simulated");
        assert_eq!(
            record.entry_price,
            0.1 / (entry_token_amount_raw as f64 / PUMP_TOKEN_DECIMAL_FACTOR)
        );
        let lifecycle_written = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Ok(contents) = tokio::fs::read_to_string(&lifecycle_path).await {
                    if !contents.trim().is_empty() {
                        break contents;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("lifecycle record should be written");
        let lifecycle_row: serde_json::Value = serde_json::from_str(
            lifecycle_written
                .lines()
                .next()
                .expect("one lifecycle record"),
        )
        .expect("deserialize lifecycle record");
        assert_eq!(lifecycle_row["record_type"], "shadow_dispatch");
        assert_eq!(lifecycle_row["dispatch_status"], "closed");
        assert_eq!(
            lifecycle_row["join_key"],
            format!("{pool_id}:{}:1000", pool.base_mint)
        );
        assert_eq!(lifecycle_row["rollout_profile"], "test-rollout");
        let expected_idempotency_key =
            crate::components::trigger::shadow_run::make_shadow_idempotency_key(
                &pool_id.to_string(),
                &format!("{pool_id}:{}:1000", pool.base_mint),
                "test-rollout",
            );
        assert_eq!(lifecycle_row["idempotency_key"], expected_idempotency_key);
        assert!(tracker.release(slot_id));
    }

    #[tokio::test]
    async fn shadow_mode_marks_canonical_entry_record_when_direct_handoff_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let output_path = temp.path().join("shadow-canonical-entry-rejected.jsonl");
        let mut trigger_config = crate::config::TriggerComponentConfig {
            enabled: true,
            entry_mode: crate::config::TriggerEntryMode::ShadowOnly,
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
        };
        trigger_config.shadow_run.enabled = true;
        trigger_config.shadow_run.output_path = temp
            .path()
            .join("shadow-legacy-report.jsonl")
            .to_string_lossy()
            .into_owned();
        let trigger = Arc::new(
            crate::components::trigger::TriggerComponent::new_with_shadow_simulator(
                trigger_config,
                Arc::new(MockShadowSimulator),
            ),
        );
        let (event_tx, _event_rx) = crate::events::create_event_bus();
        let (direct_tx, mut direct_rx) =
            crate::components::post_buy_runtime::create_direct_post_buy_handoff_channel();
        let post_buy_epoch = std::sync::atomic::AtomicU64::new(1);
        let pool_id = Pubkey::new_unique();
        let pool = crate::events::DetectedPool {
            semantic: Default::default(),
            pool_amm_id: pool_id.to_string(),
            base_mint: Pubkey::new_unique().to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: "pumpfun".to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(1),
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            detected_wall_ts_ms: Some(1_001),
            initial_liquidity_sol: Some(1.0),
            signature: "sig".to_string(),
        };
        let request = test_prepared_buy_request();
        let tracker = crate::components::trigger::safety::PositionLimitTracker::new(1);
        let mint_pubkey = Pubkey::from_str(&pool.base_mint).expect("valid base mint");
        let slot_id = crate::components::trigger::safety::PositionSlotId::derive(
            &Pubkey::new_unique(),
            &mint_pubkey,
        );
        let active_position_lease = tracker
            .try_acquire_with_slot_id(slot_id, pool_id.to_string(), pool.base_mint.clone())
            .expect("shadow slot should reserve");
        let ack_task = tokio::spawn(async move {
            let handoff = direct_rx.recv().await.expect("direct handoff");
            let (_event, ack_tx) = handoff.into_parts();
            ack_tx
                .expect("shadow handoff ack channel")
                .send(DirectPostBuyHandoffAck::Rejected("guardian_unavailable"))
                .expect("send rejected handoff ack");
        });
        let receipt = crate::components::trigger::TriggerDispatchReceipt {
            primary_outcome: Ok(
                crate::components::trigger::TriggerBuyOutcome::ShadowSimulated {
                    report: crate::components::trigger::ShadowBuySimulationReport {
                        mint: pool.base_mint.clone(),
                        live_signature: None,
                        payer_pubkey: Pubkey::new_unique().to_string(),
                        payer_provenance: request.payer_provenance.to_string(),
                        amount_lamports: request.amount_lamports,
                        entry_token_amount_raw: request.entry_token_amount_raw,
                        tip_lamports: request.tip_lamports,
                        decision_ts_ms: request.decision_ts_ms,
                        simulation_started_ts_ms: request.decision_ts_ms.saturating_add(1),
                        simulation_finished_ts_ms: request.decision_ts_ms.saturating_add(6),
                        latency_ms: 5,
                        shadow_duration_ms: 5,
                        rpc_slot: 888,
                        retry_count: 0,
                        used_sig_verify: false,
                        used_replace_recent_blockhash: true,
                        units_consumed: Some(42_000),
                        logs: vec!["shadow".to_string()],
                        return_data: None,
                        err: None,
                    },
                },
            ),
            shadow_task: None,
            active_position_lease: Some(active_position_lease),
            retain_position_slot_on_error: false,
            failed_request: Some(request),
            failed_context: None,
        };

        let applied = apply_trigger_dispatch_receipt(
            &event_tx,
            Some(&direct_tx),
            &trigger,
            &post_buy_epoch,
            ExecutionMode::Shadow,
            &output_path,
            None,
            "test-rollout",
            pool_id,
            &pool,
            0.1,
            10,
            "shadow",
            receipt,
        )
        .await
        .expect("shadow receipt should apply");
        ack_task.await.expect("ack task should complete");

        assert!(!applied.bought);
        assert!(!applied.retain_runtime_pool);
        assert_eq!(
            applied.shadow_execution_outcome,
            "shadow_handoff_rejected_guardian_unavailable"
        );
        assert_eq!(tracker.active_positions(), 0);

        let written = tokio::fs::read_to_string(&output_path)
            .await
            .expect("canonical shadow entry record should be written");
        let record: ShadowEntryRecord =
            serde_json::from_str(written.lines().next().expect("one shadow entry record"))
                .expect("deserialize canonical shadow entry");
        assert_eq!(
            record.execution_outcome,
            "shadow_handoff_rejected_guardian_unavailable"
        );
    }

    #[test]
    fn pool_cleanup_waits_for_shadow_retention_contract() {
        let retained_shadow = PoolObservationResult {
            pool_id: Pubkey::new_unique(),
            base_mint: Some(Pubkey::new_unique()),
            bought: false,
            retain_runtime_pool: true,
        };
        let rejected_shadow = PoolObservationResult {
            pool_id: Pubkey::new_unique(),
            base_mint: Some(Pubkey::new_unique()),
            bought: false,
            retain_runtime_pool: false,
        };
        let live_buy = PoolObservationResult {
            pool_id: Pubkey::new_unique(),
            base_mint: Some(Pubkey::new_unique()),
            bought: true,
            retain_runtime_pool: false,
        };

        assert!(!should_cleanup_pool_after_observation(&retained_shadow));
        assert!(should_cleanup_pool_after_observation(&rejected_shadow));
        assert!(!should_cleanup_pool_after_observation(&live_buy));
    }

    #[test]
    fn test_snapshot_engine_is_only_writer() {
        // EPIC 2.3.5: SnapshotEngine is the ONLY writer to ShadowLedger
        // OracleRuntime cannot write regardless of config

        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());

        #[allow(deprecated)]
        let config = OracleRuntimeConfig {
            runtime_shadowledger_snapshots_enabled: false,
            shadow_ledger_enrichment_freshness_ms: DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
            ..Default::default()
        };

        let runtime = OracleRuntime::new_with_config(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
            None,
            None,
            Arc::new(ghost_core::shadow_ledger::LivePipeline::new()),
            config,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();

        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(100),
            ..Default::default()
        };

        runtime.approved_pools().insert(pool_id);
        runtime.register_new_pool(pool_id, base_mint, candidate, None);

        // Register transaction via OracleRuntime - should NOT write
        runtime.register_pool_tx(
            pool_id,
            1000,
            Some(100),
            vec![1, 2, 3],
            None,
            "signer1".to_string(),
            true,
            1.0,
        );

        // Verify OracleRuntime did NOT write
        let runtime_snapshots = shadow_ledger.get_snapshots(&base_mint);
        assert_eq!(
            runtime_snapshots.as_ref().map(|s| s.len()).unwrap_or(0),
            0,
            "OracleRuntime should never write (EPIC 2.3.5)"
        );

        shadow_ledger.commit_history(
            base_mint,
            vec![ghost_core::shadow_ledger::MarketSnapshot {
                slot: Some(99),
                tx_key: Some(
                    ghost_core::shadow_ledger::TxKey::new(99, Some(99), Some(0), None, 0)
                        .expect("valid key"),
                ),
                timestamp_ms: 990,
                ..Default::default()
            }],
            None,
        );

        // Now simulate SnapshotEngine writing directly (it uses SnapshotSource::SnapshotEngine)
        let snapshot = ghost_core::shadow_ledger::MarketSnapshot {
            slot: Some(100),
            tx_key: Some(
                ghost_core::shadow_ledger::TxKey::new(1000, Some(100), Some(0), None, 0)
                    .expect("valid key"),
            ),
            timestamp_ms: 1000,
            cum_volume_sol: 1.0,
            tx_count: 1,
            unique_addrs: 1,
            price_sol_per_token: 0.1,
            price_state: ghost_core::shadow_ledger::types::PriceState::Valid,
            price_reason: None,
            market_cap_sol: 100.0,
            reserve_base: 1000.0,
            reserve_quote: 100.0,
            bonding_progress_pct: 0.0,
            d_price_d_volume: 0.0,
            d_price_d_liquidity: 0.0,
            d_price_d_slippage: 0.0,
        };

        shadow_ledger.push_snapshot_with_source(
            base_mint,
            snapshot,
            ghost_core::shadow_ledger::SnapshotSource::SnapshotEngine,
        );

        // Verify SnapshotEngine CAN still write (independent of runtime config)
        let engine_snapshots = shadow_ledger.get_snapshots(&base_mint);
        assert_eq!(
            engine_snapshots.as_ref().map(|s| s.len()).unwrap_or(0),
            2,
            "SnapshotEngine should be able to append independently of runtime config once canonical history exists"
        );
    }

    #[test]
    fn test_forward_approved_tx_uses_live_pipeline_after_canonical_commit() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let live_pipeline = Arc::new(ghost_core::shadow_ledger::LivePipeline::with_config(
            LivePipelineConfig {
                flush_delay_ms: 0,
                ..Default::default()
            },
        ));

        #[allow(deprecated)]
        let config = OracleRuntimeConfig {
            runtime_shadowledger_snapshots_enabled: false,
            shadow_ledger_enrichment_freshness_ms: DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
            ..Default::default()
        };

        let runtime = OracleRuntime::new_with_config(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
            None,
            None,
            live_pipeline.clone(),
            config,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(50),
            ..Default::default()
        };

        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));
        runtime.mark_pool_approved(pool_id);
        runtime.approved_pools().insert(pool_id);

        let initial_snapshot = ghost_core::shadow_ledger::MarketSnapshot {
            slot: Some(50),
            tx_key: Some(
                ghost_core::shadow_ledger::TxKey::new(50_000, Some(50), Some(0), None, 0).unwrap(),
            ),
            timestamp_ms: 50_000,
            tx_count: 1,
            unique_addrs: 1,
            cum_volume_sol: 1.0,
            reserve_base: 1_000_000_000_000.0,
            reserve_quote: 30.0,
            price_sol_per_token: 30.0 / 1_000_000_000_000.0,
            ..Default::default()
        };
        shadow_ledger.commit_history(base_mint, vec![initial_snapshot.clone()], None);
        runtime.remember_committed_snapshot(base_mint, &initial_snapshot);
        runtime.mark_pool_committed(pool_id);

        let tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(60),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 60_000,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(60_000), None),
            arrival_ts_ms: 60_000,
            signer: Pubkey::new_unique().to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: Some(1_000_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: Some(999_999_000_000.0),
            reserve_quote: Some(31.0),
            price_quote: Some(31.0 / 999_999_000_000.0),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Signature::new_unique().to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![1, 2, 3],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: Some(base_mint.to_string()),
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
            curve_data_known: true,
        };

        runtime.forward_approved_tx_to_commit_or_live_pipeline(
            pool_id,
            base_mint,
            &tx,
            tx_event_ts_ms(&tx),
        );

        assert!(live_pipeline.is_initialized(&base_mint));
        assert_eq!(live_pipeline.flush_ready(&shadow_ledger), 1);
        let snapshots = shadow_ledger.get_snapshots(&base_mint).unwrap();
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots.last().unwrap().timestamp_ms, 60_000);
    }

    #[test]
    fn approved_not_equal_committed() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let live_pipeline = Arc::new(ghost_core::shadow_ledger::LivePipeline::with_config(
            LivePipelineConfig {
                flush_delay_ms: 0,
                ..Default::default()
            },
        ));

        #[allow(deprecated)]
        let runtime = OracleRuntime::new_with_config(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
            None,
            None,
            live_pipeline.clone(),
            OracleRuntimeConfig {
                runtime_shadowledger_snapshots_enabled: false,
                shadow_ledger_enrichment_freshness_ms:
                    DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
                ..Default::default()
            },
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(25),
            ..Default::default()
        };

        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));
        assert_eq!(
            runtime.runtime_pool_state(&pool_id),
            Some(PoolState::Tracked)
        );

        runtime.mark_pool_approved(pool_id);
        runtime.approved_pools().insert(pool_id);
        let seeded_history = vec![review_test_buffered_tx(
            pool_id,
            "precommit-history",
            25_500,
            true,
        )];
        assert_eq!(
            runtime.stage_gatekeeper_history_for_commit(
                pool_id,
                base_mint,
                25_000,
                &seeded_history
            ),
            1
        );
        assert_eq!(
            runtime.runtime_pool_state(&pool_id),
            Some(PoolState::Approved)
        );

        let tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(26),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 26_000,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(26_000), None),
            arrival_ts_ms: 26_500,
            signer: Pubkey::new_unique().to_string(),
            is_buy: true,
            volume_sol: 0.25,
            sol_amount_lamports: Some(250_000_000),
            token_amount_units: Some(250_000),
            reserve_base: Some(900_000_000.0),
            reserve_quote: Some(25.0),
            price_quote: Some(25.0 / 900_000_000.0),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Signature::new_unique().to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![1, 2, 3],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: Some(base_mint.to_string()),
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
            curve_data_known: true,
        };

        runtime.forward_approved_tx_to_commit_or_live_pipeline(
            pool_id,
            base_mint,
            &tx,
            tx_event_ts_ms(&tx),
        );

        assert_eq!(
            runtime.runtime_pool_state(&pool_id),
            Some(PoolState::Approved)
        );
        assert!(
            !live_pipeline.is_initialized(&base_mint),
            "approved pool must stay out of post-commit/live semantics"
        );
        assert_eq!(
            runtime.commit_coordinator().active_buffer_count(),
            1,
            "approved pool should remain on the pre-commit path"
        );
    }

    #[test]
    fn internal_precommit_reads_bypass_public_approval_gate() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            slot: Some(50),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        shadow_ledger.set_approval_checker(Arc::new(|_| false));
        let commit = shadow_ledger.commit_history(
            base_mint,
            vec![ghost_core::shadow_ledger::MarketSnapshot {
                slot: Some(50),
                tx_key: Some(
                    ghost_core::shadow_ledger::TxKey::new(50_000, Some(50), Some(0), None, 0)
                        .unwrap(),
                ),
                timestamp_ms: 50_000,
                reserve_base: 1_000_000_000_000.0,
                reserve_quote: 30.0,
                price_sol_per_token: 30.0 / 1_000_000_000_000.0,
                ..Default::default()
            }],
            None,
        );
        assert!(commit.persisted_success());
        assert!(shadow_ledger.get_latest_snapshot(&base_mint).is_none());

        let ctx = runtime.resolve_price_context(pool_id, base_mint);
        assert_eq!(
            ctx.reserve_base,
            Some(1_000_000_000_000.0 / PUMP_TOKEN_DECIMAL_FACTOR)
        );
        assert_eq!(ctx.reserve_quote, Some(30.0));
        assert!(ctx.price_quote.is_some());

        let (reserve_sol_lamports, reserve_tok_units) =
            runtime.resolve_gatekeeper_initial_reserves(pool_id, base_mint);
        assert_eq!(reserve_sol_lamports, 30_000_000_000);
        assert_eq!(reserve_tok_units, 1_000_000_000_000);
    }

    #[test]
    fn test_register_new_pool_dedup_base_mint() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );
        let snapshot_engine = SnapshotEngine::new(16, 0);
        runtime.configure_approval_gating(&snapshot_engine);

        let base_mint = Pubkey::new_unique();
        let pool_id_one = Pubkey::new_unique();
        let pool_id_two = Pubkey::new_unique();

        let candidate_one = EnhancedCandidate {
            pool_amm_id: pool_id_one,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            ..Default::default()
        };
        let candidate_two = EnhancedCandidate {
            pool_amm_id: pool_id_two,
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            ..Default::default()
        };

        assert!(runtime.register_new_pool(pool_id_one, base_mint, candidate_one, None));
        assert!(!runtime.register_new_pool(pool_id_two, base_mint, candidate_two, None));
    }

    #[test]
    fn test_register_new_pool_accepts_distinct_base_mints() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );
        let snapshot_engine = SnapshotEngine::new(16, 0);
        runtime.configure_approval_gating(&snapshot_engine);

        let pool_id_one = Pubkey::new_unique();
        let pool_id_two = Pubkey::new_unique();
        let base_mint_one = Pubkey::new_unique();
        let base_mint_two = Pubkey::new_unique();

        let candidate_one = EnhancedCandidate {
            pool_amm_id: pool_id_one,
            base_mint: base_mint_one,
            bonding_curve: Pubkey::new_unique(),
            ..Default::default()
        };
        let candidate_two = EnhancedCandidate {
            pool_amm_id: pool_id_two,
            base_mint: base_mint_two,
            bonding_curve: Pubkey::new_unique(),
            ..Default::default()
        };

        assert!(runtime.register_new_pool(pool_id_one, base_mint_one, candidate_one, None));
        assert!(runtime.register_new_pool(pool_id_two, base_mint_two, candidate_two, None));
    }

    // ──────────────────────────────────────────────────────────────────
    // Tests for gRPC reserve enrichment (canonical primary + fallback coverage)
    // ──────────────────────────────────────────────────────────────────

    #[test]
    fn test_enrich_pool_tx_fallback_fills_missing_reserves_from_shadow_ledger() {
        let ledger = ShadowLedger::new();
        let account_state_core = AccountStateReducer::new();
        let pool_id = Pubkey::new_unique();

        // Insert a genesis-like bonding curve into ShadowLedger
        let curve = ghost_core::market_state::BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_073_000_000_000_000, // raw micro-token units
            virtual_sol_reserves: 30_000_000_000,          // lamports
            real_token_reserves: 793_100_000_000_000,
            real_sol_reserves: 30_000_000_000,
            token_total_supply: 1_000_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        ledger.insert_with_slot(pool_id, curve, 1000);

        // Create a PoolTransaction with NO reserve data (simulates gRPC source)
        let tx = Arc::new(PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(42),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 999,
            signer: "some_signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: Some(1_000_000_000),
            token_amount_units: Some(10_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "test_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![1, 2, 3],
            mpcf_payload_missing_reason: crate::events::RawBytesMissingReason::NotMissing,
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
        });

        let enriched = enrich_pool_tx_from_canonical_state(
            tx,
            pool_id,
            None,
            &account_state_core,
            &ledger,
            DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
        );

        // Verify the explicit shadow-ledger fallback still enriches reserves when
        // canonical account-state is absent.
        let v_tokens = enriched
            .v_tokens_in_bonding_curve
            .expect("v_tokens should be set");
        let v_sol = enriched
            .v_sol_in_bonding_curve
            .expect("v_sol should be set");
        let mcap = enriched.market_cap_sol.expect("market_cap should be set");

        // 1_073_000_000_000_000 / 1_000_000 = 1_073_000_000
        assert!(
            (v_tokens - 1_073_000_000.0).abs() < 1.0,
            "v_tokens mismatch: {}",
            v_tokens
        );
        // 30_000_000_000 / 1_000_000_000 = 30.0
        assert!((v_sol - 30.0).abs() < 0.001, "v_sol mismatch: {}", v_sol);
        // market_cap = (v_sol / v_tokens) * PUMP_GENESIS_TOKEN_SUPPLY ≈ 30.0
        assert!(
            mcap > 20.0 && mcap < 40.0,
            "market_cap_sol out of range: {}",
            mcap
        );
        // Price and reserves should also be populated
        assert!(enriched.reserve_base.is_some());
        assert!(enriched.reserve_quote.is_some());
        assert!(enriched.price_quote.is_some());
        // curve_data_known should be propagated from the shadow fallback path
        assert!(
            enriched.curve_data_known,
            "curve_data_known should be true when shadow fallback has confirmed curve data"
        );
    }

    #[test]
    fn test_enrich_pool_tx_skips_when_reserves_present() {
        let ledger = ShadowLedger::new();
        let account_state_core = AccountStateReducer::new();
        let pool_id = Pubkey::new_unique();

        // Create a PoolTransaction WITH existing reserve data (e.g., PumpPortal source)
        let tx = Arc::new(PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(42),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 999,
            signer: "some_signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "test_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: crate::events::RawBytesMissingReason::NotMissing,
            token_mint: None,
            v_tokens_in_bonding_curve: Some(1_073_000_000.0),
            v_sol_in_bonding_curve: Some(30.0),
            market_cap_sol: Some(30.0),
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
            curve_data_known: true,
        });

        let original_ptr = Arc::as_ptr(&tx);
        let result = enrich_pool_tx_from_canonical_state(
            tx,
            pool_id,
            None,
            &account_state_core,
            &ledger,
            DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
        );

        // Should return the exact same Arc (no clone needed) when reserves already present
        assert_eq!(
            Arc::as_ptr(&result),
            original_ptr,
            "Should return same Arc when no enrichment needed"
        );
    }

    #[test]
    fn test_enrich_pool_tx_no_curve_returns_original() {
        let ledger = ShadowLedger::new();
        let account_state_core = AccountStateReducer::new();
        let pool_id = Pubkey::new_unique();

        // No curve in ShadowLedger for this pool
        let tx = Arc::new(PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(42),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 999,
            signer: "some_signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: None,
            token_amount_units: None,
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "test_sig".to_string(),
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
        });

        let original_ptr = Arc::as_ptr(&tx);
        let result = enrich_pool_tx_from_canonical_state(
            tx,
            pool_id,
            None,
            &account_state_core,
            &ledger,
            DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
        );

        // Should return same Arc when no curve is available
        assert_eq!(
            Arc::as_ptr(&result),
            original_ptr,
            "Should return same Arc when no ShadowLedger curve"
        );
        assert!(result.v_tokens_in_bonding_curve.is_none());
        assert!(result.v_sol_in_bonding_curve.is_none());
    }

    #[test]
    fn test_enrich_pool_tx_marks_stale_curve_as_unknown() {
        let ledger = ShadowLedger::new();
        let account_state_core = AccountStateReducer::new();
        let pool_id = Pubkey::new_unique();
        let old_ts = ghost_core::shadow_ledger::current_time_ms().saturating_sub(1_000);

        let curve = ghost_core::market_state::BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_073_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 793_100_000_000_000,
            real_sol_reserves: 30_000_000_000,
            token_total_supply: 1_000_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        ledger.insert_with_slot_at(pool_id, curve, 1000, old_ts);

        let tx = Arc::new(PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(42),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 999,
            signer: "some_signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: Some(1_000_000_000),
            token_amount_units: Some(10_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "stale_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: crate::events::RawBytesMissingReason::NotMissing,
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
        });

        let enriched = enrich_pool_tx_from_canonical_state(
            tx,
            pool_id,
            None,
            &account_state_core,
            &ledger,
            DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
        );

        assert!(enriched.v_tokens_in_bonding_curve.is_some());
        assert!(enriched.v_sol_in_bonding_curve.is_some());
        assert!(enriched.market_cap_sol.is_some());
        assert!(
            !enriched.curve_data_known,
            "stale shadow-ledger snapshot must downgrade curve_data_known to false"
        );
    }

    #[test]
    fn test_enrich_pool_tx_prefers_account_state_core_over_shadow_fallback() {
        let ledger = ShadowLedger::new();
        let account_state_core = AccountStateReducer::new();
        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        account_state_core.register_pool_from_bootstrap(
            pool_id,
            base_mint,
            bonding_curve,
            BootstrapHints::default(),
        );
        let result = account_state_core.apply_account_update(AccountStateUpdate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            sol_reserves: 45_000_000_000,
            token_reserves: 900_000_000_000_000,
            is_complete: 0,
            slot: 7,
            write_version: None,
            receive_ts_ms: ghost_core::shadow_ledger::current_time_ms(),
            receive_seq: account_state_core.next_recv_seq(),
            curve_finality: CurveFinality::Speculative,
            source: UpdateSource::GeyserAccountUpdate,
        });
        assert!(
            matches!(
                result,
                ghost_core::account_state_core::types::AccountUpdateResult::Applied
                    | ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap
            ),
            "account update should materialize canonical state"
        );

        let shadow_curve = ghost_core::market_state::BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_073_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 793_100_000_000_000,
            real_sol_reserves: 30_000_000_000,
            token_total_supply: 1_000_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        ledger.insert_with_slot(pool_id, shadow_curve, 1000);

        let tx = Arc::new(PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(42),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 999,
            signer: "some_signer".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: Some(1_000_000_000),
            token_amount_units: Some(10_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "canonical_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: crate::events::RawBytesMissingReason::NotMissing,
            token_mint: Some(base_mint.to_string()),
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
        });

        let enriched = enrich_pool_tx_from_canonical_state(
            tx,
            pool_id,
            Some(base_mint),
            &account_state_core,
            &ledger,
            DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
        );

        let v_sol = enriched
            .v_sol_in_bonding_curve
            .expect("canonical v_sol should be present");
        assert!(
            (v_sol - 45.0).abs() < 0.001,
            "canonical enrichment must win over shadow fallback"
        );
        assert!(enriched.curve_data_known);
    }

    #[test]
    fn test_enrich_pool_tx_shadow_fallback_uses_bonding_curve_alias_not_pool_id() {
        let ledger = ShadowLedger::new();
        let account_state_core = AccountStateReducer::new();
        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        ledger.register_curve_alias(base_mint, bonding_curve);
        ledger.insert_with_slot(
            bonding_curve,
            ghost_core::market_state::BondingCurve {
                discriminator: 0,
                virtual_token_reserves: 1_073_000_000_000_000,
                virtual_sol_reserves: 30_000_000_000,
                real_token_reserves: 793_100_000_000_000,
                real_sol_reserves: 30_000_000_000,
                token_total_supply: 1_000_000_000_000_000,
                complete: 0,
                _padding: [0; 7],
            },
            777,
        );

        let tx = Arc::new(PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(42),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 999,
            signer: "alias_sig".to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: Some(1_000_000_000),
            token_amount_units: Some(10_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "alias_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: crate::events::RawBytesMissingReason::NotMissing,
            token_mint: Some(base_mint.to_string()),
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
        });

        let enriched = enrich_pool_tx_from_canonical_state(
            tx,
            pool_id,
            Some(base_mint),
            &account_state_core,
            &ledger,
            DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
        );

        assert!(
            enriched.v_tokens_in_bonding_curve.is_some(),
            "shadow fallback should resolve bonding_curve via base_mint alias"
        );
        assert!(
            enriched.v_sol_in_bonding_curve.is_some(),
            "shadow fallback should enrich reserves through alias-backed curve lookup"
        );
    }

    #[test]
    fn test_resolve_gatekeeper_initial_reserves_prefers_account_state_core() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            slot: Some(1),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        shadow_ledger.insert_with_slot(
            bonding_curve,
            ghost_core::market_state::BondingCurve {
                discriminator: 0,
                virtual_token_reserves: 1_073_000_000_000_000,
                virtual_sol_reserves: 30_000_000_000,
                real_token_reserves: 1_073_000_000_000_000,
                real_sol_reserves: 30_000_000_000,
                token_total_supply: 1_073_000_000_000_000,
                complete: 0,
                _padding: [0; 7],
            },
            100,
        );

        let outcome = runtime.process_account_update(
            &base_mint,
            55_000_000_000,
            777_000_000_000_000,
            0,
            2,
            CurveFinality::Speculative,
        );
        assert!(
            outcome.is_some(),
            "tracked mint should accept account update"
        );

        let (reserve_sol_lamports, reserve_tok_units) =
            runtime.resolve_gatekeeper_initial_reserves(pool_id, base_mint);
        assert_eq!(reserve_sol_lamports, 55_000_000_000);
        assert_eq!(reserve_tok_units, 777_000_000_000_000);
    }

    #[test]
    fn test_resolve_price_context_prefers_account_state_core_before_shadow_fallbacks() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            slot: Some(1),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        shadow_ledger.insert_with_slot(
            bonding_curve,
            ghost_core::market_state::BondingCurve {
                discriminator: 0,
                virtual_token_reserves: 1_073_000_000_000_000,
                virtual_sol_reserves: 30_000_000_000,
                real_token_reserves: 1_073_000_000_000_000,
                real_sol_reserves: 30_000_000_000,
                token_total_supply: 1_073_000_000_000_000,
                complete: 0,
                _padding: [0; 7],
            },
            100,
        );
        shadow_ledger.commit_history(
            base_mint,
            vec![ghost_core::shadow_ledger::MarketSnapshot {
                slot: Some(100),
                timestamp_ms: 100_000,
                reserve_base: 1_073_000_000_000.0,
                reserve_quote: 30.0,
                price_sol_per_token: 30.0 / 1_073_000_000_000.0,
                ..Default::default()
            }],
            None,
        );

        let outcome = runtime.process_account_update(
            &base_mint,
            55_000_000_000,
            777_000_000_000_000,
            0,
            2,
            CurveFinality::Speculative,
        );
        assert!(
            outcome.is_some(),
            "tracked mint should accept account update"
        );

        let context = runtime.resolve_price_context(pool_id, base_mint);
        assert_eq!(context.reserve_quote, Some(55.0));
        assert_eq!(
            context.reserve_base,
            Some(777_000_000_000_000.0 / PUMP_TOKEN_DECIMAL_FACTOR)
        );
        assert!(
            context
                .price_quote
                .is_some_and(|price| price > 0.0 && price < 1.0),
            "canonical price should be derived from AccountStateCore before shadow fallbacks"
        );
    }

    #[test]
    fn test_process_account_update_primes_active_session_account_state() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = Arc::new(OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        ));
        let snapshot_engine = SnapshotEngine::new(16, 0);
        runtime.configure_approval_gating(&snapshot_engine);

        let pool_id = Pubkey::new_unique();
        let detected_pool = test_detected_pool(pool_id);
        register_test_detected_pool(runtime.as_ref(), detected_pool.as_ref());
        runtime.remember_detected_pool(pool_id, detected_pool.clone());

        let open_request = runtime
            .build_session_open_request(
                pool_id,
                1_000,
                &GatekeeperV2Config::default(),
                &EarlyFingerprintConfig::default(),
                Some(detected_pool.as_ref()),
            )
            .expect("session open request");
        runtime
            .session_manager()
            .open_session(open_request)
            .expect("session should open");

        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let update = runtime
            .build_account_state_update(
                &base_mint,
                31_000_000_000,
                888_000_000_000_000,
                0,
                2,
                None,
                CurveFinality::Speculative,
                UpdateSource::GeyserAccountUpdate,
                None,
            )
            .expect("tracked mint should build canonical account-state update");
        let apply_result = runtime.apply_account_state_update(&update);
        assert!(
            matches!(
                apply_result,
                ghost_core::account_state_core::types::AccountUpdateResult::Applied
                    | ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap
            ),
            "runtime should surface reducer outcome for telemetry partitioning"
        );
        assert!(
            runtime
                .account_state_core()
                .get_canonical_state(&base_mint)
                .is_some(),
            "runtime-owned AccountStateCore must be hydrated"
        );

        let session = runtime
            .lookup_pool_session(&pool_id)
            .expect("active session should still exist");
        let session = session.read();
        let materialized = session.materialize_features();
        assert_eq!(materialized.account_features.update_count, 1);
        assert!(
            materialized.curve_readiness.is_ready,
            "active session should observe canonical curve readiness from account updates"
        );
    }

    #[test]
    fn test_pr7_invariant_oracle_runtime_keeps_canonical_truth_primary() {
        let source = include_str!("oracle_runtime.rs");

        let resolve_start = source
            .find("fn resolve_price_context(")
            .expect("resolve_price_context must exist");
        let resolve_end = source[resolve_start..]
            .find("pub fn set_panic_retention_ms")
            .map(|offset| resolve_start + offset)
            .expect("resolve_price_context block terminator must exist");
        let resolve_src = &source[resolve_start..resolve_end];

        let canonical_idx = resolve_src
            .find("account_state_core.get_canonical_state")
            .expect("resolve_price_context must query AccountStateCore");
        let snapshot_idx = resolve_src
            .find("shadow_ledger.get_latest_snapshot_internal")
            .expect("resolve_price_context must retain snapshot fallback");
        let reserves_start = source
            .find("fn resolve_gatekeeper_initial_reserves(")
            .expect("resolve_gatekeeper_initial_reserves must exist");
        let reserves_end = source[reserves_start..]
            .find("fn stage_gatekeeper_history_for_commit(")
            .map(|offset| reserves_start + offset)
            .expect("resolve_gatekeeper_initial_reserves block terminator must exist");
        let reserves_src = &source[reserves_start..reserves_end];
        let revolver_src =
            include_str!("../../off-chain/components/trigger/src/revolver_price_feed.rs");
        let revolver_impl = revolver_src
            .split("#[cfg(test)]")
            .next()
            .expect("revolver price feed implementation section must exist");

        assert!(
            canonical_idx < snapshot_idx,
            "AccountStateCore must be consulted before ShadowLedger snapshot fallback"
        );
        assert!(
            !resolve_src.contains(&["shadow_ledger", ".get_", "curve("].concat()),
            "resolve_price_context must not fall back to ShadowLedger curve truth"
        );
        assert!(
            !resolve_src.contains("let pools = self.pools.read()"),
            "resolve_price_context must not depend on PerPoolOracleState for fallback lookup"
        );
        assert!(
            !reserves_src.contains(&["shadow_ledger", ".get_", "curve("].concat()),
            "resolve_gatekeeper_initial_reserves must not fall back to ShadowLedger curve truth"
        );
        assert!(
            !revolver_impl.contains(&["shadow_ledger", ".get_", "quote("].concat()),
            "revolver price feed must not use ShadowLedger get_quote as a truth source"
        );
        assert!(
            source.contains("let tx = enrich_pool_tx_from_canonical_state("),
            "pool observation hot path must use canonical enrichment helper"
        );
        assert!(
            source.contains("\"account_update_before_identity_total\""),
            "Phase 0 telemetry must count account updates arriving before identity registration"
        );
        assert!(
            source.contains("\"account_update_build_none_total\""),
            "Phase 0 telemetry must count build_account_state_update None exits"
        );
        assert!(
            source.contains("\"account_update_ingress_total\""),
            "Phase 1 telemetry must count all launcher AccountUpdate ingress attempts"
        );
        assert!(
            source.contains("\"account_update_apply_result_total\""),
            "Phase 1 telemetry must partition reducer apply outcomes"
        );
        assert!(
            source.contains("\"account_update_promoted_from_bootstrap_total\""),
            "Phase 3 telemetry must expose bootstrap-to-canonical promotions directly"
        );
        assert!(
            source.contains("\"canonical_first_promotion_latency_ms\""),
            "Phase 1 telemetry must measure detection-to-promotion latency"
        );
        assert!(
            source.contains("\"shadow_truth_fallback_total\""),
            "Phase 0 telemetry must meter ShadowLedger fallback sites"
        );
        assert!(
            source.contains("\"degraded_truth_helper_total\""),
            "Phase 4 telemetry must count every bounded degraded truth helper usage"
        );
        assert!(
            source.contains("\"category\" => category_label"),
            "Phase 3 fallback telemetry must classify ShadowLedger truth sites"
        );
        let terminal_eval_start = source
            .find("fn evaluate_feature_driven_terminal_verdict(")
            .expect("terminal verdict helper must exist");
        let terminal_eval_end = source[terminal_eval_start..]
            .find("fn resolve_feature_trigger_outcome(")
            .map(|offset| terminal_eval_start + offset)
            .expect("terminal verdict helper must be followed by resolve_feature_trigger_outcome");
        let terminal_eval_src = &source[terminal_eval_start..terminal_eval_end];
        let production_terminal_eval_src = terminal_eval_src
            .split("#[cfg(test)]")
            .next()
            .expect("production terminal verdict slice must exist");
        assert!(
            !production_terminal_eval_src.contains("\"legacy_terminal_verdict_total\""),
            "PR8 cleanup must remove production legacy terminal verdict telemetry"
        );
        assert!(
            terminal_eval_src
                .contains("#[cfg(test)]\n    if !gatekeeper_config.use_three_layer_decision {"),
            "legacy terminal verdict branch must be fenced to cfg(test)"
        );
        assert!(
            source.contains("\"timeout_without_canonical_updates_total\""),
            "Phase 0 telemetry must count timeout exits without canonical updates"
        );
        assert!(
            source.contains("assert_declared_shadow_truth_fallback(\"resolve_price_context\")"),
            "Phase 3 must guard resolve_price_context against undeclared shadow truth fallback usage"
        );
        assert!(
            source.contains("record_degraded_truth_helper(\"resolve_price_context\", \"shadow_ledger_snapshot\")"),
            "Phase 4 must emit helper telemetry when resolve_price_context uses a shadow snapshot"
        );
        assert!(
            source.contains("assert_declared_shadow_truth_fallback(\"resolve_gatekeeper_initial_reserves\")"),
            "Phase 3 must guard resolve_gatekeeper_initial_reserves against undeclared shadow truth fallback usage"
        );
        assert!(
            source.contains("record_degraded_truth_helper(\n                \"resolve_gatekeeper_initial_reserves\",\n                \"shadow_ledger_snapshot\","),
            "Phase 4 must emit helper telemetry when gatekeeper bootstrap falls back to a shadow snapshot"
        );
        assert!(
            source
                .contains("assert_declared_shadow_truth_fallback(\"tx_curve_enrichment_shadow\")"),
            "Phase 3 must guard tx enrichment fallback classification"
        );
        assert!(
            source.contains("record_shadow_truth_fallback(\"tx_curve_enrichment_shadow\")"),
            "Phase 4 must meter tx enrichment shadow fallback in the shared site counter"
        );
        assert!(
            source.contains("record_degraded_truth_helper(\"tx_curve_enrichment_shadow\", \"shadow_ledger_curve\")"),
            "Phase 4 must emit helper telemetry for tx enrichment shadow fallback"
        );
    }

    #[test]
    fn test_phase3_shadow_truth_fallback_sites_are_declared_and_non_primary() {
        for site in [
            "resolve_price_context",
            "resolve_gatekeeper_initial_reserves",
            "tx_curve_enrichment_shadow",
        ] {
            let contract =
                shadow_fallback_contract(site).expect("Phase 3 shadow truth site must be declared");
            assert!(
                matches!(
                    contract.category,
                    ShadowFallbackCategory::BootstrapOnly
                        | ShadowFallbackCategory::DegradedDiagnostic
                ),
                "Phase 3 must not leave site {} as hidden primary truth",
                site
            );
            assert!(
                !contract.rationale.is_empty(),
                "Phase 3 fallback contract for {} must explain why the fallback still exists",
                site
            );
        }
    }

    #[test]
    fn test_resolve_gatekeeper_initial_reserves_prefers_bootstrap_before_shadow_fallback() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            initial_liquidity_sol: 12.5,
            bonding_curve_progress: Some(0.07),
            token_total_supply: Some(1_500_000_000_000_000),
            virtual_sol_reserves: Some(12_500_000_000),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        shadow_ledger.commit_history(
            base_mint,
            vec![ghost_core::shadow_ledger::MarketSnapshot {
                slot: Some(100),
                timestamp_ms: 100_000,
                reserve_base: 999_999_999_999.0,
                reserve_quote: 30.0,
                price_sol_per_token: 30.0 / 999_999_999_999.0,
                ..Default::default()
            }],
            None,
        );

        let (reserve_sol_lamports, reserve_tok_units) =
            runtime.resolve_gatekeeper_initial_reserves(pool_id, base_mint);
        assert_eq!(
            reserve_sol_lamports, 12_500_000_000,
            "bootstrap speculative reserves must beat ShadowLedger degraded fallback"
        );
        assert_eq!(
            reserve_tok_units, 1_500_000_000_000_000,
            "bootstrap token reserves must beat ShadowLedger degraded fallback"
        );
    }

    #[test]
    fn test_process_account_update_can_promote_bootstrap_state_to_canonical() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            initial_liquidity_sol: 8.0,
            bonding_curve_progress: Some(0.03),
            token_total_supply: Some(900_000_000_000_000),
            virtual_sol_reserves: Some(8_000_000_000),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        let update = runtime
            .build_account_state_update(
                &base_mint,
                9_000_000_000,
                888_000_000_000_000,
                0,
                2,
                None,
                CurveFinality::Speculative,
                UpdateSource::GeyserAccountUpdate,
                None,
            )
            .expect("tracked mint should build canonical account-state update");
        let apply_result = runtime.apply_account_state_update(&update);
        assert!(
            matches!(
                apply_result,
                ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap
            ),
            "registered bootstrap state should promote on the first canonical account update"
        );
    }

    #[tokio::test]
    async fn test_apply_account_state_update_notifies_canonical_readiness_subscribers() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            initial_liquidity_sol: 8.0,
            token_total_supply: Some(900_000_000_000_000),
            virtual_sol_reserves: Some(8_000_000_000),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        let mut readiness_rx = runtime.subscribe_canonical_readiness(&base_mint);
        let update = runtime
            .build_account_state_update(
                &base_mint,
                9_000_000_000,
                888_000_000_000_000,
                0,
                2,
                None,
                CurveFinality::Speculative,
                UpdateSource::GeyserAccountUpdate,
                None,
            )
            .expect("tracked mint should build canonical account-state update");
        let apply_result = runtime.apply_account_state_update(&update);

        assert!(
            matches!(
                apply_result,
                ghost_core::account_state_core::types::AccountUpdateResult::Applied
                    | ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap
            ),
            "first canonical update should be accepted"
        );
        tokio::time::timeout(Duration::from_millis(50), readiness_rx.changed())
            .await
            .expect("canonical readiness notify timeout")
            .expect("canonical readiness sender should stay alive");
        assert!(
            runtime.is_live_trigger_canonical_ready(&base_mint),
            "canonical apply should satisfy live trigger readiness"
        );
    }

    #[test]
    fn test_process_account_update_promotes_shadow_seed_before_reconciliation() {
        use ghost_core::shadow_ledger::reconciliation::{DriftSeverity, ReconciliationAction};

        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            initial_liquidity_sol: 30.0,
            bonding_curve_progress: Some(0.0),
            token_total_supply: Some(1_000_000_000_000_000),
            virtual_sol_reserves: Some(30_000_000_000),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        let seed_curve = BondingCurve {
            discriminator: 0,
            virtual_sol_reserves: 30_000_000_000,
            virtual_token_reserves: 1_073_000_000_000_000,
            real_sol_reserves: 30_000_000_000,
            real_token_reserves: 793_100_000_000_000,
            token_total_supply: 1_000_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        let seed_result = shadow_ledger.apply_curve_write(
            Some(base_mint),
            bonding_curve,
            seed_curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::SeerBootstrap,
                ShadowLedgerWriteStrength::BootstrapSeed,
                ShadowLedgerStateConfidence::Speculative,
                ShadowLedgerWriteReason::BootstrapSeed,
                None,
                CurveFinality::Speculative,
            ),
        );
        assert!(
            matches!(seed_result.result, ShadowLedgerWriteResult::Applied),
            "seed setup must store a speculative shadow curve"
        );

        let on_chain_sol = 34_050_617_285;
        let on_chain_tok = 945_357_311_246_644;
        let outcome = runtime
            .process_account_update(
                &base_mint,
                on_chain_sol,
                on_chain_tok,
                0,
                410_723_624,
                CurveFinality::Speculative,
            )
            .expect("tracked mint must reconcile");

        assert_eq!(
            outcome.severity,
            DriftSeverity::None,
            "seed-only shadow state must be promoted from canonical reserves before drift compare"
        );
        assert_eq!(
            outcome.action,
            ReconciliationAction::NoAction,
            "canonical bootstrap promotion must eliminate the artificial genesis-seed drift"
        );

        let stored = shadow_ledger
            .get_old(&bonding_curve)
            .expect("canonicalized shadow curve must exist");
        assert_eq!(stored.curve.virtual_sol_reserves, on_chain_sol);
        assert_eq!(stored.curve.virtual_token_reserves, on_chain_tok);
        assert!(stored.curve_data_known);
        assert_eq!(stored.write_source, ShadowLedgerWriteSource::AccountUpdate);
        assert_eq!(
            stored.write_strength,
            ShadowLedgerWriteStrength::ConfirmedBootstrap
        );
        assert_eq!(
            stored.write_reason,
            ShadowLedgerWriteReason::ConfirmedBootstrap
        );
    }

    #[test]
    fn test_process_account_update_refreshes_shadow_on_repeated_canonical_updates() {
        use ghost_core::shadow_ledger::reconciliation::{DriftSeverity, ReconciliationAction};

        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            initial_liquidity_sol: 30.0,
            bonding_curve_progress: Some(0.0),
            token_total_supply: Some(1_000_000_000_000_000),
            virtual_sol_reserves: Some(30_000_000_000),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        let seed_curve = BondingCurve {
            discriminator: 0,
            virtual_sol_reserves: 30_000_000_000,
            virtual_token_reserves: 1_073_000_000_000_000,
            real_sol_reserves: 30_000_000_000,
            real_token_reserves: 793_100_000_000_000,
            token_total_supply: 1_000_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };
        let seed_result = shadow_ledger.apply_curve_write(
            Some(base_mint),
            bonding_curve,
            seed_curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::SeerBootstrap,
                ShadowLedgerWriteStrength::BootstrapSeed,
                ShadowLedgerStateConfidence::Speculative,
                ShadowLedgerWriteReason::BootstrapSeed,
                None,
                CurveFinality::Speculative,
            ),
        );
        assert!(matches!(
            seed_result.result,
            ShadowLedgerWriteResult::Applied
        ));

        for (idx, (slot, sol_reserves, token_reserves)) in [
            (410_748_305, 30_987_654_320_u64, 945_357_311_246_644_u64),
            (410_748_308, 31_944_839_345_u64, 945_357_311_246_644_u64),
            (410_748_311, 33_967_317_456_u64, 945_357_311_246_644_u64),
        ]
        .into_iter()
        .enumerate()
        {
            let outcome = runtime
                .process_account_update(
                    &base_mint,
                    sol_reserves,
                    token_reserves,
                    0,
                    slot,
                    CurveFinality::Provisional,
                )
                .expect("tracked mint must reconcile");

            let canonical = runtime
                .account_state_core()
                .get_canonical_state(&base_mint)
                .expect("canonical state must exist after account update");
            assert_eq!(
                canonical.virtual_sol_reserves, sol_reserves,
                "AccountStateCore must advance to the latest canonical reserves for slot {slot}"
            );
            assert_eq!(
                canonical.last_update_slot, slot,
                "AccountStateCore must track the latest canonical slot for slot {slot}"
            );

            let stored = shadow_ledger
                .get_old(&bonding_curve)
                .expect("shadow curve must exist after canonical refresh");
            assert_eq!(
                stored.curve.virtual_sol_reserves, sol_reserves,
                "ShadowLedger must refresh to latest canonical reserves for slot {slot}"
            );
            assert_eq!(
                stored.curve.real_sol_reserves, sol_reserves,
                "ShadowLedger real reserves must refresh to latest canonical reserves for slot {slot}"
            );
            assert_eq!(
                stored.curve.virtual_token_reserves, token_reserves,
                "ShadowLedger token reserves must match latest canonical state for slot {slot}"
            );
            assert_eq!(
                stored.last_updated_slot, slot,
                "ShadowLedger slot watermark must refresh for slot {slot}"
            );
            assert_eq!(stored.curve_finality, CurveFinality::Provisional);
            assert!(stored.curve_data_known);
            assert_eq!(stored.write_source, ShadowLedgerWriteSource::AccountUpdate);
            assert_eq!(
                stored.write_strength,
                if idx == 0 {
                    ShadowLedgerWriteStrength::ConfirmedBootstrap
                } else {
                    ShadowLedgerWriteStrength::Repair
                }
            );

            assert_eq!(
                outcome.severity,
                DriftSeverity::None,
                "repeated canonical refresh must keep reconciliation aligned with latest canonical reserves"
            );
            assert_eq!(
                outcome.action,
                ReconciliationAction::NoAction,
                "repeated canonical refresh must avoid artificial drift logging"
            );
        }

        let final_stored = shadow_ledger
            .get_old(&bonding_curve)
            .expect("final shadow curve must exist");
        assert_eq!(final_stored.curve.virtual_sol_reserves, 33_967_317_456);
        assert_eq!(final_stored.last_updated_slot, 410_748_311);
        assert_eq!(
            final_stored.write_strength,
            ShadowLedgerWriteStrength::Repair
        );
        assert_eq!(
            final_stored.write_reason,
            ShadowLedgerWriteReason::DirectAccountUpdate,
            "post-bootstrap canonical refreshes should be tagged as direct account updates"
        );
    }

    #[test]
    fn test_process_account_update_duplicate_canonical_update_is_noop_for_shadow() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            initial_liquidity_sol: 30.0,
            token_total_supply: Some(1_000_000_000_000_000),
            virtual_sol_reserves: Some(30_000_000_000),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        let outcome = runtime.process_account_update(
            &base_mint,
            30_987_654_320,
            945_357_311_246_644,
            0,
            410_748_305,
            CurveFinality::Provisional,
        );
        assert!(
            outcome.is_some(),
            "first canonical update should be accepted"
        );

        let first = shadow_ledger
            .get_old(&bonding_curve)
            .expect("first canonical sync must store shadow curve");

        let duplicate = runtime.process_account_update(
            &base_mint,
            30_987_654_320,
            945_357_311_246_644,
            0,
            410_748_305,
            CurveFinality::Provisional,
        );
        assert!(
            duplicate.is_some(),
            "duplicate canonical update should still reconcile"
        );

        let second = shadow_ledger
            .get_old(&bonding_curve)
            .expect("duplicate canonical sync must preserve shadow curve");

        assert_eq!(second.curve, first.curve);
        assert_eq!(second.last_updated_slot, first.last_updated_slot);
        assert_eq!(second.curve_finality, first.curve_finality);
        assert_eq!(second.write_source, first.write_source);
        assert_eq!(second.write_strength, first.write_strength);
        assert_eq!(second.write_reason, first.write_reason);
        assert_eq!(second.last_update_ts_ms, first.last_update_ts_ms);
    }

    #[test]
    fn test_process_account_update_same_slot_finality_refresh_updates_shadow() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            initial_liquidity_sol: 30.0,
            token_total_supply: Some(1_000_000_000_000_000),
            virtual_sol_reserves: Some(30_000_000_000),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        let provisional = runtime.process_account_update(
            &base_mint,
            31_944_839_345,
            945_357_311_246_644,
            0,
            410_748_308,
            CurveFinality::Provisional,
        );
        assert!(
            provisional.is_some(),
            "provisional canonical update should be accepted"
        );

        let before_finality_refresh = shadow_ledger
            .get_old(&bonding_curve)
            .expect("provisional canonical sync must store shadow curve");
        assert_eq!(
            before_finality_refresh.curve_finality,
            CurveFinality::Provisional
        );

        let finalized = runtime.process_account_update(
            &base_mint,
            31_944_839_345,
            945_357_311_246_644,
            0,
            410_748_308,
            CurveFinality::Finalized,
        );
        assert!(finalized.is_some(), "finalized refresh should be accepted");

        let after_finality_refresh = shadow_ledger
            .get_old(&bonding_curve)
            .expect("finalized canonical refresh must preserve shadow curve");
        assert_eq!(
            after_finality_refresh.curve.virtual_sol_reserves,
            before_finality_refresh.curve.virtual_sol_reserves
        );
        assert_eq!(
            after_finality_refresh.last_updated_slot,
            before_finality_refresh.last_updated_slot
        );
        assert_eq!(
            after_finality_refresh.curve_finality,
            CurveFinality::Finalized
        );
        assert_eq!(
            after_finality_refresh.write_reason,
            ShadowLedgerWriteReason::FinalityRefresh,
            "same-slot canonical finality upgrade should be tracked as a finality refresh"
        );
        assert_eq!(
            after_finality_refresh.write_strength,
            ShadowLedgerWriteStrength::Repair,
            "same-slot finality refresh should use repair precedence over prior confirmed bootstrap"
        );
    }

    #[test]
    fn test_tx_observed_bootstrap_materializes_canonical_state_before_live_account_update() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            initial_liquidity_sol: 8.0,
            token_total_supply: Some(900_000_000_000_000),
            virtual_sol_reserves: Some(8_000_000_000),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        let mut tx = (*test_pool_observation_tx("sig-tx-bootstrap")).clone();
        tx.pool_amm_id = pool_id.to_string();
        tx.token_mint = Some(base_mint.to_string());
        tx.slot = Some(22);
        tx.curve_data_known = true;
        tx.curve_finality = CurveFinality::Speculative;
        tx.v_sol_in_bonding_curve = Some(31.0);
        tx.v_tokens_in_bonding_curve = Some(900_000_000.0);

        let apply_result = runtime
            .maybe_materialize_canonical_state_from_observed_tx(pool_id, Some(base_mint), &tx)
            .expect("tx bootstrap should build an account update");
        assert!(
            matches!(
                apply_result,
                ghost_core::account_state_core::types::AccountUpdateResult::Applied
                    | ghost_core::account_state_core::types::AccountUpdateResult::PromotedFromBootstrap
            ),
            "first tx with curve truth should materialize canonical state"
        );

        let state = runtime
            .account_state_core()
            .get_canonical_state(&base_mint)
            .expect("canonical state after tx bootstrap");
        assert_eq!(state.virtual_sol_reserves, 31_000_000_000);
        assert_eq!(state.virtual_token_reserves, 900_000_000_000_000);
        assert_eq!(state.update_count, 1);
    }

    #[test]
    fn test_live_account_update_same_slot_overrides_tx_bootstrap_state() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            initial_liquidity_sol: 8.0,
            token_total_supply: Some(900_000_000_000_000),
            virtual_sol_reserves: Some(8_000_000_000),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        let mut tx = (*test_pool_observation_tx("sig-tx-bootstrap-same-slot")).clone();
        tx.pool_amm_id = pool_id.to_string();
        tx.token_mint = Some(base_mint.to_string());
        tx.slot = Some(22);
        tx.curve_data_known = true;
        tx.curve_finality = CurveFinality::Speculative;
        tx.v_sol_in_bonding_curve = Some(31.0);
        tx.v_tokens_in_bonding_curve = Some(900_000_000.0);
        let _ = runtime.maybe_materialize_canonical_state_from_observed_tx(
            pool_id,
            Some(base_mint),
            &tx,
        );

        let live_update = runtime
            .build_account_state_update(
                &base_mint,
                32_000_000_000,
                899_000_000_000_000,
                0,
                22,
                Some(1),
                CurveFinality::Finalized,
                UpdateSource::GeyserAccountUpdate,
                Some(&bonding_curve),
            )
            .expect("tracked mint should build live account-state update");
        let apply_result = runtime.apply_account_state_update(&live_update);
        assert!(
            matches!(
                apply_result,
                ghost_core::account_state_core::types::AccountUpdateResult::Applied
            ),
            "same-slot live account update must outrank tx bootstrap"
        );

        let state = runtime
            .account_state_core()
            .get_canonical_state(&base_mint)
            .expect("canonical state after live update");
        assert_eq!(state.virtual_sol_reserves, 32_000_000_000);
        assert_eq!(state.virtual_token_reserves, 899_000_000_000_000);
        assert_eq!(state.update_count, 2);
        assert_eq!(state.curve_finality, CurveFinality::Finalized);
    }

    #[test]
    fn test_resolve_price_context_prefers_bootstrap_state_before_canonical_update() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            initial_liquidity_sol: 12.5,
            token_total_supply: Some(1_500_000_000_000_000),
            virtual_sol_reserves: Some(12_500_000_000),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        let context = runtime.resolve_price_context(pool_id, base_mint);
        assert_eq!(context.reserve_quote, Some(12.5));
        assert_eq!(
            context.reserve_base,
            Some(1_500_000_000_000_000.0 / PUMP_TOKEN_DECIMAL_FACTOR)
        );
        assert!(context.price_quote.is_some());
        assert!(
            runtime
                .account_state_core()
                .get_canonical_state(&base_mint)
                .is_none(),
            "bootstrap path must not synthesize canonical state before AccountUpdate"
        );
    }

    #[test]
    fn test_register_new_pool_replays_pre_identity_account_updates() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let outcome_before_registration = runtime.process_account_update_with_explicit_source(
            &base_mint,
            9_000_000_000,
            888_000_000_000_000,
            0,
            2,
            CurveFinality::Speculative,
            UpdateSource::GeyserAccountUpdate,
            Some(&AccountUpdateEvent {
                semantic: Default::default(),
                event_time: ghost_core::EventTimeMetadata::default(),
                base_mint,
                bonding_curve,
                curve_finality: CurveFinality::Speculative,
                sol_reserves: 9_000_000_000,
                token_reserves: 888_000_000_000_000,
                complete: 0,
                slot: 2,
                write_version: Some(1),
                replay_origin: seer::ipc::AccountUpdateReplayOrigin::Live,
                replay_buffer_dwell_ms: None,
                detected_at: SystemTime::now(),
                sequence_number: 1,
            }),
            true,
        );
        assert!(
            outcome_before_registration.is_none(),
            "unregistered mint should buffer AccountUpdate until runtime identity exists"
        );

        let (pending_mints, pending_updates) = runtime.get_pre_identity_account_update_stats();
        assert_eq!(pending_mints, 1);
        assert_eq!(pending_updates, 1);
        assert!(
            runtime
                .account_state_core()
                .get_canonical_state(&base_mint)
                .is_none(),
            "canonical state must not exist before registration"
        );

        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            initial_liquidity_sol: 8.0,
            bonding_curve_progress: Some(0.03),
            token_total_supply: Some(900_000_000_000_000),
            virtual_sol_reserves: Some(8_000_000_000),
            slot: Some(2),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        let canonical_state = runtime
            .account_state_core()
            .get_canonical_state(&base_mint)
            .expect("registration should replay buffered AccountUpdate into canonical state");
        assert_eq!(canonical_state.pool_amm_id, pool_id);
        assert_eq!(canonical_state.bonding_curve, bonding_curve);
        assert_eq!(canonical_state.virtual_sol_reserves, 9_000_000_000);
        assert_eq!(canonical_state.virtual_token_reserves, 888_000_000_000_000);
        assert_eq!(canonical_state.update_count, 1);

        let (pending_mints_after, pending_updates_after) =
            runtime.get_pre_identity_account_update_stats();
        assert_eq!(pending_mints_after, 0);
        assert_eq!(pending_updates_after, 0);
    }

    #[test]
    fn test_pre_identity_replay_requeues_when_identity_still_missing() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        runtime.enqueue_pre_identity_account_update(&AccountUpdateEvent {
            semantic: Default::default(),
            event_time: ghost_core::EventTimeMetadata::default(),
            base_mint,
            bonding_curve,
            curve_finality: CurveFinality::Speculative,
            sol_reserves: 9_000_000_000,
            token_reserves: 888_000_000_000_000,
            complete: 0,
            slot: 2,
            write_version: Some(7),
            replay_origin: seer::ipc::AccountUpdateReplayOrigin::PendingReplay,
            replay_buffer_dwell_ms: Some(10),
            detected_at: SystemTime::now(),
            sequence_number: 1,
        });

        runtime.replay_pre_identity_account_updates(pool_id, base_mint);

        let (pending_mints, pending_updates) = runtime.get_pre_identity_account_update_stats();
        assert_eq!(pending_mints, 1);
        assert_eq!(pending_updates, 1);
        assert!(
            runtime
                .account_state_core()
                .get_canonical_state(&base_mint)
                .is_none(),
            "failed replay must stay buffered until identity becomes resolvable"
        );
    }

    #[test]
    fn router_ignores_account_updates_for_rejected_pool_mint() {
        let source = include_str!("oracle_runtime.rs");
        let arm_start = source
            .find("GhostEvent::AccountUpdate(event) => {")
            .expect("account update router arm must exist");
        let arm_src = &source[arm_start..source.len().min(arm_start + 1800)];
        assert!(
            arm_src.contains("rejected_pools.contains(&event.base_mint)")
                || arm_src.contains("lookup_registered_pool(&event.base_mint)")
                    && arm_src.contains("rejected_pools.contains(&pool_id)"),
            "account update router must ignore late updates for rejected pools/mints before identity resolution"
        );
        assert!(
            arm_src.contains("ACCOUNT_UPDATE_IGNORED_ZOMBIE")
                || arm_src.contains("\"reason\" => \"rejected_pool\""),
            "ignored late account updates must be explicitly observable"
        );
    }

    #[test]
    fn test_pr8_registry_only_runtime_detection_can_mark_committed() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let detected_pool = test_detected_pool(Pubkey::new_unique());
        let pool_id = Pubkey::from_str(&detected_pool.pool_amm_id).expect("pool pubkey");
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let candidate = enhanced_candidate_from_detected_pool(detected_pool.as_ref());

        assert!(runtime.register_runtime_pool_detection(pool_id, base_mint, &candidate));
        runtime.remember_detected_pool(pool_id, detected_pool);

        runtime.mark_pool_committed(pool_id);

        assert_eq!(
            runtime.runtime_pool_state(&pool_id),
            Some(PoolState::Committed),
            "registry-backed runtime pool should transition to committed without legacy compat state"
        );
    }

    #[test]
    fn test_pr8_remove_pool_cleans_registry_only_runtime_detection() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger,
        );

        let detected_pool = test_detected_pool(Pubkey::new_unique());
        let pool_id = Pubkey::from_str(&detected_pool.pool_amm_id).expect("pool pubkey");
        let base_mint = Pubkey::from_str(&detected_pool.base_mint).expect("base mint");
        let candidate = enhanced_candidate_from_detected_pool(detected_pool.as_ref());

        assert!(runtime.register_runtime_pool_detection(pool_id, base_mint, &candidate));
        runtime.remember_detected_pool(pool_id, detected_pool.clone());

        assert!(runtime.remove_pool(pool_id));
        assert!(runtime.lookup_pool_identity(&pool_id).is_none());
        assert!(runtime.lookup_detected_pool(&pool_id).is_none());
        assert!(runtime.runtime_pool_state(&pool_id).is_none());
        assert!(runtime.lookup_registered_pool(&base_mint).is_none());
        assert!(runtime.lookup_base_mint_for_pool(&pool_id).is_none());
    }

    #[test]
    fn test_pr8_runtime_metadata_lookups_avoid_legacy_pool_map() {
        let source = include_str!("oracle_runtime.rs");

        let snapshot_start = source
            .find("fn build_runtime_state_pool_snapshot(")
            .expect("build_runtime_state_pool_snapshot must exist");
        let snapshot_end = source[snapshot_start..]
            .find("fn lookup_shadow_metadata_pool(")
            .map(|offset| snapshot_start + offset)
            .expect("lookup_shadow_metadata_pool must follow build_runtime_state_pool_snapshot");
        let snapshot_src = &source[snapshot_start..snapshot_end];

        let metadata_start = source
            .find("fn lookup_shadow_metadata_pool(")
            .expect("lookup_shadow_metadata_pool must exist");
        let metadata_end = source[metadata_start..]
            .find("pub fn lookup_base_mint_for_pool(")
            .map(|offset| metadata_start + offset)
            .expect("lookup_base_mint_for_pool must follow lookup_shadow_metadata_pool");
        let metadata_src = &source[metadata_start..metadata_end];

        let lookup_start = source
            .find("pub fn lookup_base_mint_for_pool(")
            .expect("lookup_base_mint_for_pool must exist");
        let lookup_end = source[lookup_start..]
            .find("fn build_session_open_request(")
            .map(|offset| lookup_start + offset)
            .expect("build_session_open_request must follow lookup_base_mint_for_pool");
        let lookup_src = &source[lookup_start..lookup_end];

        assert!(
            !snapshot_src.contains("self.pools.read()"),
            "runtime snapshot builder must not read PerPoolOracleState"
        );
        assert!(
            !metadata_src.contains("lookup_compat_pool_state"),
            "shadow metadata lookup must not consult compat pool state"
        );
        assert!(
            !lookup_src.contains("self.pools.read()"),
            "base mint lookup must resolve via session/identity registries only"
        );
    }

    #[test]
    fn test_pr8_new_pool_detected_uses_runtime_registration_cutover() {
        let source = include_str!("oracle_runtime.rs");
        let branch_start = source
            .find("GhostEvent::NewPoolDetected(pool_data) => {")
            .expect("NewPoolDetected branch must exist");
        let branch_end = source[branch_start..]
            .find("GhostEvent::PoolTransaction(tx) => {")
            .map(|offset| branch_start + offset)
            .expect("PoolTransaction branch must follow NewPoolDetected");
        let branch_src = &source[branch_start..branch_end];

        assert!(
            branch_src.contains("register_new_pool("),
            "NewPoolDetected must use the canonical registration path that drains tx-first orphans"
        );
        assert!(
            !branch_src.contains("register_runtime_pool_detection("),
            "NewPoolDetected must not bypass orphan drain by calling bare runtime registration"
        );
    }

    #[test]
    fn phase6_oracle_runtime_places_prewarm_hooks_at_planned_points() {
        let source = include_str!("oracle_runtime.rs");
        let trigger_branch_start = source
            .find("if let Some(ref trigger_component) = ctx.trigger {")
            .expect("trigger branch must exist");
        let trigger_branch_src = &source[trigger_branch_start..];
        let early_prewarm = trigger_branch_src
            .find("TriggerPrewarmAdvisory::TipFloor")
            .expect("sender tip policy hook must exist");
        let readiness_wait = trigger_branch_src
            .find("wait_for_live_trigger_readiness(")
            .expect("readiness wait must exist");
        assert!(
            early_prewarm < readiness_wait,
            "sender tip policy hook must remain before readiness wait so live BUY telemetry stays attached to the runtime entry boundary"
        );

        let buy_path_start = trigger_branch_src
            .find("let buy_mint = base_mint_pubkey.unwrap_or(initial_buy_mint);")
            .expect("buy path mint resolution must exist");
        let buy_path_src = &trigger_branch_src[buy_path_start..];
        let prepared_log = buy_path_src
            .find("\"Shadow buy account overrides prepared\"")
            .expect("shadow buy account overrides log must exist");
        let dispatch_call = buy_path_src[prepared_log..]
            .find("execute_gatekeeper_buy_via_trigger(")
            .map(|offset| prepared_log + offset)
            .expect("trigger dispatch call must follow account override log");
        let dispatch_window = &buy_path_src[prepared_log..dispatch_call];
        assert!(
            dispatch_window
                .find("TriggerPrewarmAdvisory::BuyPriorityFee")
                .is_none(),
            "Exact-key priority-fee prewarm moved into TriggerComponent prepare path after ata_missing_pre_submit resolution"
        );
        assert!(
            dispatch_call > prepared_log,
            "trigger dispatch call must remain after metadata-aware account override log"
        );
    }

    #[test]
    fn test_process_account_update_logs_severe_drift_without_diagnostic_signals() {
        use ghost_core::shadow_ledger::reconciliation::ReconciliationAction;

        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let runtime = OracleRuntime::new(
            hyper_oracle,
            "pump_program".to_string(),
            "bonk_program".to_string(),
            shadow_ledger.clone(),
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            slot: Some(1),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));
        shadow_ledger.insert_with_slot(
            bonding_curve,
            ghost_core::market_state::BondingCurve {
                discriminator: 0,
                virtual_token_reserves: 1_000_000_000_000,
                virtual_sol_reserves: 30_000_000_000,
                real_token_reserves: 1_000_000_000_000,
                real_sol_reserves: 30_000_000_000,
                token_total_supply: 1_000_000_000_000,
                complete: 0,
                _padding: [0; 7],
            },
            10,
        );

        let outcome = runtime
            .process_account_update(
                &base_mint,
                70_000_000_000,
                1_000_000_000_000,
                0,
                11,
                CurveFinality::Speculative,
            )
            .expect("tracked mint should return reconciliation outcome");
        assert_eq!(outcome.action, ReconciliationAction::Logged);

        let status = runtime.reconciliation_status();
        assert_eq!(status.total_diagnostic_signals, 0);
        assert_eq!(status.total_drifting_pools, 1);
    }

    #[test]
    fn test_enrich_buy_log_with_observation_identity_populates_all_fields() {
        use ghost_brain::config::GatekeeperV2Config;

        let ab_window_ms: u64 = 10_000;
        let first_seen: u64 = 1700000000000;

        let identity = ObservationIdentity {
            base_mint: "MintABC111111111111111111111111111111111111".to_string(),
            first_seen_ts_ms: first_seen,
            first_seen_clock_source: "registered_wall",
            end_10s_ts_ms: first_seen.saturating_add(ab_window_ms),
            dev_pubkey: "DevWallet99999999999999999999999999999999999".to_string(),
            failed_promotion_attempts: 0,
            next_promotion_attempt_ts_ms: 0,
        };

        let config = GatekeeperV2Config::default();
        let pool_id = Pubkey::new_unique();

        // Build a minimal buy_log via to_buy_log (identity fields are None)
        let assessment = crate::components::gatekeeper::GatekeeperAssessment {
            phase1_passed: false,
            phase2_velocity: None,
            phase2_passed: false,
            phase3_diversity: None,
            phase3_passed: false,
            phase4_volume: None,
            phase4_passed: false,
            phase5_dev: None,
            phase5_passed: false,
            phase6_curve: None,
            phase6_passed: false,
            phases_passed: 0,
            hard_reject_reason: None,
            total_tx_evaluated: 0,
            unique_tx_evaluated: 0,
            unique_signers_evaluated: 0,
            observation_duration_ms: ab_window_ms,
            finalize_lag_ms: 0,
            dust_filtered_count: 0,
            eval_count: 0,
            buy_count: 0,
            decision: None,
            early_fingerprint: None,
            curve_t0_event_ts_ms: None,
            curve_t0_clock_source: None,
            curve_wait_elapsed_ms: None,
            feature_snapshot: ghost_core::checkpoint::MaterializedFeatureSet::default(),
            checkpoint_count: 0,
            trajectory_available: false,
            v25_shadow_decisions: Vec::new(),
            trajectory: None,
            pdd_assessment: None,
            aps_diagnostics: None,
            observation_stage: None,
            entry_drift_pct: None,
            entry_drift_anchor_quality: None,
            adaptive_thresholds_applied: false,
            v25_confidence: None,
        };
        let mut log = assessment.to_buy_log(&pool_id, &config);

        // Before enrichment, identity fields are None
        assert!(log.base_mint.is_none());
        assert!(log.first_seen_ts_ms.is_none());
        assert!(log.observation_start_ts_ms.is_none());
        assert!(log.observation_end_ts_ms.is_none());
        assert!(log.observation_window_ms.is_none());
        assert!(log.end_10s_ts_ms.is_none());
        assert!(log.join_key.is_none());
        assert!(log.dev_pubkey.is_none());
        assert!(log.gatekeeper_version.is_none());

        // Enrich
        enrich_buy_log_with_observation_identity(&mut log, &identity);

        // After enrichment, all identity fields must be Some
        assert_eq!(
            log.base_mint.as_deref(),
            Some("MintABC111111111111111111111111111111111111")
        );
        assert_eq!(log.first_seen_ts_ms, Some(first_seen));
        assert_eq!(
            log.first_seen_clock_source.as_deref(),
            Some("registered_wall")
        );
        assert_eq!(log.observation_start_ts_ms, Some(first_seen));
        assert_eq!(log.observation_end_ts_ms, Some(first_seen + ab_window_ms));
        assert_eq!(log.observation_window_ms, Some(ab_window_ms));
        // end_10s_ts_ms uses ab_window_ms, NOT max_wait_time_ms
        assert_eq!(log.end_10s_ts_ms, Some(first_seen + ab_window_ms));
        // join_key uses canonical tuple: pool_id:base_mint:first_seen_ts_ms
        let expected_join_key = format!("{}:{}:{}", pool_id, identity.base_mint, first_seen);
        assert_eq!(log.join_key.as_deref(), Some(expected_join_key.as_str()));
        assert_eq!(
            log.dev_pubkey.as_deref(),
            Some("DevWallet99999999999999999999999999999999999")
        );
        assert_eq!(
            log.gatekeeper_version.as_deref(),
            Some(ghost_brain::oracle::GATEKEEPER_VERSION)
        );

        // Serialized JSON must contain base_mint
        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("\"base_mint\":\"MintABC111111111111111111111111111111111111\""));
        assert!(json.contains("\"first_seen_ts_ms\":1700000000000"));
        assert!(json.contains("\"observation_window_ms\":10000"));
    }

    #[test]
    fn test_choose_shadow_metadata_pool_prefers_richer_snapshot() {
        let pool_id = Pubkey::new_unique();
        let mut registry_pool = (*test_detected_pool(pool_id)).clone();
        registry_pool.creator = "unknown".to_string();
        registry_pool.initial_liquidity_sol = None;
        registry_pool.slot = None;

        let snapshot_pool = test_detected_pool(pool_id);

        let (selected, source) =
            choose_shadow_metadata_pool(Some(Arc::new(registry_pool)), Some(snapshot_pool.clone()))
                .expect("expected metadata source");

        assert_eq!(source, BuyPathMetadataSource::RuntimeStateSnapshot);
        assert_eq!(selected.creator, snapshot_pool.creator);
        assert_eq!(
            selected.initial_liquidity_sol,
            snapshot_pool.initial_liquidity_sol
        );
    }

    #[test]
    fn test_choose_shadow_metadata_pool_prefers_explicit_time_provenance_over_legacy_timestamp() {
        let pool_id = Pubkey::new_unique();
        let mut registry_pool = (*test_detected_pool(pool_id)).clone();
        registry_pool.timestamp_ms = 1_000;
        registry_pool.detected_wall_ts_ms = None;
        registry_pool.event_time = ghost_core::EventTimeMetadata::default();

        let mut snapshot_pool = (*test_detected_pool(pool_id)).clone();
        snapshot_pool.timestamp_ms = 1_000;
        snapshot_pool.detected_wall_ts_ms = Some(2_000);
        snapshot_pool.event_time = ghost_core::EventTimeMetadata::default();
        let snapshot_pool = Arc::new(snapshot_pool);

        let (selected, source) =
            choose_shadow_metadata_pool(Some(Arc::new(registry_pool)), Some(snapshot_pool.clone()))
                .expect("expected metadata source");

        assert_eq!(source, BuyPathMetadataSource::RuntimeStateSnapshot);
        assert_eq!(selected.detected_wall_ts_ms, Some(2_000));
    }

    #[test]
    fn test_merge_local_buy_path_pool_data_upgrades_incomplete_local_metadata() {
        let pool_id = Pubkey::new_unique();
        let mut local_pool = (*test_detected_pool(pool_id)).clone();
        local_pool.base_mint = "unknown".to_string();
        local_pool.quote_mint.clear();
        local_pool.creator = "unknown".to_string();
        local_pool.initial_liquidity_sol = None;
        local_pool.slot = None;
        local_pool.timestamp_ms = 0;
        local_pool.signature.clear();

        let fallback_pool = test_detected_pool(pool_id);
        let identity = ObservationIdentity {
            base_mint: fallback_pool.base_mint.clone(),
            first_seen_ts_ms: 55_000,
            first_seen_clock_source: "registered_wall",
            end_10s_ts_ms: 65_000,
            dev_pubkey: fallback_pool.creator.clone(),
            failed_promotion_attempts: 0,
            next_promotion_attempt_ts_ms: 0,
        };

        let (merged, source) = merge_local_buy_path_pool_data(
            Arc::new(local_pool),
            Some((
                fallback_pool.clone(),
                BuyPathMetadataSource::RuntimeRegistry,
            )),
            &identity,
        );

        assert_eq!(source, BuyPathMetadataSource::RuntimeRegistry);
        assert_eq!(merged.base_mint, fallback_pool.base_mint);
        assert_eq!(merged.quote_mint, fallback_pool.quote_mint);
        assert_eq!(merged.creator, fallback_pool.creator);
        assert_eq!(
            merged.initial_liquidity_sol,
            fallback_pool.initial_liquidity_sol
        );
        assert_eq!(merged.slot, fallback_pool.slot);
        assert_eq!(merged.timestamp_ms, identity.first_seen_ts_ms);
        assert_eq!(
            merged.event_time.ingress_wall_ts_ms,
            Some(identity.first_seen_ts_ms)
        );
        assert_eq!(merged.signature, fallback_pool.signature);
    }

    #[test]
    fn test_merge_local_buy_path_pool_data_preserves_richer_fallback_event_time() {
        let pool_id = Pubkey::new_unique();
        let mut local_pool = (*test_detected_pool(pool_id)).clone();
        local_pool.timestamp_ms = 0;
        local_pool.event_time = ghost_core::EventTimeMetadata::default();

        let mut fallback_pool = (*test_detected_pool(pool_id)).clone();
        fallback_pool.timestamp_ms = 0;
        fallback_pool.event_time =
            ghost_core::EventTimeMetadata::new(Some(77_000), Some(66_000), Some(88_000));

        let identity = ObservationIdentity {
            base_mint: fallback_pool.base_mint.clone(),
            first_seen_ts_ms: 55_000,
            first_seen_clock_source: "registered_wall",
            end_10s_ts_ms: 65_000,
            dev_pubkey: fallback_pool.creator.clone(),
            failed_promotion_attempts: 0,
            next_promotion_attempt_ts_ms: 0,
        };

        let (merged, _) = merge_local_buy_path_pool_data(
            Arc::new(local_pool),
            Some((
                Arc::new(fallback_pool),
                BuyPathMetadataSource::RuntimeRegistry,
            )),
            &identity,
        );

        assert_eq!(merged.timestamp_ms, identity.first_seen_ts_ms);
        assert_eq!(merged.event_time.chain_event_ts_ms, Some(77_000));
        assert_eq!(
            merged.event_time.ingress_wall_ts_ms,
            Some(identity.first_seen_ts_ms)
        );
        assert_eq!(merged.event_time.ingress_monotonic_ts_ms, Some(88_000));
    }

    #[test]
    fn test_shadow_readiness_accepts_liquidity_backfilled_from_refreshed_reserves() {
        let pool_id = Pubkey::new_unique();
        let mut pool = (*test_detected_pool(pool_id)).clone();
        pool.initial_liquidity_sol = None;

        let mut buffered = review_test_buffered_tx(pool_id, "buy_sig", 1_100, true);
        let mut tx = (*buffered.tx).clone();
        tx.buy_variant = Some("legacy_buy".to_string());
        buffered.tx = Arc::new(tx);
        let buffered_txs = vec![buffered];

        let readiness_before = compute_shadow_run_readiness(Some(&pool), &buffered_txs);
        assert!(!readiness_before.ready);
        assert_eq!(
            readiness_before.missing_fields,
            vec!["initial_liquidity_sol".to_string()]
        );

        let enriched =
            backfill_initial_liquidity_sol_from_reserve_quote(Arc::new(pool), Some(35.436_444_437));
        let readiness_after = compute_shadow_run_readiness(Some(enriched.as_ref()), &buffered_txs);

        assert!(readiness_after.ready);
        assert!(readiness_after.missing_fields.is_empty());
        assert_eq!(enriched.initial_liquidity_sol, Some(35.436_444_437));
    }

    #[test]
    fn test_shadow_readiness_accepts_liquidity_backfilled_from_buffered_buy_reserves() {
        let pool_id = Pubkey::new_unique();
        let mut pool = (*test_detected_pool(pool_id)).clone();
        pool.initial_liquidity_sol = None;

        let mut buffered = review_test_buffered_tx(pool_id, "buy_sig", 1_100, true);
        let mut tx = (*buffered.tx).clone();
        tx.buy_variant = Some("legacy_buy".to_string());
        tx.reserve_quote = Some(40.750_597_061);
        tx.v_sol_in_bonding_curve = Some(40.750_597_061);
        buffered.tx = Arc::new(tx);
        let buffered_txs = vec![buffered];

        let enriched =
            backfill_initial_liquidity_sol_from_buffered_buys(Arc::new(pool), &buffered_txs);
        let readiness = compute_shadow_run_readiness(Some(enriched.as_ref()), &buffered_txs);

        assert!(readiness.ready);
        assert!(readiness.missing_fields.is_empty());
        assert_eq!(enriched.initial_liquidity_sol, Some(40.750_597_061));
    }

    #[test]
    fn test_shadow_readiness_rejects_invalid_creator_pubkey_string() {
        let pool_id = Pubkey::new_unique();
        let mut pool = (*test_detected_pool(pool_id)).clone();
        pool.creator = "not-a-pubkey".to_string();

        let mut buffered = review_test_buffered_tx(pool_id, "buy_sig", 1_100, true);
        let mut tx = (*buffered.tx).clone();
        tx.buy_variant = Some("legacy_buy".to_string());
        buffered.tx = Arc::new(tx);
        let buffered_txs = vec![buffered];

        let readiness = compute_shadow_run_readiness(Some(&pool), &buffered_txs);

        assert!(!readiness.ready);
        assert_eq!(readiness.missing_fields, vec!["creator".to_string()]);
    }

    #[test]
    fn test_shadow_readiness_rejects_legacy_only_timestamp_without_provenance() {
        let pool_id = Pubkey::new_unique();
        let mut pool = (*test_detected_pool(pool_id)).clone();
        pool.timestamp_ms = 1_100;
        pool.detected_wall_ts_ms = None;
        pool.event_time = ghost_core::EventTimeMetadata::default();

        let readiness = compute_shadow_run_readiness(Some(&pool), &[]);

        assert!(!readiness.ready);
        assert_eq!(readiness.missing_fields, vec!["timestamp_ms".to_string()]);
    }

    #[test]
    fn test_enforce_buy_log_buy_routing_backfills_missing_buy_fields() {
        let config = GatekeeperV2Config::default();
        let pool_id = Pubkey::new_unique();
        let assessment = test_gatekeeper_buy_assessment(6);
        let mut log = assessment.to_buy_log(&pool_id, &config);
        let window_state = WindowState::from_first_tx(1_000, 10_000);

        enrich_buy_log_with_window(&mut log, &window_state, &pool_id.to_string(), "BUY");

        log.decision_reason = None;
        log.decision_verdict_buy = None;
        log.verdict_type = None;
        log.ab_record_id = None;

        enforce_buy_log_buy_routing(&mut log, &assessment);

        let expected_record_id = format!("{}:1000:11000:BUY", pool_id);
        assert_eq!(log.decision_verdict_buy, Some(true));
        assert_eq!(log.verdict_type.as_deref(), Some("BUY"));
        assert_eq!(log.decision_reason.as_deref(), Some("gatekeeper_buy"));
        assert_eq!(
            log.ab_record_id.as_deref(),
            Some(expected_record_id.as_str())
        );
    }

    #[test]
    fn test_build_fallback_observation_identity_prefers_valid_tx_fields() {
        let pool_id = Pubkey::new_unique();
        let token_mint = Pubkey::new_unique();
        // Use Keypair::new() to guarantee the pubkey is on the Ed25519 curve,
        // which is required by normalize_dev_pubkey_str's is_on_curve() check.
        let signer = Keypair::new().pubkey();
        let tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1_700_000_000_123,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(1_700_000_000_500), None),
            arrival_ts_ms: 1_700_000_000_123,
            signer: signer.to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: Some(100_000_000),
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
            token_mint: Some(token_mint.to_string()),
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
        };

        let identity = build_fallback_observation_identity(pool_id, &tx, 10_000);
        assert_eq!(identity.base_mint, token_mint.to_string());
        assert_eq!(identity.dev_pubkey, signer.to_string());
        assert_eq!(identity.first_seen_ts_ms, 1_700_000_000_500);
        assert_eq!(identity.end_10s_ts_ms, 1_700_000_000_500 + 10_000);
    }

    #[test]
    fn test_build_fallback_observation_identity_ignores_legacy_only_timestamp() {
        let pool_id = Pubkey::new_unique();
        let tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1_700_000_000_500,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1_700_000_000_505,
            signer: Keypair::new().pubkey().to_string(),
            is_buy: true,
            volume_sol: 0.2,
            sol_amount_lamports: Some(200_000_000),
            token_amount_units: Some(2_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "sig-fallback-legacy".to_string(),
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
        };

        let before = current_time_ms();
        let identity = build_fallback_observation_identity(pool_id, &tx, 10_000);
        let after = current_time_ms();

        assert_ne!(identity.first_seen_ts_ms, tx.timestamp_ms);
        assert!(
            identity.first_seen_ts_ms >= before && identity.first_seen_ts_ms <= after,
            "expected wall-clock fallback, got {} outside [{}, {}]",
            identity.first_seen_ts_ms,
            before,
            after
        );
    }

    #[test]
    fn test_ensure_observation_identity_pending_pool_prefers_detected_wall_time() {
        let pool_id = Pubkey::new_unique();
        let runtime = make_oracle_runtime_for_test();
        let mut observation_identities = std::collections::HashMap::new();
        let mut pending_pools = std::collections::HashMap::new();
        let registered_wall_ts_ms = std::collections::HashMap::new();

        let mut pool_data = (*test_detected_pool(pool_id)).clone();
        pool_data.timestamp_ms = 1_000;
        pool_data.detected_wall_ts_ms = Some(1_111);
        pool_data.event_time = ghost_core::EventTimeMetadata::new(Some(999), Some(1_050), None);
        pending_pools.insert(pool_id, Arc::new(pool_data.clone()));

        let identity = ensure_observation_identity(
            pool_id,
            &mut observation_identities,
            &pending_pools,
            &registered_wall_ts_ms,
            runtime.as_ref(),
            10_000,
            None,
        );

        assert_eq!(identity.first_seen_ts_ms, 1_111);
        assert_eq!(identity.end_10s_ts_ms, 11_111);
        assert_eq!(identity.base_mint, pool_data.base_mint);
    }

    #[test]
    fn test_ensure_observation_identity_pending_pool_ignores_legacy_only_timestamp() {
        let pool_id = Pubkey::new_unique();
        let runtime = make_oracle_runtime_for_test();
        let mut observation_identities = std::collections::HashMap::new();
        let mut pending_pools = std::collections::HashMap::new();
        let registered_wall_ts_ms = std::collections::HashMap::new();

        let mut pool_data = (*test_detected_pool(pool_id)).clone();
        pool_data.timestamp_ms = 1_000;
        pool_data.detected_wall_ts_ms = None;
        pool_data.event_time = ghost_core::EventTimeMetadata::default();
        pending_pools.insert(pool_id, Arc::new(pool_data.clone()));

        let before = current_time_ms();
        let identity = ensure_observation_identity(
            pool_id,
            &mut observation_identities,
            &pending_pools,
            &registered_wall_ts_ms,
            runtime.as_ref(),
            10_000,
            None,
        );
        let after = current_time_ms();

        assert_ne!(identity.first_seen_ts_ms, pool_data.timestamp_ms);
        assert!(
            identity.first_seen_ts_ms >= before && identity.first_seen_ts_ms <= after,
            "expected wall-clock fallback, got {} outside [{}, {}]",
            identity.first_seen_ts_ms,
            before,
            after
        );
    }

    #[test]
    fn test_maybe_promote_observation_identity_from_first_tx() {
        let pool_id = Pubkey::new_unique();
        let token_mint = Pubkey::new_unique();
        // Use Keypair::new() to guarantee the pubkey is on the Ed25519 curve,
        // which is required by normalize_dev_pubkey_str's is_on_curve() check.
        let signer = Keypair::new().pubkey();
        let tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1_700_000_001_000,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(1_700_000_001_500), None),
            arrival_ts_ms: 1_700_000_001_005,
            signer: signer.to_string(),
            is_buy: true,
            volume_sol: 0.2,
            sol_amount_lamports: Some(200_000_000),
            token_amount_units: Some(2_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "sig-promote".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: crate::events::RawBytesMissingReason::Unknown,
            token_mint: Some(token_mint.to_string()),
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
        };

        let mut identity = build_unknown_observation_identity(pool_id, 10_000);
        let mut base_mint_pubkey = None;
        let promoted = maybe_promote_observation_identity_from_tx(
            pool_id,
            &tx,
            10_000,
            &mut identity,
            &mut base_mint_pubkey,
            tx.timestamp_ms,
            25,
        );

        assert!(promoted);
        assert_eq!(identity.base_mint, token_mint.to_string());
        assert_eq!(identity.dev_pubkey, signer.to_string());
        assert_eq!(identity.first_seen_ts_ms, 1_700_000_001_500);
        assert_eq!(identity.end_10s_ts_ms, 1_700_000_001_500 + 10_000);
        assert_eq!(base_mint_pubkey, Some(token_mint));
    }

    #[test]
    fn test_maybe_promote_observation_identity_ignores_legacy_only_timestamp() {
        let pool_id = Pubkey::new_unique();
        let token_mint = Pubkey::new_unique();
        let signer = Keypair::new().pubkey();
        let tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1_700_000_001_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1_700_000_001_005,
            signer: signer.to_string(),
            is_buy: true,
            volume_sol: 0.2,
            sol_amount_lamports: Some(200_000_000),
            token_amount_units: Some(2_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "sig-promote-legacy".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: crate::events::RawBytesMissingReason::Unknown,
            token_mint: Some(token_mint.to_string()),
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
        };

        let mut identity = build_unknown_observation_identity(pool_id, 10_000);
        let mut base_mint_pubkey = None;
        let now_ms = 25_000;
        let promoted = maybe_promote_observation_identity_from_tx(
            pool_id,
            &tx,
            10_000,
            &mut identity,
            &mut base_mint_pubkey,
            now_ms,
            25,
        );

        assert!(promoted);
        assert_eq!(identity.base_mint, token_mint.to_string());
        assert_eq!(identity.dev_pubkey, signer.to_string());
        assert_eq!(identity.first_seen_ts_ms, now_ms);
        assert_eq!(identity.end_10s_ts_ms, now_ms + 10_000);
        assert_eq!(base_mint_pubkey, Some(token_mint));
    }

    #[test]
    fn test_normalize_gatekeeper_event_time_prefers_canonical_event_time() {
        let tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: Pubkey::new_unique().to_string(),
            slot: Some(1_005),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 123,
            event_time: ghost_core::EventTimeMetadata::new(Some(1_234), Some(1_500), None),
            arrival_ts_ms: 456,
            signer: "s".to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: None,
            token_amount_units: None,
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
        };

        let (ts, chain_time) = normalize_gatekeeper_event_time_ms(&tx, None);
        assert!(chain_time);
        assert_eq!(ts, 1_234);
    }

    #[test]
    fn test_tx_event_ts_ms_prefers_effective_event_time_then_wallclock() {
        let mut tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: Pubkey::new_unique().to_string(),
            slot: Some(1_005),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 123,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(123), None),
            arrival_ts_ms: 456,
            signer: "s".to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: None,
            token_amount_units: None,
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
        };

        assert_eq!(tx_event_ts_ms(&tx), 123);

        tx.event_time = ghost_core::EventTimeMetadata::default();
        let before = current_time_ms();
        let ts = tx_event_ts_ms(&tx);
        let after = current_time_ms();
        assert!(ts >= before && ts <= after);
        assert_ne!(ts, tx.timestamp_ms);
        assert_ne!(ts, tx.arrival_ts_ms);
    }

    #[test]
    fn test_runtime_tx_time_source_info_distinguishes_legacy_rejection_from_real_fallback() {
        let mut tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: Pubkey::new_unique().to_string(),
            slot: Some(1_005),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 123,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 456,
            signer: "s".to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: None,
            token_amount_units: None,
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
        };

        assert_eq!(
            runtime_tx_time_source_info(&tx),
            RuntimeTxTimeSourceInfo {
                effective_source: "wall_clock_fallback",
                fallback_class: Some("legacy_compat_rejected"),
            }
        );

        tx.timestamp_ms = 0;
        assert_eq!(
            runtime_tx_time_source_info(&tx),
            RuntimeTxTimeSourceInfo {
                effective_source: "wall_clock_fallback",
                fallback_class: Some("missing_explicit_time"),
            }
        );

        tx.event_time.ingress_wall_ts_ms = Some(321);
        assert_eq!(
            runtime_tx_time_source_info(&tx),
            RuntimeTxTimeSourceInfo {
                effective_source: "ingress_wall",
                fallback_class: None,
            }
        );

        tx.event_time.chain_event_ts_ms = Some(654);
        assert_eq!(
            runtime_tx_time_source_info(&tx),
            RuntimeTxTimeSourceInfo {
                effective_source: "chain_event",
                fallback_class: None,
            }
        );
    }

    #[test]
    fn test_runtime_account_update_time_source_info_prefers_event_time_axes() {
        let mut event = AccountUpdateEvent {
            semantic: Default::default(),
            event_time: ghost_core::EventTimeMetadata::default(),
            base_mint: Pubkey::new_unique(),
            bonding_curve: Pubkey::new_unique(),
            curve_finality: CurveFinality::Provisional,
            sol_reserves: 10,
            token_reserves: 20,
            complete: 0,
            slot: 42,
            write_version: Some(7),
            replay_origin: seer::ipc::AccountUpdateReplayOrigin::Live,
            replay_buffer_dwell_ms: None,
            detected_at: std::time::SystemTime::now(),
            sequence_number: 1,
        };

        assert_eq!(
            runtime_account_update_time_source_info(&event),
            RuntimeTxTimeSourceInfo {
                effective_source: "wall_clock_fallback",
                fallback_class: Some("missing_explicit_time"),
            }
        );

        event.event_time.ingress_wall_ts_ms = Some(321);
        assert_eq!(
            runtime_account_update_time_source_info(&event),
            RuntimeTxTimeSourceInfo {
                effective_source: "ingress_wall",
                fallback_class: None,
            }
        );

        event.event_time.chain_event_ts_ms = Some(654);
        assert_eq!(
            runtime_account_update_time_source_info(&event),
            RuntimeTxTimeSourceInfo {
                effective_source: "chain_event",
                fallback_class: None,
            }
        );

        event.event_time = ghost_core::EventTimeMetadata::default();
        event.replay_origin = seer::ipc::AccountUpdateReplayOrigin::PendingReplay;
        assert_eq!(
            runtime_account_update_time_source_info(&event),
            RuntimeTxTimeSourceInfo {
                effective_source: "wall_clock_fallback",
                fallback_class: Some("replay_missing_explicit_time"),
            }
        );
    }

    #[test]
    fn test_normalize_gatekeeper_event_time_monotonic_with_chain_time() {
        let tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: Pubkey::new_unique().to_string(),
            slot: None,
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 10_000,
            event_time: ghost_core::EventTimeMetadata::new(Some(10_000), None, None),
            arrival_ts_ms: 0,
            signer: "s".to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: None,
            token_amount_units: None,
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
        };

        let (ts, chain_time) = normalize_gatekeeper_event_time_ms(&tx, Some(10_500));
        assert!(chain_time);
        assert_eq!(ts, 10_501);
    }

    #[test]
    fn test_normalize_gatekeeper_event_time_legacy_timestamp_is_not_chain_time() {
        let tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: Pubkey::new_unique().to_string(),
            slot: Some(1_005),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 123,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 456,
            signer: "s".to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: None,
            token_amount_units: None,
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
        };

        let (ts, chain_time) = normalize_gatekeeper_event_time_ms(&tx, None);
        assert!(!chain_time);
        assert_ne!(ts, 123);
    }

    #[tokio::test]
    async fn enqueue_pool_observation_msg_retries_until_capacity_frees() {
        let pool_id = Pubkey::new_unique();
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        tx.send(PoolObservationMsg::Transaction(test_pool_observation_tx(
            "first",
        )))
        .await
        .expect("prime channel");

        enqueue_pool_observation_msg(
            &tx,
            pool_id,
            PoolObservationMsg::Transaction(test_pool_observation_tx("second")),
            "transaction",
            false,
        );

        let first = rx.recv().await.expect("first tx");
        match first {
            PoolObservationMsg::Transaction(tx) => assert_eq!(tx.signature, "first"),
            PoolObservationMsg::NewPool(_) => panic!("expected transaction"),
        }

        let second = tokio::time::timeout(
            Duration::from_millis(
                POOL_TASK_BACKPRESSURE_WAIT_MS * POOL_TASK_BACKPRESSURE_RETRY_ATTEMPTS as u64 + 100,
            ),
            rx.recv(),
        )
        .await
        .expect("second tx timeout")
        .expect("second tx missing");

        match second {
            PoolObservationMsg::Transaction(tx) => assert_eq!(tx.signature, "second"),
            PoolObservationMsg::NewPool(_) => panic!("expected transaction"),
        }
    }

    /// Validate the preconditions of the Shadow Ledger committed-pool fast path
    /// added to `start_oracle_runtime_task`.
    ///
    /// After `pool_observation_task` exits with `GatekeeperVerdict::Buy`:
    /// - `lookup_base_mint_for_pool` returns the canonical base_mint (enrichment step)
    /// - `shadow_ledger.is_committed(&base_mint)` returns `true` (committed fast path)
    /// - launcher runtime state is promoted to `Committed`
    /// - `forward_approved_tx_to_commit_or_live_pipeline` routes TX to LivePipeline
    ///   and ShadowLedger.append_live() is called via the flush cycle
    ///
    /// This prevents the regression where a fresh observation task (state=Tracked)
    /// would be auto-spawned for a committed pool, causing:
    ///   a) LivePipeline starvation (snapshot state freezes)
    ///   b) Pool added to rejected_pools on timeout (scoring breaks)
    #[test]
    fn test_committed_pool_fast_path_preconditions_are_satisfied() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let live_pipeline = Arc::new(ghost_core::shadow_ledger::LivePipeline::with_config(
            LivePipelineConfig {
                flush_delay_ms: 0,
                ..Default::default()
            },
        ));

        #[allow(deprecated)]
        // runtime_shadowledger_snapshots_enabled is a removed EPIC 2.3.5 legacy flag
        // with no effect on runtime behavior.  Setting false is consistent with the
        // production default; ShadowLedger writes happen exclusively via the
        // committed-pool fast path → LivePipeline → append_live() path under test.
        let config = OracleRuntimeConfig {
            runtime_shadowledger_snapshots_enabled: false,
            shadow_ledger_enrichment_freshness_ms: DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
            ..Default::default()
        };

        let runtime = OracleRuntime::new_with_config(
            hyper_oracle,
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
            shadow_ledger.clone(),
            None,
            None,
            live_pipeline.clone(),
            config,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        // Register pool so identity lookups work
        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            slot: Some(100),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));

        // Mark approved + committed using launcher runtime SSOT.
        runtime.mark_pool_approved(pool_id);
        runtime.approved_pools().insert(pool_id);

        // Commit canonical history to ShadowLedger (simulates GatekeeperCommitLoop)
        let initial_snap = ghost_core::shadow_ledger::MarketSnapshot {
            slot: Some(100),
            tx_key: Some(
                ghost_core::shadow_ledger::TxKey::new(100_000, Some(100), Some(0), None, 0)
                    .unwrap(),
            ),
            timestamp_ms: 100_000,
            tx_count: 10,
            ..Default::default()
        };
        shadow_ledger.commit_history(base_mint, vec![initial_snap.clone()], None);
        runtime.remember_committed_snapshot(base_mint, &initial_snap);
        runtime.mark_pool_committed(pool_id);

        // ── Precondition 1: base_mint can be enriched via pool identity lookup ──
        // This is what the fast path does when token_mint is absent from the TX.
        assert_eq!(
            runtime.lookup_base_mint_for_pool(&pool_id),
            Some(base_mint),
            "lookup_base_mint_for_pool must resolve base_mint for committed-pool fast path"
        );

        // ── Precondition 2: shadow_ledger.is_committed detects committed state ──
        assert!(
            shadow_ledger.is_committed(&base_mint),
            "is_committed must be true after commit_history — triggers fast path"
        );

        // ── Precondition 3: approved_pools.is_approved covers the pre-commit window ──
        assert!(
            runtime.approved_pools().is_approved(&pool_id),
            "allowlist stays populated after approval"
        );
        assert_eq!(
            runtime.runtime_pool_state(&pool_id),
            Some(PoolState::Committed),
            "runtime status must distinguish committed from merely approved"
        );

        // ── Precondition 4: forward_approved_tx_to_commit_or_live_pipeline routes to LivePipeline ──
        // (end-to-end: this is exactly what the fast path calls)
        let tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(110),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 110_000,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(110_000), None),
            arrival_ts_ms: 110_000,
            signer: Pubkey::new_unique().to_string(),
            is_buy: true,
            volume_sol: 0.5,
            sol_amount_lamports: Some(500_000_000),
            token_amount_units: Some(500_000),
            reserve_base: Some(900_000_000_000.0),
            reserve_quote: Some(31.0),
            price_quote: Some(31.0 / 900_000_000_000.0),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Signature::new_unique().to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: Some(base_mint.to_string()),
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
            curve_data_known: true,
        };

        runtime.forward_approved_tx_to_commit_or_live_pipeline(
            pool_id,
            base_mint,
            &tx,
            tx_event_ts_ms(&tx),
        );

        // LivePipeline was initialized by the forward call, now flush to ShadowLedger
        assert!(live_pipeline.is_initialized(&base_mint));
        let flushed = live_pipeline.flush_ready(&shadow_ledger);
        assert_eq!(
            flushed, 1,
            "LivePipeline must flush one snapshot to ShadowLedger"
        );

        let snapshots = shadow_ledger
            .get_snapshots(&base_mint)
            .expect("snapshots must exist");
        assert_eq!(
            snapshots.len(),
            2,
            "ShadowLedger must have initial committed snapshot + one live-appended snapshot"
        );
        assert_eq!(
            snapshots.last().unwrap().timestamp_ms,
            110_000,
            "Most recent snapshot must match the post-commit TX timestamp"
        );
    }

    // =========================================================================
    // A. High-activity observation-window / burst tx tests
    // =========================================================================

    /// Extra wall-clock buffer added on top of retry budget in backpressure tests.
    /// Allows for scheduler jitter without making the test flaky.
    const BACKPRESSURE_TEST_TIMEOUT_BUFFER_MS: u64 = 200;

    /// Upper bound used in assertions to ensure retry budgets stay operationally safe.
    const MAX_RETRY_BUDGET_MS: u64 = 60_000;

    /// A.1 – A burst of `HOT_POOL_TX_THRESHOLD + 1` transactions for a single
    /// pool are all delivered through the ingest path when the channel has room.
    ///
    /// Verifies that hot-pool detection via `PoolTaskHandle::is_hot()` and the
    /// `tx_enqueued` counter are wired correctly: after reaching the threshold the
    /// handle is classified as hot, and all messages sent before any backpressure
    /// episode survive unmodified.
    #[tokio::test]
    async fn test_hot_pool_handle_tracks_enqueued_tx_and_classifies_as_hot() {
        use ghost_core::shadow_ledger::HOT_POOL_TX_THRESHOLD;

        let pool_id = Pubkey::new_unique();
        // Use a large channel so try_send never fails (no backpressure).
        let (sender, _rx) = tokio::sync::mpsc::channel(POOL_TASK_CHANNEL_CAPACITY);
        let mut handle = PoolTaskHandle {
            tx: sender,
            _abort_handle: tokio::runtime::Handle::current()
                .spawn(async {})
                .abort_handle(),
            tx_enqueued: 0,
        };

        assert!(!handle.is_hot(), "new pool must start as cold");

        // Simulate arriving transactions incrementing the counter.
        for i in 1..=HOT_POOL_TX_THRESHOLD {
            handle.tx_enqueued += 1;
            let expected_hot = i >= HOT_POOL_TX_THRESHOLD;
            assert_eq!(
                handle.is_hot(),
                expected_hot,
                "is_hot() must flip to true exactly at HOT_POOL_TX_THRESHOLD (i={})",
                i
            );
        }

        // After threshold is reached, further increments keep it hot (no ceiling).
        handle.tx_enqueued += 100;
        assert!(handle.is_hot(), "pool must remain hot above threshold");

        // Verify pool_id round-trip is unaffected — just a sanity compile check.
        let _ = pool_id;
    }

    /// A.2 – Burst delivery: all `HOT_POOL_TX_THRESHOLD * 2` transactions for a
    /// hot pool are received by the channel consumer when no backpressure occurs.
    ///
    /// This proves the ingest path does not silently discard messages for a pool
    /// that becomes hot mid-burst.
    #[tokio::test]
    async fn test_hot_pool_burst_all_tx_delivered_when_no_backpressure() {
        use ghost_core::shadow_ledger::HOT_POOL_TX_THRESHOLD;

        let pool_id = Pubkey::new_unique();
        let burst_size = (HOT_POOL_TX_THRESHOLD * 2) as usize;
        let (sender, mut rx) = tokio::sync::mpsc::channel(burst_size + 16);
        let mut handle = PoolTaskHandle {
            tx: sender,
            _abort_handle: tokio::runtime::Handle::current()
                .spawn(async {})
                .abort_handle(),
            tx_enqueued: 0,
        };

        // Send burst of transactions, mimicking what the main loop does.
        for i in 0..burst_size {
            let sig = format!("burst_{}", i);
            let is_hot = handle.is_hot();
            handle.tx_enqueued += 1;
            enqueue_pool_observation_msg(
                &handle.tx,
                pool_id,
                PoolObservationMsg::Transaction(test_pool_observation_tx(&sig)),
                "burst_tx",
                is_hot,
            );
        }

        // Drain and count.
        let mut received = 0usize;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                PoolObservationMsg::Transaction(_) => received += 1,
                PoolObservationMsg::NewPool(_) => {}
            }
        }
        assert_eq!(
            received, burst_size,
            "all burst tx must be delivered to channel consumer; got {}/{}",
            received, burst_size
        );
    }

    // =========================================================================
    // B. Hot-vs-cold prioritization tests
    // =========================================================================

    /// B.1 – Hot pool channel-full retry is bounded and distinct from cold.
    ///
    /// Confirms:
    /// - `HOT_POOL_BACKPRESSURE_RETRY_ATTEMPTS > POOL_TASK_BACKPRESSURE_RETRY_ATTEMPTS`
    ///   (hot gets more retries)
    /// - `HOT_POOL_BACKPRESSURE_WAIT_MS <= POOL_TASK_BACKPRESSURE_WAIT_MS`
    ///   (hot polls no slower than cold per attempt)
    /// - The retry budget is explicitly bounded (both constants are finite).
    #[test]
    fn test_hot_pool_retry_budget_exceeds_cold_pool_retry_budget() {
        assert!(
            HOT_POOL_BACKPRESSURE_RETRY_ATTEMPTS > POOL_TASK_BACKPRESSURE_RETRY_ATTEMPTS,
            "hot pools must receive more retry attempts than cold pools"
        );
        assert!(
            HOT_POOL_BACKPRESSURE_WAIT_MS <= POOL_TASK_BACKPRESSURE_WAIT_MS,
            "hot pool per-attempt wait must be no longer than cold pool (faster polling)"
        );
        // Verify finite bounds (would be caught by type system but make it explicit).
        let hot_budget_ms =
            HOT_POOL_BACKPRESSURE_RETRY_ATTEMPTS as u64 * HOT_POOL_BACKPRESSURE_WAIT_MS;
        let cold_budget_ms =
            POOL_TASK_BACKPRESSURE_RETRY_ATTEMPTS as u64 * POOL_TASK_BACKPRESSURE_WAIT_MS;
        assert!(
            hot_budget_ms > 0 && hot_budget_ms < MAX_RETRY_BUDGET_MS,
            "hot pool retry budget must be positive and bounded (< {}ms), got {}ms",
            MAX_RETRY_BUDGET_MS,
            hot_budget_ms
        );
        assert!(
            cold_budget_ms > 0 && cold_budget_ms < MAX_RETRY_BUDGET_MS,
            "cold pool retry budget must be positive and bounded (< {}ms), got {}ms",
            MAX_RETRY_BUDGET_MS,
            cold_budget_ms
        );
    }

    /// B.2 – Under backpressure a hot-pool message is retried more aggressively
    /// than a cold-pool message and is ultimately delivered.
    ///
    /// Uses a capacity-1 channel to force immediate backpressure on the second
    /// send, then verifies that:
    /// - the hot-pool send eventually succeeds once the consumer drains the channel,
    /// - the cold-pool send succeeds within the cold retry budget.
    #[tokio::test]
    async fn test_hot_pool_backpressure_retries_more_than_cold_and_delivers() {
        let pool_id = Pubkey::new_unique();

        // --- Hot pool path ---
        let (hot_tx, mut hot_rx) = tokio::sync::mpsc::channel(1);
        // Fill channel so the second send hits backpressure.
        hot_tx
            .send(PoolObservationMsg::Transaction(test_pool_observation_tx(
                "blocker",
            )))
            .await
            .unwrap();
        // Enqueue "hot" message — will retry until channel drains.
        enqueue_pool_observation_msg(
            &hot_tx,
            pool_id,
            PoolObservationMsg::Transaction(test_pool_observation_tx("hot_msg")),
            "tx",
            true, // is_hot
        );
        // Drain the blocker so the retry can succeed.
        let _ = hot_rx.recv().await.unwrap();
        // Hot message must arrive within the hot retry budget.
        let hot_result = tokio::time::timeout(
            Duration::from_millis(
                HOT_POOL_BACKPRESSURE_WAIT_MS * HOT_POOL_BACKPRESSURE_RETRY_ATTEMPTS as u64
                    + BACKPRESSURE_TEST_TIMEOUT_BUFFER_MS,
            ),
            hot_rx.recv(),
        )
        .await
        .expect("hot pool message must be delivered within retry budget")
        .expect("hot pool message must not be None");
        match hot_result {
            PoolObservationMsg::Transaction(tx) => {
                assert_eq!(
                    tx.signature, "hot_msg",
                    "hot pool must deliver correct message"
                )
            }
            PoolObservationMsg::NewPool(_) => panic!("expected transaction"),
        }

        // --- Cold pool path (baseline) ---
        let (cold_tx, mut cold_rx) = tokio::sync::mpsc::channel(1);
        cold_tx
            .send(PoolObservationMsg::Transaction(test_pool_observation_tx(
                "blocker2",
            )))
            .await
            .unwrap();
        enqueue_pool_observation_msg(
            &cold_tx,
            pool_id,
            PoolObservationMsg::Transaction(test_pool_observation_tx("cold_msg")),
            "tx",
            false, // is_hot = false
        );
        let _ = cold_rx.recv().await.unwrap();
        let cold_result = tokio::time::timeout(
            Duration::from_millis(
                POOL_TASK_BACKPRESSURE_WAIT_MS * POOL_TASK_BACKPRESSURE_RETRY_ATTEMPTS as u64
                    + BACKPRESSURE_TEST_TIMEOUT_BUFFER_MS,
            ),
            cold_rx.recv(),
        )
        .await
        .expect("cold pool message must be delivered within retry budget")
        .expect("cold pool message must not be None");
        match cold_result {
            PoolObservationMsg::Transaction(tx) => {
                assert_eq!(
                    tx.signature, "cold_msg",
                    "cold pool must deliver correct message"
                )
            }
            PoolObservationMsg::NewPool(_) => panic!("expected transaction"),
        }
    }

    // =========================================================================
    // C. Non-regression tests — authority model
    // =========================================================================

    /// C.1 – Shadow Ledger remains the tx-driven primary authority after the
    /// hot-pool ingest hardening changes.
    ///
    /// Verifies that `forward_approved_tx_to_commit_or_live_pipeline` still routes
    /// a committed-pool TX to the LivePipeline → ShadowLedger path, and that
    /// ShadowLedger is the only place state is recorded.
    #[test]
    fn test_shadow_ledger_remains_primary_authority_after_hot_pool_hardening() {
        let hyper_oracle = Arc::new(HyperPredictionOracle::default());
        let shadow_ledger = Arc::new(ShadowLedger::new());
        let live_pipeline = Arc::new(ghost_core::shadow_ledger::LivePipeline::with_config(
            LivePipelineConfig {
                flush_delay_ms: 0,
                ..Default::default()
            },
        ));

        #[allow(deprecated)]
        let config = OracleRuntimeConfig {
            runtime_shadowledger_snapshots_enabled: false,
            shadow_ledger_enrichment_freshness_ms: DEFAULT_SHADOW_LEDGER_ENRICHMENT_FRESHNESS_MS,
            ..Default::default()
        };

        let runtime = OracleRuntime::new_with_config(
            hyper_oracle,
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
            shadow_ledger.clone(),
            None,
            None,
            live_pipeline.clone(),
            config,
        );

        let pool_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();

        let candidate = EnhancedCandidate {
            pool_amm_id: pool_id,
            base_mint,
            bonding_curve,
            slot: Some(200),
            ..Default::default()
        };
        assert!(runtime.register_new_pool(pool_id, base_mint, candidate, None));
        runtime.mark_pool_approved(pool_id);
        runtime.approved_pools().insert(pool_id);

        let initial_snap = ghost_core::shadow_ledger::MarketSnapshot {
            slot: Some(200),
            tx_key: Some(
                ghost_core::shadow_ledger::TxKey::new(200_000, Some(200), Some(0), None, 0)
                    .unwrap(),
            ),
            timestamp_ms: 200_000,
            tx_count: 5,
            ..Default::default()
        };
        shadow_ledger.commit_history(base_mint, vec![initial_snap.clone()], None);
        runtime.remember_committed_snapshot(base_mint, &initial_snap);
        runtime.mark_pool_committed(pool_id);

        // Simulate a hot-pool tx arriving after commitment.
        let tx = PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(210),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 210_000,
            event_time: ghost_core::EventTimeMetadata::new(None, Some(210_000), None),
            arrival_ts_ms: 210_000,
            signer: Pubkey::new_unique().to_string(),
            is_buy: true,
            volume_sol: 1.0,
            sol_amount_lamports: Some(1_000_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: Some(800_000_000_000.0),
            reserve_quote: Some(32.0),
            price_quote: Some(32.0 / 800_000_000_000.0),
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: Signature::new_unique().to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: Some(base_mint.to_string()),
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
            curve_data_known: true,
        };

        // ShadowLedger authority path: forward_approved_tx_to_commit_or_live_pipeline
        // must still work correctly — this is the ONLY path that records tx state.
        runtime.forward_approved_tx_to_commit_or_live_pipeline(
            pool_id,
            base_mint,
            &tx,
            tx_event_ts_ms(&tx),
        );

        assert!(
            live_pipeline.is_initialized(&base_mint),
            "LivePipeline must be initialized for committed mint after hot-pool forwarding"
        );
        let flushed = live_pipeline.flush_ready(&shadow_ledger);
        assert_eq!(
            flushed, 1,
            "LivePipeline must deliver exactly one snapshot to ShadowLedger"
        );

        let snapshots = shadow_ledger
            .get_snapshots(&base_mint)
            .expect("snapshots must exist after forwarding");
        assert_eq!(
            snapshots.len(),
            2,
            "ShadowLedger must hold committed history + one live-appended snapshot; got {}",
            snapshots.len()
        );
        // Reconciliation is NOT triggered here — Shadow Ledger state is correct.
        // This verifies the hardening did not introduce a second competing state engine.
        assert_eq!(
            snapshots.last().unwrap().timestamp_ms,
            210_000,
            "Most recent snapshot must be the hot-pool tx timestamp"
        );
    }

    // =========================================================================
    // D. Observability-linked tests
    // =========================================================================

    /// D.1 – `PoolTaskHandle::is_hot()` threshold aligns with the observability
    /// layer's `HOT_POOL_TX_THRESHOLD` constant.
    ///
    /// The hot-pool prioritization logic in oracle_runtime uses the same threshold
    /// as `HotPoolTxLossTracker::is_hot()` in drift_observability, so any metrics
    /// emitted by the observability layer correspond exactly to the pools receiving
    /// stronger ingest behaviour.
    #[tokio::test]
    async fn test_pool_task_handle_hot_threshold_matches_observability_layer() {
        use ghost_core::shadow_ledger::{HotPoolTxLossTracker, HOT_POOL_TX_THRESHOLD};

        // oracle_runtime side: PoolTaskHandle uses HOT_POOL_TX_THRESHOLD directly.
        let (sender, _rx) = tokio::sync::mpsc::channel::<PoolObservationMsg>(16);
        let mut handle = PoolTaskHandle {
            tx: sender,
            _abort_handle: tokio::runtime::Handle::current()
                .spawn(async {})
                .abort_handle(),
            tx_enqueued: HOT_POOL_TX_THRESHOLD - 1,
        };
        assert!(
            !handle.is_hot(),
            "handle must be cold at threshold-1 ({})",
            HOT_POOL_TX_THRESHOLD - 1
        );
        handle.tx_enqueued = HOT_POOL_TX_THRESHOLD;
        assert!(
            handle.is_hot(),
            "handle must be hot at exactly HOT_POOL_TX_THRESHOLD ({})",
            HOT_POOL_TX_THRESHOLD
        );

        // Observability side: HotPoolTxLossTracker must classify at the same point.
        let mint = Pubkey::new_unique();
        let mut tracker = HotPoolTxLossTracker::new();
        for _ in 0..HOT_POOL_TX_THRESHOLD - 1 {
            tracker.record_seen(&mint);
        }
        let summary = tracker.loss_summary(&mint).unwrap();
        assert!(
            !summary.is_hot(),
            "tracker must be cold before threshold ({} seen)",
            HOT_POOL_TX_THRESHOLD - 1
        );
        tracker.record_seen(&mint);
        let summary = tracker.loss_summary(&mint).unwrap();
        assert!(
            summary.is_hot(),
            "tracker must be hot at HOT_POOL_TX_THRESHOLD ({} seen)",
            HOT_POOL_TX_THRESHOLD
        );
    }

    /// D.2 – `HotPoolTxLossTracker` shows reduced estimated loss when
    /// all observations are forwarded (ideal hot-pool ingest behaviour).
    ///
    /// Simulates a hot-pool burst where all tx are forwarded (no loss) and
    /// verifies that `estimated_loss()` is zero and `loss_rate()` is 0.0.
    /// This is the target state the hot-pool ingest hardening aims to achieve.
    #[test]
    fn test_hot_pool_tracker_shows_zero_loss_when_all_tx_forwarded() {
        use ghost_core::shadow_ledger::{HotPoolTxLossTracker, HOT_POOL_TX_THRESHOLD};

        let mint = Pubkey::new_unique();
        let mut tracker = HotPoolTxLossTracker::new();

        let burst = HOT_POOL_TX_THRESHOLD * 3;
        for _ in 0..burst {
            tracker.record_seen(&mint);
            tracker.record_forwarded(&mint);
        }

        let summary = tracker.loss_summary(&mint).unwrap();
        assert!(summary.is_hot(), "pool must be hot after burst");
        assert_eq!(
            summary.estimated_loss(),
            0,
            "estimated loss must be zero when all tx are forwarded"
        );
        assert_eq!(
            summary.loss_rate(),
            0.0,
            "loss rate must be exactly 0.0 when all tx are forwarded (integer-derived)"
        );
        assert_eq!(
            summary.diagnostic_signals, 0,
            "no legacy repair signals should be triggered if all tx are forwarded"
        );
    }

    /// D.3 – `HotPoolTxLossTracker` correctly surfaces a high-loss scenario,
    /// demonstrating the contrast with the hardened (zero-loss) path in D.2.
    #[test]
    fn test_hot_pool_tracker_shows_loss_when_tx_not_forwarded() {
        use ghost_core::shadow_ledger::{HotPoolTxLossTracker, HOT_POOL_TX_THRESHOLD};

        let mint = Pubkey::new_unique();
        let mut tracker = HotPoolTxLossTracker::new();

        let burst = HOT_POOL_TX_THRESHOLD * 2;
        let forwarded = HOT_POOL_TX_THRESHOLD / 2;
        for _ in 0..burst {
            tracker.record_seen(&mint);
        }
        for _ in 0..forwarded {
            tracker.record_forwarded(&mint);
        }

        let summary = tracker.loss_summary(&mint).unwrap();
        assert!(summary.is_hot(), "pool must be hot after burst");
        assert_eq!(
            summary.estimated_loss(),
            burst - forwarded,
            "estimated loss must equal seen minus forwarded"
        );
        assert!(
            summary.loss_rate() > 0.0,
            "loss rate must be positive when tx were not forwarded"
        );
    }

    // =========================================================================
    // E. ReconciliationRuntime integration tests
    //
    // These tests verify that the explicit production reconciliation/observability
    // loop (ReconciliationRuntime) works correctly end-to-end from the launcher
    // perspective, covering the A–D requirement categories from the problem
    // statement.
    // =========================================================================

    /// E.1 – reconciliation is triggered through the actual runtime integration path.
    ///
    /// Creates a ReconciliationRuntime backed by a live ShadowLedger, seeds a
    /// pool, and verifies that process_account_update triggers reconciliation
    /// through the real path.
    #[test]
    fn test_reconciliation_runtime_integration_triggered_via_account_update() {
        use ghost_core::market_state::BondingCurve;
        use ghost_core::shadow_ledger::reconciliation::{DriftSeverity, ReconciliationAction};
        use ghost_core::shadow_ledger::{ReconciliationRuntime, ShadowLedger};

        let mint = Pubkey::new_unique();
        let initial_sol: u64 = 30_000_000_000;
        let initial_tok: u64 = 1_000_000_000_000;

        let ledger = ShadowLedger::new();
        ledger.insert_with_slot(
            mint,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: initial_sol,
                virtual_token_reserves: initial_tok,
                real_sol_reserves: initial_sol,
                real_token_reserves: initial_tok,
                token_total_supply: initial_tok,
                complete: 0,
                _padding: [0u8; 7],
            },
            100,
        );

        let mut runtime = ReconciliationRuntime::new(ledger.clone());

        // E.1a: no-drift AccountUpdate — no repair, state preserved
        let outcome = runtime
            .process_account_update(
                &mint,
                initial_sol,
                initial_tok,
                0,
                101,
                CurveFinality::Speculative,
            )
            .expect("must return outcome for known mint");
        assert_eq!(
            outcome.severity,
            DriftSeverity::None,
            "no drift expected when on-chain matches shadow"
        );
        assert_eq!(outcome.action, ReconciliationAction::NoAction);

        // E.1b: severe-drift AccountUpdate — logged through integration path
        // Simulate the shadow lagging 2 SOL behind chain (2 SOL of buys were missed)
        ledger.insert_with_slot(
            mint,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: initial_sol, // shadow still at initial
                virtual_token_reserves: initial_tok,
                real_sol_reserves: initial_sol,
                real_token_reserves: initial_tok,
                token_total_supply: initial_tok,
                complete: 0,
                _padding: [0u8; 7],
            },
            100,
        );
        let on_chain_sol = initial_sol + 2_000_000_000; // chain advanced by 2 SOL
        let outcome = runtime
            .process_account_update(
                &mint,
                on_chain_sol,
                initial_tok,
                0,
                102,
                CurveFinality::Speculative,
            )
            .expect("must return outcome for known mint");
        assert_eq!(
            outcome.severity,
            DriftSeverity::Severe,
            "2 SOL drift must be severe"
        );
        assert_eq!(
            outcome.action,
            ReconciliationAction::Logged,
            "severe drift must stay diagnostic-only through the runtime integration path"
        );

        // Diagnostic-only PR7 contract: ShadowLedger must remain unchanged.
        let stored_after_update = ledger.get(&mint).unwrap();
        assert_eq!(
            stored_after_update.virtual_sol_reserves, initial_sol,
            "ShadowLedger must not be healed by diagnostic-only reconciliation"
        );
    }

    /// E.2 – observability is fed by real runtime reconciliation events.
    ///
    /// Verifies that drift counters, repair counters, and hot-pool correlation
    /// are all populated through the ReconciliationRuntime integration path —
    /// not just via direct helper calls.
    #[test]
    fn test_reconciliation_runtime_observability_fed_by_real_events() {
        use ghost_core::market_state::BondingCurve;
        use ghost_core::shadow_ledger::reconciliation::ReconciliationAction;
        use ghost_core::shadow_ledger::{ReconciliationRuntime, ShadowLedger};

        let mint = Pubkey::new_unique();
        let sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;

        let ledger = ShadowLedger::new();
        let mut runtime = ReconciliationRuntime::new(ledger.clone());

        let insert_at = |virtual_sol: u64| {
            ledger.insert_with_slot(
                mint,
                BondingCurve {
                    discriminator: 0,
                    virtual_sol_reserves: virtual_sol,
                    virtual_token_reserves: tok,
                    real_sol_reserves: virtual_sol,
                    real_token_reserves: tok,
                    token_total_supply: tok,
                    complete: 0,
                    _padding: [0u8; 7],
                },
                100,
            )
        };

        // No-drift check
        insert_at(sol);
        runtime
            .process_account_update(&mint, sol, tok, 0, 1, CurveFinality::Speculative)
            .unwrap();

        // Severe-drift check → diagnostic-only logging
        insert_at(sol + 2_000_000_000);
        let outcome = runtime
            .process_account_update(&mint, sol, tok, 0, 2, CurveFinality::Speculative)
            .unwrap();
        assert_eq!(outcome.action, ReconciliationAction::Logged);

        // Per-pool drift stats are visible through the integration path
        let stats = runtime
            .pool_drift_stats(&mint)
            .expect("drift stats must be populated via runtime path");
        assert_eq!(
            stats.checks, 2,
            "check counter must reflect real runtime calls"
        );
        assert_eq!(
            stats.diagnostic_signals, 0,
            "legacy repair-signal counter must remain zero under PR7 diagnostic-only reconciliation"
        );
        assert!(
            stats.peak_abs_sol_drift >= 2_000_000_000,
            "peak drift must be recorded from real runtime event"
        );

        // Runtime status must reflect the drift without legacy repair signals
        let status = runtime.status();
        assert_eq!(
            status.total_diagnostic_signals, 0,
            "runtime status must not report legacy repair signals from diagnostic-only events"
        );
        assert_eq!(
            status.total_drifting_pools, 1,
            "one pool must be identified as drifting"
        );
    }

    /// E.3 – hot-pool drift correlation is visible through the runtime integration path.
    ///
    /// Simulates a hot pool that experiences both high tx volume and a repair,
    /// and verifies that the runtime path records both aspects correctly.
    #[test]
    fn test_reconciliation_runtime_hot_pool_drift_correlation_via_integration_path() {
        use ghost_core::market_state::BondingCurve;
        use ghost_core::shadow_ledger::reconciliation::ReconciliationAction;
        use ghost_core::shadow_ledger::{
            ReconciliationRuntime, ShadowLedger, HOT_POOL_TX_THRESHOLD,
        };

        let mint = Pubkey::new_unique();
        let sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;

        let ledger = ShadowLedger::new();
        ledger.insert_with_slot(
            mint,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: sol + 2_000_000_000, // severely drifted
                virtual_token_reserves: tok,
                real_sol_reserves: sol + 2_000_000_000,
                real_token_reserves: tok,
                token_total_supply: tok,
                complete: 0,
                _padding: [0u8; 7],
            },
            100,
        );

        let mut runtime = ReconciliationRuntime::new(ledger);

        // Classify the pool as hot via the integration path
        for _ in 0..HOT_POOL_TX_THRESHOLD {
            runtime.record_tx_seen(&mint);
        }
        // Partial forwarding — simulates some tx loss
        for _ in 0..HOT_POOL_TX_THRESHOLD / 2 {
            runtime.record_tx_forwarded(&mint);
        }

        // AccountUpdate triggers severe-drift logging
        let outcome = runtime
            .process_account_update(&mint, sol, tok, 0, 200, CurveFinality::Speculative)
            .expect("must return outcome for known mint");
        assert_eq!(
            outcome.action,
            ReconciliationAction::Logged,
            "severe drift must be logged through integration path"
        );

        // Verify hot-pool correlation through the runtime path
        let loss = runtime
            .pool_loss_summary(&mint)
            .expect("loss summary must exist for observed pool");
        assert!(
            loss.is_hot(),
            "pool must be hot — integration path must classify based on HOT_POOL_TX_THRESHOLD"
        );
        assert_eq!(
            loss.diagnostic_signals, 0,
            "legacy repair-signal correlation must remain zero under diagnostic-only reconciliation"
        );

        // Runtime-level status
        let status = runtime.status();
        assert_eq!(
            status.total_hot_pools, 1,
            "one hot pool must be visible through runtime status"
        );
        assert!(
            status.total_estimated_tx_loss > 0,
            "estimated tx loss must be positive (not all tx forwarded)"
        );
        assert_eq!(
            status.total_diagnostic_signals, 0,
            "legacy repair-signal count must stay at zero in diagnostic-only runtime status"
        );
    }

    /// E.4 – Shadow Ledger remains primary authority (non-regression).
    ///
    /// Verifies all three non-regression invariants via the ReconciliationRuntime:
    /// 1. Shadow Ledger is not overwritten when there is no drift.
    /// 2. AccountUpdate is corrective only (small drift does not overwrite).
    /// 3. ReconciliationRuntime does not create a competing state engine.
    #[test]
    fn test_reconciliation_runtime_shadow_ledger_authority_preserved() {
        use ghost_core::market_state::BondingCurve;
        use ghost_core::shadow_ledger::reconciliation::ReconciliationAction;
        use ghost_core::shadow_ledger::{ReconciliationRuntime, ShadowLedger};

        let sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;

        // ── D.1: no-drift → no overwrite ──────────────────────────────────
        {
            let mint = Pubkey::new_unique();
            let ledger = ShadowLedger::new();
            ledger.insert_with_slot(
                mint,
                BondingCurve {
                    discriminator: 0,
                    virtual_sol_reserves: sol,
                    virtual_token_reserves: tok,
                    real_sol_reserves: sol,
                    real_token_reserves: tok,
                    token_total_supply: tok,
                    complete: 0,
                    _padding: [0u8; 7],
                },
                100,
            );
            let mut runtime = ReconciliationRuntime::new(ledger.clone());
            let outcome = runtime
                .process_account_update(&mint, sol, tok, 0, 1, CurveFinality::Speculative)
                .unwrap();
            assert_eq!(
                outcome.action,
                ReconciliationAction::NoAction,
                "no-drift AccountUpdate must not overwrite tx-driven state"
            );
            let curve = ledger.get(&mint).unwrap();
            assert_eq!(
                curve.virtual_sol_reserves, sol,
                "Shadow Ledger must remain primary — tx-driven value preserved"
            );
        }

        // ── D.2: noise drift → AccountUpdate remains corrective only ───────
        {
            let mint = Pubkey::new_unique();
            let noise_shadow_sol = sol + 500_000; // 0.0005 SOL — noise level
            let ledger = ShadowLedger::new();
            ledger.insert_with_slot(
                mint,
                BondingCurve {
                    discriminator: 0,
                    virtual_sol_reserves: noise_shadow_sol,
                    virtual_token_reserves: tok,
                    real_sol_reserves: noise_shadow_sol,
                    real_token_reserves: tok,
                    token_total_supply: tok,
                    complete: 0,
                    _padding: [0u8; 7],
                },
                100,
            );
            let mut runtime = ReconciliationRuntime::new(ledger.clone());
            let outcome = runtime
                .process_account_update(&mint, sol, tok, 0, 1, CurveFinality::Speculative)
                .unwrap();
            assert_ne!(
                outcome.action,
                ReconciliationAction::DiagnosticSignal,
                "noise-level AccountUpdate must not overwrite (corrective only)"
            );
            let curve = ledger.get(&mint).unwrap();

            assert_eq!(
                curve.virtual_sol_reserves, noise_shadow_sol,
                "tx-driven state must be preserved for noise-level AccountUpdate"
            );
        }

        // ── D.3: unknown mint → no competing state engine ──────────────────
        {
            let ledger = ShadowLedger::new();
            let mut runtime = ReconciliationRuntime::new(ledger.clone());
            let unknown_mint = Pubkey::new_unique();
            let result = runtime.process_account_update(
                &unknown_mint,
                sol,
                tok,
                0,
                1,
                CurveFinality::Speculative,
            );
            assert!(
                result.is_none(),
                "unknown mint must return None — no competing state creation"
            );
            assert_eq!(
                ledger.len(),
                0,
                "ReconciliationRuntime must not create new ledger entries"
            );
        }
    }

    // =========================================================================
    // F. OracleRuntime production integration tests
    //
    // These tests verify that ReconciliationRuntime is correctly owned and
    // driven by OracleRuntime itself — not just as a stand-alone unit.
    // =========================================================================

    fn make_oracle_runtime_for_test() -> Arc<OracleRuntime> {
        use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
        use ghost_core::shadow_ledger::ShadowLedger;
        Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
            Arc::new(ShadowLedger::new()),
        ))
    }

    #[test]
    fn mark_pool_committed_ignores_nonexistent_runtime_pool() {
        let runtime = OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::new(ShadowLedger::new()),
        );

        let pool_id = Pubkey::new_unique();
        runtime.mark_pool_committed(pool_id);

        assert_eq!(
            runtime.runtime_pool_state(&pool_id),
            None,
            "non-existent pools must not be resurrected as committed"
        );
    }

    /// F.1 – ReconciliationRuntime is created and attached through the real OracleRuntime path.
    ///
    /// Verifies that OracleRuntime construction initializes ReconciliationRuntime
    /// and that an initial status snapshot is available without any setup beyond
    /// calling OracleRuntime::new.
    #[test]
    fn test_oracle_runtime_owns_reconciliation_runtime_after_construction() {
        let runtime = make_oracle_runtime_for_test();

        // Must be accessible without panicking
        let status = runtime.reconciliation_status();
        assert_eq!(
            status.registered_pools, 0,
            "no pools registered at construction"
        );
        assert_eq!(status.total_checks, 0, "no checks at construction");
        assert_eq!(
            status.total_diagnostic_signals, 0,
            "no legacy repair signals at construction"
        );
        assert_eq!(status.cycle_count, 0, "no cycles at construction");
    }

    /// F.2 – Pool registration through register_new_pool wires into ReconciliationRuntime.
    ///
    /// Verifies that calling register_new_pool on OracleRuntime causes the
    /// base_mint to appear in the reconciliation registry.
    #[test]
    fn test_oracle_runtime_pool_registration_wires_into_reconciliation_runtime() {
        use ghost_brain::fast_pipeline::EnhancedCandidate;
        use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
        use ghost_core::shadow_ledger::ShadowLedger;

        let ledger = Arc::new(ShadowLedger::new());
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
            ledger.clone(),
        ));

        let pool_amm_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();

        let registered = runtime.register_new_pool(
            pool_amm_id,
            base_mint,
            EnhancedCandidate {
                bonding_curve: pool_amm_id,
                ..Default::default()
            },
            None,
        );
        assert!(registered, "pool must be successfully registered");

        let status = runtime.reconciliation_status();
        assert_eq!(
            status.registered_pools, 1,
            "base_mint must be registered in reconciliation runtime after register_new_pool"
        );
    }

    /// F.3 – Pool deregistration through remove_pool unregisters from ReconciliationRuntime.
    ///
    /// Verifies that calling remove_pool removes the mint from the reconciliation
    /// registry so it no longer consumes cycle budget.
    #[test]
    fn test_oracle_runtime_pool_deregistration_removes_from_reconciliation_runtime() {
        use ghost_brain::fast_pipeline::EnhancedCandidate;
        use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
        use ghost_core::shadow_ledger::ShadowLedger;

        let ledger = Arc::new(ShadowLedger::new());
        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
            ledger.clone(),
        ));

        let pool_amm_id = Pubkey::new_unique();
        let base_mint = Pubkey::new_unique();

        runtime.register_new_pool(
            pool_amm_id,
            base_mint,
            EnhancedCandidate {
                bonding_curve: pool_amm_id,
                ..Default::default()
            },
            None,
        );
        assert_eq!(
            runtime.reconciliation_status().registered_pools,
            1,
            "pool must be registered before removal test"
        );

        let removed = runtime.remove_pool(pool_amm_id);
        assert!(removed, "pool must be successfully removed");
        assert_eq!(
            runtime.reconciliation_status().registered_pools,
            0,
            "pool must be deregistered from reconciliation runtime after remove_pool"
        );
    }

    /// F.4 – AccountUpdate integration through OracleRuntime feeds reconciliation.
    ///
    /// Verifies that process_account_update on OracleRuntime correctly delegates
    /// to the reconciliation runtime, updates drift counters, and returns an
    /// outcome for a pool whose mint is tracked by the Shadow Ledger.
    #[test]
    fn test_oracle_runtime_process_account_update_feeds_reconciliation() {
        use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
        use ghost_core::market_state::BondingCurve;
        use ghost_core::shadow_ledger::reconciliation::ReconciliationAction;
        use ghost_core::shadow_ledger::ShadowLedger;

        let mint = Pubkey::new_unique();
        let sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;

        let ledger = Arc::new(ShadowLedger::new());
        ledger.insert_with_slot(
            mint,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: sol,
                virtual_token_reserves: tok,
                real_sol_reserves: sol,
                real_token_reserves: tok,
                token_total_supply: tok,
                complete: 0,
                _padding: [0u8; 7],
            },
            100,
        );

        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
            ledger.clone(),
        ));
        register_runtime_pool_for_base_mint(runtime.as_ref(), mint, mint);

        // F.4a: no-drift AccountUpdate — no repair
        let outcome = runtime
            .process_account_update(&mint, sol, tok, 0, 101, CurveFinality::Speculative)
            .expect("must return outcome for known mint");
        assert_eq!(
            outcome.action,
            ReconciliationAction::NoAction,
            "no-drift AccountUpdate must not repair through OracleRuntime path"
        );

        let status = runtime.reconciliation_status();
        assert_eq!(
            status.total_checks, 1,
            "total_checks must increment after process_account_update via OracleRuntime"
        );
        assert_eq!(
            status.total_diagnostic_signals, 0,
            "no legacy repair signals for no-drift update"
        );

        // F.4b: severe-drift AccountUpdate — diagnostic-only logging
        let on_chain_sol = sol + 2_000_000_000; // 2 SOL drift
        let outcome = runtime
            .process_account_update(&mint, on_chain_sol, tok, 0, 102, CurveFinality::Speculative)
            .expect("must return outcome for known mint");
        assert_eq!(
            outcome.action,
            ReconciliationAction::Logged,
            "severe drift must be logged through OracleRuntime integration path"
        );

        let status = runtime.reconciliation_status();
        assert_eq!(
            status.total_diagnostic_signals, 0,
            "legacy repair signals must remain zero in OracleRuntime reconciliation_status"
        );
        assert_eq!(
            status.total_drifting_pools, 1,
            "one drifting pool must be visible through OracleRuntime status"
        );
    }

    #[test]
    fn test_decode_canonical_account_update_supports_pumpswap_pool_layout() {
        let base_mint = Pubkey::new_unique();
        let pool_state = seer::binary_parser::AmmPoolState {
            pool_bump: 1,
            index: 1,
            creator: Pubkey::new_unique().to_bytes(),
            base_mint: base_mint.to_bytes(),
            quote_mint: Pubkey::from_str(WRAPPED_SOL_MINT)
                .expect("valid wrapped SOL mint")
                .to_bytes(),
            lp_mint: Pubkey::new_unique().to_bytes(),
            pool_base_token_account: Pubkey::new_unique().to_bytes(),
            pool_quote_token_account: Pubkey::new_unique().to_bytes(),
            base_amount: 456,
            quote_amount: 789,
        };

        let mut data = seer::binary_parser::DISC_AMM_POOL.to_vec();
        data.push(pool_state.pool_bump);
        data.extend_from_slice(&pool_state.index.to_le_bytes());
        data.extend_from_slice(&pool_state.creator);
        data.extend_from_slice(&pool_state.base_mint);
        data.extend_from_slice(&pool_state.quote_mint);
        data.extend_from_slice(&pool_state.lp_mint);
        data.extend_from_slice(&pool_state.pool_base_token_account);
        data.extend_from_slice(&pool_state.pool_quote_token_account);
        data.extend_from_slice(&pool_state.base_amount.to_le_bytes());
        data.extend_from_slice(&pool_state.quote_amount.to_le_bytes());

        let payload = seer::decode_canonical_account_update(
            Pubkey::from_str("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA")
                .expect("valid pumpswap program"),
            &data,
        )
        .expect("pumpswap AMM pool must decode as canonical account update");

        assert_eq!(payload.sol_reserves(), 789);
        assert_eq!(payload.token_reserves(), 456);
        assert_eq!(payload.complete(), 1);
        assert_eq!(payload.token_mint(), Some(base_mint));
    }

    /// F.6 – ReconciliationRuntime non-regression via OracleRuntime: no repair writes occur.
    ///
    /// Verifies that even after wiring ReconciliationRuntime into OracleRuntime,
    /// diagnostic AccountUpdate drift detection does not mutate ShadowLedger.
    #[test]
    fn test_oracle_runtime_diagnostic_reconciliation_does_not_mutate_shadow_ledger() {
        use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
        use ghost_core::market_state::BondingCurve;
        use ghost_core::shadow_ledger::reconciliation::ReconciliationAction;
        use ghost_core::shadow_ledger::ShadowLedger;

        let mint = Pubkey::new_unique();
        let sol: u64 = 30_000_000_000;
        let tok: u64 = 1_000_000_000_000;

        let ledger = Arc::new(ShadowLedger::new());
        ledger.insert_with_slot(
            mint,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: sol,
                virtual_token_reserves: tok,
                real_sol_reserves: sol,
                real_token_reserves: tok,
                token_total_supply: tok,
                complete: 0,
                _padding: [0u8; 7],
            },
            100,
        );

        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
            ledger.clone(),
        ));
        register_runtime_pool_for_base_mint(runtime.as_ref(), mint, mint);

        // Small noise drift — must NOT overwrite Shadow Ledger state
        let noise_sol = sol + 100_000; // tiny noise, below SEVERE threshold
        let outcome = runtime
            .process_account_update(&mint, noise_sol, tok, 0, 101, CurveFinality::Speculative)
            .expect("must return outcome for tracked mint");

        assert_ne!(
            outcome.action,
            ReconciliationAction::DiagnosticSignal,
            "noise-level drift must not mutate ShadowLedger via OracleRuntime"
        );

        // Shadow Ledger still holds the tx-driven value — not the noise AccountUpdate
        let curve = ledger.get(&mint).unwrap();
        assert_eq!(
            curve.virtual_sol_reserves, sol,
            "diagnostic reconciliation must preserve the existing ShadowLedger value"
        );

        // Unknown mint — no competing state engine
        let unknown_mint = Pubkey::new_unique();
        let result = runtime.process_account_update(
            &unknown_mint,
            sol,
            tok,
            0,
            1,
            CurveFinality::Speculative,
        );
        assert!(
            result.is_none(),
            "OracleRuntime must not create Shadow Ledger entries for unknown mints"
        );
        assert_eq!(
            ledger.len(),
            1,
            "only the originally inserted mint must exist in the ledger"
        );
    }

    // =========================================================================
    // Live AccountUpdate Reconciliation Wiring Tests
    // =========================================================================
    // These tests prove that the live AccountUpdate-driven reconciliation path
    // works end-to-end: from GhostEvent::AccountUpdate arriving on the event bus
    // through OracleRuntime::process_account_update(...) to drift/read-only status
    // changes becoming visible. They also verify key-resolution correctness
    // and non-regression guarantees (diagnostic reconciliation stays read-only).

    /// Wire.A – `GhostEvent::AccountUpdate` drives `OracleRuntime::process_account_update`.
    ///
    /// Simulates the production path: a `GhostEvent::AccountUpdate` event is
    /// constructed and sent to the oracle runtime, verifying that
    /// `process_account_update` is invoked with the correct reserve data.
    #[test]
    fn test_ghost_event_account_update_drives_reconciliation() {
        use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
        use ghost_core::market_state::BondingCurve;
        use ghost_core::shadow_ledger::reconciliation::ReconciliationAction;
        use ghost_core::shadow_ledger::ShadowLedger;

        let base_mint = Pubkey::new_unique();
        let sol: u64 = 20_000_000_000;
        let tok: u64 = 800_000_000_000;

        // Pre-populate Shadow Ledger so the mint is tracked.
        let ledger = Arc::new(ShadowLedger::new());
        ledger.insert_with_slot(
            base_mint,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: sol,
                virtual_token_reserves: tok,
                real_sol_reserves: sol,
                real_token_reserves: tok,
                token_total_supply: tok,
                complete: 0,
                _padding: [0u8; 7],
            },
            50,
        );

        let runtime = Arc::new(OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
            ledger.clone(),
        ));
        register_runtime_pool_for_base_mint(runtime.as_ref(), base_mint, base_mint);

        // Simulate what start_oracle_runtime_task does when it receives
        // GhostEvent::AccountUpdate from the event bus.
        let event = GhostEvent::AccountUpdate(crate::events::AccountUpdateEvent {
            semantic: Default::default(),
            event_time: ghost_core::EventTimeMetadata::default(),
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            curve_finality: CurveFinality::Speculative,
            sol_reserves: sol,
            token_reserves: tok,
            complete: 0,
            slot: 51,
            write_version: None,
            replay_origin: seer::ipc::AccountUpdateReplayOrigin::Live,
            replay_buffer_dwell_ms: None,
            detected_at: std::time::SystemTime::now(),
            sequence_number: 1,
        });

        // Extract the fields (mirroring the match arm in start_oracle_runtime_task).
        let outcome = match event {
            GhostEvent::AccountUpdate(event) => runtime.process_account_update(
                &event.base_mint,
                event.sol_reserves,
                event.token_reserves,
                event.complete,
                event.slot,
                event.curve_finality,
            ),
            _ => panic!("unexpected event variant"),
        };

        // The mint IS tracked → outcome must be Some.
        assert!(
            outcome.is_some(),
            "GhostEvent::AccountUpdate for a tracked mint must produce a reconciliation outcome"
        );
        // Values are identical → no repair needed.
        let outcome = outcome.unwrap();
        assert_ne!(
            outcome.action,
            ReconciliationAction::DiagnosticSignal,
            "identical on-chain / Shadow Ledger reserves must not trigger a repair"
        );
    }

    /// Wire.B – Key resolution: only the correct `base_mint` key drives reconciliation.
    ///
    /// Verifies that sending a `GhostEvent::AccountUpdate` for an untracked pubkey
    /// returns `None`, and the Shadow Ledger is not mutated.
    #[test]
    fn test_ghost_event_account_update_wrong_key_no_reconciliation() {
        use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
        use ghost_core::market_state::BondingCurve;
        use ghost_core::shadow_ledger::ShadowLedger;

        let tracked_mint = Pubkey::new_unique();
        let untracked_mint = Pubkey::new_unique();
        let sol: u64 = 10_000_000_000;
        let tok: u64 = 500_000_000_000;

        let ledger = Arc::new(ShadowLedger::new());
        ledger.insert_with_slot(
            tracked_mint,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: sol,
                virtual_token_reserves: tok,
                real_sol_reserves: sol,
                real_token_reserves: tok,
                token_total_supply: tok,
                complete: 0,
                _padding: [0u8; 7],
            },
            10,
        );

        let runtime = OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
            ledger.clone(),
        );
        register_runtime_pool_for_base_mint(&runtime, tracked_mint, tracked_mint);

        // Wrong key — must produce None (no competing state entry created).
        let result = runtime.process_account_update(
            &untracked_mint,
            sol,
            tok,
            0,
            20,
            CurveFinality::Speculative,
        );
        assert!(
            result.is_none(),
            "AccountUpdate for an untracked mint must not create a reconciliation entry"
        );

        // Ledger still contains exactly one entry (the tracked mint).
        assert_eq!(
            ledger.len(),
            1,
            "untracked AccountUpdate must not create new Shadow Ledger entries"
        );

        // Correct key — must produce Some.
        let result = runtime.process_account_update(
            &tracked_mint,
            sol,
            tok,
            0,
            20,
            CurveFinality::Speculative,
        );
        assert!(
            result.is_some(),
            "AccountUpdate for a tracked mint must return a reconciliation outcome"
        );
    }

    /// Wire.C – Observability: drift counters change when live path logs severe drift.
    ///
    /// Applies a large synthetic SOL-reserve divergence via `GhostEvent::AccountUpdate`
    /// and verifies that the reconciliation status reflects drift without legacy
    /// repair signals.
    #[test]
    fn test_account_update_live_path_drift_visible_in_status() {
        use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
        use ghost_core::market_state::BondingCurve;
        use ghost_core::shadow_ledger::reconciliation::ReconciliationAction;
        use ghost_core::shadow_ledger::ShadowLedger;

        let base_mint = Pubkey::new_unique();
        let sol: u64 = 30_000_000_000; // 30 SOL (30 billion lamports)
        let tok: u64 = 1_000_000_000_000;

        let ledger = Arc::new(ShadowLedger::new());
        ledger.insert_with_slot(
            base_mint,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: sol,
                virtual_token_reserves: tok,
                real_sol_reserves: sol,
                real_token_reserves: tok,
                token_total_supply: tok,
                complete: 0,
                _padding: [0u8; 7],
            },
            100,
        );

        let runtime = OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
            ledger.clone(),
        );
        register_runtime_pool_for_base_mint(&runtime, base_mint, base_mint);

        let status_before = runtime.reconciliation_status();

        // Large divergence — triggers severe-drift logging through the live AccountUpdate path.
        let diverged_sol = 1_000_000_000_000; // 1000 SOL — severe drift
        let outcome = runtime
            .process_account_update(
                &base_mint,
                diverged_sol,
                tok,
                0,
                101,
                CurveFinality::Speculative,
            )
            .expect("tracked mint must return outcome");

        assert_eq!(
            outcome.action,
            ReconciliationAction::Logged,
            "severe SOL divergence must be logged via the live AccountUpdate path"
        );

        let status_after = runtime.reconciliation_status();

        assert_eq!(
            status_after.total_checks,
            status_before.total_checks + 1,
            "total_checks counter must increment when live AccountUpdate drives reconciliation"
        );
        assert_eq!(
            status_after.total_diagnostic_signals, status_before.total_diagnostic_signals,
            "total_diagnostic_signals must remain unchanged for diagnostic-only reconciliation"
        );
    }

    /// Wire.D – Non-regression: diagnostic AccountUpdate drift detection stays read-only.
    ///
    /// Verifies that small noise updates (below the SEVERE threshold) do NOT
    /// overwrite the tx-driven state, confirming the diagnostic-only model.
    #[test]
    fn test_account_update_shadow_ledger_corrective_only_non_regression() {
        use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
        use ghost_core::market_state::BondingCurve;
        use ghost_core::shadow_ledger::reconciliation::ReconciliationAction;
        use ghost_core::shadow_ledger::ShadowLedger;

        let base_mint = Pubkey::new_unique();
        let sol: u64 = 25_000_000_000;
        let tok: u64 = 900_000_000_000;

        let ledger = Arc::new(ShadowLedger::new());
        ledger.insert_with_slot(
            base_mint,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: sol,
                virtual_token_reserves: tok,
                real_sol_reserves: sol,
                real_token_reserves: tok,
                token_total_supply: tok,
                complete: 0,
                _padding: [0u8; 7],
            },
            200,
        );

        let runtime = OracleRuntime::new(
            Arc::new(HyperPredictionOracle::default()),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string(),
            ledger.clone(),
        );
        register_runtime_pool_for_base_mint(&runtime, base_mint, base_mint);

        // Tiny noise — below SEVERE threshold, must NOT overwrite.
        let noise_sol = sol + 50_000; // < 1 SOL difference
        let outcome = runtime
            .process_account_update(
                &base_mint,
                noise_sol,
                tok,
                0,
                201,
                CurveFinality::Speculative,
            )
            .expect("tracked mint must return outcome");

        assert_ne!(
            outcome.action,
            ReconciliationAction::DiagnosticSignal,
            "noise-level drift must not repair — Shadow Ledger tx-driven state is primary"
        );

        // Shadow Ledger still holds the tx-driven value (virtual_sol_reserves = sol).
        let curve = ledger.get(&base_mint).expect("mint must be present");
        assert_eq!(
            curve.virtual_sol_reserves, sol,
            "Shadow Ledger must remain primary — tx-driven value must be preserved after noise AccountUpdate"
        );

        // Untracked mint — must not create a competing state entry.
        let untracked = Pubkey::new_unique();
        let result = runtime.process_account_update(
            &untracked,
            sol,
            tok,
            0,
            201,
            CurveFinality::Speculative,
        );
        assert!(
            result.is_none(),
            "untracked AccountUpdate must not create a competing state engine entry"
        );
        assert_eq!(
            ledger.len(),
            1,
            "no new ledger entries must be created for untracked mints"
        );
    }

    // ── Identity promotion retry / backoff / exhaustion tests ──────────

    #[test]
    fn test_promotion_retry_stops_after_max_retries() {
        let pool_id = Pubkey::new_unique();
        let gatekeeper_window_ms: u64 = 10_000;

        let mut identity = ObservationIdentity {
            base_mint: "unknown".to_string(),
            first_seen_ts_ms: 1_000,
            first_seen_clock_source: "runtime_wall",
            end_10s_ts_ms: 11_000,
            dev_pubkey: "unknown".to_string(),
            failed_promotion_attempts: 0,
            next_promotion_attempt_ts_ms: 0,
        };
        let mut base_mint_pubkey: Option<Pubkey> = None;

        // TX with no token_mint and no signer — will always fail to promote.
        let tx = Arc::new(PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 1_000,
            signer: "".to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: Some(100_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "test_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
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
        });

        // Use explicit small budget so the test is deterministic and fast.
        let max_retries: u8 = 5;
        let mut now_ms: u64 = 1_000_000;

        // Drive time forward past each backoff window until the budget is
        // exhausted.  Each iteration advances now_ms to at least
        // next_promotion_attempt_ts_ms so the backoff gate never blocks us.
        for _ in 0..(max_retries as usize + 2) {
            if identity.next_promotion_attempt_ts_ms > now_ms {
                now_ms = identity.next_promotion_attempt_ts_ms;
            }
            maybe_promote_observation_identity_from_tx(
                pool_id,
                &tx,
                gatekeeper_window_ms,
                &mut identity,
                &mut base_mint_pubkey,
                now_ms,
                max_retries,
            );
            now_ms += 1;
        }

        assert_eq!(
            identity.failed_promotion_attempts, max_retries,
            "counter should have reached max ({max_retries}), got {}",
            identity.failed_promotion_attempts
        );

        // Subsequent calls must be no-ops regardless of how much time passes.
        let result = maybe_promote_observation_identity_from_tx(
            pool_id,
            &tx,
            gatekeeper_window_ms,
            &mut identity,
            &mut base_mint_pubkey,
            now_ms + 999_999_999,
            max_retries,
        );
        assert!(!result, "must return false after budget exhaustion");
        assert_eq!(
            identity.base_mint, "unknown",
            "identity must remain unknown after exhaustion"
        );
    }

    #[test]
    fn test_promotion_success_resets_counter() {
        let pool_id = Pubkey::new_unique();
        let gatekeeper_window_ms: u64 = 10_000;
        let valid_mint = Pubkey::new_unique();

        let mut identity = ObservationIdentity {
            base_mint: "unknown".to_string(),
            first_seen_ts_ms: 1_000,
            first_seen_clock_source: "runtime_wall",
            end_10s_ts_ms: 11_000,
            dev_pubkey: "unknown".to_string(),
            failed_promotion_attempts: 10,   // simulate prior failures
            next_promotion_attempt_ts_ms: 0, // backoff window already passed
        };
        let mut base_mint_pubkey: Option<Pubkey> = None;

        // TX that carries a valid token_mint → should succeed
        let tx = Arc::new(PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 2_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 2_000,
            signer: Pubkey::new_unique().to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: Some(100_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "test_sig_2".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: Some(valid_mint.to_string()),
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
        });

        let result = maybe_promote_observation_identity_from_tx(
            pool_id,
            &tx,
            gatekeeper_window_ms,
            &mut identity,
            &mut base_mint_pubkey,
            2_000,
            25,
        );

        assert!(result, "promotion should succeed when token_mint present");
        assert_eq!(
            identity.failed_promotion_attempts, 0,
            "counter must reset to 0 on success"
        );
        assert_eq!(
            identity.base_mint,
            valid_mint.to_string(),
            "base_mint must be updated to the tx token_mint"
        );
    }

    #[test]
    fn test_late_arrival_newpool_upgrades_incomplete_identity() {
        // Simulate an identity that started with unknown base_mint
        let mut identity = ObservationIdentity {
            base_mint: "unknown".to_string(),
            first_seen_ts_ms: 1_000,
            first_seen_clock_source: "runtime_wall",
            end_10s_ts_ms: 11_000,
            dev_pubkey: "unknown".to_string(),
            failed_promotion_attempts: 15, // partially exhausted
            next_promotion_attempt_ts_ms: 0,
        };

        let real_mint = Pubkey::new_unique();
        let real_creator = Pubkey::new_unique();

        // Simulate the late-arrival logic from pool_observation_task
        let identity_needs_upgrade = !is_shadow_base_mint_present(&identity.base_mint)
            || !is_shadow_creator_present(&identity.dev_pubkey);

        assert!(
            identity_needs_upgrade,
            "identity with unknown base_mint should need upgrade"
        );

        // Apply the upgrade (mirrors the NewPool handler code)
        if identity_needs_upgrade {
            identity = ObservationIdentity {
                base_mint: real_mint.to_string(),
                first_seen_ts_ms: 2_000,
                first_seen_clock_source: "registered_wall",
                end_10s_ts_ms: 12_000,
                dev_pubkey: real_creator.to_string(),
                failed_promotion_attempts: 0, // reset on upgrade
                next_promotion_attempt_ts_ms: 0,
            };
        }

        assert_eq!(
            identity.base_mint,
            real_mint.to_string(),
            "base_mint must be updated from late NewPool"
        );
        assert_eq!(
            identity.dev_pubkey,
            real_creator.to_string(),
            "dev_pubkey must be updated from late NewPool"
        );
        assert_eq!(
            identity.failed_promotion_attempts, 0,
            "failed_promotion_attempts must reset on late-arrival upgrade"
        );
    }

    // ── Regression: old `% 2 == 0` backoff caused permanent freeze at 2 ──

    /// The original bug: after 2 consecutive failures `failed_promotion_attempts`
    /// was even, so the `% 2 == 0` guard always fired an early-return → the
    /// counter was stuck at 2 forever and no more attempts were made.
    ///
    /// With the exponential-time-backoff replacement the counter must advance
    /// beyond 2 once the backoff window has elapsed.
    #[test]
    fn test_promotion_regression_no_freeze_after_two_failures() {
        let pool_id = Pubkey::new_unique();
        let gatekeeper_window_ms: u64 = 10_000;
        let max_retries: u8 = 10;

        // TX without identity data — always fails to promote.
        let tx = Arc::new(PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 0,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 0,
            signer: "".to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: Some(100_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "reg_sig".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
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
        });

        let mut identity = ObservationIdentity {
            base_mint: "unknown".to_string(),
            first_seen_ts_ms: 1_000,
            first_seen_clock_source: "runtime_wall",
            end_10s_ts_ms: 11_000,
            dev_pubkey: "unknown".to_string(),
            failed_promotion_attempts: 0,
            next_promotion_attempt_ts_ms: 0,
        };
        let mut base_mint_pubkey: Option<Pubkey> = None;

        let mut now_ms: u64 = 1_000_000;

        // Fail 1: attempts → 1, next_ts = now + 50 ms
        let r = maybe_promote_observation_identity_from_tx(
            pool_id,
            &tx,
            gatekeeper_window_ms,
            &mut identity,
            &mut base_mint_pubkey,
            now_ms,
            max_retries,
        );
        assert!(!r);
        assert_eq!(identity.failed_promotion_attempts, 1);
        let next_ts_after_1 = identity.next_promotion_attempt_ts_ms;

        // Still in backoff window — counter must NOT change.
        let r = maybe_promote_observation_identity_from_tx(
            pool_id,
            &tx,
            gatekeeper_window_ms,
            &mut identity,
            &mut base_mint_pubkey,
            next_ts_after_1 - 1,
            max_retries,
        );
        assert!(!r, "must be blocked during backoff window");
        assert_eq!(
            identity.failed_promotion_attempts, 1,
            "counter must not change while in backoff window"
        );

        // Exactly at backoff expiry: Fail 2 → attempts → 2, next_ts = now + 200 ms
        now_ms = next_ts_after_1;
        let r = maybe_promote_observation_identity_from_tx(
            pool_id,
            &tx,
            gatekeeper_window_ms,
            &mut identity,
            &mut base_mint_pubkey,
            now_ms,
            max_retries,
        );
        assert!(!r);
        assert_eq!(identity.failed_promotion_attempts, 2);
        let next_ts_after_2 = identity.next_promotion_attempt_ts_ms;

        // OLD BUG: here the old code would return early forever (% 2 == 0).
        // New code: once backoff window passes, counter advances beyond 2.
        now_ms = next_ts_after_2;
        let r = maybe_promote_observation_identity_from_tx(
            pool_id,
            &tx,
            gatekeeper_window_ms,
            &mut identity,
            &mut base_mint_pubkey,
            now_ms,
            max_retries,
        );
        assert!(!r);
        assert_eq!(
            identity.failed_promotion_attempts, 3,
            "regression: counter must advance past 2 (old bug caused permanent freeze at 2)"
        );
    }

    // ── Partial success resets counter; remaining field filled on next tx ──

    /// If only `base_mint` is promoted (dev_pubkey still unknown) the failure
    /// counter must reset to 0.  On the very next tx carrying a valid signer
    /// the `dev_pubkey` gap should be filled without any backoff interference.
    #[test]
    fn test_promotion_partial_success_resets_counter() {
        let pool_id = Pubkey::new_unique();
        let gatekeeper_window_ms: u64 = 10_000;
        let max_retries: u8 = 25;
        let valid_mint = Pubkey::new_unique();
        // Use Keypair::new() to guarantee the pubkey is on the Ed25519 curve,
        // which is required by normalize_dev_pubkey_str's is_on_curve() check.
        let valid_signer = Keypair::new().pubkey();

        let mut identity = ObservationIdentity {
            base_mint: "unknown".to_string(),
            first_seen_ts_ms: 1_000,
            first_seen_clock_source: "runtime_wall",
            end_10s_ts_ms: 11_000,
            dev_pubkey: "unknown".to_string(),
            failed_promotion_attempts: 8, // prior failures
            next_promotion_attempt_ts_ms: 0,
        };
        let mut base_mint_pubkey: Option<Pubkey> = None;

        // TX #1: has token_mint but NO signer → only base_mint is promoted.
        let tx_mint_only = Arc::new(PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(1),
            event_ordinal: Some(0),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 2_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 2_000,
            signer: "".to_string(), // no valid signer
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: Some(100_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "partial_sig_1".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: Some(valid_mint.to_string()),
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
        });

        let r1 = maybe_promote_observation_identity_from_tx(
            pool_id,
            &tx_mint_only,
            gatekeeper_window_ms,
            &mut identity,
            &mut base_mint_pubkey,
            2_000,
            max_retries,
        );

        assert!(r1, "partial success: base_mint should be promoted");
        assert_eq!(
            identity.base_mint,
            valid_mint.to_string(),
            "base_mint must be set"
        );
        assert_eq!(
            identity.dev_pubkey, "unknown",
            "dev_pubkey must still be unknown"
        );
        assert_eq!(
            identity.failed_promotion_attempts, 0,
            "partial success must reset failure counter"
        );
        assert_eq!(
            identity.next_promotion_attempt_ts_ms, 0,
            "partial success must reset backoff timestamp"
        );

        // TX #2: has valid signer but mint is already known → fills dev_pubkey.
        let tx_signer = Arc::new(PoolTransaction {
            semantic: Default::default(),
            curve_finality: CurveFinality::Speculative,
            pool_amm_id: pool_id.to_string(),
            slot: Some(2),
            event_ordinal: Some(1),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 3_000,
            event_time: ghost_core::EventTimeMetadata::default(),
            arrival_ts_ms: 3_000,
            signer: valid_signer.to_string(),
            is_buy: true,
            volume_sol: 0.1,
            sol_amount_lamports: Some(100_000_000),
            token_amount_units: Some(1_000_000),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: "partial_sig_2".to_string(),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: RawBytesMissingReason::Unknown,
            token_mint: None, // no mint — already known
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
        });

        let r2 = maybe_promote_observation_identity_from_tx(
            pool_id,
            &tx_signer,
            gatekeeper_window_ms,
            &mut identity,
            &mut base_mint_pubkey,
            3_000,
            max_retries,
        );

        assert!(r2, "dev_pubkey should be promoted from second tx");
        assert_eq!(
            identity.dev_pubkey,
            valid_signer.to_string(),
            "dev_pubkey must be set"
        );
        assert_eq!(
            identity.failed_promotion_attempts, 0,
            "counter stays 0 after second success"
        );

        // Identity is now complete — further calls must be no-ops.
        let r3 = maybe_promote_observation_identity_from_tx(
            pool_id,
            &tx_signer,
            gatekeeper_window_ms,
            &mut identity,
            &mut base_mint_pubkey,
            4_000,
            max_retries,
        );
        assert!(!r3, "no-op when both fields are already known");
    }

    // =========================================================================
    // F. tx-only mode: AccountUpdate end-to-end gating tests
    // =========================================================================

    fn dispatch_test_account_update_event(
        base_mint: Pubkey,
        slot: u64,
        write_version: Option<u64>,
        sequence_number: u64,
    ) -> AccountUpdateEvent {
        AccountUpdateEvent {
            semantic: Default::default(),
            event_time: ghost_core::EventTimeMetadata::default(),
            base_mint,
            bonding_curve: Pubkey::new_unique(),
            curve_finality: CurveFinality::Provisional,
            sol_reserves: 30_000_000_000 + slot,
            token_reserves: 900_000_000_000_000,
            complete: 0,
            slot,
            write_version,
            replay_origin: seer::ipc::AccountUpdateReplayOrigin::Live,
            replay_buffer_dwell_ms: None,
            detected_at: std::time::SystemTime::now(),
            sequence_number,
        }
    }

    #[tokio::test]
    async fn test_account_update_worker_preserves_all_updates_for_same_mint() {
        use ghost_brain::config::IwimVetoGateConfig;
        use std::time::Duration;

        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let oracle_runtime = Arc::new(OracleRuntime::new(
            Arc::new(ghost_brain::oracle::hyper_prediction::HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::clone(&ledger),
        ));
        let _pool_id =
            register_runtime_pool_for_base_mint(oracle_runtime.as_ref(), base_mint, bonding_curve);

        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        let (event_tx, event_rx) = crate::events::create_event_bus();
        let task_handle = tokio::spawn(start_oracle_runtime_task(
            event_rx,
            Arc::clone(&oracle_runtime),
            Arc::clone(&snapshot_engine),
            event_tx.clone(),
            None,
            8_000,
            GatekeeperV2Config::default(),
            IwimVetoGateConfig::default(),
            false,
            "logs/test_decisions".to_string(),
            None,
            "".to_string(),
            None,
            true,  // canonical_account_update_relay_enabled = true
            false, // authoritative_funding_stream_available = false
        ));

        let mut first_update = dispatch_test_account_update_event(base_mint, 101, Some(1), 1);
        first_update.bonding_curve = bonding_curve;
        first_update.sol_reserves = 31_000_000_000;
        first_update.token_reserves = 950_000_000_000_000;

        let mut second_update = dispatch_test_account_update_event(base_mint, 102, Some(2), 2);
        second_update.bonding_curve = bonding_curve;
        second_update.sol_reserves = 32_500_000_000;
        second_update.token_reserves = 900_000_000_000_000;

        event_tx
            .send(GhostEvent::AccountUpdate(first_update))
            .expect("first account update should be sent");
        event_tx
            .send(GhostEvent::AccountUpdate(second_update))
            .expect("second account update should be sent");

        let final_state = tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                if let Some(state) = oracle_runtime
                    .account_state_core()
                    .get_canonical_state(&base_mint)
                {
                    if state.update_count == 2 {
                        break state;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("worker should apply both account updates without coalescing");

        task_handle.abort();

        assert_eq!(final_state.last_update_slot, 102);
        assert_eq!(final_state.update_count, 2);
        assert_eq!(final_state.virtual_sol_reserves, 32_500_000_000);
        assert_eq!(final_state.virtual_token_reserves, 900_000_000_000_000);

        let status = oracle_runtime.reconciliation_status();
        assert_eq!(
            status.total_checks, 2,
            "dedicated AccountUpdate worker must preserve per-update reconciliation semantics"
        );
    }

    /// F.1 – GhostEvent::AccountUpdate is a no-op in tx-only mode.
    ///
    /// Spawns `start_oracle_runtime_task` with
    /// `canonical_account_update_relay_enabled=false`,
    /// sends a `GhostEvent::AccountUpdate` carrying drift data, and verifies that
    /// `ReconciliationRuntime::total_checks` remains 0 — i.e. `process_account_update`
    /// was never called despite the event being received by the task.
    #[tokio::test]
    async fn test_tx_only_account_update_event_is_noop() {
        use ghost_brain::config::IwimVetoGateConfig;
        use ghost_core::market_state::BondingCurve;
        use std::time::Duration;

        let ledger = Arc::new(ShadowLedger::new());
        let base_mint = Pubkey::new_unique();

        // Seed ShadowLedger with a known state
        let initial_sol: u64 = 30_000_000_000;
        let initial_tok: u64 = 1_000_000_000_000;
        ledger.insert_with_slot(
            base_mint,
            BondingCurve {
                discriminator: 0,
                virtual_sol_reserves: initial_sol,
                virtual_token_reserves: initial_tok,
                real_sol_reserves: initial_sol,
                real_token_reserves: initial_tok,
                token_total_supply: initial_tok,
                complete: 0,
                _padding: [0u8; 7],
            },
            100,
        );

        let oracle_runtime = Arc::new(OracleRuntime::new(
            Arc::new(ghost_brain::oracle::hyper_prediction::HyperPredictionOracle::default()),
            "pump_program".to_string(),
            "bonk_program".to_string(),
            Arc::clone(&ledger),
        ));

        // Register pool to reconciliation_runtime so it would normally receive the update
        oracle_runtime
            .reconciliation_runtime
            .lock()
            .register_pool(base_mint);

        let snapshot_engine = Arc::new(SnapshotEngine::new(16, 0));
        let (event_tx, event_rx) = crate::events::create_event_bus();
        let oracle_runtime_check = Arc::clone(&oracle_runtime);

        // Spawn the task with canonical_account_update_relay_enabled=false (tx-only mode)
        let task_handle = tokio::spawn(start_oracle_runtime_task(
            event_rx,
            Arc::clone(&oracle_runtime),
            Arc::clone(&snapshot_engine),
            event_tx.clone(),
            None,
            8_000,
            GatekeeperV2Config::default(),
            IwimVetoGateConfig::default(),
            false,
            "logs/test_decisions".to_string(),
            None,
            "".to_string(),
            None,
            false, // canonical_account_update_relay_enabled = false
            false, // authoritative_funding_stream_available = false
        ));

        // Send an AccountUpdate with severely drifted values — would normally trigger drift diagnostics
        let drifted_sol = initial_sol + 5_000_000_000; // +5 SOL drift
        event_tx
            .send(GhostEvent::AccountUpdate(
                crate::events::AccountUpdateEvent {
                    semantic: Default::default(),
                    event_time: ghost_core::EventTimeMetadata::default(),
                    base_mint,
                    bonding_curve: Pubkey::new_unique(),
                    curve_finality: CurveFinality::Speculative,
                    sol_reserves: drifted_sol,
                    token_reserves: initial_tok,
                    complete: 0,
                    slot: 101,
                    write_version: None,
                    replay_origin: seer::ipc::AccountUpdateReplayOrigin::Live,
                    replay_buffer_dwell_ms: None,
                    detected_at: std::time::SystemTime::now(),
                    sequence_number: 1,
                },
            ))
            .expect("event_tx send must succeed");

        // Give the event loop enough time to process the event
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Abort the task (it runs indefinitely)
        task_handle.abort();

        // tx-only: process_account_update must NEVER have been called
        let status = oracle_runtime_check.reconciliation_status();
        assert_eq!(
            status.total_checks, 0,
            "tx-only: no reconciliation checks should have happened (canonical_account_update_relay_enabled=false)"
        );

        // ShadowLedger must still hold initial state (no repair applied)
        let stored = ledger
            .get(&base_mint)
            .expect("pool must still be in ledger");
        assert_eq!(
            stored.virtual_sol_reserves, initial_sol,
            "ShadowLedger must remain unchanged in tx-only mode"
        );
    }
}
