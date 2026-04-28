//! Early Fingerprint Metrics — gRPC / Yellowstone-derived manipulation signals.
//!
//! Computes bounded-window execution fingerprints from real runtime / tx-meta data.
//! Existing metrics are preserved and extended with hybrid market-structure signals:
//!
//! 1. **block0_sniped_supply_pct** — supply bought in the creation slot
//! 2. **flip_ratio_10s** — fraction of buyers that dump ≥ 50 % within the window
//! 3. **cu_price_p90_1s / cu_price_p90_10s / priority_fee_surge_slope**
//! 4. **buyer_pre_balance_cv** — coefficient of variation of buyer SOL balances
//! 5. **avg_inner_ix_count_50tx / avg_cpi_depth_50tx** — inner-instruction complexity
//! 6. **sell_buy_ratio** — sell pressure vs buy flow
//! 7. **compute_unit_cluster_dominance** — CU-consumption clustering density
//! 8. **static_fee_profile_ratio** — repeated exact `(cu_limit, cu_price)` profiles
//! 9. **fixed_size_buy_ratio / fixed_size_buy_ratio_1e4** — cloned size buckets
//! 10. **flipper_presence_ratio** — owners that both buy and sell in-window
//! 11. **jito_tip_intensity** — fraction of tx with deterministic Jito tips
//! 12. **early_slot_volume_dominance_buy** — buy-volume concentration near birth
//! 13. **early_top3_buy_volume_pct_3s** — top-3 buyer concentration in the first 3 seconds
//! 14. **whale_reversal_ratio_top3 / whale_reversal_ratio_top1** — top buyers distributing
//! 15. **dev_paperhand_latency_ms / dev_sold_within_3s / dev_sold_within_5s**
//!
//! All metrics are deterministic, O(events) or O(events log events) with bounded
//! memory, and robust to missing optional fields.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const EARLY_TOP3_BUY_WINDOW_MS: u64 = 3_000;

// ═══════════════════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════════════════

/// Tunable knobs for the fingerprint aggregator (all have sensible defaults).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarlyFingerprintConfig {
    /// Window duration in seconds (default 10).
    pub window_secs: u64,
    /// Max transactions to consider for CPI metrics (default 50).
    pub max_tx_for_cpi: usize,
    /// Minimum percentage of supply for a buy to count (default 0.001 = 0.1 %).
    pub dust_supply_pct: f64,
    /// Dump-threshold for a "flip" wallet (default 0.50 = 50 %).
    pub flip_dump_pct: f64,
    /// Max slot delta between first buy and last sell for a flip (default 20).
    pub max_flip_slots: u64,
    /// Max wallets tracked per mint (default 4096).
    pub max_wallets: usize,
    /// Min buyers needed before computing pre-balance CV (default 10).
    pub min_buyers_for_cv: usize,
    /// Pre-balance CV window in seconds (default 5).
    pub cv_window_secs: u64,
    /// Whether to subtract ATA-creation boilerplate from inner ix count.
    pub subtract_ata_boilerplate: bool,
    /// CU-cluster tolerance as +/- percentage around a reference CU value.
    pub cu_cluster_tolerance_pct: f64,
    /// Minimum number of tx with CU-consumed data for CU clustering.
    pub min_tx_for_cu_cluster: usize,
    /// Static fee profile observation window in seconds from pool birth.
    pub static_fee_window_secs: u64,
    /// Minimum number of BUY tx with exact fee profiles to emit the metric.
    pub min_buy_txs_for_static_fee: usize,
    /// Minimum number of BUY tx required for fixed-size buy clustering.
    pub min_buy_txs_for_fixed_size: usize,
    /// Minimum number of owner wallets required for flipper presence ratio.
    pub min_wallets_for_flipper: usize,
    /// Minimum number of tx with known Jito-tip status.
    pub min_tx_for_jito_tip: usize,
    /// Number of slots from creation used for early-slot volume dominance.
    pub early_slot_count: u64,
    /// Ignore developer sells below this raw token threshold.
    pub dev_sell_dust_token_amount_raw: u64,
}

impl Default for EarlyFingerprintConfig {
    fn default() -> Self {
        Self {
            window_secs: 10,
            max_tx_for_cpi: 50,
            dust_supply_pct: 0.001,
            flip_dump_pct: 0.50,
            max_flip_slots: 20,
            max_wallets: 4096,
            min_buyers_for_cv: 10,
            cv_window_secs: 5,
            subtract_ata_boilerplate: false,
            cu_cluster_tolerance_pct: 0.05,
            min_tx_for_cu_cluster: 8,
            static_fee_window_secs: 3,
            min_buy_txs_for_static_fee: 5,
            min_buy_txs_for_fixed_size: 5,
            min_wallets_for_flipper: 5,
            min_tx_for_jito_tip: 5,
            early_slot_count: 3,
            dev_sell_dust_token_amount_raw: 1_000_000,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Output struct
// ═══════════════════════════════════════════════════════════════════════════

/// Compact output produced once the observation window ends.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EarlyFingerprintMetrics {
    pub block0_sniped_supply_pct: Option<f64>,
    pub flip_ratio_10s: Option<f64>,
    pub cu_price_p90_1s: Option<f64>,
    pub cu_price_p90_10s: Option<f64>,
    pub priority_fee_surge_slope: Option<f64>,
    pub buyer_pre_balance_cv: Option<f64>,
    pub avg_inner_ix_count_50tx: Option<f64>,
    pub avg_cpi_depth_50tx: Option<f64>,
    pub sell_buy_ratio: Option<f64>,
    pub compute_unit_cluster_dominance: Option<f64>,
    pub static_fee_profile_ratio: Option<f64>,
    pub fixed_size_buy_ratio: Option<f64>,
    pub fixed_size_buy_ratio_1e4: Option<f64>,
    pub flipper_presence_ratio: Option<f64>,
    pub jito_tip_intensity: Option<f64>,
    pub early_slot_volume_dominance_buy: Option<f64>,
    pub early_top3_buy_volume_pct_3s: Option<f64>,
    pub whale_reversal_ratio_top3: Option<f64>,
    pub whale_reversal_ratio_top1: Option<f64>,
    pub dev_paperhand_latency_ms: Option<u64>,
    pub dev_sold_within_3s: Option<bool>,
    pub dev_sold_within_5s: Option<bool>,
    pub fingerprint_degraded: bool,
    pub fingerprint_reason: Option<String>,
}

impl EarlyFingerprintMetrics {
    /// One-line summary for structured logging.
    pub fn log_line(&self, pool: &str, mint: &str) -> String {
        fn fmt_f64(v: &Option<f64>) -> String {
            match v {
                Some(x) => format!("{:.4}", x),
                None => "null".to_string(),
            }
        }
        fn fmt_u64(v: &Option<u64>) -> String {
            match v {
                Some(x) => x.to_string(),
                None => "null".to_string(),
            }
        }
        fn fmt_bool(v: &Option<bool>) -> String {
            match v {
                Some(x) => x.to_string(),
                None => "null".to_string(),
            }
        }
        format!(
            "FINGERPRINT pool={} mint={} block0={} flip={} cu_p90_1s={} cu_p90_10s={} slope={} prebal_cv={} inner_avg={} cpi_avg={} sell_buy={} cu_cluster={} static_fee={} fixed_buy={} fixed_buy_1e4={} flipper={} jito_tip={} early_slot_dom={} early_top3_buy_3s={} whale_rev_top3={} whale_rev_top1={} dev_latency_ms={} dev_3s={} dev_5s={}",
            pool,
            mint,
            fmt_f64(&self.block0_sniped_supply_pct),
            fmt_f64(&self.flip_ratio_10s),
            fmt_f64(&self.cu_price_p90_1s),
            fmt_f64(&self.cu_price_p90_10s),
            fmt_f64(&self.priority_fee_surge_slope),
            fmt_f64(&self.buyer_pre_balance_cv),
            fmt_f64(&self.avg_inner_ix_count_50tx),
            fmt_f64(&self.avg_cpi_depth_50tx),
            fmt_f64(&self.sell_buy_ratio),
            fmt_f64(&self.compute_unit_cluster_dominance),
            fmt_f64(&self.static_fee_profile_ratio),
            fmt_f64(&self.fixed_size_buy_ratio),
            fmt_f64(&self.fixed_size_buy_ratio_1e4),
            fmt_f64(&self.flipper_presence_ratio),
            fmt_f64(&self.jito_tip_intensity),
            fmt_f64(&self.early_slot_volume_dominance_buy),
            fmt_f64(&self.early_top3_buy_volume_pct_3s),
            fmt_f64(&self.whale_reversal_ratio_top3),
            fmt_f64(&self.whale_reversal_ratio_top1),
            fmt_u64(&self.dev_paperhand_latency_ms),
            fmt_bool(&self.dev_sold_within_3s),
            fmt_bool(&self.dev_sold_within_5s),
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Lightweight input event (adapter layer)
// ═══════════════════════════════════════════════════════════════════════════

/// A single transaction event consumed by the aggregator.
///
/// Adapters (gRPC / PumpPortal / replay) build this from whatever raw format
/// they have; the aggregator only sees `FingerprintTxEvent`.
#[derive(Debug, Clone)]
pub struct FingerprintTxEvent {
    /// Slot in which this tx was included.
    pub slot: u64,
    /// Transaction index within the slot (for ordering).
    pub tx_index: u32,
    /// Unique tx signature (base-58).
    pub signature: String,
    /// Wall-clock / event timestamp in milliseconds since UNIX epoch.
    pub timestamp_ms: u64,
    /// Whether the parsed trade is a BUY.
    pub is_buy: bool,
    /// Canonical SOL amount for the trade in SOL units when available.
    pub sol_amount_sol: Option<f64>,
    /// Token balance changes per owner pubkey: positive = bought, negative = sold.
    /// Values are in raw token units (before decimal normalization).
    pub token_deltas: Vec<TokenDelta>,
    /// Owner-resolved deltas derived strictly from pre/post token balances and ATA ownership.
    pub resolved_owner_deltas: Vec<TokenDelta>,
    /// SOL pre-balances for each account involved, keyed by owner pubkey.
    pub sol_pre_balances: HashMap<String, u64>,
    /// Compute-unit price in micro-lamports / CU.
    pub cu_price_micro_lamports: Option<u64>,
    /// Requested CU limit from ComputeBudget.
    pub compute_unit_limit: Option<u32>,
    /// Consumed compute units from tx meta.
    pub compute_units_consumed: Option<u64>,
    /// Total number of inner instructions across all groups.
    pub inner_ix_count: Option<u32>,
    /// CPI depth proxy (number of inner-instruction groups, or 1+invocations).
    pub cpi_depth: Option<u32>,
    /// Number of ATA-creation inner instructions (for boilerplate subtraction).
    pub ata_create_count: Option<u32>,
    /// Deterministic Jito tip detection: Some(true/false) when known, None when unavailable.
    pub jito_tip_detected: Option<bool>,
}

/// A single token balance delta for one owner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenDelta {
    /// Owner pubkey (base-58).
    pub owner: String,
    /// Signed token delta in raw units (positive = buy, negative = sell).
    pub delta_raw: i128,
    /// Token decimals.
    pub decimals: u8,
}

// ═══════════════════════════════════════════════════════════════════════════
// Per-wallet flow tracker (Metric 2)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Default)]
struct WalletFlow {
    bought_tokens: u128,
    sold_tokens: u128,
    first_buy_slot: u64,
    last_sell_slot: u64,
}

#[derive(Debug, Clone, Default)]
struct OwnerFlow {
    bought_tokens: u128,
    sold_tokens: u128,
    has_buy: bool,
    has_sell: bool,
}

// ═══════════════════════════════════════════════════════════════════════════
// Aggregator
// ═══════════════════════════════════════════════════════════════════════════

/// In-memory aggregator keyed by mint. Created on "create" event, fed with
/// each subsequent tx, frozen when the window closes or gatekeeper decides.
#[derive(Debug)]
pub struct FingerprintAggregator {
    config: EarlyFingerprintConfig,
    /// Slot of the creation tx.
    creation_slot: u64,
    /// True when the creation slot is known; false when we used a sentinel.
    creation_slot_known: bool,
    /// Timestamp (ms) of the creation tx (t0).
    t0_ms: u64,
    /// Total token supply (raw units, before decimal normalization).
    supply_raw: Option<u128>,
    /// Token decimals.
    _decimals: u8,
    /// Deterministically identified developer wallet for the pool, if known.
    dev_pubkey: Option<String>,

    // Metric 1 — block-0 sniper
    tokens_bought_in_creation_slot: u128,

    // Metric 2 — flip tracking
    wallets: HashMap<String, WalletFlow>,
    wallets_degraded: bool,
    owner_flows: HashMap<String, OwnerFlow>,
    owner_flows_degraded: bool,

    // Metric 3 — CU price samples
    cu_prices_1s: Vec<u64>,
    cu_prices_10s: Vec<u64>,
    compute_units_consumed: Vec<u64>,

    // Metric 4 — buyer pre-balance (lamports)
    buyer_pre_balances: HashMap<String, u64>,

    // Metric 5 — inner ix complexity
    inner_ix_counts: Vec<u32>,
    cpi_depths: Vec<u32>,

    // Hybrid fingerprint telemetry
    buy_count: usize,
    static_fee_profiles: HashMap<(u32, u64), usize>,
    static_fee_valid_buy_count: usize,
    fixed_buy_buckets_1e3: HashMap<u64, usize>,
    fixed_buy_buckets_1e4: HashMap<u64, usize>,
    total_buy_volume_sol: f64,
    buy_volume_by_slot: HashMap<u64, f64>,
    early_buy_volume_total_sol_3s: f64,
    early_buy_volume_by_owner_3s: HashMap<String, f64>,
    early_buy_volume_by_owner_3s_degraded: bool,
    known_jito_tip_count: usize,
    jito_tip_count: usize,
    first_dev_sell_ms: Option<u64>,

    /// Total events ingested.
    tx_count: usize,
}

impl FingerprintAggregator {
    /// Create a new aggregator anchored at the creation event.
    ///
    /// `creation_slot_known` must be `true` only when `creation_slot` is a
    /// real on-chain slot. Pass `false` (and a sentinel value for
    /// `creation_slot`) when the slot is unavailable, so that `finalize()`
    /// emits `CREATION_SLOT_UNKNOWN` instead of `SUPPLY_UNKNOWN`.
    pub fn new(
        config: EarlyFingerprintConfig,
        creation_slot: u64,
        creation_slot_known: bool,
        t0_ms: u64,
        supply_raw: Option<u128>,
        decimals: u8,
        dev_pubkey: Option<String>,
    ) -> Self {
        Self {
            config,
            creation_slot,
            creation_slot_known,
            t0_ms,
            supply_raw,
            _decimals: decimals,
            dev_pubkey,
            tokens_bought_in_creation_slot: 0,
            wallets: HashMap::new(),
            wallets_degraded: false,
            owner_flows: HashMap::new(),
            owner_flows_degraded: false,
            cu_prices_1s: Vec::new(),
            cu_prices_10s: Vec::new(),
            compute_units_consumed: Vec::new(),
            buyer_pre_balances: HashMap::new(),
            inner_ix_counts: Vec::new(),
            cpi_depths: Vec::new(),
            buy_count: 0,
            static_fee_profiles: HashMap::new(),
            static_fee_valid_buy_count: 0,
            fixed_buy_buckets_1e3: HashMap::new(),
            fixed_buy_buckets_1e4: HashMap::new(),
            total_buy_volume_sol: 0.0,
            buy_volume_by_slot: HashMap::new(),
            early_buy_volume_total_sol_3s: 0.0,
            early_buy_volume_by_owner_3s: HashMap::new(),
            early_buy_volume_by_owner_3s_degraded: false,
            known_jito_tip_count: 0,
            jito_tip_count: 0,
            first_dev_sell_ms: None,
            tx_count: 0,
        }
    }

    /// Returns `true` if the event is within the observation window.
    pub fn in_window(&self, event: &FingerprintTxEvent) -> bool {
        let window_end_ms = self.t0_ms + self.config.window_secs * 1000;
        event.timestamp_ms <= window_end_ms
    }

    /// Ingest one transaction event. Caller must check `in_window` first.
    pub fn ingest(&mut self, event: &FingerprintTxEvent) {
        self.tx_count += 1;
        let elapsed_ms = event.timestamp_ms.saturating_sub(self.t0_ms);
        if event.is_buy {
            self.buy_count += 1;
        }

        // ── Metric 1: block-0 sniper ─────────────────────────────────
        if event.slot == self.creation_slot {
            // Dust threshold: ignore buys below dust_supply_pct of total supply.
            // Use integer arithmetic (ppm = parts per million) to avoid f64 precision loss.
            let dust_ppm = (self.config.dust_supply_pct * 1_000_000.0) as u128;
            let dust_threshold: u128 = self
                .supply_raw
                .map(|s| s * dust_ppm / 1_000_000)
                .unwrap_or(0);
            for td in &event.token_deltas {
                if td.delta_raw > 0 {
                    let amount = td.delta_raw as u128;
                    if amount > dust_threshold {
                        self.tokens_bought_in_creation_slot += amount;
                    }
                }
            }
        }

        // ── Metric 2: flip tracking ──────────────────────────────────
        for td in &event.token_deltas {
            if self.wallets.len() >= self.config.max_wallets
                && !self.wallets.contains_key(&td.owner)
            {
                self.wallets_degraded = true;
                continue;
            }
            let flow = self.wallets.entry(td.owner.clone()).or_default();
            if td.delta_raw > 0 {
                flow.bought_tokens += td.delta_raw as u128;
                if flow.first_buy_slot == 0 {
                    flow.first_buy_slot = event.slot;
                }
            } else if td.delta_raw < 0 {
                flow.sold_tokens += td.delta_raw.unsigned_abs();
                flow.last_sell_slot = event.slot;
            }
        }

        // ── Owner-resolved tracking (whales / flippers / dev) ───────
        for td in &event.resolved_owner_deltas {
            if self.owner_flows.len() >= self.config.max_wallets
                && !self.owner_flows.contains_key(&td.owner)
            {
                self.owner_flows_degraded = true;
                continue;
            }
            let flow = self.owner_flows.entry(td.owner.clone()).or_default();
            if td.delta_raw > 0 {
                flow.bought_tokens += td.delta_raw as u128;
                flow.has_buy = true;
            } else if td.delta_raw < 0 {
                let sold = td.delta_raw.unsigned_abs();
                flow.sold_tokens += sold;
                flow.has_sell = true;
                if self.dev_pubkey.as_deref() == Some(td.owner.as_str())
                    && sold >= self.config.dev_sell_dust_token_amount_raw as u128
                    && self.first_dev_sell_ms.is_none()
                {
                    self.first_dev_sell_ms = Some(event.timestamp_ms);
                }
            }
        }

        // ── Metric 3: CU price ───────────────────────────────────────
        if let Some(cu_price) = event.cu_price_micro_lamports {
            self.cu_prices_10s.push(cu_price);
            if elapsed_ms < 1000 {
                self.cu_prices_1s.push(cu_price);
            }
        }
        if let Some(consumed) = event.compute_units_consumed {
            self.compute_units_consumed.push(consumed);
        }

        // ── Metric 4: buyer pre-balance (first buy only) ─────────────
        let cv_end_ms = self.config.cv_window_secs * 1000;
        if elapsed_ms <= cv_end_ms {
            for td in &event.token_deltas {
                if td.delta_raw > 0 && !self.buyer_pre_balances.contains_key(&td.owner) {
                    if let Some(&pre_bal) = event.sol_pre_balances.get(&td.owner) {
                        self.buyer_pre_balances.insert(td.owner.clone(), pre_bal);
                    }
                }
            }
        }

        // ── Metric 5: inner ix complexity ────────────────────────────
        if self.inner_ix_counts.len() < self.config.max_tx_for_cpi {
            if let Some(mut ix_count) = event.inner_ix_count {
                if self.config.subtract_ata_boilerplate {
                    ix_count = ix_count.saturating_sub(event.ata_create_count.unwrap_or(0));
                }
                self.inner_ix_counts.push(ix_count);
            }
            if let Some(mut depth) = event.cpi_depth {
                if self.config.subtract_ata_boilerplate {
                    depth = depth.saturating_sub(event.ata_create_count.unwrap_or(0));
                }
                self.cpi_depths.push(depth);
            }
        }

        // ── Hybrid metric inputs ─────────────────────────────────────
        if event.is_buy {
            if elapsed_ms <= self.config.static_fee_window_secs * 1000 {
                if let (Some(limit), Some(price)) =
                    (event.compute_unit_limit, event.cu_price_micro_lamports)
                {
                    *self.static_fee_profiles.entry((limit, price)).or_insert(0) += 1;
                    self.static_fee_valid_buy_count += 1;
                }
            }

            if let Some(sol_amount_sol) = event.sol_amount_sol {
                let bucket_1e3 = bucket_sol(sol_amount_sol, 1_000.0);
                let bucket_1e4 = bucket_sol(sol_amount_sol, 10_000.0);
                *self.fixed_buy_buckets_1e3.entry(bucket_1e3).or_insert(0) += 1;
                *self.fixed_buy_buckets_1e4.entry(bucket_1e4).or_insert(0) += 1;
                self.total_buy_volume_sol += sol_amount_sol;
                *self.buy_volume_by_slot.entry(event.slot).or_insert(0.0) += sol_amount_sol;
                if elapsed_ms <= EARLY_TOP3_BUY_WINDOW_MS {
                    let owner_weights = positive_buy_owner_weights(event);
                    if owner_weights.is_empty() {
                        self.early_buy_volume_by_owner_3s_degraded = true;
                    } else {
                        self.early_buy_volume_total_sol_3s += sol_amount_sol;
                        for (owner, weight) in owner_weights {
                            if self.early_buy_volume_by_owner_3s.len() >= self.config.max_wallets
                                && !self.early_buy_volume_by_owner_3s.contains_key(&owner)
                            {
                                self.early_buy_volume_by_owner_3s_degraded = true;
                                continue;
                            }
                            *self
                                .early_buy_volume_by_owner_3s
                                .entry(owner)
                                .or_insert(0.0) += sol_amount_sol * weight;
                        }
                    }
                }
            }
        }

        if let Some(jito_tip) = event.jito_tip_detected {
            self.known_jito_tip_count += 1;
            if jito_tip {
                self.jito_tip_count += 1;
            }
        }
    }

    /// Freeze the aggregator and produce final metrics.
    pub fn finalize(&self) -> EarlyFingerprintMetrics {
        let mut degraded = false;
        let mut reasons: Vec<&'static str> = Vec::new();

        // ── Metric 1 ────────────────────────────────────────────────
        let block0 = if !self.creation_slot_known {
            // Slot was unknown at aggregator creation — we cannot compute a
            // meaningful block-0 metric; report precise reason.
            degraded = true;
            push_reason(&mut reasons, "CREATION_SLOT_UNKNOWN");
            None
        } else {
            match self.supply_raw {
                Some(supply) if supply > 0 => {
                    let pct = self.tokens_bought_in_creation_slot as f64 / supply as f64;
                    Some(pct)
                }
                _ => {
                    degraded = true;
                    push_reason(&mut reasons, "SUPPLY_UNKNOWN");
                    None
                }
            }
        };

        // ── Metric 2 ────────────────────────────────────────────────
        let (flip_ratio, unique_buyers) = self.compute_flip_ratio();
        if self.wallets_degraded {
            degraded = true;
            push_reason(&mut reasons, "WALLET_CAP_REACHED");
        }
        let flip = if unique_buyers > 0 {
            Some(flip_ratio)
        } else {
            degraded = true;
            push_reason(&mut reasons, "FLIP_RATIO_NO_BUYERS");
            None
        };

        // ── Metric 3 ────────────────────────────────────────────────
        let p90_1s = percentile_90(&self.cu_prices_1s);
        let p90_10s = percentile_90(&self.cu_prices_10s);
        let slope = match (p90_1s, p90_10s) {
            (Some(a), Some(b)) if self.config.window_secs > 1 => {
                Some((b - a) / (self.config.window_secs as f64 - 1.0))
            }
            _ => {
                degraded = true;
                push_reason(&mut reasons, "PRIORITY_FEE_SLOPE_UNAVAILABLE");
                None
            }
        };
        if p90_1s.is_none() {
            degraded = true;
            push_reason(&mut reasons, "CU_PRICE_P90_1S_UNAVAILABLE");
        }
        if p90_10s.is_none() {
            degraded = true;
            push_reason(&mut reasons, "CU_PRICE_P90_10S_UNAVAILABLE");
        }

        // ── Metric 4 ────────────────────────────────────────────────
        let cv = self.compute_buyer_cv();
        if cv.is_none() {
            degraded = true;
            push_reason(&mut reasons, "BUYER_PRE_BALANCE_CV_UNAVAILABLE");
        }

        // ── Metric 5 ────────────────────────────────────────────────
        let avg_inner = mean_u32(&self.inner_ix_counts);
        let avg_cpi = mean_u32(&self.cpi_depths);
        if avg_inner.is_none() {
            degraded = true;
            push_reason(&mut reasons, "INNER_IX_AVG_UNAVAILABLE");
        }
        if avg_cpi.is_none() {
            degraded = true;
            push_reason(&mut reasons, "CPI_DEPTH_AVG_UNAVAILABLE");
        }

        // ── Hybrid metrics ──────────────────────────────────────────
        let sell_buy_ratio = if self.buy_count == 0 {
            degraded = true;
            push_reason(&mut reasons, "BUY_COUNT_ZERO");
            None
        } else {
            Some((self.tx_count.saturating_sub(self.buy_count)) as f64 / self.buy_count as f64)
        };

        let compute_unit_cluster_dominance =
            if self.compute_units_consumed.len() < self.config.min_tx_for_cu_cluster {
                degraded = true;
                push_reason(&mut reasons, "CU_CLUSTER_MIN_TX");
                None
            } else {
                compute_unit_cluster_dominance(
                    &self.compute_units_consumed,
                    self.config.cu_cluster_tolerance_pct,
                )
            };

        let static_fee_profile_ratio =
            if self.static_fee_valid_buy_count < self.config.min_buy_txs_for_static_fee {
                degraded = true;
                push_reason(&mut reasons, "STATIC_FEE_MIN_BUYS");
                None
            } else {
                ratio_from_mode_counts(&self.static_fee_profiles, self.static_fee_valid_buy_count)
            };

        let (fixed_size_buy_ratio, fixed_size_buy_ratio_1e4) =
            if self.buy_count < self.config.min_buy_txs_for_fixed_size {
                degraded = true;
                push_reason(&mut reasons, "FIXED_SIZE_MIN_BUYS");
                (None, None)
            } else {
                (
                    ratio_from_mode_counts(&self.fixed_buy_buckets_1e3, self.buy_count),
                    ratio_from_mode_counts(&self.fixed_buy_buckets_1e4, self.buy_count),
                )
            };

        let flipper_presence_ratio = match self.compute_flipper_presence_ratio() {
            Ok(value) => Some(value),
            Err(reason) => {
                degraded = true;
                push_reason(&mut reasons, reason);
                None
            }
        };

        let jito_tip_intensity = if self.known_jito_tip_count < self.config.min_tx_for_jito_tip {
            degraded = true;
            push_reason(&mut reasons, "JITO_TIP_MIN_TX");
            None
        } else {
            Some(self.jito_tip_count as f64 / self.known_jito_tip_count as f64)
        };

        let early_slot_volume_dominance_buy = if !self.creation_slot_known {
            degraded = true;
            push_reason(&mut reasons, "CREATION_SLOT_UNKNOWN");
            None
        } else if self.total_buy_volume_sol <= f64::EPSILON {
            degraded = true;
            push_reason(&mut reasons, "EARLY_SLOT_BUY_VOLUME_ZERO");
            None
        } else {
            let end_slot = self
                .creation_slot
                .saturating_add(self.config.early_slot_count.saturating_sub(1));
            let volume = self
                .buy_volume_by_slot
                .iter()
                .filter(|(slot, _)| **slot >= self.creation_slot && **slot <= end_slot)
                .map(|(_, volume)| *volume)
                .sum::<f64>();
            Some(volume / self.total_buy_volume_sol)
        };

        let early_top3_buy_volume_pct_3s = match self.compute_early_top3_buy_volume_pct_3s() {
            Ok(value) => Some(value),
            Err(reason) => {
                degraded = true;
                push_reason(&mut reasons, reason);
                None
            }
        };

        let (whale_reversal_ratio_top3, whale_reversal_ratio_top1) =
            match self.compute_whale_reversal_ratios() {
                Ok(values) => values,
                Err(reason) => {
                    degraded = true;
                    push_reason(&mut reasons, reason);
                    (None, None)
                }
            };

        if self.owner_flows_degraded {
            degraded = true;
            push_reason(&mut reasons, "OWNER_WALLET_CAP_REACHED");
        }

        let (dev_paperhand_latency_ms, dev_sold_within_3s, dev_sold_within_5s) =
            match self.compute_dev_paperhand() {
                Ok(values) => values,
                Err(reason) => {
                    degraded = true;
                    push_reason(&mut reasons, reason);
                    (None, None, None)
                }
            };

        if degraded && reasons.is_empty() {
            push_reason(&mut reasons, "UNKNOWN");
        }

        EarlyFingerprintMetrics {
            block0_sniped_supply_pct: block0,
            flip_ratio_10s: flip,
            cu_price_p90_1s: p90_1s,
            cu_price_p90_10s: p90_10s,
            priority_fee_surge_slope: slope,
            buyer_pre_balance_cv: cv,
            avg_inner_ix_count_50tx: avg_inner,
            avg_cpi_depth_50tx: avg_cpi,
            sell_buy_ratio,
            compute_unit_cluster_dominance,
            static_fee_profile_ratio,
            fixed_size_buy_ratio,
            fixed_size_buy_ratio_1e4,
            flipper_presence_ratio,
            jito_tip_intensity,
            early_slot_volume_dominance_buy,
            early_top3_buy_volume_pct_3s,
            whale_reversal_ratio_top3,
            whale_reversal_ratio_top1,
            dev_paperhand_latency_ms,
            dev_sold_within_3s,
            dev_sold_within_5s,
            fingerprint_degraded: degraded,
            fingerprint_reason: if reasons.is_empty() {
                None
            } else {
                Some(reasons.join(","))
            },
        }
    }

    // ── helpers ──────────────────────────────────────────────────────

    fn compute_flip_ratio(&self) -> (f64, usize) {
        let mut buyers = 0usize;
        let mut flips = 0usize;
        for flow in self.wallets.values() {
            if flow.bought_tokens == 0 {
                continue;
            }
            buyers += 1;
            let threshold = (flow.bought_tokens as f64 * self.config.flip_dump_pct) as u128;
            if flow.sold_tokens >= threshold {
                let slot_gap = flow.last_sell_slot.saturating_sub(flow.first_buy_slot);
                if slot_gap <= self.config.max_flip_slots {
                    flips += 1;
                }
            }
        }
        if buyers == 0 {
            return (0.0, 0);
        }
        (flips as f64 / buyers as f64, buyers)
    }

    fn compute_buyer_cv(&self) -> Option<f64> {
        if self.buyer_pre_balances.len() < self.config.min_buyers_for_cv {
            return None;
        }
        let vals: Vec<f64> = self
            .buyer_pre_balances
            .values()
            .map(|&lam| lam as f64 / 1_000_000_000.0) // lamports → SOL
            .collect();
        let n = vals.len() as f64;
        let mean = vals.iter().sum::<f64>() / n;
        if mean < 1e-12 {
            return None; // near-zero mean → undefined CV
        }
        let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let stddev = variance.sqrt();
        Some(stddev / mean)
    }

    fn compute_flipper_presence_ratio(&self) -> Result<f64, &'static str> {
        let unique_buyers = self
            .owner_flows
            .values()
            .filter(|flow| flow.has_buy)
            .count();
        if unique_buyers == 0 {
            return Err("OWNER_DELTAS_UNAVAILABLE");
        }
        if unique_buyers < self.config.min_wallets_for_flipper {
            return Err("FLIPPER_MIN_WALLETS");
        }
        let flippers = self
            .owner_flows
            .values()
            .filter(|flow| flow.has_buy && flow.has_sell)
            .count();
        Ok(flippers as f64 / unique_buyers as f64)
    }

    fn compute_whale_reversal_ratios(&self) -> Result<(Option<f64>, Option<f64>), &'static str> {
        let mut ranked: Vec<&OwnerFlow> = self
            .owner_flows
            .values()
            .filter(|flow| flow.bought_tokens > 0)
            .collect();
        if ranked.is_empty() {
            return Err("WHALE_OWNER_DELTAS_MISSING");
        }
        ranked.sort_by(|a, b| b.bought_tokens.cmp(&a.bought_tokens));

        let top1 = ranked
            .first()
            .and_then(|flow| ratio_u128(flow.sold_tokens, flow.bought_tokens));
        let top3_bought: u128 = ranked.iter().take(3).map(|flow| flow.bought_tokens).sum();
        let top3_sold: u128 = ranked.iter().take(3).map(|flow| flow.sold_tokens).sum();
        Ok((ratio_u128(top3_sold, top3_bought), top1))
    }

    fn compute_early_top3_buy_volume_pct_3s(&self) -> Result<f64, &'static str> {
        if self.early_buy_volume_by_owner_3s_degraded {
            return Err("EARLY_TOP3_BUY_VOLUME_OWNER_UNAVAILABLE");
        }
        if self.early_buy_volume_total_sol_3s <= f64::EPSILON
            || self.early_buy_volume_by_owner_3s.is_empty()
        {
            return Err("EARLY_TOP3_BUY_VOLUME_ZERO");
        }
        let mut ranked: Vec<f64> = self
            .early_buy_volume_by_owner_3s
            .values()
            .copied()
            .collect();
        ranked.sort_by(|a, b| b.total_cmp(a));
        let top3 = ranked.into_iter().take(3).sum::<f64>();
        Ok(top3 / self.early_buy_volume_total_sol_3s)
    }

    fn compute_dev_paperhand(
        &self,
    ) -> Result<(Option<u64>, Option<bool>, Option<bool>), &'static str> {
        if self.dev_pubkey.is_none() {
            return Err("DEV_WALLET_UNKNOWN");
        }
        if self.t0_ms == 0 {
            return Err("POOL_BIRTH_MS_UNKNOWN");
        }
        match self.first_dev_sell_ms {
            Some(first_sell_ms) => {
                let latency = first_sell_ms.saturating_sub(self.t0_ms);
                Ok((Some(latency), Some(latency < 3_000), Some(latency < 5_000)))
            }
            None => Ok((None, Some(false), Some(false))),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Free-standing helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Compute the 90th percentile of a `u64` slice (returns micro-lamports as f64).
fn percentile_90(samples: &[u64]) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64) * 0.9).ceil() as usize;
    let idx = idx.saturating_sub(1).min(sorted.len() - 1);
    Some(sorted[idx] as f64)
}

/// Arithmetic mean of a `u32` slice, returned as `Option<f64>`.
fn mean_u32(vals: &[u32]) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    let sum: u64 = vals.iter().map(|&v| v as u64).sum();
    Some(sum as f64 / vals.len() as f64)
}

fn push_reason(reasons: &mut Vec<&'static str>, reason: &'static str) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

fn positive_buy_owner_weights(event: &FingerprintTxEvent) -> Vec<(String, f64)> {
    let resolved = aggregate_positive_owner_deltas(&event.resolved_owner_deltas);
    let owners = if resolved.is_empty() {
        aggregate_positive_owner_deltas(&event.token_deltas)
    } else {
        resolved
    };
    let total_raw: u128 = owners.values().copied().sum();
    if total_raw == 0 {
        return Vec::new();
    }
    owners
        .into_iter()
        .map(|(owner, raw)| (owner, raw as f64 / total_raw as f64))
        .collect()
}

fn aggregate_positive_owner_deltas(deltas: &[TokenDelta]) -> HashMap<String, u128> {
    let mut owners = HashMap::new();
    for delta in deltas {
        if delta.delta_raw > 0 {
            *owners.entry(delta.owner.clone()).or_insert(0) += delta.delta_raw as u128;
        }
    }
    owners
}

fn bucket_sol(sol_amount: f64, scale: f64) -> u64 {
    (sol_amount * scale).round().max(0.0) as u64
}

fn ratio_u128(numerator: u128, denominator: u128) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}

fn ratio_from_mode_counts<K>(counts: &HashMap<K, usize>, denominator: usize) -> Option<f64>
where
    K: Eq + std::hash::Hash,
{
    if denominator == 0 {
        return None;
    }
    counts
        .values()
        .max()
        .map(|count| *count as f64 / denominator as f64)
}

fn compute_unit_cluster_dominance(samples: &[u64], tolerance_pct: f64) -> Option<f64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let mut max_cluster = 0usize;
    for &anchor in &sorted {
        let low = ((anchor as f64) * (1.0 - tolerance_pct)).floor().max(0.0) as u64;
        let high = ((anchor as f64) * (1.0 + tolerance_pct)).ceil() as u64;
        let start = sorted.partition_point(|value| *value < low);
        let end = sorted.partition_point(|value| *value <= high);
        max_cluster = max_cluster.max(end.saturating_sub(start));
    }
    Some(max_cluster as f64 / sorted.len() as f64)
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> EarlyFingerprintConfig {
        EarlyFingerprintConfig::default()
    }

    fn make_event(
        slot: u64,
        tx_index: u32,
        timestamp_ms: u64,
        token_deltas: Vec<TokenDelta>,
        sol_pre_balances: HashMap<String, u64>,
        cu_price: u64,
        inner_ix: u32,
        cpi_depth: u32,
    ) -> FingerprintTxEvent {
        FingerprintTxEvent {
            slot,
            tx_index,
            signature: format!("sig_{}_{}", slot, tx_index),
            timestamp_ms,
            is_buy: token_deltas.iter().any(|delta| delta.delta_raw > 0),
            sol_amount_sol: None,
            resolved_owner_deltas: token_deltas.clone(),
            token_deltas,
            sol_pre_balances,
            cu_price_micro_lamports: Some(cu_price),
            compute_unit_limit: None,
            compute_units_consumed: None,
            inner_ix_count: Some(inner_ix),
            cpi_depth: Some(cpi_depth),
            ata_create_count: Some(0),
            jito_tip_detected: None,
        }
    }

    // ─── Metric 1: block0_sniped_supply_pct ──────────────────────────

    #[test]
    fn test_block0_sniped_supply() {
        let config = default_config();
        let creation_slot = 100;
        let t0 = 1000;
        let supply = 1_000_000u128; // 1M tokens raw
        let mut agg =
            FingerprintAggregator::new(config, creation_slot, true, t0, Some(supply), 6, None);

        // Buy 200_000 tokens in creation slot
        let evt = make_event(
            100,
            1,
            t0 + 100,
            vec![TokenDelta {
                owner: "walletA".into(),
                delta_raw: 200_000,
                decimals: 6,
            }],
            HashMap::new(),
            0,
            0,
            0,
        );
        agg.ingest(&evt);

        let m = agg.finalize();
        let pct = m.block0_sniped_supply_pct.unwrap();
        assert!((pct - 0.2).abs() < 1e-9, "expected 20% snipe, got {}", pct);
    }

    #[test]
    fn test_block0_supply_unknown() {
        let config = default_config();
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, None, 6, None);
        let evt = make_event(
            100,
            1,
            1100,
            vec![TokenDelta {
                owner: "w".into(),
                delta_raw: 500,
                decimals: 6,
            }],
            HashMap::new(),
            0,
            0,
            0,
        );
        agg.ingest(&evt);
        let m = agg.finalize();
        assert!(m.block0_sniped_supply_pct.is_none());
        assert!(m.fingerprint_degraded);
        assert!(m
            .fingerprint_reason
            .as_deref()
            .unwrap()
            .contains("SUPPLY_UNKNOWN"));
    }

    #[test]
    fn test_block0_creation_slot_unknown() {
        // When creation_slot_known=false the reason must be CREATION_SLOT_UNKNOWN,
        // NOT SUPPLY_UNKNOWN, even if supply_raw is also None.
        let config = default_config();
        let mut agg = FingerprintAggregator::new(config, u64::MAX, false, 1000, None, 6, None);
        let evt = make_event(
            0, // slot 0 would normally trigger block-0, but slot is unknown
            1,
            1100,
            vec![TokenDelta {
                owner: "w".into(),
                delta_raw: 500,
                decimals: 6,
            }],
            HashMap::new(),
            0,
            0,
            0,
        );
        agg.ingest(&evt);
        let m = agg.finalize();
        assert!(m.block0_sniped_supply_pct.is_none());
        assert!(m.fingerprint_degraded);
        let reason = m.fingerprint_reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("CREATION_SLOT_UNKNOWN"),
            "expected CREATION_SLOT_UNKNOWN, got: {}",
            reason
        );
        assert!(
            !reason.contains("SUPPLY_UNKNOWN"),
            "should not contain SUPPLY_UNKNOWN when slot is the root cause"
        );
    }

    // ─── Metric 2: flip_ratio_10s ────────────────────────────────────

    #[test]
    fn test_flip_ratio_basic() {
        let config = default_config();
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        // 10 buyers
        for i in 0..10 {
            let evt = make_event(
                101 + i,
                0,
                1100 + i * 100,
                vec![TokenDelta {
                    owner: format!("buyer_{}", i),
                    delta_raw: 1000,
                    decimals: 6,
                }],
                HashMap::new(),
                0,
                0,
                0,
            );
            agg.ingest(&evt);
        }

        // 2 of them sell ≥50% within max_flip_slots
        for i in 0..2 {
            let evt = make_event(
                101 + i + 5, // within 20 slot gap
                1,
                1200 + i * 100,
                vec![TokenDelta {
                    owner: format!("buyer_{}", i),
                    delta_raw: -600, // 60% dump
                    decimals: 6,
                }],
                HashMap::new(),
                0,
                0,
                0,
            );
            agg.ingest(&evt);
        }

        let m = agg.finalize();
        let flip = m.flip_ratio_10s.unwrap();
        assert!(
            (flip - 0.2).abs() < 1e-9,
            "expected 20% flip ratio, got {}",
            flip
        );
    }

    #[test]
    fn test_flip_ratio_slot_gap_too_large() {
        let mut config = default_config();
        config.max_flip_slots = 5;
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        // Buyer buys at slot 101
        agg.ingest(&make_event(
            101,
            0,
            1100,
            vec![TokenDelta {
                owner: "b1".into(),
                delta_raw: 1000,
                decimals: 6,
            }],
            HashMap::new(),
            0,
            0,
            0,
        ));

        // Sells at slot 120 (gap=19, exceeds max_flip_slots=5)
        agg.ingest(&make_event(
            120,
            0,
            5000,
            vec![TokenDelta {
                owner: "b1".into(),
                delta_raw: -600,
                decimals: 6,
            }],
            HashMap::new(),
            0,
            0,
            0,
        ));

        let m = agg.finalize();
        let flip = m.flip_ratio_10s.unwrap();
        assert!(
            flip < 1e-9,
            "expected 0 flip ratio (gap too large), got {}",
            flip
        );
    }

    // ─── Metric 3: CU P90 + slope ───────────────────────────────────

    #[test]
    fn test_cu_p90_and_slope() {
        let config = default_config();
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        // First-second txs (t0..t0+1s): CU prices 100..109
        for i in 0..10 {
            agg.ingest(&make_event(
                100,
                i as u32,
                1000 + i * 50,
                vec![],
                HashMap::new(),
                100 + i,
                0,
                0,
            ));
        }

        // Later txs (t0+9s..t0+10s): CU prices 200..209
        for i in 0..10 {
            agg.ingest(&make_event(
                110,
                i as u32,
                10000 + i * 50,
                vec![],
                HashMap::new(),
                200 + i,
                0,
                0,
            ));
        }

        let m = agg.finalize();
        let p90_1s = m.cu_price_p90_1s.unwrap();
        let p90_10s = m.cu_price_p90_10s.unwrap();
        // 1s bucket has prices [100..109], P90 = 108
        assert!(
            (p90_1s - 108.0).abs() < 1e-9,
            "expected P90_1s=108, got {}",
            p90_1s
        );
        // 10s bucket has all 20 prices, P90 = 207
        assert!(p90_10s >= 200.0, "expected P90_10s >= 200, got {}", p90_10s);

        let slope = m.priority_fee_surge_slope.unwrap();
        // slope = (p90_10s - p90_1s) / 9
        let expected_slope = (p90_10s - p90_1s) / 9.0;
        assert!(
            (slope - expected_slope).abs() < 1e-6,
            "unexpected slope {}",
            slope
        );
    }

    // ─── Metric 4: buyer_pre_balance_cv ──────────────────────────────

    #[test]
    fn test_prebalance_cv_identical() {
        let mut config = default_config();
        config.min_buyers_for_cv = 3;
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        // 5 buyers all with identical balance (1 SOL = 1_000_000_000 lamports)
        for i in 0..5 {
            let mut bals = HashMap::new();
            let owner = format!("buyer_{}", i);
            bals.insert(owner.clone(), 1_000_000_000u64);
            agg.ingest(&make_event(
                101,
                i as u32,
                1100,
                vec![TokenDelta {
                    owner,
                    delta_raw: 100,
                    decimals: 6,
                }],
                bals,
                0,
                0,
                0,
            ));
        }

        let m = agg.finalize();
        let cv = m.buyer_pre_balance_cv.unwrap();
        assert!(cv < 1e-9, "identical balances should give CV~0, got {}", cv);
    }

    #[test]
    fn test_prebalance_cv_varied() {
        let mut config = default_config();
        config.min_buyers_for_cv = 3;
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        // 4 buyers with varied balances
        let balances = [1_000_000_000u64, 5_000_000_000, 100_000_000, 10_000_000_000];
        for (i, &bal) in balances.iter().enumerate() {
            let mut bals = HashMap::new();
            let owner = format!("buyer_{}", i);
            bals.insert(owner.clone(), bal);
            agg.ingest(&make_event(
                101,
                i as u32,
                1100,
                vec![TokenDelta {
                    owner,
                    delta_raw: 100,
                    decimals: 6,
                }],
                bals,
                0,
                0,
                0,
            ));
        }

        let m = agg.finalize();
        let cv = m.buyer_pre_balance_cv.unwrap();
        assert!(cv > 0.5, "varied balances should give CV > 0.5, got {}", cv);
    }

    // ─── Metric 5: inner instruction complexity ──────────────────────

    #[test]
    fn test_inner_ix_complexity() {
        let config = default_config();
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        // 5 txs: inner_ix = [0, 10, 2, 8, 5], cpi_depth = [1, 3, 1, 2, 2]
        let ix_counts = [0u32, 10, 2, 8, 5];
        let cpi_depths = [1u32, 3, 1, 2, 2];
        for i in 0..5 {
            agg.ingest(&make_event(
                101,
                i as u32,
                1100,
                vec![],
                HashMap::new(),
                0,
                ix_counts[i as usize],
                cpi_depths[i as usize],
            ));
        }

        let m = agg.finalize();
        let avg_ix = m.avg_inner_ix_count_50tx.unwrap();
        assert!(
            (avg_ix - 5.0).abs() < 1e-9,
            "expected avg_ix=5.0, got {}",
            avg_ix
        );
        let avg_cpi = m.avg_cpi_depth_50tx.unwrap();
        assert!(
            (avg_cpi - 1.8).abs() < 1e-9,
            "expected avg_cpi=1.8, got {}",
            avg_cpi
        );
    }

    #[test]
    fn test_sell_buy_ratio_basic_cases() {
        let mut agg =
            FingerprintAggregator::new(default_config(), 100, true, 1000, Some(1_000_000), 6, None);

        for i in 0..2 {
            agg.ingest(&make_event(
                101,
                i,
                1100 + i as u64 * 100,
                vec![TokenDelta {
                    owner: format!("buyer_{}", i),
                    delta_raw: 1_000,
                    decimals: 6,
                }],
                HashMap::new(),
                1,
                0,
                0,
            ));
        }
        agg.ingest(&make_event(
            102,
            2,
            1400,
            vec![TokenDelta {
                owner: "seller".into(),
                delta_raw: -500,
                decimals: 6,
            }],
            HashMap::new(),
            1,
            0,
            0,
        ));

        let metrics = agg.finalize();
        assert_eq!(metrics.sell_buy_ratio, Some(0.5));

        let mut no_buy_agg =
            FingerprintAggregator::new(default_config(), 100, true, 1000, Some(1_000_000), 6, None);
        no_buy_agg.ingest(&make_event(
            101,
            0,
            1100,
            vec![TokenDelta {
                owner: "seller".into(),
                delta_raw: -500,
                decimals: 6,
            }],
            HashMap::new(),
            1,
            0,
            0,
        ));
        let no_buy_metrics = no_buy_agg.finalize();
        assert!(no_buy_metrics.sell_buy_ratio.is_none());
        assert!(no_buy_metrics
            .fingerprint_reason
            .as_deref()
            .unwrap_or("")
            .contains("BUY_COUNT_ZERO"));
    }

    #[test]
    fn test_compute_unit_cluster_dominance_obvious_cluster() {
        let mut config = default_config();
        config.min_tx_for_cu_cluster = 6;
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        for (i, consumed) in [98_000_u64, 100_000, 101_000, 102_000, 210_000, 400_000]
            .into_iter()
            .enumerate()
        {
            let mut event = make_event(
                101 + i as u64,
                i as u32,
                1100 + i as u64 * 100,
                vec![],
                HashMap::new(),
                1,
                0,
                0,
            );
            event.compute_units_consumed = Some(consumed);
            agg.ingest(&event);
        }

        let metrics = agg.finalize();
        let dominance = metrics.compute_unit_cluster_dominance.unwrap();
        assert!(
            (dominance - (4.0 / 6.0)).abs() < 1e-9,
            "expected 4/6 cluster dominance, got {}",
            dominance
        );
    }

    #[test]
    fn test_static_fee_profile_ratio_repeated_exact_pairs() {
        let mut config = default_config();
        config.min_buy_txs_for_static_fee = 5;
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        for i in 0..5 {
            let mut event = make_event(
                100 + i as u64,
                i,
                1100 + i as u64 * 400,
                vec![TokenDelta {
                    owner: format!("buyer_{}", i),
                    delta_raw: 1_000,
                    decimals: 6,
                }],
                HashMap::new(),
                if i == 4 { 7 } else { 5 },
                0,
                0,
            );
            event.compute_unit_limit = Some(if i == 4 { 180_000 } else { 200_000 });
            agg.ingest(&event);
        }

        assert_eq!(agg.finalize().static_fee_profile_ratio, Some(0.8));
    }

    #[test]
    fn test_fixed_size_buy_ratio_bucketed_values() {
        let mut config = default_config();
        config.min_buy_txs_for_fixed_size = 5;
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        let sizes = [1.2344_f64, 1.2345, 1.2344, 0.7500, 0.7501];
        for (i, sol_amount) in sizes.into_iter().enumerate() {
            let mut event = make_event(
                101 + i as u64,
                i as u32,
                1100 + i as u64 * 100,
                vec![TokenDelta {
                    owner: format!("buyer_{}", i),
                    delta_raw: 1_000,
                    decimals: 6,
                }],
                HashMap::new(),
                1,
                0,
                0,
            );
            event.sol_amount_sol = Some(sol_amount);
            agg.ingest(&event);
        }

        let metrics = agg.finalize();
        assert_eq!(metrics.fixed_size_buy_ratio, Some(0.4));
        assert_eq!(metrics.fixed_size_buy_ratio_1e4, Some(0.4));
    }

    #[test]
    fn test_flipper_presence_ratio_tracks_buy_and_sell_wallets() {
        let mut config = default_config();
        config.min_wallets_for_flipper = 5;
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        for i in 0..5 {
            agg.ingest(&make_event(
                101,
                i,
                1100 + i as u64 * 100,
                vec![TokenDelta {
                    owner: format!("wallet_{}", i),
                    delta_raw: 1_000,
                    decimals: 6,
                }],
                HashMap::new(),
                1,
                0,
                0,
            ));
        }
        for i in 0..2 {
            agg.ingest(&make_event(
                102,
                (i + 10) as u32,
                1800 + i as u64 * 100,
                vec![TokenDelta {
                    owner: format!("wallet_{}", i),
                    delta_raw: -500,
                    decimals: 6,
                }],
                HashMap::new(),
                1,
                0,
                0,
            ));
        }

        assert_eq!(agg.finalize().flipper_presence_ratio, Some(0.4));
    }

    #[test]
    fn test_jito_tip_intensity_uses_known_status_only() {
        let mut config = default_config();
        config.min_tx_for_jito_tip = 5;
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        for (i, tip) in [
            Some(true),
            Some(true),
            Some(false),
            Some(true),
            Some(false),
            None,
        ]
        .into_iter()
        .enumerate()
        {
            let mut event = make_event(
                101,
                i as u32,
                1100 + i as u64 * 100,
                vec![],
                HashMap::new(),
                1,
                0,
                0,
            );
            event.jito_tip_detected = tip;
            agg.ingest(&event);
        }

        assert_eq!(agg.finalize().jito_tip_intensity, Some(0.6));
    }

    #[test]
    fn test_early_slot_volume_dominance_buy() {
        let mut config = default_config();
        config.early_slot_count = 3;
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(1_000_000), 6, None);

        let per_slot = [(100, 1.0_f64), (101, 1.5), (102, 0.5), (103, 2.0)];
        for (i, (slot, volume)) in per_slot.into_iter().enumerate() {
            let mut event = make_event(
                slot,
                i as u32,
                1100 + i as u64 * 300,
                vec![TokenDelta {
                    owner: format!("buyer_{}", i),
                    delta_raw: 1_000,
                    decimals: 6,
                }],
                HashMap::new(),
                1,
                0,
                0,
            );
            event.sol_amount_sol = Some(volume);
            agg.ingest(&event);
        }

        assert_eq!(agg.finalize().early_slot_volume_dominance_buy, Some(0.6));
    }

    #[test]
    fn test_early_top3_buy_volume_pct_3s() {
        let mut agg =
            FingerprintAggregator::new(default_config(), 100, true, 1000, Some(1_000_000), 6, None);

        let within_window = [("a", 2.0_f64), ("b", 2.0), ("c", 2.5), ("d", 2.0)];
        for (i, (owner, volume)) in within_window.into_iter().enumerate() {
            let mut event = make_event(
                100,
                i as u32,
                1_100 + i as u64 * 100,
                vec![TokenDelta {
                    owner: owner.to_string(),
                    delta_raw: 1_000,
                    decimals: 6,
                }],
                HashMap::new(),
                1,
                0,
                0,
            );
            event.sol_amount_sol = Some(volume);
            agg.ingest(&event);
        }

        let mut late_event = make_event(
            101,
            99,
            4_500,
            vec![TokenDelta {
                owner: "late".to_string(),
                delta_raw: 1_000,
                decimals: 6,
            }],
            HashMap::new(),
            1,
            0,
            0,
        );
        late_event.sol_amount_sol = Some(10.0);
        agg.ingest(&late_event);

        let metrics = agg.finalize();
        let early_top3 = metrics.early_top3_buy_volume_pct_3s.unwrap();
        assert!((early_top3 - (6.5 / 8.5)).abs() < 1e-9);
    }

    #[test]
    fn test_whale_reversal_ratio_top3() {
        let mut agg =
            FingerprintAggregator::new(default_config(), 100, true, 1000, Some(1_000_000), 6, None);

        for (idx, (owner, bought)) in [("a", 1_000_i128), ("b", 800), ("c", 600), ("d", 100)]
            .into_iter()
            .enumerate()
        {
            agg.ingest(&make_event(
                101,
                idx as u32,
                1100 + idx as u64 * 100,
                vec![TokenDelta {
                    owner: owner.to_string(),
                    delta_raw: bought,
                    decimals: 6,
                }],
                HashMap::new(),
                1,
                0,
                0,
            ));
        }
        for (idx, (owner, sold)) in [("a", -500_i128), ("b", -200), ("c", -300), ("d", -100)]
            .into_iter()
            .enumerate()
        {
            agg.ingest(&make_event(
                102,
                (idx + 10) as u32,
                2000 + idx as u64 * 100,
                vec![TokenDelta {
                    owner: owner.to_string(),
                    delta_raw: sold,
                    decimals: 6,
                }],
                HashMap::new(),
                1,
                0,
                0,
            ));
        }

        let metrics = agg.finalize();
        let top3 = metrics.whale_reversal_ratio_top3.unwrap();
        assert!((top3 - (1000.0 / 2400.0)).abs() < 1e-9);
        assert_eq!(metrics.whale_reversal_ratio_top1, Some(0.5));
    }

    #[test]
    fn test_dev_paperhand_latency_ms() {
        let mut config = default_config();
        config.dev_sell_dust_token_amount_raw = 100;
        let mut agg = FingerprintAggregator::new(
            config,
            100,
            true,
            1_000,
            Some(1_000_000),
            6,
            Some("dev".into()),
        );

        agg.ingest(&make_event(
            100,
            0,
            1_100,
            vec![TokenDelta {
                owner: "dev".into(),
                delta_raw: 1_000,
                decimals: 6,
            }],
            HashMap::new(),
            1,
            0,
            0,
        ));
        agg.ingest(&make_event(
            101,
            1,
            3_500,
            vec![TokenDelta {
                owner: "dev".into(),
                delta_raw: -400,
                decimals: 6,
            }],
            HashMap::new(),
            1,
            0,
            0,
        ));

        let metrics = agg.finalize();
        assert_eq!(metrics.dev_paperhand_latency_ms, Some(2_500));
        assert_eq!(metrics.dev_sold_within_3s, Some(true));
        assert_eq!(metrics.dev_sold_within_5s, Some(true));
    }

    #[test]
    fn test_dev_sell_dust_threshold_ignored() {
        let mut config = default_config();
        config.dev_sell_dust_token_amount_raw = 1_000;
        let mut agg = FingerprintAggregator::new(
            config,
            100,
            true,
            1_000,
            Some(1_000_000),
            6,
            Some("dev".into()),
        );

        agg.ingest(&make_event(
            101,
            0,
            1_200,
            vec![TokenDelta {
                owner: "dev".into(),
                delta_raw: -999,
                decimals: 6,
            }],
            HashMap::new(),
            1,
            0,
            0,
        ));

        let metrics = agg.finalize();
        assert_eq!(metrics.dev_paperhand_latency_ms, None);
        assert_eq!(metrics.dev_sold_within_3s, Some(false));
        assert_eq!(metrics.dev_sold_within_5s, Some(false));
    }

    #[test]
    fn test_block0_dust_threshold_ignored() {
        let mut config = default_config();
        config.dust_supply_pct = 0.01;
        let mut agg = FingerprintAggregator::new(config, 100, true, 1000, Some(100_000), 6, None);

        agg.ingest(&make_event(
            100,
            0,
            1_100,
            vec![TokenDelta {
                owner: "sniper".into(),
                delta_raw: 999,
                decimals: 6,
            }],
            HashMap::new(),
            1,
            0,
            0,
        ));

        assert_eq!(agg.finalize().block0_sniped_supply_pct, Some(0.0));
    }

    // ─── Integration test with synthetic gRPC data ───────────────────

    #[test]
    fn test_integration_synthetic_stream() {
        let mut config = default_config();
        config.min_buyers_for_cv = 3;
        config.min_tx_for_cu_cluster = 8;
        config.min_buy_txs_for_static_fee = 5;
        config.min_buy_txs_for_fixed_size = 5;
        config.min_wallets_for_flipper = 5;
        config.min_tx_for_jito_tip = 5;
        config.early_slot_count = 3;
        config.dev_sell_dust_token_amount_raw = 100;
        let creation_slot = 200;
        let t0 = 5000u64;
        let supply = 10_000_000u128;
        let mut agg = FingerprintAggregator::new(
            config,
            creation_slot,
            true,
            t0,
            Some(supply),
            9,
            Some("dev".into()),
        );

        let buy_specs = [
            (
                "dev",
                1_000_000_i128,
                2.0004_f64,
                2_000_000_000_u64,
                200_000_u32,
                15_u64,
                100_000_u64,
                Some(true),
                200_u64,
                5_u32,
                2_u32,
            ),
            (
                "whale1",
                800_000_i128,
                2.0005,
                1_800_000_000,
                200_000,
                15,
                101_000,
                Some(true),
                600,
                4,
                2,
            ),
            (
                "whale2",
                700_000_i128,
                2.0004,
                1_600_000_000,
                200_000,
                15,
                99_000,
                Some(true),
                900,
                4,
                2,
            ),
            (
                "whale3",
                600_000_i128,
                0.7502,
                1_400_000_000,
                200_000,
                15,
                102_000,
                Some(false),
                1_200,
                3,
                1,
            ),
            (
                "retail1",
                100_000_i128,
                0.7501,
                1_200_000_000,
                180_000,
                20,
                100_500,
                Some(false),
                1_500,
                3,
                1,
            ),
        ];
        for (
            idx,
            (
                owner,
                delta,
                sol_amount,
                pre_bal,
                cu_limit,
                cu_price,
                cu_consumed,
                jito,
                offset_ms,
                inner_ix,
                cpi_depth,
            ),
        ) in buy_specs.into_iter().enumerate()
        {
            let mut balances = HashMap::new();
            balances.insert(owner.to_string(), pre_bal);
            let mut event = make_event(
                200 + idx.min(3) as u64,
                idx as u32,
                t0 + offset_ms,
                vec![TokenDelta {
                    owner: owner.to_string(),
                    delta_raw: delta,
                    decimals: 9,
                }],
                balances,
                cu_price,
                inner_ix,
                cpi_depth,
            );
            event.compute_unit_limit = Some(cu_limit);
            event.compute_units_consumed = Some(cu_consumed);
            event.sol_amount_sol = Some(sol_amount);
            event.jito_tip_detected = jito;
            agg.ingest(&event);
        }

        let sell_specs = [
            (
                "dev",
                -300_000_i128,
                0.55_f64,
                101_500_u64,
                Some(true),
                2_500_u64,
                3_u32,
                2_u32,
            ),
            (
                "whale1",
                -300_000_i128,
                0.45,
                100_800,
                Some(false),
                3_000,
                3,
                2,
            ),
            (
                "whale2",
                -200_000_i128,
                0.40,
                99_500,
                Some(false),
                3_500,
                2,
                1,
            ),
        ];
        for (idx, (owner, delta, sol_amount, cu_consumed, jito, offset_ms, inner_ix, cpi_depth)) in
            sell_specs.into_iter().enumerate()
        {
            let mut event = make_event(
                202 + idx as u64,
                (idx + 10) as u32,
                t0 + offset_ms,
                vec![TokenDelta {
                    owner: owner.to_string(),
                    delta_raw: delta,
                    decimals: 9,
                }],
                HashMap::new(),
                18 + idx as u64,
                inner_ix,
                cpi_depth,
            );
            event.compute_unit_limit = Some(180_000);
            event.compute_units_consumed = Some(cu_consumed);
            event.sol_amount_sol = Some(sol_amount);
            event.jito_tip_detected = jito;
            agg.ingest(&event);
        }

        let m = agg.finalize();

        // All metrics should be non-null
        assert!(m.block0_sniped_supply_pct.is_some(), "block0 missing");
        assert!(m.flip_ratio_10s.is_some(), "flip missing");
        assert!(m.cu_price_p90_1s.is_some(), "cu_p90_1s missing");
        assert!(m.cu_price_p90_10s.is_some(), "cu_p90_10s missing");
        assert!(m.priority_fee_surge_slope.is_some(), "slope missing");
        assert!(m.buyer_pre_balance_cv.is_some(), "cv missing");
        assert!(m.avg_inner_ix_count_50tx.is_some(), "inner_ix missing");
        assert!(m.avg_cpi_depth_50tx.is_some(), "cpi_depth missing");
        assert!(m.sell_buy_ratio.is_some(), "sell_buy missing");
        assert!(
            m.compute_unit_cluster_dominance.is_some(),
            "cu_cluster missing"
        );
        assert!(m.static_fee_profile_ratio.is_some(), "static_fee missing");
        assert!(m.fixed_size_buy_ratio.is_some(), "fixed_buy missing");
        assert!(
            m.fixed_size_buy_ratio_1e4.is_some(),
            "fixed_buy_1e4 missing"
        );
        assert!(m.flipper_presence_ratio.is_some(), "flipper missing");
        assert!(m.jito_tip_intensity.is_some(), "jito missing");
        assert!(
            m.early_slot_volume_dominance_buy.is_some(),
            "early_slot missing"
        );
        assert!(
            m.early_top3_buy_volume_pct_3s.is_some(),
            "early_top3_buy_3s missing"
        );
        assert!(m.whale_reversal_ratio_top3.is_some(), "whale top3 missing");
        assert!(m.whale_reversal_ratio_top1.is_some(), "whale top1 missing");
        assert!(m.dev_paperhand_latency_ms.is_some(), "dev latency missing");
        assert!(!m.fingerprint_degraded, "should not be degraded");

        // block0 only counts buys in creation slot.
        let block0 = m.block0_sniped_supply_pct.unwrap();
        assert!(
            (block0 - 0.1).abs() < 1e-9,
            "block0 expected 0.1, got {}",
            block0
        );

        let sell_buy = m.sell_buy_ratio.unwrap();
        assert!(
            (sell_buy - 0.6).abs() < 1e-9,
            "sell_buy unexpected: {}",
            sell_buy
        );

        let flip = m.flip_ratio_10s.unwrap();
        assert!(flip.abs() < 1e-9, "flip ratio unexpected: {}", flip);
        assert!((m.compute_unit_cluster_dominance.unwrap() - 1.0).abs() < 1e-9);
        assert!((m.static_fee_profile_ratio.unwrap() - 0.8).abs() < 1e-9);
        assert!((m.fixed_size_buy_ratio.unwrap() - 0.4).abs() < 1e-9);
        assert!((m.fixed_size_buy_ratio_1e4.unwrap() - 0.4).abs() < 1e-9);
        assert!((m.flipper_presence_ratio.unwrap() - 0.6).abs() < 1e-9);
        assert!((m.jito_tip_intensity.unwrap() - 0.5).abs() < 1e-9);
        assert!((m.early_slot_volume_dominance_buy.unwrap() - (6.0013 / 7.5016)).abs() < 1e-9);
        assert!((m.early_top3_buy_volume_pct_3s.unwrap() - (6.0013 / 7.5016)).abs() < 1e-9);
        assert!((m.whale_reversal_ratio_top3.unwrap() - (800_000.0 / 2_500_000.0)).abs() < 1e-9);
        assert!((m.whale_reversal_ratio_top1.unwrap() - 0.3).abs() < 1e-9);
        assert_eq!(m.dev_paperhand_latency_ms, Some(2_500));
        assert_eq!(m.dev_sold_within_3s, Some(true));
        assert_eq!(m.dev_sold_within_5s, Some(true));

        // Verify log line format
        let line = m.log_line("pool123", "mintABC");
        assert!(line.starts_with("FINGERPRINT "));
        assert!(line.contains("pool=pool123"));
        assert!(line.contains("mint=mintABC"));
        assert!(line.contains("sell_buy=0.6000"));
        assert!(line.contains("cu_cluster=1.0000"));
        assert!(line.contains("static_fee=0.8000"));
        assert!(line.contains("fixed_buy=0.4000"));
        assert!(line.contains("flipper=0.6000"));
        assert!(line.contains("jito_tip=0.5000"));
        assert!(line.contains("early_slot_dom=0.8000"));
        assert!(line.contains("early_top3_buy_3s=0.8000"));
        assert!(line.contains("whale_rev_top3=0.3200"));
        assert!(line.contains("dev_latency_ms=2500"));
        assert!(line.contains("dev_3s=true"));
        assert!(line.contains("dev_5s=true"));
    }

    // ─── Percentile helper ───────────────────────────────────────────

    #[test]
    fn test_percentile_90() {
        assert_eq!(percentile_90(&[]), None);
        assert_eq!(percentile_90(&[42]), Some(42.0));
        // [1,2,3,4,5,6,7,8,9,10] → P90 = 9
        let v: Vec<u64> = (1..=10).collect();
        let p = percentile_90(&v).unwrap();
        assert!((p - 9.0).abs() < 1e-9);
    }

    #[test]
    fn test_mean_u32() {
        assert_eq!(mean_u32(&[]), None);
        assert_eq!(mean_u32(&[10]), Some(10.0));
        let v = [2, 4, 6];
        assert!((mean_u32(&v).unwrap() - 4.0).abs() < 1e-9);
    }
}
