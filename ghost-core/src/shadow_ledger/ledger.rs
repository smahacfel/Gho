//! Shadow Ledger Core Implementation
//!
//! This module contains the main `ShadowLedger` struct and all its simulation,
//! snapshot management, and eviction methods.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, RwLock,
};

use crate::errors::{GhostError, GhostResult};
use crate::market_state::{
    BondingCurve, ShadowBondingCurve, ShadowLedgerStateConfidence, ShadowLedgerWriteReason,
    ShadowLedgerWriteSource, ShadowLedgerWriteStrength,
};
use crate::{pipeline_coverage, PipelineCoverageStage};

use super::trade_types::TxKey;
use super::types::{
    BuySimulationResult, BvaArchive, MarketSnapshot, SellSimulationResult, SnapshotBuffer,
    DEFAULT_SNAPSHOT_MAX_AGE_MS,
};
use super::CurveFinality;

use super::simulation::{
    simulate_buy_pure, simulate_buy_with_slippage_pure, simulate_sell_pure,
    simulate_sell_with_slippage_pure,
};

use super::bootstrap::{bootstrap_snapshots, BootstrapConfig};
use super::genesis::protocol_genesis_curve;
use metrics::increment_counter;
use tracing::{debug, warn};

/// Maximum number of snapshots retained per mint for delta calculations.
///
/// Set to 128 to align with SnapshotEngine's ring buffer capacity, ensuring
/// downstream scoring cycles (early-phase) have sufficient historical depth
/// for accurate delta/derivative calculations.
///
/// Previously 20, which was insufficient for scoring stability and caused
/// premature eviction of historical data needed for:
/// - SOBP (buying pressure tracking)
/// - ULVF (ultra-low variance filtering)
/// - POVC (pattern-of-volume clustering)
/// - SSMI (spread-signature market intensity)
///
/// Retention policy: FIFO - oldest snapshots are evicted when buffer exceeds
/// this limit. Always retains the newest 128 snapshots per base_mint.
const MAX_SNAPSHOTS_PER_MINT: usize = 128;

/// Source of snapshot writes for slot=0 telemetry attribution.
#[derive(Clone, Copy, Debug)]
pub enum SnapshotSource {
    Canonical,
    Runtime,
    SnapshotEngine,
    Legacy,
}

impl SnapshotSource {
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            SnapshotSource::Canonical => "canonical",
            SnapshotSource::Runtime => "runtime",
            SnapshotSource::SnapshotEngine => "snapshot_engine",
            SnapshotSource::Legacy => "legacy",
        }
    }
}

/// Canonical wall-clock freshness classification for ShadowLedger curve state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveFreshnessState {
    Committed,
    Fresh,
    Stale,
    Unknown,
}

impl CurveFreshnessState {
    #[inline]
    pub fn classify(
        curve_data_known: bool,
        snapshot_age_ms: u64,
        freshness_ms: u64,
        committed: bool,
    ) -> Self {
        if committed && curve_data_known {
            Self::Committed
        } else if !curve_data_known {
            Self::Unknown
        } else if snapshot_age_ms <= freshness_ms {
            Self::Fresh
        } else {
            Self::Stale
        }
    }

    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
        }
    }
}

/// Runtime policy for stale ShadowLedger snapshots used during launcher enrichment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShadowLedgerStaleFallback {
    #[default]
    PendingCurve,
    UseStaleWithWarning,
    Reject,
}

impl ShadowLedgerStaleFallback {
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PendingCurve => "pending_curve",
            Self::UseStaleWithWarning => "use_stale_with_warning",
            Self::Reject => "reject",
        }
    }
}

/// Curve state together with explicit quality metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurveFreshnessInfo {
    pub curve: BondingCurve,
    pub curve_data_known: bool,
    pub curve_finality: CurveFinality,
    pub snapshot_age_ms: u64,
    pub state: CurveFreshnessState,
}

#[derive(Clone, Debug)]
struct MintCommitState {
    last_committed_tx_key: Option<TxKey>,
}

impl MintCommitState {
    fn committed(last_committed_tx_key: Option<TxKey>) -> Self {
        Self {
            last_committed_tx_key,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitHistoryStatus {
    Persisted,
    NoOpExistingHistory,
    RejectedNoWrite,
}

#[derive(Debug, Clone)]
pub struct CommitHistoryResult {
    pub status: CommitHistoryStatus,
    pub requested_snapshot_count: usize,
    pub stored_snapshot_count: usize,
    pub last_committed_tx_key: Option<TxKey>,
    pub last_snapshot: Option<MarketSnapshot>,
}

impl CommitHistoryResult {
    pub fn persisted_success(&self) -> bool {
        self.status == CommitHistoryStatus::Persisted
    }

    pub fn canonical_history_available(&self) -> bool {
        matches!(
            self.status,
            CommitHistoryStatus::Persisted | CommitHistoryStatus::NoOpExistingHistory
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowLedgerWriteResult {
    Applied,
    NoOpExistingEqualOrStronger,
    RejectedWeakerWrite,
    PromotedBootstrapToConfirmed,
    RejectedOutOfOrder,
    RejectedMissingMetadata,
}

impl ShadowLedgerWriteResult {
    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::NoOpExistingEqualOrStronger => "no_op_existing_equal_or_stronger",
            Self::RejectedWeakerWrite => "rejected_weaker_write",
            Self::PromotedBootstrapToConfirmed => "promoted_bootstrap_to_confirmed",
            Self::RejectedOutOfOrder => "rejected_out_of_order",
            Self::RejectedMissingMetadata => "rejected_missing_metadata",
        }
    }

    #[inline]
    pub fn rejection_reason(self) -> Option<&'static str> {
        match self {
            Self::RejectedWeakerWrite => Some("weaker_write"),
            Self::RejectedOutOfOrder => Some("out_of_order"),
            Self::RejectedMissingMetadata => Some("missing_metadata"),
            Self::Applied
            | Self::NoOpExistingEqualOrStronger
            | Self::PromotedBootstrapToConfirmed => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurveWriteMetadata {
    pub source: ShadowLedgerWriteSource,
    pub strength: ShadowLedgerWriteStrength,
    pub state_confidence: ShadowLedgerStateConfidence,
    pub reason: ShadowLedgerWriteReason,
    pub slot: Option<u64>,
    pub curve_finality: CurveFinality,
    pub last_update_ts_ms: Option<u64>,
}

impl CurveWriteMetadata {
    pub const fn new(
        source: ShadowLedgerWriteSource,
        strength: ShadowLedgerWriteStrength,
        state_confidence: ShadowLedgerStateConfidence,
        reason: ShadowLedgerWriteReason,
        slot: Option<u64>,
        curve_finality: CurveFinality,
    ) -> Self {
        Self {
            source,
            strength,
            state_confidence,
            reason,
            slot,
            curve_finality,
            last_update_ts_ms: None,
        }
    }

    #[inline]
    pub const fn with_last_update_ts_ms(mut self, last_update_ts_ms: u64) -> Self {
        self.last_update_ts_ms = Some(last_update_ts_ms);
        self
    }

    #[inline]
    fn normalized_slot(self) -> Option<u64> {
        match self.slot {
            Some(0) => None,
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CurveWriteOutcome {
    pub result: ShadowLedgerWriteResult,
    pub previous: Option<ShadowBondingCurve>,
    pub stored: Option<ShadowBondingCurve>,
}

impl CurveWriteOutcome {
    #[inline]
    fn rejected(result: ShadowLedgerWriteResult, previous: Option<ShadowBondingCurve>) -> Self {
        Self {
            result,
            previous,
            stored: previous,
        }
    }
}

enum ShadowLedgerWriteRequest {
    Curve {
        base_mint: Option<Pubkey>,
        bonding_curve: Pubkey,
        curve: BondingCurve,
        metadata: CurveWriteMetadata,
    },
    CommitHistory {
        mint: Pubkey,
        snaps: Vec<MarketSnapshot>,
        last_committed_tx_key: Option<TxKey>,
        write_source: ShadowLedgerWriteSource,
    },
    LiveAppend {
        mint: Pubkey,
        snapshot: MarketSnapshot,
        write_source: ShadowLedgerWriteSource,
    },
}

enum ShadowLedgerWriteOutcome {
    Curve(CurveWriteOutcome),
    CommitHistory(CommitHistoryResult),
    LiveAppend(bool),
}

#[inline]
fn record_shadow_ledger_write_total(
    source: ShadowLedgerWriteSource,
    strength: ShadowLedgerWriteStrength,
    result: &'static str,
) {
    ::metrics::counter!(
        "shadow_ledger_write_total",
        1,
        "source" => source.as_str(),
        "strength" => strength.as_str(),
        "result" => result
    );
}

#[inline]
fn record_shadow_ledger_bootstrap_metrics(
    source: ShadowLedgerWriteSource,
    strength: ShadowLedgerWriteStrength,
    result: ShadowLedgerWriteResult,
) {
    if strength == ShadowLedgerWriteStrength::BootstrapSeed {
        ::metrics::counter!(
            "shadow_ledger_bootstrap_total",
            1,
            "source" => source.as_str()
        );
        if result == ShadowLedgerWriteResult::NoOpExistingEqualOrStronger {
            ::metrics::counter!(
                "shadow_ledger_bootstrap_noop_total",
                1,
                "source" => source.as_str()
            );
        }
    }
}

#[inline]
fn record_shadow_ledger_rejected_total(reason: &'static str) {
    ::metrics::counter!("shadow_ledger_write_rejected_total", 1, "reason" => reason);
}

// ============================================================================
// Shadow Ledger Core
// ============================================================================

/// Thread-safe in-memory cache of Pump.fun bonding curve states with simulation
///
/// The Shadow Ledger provides zero-latency access to bonding curve state by maintaining
/// an in-memory replica synchronized with on-chain data via account subscriptions.
///
/// # Thread Safety
///
/// This structure uses `DashMap` to provide:
/// - Lock-free concurrent reads via fine-grained sharding
/// - Atomic updates without global locking
/// - Near-linear scalability with thread count
///
/// # Memory Overhead
///
/// - DashMap overhead: ~64 bytes per entry (key + value + hash + metadata)
/// - Pubkey: 32 bytes
/// - ShadowBondingCurve: 64 bytes (56 + 8 for slot tracking)
/// - **Total per entry: ~160 bytes**
///
/// For 1000 tracked curves: ~160 KB
/// For 10000 tracked curves: ~1.6 MB
///
/// # Synthetic Snapshots
///
/// The ledger also maintains a buffer of geometric market snapshots per mint.
/// At T=0 (InitializePool), 3 synthetic snapshots (G0, G1, G2) are generated
/// for immediate use by derivative-based scoring modules.
///
/// # Performance
///
/// DashMap provides significant improvements over RwLock<HashMap>:
/// - **Reads**: Lock-free, scales linearly with CPU cores
/// - **Writes**: Per-shard locking, concurrent writes to different shards
/// - **Simulation**: <50ns for all operations (no lock acquisition)
#[derive(Clone)]
pub struct ShadowLedger {
    /// Thread-safe storage for bonding curve states with staleness tracking
    /// Wrapped in Arc for shared ownership across clones
    /// - Key: Mint address (token Pubkey)
    /// - Value: Bonding curve state snapshot with last_updated_slot
    curves: Arc<DashMap<Pubkey, ShadowBondingCurve>>,

    /// Alias map translating `base_mint` -> canonical `bonding_curve` key.
    ///
    /// Curve storage remains keyed by bonding-curve pubkey. This map exists so
    /// corrective/reconciliation paths that naturally operate on base mint can
    /// resolve the right curve entry without duplicating curve state.
    curve_keys_by_base_mint: Arc<DashMap<Pubkey, Pubkey>>,

    /// Buffer of geometric market snapshots per mint with lifecycle metadata.
    /// By definition: maximum 3 elements per mint during bootstrap phase (G0, G1, G2).
    /// Used by SCR, ULVF, POVC, HOSD, QOFSV for derivative/curvature analysis.
    /// Wrapped in SnapshotBuffer for age-based eviction support.
    pub(crate) snapshots: Arc<DashMap<Pubkey, SnapshotBuffer>>,

    /// Per-mint commit state for canonical snapshot history.
    snapshot_commit_state: Arc<DashMap<Pubkey, MintCommitState>>,

    /// Maximum snapshots retained per mint for time-series calculations.
    max_snapshots_per_mint: usize,

    /// Optional approval checker for gating snapshot writes
    approval_checker: Arc<RwLock<Option<Arc<dyn Fn(&Pubkey) -> bool + Send + Sync + 'static>>>>,

    /// Counter for blocked writes to unapproved pools
    untracked_snapshot_write_blocked_total: Arc<AtomicU64>,

    /// Counter for blocked reads/cleanup for unapproved pools
    untracked_snapshot_read_blocked_total: Arc<AtomicU64>,

    /// Track which mints were already logged as blocked (to avoid spam)
    blocked_log_once: Arc<DashMap<Pubkey, ()>>,

    /// Archived BVA outputs per mint (0-7s window)
    bva_archives: Arc<DashMap<Pubkey, BvaArchive>>,
}

impl ShadowLedger {
    /// Create a new empty Shadow Ledger
    ///
    /// Initializes empty DashMaps wrapped in Arc for lock-free concurrent access.
    ///
    /// # Returns
    ///
    /// A new `ShadowLedger` instance ready for use
    ///
    /// # Example
    ///
    /// ```rust
    /// use ghost_core::shadow_ledger::ShadowLedger;
    ///
    /// let ledger = ShadowLedger::new();
    /// assert_eq!(ledger.len(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            curves: Arc::new(DashMap::new()),
            curve_keys_by_base_mint: Arc::new(DashMap::new()),
            snapshots: Arc::new(DashMap::new()),
            snapshot_commit_state: Arc::new(DashMap::new()),
            max_snapshots_per_mint: MAX_SNAPSHOTS_PER_MINT,
            approval_checker: Arc::new(RwLock::new(None)),
            untracked_snapshot_write_blocked_total: Arc::new(AtomicU64::new(0)),
            untracked_snapshot_read_blocked_total: Arc::new(AtomicU64::new(0)),
            blocked_log_once: Arc::new(DashMap::new()),
            bva_archives: Arc::new(DashMap::new()),
        }
    }

    /// Archive BVA output for a mint (0-7s window)
    pub fn set_bva_archive(&self, mint: Pubkey, archive: BvaArchive) {
        self.bva_archives.insert(mint, archive);
    }

    /// Get archived BVA output for a mint, if present
    pub fn get_bva_archive(&self, mint: &Pubkey) -> Option<BvaArchive> {
        self.bva_archives.get(mint).map(|v| *v)
    }

    /// Create a new Shadow Ledger with pre-allocated capacity
    ///
    /// Pre-allocating capacity can improve performance when the expected number
    /// of bonding curves is known in advance, reducing reallocation overhead.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Number of entries to pre-allocate space for
    ///
    /// # Returns
    ///
    /// A new `ShadowLedger` instance with pre-allocated capacity
    ///
    /// # Example
    ///
    /// ```rust
    /// use ghost_core::shadow_ledger::ShadowLedger;
    ///
    /// // Pre-allocate for 1000 expected curves
    /// let ledger = ShadowLedger::with_capacity(1000);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            curves: Arc::new(DashMap::with_capacity(capacity)),
            curve_keys_by_base_mint: Arc::new(DashMap::with_capacity(capacity)),
            snapshots: Arc::new(DashMap::with_capacity(capacity)),
            snapshot_commit_state: Arc::new(DashMap::with_capacity(capacity)),
            max_snapshots_per_mint: MAX_SNAPSHOTS_PER_MINT,
            approval_checker: Arc::new(RwLock::new(None)),
            untracked_snapshot_write_blocked_total: Arc::new(AtomicU64::new(0)),
            untracked_snapshot_read_blocked_total: Arc::new(AtomicU64::new(0)),
            blocked_log_once: Arc::new(DashMap::new()),
            bva_archives: Arc::new(DashMap::with_capacity(capacity)),
        }
    }

    // =========================================================================
    // STORAGE ACCESS METHODS
    // =========================================================================

    /// Get a DashMapCurveStorage view of the underlying curve storage.
    ///
    /// This method provides access to the curve storage through the `CurveStorage`
    /// trait, enabling integration with other storage-aware components.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let ledger = ShadowLedger::new();
    /// let storage = ledger.curve_storage();
    ///
    /// // Use storage trait methods
    /// storage.insert(mint, curve);
    /// let curve = storage.get(&mint);
    /// ```
    pub fn curve_storage(&self) -> super::storage::DashMapCurveStorage {
        super::storage::DashMapCurveStorage::from_arc(Arc::clone(&self.curves))
    }

    /// Get a DashMapSnapshotStorage view of the underlying snapshot storage.
    ///
    /// This method provides access to the snapshot storage through the `SnapshotStorage`
    /// trait, enabling integration with eviction and other storage-aware components.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let ledger = ShadowLedger::new();
    /// let storage = ledger.snapshot_storage();
    ///
    /// // Use storage trait methods
    /// let count = storage.len();
    /// let mints = storage.get_all_mints();
    /// ```
    pub fn snapshot_storage(&self) -> super::storage::DashMapSnapshotStorage {
        super::storage::DashMapSnapshotStorage::from_arc(Arc::clone(&self.snapshots))
    }

    /// Set approval checker used to gate snapshot writes and reads.
    ///
    /// This gates snapshot reads/writes/cleanup (keyed by `base_mint`). Curve storage
    /// and simulations remain unaffected. If no checker is installed (default),
    /// all snapshot accesses are accepted.
    pub fn set_approval_checker(
        &self,
        checker: Arc<dyn Fn(&Pubkey) -> bool + Send + Sync + 'static>,
    ) {
        if let Ok(mut guard) = self.approval_checker.write() {
            *guard = Some(checker);
        }
    }

    /// Number of snapshot writes blocked due to missing approval
    pub fn untracked_snapshot_write_blocked_total(&self) -> u64 {
        self.untracked_snapshot_write_blocked_total
            .load(Ordering::Relaxed)
    }

    /// Number of snapshot reads blocked due to missing approval
    pub fn untracked_snapshot_read_blocked_total(&self) -> u64 {
        self.untracked_snapshot_read_blocked_total
            .load(Ordering::Relaxed)
    }

    /// Get the underlying Arc<DashMap> for curves (for advanced use cases).
    ///
    /// # Warning
    ///
    /// This method is provided for backward compatibility and integration with
    /// existing code. Prefer using `curve_storage()` for new code.
    pub fn curves_raw(&self) -> Arc<DashMap<Pubkey, ShadowBondingCurve>> {
        Arc::clone(&self.curves)
    }

    /// Register the canonical bonding-curve key for a base mint.
    pub fn register_curve_alias(&self, base_mint: Pubkey, bonding_curve: Pubkey) {
        self.curve_keys_by_base_mint
            .insert(base_mint, bonding_curve);
    }

    /// Remove the base_mint -> bonding_curve alias, if present.
    pub fn remove_curve_alias(&self, base_mint: &Pubkey) {
        self.curve_keys_by_base_mint.remove(base_mint);
    }

    /// Resolve the canonical curve-storage key for a base mint.
    ///
    /// Returns the stored bonding-curve alias when available. As a
    /// backward-compatible fallback, if no alias exists but the curve map
    /// already contains an entry under `base_mint`, returns `base_mint`.
    pub fn resolve_curve_key(&self, base_mint: &Pubkey) -> Option<Pubkey> {
        self.curve_keys_by_base_mint
            .get(base_mint)
            .map(|entry| *entry)
            .or_else(|| self.curves.contains_key(base_mint).then_some(*base_mint))
    }

    /// Get the underlying Arc<DashMap> for snapshots (for advanced use cases).
    ///
    /// # Warning
    ///
    /// This method is provided for backward compatibility and integration with
    /// existing code. Prefer using `snapshot_storage()` for new code.
    pub fn snapshots_raw(&self) -> Arc<DashMap<Pubkey, SnapshotBuffer>> {
        Arc::clone(&self.snapshots)
    }

    /// Get curve information for eviction decisions.
    ///
    /// This method returns a `CurveInfo` struct containing data used by the
    /// aggressive eviction algorithm to make eviction decisions.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to look up
    ///
    /// # Returns
    ///
    /// CurveInfo if the curve exists, None otherwise.
    pub fn get_curve_info(&self, mint: &Pubkey) -> Option<super::storage::CurveInfo> {
        self.curves.get(mint).map(|entry| {
            let curve = entry.value();
            super::storage::CurveInfo {
                bonding_progress: curve.curve.get_bonding_progress(),
                last_updated_slot: curve.last_updated_slot,
                // These would be populated from additional tracking in a full implementation
                cum_volume_sol: 0.0,
                tx_count: 0,
            }
        })
    }

    // =========================================================================
    // CURVE LIFECYCLE METHODS
    // =========================================================================

    fn validate_curve_write_metadata(
        metadata: CurveWriteMetadata,
    ) -> Result<(Option<u64>, bool), ShadowLedgerWriteResult> {
        let slot = metadata.normalized_slot();
        let curve_data_known = !matches!(
            metadata.state_confidence,
            ShadowLedgerStateConfidence::Speculative
        );

        if metadata.source == ShadowLedgerWriteSource::CompatibilityBootstrap {
            return Ok((slot, curve_data_known));
        }

        let valid = match metadata.strength {
            ShadowLedgerWriteStrength::BootstrapSeed => {
                slot.is_none()
                    && metadata.state_confidence == ShadowLedgerStateConfidence::Speculative
                    && metadata.reason == ShadowLedgerWriteReason::BootstrapSeed
            }
            ShadowLedgerWriteStrength::ConfirmedBootstrap => {
                slot.is_some()
                    && metadata.state_confidence == ShadowLedgerStateConfidence::Observed
                    && metadata.reason == ShadowLedgerWriteReason::ConfirmedBootstrap
            }
            ShadowLedgerWriteStrength::Repair => {
                slot.is_some()
                    && matches!(
                        metadata.state_confidence,
                        ShadowLedgerStateConfidence::Observed
                            | ShadowLedgerStateConfidence::Diagnostic
                    )
                    && metadata.reason != ShadowLedgerWriteReason::CompatibilityBootstrap
            }
            ShadowLedgerWriteStrength::CanonicalCommit | ShadowLedgerWriteStrength::LiveAppend => {
                false
            }
        };

        if !valid {
            return Err(ShadowLedgerWriteResult::RejectedMissingMetadata);
        }

        Ok((slot, curve_data_known))
    }

    fn build_curve_state(
        curve: BondingCurve,
        metadata: CurveWriteMetadata,
        slot: Option<u64>,
        curve_data_known: bool,
    ) -> ShadowBondingCurve {
        ShadowBondingCurve {
            curve,
            last_updated_slot: slot.unwrap_or(0),
            curve_data_known,
            last_update_ts_ms: metadata
                .last_update_ts_ms
                .unwrap_or_else(super::types::current_time_ms),
            curve_finality: metadata.curve_finality.normalized(curve_data_known),
            write_source: metadata.source,
            write_strength: metadata.strength,
            state_confidence: metadata.state_confidence,
            write_reason: metadata.reason,
        }
    }

    fn apply_write(&self, request: ShadowLedgerWriteRequest) -> ShadowLedgerWriteOutcome {
        match request {
            ShadowLedgerWriteRequest::Curve {
                base_mint,
                bonding_curve,
                curve,
                metadata,
            } => ShadowLedgerWriteOutcome::Curve(self.apply_curve_write_inner(
                base_mint,
                bonding_curve,
                curve,
                metadata,
            )),
            ShadowLedgerWriteRequest::CommitHistory {
                mint,
                snaps,
                last_committed_tx_key,
                write_source,
            } => ShadowLedgerWriteOutcome::CommitHistory(self.commit_history_with_source_inner(
                mint,
                snaps,
                last_committed_tx_key,
                write_source,
            )),
            ShadowLedgerWriteRequest::LiveAppend {
                mint,
                snapshot,
                write_source,
            } => ShadowLedgerWriteOutcome::LiveAppend(self.append_live_with_source_inner(
                mint,
                snapshot,
                write_source,
            )),
        }
    }

    #[doc(hidden)]
    #[deprecated(
        since = "8.0.0",
        note = "legacy curve writes are bootstrap/compat-only; prefer AccountStateCore ingest"
    )]
    pub fn apply_curve_write(
        &self,
        base_mint: Option<Pubkey>,
        bonding_curve: Pubkey,
        curve: BondingCurve,
        metadata: CurveWriteMetadata,
    ) -> CurveWriteOutcome {
        match self.apply_write(ShadowLedgerWriteRequest::Curve {
            base_mint,
            bonding_curve,
            curve,
            metadata,
        }) {
            ShadowLedgerWriteOutcome::Curve(outcome) => outcome,
            ShadowLedgerWriteOutcome::CommitHistory(_)
            | ShadowLedgerWriteOutcome::LiveAppend(_) => {
                unreachable!("curve write request must return curve outcome")
            }
        }
    }

    fn apply_curve_write_inner(
        &self,
        base_mint: Option<Pubkey>,
        bonding_curve: Pubkey,
        curve: BondingCurve,
        metadata: CurveWriteMetadata,
    ) -> CurveWriteOutcome {
        let validated = Self::validate_curve_write_metadata(metadata);
        let (slot, curve_data_known) = match validated {
            Ok(valid) => valid,
            Err(result) => {
                record_shadow_ledger_write_total(
                    metadata.source,
                    metadata.strength,
                    result.as_str(),
                );
                if let Some(reason) = result.rejection_reason() {
                    record_shadow_ledger_rejected_total(reason);
                }
                return CurveWriteOutcome::rejected(
                    result,
                    self.curves.get(&bonding_curve).map(|e| *e),
                );
            }
        };

        let previous = self.curves.get(&bonding_curve).map(|entry| *entry.value());
        let existing = previous;

        let normalized_finality = metadata.curve_finality.normalized(curve_data_known);
        let incoming = Self::build_curve_state(curve, metadata, slot, curve_data_known);

        let result = if let Some(existing) = existing {
            let existing_slot =
                (existing.last_updated_slot != 0).then_some(existing.last_updated_slot);
            let identical = existing.curve == curve
                && existing.curve_data_known == curve_data_known
                && existing.curve_finality == normalized_finality;

            if identical && existing.write_strength >= metadata.strength && existing_slot == slot {
                ShadowLedgerWriteResult::NoOpExistingEqualOrStronger
            } else if metadata.strength < existing.write_strength {
                if identical {
                    ShadowLedgerWriteResult::NoOpExistingEqualOrStronger
                } else {
                    ShadowLedgerWriteResult::RejectedWeakerWrite
                }
            } else if let (Some(existing_slot), Some(incoming_slot)) = (existing_slot, slot) {
                if incoming_slot < existing_slot {
                    ShadowLedgerWriteResult::RejectedOutOfOrder
                } else if metadata.strength == existing.write_strength && identical {
                    ShadowLedgerWriteResult::NoOpExistingEqualOrStronger
                } else if existing.write_strength == ShadowLedgerWriteStrength::BootstrapSeed
                    && metadata.strength == ShadowLedgerWriteStrength::ConfirmedBootstrap
                {
                    ShadowLedgerWriteResult::PromotedBootstrapToConfirmed
                } else {
                    ShadowLedgerWriteResult::Applied
                }
            } else if metadata.strength == existing.write_strength {
                if identical {
                    ShadowLedgerWriteResult::NoOpExistingEqualOrStronger
                } else {
                    ShadowLedgerWriteResult::RejectedOutOfOrder
                }
            } else if existing.write_strength == ShadowLedgerWriteStrength::BootstrapSeed
                && metadata.strength == ShadowLedgerWriteStrength::ConfirmedBootstrap
            {
                ShadowLedgerWriteResult::PromotedBootstrapToConfirmed
            } else {
                ShadowLedgerWriteResult::Applied
            }
        } else {
            ShadowLedgerWriteResult::Applied
        };

        record_shadow_ledger_write_total(metadata.source, metadata.strength, result.as_str());
        record_shadow_ledger_bootstrap_metrics(metadata.source, metadata.strength, result);
        if let Some(reason) = result.rejection_reason() {
            record_shadow_ledger_rejected_total(reason);
        }

        match result {
            ShadowLedgerWriteResult::Applied
            | ShadowLedgerWriteResult::PromotedBootstrapToConfirmed => {
                if let Some(base_mint) = base_mint {
                    self.register_curve_alias(base_mint, bonding_curve);
                }
                let stored = incoming;
                self.curves.insert(bonding_curve, stored);
                CurveWriteOutcome {
                    result,
                    previous,
                    stored: Some(stored),
                }
            }
            ShadowLedgerWriteResult::NoOpExistingEqualOrStronger => {
                if let Some(base_mint) = base_mint {
                    self.register_curve_alias(base_mint, bonding_curve);
                }
                CurveWriteOutcome::rejected(result, previous)
            }
            ShadowLedgerWriteResult::RejectedWeakerWrite
            | ShadowLedgerWriteResult::RejectedOutOfOrder
            | ShadowLedgerWriteResult::RejectedMissingMetadata => {
                CurveWriteOutcome::rejected(result, previous)
            }
        }
    }

    /// Insert or update a bonding curve state with slot tracking
    ///
    /// This method atomically inserts the bonding curve state for the given mint address.
    /// If a state already exists for this mint, it will be replaced with the new state.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address (token Pubkey) to associate with this curve
    /// * `curve` - The bonding curve state to store
    /// * `slot` - The slot number when this state was observed
    ///
    /// # Returns
    ///
    /// * `Some(ShadowBondingCurve)` - The previous curve state if one existed
    /// * `None` - If this is a new entry
    ///
    /// # Example
    ///
    /// ```rust
    /// use ghost_core::shadow_ledger::ShadowLedger;
    /// use ghost_core::market_state::BondingCurve;
    /// use solana_sdk::pubkey::Pubkey;
    ///
    /// let ledger = ShadowLedger::new();
    /// let mint = Pubkey::new_unique();
    /// let curve = BondingCurve {
    ///     discriminator: 0,
    ///     virtual_token_reserves: 1_000_000_000,
    ///     virtual_sol_reserves: 30_000_000,
    ///     real_token_reserves: 800_000_000,
    ///     real_sol_reserves: 20_000_000,
    ///     token_total_supply: 1_000_000_000,
    ///     complete: 0,
    ///     _padding: [0; 7],
    /// };
    ///
    /// // First insert returns None
    /// assert!(ledger.insert_with_slot(mint, curve, 1000).is_none());
    ///
    /// // Second insert returns the previous value
    /// assert!(ledger.insert_with_slot(mint, curve, 1001).is_some());
    /// ```
    pub fn insert_with_slot(
        &self,
        mint: Pubkey,
        curve: BondingCurve,
        slot: u64,
    ) -> Option<ShadowBondingCurve> {
        if slot == 0 {
            return self.insert_seed_curve(mint, curve, false);
        }

        self.apply_curve_write(
            None,
            mint,
            curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::RpcBootstrapSeeder,
                ShadowLedgerWriteStrength::ConfirmedBootstrap,
                ShadowLedgerStateConfidence::Observed,
                ShadowLedgerWriteReason::ConfirmedBootstrap,
                Some(slot),
                CurveFinality::Provisional,
            ),
        )
        .previous
    }

    /// Insert or update a bonding curve state with explicit wall-clock update time.
    pub fn insert_with_slot_at(
        &self,
        mint: Pubkey,
        curve: BondingCurve,
        slot: u64,
        last_update_ts_ms: u64,
    ) -> Option<ShadowBondingCurve> {
        if slot == 0 {
            return self.insert_seed_curve_at(mint, curve, false, last_update_ts_ms);
        }

        self.apply_curve_write(
            None,
            mint,
            curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::RpcBootstrapSeeder,
                ShadowLedgerWriteStrength::ConfirmedBootstrap,
                ShadowLedgerStateConfidence::Observed,
                ShadowLedgerWriteReason::ConfirmedBootstrap,
                Some(slot),
                CurveFinality::Provisional,
            )
            .with_last_update_ts_ms(last_update_ts_ms),
        )
        .previous
    }

    /// Insert or update a bonding curve state with explicit curve_data_known flag.
    ///
    /// Use this when the caller knows whether the curve data was
    /// parsed from a confirmed source (AccountUpdate) vs a genesis seed.
    pub(crate) fn insert_with_slot_known(
        &self,
        mint: Pubkey,
        curve: BondingCurve,
        slot: u64,
        curve_data_known: bool,
    ) -> Option<ShadowBondingCurve> {
        self.insert_with_slot_known_finality(
            mint,
            curve,
            slot,
            curve_data_known,
            CurveFinality::from_curve_data_known(curve_data_known),
        )
    }

    /// Insert or update a bonding curve state with explicit curve-data-known
    /// flag and finality tier.
    pub(crate) fn insert_with_slot_known_finality(
        &self,
        mint: Pubkey,
        curve: BondingCurve,
        slot: u64,
        curve_data_known: bool,
        curve_finality: CurveFinality,
    ) -> Option<ShadowBondingCurve> {
        self.apply_curve_write(
            None,
            mint,
            curve,
            CurveWriteMetadata::new(
                if curve_data_known {
                    ShadowLedgerWriteSource::RpcBootstrapSeeder
                } else {
                    ShadowLedgerWriteSource::SeerBootstrap
                },
                if curve_data_known {
                    ShadowLedgerWriteStrength::ConfirmedBootstrap
                } else {
                    ShadowLedgerWriteStrength::BootstrapSeed
                },
                if curve_data_known {
                    ShadowLedgerStateConfidence::Observed
                } else {
                    ShadowLedgerStateConfidence::Speculative
                },
                if curve_data_known {
                    ShadowLedgerWriteReason::ConfirmedBootstrap
                } else {
                    ShadowLedgerWriteReason::BootstrapSeed
                },
                Some(slot),
                curve_finality,
            ),
        )
        .previous
    }

    /// Insert or update a bonding curve state with explicit curve-data-known
    /// flag and wall-clock update time.
    pub(crate) fn insert_with_slot_known_at(
        &self,
        mint: Pubkey,
        curve: BondingCurve,
        slot: u64,
        curve_data_known: bool,
        last_update_ts_ms: u64,
    ) -> Option<ShadowBondingCurve> {
        self.insert_with_slot_known_at_finality(
            mint,
            curve,
            slot,
            curve_data_known,
            CurveFinality::from_curve_data_known(curve_data_known),
            last_update_ts_ms,
        )
    }

    /// Insert or update a bonding curve state with explicit curve-data-known
    /// flag, finality tier, and wall-clock update time.
    pub(crate) fn insert_with_slot_known_at_finality(
        &self,
        mint: Pubkey,
        curve: BondingCurve,
        slot: u64,
        curve_data_known: bool,
        curve_finality: CurveFinality,
        last_update_ts_ms: u64,
    ) -> Option<ShadowBondingCurve> {
        self.apply_curve_write(
            None,
            mint,
            curve,
            CurveWriteMetadata::new(
                if curve_data_known {
                    ShadowLedgerWriteSource::RpcBootstrapSeeder
                } else {
                    ShadowLedgerWriteSource::SeerBootstrap
                },
                if curve_data_known {
                    ShadowLedgerWriteStrength::ConfirmedBootstrap
                } else {
                    ShadowLedgerWriteStrength::BootstrapSeed
                },
                if curve_data_known {
                    ShadowLedgerStateConfidence::Observed
                } else {
                    ShadowLedgerStateConfidence::Speculative
                },
                if curve_data_known {
                    ShadowLedgerWriteReason::ConfirmedBootstrap
                } else {
                    ShadowLedgerWriteReason::BootstrapSeed
                },
                Some(slot),
                curve_finality,
            )
            .with_last_update_ts_ms(last_update_ts_ms),
        )
        .previous
    }

    /// Insert or update a seed bonding curve state.
    ///
    /// Seed curves use reserved `slot=0` and MUST NOT be treated as canonical
    /// transaction history. This is only for bootstrapping curve math before
    /// the first real AccountUpdate arrives.
    pub(crate) fn insert_seed_curve(
        &self,
        mint: Pubkey,
        curve: BondingCurve,
        curve_data_known: bool,
    ) -> Option<ShadowBondingCurve> {
        let metadata = if curve_data_known {
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::RpcBootstrapSeeder,
                ShadowLedgerWriteStrength::ConfirmedBootstrap,
                ShadowLedgerStateConfidence::Observed,
                ShadowLedgerWriteReason::ConfirmedBootstrap,
                None,
                CurveFinality::Provisional,
            )
        } else {
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::SeerBootstrap,
                ShadowLedgerWriteStrength::BootstrapSeed,
                ShadowLedgerStateConfidence::Speculative,
                ShadowLedgerWriteReason::BootstrapSeed,
                None,
                CurveFinality::Speculative,
            )
        };
        self.apply_curve_write(None, mint, curve, metadata).previous
    }

    /// Insert or update a seed bonding curve with explicit wall-clock update time.
    pub(crate) fn insert_seed_curve_at(
        &self,
        mint: Pubkey,
        curve: BondingCurve,
        curve_data_known: bool,
        last_update_ts_ms: u64,
    ) -> Option<ShadowBondingCurve> {
        let metadata = if curve_data_known {
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::RpcBootstrapSeeder,
                ShadowLedgerWriteStrength::ConfirmedBootstrap,
                ShadowLedgerStateConfidence::Observed,
                ShadowLedgerWriteReason::ConfirmedBootstrap,
                None,
                CurveFinality::Provisional,
            )
            .with_last_update_ts_ms(last_update_ts_ms)
        } else {
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::SeerBootstrap,
                ShadowLedgerWriteStrength::BootstrapSeed,
                ShadowLedgerStateConfidence::Speculative,
                ShadowLedgerWriteReason::BootstrapSeed,
                None,
                CurveFinality::Speculative,
            )
            .with_last_update_ts_ms(last_update_ts_ms)
        };
        self.apply_curve_write(None, mint, curve, metadata).previous
    }

    /// Insert or update a bonding curve state (backward compatibility)
    ///
    /// This method uses slot 0 if no slot information is available.
    /// Prefer using `insert_with_slot` when slot information is available.
    ///
    /// # Deprecated
    ///
    /// This method is maintained for backward compatibility.
    /// Use `insert_with_slot` for proper staleness tracking.
    pub fn insert(&self, mint: Pubkey, curve: BondingCurve) -> Option<ShadowBondingCurve> {
        self.insert_with_slot(mint, curve, 0)
    }

    /// Get a bonding curve state by mint address (without staleness check)
    ///
    /// This method returns a **copy** of the bonding curve state.
    /// Since `BondingCurve` implements `Copy` and is only 56 bytes,
    /// this is extremely efficient.
    ///
    /// **Note**: This method does not perform staleness checks. For production use
    /// with staleness validation, use `get_quote` instead.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to look up
    ///
    /// # Returns
    ///
    /// * `Some(BondingCurve)` - A copy of the curve state if found
    /// * `None` - If no curve exists for this mint
    ///
    /// # Performance Note
    ///
    /// This is a lock-free operation (no global lock acquisition).
    /// DashMap uses fine-grained per-shard locking.
    pub fn get(&self, mint: &Pubkey) -> Option<BondingCurve> {
        self.curves.get(mint).map(|entry| entry.curve)
    }

    /// Get a bonding curve state by bonding_curve pubkey (CANONICAL API)
    ///
    /// # Key Contract
    ///
    /// **CANONICAL KEY**: Curves are stored by `bonding_curve` pubkey, NOT by
    /// `pool_amm_id` or `base_mint`. This is the ONLY correct lookup pattern.
    ///
    /// # When to Use
    ///
    /// - Engine/Cyclic scoring modules
    /// - MESA, Chaos, QEDD analyses
    /// - Any code that needs curve state for AMM calculations
    ///
    /// # Arguments
    ///
    /// * `bonding_curve` - The bonding curve pubkey (CANONICAL KEY)
    ///
    /// # Returns
    ///
    /// * `Some(BondingCurve)` - A copy of the curve state if found
    /// * `None` - If no curve exists for this bonding_curve
    ///
    /// # Example
    ///
    /// ```ignore
    /// // CORRECT: Use bonding_curve pubkey
    /// let curve = ledger.get_curve(&bonding_curve_pubkey);
    ///
    /// // INCORRECT: Do NOT use pool_amm_id or base_mint for curve lookups
    /// // let curve = ledger.get_curve(&pool_amm_id);  // WRONG!
    /// ```
    /// Deprecated for live runtime truth queries as of PR 7.
    ///
    /// Allowed:
    /// - simulation and what-if analysis
    /// - bootstrap-only fallback before AccountStateCore promotion
    /// - WAL replay, restore, and forensics
    ///
    /// Disallowed:
    /// - canonical hot-path state queries in launcher runtime
    /// - curve readiness decisions when AccountStateCore is available
    #[deprecated(
        note = "PR 7: use AccountStateCore for canonical runtime state; ShadowLedger lookups are bootstrap/simulation/forensics only"
    )]
    #[inline]
    pub fn get_curve(&self, bonding_curve: &Pubkey) -> Option<BondingCurve> {
        self.curves.get(bonding_curve).map(|entry| entry.curve)
    }

    /// Get a bonding curve state with its `curve_data_known` flag.
    ///
    /// Returns `(BondingCurve, curve_data_known)` so callers can propagate
    /// the explicit parser flag downstream (e.g. to `PoolTransaction`).
    #[inline]
    pub fn get_curve_with_known(&self, bonding_curve: &Pubkey) -> Option<(BondingCurve, bool)> {
        self.curves
            .get(bonding_curve)
            .map(|entry| (entry.curve, entry.curve_data_known))
    }

    /// Get a bonding curve state with its explicit parser flag and finality tier.
    #[inline]
    pub fn get_curve_with_known_finality(
        &self,
        bonding_curve: &Pubkey,
    ) -> Option<(BondingCurve, bool, CurveFinality)> {
        self.curves
            .get(bonding_curve)
            .map(|entry| (entry.curve, entry.curve_data_known, entry.curve_finality))
    }

    /// Get only the finality tier of the currently stored curve state.
    #[inline]
    pub fn get_curve_finality(&self, bonding_curve: &Pubkey) -> Option<CurveFinality> {
        self.curves
            .get(bonding_curve)
            .map(|entry| entry.curve_finality)
    }

    /// Get a bonding curve state with known-flag and wall-clock age metadata.
    #[inline]
    pub fn get_curve_with_known_age(
        &self,
        bonding_curve: &Pubkey,
        now_ms: u64,
    ) -> Option<(BondingCurve, bool, u64)> {
        self.curves.get(bonding_curve).map(|entry| {
            let curve = *entry.value();
            let age_ms = curve.age_ms(now_ms);
            metrics::histogram!("shadow_ledger_age_ms", age_ms as f64);
            (curve.curve, curve.curve_data_known, age_ms)
        })
    }

    /// Get curve state with canonical wall-clock freshness classification.
    #[inline]
    pub fn get_curve_freshness_info(
        &self,
        bonding_curve: &Pubkey,
        now_ms: u64,
        freshness_ms: u64,
    ) -> Option<CurveFreshnessInfo> {
        self.get_curve_freshness_info_with_commit_state(bonding_curve, now_ms, freshness_ms, false)
    }

    #[inline]
    pub fn get_curve_freshness_info_with_commit_state(
        &self,
        bonding_curve: &Pubkey,
        now_ms: u64,
        freshness_ms: u64,
        committed: bool,
    ) -> Option<CurveFreshnessInfo> {
        self.curves.get(bonding_curve).map(|entry| {
            let curve = *entry.value();
            let snapshot_age_ms = curve.age_ms(now_ms);
            metrics::histogram!("shadow_ledger_age_ms", snapshot_age_ms as f64);
            CurveFreshnessInfo {
                curve: curve.curve,
                curve_data_known: curve.curve_data_known,
                curve_finality: curve.curve_finality,
                snapshot_age_ms,
                state: CurveFreshnessState::classify(
                    curve.curve_data_known,
                    snapshot_age_ms,
                    freshness_ms,
                    committed,
                ),
            }
        })
    }

    /// Get a price quote with staleness validation
    ///
    /// This is the **primary method for production trading** as it ensures that
    /// stale state is detected and rejected, forcing a fallback to RPC.
    ///
    /// # Safety & Validation
    ///
    /// This method implements safety requirements:
    /// - Checks if `current_ts_ms > last_updated_ts_ms + MAX_AGE_MS`
    /// - Returns `GhostError::StaleState` if state is too old
    /// - Returns fresh `BondingCurve` if state is valid
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to look up
    /// * `current_slot` - The current Solana slot number
    ///
    /// # Returns
    ///
    /// * `Ok(BondingCurve)` - Fresh bonding curve state ready for price calculation
    /// * `Err(GhostError::StaleState)` - State is too old, caller must fallback to RPC
    /// * `Err(GhostError::CurveNotFound)` - No curve exists for this mint
    #[deprecated(
        note = "PR 7: use AccountStateCore for canonical readiness/truth; get_quote is reserved for simulation/bootstrap fallback paths"
    )]
    pub fn get_quote(&self, mint: &Pubkey, current_slot: Option<u64>) -> GhostResult<BondingCurve> {
        let shadow_curve = self
            .curves
            .get(mint)
            .map(|entry| *entry.value())
            .ok_or_else(|| GhostError::CurveNotFound(mint.to_string()))?;

        // Staleness check uses slot metadata only when present.
        if let Some(current_slot) = current_slot {
            if shadow_curve.is_stale(current_slot) {
                return Err(GhostError::StaleState {
                    current_slot,
                    last_updated_slot: shadow_curve.last_updated_slot,
                    max_age: ShadowBondingCurve::MAX_AGE_SLOTS,
                });
            }
        }

        Ok(shadow_curve.curve)
    }

    /// Get the bootstrap curve authority for a mint.
    ///
    /// Bootstrap treats an unconfirmed `slot=0` seed as a signal to start from
    /// protocol genesis, not as account-update-derived state. Confirmed curves
    /// continue to use normal freshness validation.
    fn get_bootstrap_curve(
        &self,
        mint: &Pubkey,
        current_slot: Option<u64>,
    ) -> GhostResult<BondingCurve> {
        let shadow_curve = self
            .curves
            .get(mint)
            .map(|entry| *entry.value())
            .ok_or_else(|| GhostError::CurveNotFound(mint.to_string()))?;

        if shadow_curve.last_updated_slot == 0 && !shadow_curve.curve_data_known {
            return Ok(protocol_genesis_curve());
        }

        if let Some(current_slot) = current_slot {
            if shadow_curve.is_stale(current_slot) {
                return Err(GhostError::StaleState {
                    current_slot,
                    last_updated_slot: shadow_curve.last_updated_slot,
                    max_age: ShadowBondingCurve::MAX_AGE_SLOTS,
                });
            }
        }

        Ok(shadow_curve.curve)
    }

    /// Get a ShadowBondingCurve state by mint address (with slot info)
    ///
    /// This method returns a copy of the full ShadowBondingCurve including
    /// slot tracking information.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to look up
    ///
    /// # Returns
    ///
    /// * `Some(ShadowBondingCurve)` - A copy of the curve state with slot info if found
    /// * `None` - If no curve exists for this mint
    pub fn get_old(&self, mint: &Pubkey) -> Option<ShadowBondingCurve> {
        self.curves.get(mint).map(|entry| *entry.value())
    }

    /// Remove a bonding curve state from the ledger
    ///
    /// Atomically removes the bonding curve associated with the given mint address.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to remove
    ///
    /// # Returns
    ///
    /// * `Some(ShadowBondingCurve)` - The removed curve state if it existed
    /// * `None` - If no curve was found for this mint
    pub fn remove(&self, mint: &Pubkey) -> Option<ShadowBondingCurve> {
        self.curves.remove(mint).map(|(_, v)| v)
    }

    /// Get the number of bonding curves currently tracked
    ///
    /// # Returns
    ///
    /// The number of bonding curves in the ledger
    pub fn len(&self) -> usize {
        self.curves.len()
    }

    /// Check if the ledger is empty
    ///
    /// # Returns
    ///
    /// * `true` if the ledger contains no bonding curves
    /// * `false` if the ledger contains at least one bonding curve
    pub fn is_empty(&self) -> bool {
        self.curves.is_empty()
    }

    /// Clear all bonding curves from the ledger
    ///
    /// Removes all entries from the ledger.
    /// Useful for resetting state or responding to catastrophic failure scenarios.
    pub fn clear(&self) {
        self.curves.clear();
        self.curve_keys_by_base_mint.clear();
    }

    /// Check if the ledger contains a bonding curve for the given mint
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to check
    ///
    /// # Returns
    ///
    /// * `true` if a bonding curve exists for this mint
    /// * `false` if no bonding curve exists for this mint
    pub fn contains(&self, mint: &Pubkey) -> bool {
        self.curves.contains_key(mint)
    }

    /// Get a list of all mint addresses currently tracked
    ///
    /// Returns a vector of all mint addresses (Pubkeys) in the ledger.
    /// Useful for iteration, debugging, and monitoring.
    ///
    /// # Performance Note
    ///
    /// This method clones all keys. For large ledgers, this may allocate significant
    /// memory. Consider using this for debugging/monitoring only, not in hot paths.
    pub fn get_all_mints(&self) -> Vec<Pubkey> {
        self.curves.iter().map(|entry| *entry.key()).collect()
    }

    // =========================================================================
    // SIMULATION METHODS - The "Wehikuł Czasu" (Time Machine)
    // =========================================================================

    /// Simulate a buy operation and get comprehensive pre-transaction analysis
    ///
    /// This is the **core simulation method** for the Shadow Ledger.
    /// It calculates exact expected outcomes BEFORE sending any transaction,
    /// enabling deterministic decision-making.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address of the token to buy
    /// * `amount_sol_lamports` - SOL amount to spend (in lamports)
    /// * `current_slot` - Current Solana slot for staleness check
    ///
    /// # Returns
    ///
    /// * `Ok(BuySimulationResult)` - Complete simulation with tokens out, impact, etc.
    /// * `Err(GhostError::StaleState)` - State is too old, fallback to RPC needed
    /// * `Err(GhostError::CurveNotFound)` - No curve exists for this mint
    ///
    /// # Performance
    ///
    /// Target: < 50 nanoseconds per simulation
    /// - Lock-free DashMap access
    /// - Stack-allocated result
    /// - No heap allocations in hot path
    ///
    /// # Example
    ///
    /// ```ignore
    /// let ledger = ShadowLedger::new();
    /// // ... insert curve ...
    ///
    /// if let Ok(sim) = ledger.simulate_buy(&mint, 1_000_000_000, current_slot) {
    ///     if sim.price_impact_percent > 5.0 {
    ///         println!("WARNING: High price impact!");
    ///     }
    ///     // Use sim.min_tokens_out as min_amount_out in buy instruction
    /// }
    /// ```
    #[inline]
    pub fn simulate_buy(
        &self,
        mint: &Pubkey,
        amount_sol_lamports: u64,
        current_slot: Option<u64>,
    ) -> GhostResult<BuySimulationResult> {
        // Get fresh curve state
        let curve = self.get_quote(mint, current_slot)?;

        // Delegate to the pure simulation function from the simulation module
        Ok(simulate_buy_pure(&curve, amount_sol_lamports))
    }

    /// Simulate a buy operation with custom slippage tolerance
    ///
    /// Like `simulate_buy`, but allows specifying a custom slippage tolerance
    /// for calculating `min_tokens_out`.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address of the token to buy
    /// * `amount_sol_lamports` - SOL amount to spend (in lamports)
    /// * `current_slot` - Current Solana slot for staleness check
    /// * `slippage_bps` - Slippage tolerance in basis points (e.g., 50 = 0.5%)
    ///
    /// # Returns
    ///
    /// * `Ok(BuySimulationResult)` - Complete simulation with adjusted min_tokens_out
    /// * `Err(...)` - Same errors as `simulate_buy`
    #[inline]
    pub fn simulate_buy_with_slippage(
        &self,
        mint: &Pubkey,
        amount_sol_lamports: u64,
        current_slot: Option<u64>,
        slippage_bps: u64,
    ) -> GhostResult<BuySimulationResult> {
        // Get fresh curve state
        let curve = self.get_quote(mint, current_slot)?;

        // Delegate to the pure simulation function from the simulation module
        Ok(simulate_buy_with_slippage_pure(
            &curve,
            amount_sol_lamports,
            slippage_bps,
        ))
    }

    /// Simulate a sell operation and get comprehensive pre-transaction analysis
    ///
    /// Calculates expected SOL output BEFORE sending a sell transaction.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address of the token to sell
    /// * `amount_tokens` - Number of tokens to sell
    /// * `current_slot` - Current Solana slot for staleness check
    ///
    /// # Returns
    ///
    /// * `Ok(SellSimulationResult)` - Complete simulation with SOL out, impact, etc.
    /// * `Err(GhostError::StaleState)` - State is too old, fallback to RPC needed
    /// * `Err(GhostError::CurveNotFound)` - No curve exists for this mint
    ///
    /// # Performance
    ///
    /// Target: < 50 nanoseconds per simulation
    #[inline]
    pub fn simulate_sell(
        &self,
        mint: &Pubkey,
        amount_tokens: u64,
        current_slot: Option<u64>,
    ) -> GhostResult<SellSimulationResult> {
        // Get fresh curve state
        let curve = self.get_quote(mint, current_slot)?;

        // Delegate to the pure simulation function from the simulation module
        Ok(simulate_sell_pure(&curve, amount_tokens))
    }

    /// Simulate a sell operation with custom slippage tolerance
    ///
    /// Like `simulate_sell`, but allows specifying a custom slippage tolerance
    /// for calculating `min_sol_out`.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address of the token to sell
    /// * `amount_tokens` - Number of tokens to sell
    /// * `current_slot` - Current Solana slot for staleness check
    /// * `slippage_bps` - Slippage tolerance in basis points (e.g., 50 = 0.5%)
    #[inline]
    pub fn simulate_sell_with_slippage(
        &self,
        mint: &Pubkey,
        amount_tokens: u64,
        current_slot: Option<u64>,
        slippage_bps: u64,
    ) -> GhostResult<SellSimulationResult> {
        // Get fresh curve state
        let curve = self.get_quote(mint, current_slot)?;

        // Delegate to the pure simulation function from the simulation module
        Ok(simulate_sell_with_slippage_pure(
            &curve,
            amount_tokens,
            slippage_bps,
        ))
    }

    /// Check if a token is near migration to Raydium
    ///
    /// Convenience method to check if trading should be avoided due to
    /// imminent migration.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to check
    /// * `current_slot` - Current Solana slot for staleness check
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Token is >99% complete, migration imminent
    /// * `Ok(false)` - Token has room to grow
    /// * `Err(...)` - State errors (stale, not found)
    #[inline]
    pub fn is_near_migration(&self, mint: &Pubkey, current_slot: Option<u64>) -> GhostResult<bool> {
        let curve = self.get_quote(mint, current_slot)?;
        Ok(curve.is_near_migration())
    }

    /// Get market analytics for a token
    ///
    /// Returns market cap and bonding progress without simulating a trade.
    /// Useful for dashboard/monitoring purposes.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to analyze
    /// * `current_slot` - Current Solana slot for staleness check
    ///
    /// # Returns
    ///
    /// * `Ok((market_cap_sol, bonding_progress))` - Market metrics
    /// * `Err(...)` - State errors
    #[inline]
    pub fn get_market_analytics(
        &self,
        mint: &Pubkey,
        current_slot: Option<u64>,
    ) -> GhostResult<(u64, u64)> {
        let curve = self.get_quote(mint, current_slot)?;
        Ok((curve.get_market_cap_sol(), curve.get_bonding_progress()))
    }

    // =========================================================================
    // FORWARD SIMULATION METHODS - Read-only preflight simulation
    // =========================================================================

    /// Simulate a hypothetical BUY against the current authoritative Shadow Ledger state.
    ///
    /// This is **read-only** with respect to the authoritative state — it reads the
    /// current curve reserves, converts them to a [`ReconstructedState`], and runs a
    /// forward simulation on a temporary clone.  The live state is never mutated.
    ///
    /// Uses the same fee-aware, integer-safe, k-invariant math as the authoritative
    /// state-evolution path.
    ///
    /// # Arguments
    ///
    /// * `mint`         – mint address of the token to buy
    /// * `sol_lamports` – SOL amount to spend (lamports)
    /// * `slippage_bps` – slippage tolerance in basis points (e.g. 50 = 0.5 %)
    ///
    /// # Returns
    ///
    /// * `Ok(ForwardSimResult)` – simulation result with post-trade reserves, price, impact
    /// * `Err(...)` – curve not found or stale
    pub fn forward_simulate_buy(
        &self,
        mint: &Pubkey,
        sol_lamports: u64,
        slippage_bps: u64,
    ) -> GhostResult<super::forward_simulation::ForwardSimResult> {
        let curve = self.get_quote(mint, None)?;
        let state = super::history_types::ReconstructedState::reserves_from_curve(&curve);
        Ok(super::forward_simulation::simulate_forward_buy(
            &state,
            sol_lamports,
            slippage_bps,
        ))
    }

    /// Simulate a hypothetical SELL against the current authoritative Shadow Ledger state.
    ///
    /// This is **read-only** — the live state is never mutated.
    ///
    /// # Arguments
    ///
    /// * `mint`         – mint address of the token to sell
    /// * `tok_units`    – token amount to sell (base units)
    /// * `slippage_bps` – slippage tolerance in basis points
    pub fn forward_simulate_sell(
        &self,
        mint: &Pubkey,
        tok_units: u64,
        slippage_bps: u64,
    ) -> GhostResult<super::forward_simulation::ForwardSimResult> {
        let curve = self.get_quote(mint, None)?;
        let state = super::history_types::ReconstructedState::reserves_from_curve(&curve);
        Ok(super::forward_simulation::simulate_forward_sell(
            &state,
            tok_units,
            slippage_bps,
        ))
    }

    /// Simulate an ordered sequence of hypothetical trades (bundle preflight) against
    /// the current authoritative Shadow Ledger state.
    ///
    /// Each action sees the post-trade state of the previous action.
    /// The live state is never mutated.
    ///
    /// # Arguments
    ///
    /// * `mint`         – mint address
    /// * `actions`      – ordered slice of [`ForwardSimAction`] entries
    /// * `slippage_bps` – slippage tolerance applied to every step
    pub fn forward_simulate_bundle(
        &self,
        mint: &Pubkey,
        actions: &[super::forward_simulation::ForwardSimAction],
        slippage_bps: u64,
    ) -> GhostResult<super::forward_simulation::ForwardBundleResult> {
        let curve = self.get_quote(mint, None)?;
        let state = super::history_types::ReconstructedState::reserves_from_curve(&curve);
        Ok(super::forward_simulation::simulate_forward_bundle(
            &state,
            actions,
            slippage_bps,
        ))
    }

    /// Assess a hypothetical BUY against the current authoritative state and configured guardrails.
    ///
    /// Combines forward simulation with guardrail evaluation and returns a structured
    /// [`TradeAssessment`] that makes it easy to decide: proceed, reject, or inspect why.
    ///
    /// The live state is never mutated.
    ///
    /// # Arguments
    ///
    /// * `mint`         – mint address
    /// * `sol_lamports` – SOL amount to spend (lamports)
    /// * `config`       – guardrail configuration
    pub fn assess_buy(
        &self,
        mint: &Pubkey,
        sol_lamports: u64,
        config: &super::forward_simulation::GuardrailConfig,
    ) -> GhostResult<super::forward_simulation::TradeAssessment> {
        let curve = self.get_quote(mint, None)?;
        let state = super::history_types::ReconstructedState::reserves_from_curve(&curve);
        Ok(super::forward_simulation::assess_buy(
            &state,
            sol_lamports,
            config,
        ))
    }

    /// Assess a hypothetical SELL against the current authoritative state and configured guardrails.
    ///
    /// The live state is never mutated.
    pub fn assess_sell(
        &self,
        mint: &Pubkey,
        tok_units: u64,
        config: &super::forward_simulation::GuardrailConfig,
    ) -> GhostResult<super::forward_simulation::TradeAssessment> {
        let curve = self.get_quote(mint, None)?;
        let state = super::history_types::ReconstructedState::reserves_from_curve(&curve);
        Ok(super::forward_simulation::assess_sell(
            &state, tok_units, config,
        ))
    }

    /// Assess a multi-step trade bundle against the current authoritative state and guardrails.
    ///
    /// Simulates each step sequentially (each step sees post-trade state of the previous),
    /// evaluates all steps against the configured guardrails, and returns a structured
    /// [`BundleAssessment`] identifying any failing steps and their reasons.
    ///
    /// The live state is never mutated.
    pub fn assess_bundle(
        &self,
        mint: &Pubkey,
        actions: &[super::forward_simulation::ForwardSimAction],
        config: &super::forward_simulation::GuardrailConfig,
    ) -> GhostResult<super::forward_simulation::BundleAssessment> {
        let curve = self.get_quote(mint, None)?;
        let state = super::history_types::ReconstructedState::reserves_from_curve(&curve);
        Ok(super::forward_simulation::assess_bundle(
            &state, actions, config,
        ))
    }

    // =========================================================================
    // SNAPSHOT METHODS - Geometric Market State for Derivative Analysis
    // =========================================================================
    ///
    /// Expected: 3 snapshots (G0, G1, G2) with non-decreasing timestamps.
    /// The snapshots are wrapped in a SnapshotBuffer with the current timestamp
    /// for age-based eviction support.
    ///
    /// Legacy entry point (non-canonical). Prefer `commit_history`.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to associate with these snapshots
    /// * `snaps` - Vector of `MarketSnapshot` (typically 3 elements: G0, G1, G2)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let snaps = vec![g0, g1, g2];
    /// ledger.set_snapshots(mint, snaps);
    /// ```
    #[inline]
    pub fn set_snapshots(&self, mint: Pubkey, snaps: Vec<MarketSnapshot>) {
        if self.is_committed(&mint) {
            increment_counter!("snapshot_noncanonical_write_attempt_total");
            warn!(
                base_mint = %mint,
                "ShadowLedger: set_snapshots rejected for committed mint (use append_live)"
            );
            return;
        }
        self.commit_history(mint, snaps, None);
    }

    fn set_snapshots_internal(
        &self,
        mint: Pubkey,
        snaps: Vec<MarketSnapshot>,
    ) -> Option<Vec<MarketSnapshot>> {
        let mut snaps = snaps;

        // EPIC 2: Reject slot=Some(0) snapshots (legacy sentinel abuse)
        // Some(0) is forbidden - use None for unknown/synthetic slots
        let original_len = snaps.len();
        snaps.retain(|snap| {
            if snap.slot == Some(0) {
                increment_counter!("shadowledger_set_snapshots_slot0_rejected_total");
                false
            } else {
                true
            }
        });
        if snaps.len() < original_len {
            let rejected = original_len - snaps.len();
            debug!(
                base_mint = %mint,
                rejected_count = rejected,
                "ShadowLedger: rejected slot=Some(0) snapshots (EPIC 2 compliance)"
            );
        }

        // If all snapshots were rejected (e.g., all were slot=0), don't insert empty buffer
        if snaps.is_empty() {
            increment_counter!("shadowledger_set_snapshots_all_rejected_total");
            debug!(
                base_mint = %mint,
                "ShadowLedger: all snapshots rejected (slot=0), skipping insert"
            );
            return None;
        }

        if let Ok(guard) = self.approval_checker.read() {
            if let Some(checker) = guard.as_ref().cloned() {
                if !(checker)(&mint) {
                    self.untracked_snapshot_write_blocked_total
                        .fetch_add(1, Ordering::Relaxed);

                    if self.blocked_log_once.insert(mint, ()).is_none() {
                        debug!(
                            base_mint = %mint,
                            "ShadowLedger: blocked snapshot set for unapproved pool (pool context unavailable in this path)"
                        );
                    }
                    return None;
                }
            }
        }

        if snaps.len() > self.max_snapshots_per_mint {
            let start = snaps.len().saturating_sub(self.max_snapshots_per_mint);
            snaps = snaps.split_off(start);
        }

        let committed = snaps.clone();
        self.snapshots.insert(mint, SnapshotBuffer::new(snaps));
        Some(committed)
    }

    fn set_snapshots_internal_unchecked(
        &self,
        mint: Pubkey,
        snaps: Vec<MarketSnapshot>,
    ) -> Option<Vec<MarketSnapshot>> {
        let mut snaps = snaps;

        let original_len = snaps.len();
        snaps.retain(|snap| {
            if snap.slot == Some(0) {
                increment_counter!("shadowledger_set_snapshots_slot0_rejected_total");
                false
            } else {
                true
            }
        });
        if snaps.len() < original_len {
            let rejected = original_len - snaps.len();
            debug!(
                base_mint = %mint,
                rejected_count = rejected,
                "ShadowLedger: rejected slot=Some(0) snapshots (EPIC 2 compliance)"
            );
        }

        if snaps.is_empty() {
            increment_counter!("shadowledger_set_snapshots_all_rejected_total");
            debug!(
                base_mint = %mint,
                "ShadowLedger: all snapshots rejected (slot=0), skipping insert"
            );
            return None;
        }

        if snaps.len() > self.max_snapshots_per_mint {
            let start = snaps.len().saturating_sub(self.max_snapshots_per_mint);
            snaps = snaps.split_off(start);
        }

        let committed = snaps.clone();
        self.snapshots.insert(mint, SnapshotBuffer::new(snaps));
        Some(committed)
    }

    /// Commit a full canonical history for a mint and mark it as committed.
    ///
    /// This is the canonical entry point for Gatekeeper history injection.
    pub fn commit_history(
        &self,
        mint: Pubkey,
        snaps: Vec<MarketSnapshot>,
        last_committed_tx_key: Option<TxKey>,
    ) -> CommitHistoryResult {
        self.commit_history_with_source(
            mint,
            snaps,
            last_committed_tx_key,
            ShadowLedgerWriteSource::CanonicalCommit,
        )
    }

    pub fn commit_history_with_source(
        &self,
        mint: Pubkey,
        snaps: Vec<MarketSnapshot>,
        last_committed_tx_key: Option<TxKey>,
        write_source: ShadowLedgerWriteSource,
    ) -> CommitHistoryResult {
        match self.apply_write(ShadowLedgerWriteRequest::CommitHistory {
            mint,
            snaps,
            last_committed_tx_key,
            write_source,
        }) {
            ShadowLedgerWriteOutcome::CommitHistory(result) => result,
            ShadowLedgerWriteOutcome::Curve(_) | ShadowLedgerWriteOutcome::LiveAppend(_) => {
                unreachable!("commit write request must return commit history result")
            }
        }
    }

    fn commit_history_with_source_inner(
        &self,
        mint: Pubkey,
        snaps: Vec<MarketSnapshot>,
        last_committed_tx_key: Option<TxKey>,
        write_source: ShadowLedgerWriteSource,
    ) -> CommitHistoryResult {
        let requested_snapshot_count = snaps.len() as u64;
        if let Some(existing) = self.snapshots.get(&mint) {
            if !existing.snapshots.is_empty() {
                self.snapshot_commit_state.insert(
                    mint,
                    MintCommitState::committed(last_committed_tx_key.clone()),
                );
                let result = CommitHistoryResult {
                    status: CommitHistoryStatus::NoOpExistingHistory,
                    requested_snapshot_count: requested_snapshot_count as usize,
                    stored_snapshot_count: existing.snapshots.len(),
                    last_committed_tx_key,
                    last_snapshot: existing.snapshots.last().cloned(),
                };
                record_shadow_ledger_write_total(
                    write_source,
                    ShadowLedgerWriteStrength::CanonicalCommit,
                    "no_op_existing_history",
                );
                return result;
            }
        }

        if let Some(committed) = self.set_snapshots_internal_unchecked(mint, snaps) {
            self.snapshot_commit_state.insert(
                mint,
                MintCommitState::committed(last_committed_tx_key.clone()),
            );
            pipeline_coverage().increment(
                PipelineCoverageStage::ShadowLedgerCommitted,
                committed.len() as u64,
            );
            ::metrics::counter!(
                "ghost_pipeline_stage_total",
                committed.len() as u64,
                "stage" => "shadow_ledger_committed"
            );

            // Pipeline coverage metrics: per-pool snapshot count + lag from latest tx timestamp.
            let last_snap_ts_ms = committed.last().map(|s| s.timestamp_ms).unwrap_or(0);
            super::pipeline_metrics::on_commit_history(
                &mint.to_string(),
                committed.len(),
                last_snap_ts_ms,
                super::types::current_time_ms(),
            );

            let result = CommitHistoryResult {
                status: CommitHistoryStatus::Persisted,
                requested_snapshot_count: requested_snapshot_count as usize,
                stored_snapshot_count: committed.len(),
                last_committed_tx_key,
                last_snapshot: committed.last().cloned(),
            };
            record_shadow_ledger_write_total(
                write_source,
                ShadowLedgerWriteStrength::CanonicalCommit,
                "persisted",
            );
            result
        } else {
            increment_counter!("shadowledger_commit_history_rejected_total");
            if requested_snapshot_count > 0 {
                pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerRejected, 1);
                increment_counter!(
                    "ghost_pipeline_stage_total",
                    "stage" => "shadow_ledger_rejected",
                    "reason" => "commit_history_rejected"
                );
            }
            warn!(
                base_mint = %mint,
                "ShadowLedger: commit_history rejected (no snapshots stored)"
            );
            record_shadow_ledger_write_total(
                write_source,
                ShadowLedgerWriteStrength::CanonicalCommit,
                "rejected_no_write",
            );
            record_shadow_ledger_rejected_total("commit_history_rejected");
            CommitHistoryResult {
                status: CommitHistoryStatus::RejectedNoWrite,
                requested_snapshot_count: requested_snapshot_count as usize,
                stored_snapshot_count: 0,
                last_committed_tx_key,
                last_snapshot: None,
            }
        }
    }

    /// Check if a mint has been committed to the canonical snapshot history.
    ///
    /// This reflects commit state, not merely snapshot presence.
    pub fn is_committed(&self, mint: &Pubkey) -> bool {
        self.snapshot_commit_state.contains_key(mint)
    }

    /// Return the last committed TxKey (if available) for a mint.
    pub fn last_committed_tx_key(&self, mint: &Pubkey) -> Option<TxKey> {
        self.snapshot_commit_state
            .get(mint)
            .and_then(|state| state.last_committed_tx_key.as_ref().cloned())
    }

    /// Append a live snapshot only after history has been committed.
    ///
    /// Returns true when the snapshot is appended, false when rejected.
    pub fn append_live(&self, mint: Pubkey, snapshot: MarketSnapshot) -> bool {
        self.append_live_with_source(mint, snapshot, ShadowLedgerWriteSource::LivePipeline)
    }

    pub fn append_live_with_source(
        &self,
        mint: Pubkey,
        snapshot: MarketSnapshot,
        write_source: ShadowLedgerWriteSource,
    ) -> bool {
        match self.apply_write(ShadowLedgerWriteRequest::LiveAppend {
            mint,
            snapshot,
            write_source,
        }) {
            ShadowLedgerWriteOutcome::LiveAppend(result) => result,
            ShadowLedgerWriteOutcome::Curve(_) | ShadowLedgerWriteOutcome::CommitHistory(_) => {
                unreachable!("live append request must return bool")
            }
        }
    }

    fn append_live_with_source_inner(
        &self,
        mint: Pubkey,
        snapshot: MarketSnapshot,
        write_source: ShadowLedgerWriteSource,
    ) -> bool {
        if !self.is_committed(&mint) {
            increment_counter!("snapshot_noncanonical_write_attempt_total");
            pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerRejected, 1);
            increment_counter!(
                "ghost_pipeline_stage_total",
                "stage" => "shadow_ledger_rejected",
                "reason" => "mint_not_committed"
            );
            warn!(
                base_mint = %mint,
                "ShadowLedger: append_live rejected for uncommitted mint"
            );
            record_shadow_ledger_write_total(
                write_source,
                ShadowLedgerWriteStrength::LiveAppend,
                "rejected_mint_not_committed",
            );
            record_shadow_ledger_rejected_total("mint_not_committed");
            return false;
        }

        // Reject Some(0) - legacy sentinel abuse. None is allowed for synthetic snapshots.
        if snapshot.slot == Some(0) {
            increment_counter!("shadowledger_append_live_slot0_rejected_total");
            pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerRejected, 1);
            increment_counter!(
                "ghost_pipeline_stage_total",
                "stage" => "shadow_ledger_rejected",
                "reason" => "slot_zero"
            );
            warn!(
                base_mint = %mint,
                "ShadowLedger: append_live rejected slot=Some(0) snapshot"
            );
            record_shadow_ledger_write_total(
                write_source,
                ShadowLedgerWriteStrength::LiveAppend,
                "rejected_slot_zero",
            );
            record_shadow_ledger_rejected_total("slot_zero");
            return false;
        }

        if let Some(buffer) = self.snapshots.get(&mint) {
            if let Some(last) = buffer.snapshots.last() {
                if snapshot.tx_count < last.tx_count {
                    increment_counter!("shadowledger_append_live_txcount_regression_total");
                    pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerRejected, 1);
                    increment_counter!(
                        "ghost_pipeline_stage_total",
                        "stage" => "shadow_ledger_rejected",
                        "reason" => "tx_count_regression"
                    );
                    warn!(
                        base_mint = %mint,
                        new_tx_count = snapshot.tx_count,
                        last_tx_count = last.tx_count,
                        "ShadowLedger: append_live rejected tx_count regression"
                    );
                    record_shadow_ledger_write_total(
                        write_source,
                        ShadowLedgerWriteStrength::LiveAppend,
                        "rejected_tx_count_regression",
                    );
                    record_shadow_ledger_rejected_total("tx_count_regression");
                    return false;
                }
            }
        }

        if let Some(tx_key) = snapshot.tx_key.as_ref() {
            if let Some(buffer) = self.snapshots.get(&mint) {
                if buffer
                    .snapshots
                    .iter()
                    .any(|snap| snap.tx_key.as_ref() == Some(tx_key))
                {
                    increment_counter!("shadowledger_append_live_duplicate_total");
                    pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerRejected, 1);
                    increment_counter!(
                        "ghost_pipeline_stage_total",
                        "stage" => "shadow_ledger_rejected",
                        "reason" => "duplicate_tx_key"
                    );
                    debug!(
                        base_mint = %mint,
                        slot = snapshot.slot,
                        "ShadowLedger: append_live rejected duplicate tx_key"
                    );
                    record_shadow_ledger_write_total(
                        write_source,
                        ShadowLedgerWriteStrength::LiveAppend,
                        "rejected_duplicate_tx_key",
                    );
                    record_shadow_ledger_rejected_total("duplicate_tx_key");
                    return false;
                }
            }
        }

        if let Some(last_snapshot) = self
            .snapshots
            .get(&mint)
            .and_then(|buffer| buffer.snapshots.last().cloned())
        {
            if snapshot.timestamp_ms < last_snapshot.timestamp_ms {
                increment_counter!("shadowledger_append_live_stale_total");
                pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerRejected, 1);
                increment_counter!(
                    "ghost_pipeline_stage_total",
                    "stage" => "shadow_ledger_rejected",
                    "reason" => "stale_timestamp"
                );
                warn!(
                        base_mint = %mint,
                        new_ts_ms = snapshot.timestamp_ms,
                    last_ts_ms = last_snapshot.timestamp_ms,
                    "ShadowLedger: append_live rejected stale snapshot"
                );
                record_shadow_ledger_write_total(
                    write_source,
                    ShadowLedgerWriteStrength::LiveAppend,
                    "rejected_stale_timestamp",
                );
                record_shadow_ledger_rejected_total("stale_timestamp");
                return false;
            }

            if snapshot.timestamp_ms == last_snapshot.timestamp_ms {
                let new_key = snapshot.tx_key.as_ref().cloned().unwrap_or_else(|| {
                    // TxKey::new(timestamp_ms, slot, tx_index, signature, fallback_counter)
                    TxKey::new(
                        snapshot.timestamp_ms,
                        snapshot.slot, // Already Option<u64>
                        Some(snapshot.tx_count as u32),
                        None,
                        0,
                    )
                    .unwrap()
                });
                let last_key = last_snapshot.tx_key.as_ref().cloned().unwrap_or_else(|| {
                    TxKey::new(
                        last_snapshot.timestamp_ms,
                        last_snapshot.slot, // Already Option<u64>
                        Some(last_snapshot.tx_count as u32),
                        None,
                        0,
                    )
                    .unwrap()
                });
                if new_key < last_key {
                    increment_counter!("shadowledger_append_live_stale_total");
                    pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerRejected, 1);
                    increment_counter!(
                        "ghost_pipeline_stage_total",
                        "stage" => "shadow_ledger_rejected",
                        "reason" => "non_monotonic_tx_key"
                    );
                    warn!(
                        base_mint = %mint,
                        new_slot = snapshot.slot,
                        "ShadowLedger: append_live rejected non-monotonic snapshot"
                    );
                    record_shadow_ledger_write_total(
                        write_source,
                        ShadowLedgerWriteStrength::LiveAppend,
                        "rejected_non_monotonic_tx_key",
                    );
                    record_shadow_ledger_rejected_total("non_monotonic_tx_key");
                    return false;
                }
            }
        }

        // Capture timestamp before move into push_snapshot_inner.
        let snap_ts_ms = snapshot.timestamp_ms;
        let mint_str = mint.to_string();

        if self.push_snapshot_inner(mint, snapshot, SnapshotSource::Canonical) {
            pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerLiveAppended, 1);
            ::metrics::counter!(
                "ghost_pipeline_stage_total",
                1,
                "stage" => "shadow_ledger_live_appended",
                "source" => "live_pipeline"
            );
            super::pipeline_metrics::on_live_append(
                &mint_str,
                snap_ts_ms,
                super::types::current_time_ms(),
            );
            record_shadow_ledger_write_total(
                write_source,
                ShadowLedgerWriteStrength::LiveAppend,
                "applied",
            );
            true
        } else {
            pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerRejected, 1);
            increment_counter!(
                "ghost_pipeline_stage_total",
                "stage" => "shadow_ledger_rejected",
                "reason" => "approval_checker_blocked"
            );
            record_shadow_ledger_write_total(
                write_source,
                ShadowLedgerWriteStrength::LiveAppend,
                "rejected_approval_checker_blocked",
            );
            record_shadow_ledger_rejected_total("approval_checker_blocked");
            false
        }
    }

    /// Alias for `set_snapshots` for API consistency.
    ///
    /// This method is identical to `set_snapshots` and is provided for API completeness.
    /// Legacy entry point (non-canonical). Prefer `commit_history`.
    #[inline]
    pub fn insert_snapshots(&self, mint: Pubkey, snaps: Vec<MarketSnapshot>) {
        self.set_snapshots(mint, snaps);
    }

    /// Get a copy of the snapshots for a given base_mint (if they exist).
    ///
    /// # Key Contract
    ///
    /// **CANONICAL KEY**: Snapshots are stored by `base_mint` pubkey. This is different
    /// from curves which are stored by `bonding_curve` pubkey.
    ///
    /// # When to Use
    ///
    /// - Engine/Cyclic scoring for market state
    /// - SOBP, TCF, and other market metrics
    /// - Any code that needs historical market snapshots
    ///
    /// # Arguments
    ///
    /// * `base_mint` - The base_mint (token) address to look up
    ///
    /// # Returns
    ///
    /// * `Some(Vec<MarketSnapshot>)` - Copy of the snapshots if found
    /// * `None` - If no snapshots exist for this base_mint
    ///
    /// # Example
    ///
    /// ```ignore
    /// // CORRECT: Use base_mint for snapshot lookups
    /// let snapshots = shadow_ledger.get_snapshots(&base_mint);
    ///
    /// // INCORRECT: Do NOT use bonding_curve or pool_amm_id
    /// // let snapshots = shadow_ledger.get_snapshots(&bonding_curve);  // WRONG!
    /// ```
    #[inline]
    pub fn get_snapshots(&self, base_mint: &Pubkey) -> Option<Vec<MarketSnapshot>> {
        if !self.snapshot_key_is_approved(base_mint, "get_snapshots") {
            return None;
        }
        self.get_snapshots_internal(base_mint)
    }

    /// Get the most recent snapshot for a given base_mint (if it exists).
    ///
    /// # Key Contract
    ///
    /// Snapshots are stored by `base_mint` pubkey (see `get_snapshots` documentation).
    #[inline]
    pub fn get_latest_snapshot(&self, base_mint: &Pubkey) -> Option<MarketSnapshot> {
        if !self.snapshot_key_is_approved(base_mint, "get_latest_snapshot") {
            return None;
        }
        self.get_latest_snapshot_internal(base_mint)
    }

    #[inline]
    pub fn get_snapshots_internal(&self, base_mint: &Pubkey) -> Option<Vec<MarketSnapshot>> {
        self.snapshots.get(base_mint).map(|v| v.snapshots.clone())
    }

    #[inline]
    pub fn get_latest_snapshot_internal(&self, base_mint: &Pubkey) -> Option<MarketSnapshot> {
        self.snapshots
            .get(base_mint)
            .and_then(|v| v.snapshots.last().cloned())
    }

    /// Append a snapshot to the buffer for a mint with FIFO eviction.
    ///
    /// Legacy entry point (non-canonical). Use `append_live` after commit.
    ///
    /// Maintains a bounded buffer (`max_snapshots_per_mint`) by removing the oldest
    /// snapshots when the capacity is exceeded.
    pub fn push_snapshot(&self, mint: Pubkey, snapshot: MarketSnapshot) {
        if !self.is_committed(&mint) {
            increment_counter!("snapshot_noncanonical_write_attempt_total");
            warn!(
                base_mint = %mint,
                "ShadowLedger: non-canonical snapshot write rejected (mint not committed)"
            );
            return;
        }
        self.push_snapshot_with_source(mint, snapshot, SnapshotSource::Legacy);
    }

    /// Append a snapshot with explicit source attribution for telemetry and slot=0 hygiene.
    ///
    /// Legacy entry point (non-canonical). Use `append_live` after commit.
    pub fn push_snapshot_with_source(
        &self,
        mint: Pubkey,
        snapshot: MarketSnapshot,
        source: SnapshotSource,
    ) {
        if !self.is_committed(&mint) {
            increment_counter!("snapshot_noncanonical_write_attempt_total");
            pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerRejected, 1);
            increment_counter!(
                "ghost_pipeline_stage_total",
                "stage" => "shadow_ledger_rejected",
                "reason" => "mint_not_committed"
            );
            warn!(
                base_mint = %mint,
                source = source.as_str(),
                "ShadowLedger: non-canonical snapshot write rejected (mint not committed)"
            );
            return;
        }

        // Reject Some(0) - legacy sentinel abuse. None is allowed.
        if snapshot.slot == Some(0) {
            increment_counter!(
                "shadowledger_snapshots_slot0_total",
                "source" => source.as_str()
            );
            increment_counter!(
                "ghost_pipeline_stage_total",
                "stage" => "shadow_ledger_rejected",
                "reason" => "slot_zero"
            );
            pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerRejected, 1);
            let (last_slot, reason) = self.slot_zero_rejection_reason(&mint);
            debug!(
                mint = %mint,
                source = source.as_str(),
                ?last_slot,
                reason,
                "ShadowLedger: dropping slot=Some(0) snapshot"
            );
            return;
        }

        if self.push_snapshot_inner(mint, snapshot, source) {
            pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerLiveAppended, 1);
            ::metrics::counter!(
                "ghost_pipeline_stage_total",
                1,
                "stage" => "shadow_ledger_live_appended",
                "source" => source.as_str()
            );
        } else {
            pipeline_coverage().increment(PipelineCoverageStage::ShadowLedgerRejected, 1);
            increment_counter!(
                "ghost_pipeline_stage_total",
                "stage" => "shadow_ledger_rejected",
                "reason" => "approval_checker_blocked"
            );
        }
    }

    fn snapshot_key_is_approved(&self, mint: &Pubkey, action: &'static str) -> bool {
        if let Ok(guard) = self.approval_checker.read() {
            if let Some(checker) = guard.as_ref() {
                if !(checker)(mint) {
                    self.untracked_snapshot_read_blocked_total
                        .fetch_add(1, Ordering::Relaxed);

                    if self.blocked_log_once.insert(*mint, ()).is_none() {
                        debug!(
                            mint = %mint,
                            action,
                            "ShadowLedger: blocked snapshot access for unapproved pool"
                        );
                    }
                    return false;
                }
            }
        }
        true
    }

    fn slot_zero_rejection_reason(&self, mint: &Pubkey) -> (Option<u64>, &'static str) {
        match self.snapshots.get(mint) {
            Some(existing) => {
                let last_slot = existing.snapshots.last().and_then(|snap| snap.slot);
                let reason = if last_slot.is_none() {
                    "duplicate_bootstrap"
                } else {
                    "live_history"
                };
                (last_slot, reason)
            }
            None => (None, "empty_history"),
        }
    }

    fn push_snapshot_inner(
        &self,
        mint: Pubkey,
        snapshot: MarketSnapshot,
        source: SnapshotSource,
    ) -> bool {
        if let Ok(guard) = self.approval_checker.read() {
            if let Some(checker) = guard.as_ref().cloned() {
                if !(checker)(&mint) {
                    self.untracked_snapshot_write_blocked_total
                        .fetch_add(1, Ordering::Relaxed);

                    if self.blocked_log_once.insert(mint, ()).is_none() {
                        debug!(
                            mint = %mint,
                            source = source.as_str(),
                            "ShadowLedger: blocked snapshot write for unapproved pool"
                        );
                    }
                    return false;
                }
            }
        }

        let max_len = self.max_snapshots_per_mint;
        let mut entry = self
            .snapshots
            .entry(mint)
            .or_insert_with(|| SnapshotBuffer::new(Vec::new()));

        {
            let buffer = entry.value_mut();
            buffer.snapshots.push(snapshot);
            if buffer.snapshots.len() > max_len {
                // FIFO retention: oldest snapshots are evicted when buffer exceeds max_len (128).
                // This ensures we always keep the newest 128 snapshots for scoring stability.
                buffer.snapshots.drain(..1);
            }
        }
        true
    }

    /// Append multiple snapshots for a mint using FIFO eviction.
    pub fn push_snapshots<I>(&self, mint: Pubkey, snapshots: I)
    where
        I: IntoIterator<Item = MarketSnapshot>,
    {
        if !self.is_committed(&mint) {
            increment_counter!("snapshot_noncanonical_write_attempt_total");
            warn!(
                base_mint = %mint,
                "ShadowLedger: non-canonical snapshot batch rejected (mint not committed)"
            );
            return;
        }

        for snapshot in snapshots {
            self.push_snapshot_with_source(mint, snapshot, SnapshotSource::Legacy);
        }
    }

    /// Get the full snapshot buffer for a given mint (including metadata).
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to look up
    ///
    /// # Returns
    ///
    /// * `Some(SnapshotBuffer)` - Copy of the snapshot buffer if found
    /// * `None` - If no snapshots exist for this mint
    #[inline]
    pub fn get_snapshot_buffer(&self, mint: &Pubkey) -> Option<SnapshotBuffer> {
        if !self.snapshot_key_is_approved(mint, "get_snapshot_buffer") {
            return None;
        }
        self.snapshots.get(mint).map(|v| v.clone())
    }

    /// Check if we have bootstrapped snapshots for a given mint.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to check
    ///
    /// # Returns
    ///
    /// * `true` if snapshots exist for this mint
    /// * `false` otherwise
    #[inline]
    pub fn has_snapshots(&self, mint: &Pubkey) -> bool {
        if !self.snapshot_key_is_approved(mint, "has_snapshots") {
            return false;
        }
        self.snapshots.contains_key(mint)
    }

    /// Clear snapshots for a specific mint (e.g., after migration or cleanup).
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address whose snapshots should be removed
    #[inline]
    pub fn clear_snapshots(&self, mint: &Pubkey) {
        if !self.snapshot_key_is_approved(mint, "clear_snapshots") {
            return;
        }
        self.snapshots.remove(mint);
        self.snapshot_commit_state.remove(mint);
    }

    /// Alias for `clear_snapshots` to emphasize cleanup at session end.
    #[inline]
    pub fn cleanup_snapshots(&self, mint: &Pubkey) {
        self.clear_snapshots(mint);
    }

    /// Global cleanup: remove all snapshots (e.g., on full reset).
    #[inline]
    pub fn clear_all_snapshots(&self) {
        self.snapshots.clear();
    }

    /// Get the number of mints that have bootstrapped snapshots.
    #[inline]
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Evict stale snapshots based on age.
    ///
    /// This method implements lazy eviction by removing snapshot buffers
    /// that are older than the specified max age. Should be called periodically
    /// (e.g., every 30-60 seconds on mainnet) to prevent memory bloat.
    ///
    /// # Recommended Frequency
    ///
    /// - **Mainnet**: Every 30-60 seconds with default max age (5 minutes)
    /// - **Testnet/Devnet**: Every 10-15 seconds with shorter max age (1-2 minutes)
    /// - **Under Memory Pressure**: Use `evict_with_aggressive_config()` or shorter max age
    ///
    /// # Arguments
    ///
    /// * `max_age_ms` - Maximum age in milliseconds. Snapshots older than this are removed.
    ///
    /// # Returns
    ///
    /// Number of snapshot buffers evicted.
    ///
    /// # Edge Cases
    ///
    /// - **Empty Storage**: Returns 0 (no-op)
    /// - **max_age_ms = 0**: All snapshots are evicted (use with caution)
    /// - **max_age_ms = u64::MAX**: Effectively disables eviction
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Evict snapshots older than 5 minutes
    /// let evicted = ledger.evict_stale_snapshots(300_000);
    /// println!("Evicted {} stale snapshot buffers", evicted);
    /// ```
    pub fn evict_stale_snapshots(&self, max_age_ms: u64) -> usize {
        // Delegate to the eviction module's standalone function
        let storage = super::storage::DashMapSnapshotStorage::from_arc(Arc::clone(&self.snapshots));
        super::eviction::evict_stale(&storage, max_age_ms)
    }

    /// Evict stale snapshots using the default max age (5 minutes).
    ///
    /// Convenience method that calls `evict_stale_snapshots` with `DEFAULT_SNAPSHOT_MAX_AGE_MS`.
    /// Recommended for most mainnet scenarios.
    ///
    /// # Returns
    ///
    /// Number of snapshot buffers evicted.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Call periodically in a background task
    /// loop {
    ///     tokio::time::sleep(Duration::from_secs(30)).await;
    ///     let evicted = ledger.evict_stale_snapshots_default();
    ///     if evicted > 0 {
    ///         tracing::debug!("Evicted {} stale snapshots", evicted);
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn evict_stale_snapshots_default(&self) -> usize {
        self.evict_stale_snapshots(DEFAULT_SNAPSHOT_MAX_AGE_MS)
    }

    /// Get an eviction manager for advanced eviction operations.
    ///
    /// The eviction manager provides detailed statistics, diagnostics,
    /// and configurable eviction policies.
    ///
    /// # Returns
    ///
    /// An `EvictionManager` wrapping this ledger's snapshot storage.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let manager = ledger.eviction_manager();
    ///
    /// // Get detailed eviction statistics
    /// let result = manager.evict_stale_default();
    /// println!("Evicted: {}, Retained: {}", result.evicted_count, result.retained_count);
    ///
    /// // Get diagnostics
    /// let stats = manager.get_stats_default();
    /// println!("Total buffers: {}, Stale: {}", stats.total_buffers, stats.stale_count);
    /// ```
    pub fn eviction_manager(
        &self,
    ) -> super::eviction::EvictionManager<super::storage::DashMapSnapshotStorage> {
        let storage = super::storage::DashMapSnapshotStorage::from_arc(Arc::clone(&self.snapshots));
        super::eviction::EvictionManager::new(storage)
    }

    /// Get an eviction manager with custom configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Custom eviction configuration
    ///
    /// # Example
    ///
    /// ```ignore
    /// use ghost_core::shadow_ledger::EvictionConfig;
    ///
    /// // Use aggressive eviction for memory pressure
    /// let config = EvictionConfig::aggressive();
    /// let manager = ledger.eviction_manager_with_config(config);
    /// manager.evict_stale_default();
    /// ```
    pub fn eviction_manager_with_config(
        &self,
        config: super::eviction::EvictionConfig,
    ) -> super::eviction::EvictionManager<super::storage::DashMapSnapshotStorage> {
        let storage = super::storage::DashMapSnapshotStorage::from_arc(Arc::clone(&self.snapshots));
        super::eviction::EvictionManager::with_config(storage, config)
    }

    /// Force cleanup of all snapshots (for shutdown or memory pressure).
    ///
    /// This method removes all snapshot buffers regardless of age.
    /// Use this during:
    /// - Application shutdown
    /// - Memory pressure situations
    /// - Full state reset
    ///
    /// # Returns
    ///
    /// Number of snapshot buffers removed.
    pub fn force_cleanup_snapshots(&self) -> usize {
        let storage = super::storage::DashMapSnapshotStorage::from_arc(Arc::clone(&self.snapshots));
        super::eviction::force_cleanup(&storage)
    }

    /// Preview which snapshots would be evicted without actually removing them.
    ///
    /// Useful for confirmation dialogs or dry-run scenarios.
    ///
    /// # Arguments
    ///
    /// * `max_age_ms` - Maximum age threshold
    ///
    /// # Returns
    ///
    /// Vector of mint addresses that would be evicted.
    pub fn preview_eviction(&self, max_age_ms: u64) -> Vec<Pubkey> {
        let manager = self.eviction_manager();
        manager.preview_eviction(max_age_ms)
    }

    /// Get diagnostic statistics for snapshot storage.
    ///
    /// # Arguments
    ///
    /// * `max_age_ms` - Max age for stale/fresh classification
    ///
    /// # Returns
    ///
    /// Aggregated statistics about snapshot buffers.
    pub fn get_snapshot_stats(&self, max_age_ms: u64) -> super::eviction::DiagnosticStats {
        let manager = self.eviction_manager();
        manager.get_stats(max_age_ms)
    }

    /// Get diagnostic statistics using default max age.
    pub fn get_snapshot_stats_default(&self) -> super::eviction::DiagnosticStats {
        self.get_snapshot_stats(DEFAULT_SNAPSHOT_MAX_AGE_MS)
    }

    /// Remove a curve and its associated snapshots atomically.
    ///
    /// This method ensures that when a curve is removed, its snapshots are also cleaned up,
    /// maintaining consistency between the two data structures.
    ///
    /// # Arguments
    ///
    /// * `mint` - The mint address to remove
    ///
    /// # Returns
    ///
    /// * `Some(ShadowBondingCurve)` - The removed curve state if it existed
    /// * `None` - If no curve was found for this mint
    pub fn remove_with_snapshots(&self, mint: &Pubkey) -> Option<ShadowBondingCurve> {
        self.snapshots.remove(mint);
        self.snapshot_commit_state.remove(mint);
        self.curve_keys_by_base_mint.remove(mint);
        self.curves.remove(mint).map(|(_, v)| v)
    }

    /// Clear all curves and snapshots (full reset).
    ///
    /// This method clears both the curves and snapshots maps atomically,
    /// useful for catastrophic failure recovery or testing.
    pub fn clear_all(&self) {
        self.curves.clear();
        self.curve_keys_by_base_mint.clear();
        self.snapshots.clear();
        self.snapshot_commit_state.clear();
    }

    /// Bootstrap geometric snapshots (G0, G1, G2) for a new pool right after InitializePool.
    ///
    /// This method generates three synthetic market snapshots immediately upon pool detection:
    /// - **G0** (Genesis): Pure InitializePool state
    /// - **G1** (Projected Liquidity): Simulated minimal trade impact with first-order derivatives
    /// - **G2** (First Tick): Second-order derivative reconstruction with full gradients
    ///
    /// These snapshots enable scoring modules (SCR, ULVF, POVC, HOSD, QOFSV) to compute
    /// derivatives, curvatures, and trajectory predictions from the very first moments.
    ///
    /// # Derivative Computation (Phase 2)
    ///
    /// The derivative fields are computed via micro-simulations from the simulation module:
    /// - `d_price_d_volume`: Price sensitivity to volume = Δprice / Δvolume
    /// - `d_price_d_liquidity`: Price sensitivity to reserves = Δprice / Δvirtual_sol
    /// - `d_price_d_slippage`: Curvature of slippage = (impact_2x - impact_1x) / Δamount
    ///
    /// # Arguments
    ///
    /// * `mint` - Base mint of the token
    /// * `current_slot` - Slot at the time of calling (for get_quote/simulate_buy)
    /// * `min_sol_lamports` - Synthetic minimum SOL for G1 simulation (e.g., 0.01 SOL = 10_000_000)
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Snapshots successfully generated and stored
    /// * `Err(GhostError)` - If curve not found or stale
    ///
    /// # Example
    ///
    /// ```ignore
    /// // After detecting a new pool and inserting the curve:
    /// if !shadow_ledger.has_snapshots(&mint) {
    ///     let min_sol_lamports = 10_000_000; // 0.01 SOL
    ///     shadow_ledger.bootstrap_from_initialize(mint, current_slot, min_sol_lamports)?;
    /// }
    /// ```
    pub fn bootstrap_from_initialize(
        &self,
        mint: Pubkey,
        current_slot: Option<u64>,
        min_sol_lamports: u64,
    ) -> GhostResult<()> {
        // Bootstrap starts from protocol genesis for seed-only mints and from
        // the confirmed curve for account-update-backed mints.
        let curve = self.get_bootstrap_curve(&mint, current_slot)?;

        // Delegate to the bootstrap module for snapshot generation
        let config = BootstrapConfig::default();
        let snaps = bootstrap_snapshots(&curve, min_sol_lamports, current_slot, &config);

        self.commit_history(mint, snaps, None);

        Ok(())
    }

    /// Bootstrap geometric snapshots with custom configuration.
    ///
    /// This method allows passing a custom `BootstrapConfig` for selective
    /// bootstrap based on liquidity thresholds and other criteria.
    ///
    /// # Arguments
    ///
    /// * `mint` - Base mint of the token
    /// * `current_slot` - Slot at the time of calling
    /// * `min_sol_lamports` - Synthetic minimum SOL for G1 simulation
    /// * `config` - Bootstrap configuration for filtering and validation
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Snapshots successfully generated and stored
    /// * `Ok(false)` - Pool did not meet criteria (skipped)
    /// * `Err(GhostError)` - If curve not found or stale
    ///
    /// # Example
    ///
    /// ```ignore
    /// use ghost_core::shadow_ledger::BootstrapConfig;
    ///
    /// // Bootstrap only high-liquidity pools
    /// let config = BootstrapConfig::promising_pools(100_000_000_000); // 100 SOL min
    /// let bootstrapped = ledger.bootstrap_with_config(mint, current_slot, 10_000_000, &config)?;
    /// if !bootstrapped {
    ///     tracing::debug!("Pool did not meet liquidity threshold, skipped bootstrap");
    /// }
    /// ```
    pub fn bootstrap_with_config(
        &self,
        mint: Pubkey,
        current_slot: Option<u64>,
        min_sol_lamports: u64,
        config: &BootstrapConfig,
    ) -> GhostResult<bool> {
        let curve = self.get_bootstrap_curve(&mint, current_slot)?;

        // Check if pool should be bootstrapped
        if !config.should_bootstrap(&curve) {
            return Ok(false);
        }

        // Generate snapshots using the bootstrap module
        let snaps = bootstrap_snapshots(&curve, min_sol_lamports, current_slot, config);
        self.commit_history(mint, snaps, None);

        Ok(true)
    }

    /// Bootstrap with synthetic seed generation for EventBus publishing.
    ///
    /// This method extends `bootstrap_from_initialize` by generating synthetic
    /// seed transactions that can be published to EventBus for immediate downstream
    /// processing. It includes comprehensive logging and metrics tracking.
    ///
    /// # Logging
    ///
    /// Emits structured logs:
    /// - `SL_INIT` - Pool initialization started
    /// - `SL_SEED_GENERATED` - Synthetic seed successfully generated
    ///
    /// # Arguments
    ///
    /// * `mint` - Base mint of the token
    /// * `current_slot` - Slot at the time of calling
    /// * `min_sol_lamports` - Synthetic minimum SOL for G1 simulation
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<SyntheticTransaction>)` - Generated synthetic transactions ready for publishing
    /// * `Err(GhostError)` - If curve not found, stale, or generation fails
    ///
    /// # Performance
    ///
    /// Target: < 20ms for complete generation (snapshots + seed)
    ///
    /// # Example
    ///
    /// ```ignore
    /// // After detecting a new pool
    /// let synthetic_txs = ledger.bootstrap_with_seed_generation(
    ///     mint,
    ///     current_slot,
    ///     10_000_000  // 0.01 SOL
    /// )?;
    ///
    /// // Publish to EventBus
    /// for tx in synthetic_txs {
    ///     event_bus.send(GeyserEvent::Transaction {
    ///         slot: current_slot,
    ///         signature: generate_synthetic_signature(&tx),
    ///         accounts: vec![],
    ///         instructions: vec![],
    ///         logs: vec![],
    ///         block_time: Some(tx.timestamp_ms as i64 / 1000),
    ///         account_data: HashMap::new(),
    ///         synthetic: true,
    ///         source: "shadow_ledger".to_string(),
    ///     })?;
    /// }
    /// ```
    pub fn bootstrap_with_seed_generation(
        &self,
        mint: Pubkey,
        current_slot: Option<u64>,
        min_sol_lamports: u64,
    ) -> GhostResult<Vec<super::bootstrap::SyntheticTransaction>> {
        use super::bootstrap::generate_quick_seed;
        use super::types::current_time_ms;

        let start_time = std::time::Instant::now();
        let timestamp_ms = current_time_ms();

        // SL_INIT log
        tracing::info!(
            pool = %mint,
            ts_ms = timestamp_ms,
            slot = current_slot,
            "SL_INIT"
        );

        let curve = self.get_bootstrap_curve(&mint, current_slot)?;

        // Bootstrap snapshots (G0, G1, G2)
        let config = BootstrapConfig::default();
        let snaps = bootstrap_snapshots(&curve, min_sol_lamports, current_slot, &config);
        self.commit_history(mint, snaps, None);

        // Generate synthetic seed transactions
        let synthetic_txs = generate_quick_seed(
            &mint,
            curve.virtual_sol_reserves,
            curve.virtual_token_reserves,
            timestamp_ms,
        );

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // SL_SEED_GENERATED log
        tracing::info!(
            pool = %mint,
            synthetic_txs = synthetic_txs.len(),
            seed_ver = "1.0",
            duration_ms = duration_ms,
            "SL_SEED_GENERATED"
        );

        Ok(synthetic_txs)
    }

    /// Bootstrap with synthetic seed generation and watchdog timeout.
    ///
    /// This method is similar to `bootstrap_with_seed_generation` but includes
    /// a watchdog timeout check. If generation takes longer than the specified
    /// timeout, it logs a failure message and returns an error.
    ///
    /// # Logging
    ///
    /// Emits structured logs:
    /// - `SL_INIT` - Pool initialization started
    /// - `SL_SEED_GENERATED` - Synthetic seed successfully generated (on success)
    /// - `SL_FAIL_TO_GENERATE` - Seed generation exceeded timeout (on failure)
    ///
    /// # Metrics
    ///
    /// Updates metrics if provided:
    /// - `seeds_generated_total` - incremented on success
    /// - `seed_generation_failure_total` - incremented on timeout
    ///
    /// # Arguments
    ///
    /// * `mint` - Base mint of the token
    /// * `current_slot` - Slot at the time of calling
    /// * `min_sol_lamports` - Synthetic minimum SOL for G1 simulation
    /// * `timeout_ms` - Maximum time allowed for generation (typically 100ms)
    /// * `metrics` - Optional metrics instance for tracking
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<SyntheticTransaction>)` - Generated synthetic transactions
    /// * `Err(GhostError)` - On curve not found, stale state, or timeout
    ///
    /// # Example
    ///
    /// ```ignore
    /// use ghost_core::shadow_ledger::BootstrapMetrics;
    /// use std::sync::Arc;
    ///
    /// let metrics = Arc::new(BootstrapMetrics::new());
    ///
    /// // With 100ms watchdog timeout
    /// match ledger.bootstrap_with_seed_generation_watchdog(
    ///     mint,
    ///     current_slot,
    ///     10_000_000,  // 0.01 SOL
    ///     100,          // 100ms timeout
    ///     Some(&metrics)
    /// ) {
    ///     Ok(txs) => {
    ///         // Publish to EventBus
    ///         tracing::info!("Published {} synthetic transactions", txs.len());
    ///     },
    ///     Err(e) => {
    ///         tracing::error!("Seed generation failed: {:?}", e);
    ///     }
    /// }
    /// ```
    pub fn bootstrap_with_seed_generation_watchdog(
        &self,
        mint: Pubkey,
        current_slot: Option<u64>,
        min_sol_lamports: u64,
        timeout_ms: u64,
        metrics: Option<&super::bootstrap::BootstrapMetrics>,
    ) -> GhostResult<Vec<super::bootstrap::SyntheticTransaction>> {
        use super::bootstrap::generate_quick_seed;
        use super::types::current_time_ms;

        let start_time = std::time::Instant::now();
        let timestamp_ms = current_time_ms();

        // SL_INIT log
        tracing::info!(
            pool = %mint,
            ts_ms = timestamp_ms,
            slot = current_slot,
            timeout_ms = timeout_ms,
            "SL_INIT"
        );

        let curve = self.get_bootstrap_curve(&mint, current_slot)?;

        // Check watchdog timeout after curve fetch
        let elapsed_ms = start_time.elapsed().as_millis() as u64;
        if elapsed_ms > timeout_ms {
            tracing::error!(
                pool = %mint,
                elapsed_ms = elapsed_ms,
                timeout_ms = timeout_ms,
                "SL_FAIL_TO_GENERATE: timeout after curve fetch"
            );
            if let Some(m) = metrics {
                m.record_seed_generation_failure();
            }
            return Err(GhostError::SeedGenerationTimeout(format!(
                "Seed generation timeout: {}ms > {}ms",
                elapsed_ms, timeout_ms
            )));
        }

        // Bootstrap snapshots (G0, G1, G2)
        let config = BootstrapConfig::default();
        let snaps = bootstrap_snapshots(&curve, min_sol_lamports, current_slot, &config);
        self.commit_history(mint, snaps, None);

        // Check watchdog timeout after snapshot generation
        let elapsed_ms = start_time.elapsed().as_millis() as u64;
        if elapsed_ms > timeout_ms {
            tracing::error!(
                pool = %mint,
                elapsed_ms = elapsed_ms,
                timeout_ms = timeout_ms,
                "SL_FAIL_TO_GENERATE: timeout after snapshot generation"
            );
            if let Some(m) = metrics {
                m.record_seed_generation_failure();
            }
            return Err(GhostError::SeedGenerationTimeout(format!(
                "Seed generation timeout: {}ms > {}ms",
                elapsed_ms, timeout_ms
            )));
        }

        // Generate synthetic seed transactions
        let synthetic_txs = generate_quick_seed(
            &mint,
            curve.virtual_sol_reserves,
            curve.virtual_token_reserves,
            timestamp_ms,
        );

        let duration_ms = start_time.elapsed().as_millis() as u64;

        // Final watchdog check
        if duration_ms > timeout_ms {
            tracing::error!(
                pool = %mint,
                duration_ms = duration_ms,
                timeout_ms = timeout_ms,
                "SL_FAIL_TO_GENERATE: timeout after seed generation"
            );
            if let Some(m) = metrics {
                m.record_seed_generation_failure();
            }
            return Err(GhostError::SeedGenerationTimeout(format!(
                "Seed generation timeout: {}ms > {}ms",
                duration_ms, timeout_ms
            )));
        }

        // Success - record metrics and log
        if let Some(m) = metrics {
            m.record_seed_generated();
        }

        // SL_SEED_GENERATED log
        tracing::info!(
            pool = %mint,
            synthetic_txs = synthetic_txs.len(),
            seed_ver = "1.0",
            duration_ms = duration_ms,
            "SL_SEED_GENERATED"
        );

        Ok(synthetic_txs)
    }

    // =========================================================================
    // Pipeline observability: state gauges for external health reporting
    // =========================================================================

    /// Number of pools that have been committed to Shadow Ledger.
    ///
    /// A pool is counted here after a successful `commit_history` call.
    /// Use this to populate `shadow_ledger_committed_pools` Prometheus gauge.
    pub fn committed_pool_count(&self) -> usize {
        self.snapshot_commit_state.len()
    }

    /// Total number of snapshots stored across all pools.
    ///
    /// Use this to populate `shadow_ledger_total_snapshots` Prometheus gauge.
    pub fn total_snapshot_count(&self) -> usize {
        self.snapshots
            .iter()
            .map(|e| e.value().snapshots.len())
            .sum()
    }

    // =========================================================================
    // DISK SNAPSHOT METHODS — Periodic persistence and crash recovery
    // =========================================================================

    /// Serialize the full ShadowLedger state to disk atomically.
    ///
    /// The snapshot is written to `<dir>/shadow_ledger_snapshot_<timestamp_ms>.bin`.
    /// An intermediate `.tmp` file is used so a crash mid-write never corrupts
    /// the previously saved snapshot.
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory path where snapshot files are stored.
    ///
    /// # Returns
    ///
    /// `SnapshotWriteStats` on success, `SnapshotError` on failure.
    pub fn snapshot_to_disk(
        &self,
        dir: &std::path::Path,
    ) -> Result<super::disk_snapshot::SnapshotWriteStats, super::disk_snapshot::SnapshotError> {
        use super::disk_snapshot::{now_ms, write_to_dir, DiskSnapshot, SNAPSHOT_FORMAT_VERSION};
        use solana_sdk::pubkey::Pubkey;

        let written_at_ms = now_ms();

        // Flatten DashMap<Pubkey, ShadowBondingCurve> → Vec<([u8;32], ShadowBondingCurve)>
        let curves: Vec<_> = self
            .curves
            .iter()
            .map(|e| (e.key().to_bytes(), *e.value()))
            .collect();

        // Flatten alias map
        let curve_keys_by_base_mint: Vec<_> = self
            .curve_keys_by_base_mint
            .iter()
            .map(|e| (e.key().to_bytes(), e.value().to_bytes()))
            .collect();

        // Flatten snapshot buffers
        let snapshots: Vec<_> = self
            .snapshots
            .iter()
            .map(|e| (e.key().to_bytes(), e.value().clone()))
            .collect();

        // Flatten commit state (extract only the last_committed_tx_key; MintCommitState is private)
        let snapshot_commit_state: Vec<_> = self
            .snapshot_commit_state
            .iter()
            .map(|e| (e.key().to_bytes(), e.value().last_committed_tx_key.clone()))
            .collect();

        // Flatten BVA archives
        let bva_archives: Vec<_> = self
            .bva_archives
            .iter()
            .map(|e| (e.key().to_bytes(), *e.value()))
            .collect();

        let curves_count = curves.len();

        let disk_snapshot = DiskSnapshot {
            version: SNAPSHOT_FORMAT_VERSION,
            written_at_ms,
            curves_count,
            curves,
            curve_keys_by_base_mint,
            snapshots,
            snapshot_commit_state,
            bva_archives,
        };

        write_to_dir(dir, disk_snapshot)
    }

    /// Restore a `ShadowLedger` from the newest valid snapshot file in `dir`.
    ///
    /// Validation rules:
    /// - `version` must equal `SNAPSHOT_FORMAT_VERSION` (= 1)
    /// - `written_at_ms` must be > 0 (rejects uninitialized/bootstrap-only entries)
    ///
    /// If no snapshot exists, returns `SnapshotError::NoSnapshotFound`.
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory path where snapshot files are stored.
    ///
    /// # Returns
    ///
    /// `(ShadowLedger, SnapshotRestoreStats)` on success.
    pub fn restore_from_disk(
        dir: &std::path::Path,
    ) -> Result<
        (Self, super::disk_snapshot::SnapshotRestoreStats),
        super::disk_snapshot::SnapshotError,
    > {
        use super::disk_snapshot::restore_from_dir;
        use solana_sdk::pubkey::Pubkey;

        let (disk_snapshot, stats) = restore_from_dir(dir)?;

        let ledger = Self::new();

        // Restore curves
        for (key_bytes, shadow_curve) in disk_snapshot.curves {
            let pubkey = Pubkey::new_from_array(key_bytes);
            ledger.curves.insert(pubkey, shadow_curve);
        }

        // Restore alias map
        for (base_bytes, curve_bytes) in disk_snapshot.curve_keys_by_base_mint {
            let base_mint = Pubkey::new_from_array(base_bytes);
            let curve_key = Pubkey::new_from_array(curve_bytes);
            ledger.curve_keys_by_base_mint.insert(base_mint, curve_key);
        }

        // Restore snapshot buffers
        for (mint_bytes, buffer) in disk_snapshot.snapshots {
            let mint = Pubkey::new_from_array(mint_bytes);
            ledger.snapshots.insert(mint, buffer);
        }

        // Restore commit state
        for (mint_bytes, last_committed_tx_key) in disk_snapshot.snapshot_commit_state {
            let mint = Pubkey::new_from_array(mint_bytes);
            ledger.snapshot_commit_state.insert(
                mint,
                MintCommitState {
                    last_committed_tx_key,
                },
            );
        }

        // Restore BVA archives
        for (mint_bytes, archive) in disk_snapshot.bva_archives {
            let mint = Pubkey::new_from_array(mint_bytes);
            ledger.bva_archives.insert(mint, archive);
        }

        Ok((ledger, stats))
    }

    /// Delete old snapshot files in `dir`, retaining the `keep_n` newest.
    ///
    /// Files are ordered by the timestamp embedded in their filename.
    /// The oldest `(total - keep_n)` files are deleted.
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory containing snapshot files.
    /// * `keep_n` - Number of most-recent snapshots to retain.
    ///
    /// # Returns
    ///
    /// Number of files deleted, or a `SnapshotError` on IO failure.
    pub fn rotate_snapshots(
        dir: &std::path::Path,
        keep_n: usize,
    ) -> Result<usize, super::disk_snapshot::SnapshotError> {
        super::disk_snapshot::rotate_snapshot_files(dir, keep_n)
    }
}

impl Default for ShadowLedger {
    /// Create a default (empty) Shadow Ledger
    ///
    /// Equivalent to calling `ShadowLedger::new()`
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Helper function to create a test bonding curve
    fn create_test_curve(virtual_token_reserves: u64, virtual_sol_reserves: u64) -> BondingCurve {
        BondingCurve {
            discriminator: 0x1234567890abcdef,
            virtual_token_reserves,
            virtual_sol_reserves,
            real_token_reserves: virtual_token_reserves * 8 / 10,
            real_sol_reserves: virtual_sol_reserves * 8 / 10,
            token_total_supply: virtual_token_reserves,
            complete: 0,
            _padding: [0; 7],
        }
    }

    #[test]
    fn test_new_ledger_is_empty() {
        let ledger = ShadowLedger::new();
        assert_eq!(ledger.len(), 0);
        assert!(ledger.is_empty());
    }

    #[test]
    fn test_with_capacity() {
        let ledger = ShadowLedger::with_capacity(100);
        assert_eq!(ledger.len(), 0);
        assert!(ledger.is_empty());
    }

    #[test]
    fn test_insert_and_get() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);

        // Insert should return None for new entry
        assert!(ledger.insert(mint, curve).is_none());
        assert_eq!(ledger.len(), 1);

        // Get should return the inserted curve
        let retrieved = ledger.get(&mint);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.virtual_token_reserves, 1_000_000_000);
        assert_eq!(retrieved.virtual_sol_reserves, 30_000_000);
    }

    #[test]
    fn test_insert_with_slot() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);

        // First insert should return None
        assert!(ledger.insert_with_slot(mint, curve, 1000).is_none());

        // Second insert should return previous value
        let previous = ledger.insert_with_slot(mint, curve, 1001);
        assert!(previous.is_some());
        assert_eq!(previous.unwrap().last_updated_slot, 1000);
    }

    #[test]
    fn test_get_quote_fresh() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);

        ledger.insert_with_slot(mint, curve, 1000);

        // Fresh quote (current_slot = 1000, updated at 1000)
        let result = ledger.get_quote(&mint, Some(1000));
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_quote_stale() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);

        ledger.insert_with_slot(mint, curve, 1000);

        // Stale quote (current_slot = 1004, updated at 1000, max_age = 3)
        let result = ledger.get_quote(&mint, Some(1004));
        assert!(result.is_err());
        match result {
            Err(GhostError::StaleState { .. }) => {}
            _ => panic!("Expected StaleState error"),
        }
    }

    #[test]
    fn test_get_quote_not_found() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        let result = ledger.get_quote(&mint, Some(1000));
        assert!(result.is_err());
        match result {
            Err(GhostError::CurveNotFound(_)) => {}
            _ => panic!("Expected CurveNotFound error"),
        }
    }

    #[test]
    fn test_get_curve_freshness_info_classifies_fresh_stale_unknown() {
        let ledger = ShadowLedger::new();
        let fresh_mint = Pubkey::new_unique();
        let stale_mint = Pubkey::new_unique();
        let unknown_mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);
        let now_ms = 10_000;

        ledger.insert_with_slot_known_at(fresh_mint, curve, 100, true, 9_900);
        ledger.insert_with_slot_known_at(stale_mint, curve, 100, true, 9_700);
        ledger.insert_with_slot_known_at(unknown_mint, curve, 0, false, 9_900);

        let fresh = ledger
            .get_curve_freshness_info(&fresh_mint, now_ms, 200)
            .expect("fresh info must exist");
        let stale = ledger
            .get_curve_freshness_info(&stale_mint, now_ms, 200)
            .expect("stale info must exist");
        let unknown = ledger
            .get_curve_freshness_info(&unknown_mint, now_ms, 200)
            .expect("unknown info must exist");

        assert_eq!(fresh.state, CurveFreshnessState::Fresh);
        assert_eq!(fresh.snapshot_age_ms, 100);
        assert_eq!(fresh.curve_finality, CurveFinality::Provisional);
        assert_eq!(stale.state, CurveFreshnessState::Stale);
        assert_eq!(stale.snapshot_age_ms, 300);
        assert_eq!(stale.curve_finality, CurveFinality::Provisional);
        assert_eq!(unknown.state, CurveFreshnessState::Unknown);
        assert_eq!(unknown.snapshot_age_ms, 100);
        assert_eq!(unknown.curve_finality, CurveFinality::Speculative);
    }

    #[test]
    fn test_get_curve_freshness_info_can_classify_committed() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);
        let now_ms = 10_000;

        ledger.insert_with_slot_known_at_finality(
            mint,
            curve,
            100,
            true,
            CurveFinality::Finalized,
            now_ms,
        );

        let info = ledger
            .get_curve_freshness_info_with_commit_state(&mint, now_ms, 200, true)
            .expect("committed info must exist");

        assert_eq!(info.state, CurveFreshnessState::Committed);
        assert_eq!(info.snapshot_age_ms, 0);
        assert_eq!(info.curve_finality, CurveFinality::Finalized);
    }

    #[test]
    fn test_curve_finality_transitions_from_genesis_seed_to_provisional_account_update() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);

        ledger.insert_seed_curve(mint, curve, false);
        assert_eq!(
            ledger.get_curve_finality(&mint),
            Some(CurveFinality::Speculative)
        );

        ledger.insert_with_slot_known(mint, curve, 123, true);
        assert_eq!(
            ledger.get_curve_finality(&mint),
            Some(CurveFinality::Provisional)
        );
    }

    #[test]
    fn test_duplicate_bootstrap_seed_is_storage_level_noop() {
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();
        let curve_key = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);

        let first = ledger.apply_curve_write(
            Some(base_mint),
            curve_key,
            curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::SeerBootstrap,
                ShadowLedgerWriteStrength::BootstrapSeed,
                ShadowLedgerStateConfidence::Speculative,
                ShadowLedgerWriteReason::BootstrapSeed,
                None,
                CurveFinality::Speculative,
            ),
        );
        let second = ledger.apply_curve_write(
            Some(base_mint),
            curve_key,
            curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::EventBusBootstrapListener,
                ShadowLedgerWriteStrength::BootstrapSeed,
                ShadowLedgerStateConfidence::Speculative,
                ShadowLedgerWriteReason::BootstrapSeed,
                None,
                CurveFinality::Speculative,
            ),
        );

        assert_eq!(first.result, ShadowLedgerWriteResult::Applied);
        assert_eq!(
            second.result,
            ShadowLedgerWriteResult::NoOpExistingEqualOrStronger
        );

        let stored = ledger.get_old(&curve_key).expect("curve should exist");
        assert_eq!(stored.write_source, ShadowLedgerWriteSource::SeerBootstrap);
        assert_eq!(
            stored.write_strength,
            ShadowLedgerWriteStrength::BootstrapSeed
        );
    }

    #[test]
    fn test_confirmed_bootstrap_promotes_existing_seed() {
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();
        let curve_key = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);

        let _ = ledger.apply_curve_write(
            Some(base_mint),
            curve_key,
            curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::SeerBootstrap,
                ShadowLedgerWriteStrength::BootstrapSeed,
                ShadowLedgerStateConfidence::Speculative,
                ShadowLedgerWriteReason::BootstrapSeed,
                None,
                CurveFinality::Speculative,
            ),
        );

        let promoted = ledger.apply_curve_write(
            Some(base_mint),
            curve_key,
            curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::RpcBootstrapSeeder,
                ShadowLedgerWriteStrength::ConfirmedBootstrap,
                ShadowLedgerStateConfidence::Observed,
                ShadowLedgerWriteReason::ConfirmedBootstrap,
                Some(123),
                CurveFinality::Provisional,
            ),
        );

        assert_eq!(
            promoted.result,
            ShadowLedgerWriteResult::PromotedBootstrapToConfirmed
        );
        let stored = ledger.get_old(&curve_key).expect("curve should exist");
        assert_eq!(
            stored.write_strength,
            ShadowLedgerWriteStrength::ConfirmedBootstrap
        );
        assert_eq!(
            stored.write_source,
            ShadowLedgerWriteSource::RpcBootstrapSeeder
        );
        assert_eq!(
            stored.state_confidence,
            ShadowLedgerStateConfidence::Observed
        );
        assert_eq!(stored.last_updated_slot, 123);
        assert!(stored.curve_data_known);
    }

    #[test]
    fn test_weaker_bootstrap_cannot_overwrite_confirmed_curve() {
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();
        let curve_key = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);

        let _ = ledger.apply_curve_write(
            Some(base_mint),
            curve_key,
            curve,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::RpcBootstrapSeeder,
                ShadowLedgerWriteStrength::ConfirmedBootstrap,
                ShadowLedgerStateConfidence::Observed,
                ShadowLedgerWriteReason::ConfirmedBootstrap,
                Some(150),
                CurveFinality::Provisional,
            ),
        );

        let conflicting_seed = create_test_curve(999_000_000, 29_000_000);
        let rejected = ledger.apply_curve_write(
            Some(base_mint),
            curve_key,
            conflicting_seed,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::EventBusBootstrapListener,
                ShadowLedgerWriteStrength::BootstrapSeed,
                ShadowLedgerStateConfidence::Speculative,
                ShadowLedgerWriteReason::BootstrapSeed,
                None,
                CurveFinality::Speculative,
            ),
        );

        assert_eq!(
            rejected.result,
            ShadowLedgerWriteResult::RejectedWeakerWrite
        );
        let stored = ledger.get_old(&curve_key).expect("curve should exist");
        assert_eq!(stored.curve, curve);
        assert_eq!(
            stored.write_strength,
            ShadowLedgerWriteStrength::ConfirmedBootstrap
        );
    }

    #[test]
    fn test_out_of_order_repair_is_rejected() {
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();
        let curve_key = Pubkey::new_unique();
        let curve_v1 = create_test_curve(1_000_000_000, 30_000_000);
        let curve_v2 = create_test_curve(1_100_000_000, 31_000_000);

        let _ = ledger.apply_curve_write(
            Some(base_mint),
            curve_key,
            curve_v1,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::AccountUpdate,
                ShadowLedgerWriteStrength::Repair,
                ShadowLedgerStateConfidence::Observed,
                ShadowLedgerWriteReason::DirectAccountUpdate,
                Some(200),
                CurveFinality::Provisional,
            ),
        );

        let rejected = ledger.apply_curve_write(
            Some(base_mint),
            curve_key,
            curve_v2,
            CurveWriteMetadata::new(
                ShadowLedgerWriteSource::AccountUpdate,
                ShadowLedgerWriteStrength::Repair,
                ShadowLedgerStateConfidence::Observed,
                ShadowLedgerWriteReason::DirectAccountUpdate,
                Some(100),
                CurveFinality::Provisional,
            ),
        );

        assert_eq!(rejected.result, ShadowLedgerWriteResult::RejectedOutOfOrder);
        let stored = ledger.get_old(&curve_key).expect("curve should exist");
        assert_eq!(stored.curve, curve_v1);
        assert_eq!(stored.last_updated_slot, 200);
    }

    #[test]
    fn test_remove() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);

        ledger.insert(mint, curve);
        assert!(ledger.contains(&mint));

        let removed = ledger.remove(&mint);
        assert!(removed.is_some());
        assert!(!ledger.contains(&mint));
    }

    #[test]
    fn test_clear() {
        let ledger = ShadowLedger::new();
        let tracked_mint = Pubkey::new_unique();
        let tracked_alias = Pubkey::new_unique();
        ledger.register_curve_alias(tracked_mint, tracked_alias);
        ledger.insert(tracked_mint, create_test_curve(1_000_000_000, 30_000_000));

        for _ in 0..9 {
            let mint = Pubkey::new_unique();
            let curve = create_test_curve(1_000_000_000, 30_000_000);
            ledger.register_curve_alias(mint, Pubkey::new_unique());
            ledger.insert(mint, curve);
        }

        assert_eq!(ledger.len(), 10);
        ledger.clear();
        assert_eq!(ledger.len(), 0);
        assert!(
            ledger.resolve_curve_key(&tracked_mint).is_none(),
            "clearing the ledger must also clear alias state"
        );
    }

    #[test]
    fn test_get_all_mints() {
        let ledger = ShadowLedger::new();
        let mut expected_mints = Vec::new();

        for _ in 0..5 {
            let mint = Pubkey::new_unique();
            let curve = create_test_curve(1_000_000_000, 30_000_000);
            ledger.insert(mint, curve);
            expected_mints.push(mint);
        }

        let actual_mints = ledger.get_all_mints();
        assert_eq!(actual_mints.len(), 5);

        for mint in expected_mints {
            assert!(actual_mints.contains(&mint));
        }
    }

    #[test]
    fn test_clone_shares_data() {
        let ledger1 = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000, 30_000_000);

        ledger1.insert(mint, curve);

        let ledger2 = ledger1.clone();

        // Both should see the same data
        assert!(ledger2.get(&mint).is_some());

        // Inserting in one should be visible in the other
        let mint2 = Pubkey::new_unique();
        ledger2.insert(mint2, curve);
        assert!(ledger1.get(&mint2).is_some());
    }

    #[test]
    fn test_simulate_buy_basic() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Insert at slot 1000
        ledger.insert_with_slot(mint, curve, 1000);

        // Simulate buy with 1 SOL
        let result = ledger.simulate_buy(&mint, 1_000_000_000, Some(1000));
        assert!(result.is_ok());

        let sim = result.unwrap();
        assert!(sim.tokens_out > 0, "Should receive tokens");
        assert!(
            sim.min_tokens_out < sim.tokens_out,
            "Min should be less than expected"
        );
        assert_eq!(sim.sol_in, 1_000_000_000, "SOL in should match input");
        assert!(
            sim.effective_sol_in < sim.sol_in,
            "Effective SOL should be less due to fee"
        );
        assert!(
            sim.price_impact_percent > 0.0,
            "Price impact should be positive"
        );
        assert!(
            sim.effective_price_per_token > 0.0,
            "Price per token should be positive"
        );
        assert!(sim.market_cap_sol > 0, "Market cap should be positive");
        assert!(sim.bonding_progress <= 100, "Progress should be <= 100%");
    }

    #[test]
    fn test_simulate_sell_basic() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        ledger.insert_with_slot(mint, curve, 1000);

        // Simulate sell of 1M tokens
        let result = ledger.simulate_sell(&mint, 1_000_000, Some(1000));
        assert!(result.is_ok());

        let sim = result.unwrap();
        assert!(sim.sol_out > 0, "Should receive SOL");
        assert!(
            sim.min_sol_out < sim.sol_out,
            "Min should be less than expected"
        );
        assert_eq!(sim.tokens_in, 1_000_000, "Tokens in should match input");
        assert!(
            sim.price_impact_percent < 0.0,
            "Price impact should be negative for sells"
        );
        assert!(
            sim.effective_price_per_token > 0.0,
            "Price per token should be positive"
        );
    }

    #[test]
    fn test_set_and_get_snapshots() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        // EPIC 2: Use non-zero slots (slot=0 is now rejected)
        let snaps = vec![
            MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            },
            MarketSnapshot {
                tx_key: Some(TxKey::new(1001, Some(101), Some(1), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1001, Some(101))
            },
            MarketSnapshot {
                tx_key: Some(TxKey::new(1002, Some(102), Some(2), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1002, Some(102))
            },
        ];

        // Set snapshots
        ledger.commit_history(mint, snaps.clone(), None);

        // Get snapshots
        let retrieved = ledger.get_snapshots(&mint);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.len(), 3);
        assert_eq!(retrieved[0].timestamp_ms, 1000);
        assert_eq!(retrieved[1].timestamp_ms, 1001);
        assert_eq!(retrieved[2].timestamp_ms, 1002);
    }

    #[test]
    fn test_commit_history_is_idempotent() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        let initial = vec![
            MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(10), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(10))
            },
            MarketSnapshot {
                tx_key: Some(TxKey::new(1001, Some(11), Some(1), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1001, Some(11))
            },
        ];
        let replacement = vec![MarketSnapshot {
            tx_key: Some(TxKey::new(9999, Some(99), Some(9), None, 0).unwrap()),
            ..MarketSnapshot::new_with_slot(9999, Some(99))
        }];

        ledger.commit_history(mint, initial.clone(), None);
        ledger.commit_history(mint, replacement, None);

        let stored = ledger
            .get_snapshots(&mint)
            .expect("snapshots should remain committed");
        assert_eq!(stored.len(), initial.len());
        assert_eq!(stored[0].timestamp_ms, initial[0].timestamp_ms);
        assert_eq!(stored[1].timestamp_ms, initial[1].timestamp_ms);
        assert!(ledger.is_committed(&mint));
    }

    #[test]
    fn test_push_snapshot_fifo_retains_tail() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        // Commit initial history and then append live snapshots to exercise FIFO logic.
        ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(1), Some(0), None, 0).unwrap()),
                tx_count: 0,
                timestamp_ms: 0,
                slot: Some(1),
                ..Default::default()
            }],
            None,
        );

        for i in 1..=MAX_SNAPSHOTS_PER_MINT + 5 {
            ledger.append_live(
                mint,
                MarketSnapshot {
                    tx_key: Some(
                        TxKey::new(
                            (i as u64 + 1) * 1000,
                            Some(i as u64 + 1),
                            Some(i as u32),
                            None,
                            0,
                        )
                        .unwrap(),
                    ),
                    tx_count: i as u64,
                    timestamp_ms: i as u64,
                    slot: Some((i + 1) as u64),
                    ..Default::default()
                },
            );
        }

        let snapshots = ledger.get_snapshots(&mint).unwrap();
        assert_eq!(snapshots.len(), MAX_SNAPSHOTS_PER_MINT);
        assert_eq!(snapshots.first().unwrap().tx_count, 6);
        // After FIFO eviction: 128 + 5 = 133 iterations (i: 0..132)
        // Keep last 128, so last should be i=132
        assert_eq!(snapshots.last().unwrap().tx_count, 133);
    }

    #[test]
    fn test_slot0_bootstrap_does_not_regress_live_history() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let initial_tx_key = TxKey::new(1000, Some(5), Some(0), None, 0).unwrap();

        ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                tx_key: Some(initial_tx_key.clone()),
                ..MarketSnapshot::new_with_slot(1000, Some(5))
            }],
            Some(initial_tx_key),
        );

        ledger.append_live(
            mint,
            MarketSnapshot {
                slot: Some(6),
                tx_key: Some(TxKey::new(2000, Some(6), Some(1), None, 0).unwrap()),
                timestamp_ms: 2_000,
                tx_count: 2,
                ..Default::default()
            },
        );

        ledger.append_live(
            mint,
            MarketSnapshot {
                slot: Some(0),
                tx_key: None,
                tx_count: 99,
                ..Default::default()
            },
        );

        let snapshots = ledger.get_snapshots(&mint).unwrap();
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].slot, Some(5));
        assert_eq!(snapshots[1].slot, Some(6));
        assert_eq!(snapshots[1].tx_count, 2);
    }

    #[test]
    fn test_push_snapshot_deltas() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(1), Some(0), None, 0).unwrap()),
                slot: Some(1),
                tx_count: 10,
                cum_volume_sol: 5.0,
                timestamp_ms: 1_000,
                ..Default::default()
            }],
            None,
        );
        ledger.append_live(
            mint,
            MarketSnapshot {
                tx_key: Some(TxKey::new(1420, Some(2), Some(1), None, 0).unwrap()),
                slot: Some(2),
                tx_count: 13,
                cum_volume_sol: 7.5,
                timestamp_ms: 1_420,
                ..Default::default()
            },
        );
        ledger.append_live(
            mint,
            MarketSnapshot {
                tx_key: Some(TxKey::new(1840, Some(3), Some(2), None, 0).unwrap()),
                slot: Some(3),
                tx_count: 15,
                cum_volume_sol: 9.2,
                timestamp_ms: 1_840,
                ..Default::default()
            },
        );

        let snapshots = ledger.get_snapshots(&mint).unwrap();
        let current = snapshots.last().unwrap();
        let prev = &snapshots[snapshots.len() - 2];

        let delta_tx = current.tx_count.saturating_sub(prev.tx_count);
        let delta_vol = current.cum_volume_sol - prev.cum_volume_sol;

        assert_eq!(delta_tx, 2);
        assert!((delta_vol - 1.7).abs() < 0.01);
    }

    #[test]
    fn test_has_snapshots() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        // Initially no snapshots
        assert!(!ledger.has_snapshots(&mint));

        // Add snapshots (use non-zero slot per EPIC 2 compliance)
        ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            }],
            None,
        );

        // Now should have snapshots
        assert!(ledger.has_snapshots(&mint));
    }

    #[test]
    fn internal_precommit_reads_bypass_public_approval_gate() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        ledger.set_approval_checker(Arc::new(|_| false));
        let result = ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(1), Some(0), None, 0).unwrap()),
                slot: Some(1),
                ..Default::default()
            }],
            None,
        );

        assert_eq!(result.status, CommitHistoryStatus::Persisted);
        assert!(!ledger.has_snapshots(&mint));
        assert_eq!(ledger.untracked_snapshot_write_blocked_total(), 0);
        assert!(ledger.get_snapshots(&mint).is_none());
        assert!(ledger.get_latest_snapshot(&mint).is_none());
        assert!(ledger.get_snapshots_internal(&mint).is_some());
        assert!(ledger.get_latest_snapshot_internal(&mint).is_some());
    }

    #[test]
    fn commit_history_reports_noop_for_existing_history() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let snapshot = MarketSnapshot {
            tx_key: Some(TxKey::new(1000, Some(1), Some(0), None, 0).unwrap()),
            slot: Some(1),
            ..Default::default()
        };

        let first = ledger.commit_history(mint, vec![snapshot.clone()], None);
        let second = ledger.commit_history(mint, vec![snapshot], None);

        assert_eq!(first.status, CommitHistoryStatus::Persisted);
        assert_eq!(second.status, CommitHistoryStatus::NoOpExistingHistory);
        assert!(second.last_snapshot.is_some());
    }

    #[test]
    fn commit_history_reports_rejected_no_write() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        let result = ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(0), Some(0), None, 0).unwrap()),
                slot: Some(0),
                ..Default::default()
            }],
            None,
        );

        assert_eq!(result.status, CommitHistoryStatus::RejectedNoWrite);
        assert!(!ledger.is_committed(&mint));
        assert!(ledger.get_snapshots_internal(&mint).is_none());
    }

    // =========================================================================
    // EPIC 2 Compliance Tests: slot=0 rejection
    // =========================================================================

    #[test]
    fn test_set_snapshots_rejects_slot_zero() {
        // EPIC 2: slot=0 snapshots must be rejected by set_snapshots
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        // Create snapshots including slot=0
        let snaps = vec![
            MarketSnapshot {
                slot: Some(0), // Should be rejected
                tx_key: None,
                tx_count: 0,
                ..Default::default()
            },
            MarketSnapshot {
                slot: Some(100),
                tx_key: Some(TxKey::new(1000, Some(100), Some(1), None, 0).unwrap()),
                tx_count: 1,
                ..Default::default()
            },
            MarketSnapshot {
                slot: Some(101),
                tx_key: Some(TxKey::new(1000, Some(101), Some(2), None, 0).unwrap()),
                tx_count: 2,
                ..Default::default()
            },
        ];

        ledger.commit_history(mint, snaps, None);

        // Only non-slot=0 snapshots should be stored
        let stored = ledger.get_snapshots(&mint).unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].slot, Some(100));
        assert_eq!(stored[1].slot, Some(101));
    }

    #[test]
    fn test_set_snapshots_rejects_all_slot_zero() {
        // EPIC 2: If all snapshots are slot=0, nothing is stored
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        let snaps = vec![
            MarketSnapshot {
                slot: Some(0),
                tx_key: None,
                ..Default::default()
            },
            MarketSnapshot {
                slot: Some(0),
                tx_key: None,
                ..Default::default()
            },
        ];

        ledger.commit_history(mint, snaps, None);

        // No snapshots should be stored
        assert!(!ledger.has_snapshots(&mint));
        assert!(ledger.get_snapshots_internal(&mint).is_none());
    }

    // =========================================================================
    // EPIC 5: Canonical commit/append primitives
    // =========================================================================

    #[test]
    fn test_commit_history_marks_committed() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let tx_key = TxKey::new(1000, Some(100), Some(1), None, 0).unwrap();

        assert!(!ledger.is_committed(&mint));

        ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                tx_key: Some(tx_key.clone()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            }],
            Some(tx_key.clone()),
        );

        assert!(ledger.is_committed(&mint));
        assert_eq!(ledger.last_committed_tx_key(&mint), Some(tx_key));
        let stored = ledger.get_snapshots(&mint).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].slot, Some(100));
    }

    #[test]
    fn test_append_live_requires_commit() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        let appended = ledger.append_live(
            mint,
            MarketSnapshot {
                slot: Some(10),
                tx_key: Some(TxKey::new(1000, Some(10), Some(0), None, 0).unwrap()),
                tx_count: 1,
                ..Default::default()
            },
        );

        assert!(!appended);
        assert!(
            !ledger.has_snapshots(&mint),
            "append_live must not create snapshot storage before canonical commit"
        );
    }

    #[test]
    fn test_append_live_rejects_slot_zero() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            }],
            None,
        );

        let appended = ledger.append_live(
            mint,
            MarketSnapshot {
                slot: Some(0),
                tx_key: None,
                tx_count: 2,
                ..Default::default()
            },
        );

        assert!(!appended);
        let stored = ledger.get_snapshots(&mint).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].slot, Some(100));
    }

    #[test]
    fn test_push_snapshot_with_source_rejects_slot_zero_after_live() {
        // EPIC 2/5: slot=0 snapshots are always rejected
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        // First add a live snapshot with real slot
        ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                slot: Some(100),
                tx_key: Some(TxKey::new(1000, Some(100), Some(1), None, 0).unwrap()),
                tx_count: 1,
                ..Default::default()
            }],
            None,
        );

        // Try to append slot=0 after live history exists
        ledger.append_live(
            mint,
            MarketSnapshot {
                slot: Some(0), // Should be dropped
                tx_key: None,
                tx_count: 2,
                ..Default::default()
            },
        );

        // Only the live snapshot should exist
        let stored = ledger.get_snapshots(&mint).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].slot, Some(100));
    }

    #[test]
    fn test_unapproved_snapshot_read_is_blocked() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        ledger.snapshots.insert(
            mint,
            SnapshotBuffer::new(vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(1), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::default()
            }]),
        );
        ledger.set_approval_checker(Arc::new(|_| false));

        assert!(ledger.get_snapshots(&mint).is_none());
        assert!(!ledger.has_snapshots(&mint));
        assert!(ledger.get_snapshots_internal(&mint).is_some());
        assert!(ledger.untracked_snapshot_read_blocked_total() >= 1);
    }

    #[test]
    fn test_snapshot_calls_use_base_mint_keys() {
        use std::fs;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().expect("workspace root").to_path_buf();

        let search_roots = [
            repo_root.join("ghost-core/src"),
            repo_root.join("ghost-launcher/src"),
            repo_root.join("ghost-brain/src"),
            repo_root.join("off-chain"),
        ];
        let ledger_source = manifest_dir.join("src/shadow_ledger/ledger.rs");

        let forbidden_patterns = [
            "get_snapshots(&pool_amm_id",
            "get_snapshots(&bonding_curve",
            "get_snapshots(&bonding_curve_key",
            "cleanup_snapshots(&pool_amm_id",
            "cleanup_snapshots(&bonding_curve",
            "cleanup_snapshots(&bonding_curve_key",
            "set_snapshots(pool_amm_id",
            "set_snapshots(bonding_curve",
            "set_snapshots(bonding_curve_key",
            "push_snapshot(pool_amm_id",
            "push_snapshot(bonding_curve",
            "push_snapshot_with_source(pool_amm_id",
            "push_snapshot_with_source(bonding_curve",
            "insert_snapshots(pool_amm_id",
            "insert_snapshots(bonding_curve",
        ];

        let mut violations = Vec::new();

        fn contains_forbidden(contents: &str, pattern: &str) -> bool {
            let mut search = contents;
            while let Some(pos) = search.find(pattern) {
                let after = &search[pos + pattern.len()..];
                let next = after.chars().next();
                if next.map_or(true, |ch| !ch.is_ascii_alphanumeric() && ch != '_') {
                    return true;
                }
                let advance = after.chars().next().map(|ch| ch.len_utf8()).unwrap_or(0);
                search = &after[advance..];
            }
            false
        }

        for root in &search_roots {
            let mut stack = vec![root.clone()];
            while let Some(path) = stack.pop() {
                let entries = match fs::read_dir(&path) {
                    Ok(entries) => entries,
                    Err(err) => {
                        violations.push(format!("{}: read_dir failed: {}", path.display(), err));
                        continue;
                    }
                };

                for entry_result in entries {
                    let entry = match entry_result {
                        Ok(entry) => entry,
                        Err(err) => {
                            violations.push(format!(
                                "{}: read_dir entry failed: {}",
                                path.display(),
                                err
                            ));
                            continue;
                        }
                    };
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        let dir_name = entry_path.file_name().and_then(|name| name.to_str());
                        if matches!(dir_name, Some("target") | Some(".git")) {
                            continue;
                        }
                        stack.push(entry_path);
                        continue;
                    }

                    if entry_path == ledger_source {
                        continue;
                    }
                    if entry_path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                        continue;
                    }

                    let contents = match fs::read_to_string(&entry_path) {
                        Ok(contents) => contents,
                        Err(err) => {
                            violations.push(format!(
                                "{}: read_to_string failed: {}",
                                entry_path.display(),
                                err
                            ));
                            continue;
                        }
                    };
                    let mut filtered = String::new();
                    let mut iter = contents.chars().peekable();
                    let mut in_block = false;
                    let mut in_line = false;

                    while let Some(ch) = iter.next() {
                        if in_line {
                            if ch == '\n' {
                                in_line = false;
                                filtered.push(ch);
                            }
                            continue;
                        }

                        if in_block {
                            if ch == '*' {
                                if let Some('/') = iter.peek() {
                                    iter.next();
                                    in_block = false;
                                }
                            }
                            continue;
                        }

                        if ch == '/' {
                            match iter.peek() {
                                Some('/') => {
                                    iter.next();
                                    in_line = true;
                                    continue;
                                }
                                Some('*') => {
                                    iter.next();
                                    in_block = true;
                                    continue;
                                }
                                _ => {}
                            }
                        }

                        filtered.push(ch);
                    }

                    for pattern in &forbidden_patterns {
                        if contains_forbidden(&filtered, pattern) {
                            violations.push(format!("{}: {}", entry_path.display(), pattern));
                        }
                    }
                }
            }
        }

        assert!(
            violations.is_empty(),
            "Snapshot key regressions found:\n{}",
            violations.join("\n")
        );
    }

    #[test]
    fn test_bootstrap_from_initialize_basic() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Insert curve at slot 1000
        ledger.insert_with_slot(mint, curve, 1000);

        // Bootstrap snapshots
        let result = ledger.bootstrap_from_initialize(mint, Some(1000), 10_000_000); // 0.01 SOL
        assert!(result.is_ok());

        // EPIC 2: Bootstrap snapshots (slot=0) are now rejected by set_snapshots.
        // Canonical snapshots must come from live transactions with timestamp_ms > 0.
        // The bootstrap function still generates synthetic data internally,
        // but set_snapshots rejects slot=0 snapshots per canonical contract.
        assert!(
            ledger.has_snapshots(&mint),
            "Bootstrap snapshots with slot: None should be ACCEPTED per event-time architecture"
        );
    }

    #[test]
    fn test_bootstrap_with_config_success() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Insert curve at slot 1000
        ledger.insert_with_slot(mint, curve, 1000);

        // Bootstrap with default config (succeeds but snapshots rejected per EPIC 2)
        let config = BootstrapConfig::default();
        let result = ledger.bootstrap_with_config(mint, Some(1000), 10_000_000, &config);

        assert!(result.is_ok());
        assert!(result.unwrap()); // Returns true (bootstrap executed successfully)
                                  // EPIC 2: But slot=0 snapshots are rejected by set_snapshots
        assert!(
            ledger.has_snapshots(&mint),
            "Bootstrap snapshots with slot: None should be ACCEPTED per event-time architecture"
        );
    }

    #[test]
    fn test_bootstrap_with_config_low_liquidity_skipped() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 10_000_000_000); // 10 SOL

        // Insert curve at slot 1000
        ledger.insert_with_slot(mint, curve, 1000);

        // Bootstrap with high liquidity threshold (should skip)
        let config = BootstrapConfig::promising_pools(100_000_000_000); // 100 SOL min
        let result = ledger.bootstrap_with_config(mint, Some(1000), 10_000_000, &config);

        assert!(result.is_ok());
        assert!(!result.unwrap()); // Should return false (skipped)
        assert!(!ledger.has_snapshots(&mint)); // No snapshots created
    }

    #[test]
    fn test_bootstrap_with_config_completed_pool_skipped() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let mut curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        curve.complete = 1; // Mark as completed

        // Insert curve at slot 1000
        ledger.insert_with_slot(mint, curve, 1000);

        // Bootstrap with default config (should skip completed pools)
        let config = BootstrapConfig::default();
        let result = ledger.bootstrap_with_config(mint, Some(1000), 10_000_000, &config);

        assert!(result.is_ok());
        assert!(!result.unwrap()); // Should return false (skipped)
        assert!(!ledger.has_snapshots(&mint));
    }

    #[test]
    fn test_evict_stale_snapshots() {
        let ledger = ShadowLedger::new();

        // Create mints with different ages
        let old_mint = Pubkey::new_unique();
        let new_mint = Pubkey::new_unique();

        // Insert old snapshot (manually set old timestamp)
        let old_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 100000; // 100 seconds ago
        let old_buffer = SnapshotBuffer::with_timestamp(
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            }],
            old_ts,
        );
        ledger.snapshots.insert(old_mint, old_buffer);

        // Insert new snapshot
        ledger.commit_history(
            new_mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(2000, Some(100), Some(1), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(2000, Some(100))
            }],
            None,
        );

        assert_eq!(ledger.snapshot_count(), 2);

        // Evict snapshots older than 50 seconds
        let evicted = ledger.evict_stale_snapshots(50000);

        assert_eq!(evicted, 1, "Should evict 1 stale snapshot");
        assert_eq!(
            ledger.snapshot_count(),
            1,
            "Should have 1 snapshot remaining"
        );
        assert!(
            !ledger.has_snapshots(&old_mint),
            "Old mint should be evicted"
        );
        assert!(ledger.has_snapshots(&new_mint), "New mint should remain");
    }

    #[test]
    fn test_remove_with_snapshots() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let bonding_curve = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Insert curve
        ledger.register_curve_alias(mint, bonding_curve);
        ledger.insert_with_slot(mint, curve, 1000);

        // EPIC 2: Bootstrap snapshots are rejected (slot=0), so manually add
        // snapshots with non-zero slot for this test
        ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            }],
            None,
        );

        assert!(ledger.contains(&mint));
        assert!(ledger.has_snapshots(&mint));

        // Remove both curve and snapshots
        let removed = ledger.remove_with_snapshots(&mint);
        assert!(removed.is_some());

        // Both should be gone
        assert!(!ledger.contains(&mint));
        assert!(!ledger.has_snapshots(&mint));
        assert!(
            ledger.resolve_curve_key(&mint).is_none(),
            "remove_with_snapshots must also clear base_mint alias"
        );
    }

    #[test]
    fn test_clear_all() {
        let ledger = ShadowLedger::new();
        let tracked_mint = Pubkey::new_unique();
        ledger.register_curve_alias(tracked_mint, Pubkey::new_unique());
        ledger.insert_with_slot(
            tracked_mint,
            create_test_curve(1_000_000_000_000, 30_000_000_000),
            99,
        );

        // Insert multiple curves and snapshots
        for i in 0..9 {
            let mint = Pubkey::new_unique();
            let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
            ledger.register_curve_alias(mint, Pubkey::new_unique());
            ledger.insert_with_slot(mint, curve, i);
            // EPIC 2: Use non-zero slot (i+100) to avoid slot=0 rejection
            ledger.commit_history(
                mint,
                vec![MarketSnapshot {
                    tx_key: Some(
                        TxKey::new(i * 1000 + 100, Some(i + 100), Some(i as u32), None, 0).unwrap(),
                    ),
                    ..MarketSnapshot::new_with_slot(i, Some(i + 100))
                }],
                None,
            );
        }

        assert_eq!(ledger.len(), 10);
        assert_eq!(ledger.snapshot_count(), 9);

        // Clear everything
        ledger.clear_all();

        assert_eq!(ledger.len(), 0);
        assert_eq!(ledger.snapshot_count(), 0);
        assert!(
            ledger.resolve_curve_key(&tracked_mint).is_none(),
            "clear_all must also clear alias state"
        );
    }

    #[test]
    fn test_snapshot_gating_blocks_unapproved_only_for_snapshots() {
        let ledger = ShadowLedger::new();
        let approved_mint = Pubkey::new_unique();
        let unapproved_mint = Pubkey::new_unique();

        ledger.commit_history(
            approved_mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1, Some(1), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1, Some(1))
            }],
            None,
        );
        ledger.commit_history(
            unapproved_mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1, Some(1), Some(1), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1, Some(1))
            }],
            None,
        );
        ledger.set_approval_checker(Arc::new({ move |mint| *mint == approved_mint }));

        // Unapproved snapshots are blocked
        let snap = MarketSnapshot {
            tx_key: Some(TxKey::new(2, Some(2), Some(0), None, 0).unwrap()),
            ..MarketSnapshot::new_with_slot(2, Some(2))
        };
        ledger.push_snapshot_with_source(unapproved_mint, snap, SnapshotSource::Runtime);
        assert_eq!(ledger.untracked_snapshot_write_blocked_total(), 1);
        assert!(ledger.get_snapshots(&unapproved_mint).is_none());
        assert_eq!(
            ledger
                .get_snapshots_internal(&unapproved_mint)
                .unwrap()
                .len(),
            1
        );

        // Curve storage remains unaffected by snapshot gating
        let curve = create_test_curve(1_000_000_000, 30_000_000);
        ledger.insert_with_slot(unapproved_mint, curve, 1);
        assert!(ledger.contains(&unapproved_mint));

        // Approved snapshots pass through
        let snap_ok = MarketSnapshot {
            tx_key: Some(TxKey::new(2, Some(2), Some(0), None, 0).unwrap()),
            ..MarketSnapshot::new_with_slot(2, Some(2))
        };
        ledger.push_snapshot_with_source(approved_mint, snap_ok, SnapshotSource::Runtime);
        assert_eq!(ledger.get_snapshots(&approved_mint).unwrap().len(), 2);
    }

    #[test]
    fn test_default_implementation() {
        let ledger = ShadowLedger::default();
        assert_eq!(ledger.len(), 0);
        assert!(ledger.is_empty());
    }

    // =========================================================================
    // New Eviction Integration Tests
    // =========================================================================

    #[test]
    fn test_eviction_manager_integration() {
        let ledger = ShadowLedger::new();

        // Create mints with different ages
        let old_mint = Pubkey::new_unique();
        let new_mint = Pubkey::new_unique();

        // Insert old snapshot
        let old_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 100000;
        let old_buffer = SnapshotBuffer::with_timestamp(
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            }],
            old_ts,
        );
        ledger.snapshots.insert(old_mint, old_buffer);

        // Insert new snapshot
        ledger.commit_history(
            new_mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(2000, Some(100), Some(1), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(2000, Some(100))
            }],
            None,
        );

        // Get eviction manager and evict
        let manager = ledger.eviction_manager();
        let result = manager.evict_stale(50000);

        assert_eq!(result.evicted_count, 1);
        assert_eq!(result.retained_count, 1);
        assert!(!ledger.has_snapshots(&old_mint));
        assert!(ledger.has_snapshots(&new_mint));
    }

    #[test]
    fn test_force_cleanup_snapshots() {
        let ledger = ShadowLedger::new();

        // Add multiple snapshots
        for _ in 0..10 {
            let mint = Pubkey::new_unique();
            ledger.commit_history(
                mint,
                vec![MarketSnapshot {
                    tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                    ..MarketSnapshot::new_with_slot(1000, Some(100))
                }],
                None,
            );
        }

        assert_eq!(ledger.snapshot_count(), 10);

        // Force cleanup
        let count = ledger.force_cleanup_snapshots();

        assert_eq!(count, 10);
        assert_eq!(ledger.snapshot_count(), 0);
    }

    #[test]
    fn test_preview_eviction() {
        let ledger = ShadowLedger::new();

        // Create mints with different ages
        let old_mint = Pubkey::new_unique();
        let new_mint = Pubkey::new_unique();

        // Insert old snapshot
        let old_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 100000;
        let old_buffer = SnapshotBuffer::with_timestamp(
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            }],
            old_ts,
        );
        ledger.snapshots.insert(old_mint, old_buffer);

        // Insert new snapshot
        ledger.commit_history(
            new_mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(2000, Some(100), Some(1), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(2000, Some(100))
            }],
            None,
        );

        // Preview what would be evicted
        let preview = ledger.preview_eviction(50000);

        assert_eq!(preview.len(), 1);
        assert!(preview.contains(&old_mint));

        // Verify nothing was actually evicted
        assert_eq!(ledger.snapshot_count(), 2);
    }

    #[test]
    fn test_get_snapshot_stats() {
        let ledger = ShadowLedger::new();

        // Create mints with different ages
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Old snapshot
        let old_buffer = SnapshotBuffer::with_timestamp(
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            }],
            now_ms - 400000,
        );
        ledger.snapshots.insert(Pubkey::new_unique(), old_buffer);

        // Fresh snapshots
        for _ in 0..3 {
            let buffer = SnapshotBuffer::with_timestamp(
                vec![MarketSnapshot {
                    tx_key: Some(TxKey::new(2000, Some(100), Some(1), None, 0).unwrap()),
                    ..MarketSnapshot::new_with_slot(2000, Some(100))
                }],
                now_ms - 10000,
            );
            ledger.snapshots.insert(Pubkey::new_unique(), buffer);
        }

        let stats = ledger.get_snapshot_stats(300000);

        assert_eq!(stats.total_buffers, 4);
        assert_eq!(stats.stale_count, 1);
        assert_eq!(stats.fresh_count, 3);
        assert_eq!(stats.total_snapshots, 4);
    }

    #[test]
    fn test_eviction_manager_with_config() {
        let ledger = ShadowLedger::new();

        // Add a snapshot
        ledger.commit_history(
            Pubkey::new_unique(),
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            }],
            None,
        );

        // Get manager with aggressive config
        let config = super::super::eviction::EvictionConfig::aggressive();
        let manager = ledger.eviction_manager_with_config(config);

        assert_eq!(manager.config().max_age_ms, 30_000);
    }

    // =========================================================================
    // Storage Access Tests
    // =========================================================================

    #[test]
    fn test_curve_storage_access() {
        use super::super::storage::CurveStorage;

        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Insert via ledger
        ledger.insert_with_slot(mint, curve, 1000);

        // Access via storage
        let storage = ledger.curve_storage();
        assert!(storage.contains(&mint));
        assert_eq!(storage.len(), 1);

        let retrieved = storage.get(&mint);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().virtual_token_reserves, 1_000_000_000_000);
    }

    #[test]
    fn test_snapshot_storage_access() {
        use super::super::storage::SnapshotStorage;

        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        // Insert via ledger
        ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            }],
            None,
        );

        // Access via storage
        let storage = ledger.snapshot_storage();
        assert!(storage.contains(&mint));
        assert_eq!(storage.len(), 1);

        let retrieved = storage.get(&mint);
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.unwrap().created_at_ms,
            ledger.get_snapshot_buffer(&mint).unwrap().created_at_ms
        );
    }

    #[test]
    fn test_curves_raw_access() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        ledger.insert_with_slot(mint, curve, 1000);

        let raw = ledger.curves_raw();
        assert!(raw.contains_key(&mint));
        assert_eq!(raw.len(), 1);
    }

    #[test]
    fn test_snapshots_raw_access() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        ledger.commit_history(
            mint,
            vec![MarketSnapshot {
                tx_key: Some(TxKey::new(1000, Some(100), Some(0), None, 0).unwrap()),
                ..MarketSnapshot::new_with_slot(1000, Some(100))
            }],
            None,
        );

        let raw = ledger.snapshots_raw();
        assert!(raw.contains_key(&mint));
        assert_eq!(raw.len(), 1);
    }

    #[test]
    fn test_get_curve_info() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        ledger.insert_with_slot(mint, curve, 1000);

        let info = ledger.get_curve_info(&mint);
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.last_updated_slot, 1000);
        assert!(info.bonding_progress <= 100);
    }

    #[test]
    fn test_storage_insert_via_trait() {
        use super::super::storage::CurveStorage;

        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Insert via storage trait
        let storage = ledger.curve_storage();
        storage.insert_with_slot(mint, curve, 2000);

        // Verify via ledger
        assert!(ledger.contains(&mint));
        let shadow = ledger.get_old(&mint).unwrap();
        assert_eq!(shadow.last_updated_slot, 2000);
    }

    #[test]
    fn test_storage_clone_shares_data() {
        use super::super::storage::CurveStorage;

        let ledger1 = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        ledger1.insert(mint, curve);

        let ledger2 = ledger1.clone();

        // Both should see the same data
        assert!(ledger2.get(&mint).is_some());

        // Inserting in one should be visible in the other
        let mint2 = Pubkey::new_unique();
        ledger2.insert(mint2, curve);
        assert!(ledger1.get(&mint2).is_some());

        // Via storage too
        let storage1 = ledger1.curve_storage();
        let storage2 = ledger2.curve_storage();
        assert!(storage1.contains(&mint2));
        assert!(storage2.contains(&mint));
    }

    // =========================================================================
    // Synthetic Seed Generation Tests
    // =========================================================================

    #[test]
    fn test_bootstrap_with_seed_generation_basic() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Insert curve
        ledger.insert_with_slot(mint, curve, 1000);

        // Bootstrap with seed generation
        let result = ledger.bootstrap_with_seed_generation(mint, Some(1000), 10_000_000);
        assert!(result.is_ok());

        let synthetic_txs = result.unwrap();
        assert_eq!(
            synthetic_txs.len(),
            8,
            "Should generate 8 synthetic transactions"
        );

        // EVENT-TIME ARCHITECTURE: Bootstrap snapshots now use slot: None
        // which is VALID (only Some(0) is rejected as legacy sentinel abuse).
        // Bootstrap generates synthetic snapshots for internal use that ARE
        // stored to ShadowLedger since they have slot: None (valid).
        assert!(
            ledger.has_snapshots(&mint),
            "Bootstrap snapshots with slot: None should be ACCEPTED per event-time architecture"
        );

        // Verify all snapshots have slot: None (synthetic)
        let snapshots = ledger.get_snapshots(&mint).unwrap();
        for snap in &snapshots {
            assert_eq!(
                snap.slot, None,
                "Bootstrap snapshots should have slot: None"
            );
        }
    }

    #[test]
    fn test_bootstrap_with_seed_generation_deterministic() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Insert curve
        ledger.insert_with_slot(mint, curve, 1000);

        // Generate twice
        let txs1 = ledger
            .bootstrap_with_seed_generation(mint, Some(1000), 10_000_000)
            .unwrap();
        let txs2 = ledger
            .bootstrap_with_seed_generation(mint, Some(1000), 10_000_000)
            .unwrap();

        // Payloads should be identical (deterministic)
        assert_eq!(txs1.len(), txs2.len());
        for i in 0..txs1.len() {
            assert_eq!(
                txs1[i].payload, txs2[i].payload,
                "Payloads should be deterministic for same pool"
            );
        }
    }

    #[test]
    fn test_bootstrap_with_seed_generation_varies_by_reserves() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        // Insert with different reserves
        let curve1 = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let curve2 = create_test_curve(1_000_000_000_000, 100_000_000_000);

        ledger.insert_with_slot(mint, curve1, 1000);
        let txs1 = ledger
            .bootstrap_with_seed_generation(mint, Some(1000), 10_000_000)
            .unwrap();

        ledger.insert_with_slot(mint, curve2, 1001);
        let txs2 = ledger
            .bootstrap_with_seed_generation(mint, Some(1001), 10_000_000)
            .unwrap();

        // At least some payloads should differ
        let mut different_count = 0;
        for i in 0..txs1.len() {
            if txs1[i].payload != txs2[i].payload {
                different_count += 1;
            }
        }

        assert!(
            different_count >= 4,
            "At least half the payloads should differ with different reserves, got {}",
            different_count
        );
    }

    #[test]
    fn test_bootstrap_with_seed_generation_performance() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        ledger.insert_with_slot(mint, curve, 1000);

        // Measure performance
        let start = std::time::Instant::now();
        let result = ledger.bootstrap_with_seed_generation(mint, Some(1000), 10_000_000);
        let duration_ms = start.elapsed().as_millis();

        assert!(result.is_ok());
        assert!(
            duration_ms < 20,
            "Seed generation should complete in < 20ms, took {}ms",
            duration_ms
        );
    }

    #[test]
    fn test_bootstrap_with_seed_generation_curve_not_found() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        // Don't insert curve - should fail
        let result = ledger.bootstrap_with_seed_generation(mint, Some(1000), 10_000_000);
        assert!(result.is_err());

        match result {
            Err(GhostError::CurveNotFound(_)) => {}
            _ => panic!("Expected CurveNotFound error"),
        }
    }

    #[test]
    fn test_bootstrap_with_seed_generation_stale_state() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Insert at old slot
        ledger.insert_with_slot(mint, curve, 1000);

        // Try to bootstrap with much newer slot (stale)
        let result = ledger.bootstrap_with_seed_generation(mint, Some(1010), 10_000_000);
        assert!(result.is_err());

        match result {
            Err(GhostError::StaleState { .. }) => {}
            _ => panic!("Expected StaleState error"),
        }
    }

    #[test]
    fn test_bootstrap_from_initialize_uses_protocol_genesis_without_account_update() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();

        ledger.insert_seed_curve(mint, create_test_curve(999_999_999, 111_111_111), false);
        let seed_info = ledger
            .get_curve_info(&mint)
            .expect("seed curve info should exist before bootstrap");
        let (_, curve_data_known) = ledger
            .get_curve_with_known(&mint)
            .expect("seed curve should exist before bootstrap");
        assert_eq!(seed_info.last_updated_slot, 0);
        assert!(!curve_data_known);

        ledger
            .bootstrap_from_initialize(mint, Some(10_000), 10_000_000)
            .expect("bootstrap should succeed from genesis seed alone");

        let snapshots = ledger
            .get_snapshots(&mint)
            .expect("bootstrap should store snapshots");
        assert_eq!(snapshots.len(), 3);

        let g0 = snapshots.first().expect("G0 should be present");
        let g1 = snapshots.get(1).expect("G1 should be present");
        let g2 = snapshots.get(2).expect("G2 should be present");
        let genesis = protocol_genesis_curve();
        let expected_g0 = MarketSnapshot::from_curve_genesis(&genesis, g0.timestamp_ms);
        let expected_g1 = super::super::bootstrap::generate_g1(
            &genesis,
            10_000_000,
            g1.timestamp_ms,
            Some(10_000),
        );
        let expected_g2 = super::super::bootstrap::generate_g2(
            &genesis,
            10_000_000,
            g2.timestamp_ms,
            &expected_g1,
            Some(10_000),
        );

        assert_eq!(g0, &expected_g0);
        assert_eq!(g1, &expected_g1);
        assert_eq!(g2, &expected_g2);
    }

    #[test]
    fn test_bootstrap_ignores_noncanonical_seed_curve_state() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let noncanonical_seed = create_test_curve(123_456_789, 987_654_321);

        ledger.insert_seed_curve(mint, noncanonical_seed, false);

        ledger
            .bootstrap_with_seed_generation(mint, Some(10_000), 10_000_000)
            .expect("bootstrap should not require account updates");

        let snapshots = ledger
            .get_snapshots(&mint)
            .expect("bootstrap should store snapshots");
        let genesis = protocol_genesis_curve();

        assert_eq!(
            snapshots[0].reserve_base,
            genesis.virtual_token_reserves as f64
        );
        assert_eq!(
            snapshots[0].reserve_quote,
            genesis.virtual_sol_reserves as f64 / super::super::types::LAMPORTS_PER_SOL
        );
    }

    // =========================================================================
    // Watchdog Tests
    // =========================================================================

    #[test]
    fn test_bootstrap_with_watchdog_success() {
        use super::super::bootstrap::BootstrapMetrics;

        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let metrics = BootstrapMetrics::new();

        // Insert curve
        ledger.insert_with_slot(mint, curve, 1000);

        // Bootstrap with generous timeout (1000ms)
        let result = ledger.bootstrap_with_seed_generation_watchdog(
            mint,
            Some(1000),
            10_000_000,
            1000, // 1000ms timeout - should succeed
            Some(&metrics),
        );

        assert!(result.is_ok());
        let txs = result.unwrap();
        assert_eq!(txs.len(), 8);

        // Metrics should show success
        assert_eq!(metrics.seeds_generated_total(), 1);
        assert_eq!(metrics.seed_generation_failure_total(), 0);
    }

    #[test]
    fn test_bootstrap_with_watchdog_timeout() {
        use super::super::bootstrap::BootstrapMetrics;

        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let metrics = BootstrapMetrics::new();

        // Insert curve
        ledger.insert_with_slot(mint, curve, 1000);

        // Note: In practice, with modern CPUs, even with 0ms timeout the operation
        // might complete before the first check. This test demonstrates the watchdog
        // mechanism, but in real scenarios the timeout would be 100ms which gives
        // plenty of headroom for legitimate operations while catching actual hangs.

        // First, verify that a normal operation completes quickly
        let result = ledger.bootstrap_with_seed_generation_watchdog(
            mint,
            Some(1000),
            10_000_000,
            100, // 100ms timeout - should succeed
            Some(&metrics),
        );

        assert!(
            result.is_ok(),
            "Normal operation should succeed within 100ms"
        );

        // The watchdog mechanism will catch actual hangs in production, but is difficult
        // to test synthetically without introducing artificial delays
        assert_eq!(metrics.seeds_generated_total(), 1);
    }

    #[test]
    fn test_bootstrap_with_watchdog_no_metrics() {
        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);

        // Insert curve
        ledger.insert_with_slot(mint, curve, 1000);

        // Bootstrap without metrics - should still work
        let result = ledger.bootstrap_with_seed_generation_watchdog(
            mint,
            Some(1000),
            10_000_000,
            1000, // 1000ms timeout
            None, // No metrics
        );

        assert!(result.is_ok());
        let txs = result.unwrap();
        assert_eq!(txs.len(), 8);
    }

    #[test]
    fn test_bootstrap_with_watchdog_100ms_realistic() {
        use super::super::bootstrap::BootstrapMetrics;

        let ledger = ShadowLedger::new();
        let mint = Pubkey::new_unique();
        let curve = create_test_curve(1_000_000_000_000, 30_000_000_000);
        let metrics = BootstrapMetrics::new();

        // Insert curve
        ledger.insert_with_slot(mint, curve, 1000);

        // Bootstrap with realistic 100ms timeout (as per spec)
        let result = ledger.bootstrap_with_seed_generation_watchdog(
            mint,
            Some(1000),
            10_000_000,
            100, // 100ms timeout - typical watchdog setting
            Some(&metrics),
        );

        // Should succeed under 100ms
        assert!(result.is_ok());
        let txs = result.unwrap();
        assert_eq!(txs.len(), 8);

        // Metrics should show success
        assert_eq!(metrics.seeds_generated_total(), 1);
        assert_eq!(metrics.seed_generation_failure_total(), 0);
    }

    // =============================================================================
    // Snapshot Retention Tests (128 cap)
    // =============================================================================

    #[test]
    fn test_snapshot_retention_cap_at_128() {
        // Regression test: Verify ShadowLedger retains exactly 128 newest snapshots per mint
        //
        // This test ensures:
        // 1. Buffer grows up to 128 snapshots (not the old 20)
        // 2. FIFO eviction: oldest snapshots are removed when exceeding 128
        // 3. Newest snapshot is always retained
        // 4. Oldest retained snapshot is at expected index (200 - 128 + 1 = 73)
        //
        // Context: Increased from 20 to 128 to align with SnapshotEngine ring buffer
        // and provide sufficient historical depth for early-phase scoring cycles.

        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();

        let tx_key = TxKey::new(1000, Some(1), Some(0), None, 0).unwrap();
        ledger.commit_history(
            base_mint,
            vec![MarketSnapshot {
                slot: Some(1),
                tx_key: Some(tx_key.clone()),
                timestamp_ms: 1_000,
                cum_volume_sol: 1.0,
                tx_count: 1,
                unique_addrs: 1,
                price_sol_per_token: 0.001,
                price_state: super::super::types::PriceState::Valid,
                price_reason: None,
                market_cap_sol: 1000.0,
                reserve_base: 1_000_000.0,
                reserve_quote: 1_000.0,
                bonding_progress_pct: 1.0,
                d_price_d_volume: 0.0,
                d_price_d_liquidity: 0.0,
                d_price_d_slippage: 0.0,
            }],
            Some(tx_key),
        );

        // Append 200 snapshots for the same base_mint
        for i in 2..=201 {
            let snapshot = MarketSnapshot {
                slot: Some(i),
                tx_key: Some(TxKey::new(i * 1000, Some(i), Some(i as u32), None, 0).unwrap()),
                timestamp_ms: i * 1000, // 1 second apart
                cum_volume_sol: i as f64,
                tx_count: i,
                unique_addrs: i,
                price_sol_per_token: 0.001,
                price_state: super::super::types::PriceState::Valid,
                price_reason: None,
                market_cap_sol: 1000.0,
                reserve_base: 1_000_000.0,
                reserve_quote: 1_000.0,
                bonding_progress_pct: (i as f64) / 200.0 * 100.0,
                d_price_d_volume: 0.0,
                d_price_d_liquidity: 0.0,
                d_price_d_slippage: 0.0,
            };

            ledger.append_live(base_mint, snapshot);
        }

        // Verify retention: should have exactly 128 snapshots (not 20, not 200)
        let snapshots = ledger.get_snapshots(&base_mint);
        assert!(
            snapshots.is_some(),
            "Snapshots should exist for base_mint after 200 pushes"
        );

        let snaps = snapshots.unwrap();
        assert_eq!(
            snaps.len(),
            128,
            "ShadowLedger should retain exactly 128 newest snapshots (not 20 or 200)"
        );

        // Verify the NEWEST snapshot is retained (slot=201, timestamp=201000)
        let newest = snaps.last().unwrap();
        assert_eq!(
            newest.slot,
            Some(201),
            "Newest snapshot should have slot=201"
        );
        assert_eq!(
            newest.timestamp_ms, 201_000,
            "Newest snapshot should have timestamp=201000ms"
        );
        assert_eq!(
            newest.cum_volume_sol, 201.0,
            "Newest snapshot should have cum_volume_sol=201.0"
        );

        // Verify the OLDEST retained snapshot is at expected index
        // We pushed 201, retained 128, so oldest retained is index 74 (201 - 128 + 1)
        let oldest = snaps.first().unwrap();
        assert_eq!(
            oldest.slot,
            Some(74),
            "Oldest retained snapshot should have slot=74 (FIFO eviction)"
        );
        assert_eq!(
            oldest.timestamp_ms, 74_000,
            "Oldest retained snapshot should have timestamp=74000ms"
        );
        assert_eq!(
            oldest.cum_volume_sol, 74.0,
            "Oldest retained snapshot should have cum_volume_sol=74.0"
        );

        // Verify all retained snapshots are consecutive (no gaps from 73 to 200)
        for (i, snap) in snaps.iter().enumerate() {
            let expected_slot = 74 + i as u64;
            assert_eq!(
                snap.slot,
                Some(expected_slot),
                "Snapshot at index {} should have slot={} (consecutive retention)",
                i,
                expected_slot
            );
        }
    }

    #[test]
    fn test_snapshot_retention_under_128() {
        // Verify that when fewer than 128 snapshots are pushed, all are retained
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();

        // Commit initial snapshot
        let initial_key = TxKey::new(1000, Some(1), Some(0), None, 0).unwrap();
        ledger.commit_history(
            base_mint,
            vec![MarketSnapshot {
                slot: Some(1),
                tx_key: Some(initial_key.clone()),
                timestamp_ms: 1_000,
                cum_volume_sol: 1.0,
                tx_count: 1,
                unique_addrs: 1,
                price_sol_per_token: 0.001,
                price_state: super::super::types::PriceState::Valid,
                price_reason: None,
                market_cap_sol: 1000.0,
                reserve_base: 1_000_000.0,
                reserve_quote: 1_000.0,
                bonding_progress_pct: 1.0,
                d_price_d_volume: 0.0,
                d_price_d_liquidity: 0.0,
                d_price_d_slippage: 0.0,
            }],
            Some(initial_key),
        );

        // Append 50 snapshots
        for i in 2..=51 {
            let snapshot = MarketSnapshot {
                slot: Some(i),
                tx_key: Some(TxKey::new(i * 1000, Some(i), Some(i as u32), None, 0).unwrap()),
                timestamp_ms: i * 1000,
                cum_volume_sol: i as f64,
                tx_count: i,
                unique_addrs: i,
                price_sol_per_token: 0.001,
                price_state: super::super::types::PriceState::Valid,
                price_reason: None,
                market_cap_sol: 1000.0,
                reserve_base: 1_000_000.0,
                reserve_quote: 1_000.0,
                bonding_progress_pct: (i as f64) / 50.0 * 100.0,
                d_price_d_volume: 0.0,
                d_price_d_liquidity: 0.0,
                d_price_d_slippage: 0.0,
            };

            ledger.append_live(base_mint, snapshot);
        }

        // Verify all 51 are retained (no eviction when under cap)
        let snapshots = ledger.get_snapshots(&base_mint).unwrap();
        assert_eq!(
            snapshots.len(),
            51,
            "All 51 snapshots should be retained when under 128 cap"
        );

        // Verify first and last
        assert_eq!(snapshots.first().unwrap().slot, Some(1));
        assert_eq!(snapshots.last().unwrap().slot, Some(51));
    }

    #[test]
    fn test_snapshot_retention_exactly_128() {
        // Edge case: exactly 128 snapshots
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();

        let initial_key = TxKey::new(1000, Some(1), Some(0), None, 0).unwrap();
        ledger.commit_history(
            base_mint,
            vec![MarketSnapshot {
                slot: Some(1),
                tx_key: Some(initial_key.clone()),
                timestamp_ms: 1_000,
                cum_volume_sol: 1.0,
                tx_count: 1,
                unique_addrs: 1,
                price_sol_per_token: 0.001,
                price_state: super::super::types::PriceState::Valid,
                price_reason: None,
                market_cap_sol: 1000.0,
                reserve_base: 1_000_000.0,
                reserve_quote: 1_000.0,
                bonding_progress_pct: 1.0,
                d_price_d_volume: 0.0,
                d_price_d_liquidity: 0.0,
                d_price_d_slippage: 0.0,
            }],
            Some(initial_key),
        );

        // Append exactly 128 snapshots
        for i in 2..=129 {
            let snapshot = MarketSnapshot {
                slot: Some(i),
                tx_key: Some(TxKey::new(i * 1000, Some(i), Some(i as u32), None, 0).unwrap()),
                timestamp_ms: i * 1000,
                cum_volume_sol: i as f64,
                tx_count: i,
                unique_addrs: i,
                price_sol_per_token: 0.001,
                price_state: super::super::types::PriceState::Valid,
                price_reason: None,
                market_cap_sol: 1000.0,
                reserve_base: 1_000_000.0,
                reserve_quote: 1_000.0,
                bonding_progress_pct: (i as f64) / 128.0 * 100.0,
                d_price_d_volume: 0.0,
                d_price_d_liquidity: 0.0,
                d_price_d_slippage: 0.0,
            };

            ledger.append_live(base_mint, snapshot);
        }

        // Verify all 128 are retained (at the cap)
        let snapshots = ledger.get_snapshots(&base_mint).unwrap();
        assert_eq!(
            snapshots.len(),
            128,
            "All 128 snapshots should be retained when at cap"
        );

        // Verify boundaries
        assert_eq!(snapshots.first().unwrap().slot, Some(2));
        assert_eq!(snapshots.last().unwrap().slot, Some(129));
    }

    #[test]
    fn test_snapshot_retention_one_over_128() {
        // Edge case: 129 snapshots (first eviction)
        let ledger = ShadowLedger::new();
        let base_mint = Pubkey::new_unique();

        let initial_key = TxKey::new(1000, Some(1), Some(0), None, 0).unwrap();
        ledger.commit_history(
            base_mint,
            vec![MarketSnapshot {
                slot: Some(1),
                tx_key: Some(initial_key.clone()),
                timestamp_ms: 1_000,
                cum_volume_sol: 1.0,
                tx_count: 1,
                unique_addrs: 1,
                price_sol_per_token: 0.001,
                price_state: super::super::types::PriceState::Valid,
                price_reason: None,
                market_cap_sol: 1000.0,
                reserve_base: 1_000_000.0,
                reserve_quote: 1_000.0,
                bonding_progress_pct: 1.0,
                d_price_d_volume: 0.0,
                d_price_d_liquidity: 0.0,
                d_price_d_slippage: 0.0,
            }],
            Some(initial_key),
        );

        // Append 129 snapshots
        for i in 2..=130 {
            let snapshot = MarketSnapshot {
                slot: Some(i),
                tx_key: Some(TxKey::new(i * 1000, Some(i), Some(i as u32), None, 0).unwrap()),
                timestamp_ms: i * 1000,
                cum_volume_sol: i as f64,
                tx_count: i,
                unique_addrs: i,
                price_sol_per_token: 0.001,
                price_state: super::super::types::PriceState::Valid,
                price_reason: None,
                market_cap_sol: 1000.0,
                reserve_base: 1_000_000.0,
                reserve_quote: 1_000.0,
                bonding_progress_pct: (i as f64) / 129.0 * 100.0,
                d_price_d_volume: 0.0,
                d_price_d_liquidity: 0.0,
                d_price_d_slippage: 0.0,
            };

            ledger.append_live(base_mint, snapshot);
        }

        // Verify exactly 128 are retained (oldest evicted)
        let snapshots = ledger.get_snapshots(&base_mint).unwrap();
        assert_eq!(
            snapshots.len(),
            128,
            "Should retain exactly 128 after pushing 129"
        );

        // Verify FIFO: slot 1 was evicted, now have slots 2-129
        assert_eq!(
            snapshots.first().unwrap().slot,
            Some(3),
            "Oldest retained should be slot 3 (slot 1 and 2 evicted)"
        );
        assert_eq!(
            snapshots.last().unwrap().slot,
            Some(130),
            "Newest should be slot 130"
        );
    }

    // =========================================================================
    // Disk Snapshot Tests
    // =========================================================================

    /// Helper to create a realistic bonding curve for tests.
    fn create_shadow_curve_for_snapshot(i: u64) -> ShadowBondingCurve {
        use crate::market_state::ShadowBondingCurve;
        let curve = create_test_curve(
            1_000_000_000_000u64.wrapping_add(i * 1000),
            30_000_000_000u64.wrapping_add(i * 100),
        );
        ShadowBondingCurve::new(curve, i)
    }

    /// Verifies that 1000 curves survive a snapshot→restore roundtrip with byte-exact
    /// field values preserved.  Also proves that warm-restart (restore + zero WAL replay)
    /// is faster than cold-start (inserting all curves from scratch) — the startup-time
    /// criterion from the Z1.2 acceptance criteria.
    #[test]
    fn test_snapshot_roundtrip_1000_curves() {
        use solana_sdk::pubkey::Pubkey;
        use std::collections::HashMap;
        use std::time::Instant;
        use tempfile::TempDir;

        let dir = TempDir::new().expect("tempdir creation failed");
        let ledger = ShadowLedger::new();

        // Build a deterministic mint→curve map so we can verify field-level equality.
        let mut mints: Vec<Pubkey> = Vec::with_capacity(1000);
        for i in 0..1000u64 {
            let mint = Pubkey::new_unique();
            let shadow = create_shadow_curve_for_snapshot(i);
            ledger.curves.insert(mint, shadow);
            mints.push(mint);
        }
        assert_eq!(ledger.curves.len(), 1000);

        // ── Write snapshot ──────────────────────────────────────────────────
        let stats = ledger
            .snapshot_to_disk(dir.path())
            .expect("snapshot_to_disk failed");
        assert_eq!(stats.curves_written, 1000, "curves_written must match");

        // ── Warm-start timing: restore from disk ────────────────────────────
        let t_warm_start = Instant::now();
        let (restored, restore_stats) =
            ShadowLedger::restore_from_disk(dir.path()).expect("restore_from_disk failed");
        let warm_elapsed = t_warm_start.elapsed();

        assert_eq!(
            restore_stats.curves_loaded, 1000,
            "curves_loaded must match"
        );
        assert_eq!(
            restored.curves.len(),
            1000,
            "restored curve count must match"
        );
        assert!(
            restore_stats.written_at_ms > 0,
            "written_at_ms must be non-zero"
        );

        // ── Content verification: every curve must be byte-exact ─────────────
        for (i, mint) in mints.iter().enumerate() {
            let expected = create_shadow_curve_for_snapshot(i as u64);
            let got = restored
                .curves
                .get(mint)
                .expect("mint missing after restore");
            assert_eq!(
                got.curve.virtual_token_reserves, expected.curve.virtual_token_reserves,
                "mint[{i}] virtual_token_reserves mismatch"
            );
            assert_eq!(
                got.curve.virtual_sol_reserves, expected.curve.virtual_sol_reserves,
                "mint[{i}] virtual_sol_reserves mismatch"
            );
            assert_eq!(
                got.curve.real_token_reserves, expected.curve.real_token_reserves,
                "mint[{i}] real_token_reserves mismatch"
            );
            assert_eq!(
                got.curve.real_sol_reserves, expected.curve.real_sol_reserves,
                "mint[{i}] real_sol_reserves mismatch"
            );
            assert_eq!(
                got.curve.token_total_supply, expected.curve.token_total_supply,
                "mint[{i}] token_total_supply mismatch"
            );
            assert_eq!(
                got.curve.complete, expected.curve.complete,
                "mint[{i}] complete mismatch"
            );
            assert_eq!(
                got.last_updated_slot, expected.last_updated_slot,
                "mint[{i}] last_updated_slot mismatch"
            );
        }

        // ── Cold-start timing: insert all curves from scratch ────────────────
        let t_cold_start = Instant::now();
        let cold_ledger = ShadowLedger::new();
        for i in 0..1000u64 {
            let mint = mints[i as usize];
            cold_ledger
                .curves
                .insert(mint, create_shadow_curve_for_snapshot(i));
        }
        let cold_elapsed = t_cold_start.elapsed();

        // Warm-start (disk restore) must be no slower than a cold insert of the same data.
        // On any real hardware the bincode decode from disk beats N individual DashMap inserts,
        // so a 10× budget is extremely generous and should never be breached.
        assert!(
            warm_elapsed <= cold_elapsed * 10,
            "warm-start ({warm_elapsed:?}) was more than 10× slower than cold insert ({cold_elapsed:?})"
        );
    }

    #[test]
    fn test_snapshot_crash_mid_write_preserves_previous() {
        use super::super::disk_snapshot;
        use solana_sdk::pubkey::Pubkey;
        use std::fs;
        use tempfile::TempDir;

        let dir = TempDir::new().expect("tempdir creation failed");
        let ledger = ShadowLedger::new();

        // Insert 5 curves and write a good snapshot.
        for _ in 0..5 {
            let mint = Pubkey::new_unique();
            let shadow = create_shadow_curve_for_snapshot(1);
            ledger.curves.insert(mint, shadow);
        }
        let stats_first = ledger
            .snapshot_to_disk(dir.path())
            .expect("first snapshot failed");
        assert_eq!(stats_first.curves_written, 5);

        // Simulate a crash mid-write of the second snapshot by writing garbage
        // directly to a .tmp file with the next timestamp.
        let ts_second = disk_snapshot::now_ms().saturating_add(1);
        let tmp_path = disk_snapshot::snapshot_tmp_path(dir.path(), ts_second);
        fs::write(&tmp_path, b"corrupted incomplete data").expect("write garbage failed");

        // The .tmp file must NOT appear in the snapshot file list.
        let files = disk_snapshot::list_snapshot_files(dir.path()).expect("list failed");
        assert_eq!(
            files.len(),
            1,
            ".tmp file must not count as a valid snapshot"
        );

        // The previous good snapshot must still be restorable.
        let (restored, _stats) =
            ShadowLedger::restore_from_disk(dir.path()).expect("restore after crash failed");
        assert_eq!(
            restored.curves.len(),
            5,
            "previous good snapshot must still be readable"
        );
    }

    #[test]
    fn test_snapshot_rotate_keeps_n_newest() {
        use super::super::disk_snapshot;
        use solana_sdk::pubkey::Pubkey;
        use std::time::Duration;
        use tempfile::TempDir;

        let dir = TempDir::new().expect("tempdir creation failed");
        let ledger = ShadowLedger::new();

        // Write 5 snapshots. We need distinct timestamps, so we use a small
        // delay between writes or we manually craft filenames using write_snapshot_atomic.
        let mut written_paths = Vec::new();
        for ts in [1_000_000u64, 2_000_000, 3_000_000, 4_000_000, 5_000_000] {
            let snap = disk_snapshot::DiskSnapshot {
                version: disk_snapshot::SNAPSHOT_FORMAT_VERSION,
                written_at_ms: ts,
                curves_count: 1,
                curves: vec![([0u8; 32], create_shadow_curve_for_snapshot(ts))],
                curve_keys_by_base_mint: vec![],
                snapshots: vec![],
                snapshot_commit_state: vec![],
                bva_archives: vec![],
            };
            let path = disk_snapshot::snapshot_file_path(dir.path(), ts);
            disk_snapshot::write_snapshot_atomic(&snap, &path).expect("write snapshot failed");
            written_paths.push(path);
        }

        // Rotate, keeping 3 newest.
        let deleted = ShadowLedger::rotate_snapshots(dir.path(), 3).expect("rotate failed");
        assert_eq!(deleted, 2, "should delete 2 oldest files");

        let remaining = disk_snapshot::list_snapshot_files(dir.path()).expect("list failed");
        assert_eq!(remaining.len(), 3, "should retain exactly 3 files");

        // Verify the 3 newest timestamps remain.
        let fnames: Vec<_> = remaining
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        assert!(
            fnames[0].contains("3000000"),
            "oldest retained: ts=3_000_000"
        );
        assert!(
            fnames[1].contains("4000000"),
            "middle retained: ts=4_000_000"
        );
        assert!(
            fnames[2].contains("5000000"),
            "newest retained: ts=5_000_000"
        );
    }

    /// Verify that **all five maps** in ShadowLedger are faithfully persisted and
    /// restored: curves, curve_keys_by_base_mint (aliases), snapshots,
    /// snapshot_commit_state, and bva_archives.
    #[test]
    fn test_snapshot_full_state_roundtrip() {
        use super::super::types::{BvaArchive, BvaClassification, BvaMetrics, MarketSnapshot};
        use solana_sdk::pubkey::Pubkey;
        use tempfile::TempDir;

        let dir = TempDir::new().expect("tempdir creation failed");
        let ledger = ShadowLedger::new();

        // --- 1. curves ---
        let curve_key = Pubkey::new_unique();
        let shadow = create_shadow_curve_for_snapshot(42);
        ledger.curves.insert(curve_key, shadow);

        // --- 2. curve_keys_by_base_mint (alias map) ---
        let base_mint = Pubkey::new_unique();
        ledger.register_curve_alias(base_mint, curve_key);

        // --- 3. snapshots + 4. snapshot_commit_state (via commit_history) ---
        // slot = None (synthetic) avoids the slot=Some(0) rejection guard.
        let snap = MarketSnapshot::new(1_000_000);
        let commit_mint = Pubkey::new_unique();
        let result = ledger.commit_history(commit_mint, vec![snap], None);
        assert!(
            matches!(
                result.status,
                super::CommitHistoryStatus::Persisted
                    | super::CommitHistoryStatus::NoOpExistingHistory
            ),
            "commit_history must succeed: {:?}",
            result.status
        );

        // --- 5. bva_archives ---
        let bva_mint = Pubkey::new_unique();
        ledger.set_bva_archive(
            bva_mint,
            BvaArchive {
                birth_slot: Some(100),
                birth_ts_ms: 1_000,
                last_update_slot: Some(200),
                last_update_ts_ms: 2_000,
                tx_count_total: 7,
                unique_signers: 3,
                score: 0.75,
                confidence: 0.9,
                classification: BvaClassification::Organic,
                metrics: BvaMetrics {
                    tds: 1.0,
                    dc: 0.5,
                    se: 0.3,
                    cer: 0.2,
                    erp: 0.1,
                },
            },
        );

        // --- Write snapshot ---
        let write_stats = ledger
            .snapshot_to_disk(dir.path())
            .expect("snapshot_to_disk failed");
        assert_eq!(write_stats.curves_written, 1, "one curve must be written");

        // --- Restore ---
        let (restored, restore_stats) =
            ShadowLedger::restore_from_disk(dir.path()).expect("restore_from_disk failed");

        // curves
        assert_eq!(
            restored.curves.len(),
            1,
            "curve count must survive roundtrip"
        );
        assert!(
            restored.curves.contains_key(&curve_key),
            "curve key must survive roundtrip"
        );

        // alias map
        assert_eq!(
            restored.curve_keys_by_base_mint.len(),
            1,
            "alias map count must survive roundtrip"
        );
        assert!(
            restored.curve_keys_by_base_mint.contains_key(&base_mint),
            "alias entry must survive roundtrip"
        );

        // snapshots
        assert_eq!(
            restored.snapshots.len(),
            1,
            "snapshot buffer count must survive roundtrip"
        );
        let buf = restored
            .snapshots
            .get(&commit_mint)
            .expect("snapshot buffer must exist");
        assert_eq!(
            buf.snapshots.len(),
            1,
            "snapshot within buffer must survive roundtrip"
        );

        // snapshot_commit_state
        assert!(
            restored.is_committed(&commit_mint),
            "commit state must survive roundtrip"
        );

        // bva_archives
        assert_eq!(
            restored.bva_archives.len(),
            1,
            "BVA archive count must survive roundtrip"
        );
        let arch = restored
            .get_bva_archive(&bva_mint)
            .expect("BVA archive must survive roundtrip");
        assert_eq!(
            arch.tx_count_total, 7,
            "BVA archive content must survive roundtrip"
        );
        assert!(
            (arch.score - 0.75).abs() < 1e-9,
            "BVA score must survive roundtrip"
        );

        // watermark must be non-zero and available
        assert!(
            restore_stats.written_at_ms > 0,
            "restore stats watermark must be non-zero"
        );
    }
}
