//! SnapshotStore — concurrent, per-pool snapshot storage with phase tracking.
//!
//! Uses `parking_lot::RwLock` for low-contention concurrent reads.
//! Phase transitions are enforced: once `Amm`, never revert.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use solana_sdk::pubkey::Pubkey;
use tracing::{debug, info, warn};

use super::config::SsotConfig;
use super::phase::{should_switch_to_amm, PoolPhase};
use super::quote_engine::{QuoteEngine, QuoteSide};
use super::snapshot::{PoolSnapshot, SnapshotSource};

// ─── Metrics ────────────────────────────────────────────────────────────────

/// Telemetry counters for SSOT snapshot operations.
#[derive(Debug, Default)]
pub struct SsotMetrics {
    /// Total snapshot updates by phase: bonding curve.
    pub snapshots_updated_bonding: AtomicU64,
    /// Total snapshot updates by phase: AMM.
    pub snapshots_updated_amm: AtomicU64,
    /// Total snapshot updates by source: Yellowstone.
    pub snapshots_source_yellowstone: AtomicU64,
    /// Total snapshot updates by source: PumpPortal.
    pub snapshots_source_pumpportal: AtomicU64,
    /// Total snapshot updates by source: FallbackRPC.
    pub snapshots_source_fallback: AtomicU64,
    /// Total staleness events detected.
    pub snapshot_stale_total: AtomicU64,
    /// Total phase switches (Bonding → Amm).
    pub phase_switches_total: AtomicU64,
}

/// Throttle state for per-pool logging (max 10 Hz per pool).
const LOG_THROTTLE_INTERVAL_MS: u64 = 100; // 10 Hz

/// Thread-safe snapshot store keyed by pool_id.
///
/// Enforces:
/// - Atomic snapshot updates via `RwLock`.
/// - One-way phase transitions (BondingCurve → Amm).
/// - PumpPortal updates rejected once phase == Amm (except activity signal).
/// - Staleness detection via config threshold.
/// - Throttled structured logging (max 10 Hz per pool).
/// - Metrics counters for observability.
pub struct SnapshotStore {
    inner: Arc<RwLock<HashMap<Pubkey, PoolSnapshot>>>,
    /// Per-pool phase memory (once Amm, stays Amm).
    phases: Arc<RwLock<HashMap<Pubkey, PoolPhase>>>,
    /// Per-pool last-log timestamp for throttling.
    last_log_ts: Arc<RwLock<HashMap<Pubkey, u64>>>,
    config: SsotConfig,
    /// Observable metrics counters.
    pub metrics: Arc<SsotMetrics>,
}

impl SnapshotStore {
    pub fn new(config: SsotConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            phases: Arc::new(RwLock::new(HashMap::new())),
            last_log_ts: Arc::new(RwLock::new(HashMap::new())),
            config,
            metrics: Arc::new(SsotMetrics::default()),
        }
    }

    /// Update snapshot for a bonding-curve-phase pool.
    ///
    /// If the pool is already in AMM phase, bonding-curve updates from
    /// PumpPortal are rejected (logged as warning).
    pub fn update_bonding(
        &self,
        pool_id: Pubkey,
        base_mint: Pubkey,
        v_sol: u64,
        v_tokens: u64,
        market_cap_sol: Option<f64>,
        source: SnapshotSource,
    ) {
        let now_ms = Self::now_ms();

        // Check phase — reject if already AMM
        {
            let phases = self.phases.read();
            if phases.get(&pool_id) == Some(&PoolPhase::Amm) {
                if source == SnapshotSource::PumpPortal {
                    debug!(
                        pool_id = %pool_id,
                        "SSOT: rejecting PumpPortal bonding update for AMM-phase pool"
                    );
                    return;
                }
            }
        }

        let snapshot = PoolSnapshot::new_bonding(
            pool_id,
            base_mint,
            v_sol,
            v_tokens,
            market_cap_sol,
            source,
            now_ms,
        );

        // Metrics
        self.metrics
            .snapshots_updated_bonding
            .fetch_add(1, Ordering::Relaxed);
        self.record_source_metric(source);

        // Throttled structured logging (max 10 Hz per pool)
        self.log_snapshot_update(&snapshot, now_ms);

        let mut store = self.inner.write();
        store.insert(pool_id, snapshot);
    }

    /// Update snapshot for an AMM-phase pool.
    ///
    /// Also transitions phase to Amm (irreversible).
    pub fn update_amm(
        &self,
        pool_id: Pubkey,
        base_mint: Pubkey,
        reserve_sol: u64,
        reserve_token: u64,
        fee_bps: Option<u16>,
        source: SnapshotSource,
    ) {
        let now_ms = Self::now_ms();

        // Guard: AMM phase with missing/zero reserves → treat as stale
        if reserve_sol == 0 || reserve_token == 0 {
            warn!(
                pool_id = %pool_id,
                base_mint = %base_mint,
                reserve_sol = reserve_sol,
                reserve_token = reserve_token,
                "SSOT: AMM update with zero reserves → ORACLE_STALE guard"
            );
            self.metrics
                .snapshot_stale_total
                .fetch_add(1, Ordering::Relaxed);
            return;
        }

        // Transition phase to AMM (one-way, deterministic, with reason)
        {
            let mut phases = self.phases.write();
            let prev = phases.insert(pool_id, PoolPhase::Amm);
            if prev != Some(PoolPhase::Amm) {
                let reason = if prev.is_none() {
                    "first_update_is_amm"
                } else {
                    "amm_accounts_resolved"
                };
                self.metrics
                    .phase_switches_total
                    .fetch_add(1, Ordering::Relaxed);
                info!(
                    pool_id = %pool_id,
                    base_mint = %base_mint,
                    reason = reason,
                    "PHASE_SWITCH Bonding→Amm"
                );
            }
        }

        let snapshot = PoolSnapshot::new_amm(
            pool_id,
            base_mint,
            reserve_sol,
            reserve_token,
            fee_bps,
            source,
            now_ms,
        );

        // Metrics
        self.metrics
            .snapshots_updated_amm
            .fetch_add(1, Ordering::Relaxed);
        self.record_source_metric(source);

        // Throttled structured logging (max 10 Hz per pool)
        self.log_snapshot_update(&snapshot, now_ms);

        let mut store = self.inner.write();
        store.insert(pool_id, snapshot);
    }

    /// Try to switch phase if conditions are met.
    ///
    /// Returns `true` if phase switched to AMM.
    pub fn try_phase_switch(
        &self,
        pool_id: Pubkey,
        amm_accounts_resolved: bool,
        migration_observed: bool,
        bonding_progress_pct: f64,
    ) -> bool {
        let current = {
            let phases = self.phases.read();
            phases
                .get(&pool_id)
                .copied()
                .unwrap_or(PoolPhase::BondingCurve)
        };

        if should_switch_to_amm(
            current,
            amm_accounts_resolved,
            migration_observed,
            bonding_progress_pct,
            self.config.bonding_progress_threshold_pct,
        ) {
            let reason = if migration_observed {
                "migration_event_observed"
            } else if amm_accounts_resolved {
                "amm_accounts_resolved"
            } else {
                "bonding_progress_threshold"
            };
            let mut phases = self.phases.write();
            phases.insert(pool_id, PoolPhase::Amm);
            self.metrics
                .phase_switches_total
                .fetch_add(1, Ordering::Relaxed);
            info!(
                pool_id = %pool_id,
                bonding_progress_pct = bonding_progress_pct,
                reason = reason,
                "PHASE_SWITCH Bonding→Amm"
            );
            true
        } else {
            false
        }
    }

    /// Get a clone of the latest snapshot for a pool.
    pub fn get(&self, pool_id: &Pubkey) -> Option<PoolSnapshot> {
        let store = self.inner.read();
        store.get(pool_id).cloned()
    }

    /// Get the current phase for a pool.
    pub fn phase(&self, pool_id: &Pubkey) -> PoolPhase {
        let phases = self.phases.read();
        phases
            .get(pool_id)
            .copied()
            .unwrap_or(PoolPhase::BondingCurve)
    }

    /// Check staleness for a pool snapshot.
    ///
    /// Returns `(is_stale, age_ms)`. If no snapshot exists, returns `(true, u64::MAX)`.
    /// Increments `snapshot_stale_total` metric when stale.
    pub fn check_staleness(&self, pool_id: &Pubkey) -> (bool, u64) {
        let now_ms = Self::now_ms();
        let store = self.inner.read();
        match store.get(pool_id) {
            Some(snap) => {
                let age = snap.age_ms(now_ms);
                let is_stale = age > self.config.stale_ms;
                if is_stale {
                    self.metrics
                        .snapshot_stale_total
                        .fetch_add(1, Ordering::Relaxed);
                }
                (is_stale, age)
            }
            None => {
                self.metrics
                    .snapshot_stale_total
                    .fetch_add(1, Ordering::Relaxed);
                (true, u64::MAX)
            }
        }
    }

    /// Compute a sell quote for the current snapshot using the QuoteEngine.
    ///
    /// Convenience method for logging / AEM integration. Returns `None` if
    /// no snapshot or reserves are invalid.
    pub fn quote_sell(
        &self,
        pool_id: &Pubkey,
        token_amount: u64,
    ) -> Option<super::quote_engine::Quote> {
        let snap = self.get(pool_id)?;
        QuoteEngine::quote(&snap, QuoteSide::Sell, token_amount, &self.config)
    }

    /// Get a unified SSOT price quote for a pool.
    ///
    /// Returns the current snapshot, the source (Curve or Amm), and an
    /// executable sell quote for a default trade size. This is the single
    /// point of truth for position manager pricing.
    pub fn get_price_quote(
        &self,
        pool_id: &Pubkey,
        trade_amount: u64,
        side: QuoteSide,
    ) -> Option<super::quote_engine::Quote> {
        let snap = self.get(pool_id)?;
        QuoteEngine::quote(&snap, side, trade_amount, &self.config)
    }

    /// Remove a pool from the store (used during unsubscribe/cleanup).
    pub fn remove(&self, pool_id: &Pubkey) {
        self.inner.write().remove(pool_id);
        self.phases.write().remove(pool_id);
        self.last_log_ts.write().remove(pool_id);
    }

    /// Number of tracked pools.
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Record source-specific metric counter.
    fn record_source_metric(&self, source: SnapshotSource) {
        match source {
            SnapshotSource::Yellowstone => {
                self.metrics
                    .snapshots_source_yellowstone
                    .fetch_add(1, Ordering::Relaxed);
            }
            SnapshotSource::PumpPortal => {
                self.metrics
                    .snapshots_source_pumpportal
                    .fetch_add(1, Ordering::Relaxed);
            }
            SnapshotSource::FallbackRpc => {
                self.metrics
                    .snapshots_source_fallback
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Throttled structured logging (max 10 Hz per pool).
    ///
    /// On each snapshot update emits:
    /// pool_id, base_mint, phase, source, age_ms,
    /// bonding: v_sol/v_tokens, amm: reserve_sol/reserve_token,
    /// price_mark, quote_sell_effective_price (for default trade size).
    fn log_snapshot_update(&self, snap: &PoolSnapshot, now_ms: u64) {
        // Throttle: max LOG_THROTTLE_INTERVAL_MS per pool
        {
            let mut last_ts = self.last_log_ts.write();
            let prev = last_ts.get(&snap.pool_id).copied().unwrap_or(0);
            if now_ms.saturating_sub(prev) < LOG_THROTTLE_INTERVAL_MS {
                return;
            }
            last_ts.insert(snap.pool_id, now_ms);
        }

        let age_ms = snap.age_ms(now_ms);

        // Compute quote_sell_effective_price for a default trade size (1 SOL worth of tokens)
        let quote_sell_eff = QuoteEngine::quote(snap, QuoteSide::Sell, 1_000_000_000, &self.config)
            .map(|q| q.effective_price)
            .unwrap_or(0.0);

        match snap.phase {
            PoolPhase::BondingCurve => {
                info!(
                    pool_id = %snap.pool_id,
                    base_mint = %snap.base_mint,
                    phase = "BondingCurve",
                    source = %snap.source,
                    age_ms = age_ms,
                    v_sol = snap.v_sol.unwrap_or(0),
                    v_tokens = snap.v_tokens.unwrap_or(0),
                    price_mark = snap.price_mark_sol_per_token,
                    quote_sell_effective_price = quote_sell_eff,
                    "SSOT snapshot update"
                );
            }
            PoolPhase::Amm => {
                info!(
                    pool_id = %snap.pool_id,
                    base_mint = %snap.base_mint,
                    phase = "Amm",
                    source = %snap.source,
                    age_ms = age_ms,
                    reserve_sol = snap.reserve_sol.unwrap_or(0),
                    reserve_token = snap.reserve_token.unwrap_or(0),
                    price_mark = snap.price_mark_sol_per_token,
                    quote_sell_effective_price = quote_sell_eff,
                    "SSOT snapshot update"
                );
            }
        }
    }

    /// Access the config.
    pub fn config(&self) -> &SsotConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> SsotConfig {
        SsotConfig::default()
    }

    #[test]
    fn test_bonding_update_and_get() {
        let store = SnapshotStore::new(default_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        store.update_bonding(
            pool,
            mint,
            30_000_000_000,
            1_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
        );

        let snap = store.get(&pool).expect("should exist");
        assert_eq!(snap.phase, PoolPhase::BondingCurve);
        assert_eq!(snap.v_sol, Some(30_000_000_000));
    }

    #[test]
    fn test_amm_update_transitions_phase() {
        let store = SnapshotStore::new(default_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        store.update_bonding(
            pool,
            mint,
            30_000_000_000,
            1_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
        );
        assert_eq!(store.phase(&pool), PoolPhase::BondingCurve);

        store.update_amm(
            pool,
            mint,
            50_000_000_000,
            200_000_000,
            Some(25),
            SnapshotSource::Yellowstone,
        );
        assert_eq!(store.phase(&pool), PoolPhase::Amm);

        let snap = store.get(&pool).expect("should exist");
        assert_eq!(snap.phase, PoolPhase::Amm);
        assert_eq!(snap.reserve_sol, Some(50_000_000_000));
    }

    #[test]
    fn test_pump_portal_rejected_after_amm() {
        let store = SnapshotStore::new(default_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // First set to AMM
        store.update_amm(
            pool,
            mint,
            50_000_000_000,
            200_000_000,
            None,
            SnapshotSource::Yellowstone,
        );

        // Try bonding update from PumpPortal — should be rejected
        store.update_bonding(
            pool,
            mint,
            99_000_000_000,
            999_000_000_000,
            None,
            SnapshotSource::PumpPortal,
        );

        let snap = store.get(&pool).expect("should exist");
        // Snapshot should still be the AMM one
        assert_eq!(snap.phase, PoolPhase::Amm);
        assert_eq!(snap.reserve_sol, Some(50_000_000_000));
    }

    #[test]
    fn test_staleness_check() {
        let store = SnapshotStore::new(SsotConfig {
            stale_ms: 1000,
            ..default_config()
        });
        let pool = Pubkey::new_unique();

        // No snapshot → stale
        let (is_stale, _) = store.check_staleness(&pool);
        assert!(is_stale);

        // Fresh snapshot → not stale
        let mint = Pubkey::new_unique();
        store.update_bonding(
            pool,
            mint,
            1_000_000_000,
            1_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
        );
        let (is_stale, age) = store.check_staleness(&pool);
        assert!(!is_stale);
        assert!(age < 100); // should be < 100ms since we just created it
    }

    #[test]
    fn test_phase_switch() {
        let store = SnapshotStore::new(default_config());
        let pool = Pubkey::new_unique();

        // No switch without conditions
        assert!(!store.try_phase_switch(pool, false, false, 50.0));
        assert_eq!(store.phase(&pool), PoolPhase::BondingCurve);

        // Switch on migration observed
        assert!(store.try_phase_switch(pool, false, true, 50.0));
        assert_eq!(store.phase(&pool), PoolPhase::Amm);
    }

    // ── Hardening: Metrics counters ─────────────────────────────────────

    #[test]
    fn test_metrics_counters_bonding() {
        let store = SnapshotStore::new(default_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        store.update_bonding(
            pool,
            mint,
            30_000_000_000,
            1_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
        );
        store.update_bonding(
            pool,
            mint,
            31_000_000_000,
            1_000_000_000_000,
            None,
            SnapshotSource::PumpPortal,
        );

        assert_eq!(
            store
                .metrics
                .snapshots_updated_bonding
                .load(Ordering::Relaxed),
            2
        );
        assert_eq!(
            store
                .metrics
                .snapshots_source_yellowstone
                .load(Ordering::Relaxed),
            1
        );
        assert_eq!(
            store
                .metrics
                .snapshots_source_pumpportal
                .load(Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn test_metrics_counters_amm() {
        let store = SnapshotStore::new(default_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        store.update_amm(
            pool,
            mint,
            50_000_000_000,
            200_000_000,
            Some(25),
            SnapshotSource::Yellowstone,
        );
        assert_eq!(
            store.metrics.snapshots_updated_amm.load(Ordering::Relaxed),
            1
        );
        assert_eq!(
            store.metrics.phase_switches_total.load(Ordering::Relaxed),
            1
        );
    }

    // ── Hardening: AMM zero reserves guard ──────────────────────────────

    #[test]
    fn test_amm_zero_reserves_rejected() {
        let store = SnapshotStore::new(default_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // Zero reserves → rejected (ORACLE_STALE guard)
        store.update_amm(
            pool,
            mint,
            0,
            200_000_000,
            None,
            SnapshotSource::Yellowstone,
        );
        assert!(store.get(&pool).is_none());
        assert_eq!(
            store.metrics.snapshot_stale_total.load(Ordering::Relaxed),
            1
        );

        // Also reject zero token reserves
        store.update_amm(
            pool,
            mint,
            50_000_000_000,
            0,
            None,
            SnapshotSource::Yellowstone,
        );
        assert!(store.get(&pool).is_none());
        assert_eq!(
            store.metrics.snapshot_stale_total.load(Ordering::Relaxed),
            2
        );
    }

    // ── Hardening: Phase switch is deterministic and irreversible ────────

    #[test]
    fn test_phase_switch_irreversible() {
        let store = SnapshotStore::new(default_config());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // Start bonding
        store.update_bonding(
            pool,
            mint,
            30_000_000_000,
            1_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
        );
        assert_eq!(store.phase(&pool), PoolPhase::BondingCurve);

        // Migrate to AMM
        store.update_amm(
            pool,
            mint,
            50_000_000_000,
            200_000_000_000,
            Some(25),
            SnapshotSource::Yellowstone,
        );
        assert_eq!(store.phase(&pool), PoolPhase::Amm);

        // Subsequent bonding update from PumpPortal REJECTED
        store.update_bonding(
            pool,
            mint,
            99_000_000_000,
            999_000_000_000,
            None,
            SnapshotSource::PumpPortal,
        );
        assert_eq!(store.phase(&pool), PoolPhase::Amm);
        let snap = store.get(&pool).unwrap();
        assert_eq!(snap.reserve_sol, Some(50_000_000_000)); // AMM snap persists
        assert_eq!(snap.reserve_token, Some(200_000_000_000));

        // Phase switch cannot go back
        assert!(!store.try_phase_switch(pool, false, false, 0.0));
        assert_eq!(store.phase(&pool), PoolPhase::Amm);
    }

    #[test]
    fn test_phase_switch_once_per_pool() {
        let store = SnapshotStore::new(default_config());
        let pool = Pubkey::new_unique();

        // First switch
        assert!(store.try_phase_switch(pool, true, false, 0.0));
        assert_eq!(
            store.metrics.phase_switches_total.load(Ordering::Relaxed),
            1
        );

        // Second attempt — already AMM, no switch
        assert!(!store.try_phase_switch(pool, true, true, 100.0));
        assert_eq!(
            store.metrics.phase_switches_total.load(Ordering::Relaxed),
            1
        );
    }

    // ── Hardening: Integration test — Phase switch + snapshot updates ────

    #[test]
    fn test_integration_phase_switch_snapshot_updates_quote_changes() {
        use super::super::quote_engine::{QuoteEngine, QuoteSide};

        let config = SsotConfig {
            bonding_fee_bps_default: 100,
            amm_fee_bps_default: 25,
            slippage_bps_default: 100,
            stale_ms: 1500,
            ..SsotConfig::default()
        };
        let store = SnapshotStore::new(config.clone());
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // (a) Bonding snapshot updates
        store.update_bonding(
            pool,
            mint,
            30_000_000_000,
            1_073_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
        );
        let snap_bc1 = store.get(&pool).unwrap();
        assert_eq!(snap_bc1.phase, PoolPhase::BondingCurve);
        let q_bc1 = QuoteEngine::quote(&snap_bc1, QuoteSide::Sell, 1_000_000_000, &config).unwrap();

        store.update_bonding(
            pool,
            mint,
            35_000_000_000,
            900_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
        );
        let snap_bc2 = store.get(&pool).unwrap();
        let q_bc2 = QuoteEngine::quote(&snap_bc2, QuoteSide::Sell, 1_000_000_000, &config).unwrap();

        // Quotes must differ after reserve change
        assert!(
            (q_bc1.effective_price - q_bc2.effective_price).abs() > 1e-15,
            "bonding quotes must change: q1={}, q2={}",
            q_bc1.effective_price,
            q_bc2.effective_price,
        );

        // (b) Trigger migration → AMM accounts become known
        assert!(store.try_phase_switch(pool, true, false, 0.0));
        assert_eq!(store.phase(&pool), PoolPhase::Amm);

        // (c) AMM reserves update #1
        store.update_amm(
            pool,
            mint,
            50_000_000_000,
            200_000_000_000,
            Some(25),
            SnapshotSource::Yellowstone,
        );
        let snap_amm1 = store.get(&pool).unwrap();
        assert_eq!(snap_amm1.phase, PoolPhase::Amm);
        let q_amm1 =
            QuoteEngine::quote(&snap_amm1, QuoteSide::Sell, 1_000_000_000, &config).unwrap();

        // (c) AMM reserves update #2 (different reserves)
        store.update_amm(
            pool,
            mint,
            55_000_000_000,
            180_000_000_000,
            Some(25),
            SnapshotSource::Yellowstone,
        );
        let snap_amm2 = store.get(&pool).unwrap();
        let q_amm2 =
            QuoteEngine::quote(&snap_amm2, QuoteSide::Sell, 1_000_000_000, &config).unwrap();

        // Price mark changes after each AMM reserve update
        assert!(
            (snap_amm1.price_mark_sol_per_token - snap_amm2.price_mark_sol_per_token).abs() > 1e-15,
            "AMM price_mark must change: p1={}, p2={}",
            snap_amm1.price_mark_sol_per_token,
            snap_amm2.price_mark_sol_per_token,
        );

        // Quote effective_price changes after each AMM reserve update
        assert!(
            (q_amm1.effective_price - q_amm2.effective_price).abs() > 1e-15,
            "AMM quotes must change: q1={}, q2={}",
            q_amm1.effective_price,
            q_amm2.effective_price,
        );
    }

    // ── Hardening: Stale test → ORACLE_STALE ────────────────────────────

    #[test]
    fn test_stale_snapshot_produces_oracle_stale() {
        let store = SnapshotStore::new(SsotConfig {
            stale_ms: 500, // very short for testing
            ..SsotConfig::default()
        });
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // Insert snapshot with past timestamp
        {
            let past_ms = SnapshotStore::now_ms().saturating_sub(2000);
            let snap = PoolSnapshot::new_bonding(
                pool,
                mint,
                30_000_000_000,
                1_073_000_000_000_000,
                None,
                SnapshotSource::Yellowstone,
                past_ms,
            );
            let mut inner = store.inner.write();
            inner.insert(pool, snap);
        }

        let (is_stale, age_ms) = store.check_staleness(&pool);
        assert!(
            is_stale,
            "snapshot should be stale (age={}ms > 500ms)",
            age_ms
        );
        assert!(age_ms >= 1900, "age should be >= 1900ms, got {}", age_ms);
        assert!(store.metrics.snapshot_stale_total.load(Ordering::Relaxed) > 0);
    }

    // ── Hardening: quote_sell convenience ────────────────────────────────

    #[test]
    fn test_quote_sell_convenience() {
        let config = SsotConfig {
            bonding_fee_bps_default: 100,
            slippage_bps_default: 100,
            ..SsotConfig::default()
        };
        let store = SnapshotStore::new(config);
        let pool = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        // No snapshot → None
        assert!(store.quote_sell(&pool, 1_000_000_000).is_none());

        // With snapshot → Some
        store.update_bonding(
            pool,
            mint,
            30_000_000_000,
            1_073_000_000_000_000,
            None,
            SnapshotSource::Yellowstone,
        );
        let q = store
            .quote_sell(&pool, 1_000_000_000)
            .expect("should have quote");
        assert!(q.expected_out > 0.0);
        assert!(q.effective_price > 0.0);
    }
}
