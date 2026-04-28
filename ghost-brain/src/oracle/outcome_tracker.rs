//! OutcomeTracker — passive outcome aggregator for post-hoc labeling of
//! Gatekeeper BUY decisions.
//!
//! # Overview
//! After the Gatekeeper emits `verdict_type == BUY`, the OutcomeTracker:
//! 1. Records the pool as "tracked" (`add`).
//! 2. Updates aggregated outcome statistics from every subsequent PumpPortal
//!    WS transaction for that pool (`update_from_tx`).
//! 3. At 30-minute and 12-hour checkpoints writes partial / final outcome
//!    records to `outcomes.jsonl` (`flush_due`).
//!
//! **No additional RPC calls are made** — all data comes from the existing WS
//! stream.
//!
//! # Labels computed
//! * `label_market_12h`   — TRUE when mcap/tx activity indicates a real market.
//! * `label_toxic_12h`    — TRUE when mcap collapsed ≤5 % of its peak at 12 h.
//! * `label_upside_proxy` — TRUE when `mcap_peak_30m / mcap_10s ≥ threshold`.
//!
//! # Output format (`outcomes.jsonl`)
//! One JSON object per line — see [`OutcomeRecord`] for the full schema.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs::{create_dir_all, OpenOptions};
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Configuration thresholds
// ─────────────────────────────────────────────────────────────────────────────

/// Labeling thresholds (all configurable at runtime).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeThresholds {
    /// label_market_12h: min mcap peak (SOL) in first 30 min.
    pub mcap_peak_30m_min: f64,
    /// label_market_12h: min transaction count in 30 min.
    pub tx_30m_min: u64,
    /// label_market_12h: min unique signers in 30 min.
    pub signers_30m_min: u64,
    /// label_market_12h: min mcap peak (SOL) in 12 h.
    pub mcap_peak_12h_min: f64,
    /// label_toxic_12h: mcap_last/mcap_peak ratio below which pool is "toxic".
    pub last_to_peak_ratio_min: f64,
    /// label_upside_proxy: mcap_peak_30m / mcap_10s threshold.
    pub upside_30m_min: f64,
}

impl Default for OutcomeThresholds {
    fn default() -> Self {
        Self {
            mcap_peak_30m_min: 50.0,
            tx_30m_min: 20,
            signers_30m_min: 10,
            mcap_peak_12h_min: 30.0,
            last_to_peak_ratio_min: 0.05,
            upside_30m_min: 2.0,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-pool tracking state
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum number of unique signers kept in the exact HashSet per window.
///
/// Once the cap is reached new signers are silently dropped from the set, so
/// `unique_signers_*` reports the true count up to CAP and then stays at CAP.
/// This limits RAM to ~2 kB per window per pool while still providing
/// statistically meaningful diversity metrics for labeling purposes.
const SIGNER_CAP: usize = 2_000;

/// A single pool being tracked by the OutcomeTracker.
#[derive(Debug, Clone)]
pub struct TrackedPool {
    /// Stable join key: `"{pool}:{base_mint}:{first_seen_ts_ms}"`
    pub join_key: String,
    pub pool: String,
    pub base_mint: String,

    /// End of the 10-second Gatekeeper observation window (ms since epoch).
    pub t0_10s_end_ts_ms: u64,
    /// 30-minute checkpoint: t0 + 30 min (ms).
    pub t1_30m_ts_ms: u64,
    /// 12-hour checkpoint: t0 + 12 h (ms).
    pub t2_12h_ts_ms: u64,

    /// Market cap at end of Gatekeeper window (SOL), if known.
    pub mcap_10s: Option<f64>,

    // ── 30-minute window aggregates ───────────────────────────────────────
    pub mcap_peak_30m: f64,
    pub mcap_last_30m: f64,
    pub tx_count_30m: u64,
    pub volume_sol_30m: f64,
    pub data_points_30m: u64,
    signers_30m: HashSet<String>,

    // ── 12-hour window aggregates ─────────────────────────────────────────
    pub mcap_peak_12h: f64,
    pub mcap_last_12h: f64,
    pub tx_count_12h: u64,
    pub volume_sol_12h: f64,
    pub data_points_12h: u64,
    signers_12h: HashSet<String>,

    /// Last transaction timestamp seen (ms).
    pub last_seen_ts_ms: u64,

    pub outcome_30m_written: bool,
    pub outcome_12h_written: bool,
}

impl TrackedPool {
    /// Create a new `TrackedPool`.
    pub fn new(
        join_key: String,
        pool: String,
        base_mint: String,
        t0_10s_end_ts_ms: u64,
        mcap_10s: Option<f64>,
    ) -> Self {
        let t1 = t0_10s_end_ts_ms + 30 * 60 * 1_000;
        let t2 = t0_10s_end_ts_ms + 12 * 3600 * 1_000;
        let init_mcap = mcap_10s.unwrap_or(0.0);
        Self {
            join_key,
            pool,
            base_mint,
            t0_10s_end_ts_ms,
            t1_30m_ts_ms: t1,
            t2_12h_ts_ms: t2,
            mcap_10s,
            mcap_peak_30m: init_mcap,
            mcap_last_30m: init_mcap,
            tx_count_30m: 0,
            volume_sol_30m: 0.0,
            data_points_30m: 0,
            signers_30m: HashSet::new(),
            mcap_peak_12h: init_mcap,
            mcap_last_12h: init_mcap,
            tx_count_12h: 0,
            volume_sol_12h: 0.0,
            data_points_12h: 0,
            signers_12h: HashSet::new(),
            last_seen_ts_ms: t0_10s_end_ts_ms,
            outcome_30m_written: false,
            outcome_12h_written: false,
        }
    }

    /// Ingest a single transaction event.
    ///
    /// `mcap_sol` is optional — if `None` the mcap statistics are not updated but
    /// tx-count, volume, and signer diversity are always recorded.
    pub fn update(
        &mut self,
        ts_ms: u64,
        mcap_sol: Option<f64>,
        signer: Option<&str>,
        sol_amount: f64,
    ) {
        self.last_seen_ts_ms = ts_ms;

        // 30-minute window
        if ts_ms <= self.t1_30m_ts_ms {
            self.tx_count_30m += 1;
            self.volume_sol_30m += sol_amount;
            self.data_points_30m += 1;
            if let Some(mcap) = mcap_sol {
                if mcap > self.mcap_peak_30m {
                    self.mcap_peak_30m = mcap;
                }
                self.mcap_last_30m = mcap;
            }
            if let Some(s) = signer {
                if self.signers_30m.len() < SIGNER_CAP {
                    self.signers_30m.insert(s.to_string());
                }
            }
        }

        // 12-hour window (superset of 30 m)
        if ts_ms <= self.t2_12h_ts_ms {
            self.tx_count_12h += 1;
            self.volume_sol_12h += sol_amount;
            self.data_points_12h += 1;
            if let Some(mcap) = mcap_sol {
                if mcap > self.mcap_peak_12h {
                    self.mcap_peak_12h = mcap;
                }
                self.mcap_last_12h = mcap;
            }
            if let Some(s) = signer {
                if self.signers_12h.len() < SIGNER_CAP {
                    self.signers_12h.insert(s.to_string());
                }
            }
        }
    }

    /// Unique signers count in 30-minute window.
    pub fn unique_signers_30m(&self) -> u64 {
        self.signers_30m.len() as u64
    }

    /// Unique signers count in 12-hour window.
    pub fn unique_signers_12h(&self) -> u64 {
        self.signers_12h.len() as u64
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Output record
// ─────────────────────────────────────────────────────────────────────────────

/// Status of an outcome record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OutcomeStatus {
    /// Full data with activity in both windows.
    Ok,
    /// No transactions observed after the Gatekeeper window closed.
    LowData,
    /// Partial record written at the 30-minute checkpoint.
    Partial30m,
}

/// A single outcome record written to `outcomes.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeRecord {
    // ── Identity ─────────────────────────────────────────────────────────
    pub join_key: String,
    pub pool: String,
    pub base_mint: String,

    // ── Timestamps ───────────────────────────────────────────────────────
    pub t0_end_10s: u64,
    pub t1_30m: u64,
    pub t2_12h: u64,

    // ── Baseline ─────────────────────────────────────────────────────────
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcap_10s: Option<f64>,

    // ── 30-minute window ─────────────────────────────────────────────────
    pub mcap_peak_30m: f64,
    pub mcap_last_30m: f64,
    pub tx_count_30m: u64,
    pub unique_signers_30m: u64,
    pub volume_sol_30m: f64,
    pub data_points_30m: u64,

    // ── 12-hour window ───────────────────────────────────────────────────
    pub mcap_peak_12h: f64,
    pub mcap_last_12h: f64,
    pub tx_count_12h: u64,
    pub unique_signers_12h: u64,
    pub volume_sol_12h: f64,
    pub data_points_12h: u64,

    // ── Labels ───────────────────────────────────────────────────────────
    /// TRUE: pool formed a real market (all four threshold conditions met).
    pub label_market_12h: bool,
    /// TRUE: mcap collapsed to ≤5 % of peak within 12 h.
    /// `null` when 12 h data is unavailable.
    pub label_toxic_12h: Option<bool>,
    /// TRUE: mcap_peak_30m ≥ upside_30m_min × mcap_10s.
    pub label_upside_proxy: bool,

    // ── Metadata ─────────────────────────────────────────────────────────
    pub outcome_status: OutcomeStatus,
}

impl OutcomeRecord {
    /// Build an `OutcomeRecord` from a `TrackedPool` using the given thresholds.
    pub fn from_tracked(pool: &TrackedPool, thresholds: &OutcomeThresholds) -> Self {
        let label_market_12h = pool.mcap_peak_30m >= thresholds.mcap_peak_30m_min
            && pool.tx_count_30m >= thresholds.tx_30m_min
            && pool.unique_signers_30m() >= thresholds.signers_30m_min
            && pool.mcap_peak_12h >= thresholds.mcap_peak_12h_min;

        let label_toxic_12h = if pool.data_points_12h > 0 && pool.mcap_peak_12h > 0.0 {
            let ratio = pool.mcap_last_12h / pool.mcap_peak_12h;
            Some(ratio <= thresholds.last_to_peak_ratio_min)
        } else {
            None
        };

        let label_upside_proxy = pool.mcap_10s.map_or(false, |m10| {
            // Use 0.001 SOL as a meaningful minimum to avoid division near zero.
            m10 > 0.001 && pool.mcap_peak_30m / m10 >= thresholds.upside_30m_min
        });

        let outcome_status = if pool.data_points_12h == 0 {
            OutcomeStatus::LowData
        } else {
            OutcomeStatus::Ok
        };

        Self {
            join_key: pool.join_key.clone(),
            pool: pool.pool.clone(),
            base_mint: pool.base_mint.clone(),
            t0_end_10s: pool.t0_10s_end_ts_ms,
            t1_30m: pool.t1_30m_ts_ms,
            t2_12h: pool.t2_12h_ts_ms,
            mcap_10s: pool.mcap_10s,
            mcap_peak_30m: pool.mcap_peak_30m,
            mcap_last_30m: pool.mcap_last_30m,
            tx_count_30m: pool.tx_count_30m,
            unique_signers_30m: pool.unique_signers_30m(),
            volume_sol_30m: pool.volume_sol_30m,
            data_points_30m: pool.data_points_30m,
            mcap_peak_12h: pool.mcap_peak_12h,
            mcap_last_12h: pool.mcap_last_12h,
            tx_count_12h: pool.tx_count_12h,
            unique_signers_12h: pool.unique_signers_12h(),
            volume_sol_12h: pool.volume_sol_12h,
            data_points_12h: pool.data_points_12h,
            label_market_12h,
            label_toxic_12h,
            label_upside_proxy,
            outcome_status,
        }
    }

    /// Build a partial (30-minute checkpoint) record.
    pub fn partial_30m(pool: &TrackedPool, thresholds: &OutcomeThresholds) -> Self {
        let mut rec = Self::from_tracked(pool, thresholds);
        rec.outcome_status = OutcomeStatus::Partial30m;
        rec
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// OutcomeTracker
// ─────────────────────────────────────────────────────────────────────────────

/// Passive aggregator that tracks Gatekeeper BUY pools and writes outcome records.
///
/// Use from a single task or wrap in `Arc<tokio::sync::Mutex<>>`.
pub struct OutcomeTracker {
    pools: HashMap<String, TrackedPool>,
    thresholds: OutcomeThresholds,
    log_dir: PathBuf,
}

impl OutcomeTracker {
    /// Create a new `OutcomeTracker`.
    pub fn new(log_dir: impl Into<PathBuf>, thresholds: OutcomeThresholds) -> Self {
        Self {
            pools: HashMap::new(),
            thresholds,
            log_dir: log_dir.into(),
        }
    }

    /// Add a pool to the tracked set.
    ///
    /// `t0_10s_end_ts_ms` is the timestamp (ms) when the Gatekeeper finished
    /// its observation window (= `first_seen_ts_ms + max_wait_time_ms`).
    pub fn add(
        &mut self,
        join_key: String,
        pool: String,
        base_mint: String,
        t0_10s_end_ts_ms: u64,
        mcap_10s: Option<f64>,
    ) {
        if self.pools.contains_key(&pool) {
            debug!("OutcomeTracker: pool {} already tracked, skipping", pool);
            return;
        }
        info!(
            pool = %pool,
            base_mint = %base_mint,
            join_key = %join_key,
            "OutcomeTracker: tracking pool for outcome labeling"
        );
        let tp = TrackedPool::new(
            join_key,
            pool.clone(),
            base_mint,
            t0_10s_end_ts_ms,
            mcap_10s,
        );
        self.pools.insert(pool, tp);
    }

    /// Update aggregated statistics from an incoming transaction event.
    ///
    /// `mcap_sol` is optional — if `None` the mcap statistics are not updated
    /// but tx-count, volume, and signer diversity are always recorded.
    pub fn update_from_tx(
        &mut self,
        pool: &str,
        ts_ms: u64,
        mcap_sol: Option<f64>,
        signer: Option<&str>,
        sol_amount: f64,
    ) {
        if let Some(tp) = self.pools.get_mut(pool) {
            tp.update(ts_ms, mcap_sol, signer, sol_amount);
        }
    }

    /// Write outcome records for all pools whose checkpoints have passed.
    ///
    /// Returns join-keys of pools that were fully finalized (12 h) and removed.
    /// Call this from a periodic task (e.g. every 60 seconds).
    pub async fn flush_due(&mut self, now_ts_ms: u64) -> Vec<String> {
        let mut finalized = Vec::new();

        for tp in self.pools.values_mut() {
            // 30-minute partial record
            if !tp.outcome_30m_written && now_ts_ms >= tp.t1_30m_ts_ms {
                let rec = OutcomeRecord::partial_30m(tp, &self.thresholds);
                match append_outcome(&self.log_dir, &rec).await {
                    Ok(_) => {
                        tp.outcome_30m_written = true;
                        info!(join_key = %tp.join_key, "OutcomeTracker: 30m partial written");
                    }
                    Err(e) => warn!(
                        join_key = %tp.join_key,
                        "OutcomeTracker: failed to write 30m outcome: {}",
                        e
                    ),
                }
            }

            // 12-hour final record
            if !tp.outcome_12h_written && now_ts_ms >= tp.t2_12h_ts_ms {
                let rec = OutcomeRecord::from_tracked(tp, &self.thresholds);
                match append_outcome(&self.log_dir, &rec).await {
                    Ok(_) => {
                        tp.outcome_12h_written = true;
                        info!(
                            join_key = %tp.join_key,
                            label_market_12h = rec.label_market_12h,
                            label_toxic_12h = ?rec.label_toxic_12h,
                            "OutcomeTracker: 12h final written"
                        );
                        finalized.push(tp.join_key.clone());
                    }
                    Err(e) => warn!(
                        join_key = %tp.join_key,
                        "OutcomeTracker: failed to write 12h outcome: {}",
                        e
                    ),
                }
            }
        }

        // Remove finalized pools
        for jk in &finalized {
            self.pools.retain(|_, v| &v.join_key != jk);
        }

        finalized
    }

    /// Number of currently tracked pools.
    pub fn tracked_count(&self) -> usize {
        self.pools.len()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSONL writer
// ─────────────────────────────────────────────────────────────────────────────

async fn append_outcome(log_dir: &Path, record: &OutcomeRecord) -> Result<()> {
    create_dir_all(log_dir)
        .await
        .context("Failed to create outcomes log directory")?;

    let log_path = log_dir.join("outcomes.jsonl");

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .await
        .context("Failed to open outcomes.jsonl")?;

    let json = serde_json::to_string(record).context("Failed to serialize outcome record")?;
    file.write_all(json.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.flush().await?;

    debug!("Outcome record written to {:?}", log_path);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: build join_key
// ─────────────────────────────────────────────────────────────────────────────

/// Build the stable join-key used in both `GatekeeperBuyLog` and `OutcomeTracker`.
pub fn build_join_key(pool: &str, base_mint: &str, first_seen_ts_ms: u64) -> String {
    format!("{}:{}:{}", pool, base_mint, first_seen_ts_ms)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_tracker(dir: &Path) -> OutcomeTracker {
        OutcomeTracker::new(dir, OutcomeThresholds::default())
    }

    #[test]
    fn test_build_join_key() {
        let jk = build_join_key("pool1", "mint1", 1_700_000_000_000);
        assert_eq!(jk, "pool1:mint1:1700000000000");
    }

    #[test]
    fn test_tracked_pool_update_30m() {
        let t0 = 1_700_000_000_000u64;
        let mut tp = TrackedPool::new("jk".into(), "pool".into(), "mint".into(), t0, Some(10.0));

        tp.update(t0 + 60_000, Some(15.0), Some("signer_a"), 1.0);
        tp.update(t0 + 120_000, Some(20.0), Some("signer_b"), 2.0);

        assert_eq!(tp.tx_count_30m, 2);
        assert_eq!(tp.mcap_peak_30m, 20.0);
        assert_eq!(tp.mcap_last_30m, 20.0);
        assert!((tp.volume_sol_30m - 3.0).abs() < f64::EPSILON);
        assert_eq!(tp.unique_signers_30m(), 2);
    }

    #[test]
    fn test_tracked_pool_after_30m_window() {
        let t0 = 1_700_000_000_000u64;
        let mut tp = TrackedPool::new("jk".into(), "pool".into(), "mint".into(), t0, Some(10.0));

        let after_30m = t0 + 30 * 60 * 1_000 + 1_000;
        tp.update(after_30m, Some(50.0), Some("late_signer"), 5.0);

        assert_eq!(tp.tx_count_30m, 0, "Should not count outside 30m window");
        assert_eq!(tp.tx_count_12h, 1, "Should count inside 12h window");
        assert_eq!(tp.mcap_peak_12h, 50.0);
        assert_eq!(tp.unique_signers_30m(), 0);
        assert_eq!(tp.unique_signers_12h(), 1);
    }

    #[test]
    fn test_outcome_record_labels_market_pass() {
        let t0 = 1_700_000_000_000u64;
        let thresholds = OutcomeThresholds::default();
        let mut tp = TrackedPool::new("jk".into(), "pool".into(), "mint".into(), t0, Some(10.0));

        // 25 tx inside 30m window — mcap grows well above thresholds
        for i in 0..25u64 {
            tp.update(
                t0 + i * 60_000,
                Some(60.0 + i as f64),
                Some(&format!("signer_{}", i)),
                0.5,
            );
        }
        // Provide some 12h activity to satisfy mcap_peak_12h_min
        tp.update(t0 + 1_000_000, Some(80.0), Some("late"), 1.0);

        let rec = OutcomeRecord::from_tracked(&tp, &thresholds);
        assert!(rec.label_market_12h, "Should pass market label");
        assert!(
            rec.label_upside_proxy,
            "Peak/baseline should exceed threshold"
        );
    }

    #[test]
    fn test_outcome_record_labels_market_fail() {
        let t0 = 1_700_000_000_000u64;
        let thresholds = OutcomeThresholds::default();
        let tp = TrackedPool::new("jk".into(), "pool".into(), "mint".into(), t0, Some(10.0));

        let rec = OutcomeRecord::from_tracked(&tp, &thresholds);
        assert!(!rec.label_market_12h, "Should fail with no data");
        assert!(!rec.label_upside_proxy, "No upside without data");
        assert_eq!(rec.outcome_status, OutcomeStatus::LowData);
    }

    #[test]
    fn test_outcome_record_toxic_label() {
        let t0 = 1_700_000_000_000u64;
        let thresholds = OutcomeThresholds::default();
        let mut tp = TrackedPool::new("jk".into(), "pool".into(), "mint".into(), t0, Some(10.0));

        tp.update(t0 + 60_000, Some(1000.0), Some("buyer"), 1.0);
        tp.update(t0 + 3_600_000, Some(10.0), Some("dumper"), 0.1); // last/peak = 0.01

        let rec = OutcomeRecord::from_tracked(&tp, &thresholds);
        assert_eq!(rec.label_toxic_12h, Some(true), "Should be toxic");
    }

    #[tokio::test]
    async fn test_flush_due_writes_partial_and_final() {
        let tmp = TempDir::new().unwrap();
        let mut tracker = make_tracker(tmp.path());

        let t0 = 1_700_000_000_000u64;
        let jk = build_join_key("pool1", "mint1", t0);
        tracker.add(
            jk.clone(),
            "pool1".to_string(),
            "mint1".to_string(),
            t0,
            Some(25.0),
        );

        for i in 0..31u64 {
            tracker.update_from_tx(
                "pool1",
                t0 + i * 60_000,
                Some(30.0 + i as f64),
                Some("s"),
                0.5,
            );
        }

        // Flush at t1 (30 min)
        let t1 = t0 + 30 * 60 * 1_000;
        let finalized = tracker.flush_due(t1).await;
        assert!(finalized.is_empty(), "Should not finalize at 30m");

        let partial_path = tmp.path().join("outcomes.jsonl");
        assert!(partial_path.exists(), "outcomes.jsonl should be created");

        let content = tokio::fs::read_to_string(&partial_path).await.unwrap();
        assert!(
            content.contains("PARTIAL_30M"),
            "Should contain partial status"
        );

        // Flush at t2 (12 h)
        let t2 = t0 + 12 * 3_600 * 1_000;
        let finalized = tracker.flush_due(t2).await;
        assert_eq!(finalized.len(), 1, "Should finalize after 12h flush");

        let content = tokio::fs::read_to_string(&partial_path).await.unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2, "Should have partial + final records");
    }

    #[test]
    fn test_add_duplicate_pool_ignored() {
        let tmp = TempDir::new().unwrap();
        let mut tracker = make_tracker(tmp.path());

        let t0 = 1_700_000_000_000u64;
        let jk = build_join_key("pool1", "mint1", t0);
        tracker.add(
            jk.clone(),
            "pool1".to_string(),
            "mint1".to_string(),
            t0,
            Some(10.0),
        );
        tracker.add(
            jk,
            "pool1".to_string(),
            "mint1".to_string(),
            t0 + 1_000,
            Some(20.0),
        );

        assert_eq!(
            tracker.tracked_count(),
            1,
            "Duplicate add should be ignored"
        );
    }
}
