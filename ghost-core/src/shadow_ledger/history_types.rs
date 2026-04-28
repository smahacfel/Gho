//! Neutral history/replay types extracted from the transitional core gatekeeper.
//!
//! These types model canonical buffered transactions and deterministic reserve
//! reconstruction. They are domain/storage helpers and do not own runtime
//! policy, approval state, observation windows or routing semantics.

use solana_sdk::pubkey::Pubkey;

use crate::market_state::BondingCurve;

use super::trade_types::{TradeSide, TradeSnapshot, TradeSource, TxKey};

/// Epistemic/finality tier of the curve state currently stored in ShadowLedger.
///
/// This is intentionally separate from wall-clock freshness:
/// - `Speculative`  = bootstrap/genesis seed or otherwise non-authoritative curve
/// - `Provisional`  = confirmed/trusted curve, but not yet finalized
/// - `Finalized`    = finalized on-chain truth
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveFinality {
    #[default]
    Speculative,
    Provisional,
    Finalized,
}

impl CurveFinality {
    #[inline]
    pub const fn from_curve_data_known(curve_data_known: bool) -> Self {
        if curve_data_known {
            Self::Provisional
        } else {
            Self::Speculative
        }
    }

    #[inline]
    pub const fn normalized(self, curve_data_known: bool) -> Self {
        if !curve_data_known {
            Self::Speculative
        } else if matches!(self, Self::Finalized) {
            Self::Finalized
        } else {
            Self::Provisional
        }
    }

    #[inline]
    pub const fn is_finalized(self) -> bool {
        matches!(self, Self::Finalized)
    }

    #[inline]
    pub const fn requires_caution(self) -> bool {
        !self.is_finalized()
    }

    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Speculative => "speculative",
            Self::Provisional => "provisional",
            Self::Finalized => "finalized",
        }
    }
}

#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum HistoryError {
    #[error("invalid timestamp (must be > 0)")]
    InvalidTimestamp,

    #[error("k invariant violation: expected={expected}, actual={actual}, ratio={ratio:.4}")]
    KInvariantViolation {
        expected: u128,
        actual: u128,
        ratio: f64,
    },

    #[error("trade snapshot error: {0}")]
    TradeSnapshotError(#[from] super::trade_types::TradeSnapshotError),
}

/// Minimal transaction representation stored in the history buffer.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BufferedTx {
    /// Deterministic transaction key for ordering and deduplication.
    pub tx_key: TxKey,
    /// Trade direction (Buy or Sell).
    pub side: TradeSide,
    /// SOL amount in lamports (input for Buy, output for Sell).
    pub d_sol_lamports: u64,
    /// Token amount (output for Buy, input for Sell).
    pub d_tok_units: u64,
    /// Whether this is a developer buy (first buy from creator).
    pub dev_buy: bool,
    /// Trader wallet address.
    pub trader: Option<Pubkey>,
}

impl BufferedTx {
    /// Create a new BufferedTx with validation.
    pub fn new(
        tx_key: TxKey,
        side: TradeSide,
        d_sol_lamports: u64,
        d_tok_units: u64,
        dev_buy: bool,
        trader: Option<Pubkey>,
    ) -> Result<Self, HistoryError> {
        if tx_key.timestamp_ms == 0 {
            return Err(HistoryError::InvalidTimestamp);
        }
        Ok(Self {
            tx_key,
            side,
            d_sol_lamports,
            d_tok_units,
            dev_buy,
            trader,
        })
    }
}

/// Running curve state reconstructed from transaction history.
#[derive(Clone, Debug)]
pub struct ReconstructedState {
    /// Virtual SOL reserves in lamports.
    pub reserve_sol_lamports: u64,
    /// Virtual token reserves in base units.
    pub reserve_tok_units: u64,
    /// Constant product invariant (k = R_sol * R_tok).
    pub k: u128,
    /// Number of transactions applied.
    pub tx_count: u64,
    /// Cumulative BUY-side volume in SOL (lamports).
    pub cum_volume_sol_lamports: u64,
}

impl ReconstructedState {
    /// Initialize from initial curve reserves.
    pub fn from_initial_reserves(reserve_sol_lamports: u64, reserve_tok_units: u64) -> Self {
        let k = (reserve_sol_lamports as u128) * (reserve_tok_units as u128);
        Self {
            reserve_sol_lamports,
            reserve_tok_units,
            k,
            tx_count: 0,
            cum_volume_sol_lamports: 0,
        }
    }

    /// Initialize from a BondingCurve object.
    pub fn reserves_from_curve(curve: &BondingCurve) -> Self {
        Self::from_initial_reserves(curve.virtual_sol_reserves, curve.virtual_token_reserves)
    }

    /// Apply a trade using fee-aware integer-safe k-invariant math.
    pub fn apply_trade_strict(&mut self, tx: &BufferedTx) -> (f64, u64) {
        let (price_after, computed_delta) = match tx.side {
            TradeSide::Buy => {
                let sol_after_fee = tx.d_sol_lamports.saturating_mul(99) / 100;
                let new_sol = self.reserve_sol_lamports.saturating_add(sol_after_fee);
                let new_tok = if new_sol == 0 {
                    0
                } else {
                    std::cmp::min(self.k / (new_sol as u128), u64::MAX as u128) as u64
                };
                let d_tok_computed = self.reserve_tok_units.saturating_sub(new_tok);

                self.reserve_sol_lamports = new_sol;
                self.reserve_tok_units = new_tok;
                self.cum_volume_sol_lamports = self
                    .cum_volume_sol_lamports
                    .saturating_add(tx.d_sol_lamports);

                let price = if self.reserve_tok_units == 0 {
                    0.0
                } else {
                    self.reserve_sol_lamports as f64 / self.reserve_tok_units as f64
                };

                (price, d_tok_computed)
            }
            TradeSide::Sell => {
                let new_tok = self.reserve_tok_units.saturating_add(tx.d_tok_units);
                let new_sol = if new_tok == 0 {
                    0
                } else {
                    std::cmp::min(self.k / (new_tok as u128), u64::MAX as u128) as u64
                };
                let d_sol_before_fee = self.reserve_sol_lamports.saturating_sub(new_sol);
                let d_sol_computed = d_sol_before_fee.saturating_mul(99) / 100;

                self.reserve_tok_units = new_tok;
                self.reserve_sol_lamports = new_sol;

                let price = if self.reserve_tok_units == 0 {
                    0.0
                } else {
                    self.reserve_sol_lamports as f64 / self.reserve_tok_units as f64
                };

                (price, d_sol_computed)
            }
        };

        self.tx_count = self.tx_count.saturating_add(1);
        (price_after, computed_delta)
    }

    /// Apply a trade and return the post-trade price (legacy, non-authoritative).
    #[deprecated(
        since = "0.0.0",
        note = "non-authoritative: does not apply protocol fee or k-invariant math. \
                Use apply_trade_strict for all state-evolution authority paths."
    )]
    pub fn apply_trade(&mut self, tx: &BufferedTx) -> f64 {
        match tx.side {
            TradeSide::Buy => {
                self.reserve_sol_lamports =
                    self.reserve_sol_lamports.saturating_add(tx.d_sol_lamports);
                self.reserve_tok_units = self.reserve_tok_units.saturating_sub(tx.d_tok_units);
                self.cum_volume_sol_lamports = self
                    .cum_volume_sol_lamports
                    .saturating_add(tx.d_sol_lamports);
            }
            TradeSide::Sell => {
                self.reserve_tok_units = self.reserve_tok_units.saturating_add(tx.d_tok_units);
                self.reserve_sol_lamports =
                    self.reserve_sol_lamports.saturating_sub(tx.d_sol_lamports);
            }
        }

        self.tx_count += 1;

        if self.reserve_tok_units == 0 {
            0.0
        } else {
            self.reserve_sol_lamports as f64 / self.reserve_tok_units as f64
        }
    }

    /// Sanity check: verify k is approximately preserved.
    pub fn sanity_check_k_strict(&self, tolerance_pct: f64) -> Result<f64, HistoryError> {
        let new_k = (self.reserve_sol_lamports as u128) * (self.reserve_tok_units as u128);
        if self.k == 0 {
            if new_k == 0 {
                return Ok(1.0);
            } else {
                return Err(HistoryError::KInvariantViolation {
                    expected: self.k,
                    actual: new_k,
                    ratio: f64::INFINITY,
                });
            }
        }
        let ratio = new_k as f64 / self.k as f64;
        if (1.0 - tolerance_pct / 100.0) <= ratio && ratio <= (1.0 + tolerance_pct / 100.0) {
            Ok(ratio)
        } else {
            Err(HistoryError::KInvariantViolation {
                expected: self.k,
                actual: new_k,
                ratio,
            })
        }
    }

    pub fn sanity_check_k(&self, tolerance_pct: f64) -> bool {
        self.sanity_check_k_strict(tolerance_pct).is_ok()
    }

    /// Apply a hypothetical BUY trade using fee-aware integer-safe k-invariant math.
    pub fn apply_hypothetical_buy(&mut self, sol_lamports: u64) -> (f64, u64) {
        if sol_lamports == 0 {
            let price = if self.reserve_tok_units == 0 {
                0.0
            } else {
                self.reserve_sol_lamports as f64 / self.reserve_tok_units as f64
            };
            return (price, 0);
        }

        let sol_after_fee = sol_lamports.saturating_mul(99) / 100;
        let new_sol = self.reserve_sol_lamports.saturating_add(sol_after_fee);
        let new_tok = if new_sol == 0 {
            0
        } else {
            std::cmp::min(self.k / (new_sol as u128), u64::MAX as u128) as u64
        };
        let d_tok = self.reserve_tok_units.saturating_sub(new_tok);

        self.reserve_sol_lamports = new_sol;
        self.reserve_tok_units = new_tok;
        self.tx_count = self.tx_count.saturating_add(1);
        self.cum_volume_sol_lamports = self.cum_volume_sol_lamports.saturating_add(sol_lamports);

        let price = if self.reserve_tok_units == 0 {
            0.0
        } else {
            self.reserve_sol_lamports as f64 / self.reserve_tok_units as f64
        };
        (price, d_tok)
    }

    /// Apply a hypothetical SELL trade using fee-aware integer-safe k-invariant math.
    pub fn apply_hypothetical_sell(&mut self, tok_units: u64) -> (f64, u64) {
        if tok_units == 0 {
            let price = if self.reserve_tok_units == 0 {
                0.0
            } else {
                self.reserve_sol_lamports as f64 / self.reserve_tok_units as f64
            };
            return (price, 0);
        }

        let new_tok = self.reserve_tok_units.saturating_add(tok_units);
        let new_sol = if new_tok == 0 {
            0
        } else {
            std::cmp::min(self.k / (new_tok as u128), u64::MAX as u128) as u64
        };
        let d_sol_before_fee = self.reserve_sol_lamports.saturating_sub(new_sol);
        let d_sol = d_sol_before_fee.saturating_mul(99) / 100;

        self.reserve_tok_units = new_tok;
        self.reserve_sol_lamports = new_sol;
        self.tx_count = self.tx_count.saturating_add(1);

        let price = if self.reserve_tok_units == 0 {
            0.0
        } else {
            self.reserve_sol_lamports as f64 / self.reserve_tok_units as f64
        };
        (price, d_sol)
    }

    /// Capture the current state as a reconciliation point.
    pub fn to_reconciliation_point(&self) -> ReconciliationPoint {
        ReconciliationPoint {
            shadow_sol_lamports: self.reserve_sol_lamports,
            shadow_tok_units: self.reserve_tok_units,
            shadow_k: self.k,
            tx_count: self.tx_count,
        }
    }
}

/// Frozen snapshot of reconstructed state used for AccountUpdate reconciliation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconciliationPoint {
    pub shadow_sol_lamports: u64,
    pub shadow_tok_units: u64,
    pub shadow_k: u128,
    pub tx_count: u64,
}

/// Drift between reconstructed state and authoritative on-chain data.
#[derive(Debug, Clone, Copy)]
pub struct ReconciliationDiff {
    pub sol_drift_lamports: i128,
    pub tok_drift_units: i128,
    pub within_tolerance: bool,
}

impl ReconciliationPoint {
    pub fn compare_with_account_update(
        &self,
        on_chain_sol: u64,
        on_chain_tok: u64,
        tolerance_lamports: u64,
    ) -> ReconciliationDiff {
        let sol_drift = self.shadow_sol_lamports as i128 - on_chain_sol as i128;
        let tok_drift = self.shadow_tok_units as i128 - on_chain_tok as i128;
        let within_tolerance = sol_drift.unsigned_abs() <= tolerance_lamports as u128;
        ReconciliationDiff {
            sol_drift_lamports: sol_drift,
            tok_drift_units: tok_drift,
            within_tolerance,
        }
    }
}

/// Build fee-aware trade snapshots from buffered history using observed deltas.
pub fn build_trade_snapshots_observed(
    base_mint: Pubkey,
    initial_state: ReconstructedState,
    txs: &[BufferedTx],
) -> Result<Vec<TradeSnapshot>, HistoryError> {
    let mut state = initial_state;
    let mut snapshots = Vec::with_capacity(txs.len());

    let mut sorted_txs = txs.to_vec();
    sorted_txs.sort_by(|a, b| a.tx_key.cmp(&b.tx_key));

    for tx in sorted_txs {
        let (price_after, computed_delta) = state.apply_trade_strict(&tx);

        let (d_sol_final, d_tok_final) = match tx.side {
            TradeSide::Buy => (tx.d_sol_lamports, computed_delta),
            TradeSide::Sell => (computed_delta, tx.d_tok_units),
        };

        let price_avg = if d_tok_final == 0 {
            0.0
        } else {
            d_sol_final as f64 / d_tok_final as f64
        };

        let trade_snapshot = TradeSnapshot::new(
            base_mint,
            tx.tx_key,
            tx.side,
            tx.dev_buy,
            d_sol_final,
            d_tok_final,
            price_avg,
            price_after,
            state.reserve_sol_lamports,
            state.reserve_tok_units,
            None,
            tx.trader,
            TradeSource::GatekeeperBuffered,
        )?;

        snapshots.push(trade_snapshot);
    }

    Ok(snapshots)
}
