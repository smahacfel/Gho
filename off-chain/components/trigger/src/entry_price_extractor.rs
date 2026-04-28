//! Entry Price Extractor - Extract Real Entry Prices from Buy Transactions
//!
//! This module provides a simplified interface for extracting entry prices
//! from confirmed buy transactions, used by Revolver for accurate TP/SL calculations.
//!
//! ## Overview
//!
//! After a buy transaction is confirmed, we need to know the actual entry price
//! (not the estimated price) to set accurate take-profit and stop-loss levels.
//! This module wraps the underlying `TransactionMonitor` functionality in a
//! clean, easy-to-use API.
//!
//! ## Usage
//!
//! ```ignore
//! use trigger::EntryPriceExtractor;
//! use solana_client::nonblocking::rpc_client::RpcClient;
//! use std::sync::Arc;
//!
//! let rpc_client = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
//! let extractor = EntryPriceExtractor::new(rpc_client);
//!
//! // Extract entry price from a confirmed buy transaction
//! let info = extractor.extract_from_signature(
//!     &signature,
//!     &user_wallet,
//!     &token_mint,
//! ).await?;
//!
//! println!("Entry price: {} lamports per token (1e9 scale)", info.price_lamports_per_token);
//! println!("Tokens received: {}", info.tokens_received);
//! println!("Swap SOL spent: {}", info.sol_spent);
//! ```
//!
//! ## Integration with Revolver
//!
//! The extracted entry price is used to load the Revolver magazine with accurate
//! take-profit and stop-loss targets:
//!
//! ```ignore
//! let entry_info = extractor.extract_from_signature(&sig, &wallet, &mint).await?;
//!
//! // Load magazine with real entry price for TP/SL calculation
//! revolver_worker.load_magazine_from_direct_buy(
//!     mint,
//!     entry_info.tokens_received,
//!     entry_info.price_lamports_per_token,
//! ).await?;
//! ```

use crate::transaction_monitor::{BuyTransactionMetadata, TransactionMonitor};
use ghost_core::market_state::BondingCurve;
use ghost_core::shadow_ledger::types::{PriceReason, PriceState};
use ghost_core::shadow_ledger::MarketSnapshot;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};

const LAMPORTS_PER_SOL_F64: f64 = 1_000_000_000.0;
const PUMP_TOKEN_DECIMAL_FACTOR_F64: f64 = 1_000_000.0;

/// Errors that can occur during entry price extraction
#[derive(Error, Debug)]
pub enum EntryPriceError {
    /// Transaction was not found or not confirmed
    #[error("Transaction not found: {0}")]
    TransactionNotFound(String),

    /// Failed to parse transaction data
    #[error("Failed to parse: {0}")]
    ParseError(String),

    /// Entry price calculation failed
    #[error("Calculation error: {0}")]
    CalculationError(String),

    /// No balance change detected (not a valid buy transaction)
    #[error("No balance change detected")]
    NoBalanceChange,

    /// RPC or network error
    #[error("RPC error: {0}")]
    RpcError(String),

    /// Maximum retry attempts exceeded
    #[error("Max retries exceeded: {0}")]
    MaxRetriesExceeded(String),
}

/// Maximum backoff exponent to cap delay at ~6.4 seconds (100ms * 2^6 = 6400ms)
const MAX_BACKOFF_EXPONENT: u32 = 6;

/// Extracted entry price information from a buy transaction
///
/// This struct contains all the relevant information extracted from
/// a confirmed buy transaction that is needed for TP/SL calculations.
#[derive(Debug, Clone)]
pub struct EntryPriceInfo {
    /// Entry price in lamports per token (scaled by 1e9 for precision)
    ///
    /// This is calculated as: (sol_spent * 1e9) / tokens_received
    /// For example, if you spent 1 SOL for 1M tokens:
    /// - price = (1_000_000_000 * 1_000_000_000) / 1_000_000 = 1_000_000_000_000
    /// - This means 1000 lamports per token (before 1e9 scaling)
    pub price_lamports_per_token: u64,

    /// Number of tokens received in the transaction
    pub tokens_received: u64,

    /// SOL that actually entered the bonding curve during the executed BUY
    /// (in lamports), excluding inline tip, payer-side rent, and network fee noise.
    pub sol_spent: u64,

    /// Transaction signature
    pub signature: Signature,

    /// Slot when the transaction was confirmed
    pub slot: u64,

    /// Token mint address
    pub mint: Pubkey,
    /// Primary token account that received the BUY delta.
    pub token_account: Pubkey,
    /// Token balance on that account immediately after the BUY confirmed.
    pub token_balance_after_buy: u64,
    /// Token decimals reported in transaction metadata.
    pub token_decimals: u8,
    /// Token program reported in transaction metadata, when available.
    pub token_program: Option<Pubkey>,
    /// Authoritative Pump fee recipient extracted from the confirmed BUY instruction.
    pub fee_recipient: Option<Pubkey>,
}

impl From<BuyTransactionMetadata> for EntryPriceInfo {
    fn from(meta: BuyTransactionMetadata) -> Self {
        Self {
            price_lamports_per_token: meta.entry_price,
            tokens_received: meta.tokens_received,
            sol_spent: meta.sol_spent,
            signature: meta.signature,
            slot: meta.slot,
            mint: meta.mint,
            token_account: meta.token_account,
            token_balance_after_buy: meta.token_balance_after_buy,
            token_decimals: meta.token_decimals,
            token_program: meta.token_program,
            fee_recipient: meta.fee_recipient,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceTruthSource {
    ConfirmedSell,
    ShadowLedgerSnapshot,
    CanonicalAccountStateSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceTruthStatus {
    Resolved,
    Stale,
    BackfillRequired,
    Failure,
    SemanticViolation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceTruthEvidence {
    pub source: PriceTruthSource,
    pub status: PriceTruthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_state: Option<PriceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_reason: Option<PriceReason>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShadowExitPriceSample {
    pub exit_price_sol: f64,
    pub curve: BondingCurve,
    pub evidence: PriceTruthEvidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShadowExitTruth {
    pub exit_price_sol: f64,
    pub exit_token_amount_raw: u64,
    pub entry_value_sol: f64,
    pub exit_value_sol: f64,
    pub gross_pnl_sol: f64,
    pub net_pnl_sol: f64,
    pub estimated_costs_sol: f64,
    pub pnl_pct: f64,
    pub evidence: PriceTruthEvidence,
}

#[derive(Error, Debug, Clone)]
pub enum PriceTruthError {
    #[error("Shadow exit price sample stale: age_ms={age_ms} max_age_ms={max_age_ms}")]
    Stale {
        age_ms: u64,
        max_age_ms: u64,
        evidence: PriceTruthEvidence,
    },
    #[error("Shadow exit price sample requires backfill: {reason}")]
    BackfillRequired {
        reason: &'static str,
        evidence: PriceTruthEvidence,
    },
    #[error("Shadow exit price truth failed: {reason}")]
    Failure {
        reason: String,
        evidence: PriceTruthEvidence,
    },
    #[error("Shadow exit semantic violation: {reason}")]
    SemanticViolation {
        reason: String,
        evidence: PriceTruthEvidence,
    },
}

impl PriceTruthError {
    pub fn evidence(&self) -> &PriceTruthEvidence {
        match self {
            Self::Stale { evidence, .. }
            | Self::BackfillRequired { evidence, .. }
            | Self::Failure { evidence, .. }
            | Self::SemanticViolation { evidence, .. } => evidence,
        }
    }

    pub fn status(&self) -> PriceTruthStatus {
        self.evidence().status
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PriceTruthResolver;

impl PriceTruthResolver {
    fn evidence_snapshot_id(evidence: &PriceTruthEvidence) -> String {
        let slot = evidence
            .slot
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string());
        let timestamp_ms = evidence
            .timestamp_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string());
        format!("slot={slot}:timestamp_ms={timestamp_ms}")
    }

    fn curve_from_shadow_snapshot(snapshot: &MarketSnapshot) -> Option<BondingCurve> {
        let token_reserves = snapshot.reserve_base.round();
        let sol_reserves_lamports = (snapshot.reserve_quote * LAMPORTS_PER_SOL_F64).round();
        if !token_reserves.is_finite()
            || token_reserves <= 0.0
            || token_reserves > u64::MAX as f64
            || !sol_reserves_lamports.is_finite()
            || sol_reserves_lamports <= 0.0
            || sol_reserves_lamports > u64::MAX as f64
        {
            return None;
        }

        let token_reserves = token_reserves as u64;
        let sol_reserves_lamports = sol_reserves_lamports as u64;
        Some(BondingCurve {
            discriminator: 0,
            virtual_token_reserves: token_reserves,
            virtual_sol_reserves: sol_reserves_lamports,
            real_token_reserves: token_reserves,
            real_sol_reserves: sol_reserves_lamports,
            token_total_supply: token_reserves,
            complete: 0,
            _padding: [0; 7],
        })
    }

    pub fn normalize_shadow_snapshot_price_sol(snapshot: &MarketSnapshot) -> Option<f64> {
        if snapshot.reserve_base.is_finite()
            && snapshot.reserve_base > 0.0
            && snapshot.reserve_quote.is_finite()
            && snapshot.reserve_quote > 0.0
        {
            let reserve_base_tokens = snapshot.reserve_base / PUMP_TOKEN_DECIMAL_FACTOR_F64;
            if reserve_base_tokens.is_finite() && reserve_base_tokens > 0.0 {
                let price_sol_per_token = snapshot.reserve_quote / reserve_base_tokens;
                if price_sol_per_token.is_finite() && price_sol_per_token > 0.0 {
                    return Some(price_sol_per_token);
                }
            }
        }

        if snapshot.price_sol_per_token.is_finite() && snapshot.price_sol_per_token > 0.0 {
            let price_sol_per_token = snapshot.price_sol_per_token
                * (PUMP_TOKEN_DECIMAL_FACTOR_F64 / LAMPORTS_PER_SOL_F64);
            if price_sol_per_token.is_finite() && price_sol_per_token > 0.0 {
                return Some(price_sol_per_token);
            }
        }

        None
    }

    pub fn resolve_shadow_exit_sample(
        snapshot: &MarketSnapshot,
        now_ms: u64,
        stale_after_ms: u64,
    ) -> Result<ShadowExitPriceSample, PriceTruthError> {
        Self::resolve_shadow_exit_sample_with_source(
            snapshot,
            now_ms,
            stale_after_ms,
            PriceTruthSource::ShadowLedgerSnapshot,
        )
    }

    pub fn resolve_shadow_exit_sample_with_source(
        snapshot: &MarketSnapshot,
        now_ms: u64,
        stale_after_ms: u64,
        source: PriceTruthSource,
    ) -> Result<ShadowExitPriceSample, PriceTruthError> {
        let age_ms = now_ms.saturating_sub(snapshot.timestamp_ms);
        let evidence = PriceTruthEvidence {
            source,
            status: PriceTruthStatus::Resolved,
            detail: None,
            slot: snapshot.slot,
            timestamp_ms: Some(snapshot.timestamp_ms),
            age_ms: Some(age_ms),
            price_state: Some(snapshot.price_state),
            price_reason: snapshot.price_reason,
        };

        if snapshot.price_state.is_unknown() {
            return Err(PriceTruthError::BackfillRequired {
                reason: "price_state_unknown",
                evidence: PriceTruthEvidence {
                    status: PriceTruthStatus::BackfillRequired,
                    detail: Some("shadow snapshot price is not ready yet".to_string()),
                    ..evidence
                },
            });
        }

        if snapshot.price_state.is_invalid() {
            return Err(PriceTruthError::Failure {
                reason: "price_state_invalid".to_string(),
                evidence: PriceTruthEvidence {
                    status: PriceTruthStatus::Failure,
                    detail: Some("shadow snapshot price is marked invalid".to_string()),
                    ..evidence
                },
            });
        }

        let Some(exit_price_sol) = Self::normalize_shadow_snapshot_price_sol(snapshot) else {
            return Err(PriceTruthError::Failure {
                reason: format!(
                    "shadow_price_normalization_failed raw_price={} reserve_base={} reserve_quote={}",
                    snapshot.price_sol_per_token, snapshot.reserve_base, snapshot.reserve_quote
                ),
                evidence: PriceTruthEvidence {
                    status: PriceTruthStatus::Failure,
                    detail: Some(
                        "shadow snapshot price could not be normalized into canonical SOL/token"
                            .to_string(),
                    ),
                    ..evidence
                },
            });
        };

        if stale_after_ms > 0 && age_ms > stale_after_ms {
            return Err(PriceTruthError::Stale {
                age_ms,
                max_age_ms: stale_after_ms,
                evidence: PriceTruthEvidence {
                    status: PriceTruthStatus::Stale,
                    detail: Some(format!(
                        "sample_age_ms={} exceeded stale_after_ms={}",
                        age_ms, stale_after_ms
                    )),
                    ..evidence
                },
            });
        }

        let Some(curve) = Self::curve_from_shadow_snapshot(snapshot) else {
            return Err(PriceTruthError::Failure {
                reason: format!(
                    "shadow_curve_materialization_failed reserve_base={} reserve_quote={}",
                    snapshot.reserve_base, snapshot.reserve_quote
                ),
                evidence: PriceTruthEvidence {
                    status: PriceTruthStatus::Failure,
                    detail: Some(
                        "shadow snapshot reserves could not be converted into executable curve state"
                            .to_string(),
                    ),
                    ..evidence
                },
            });
        };

        Ok(ShadowExitPriceSample {
            exit_price_sol,
            curve,
            evidence,
        })
    }

    pub fn resolve_shadow_exit(
        entry_price_sol: f64,
        exit_token_amount_raw: u64,
        sample: &ShadowExitPriceSample,
        estimated_costs_sol: f64,
    ) -> Result<ShadowExitTruth, PriceTruthError> {
        if !entry_price_sol.is_finite() || entry_price_sol <= 0.0 {
            return Err(PriceTruthError::Failure {
                reason: format!("invalid_entry_price={entry_price_sol}"),
                evidence: PriceTruthEvidence {
                    source: sample.evidence.source,
                    status: PriceTruthStatus::Failure,
                    detail: Some("shadow entry price is missing or invalid".to_string()),
                    slot: sample.evidence.slot,
                    timestamp_ms: sample.evidence.timestamp_ms,
                    age_ms: sample.evidence.age_ms,
                    price_state: sample.evidence.price_state,
                    price_reason: sample.evidence.price_reason,
                },
            });
        }

        if exit_token_amount_raw == 0 {
            return Err(PriceTruthError::Failure {
                reason: "invalid_exit_token_amount_raw=0".to_string(),
                evidence: PriceTruthEvidence {
                    source: sample.evidence.source,
                    status: PriceTruthStatus::Failure,
                    detail: Some("shadow exit token amount is missing or invalid".to_string()),
                    slot: sample.evidence.slot,
                    timestamp_ms: sample.evidence.timestamp_ms,
                    age_ms: sample.evidence.age_ms,
                    price_state: sample.evidence.price_state,
                    price_reason: sample.evidence.price_reason,
                },
            });
        }

        if sample.curve.virtual_token_reserves > u64::MAX.saturating_sub(exit_token_amount_raw) {
            return Err(PriceTruthError::Failure {
                reason: format!("shadow_exit_amount_overflows_curve={exit_token_amount_raw}"),
                evidence: PriceTruthEvidence {
                    source: sample.evidence.source,
                    status: PriceTruthStatus::Failure,
                    detail: Some(
                        "shadow exit token amount cannot be applied to the current curve reserves"
                            .to_string(),
                    ),
                    slot: sample.evidence.slot,
                    timestamp_ms: sample.evidence.timestamp_ms,
                    age_ms: sample.evidence.age_ms,
                    price_state: sample.evidence.price_state,
                    price_reason: sample.evidence.price_reason,
                },
            });
        }

        if !estimated_costs_sol.is_finite() || estimated_costs_sol < 0.0 {
            return Err(PriceTruthError::Failure {
                reason: format!("invalid_estimated_costs_sol={estimated_costs_sol}"),
                evidence: PriceTruthEvidence {
                    source: sample.evidence.source,
                    status: PriceTruthStatus::Failure,
                    detail: Some(
                        "shadow estimated costs must be finite and non-negative".to_string(),
                    ),
                    slot: sample.evidence.slot,
                    timestamp_ms: sample.evidence.timestamp_ms,
                    age_ms: sample.evidence.age_ms,
                    price_state: sample.evidence.price_state,
                    price_reason: sample.evidence.price_reason,
                },
            });
        }

        let exit_qty_tokens = exit_token_amount_raw as f64 / PUMP_TOKEN_DECIMAL_FACTOR_F64;
        let entry_value_sol = entry_price_sol * exit_qty_tokens;
        let exit_value_sol =
            sample.curve.calculate_sell_price(exit_token_amount_raw) as f64 / LAMPORTS_PER_SOL_F64;
        let exit_price_sol = if exit_qty_tokens > 0.0 {
            exit_value_sol / exit_qty_tokens
        } else {
            0.0
        };
        let oracle_spot_price = sample.exit_price_sol;
        let invariant_tolerance = oracle_spot_price.abs() * 1e-9 + 1e-15;
        if !oracle_spot_price.is_finite() || oracle_spot_price <= 0.0 {
            return Err(PriceTruthError::Failure {
                reason: format!("invalid_oracle_spot_price={oracle_spot_price}"),
                evidence: PriceTruthEvidence {
                    source: sample.evidence.source,
                    status: PriceTruthStatus::Failure,
                    detail: Some(
                        "shadow oracle spot price is missing or invalid for exit invariant"
                            .to_string(),
                    ),
                    slot: sample.evidence.slot,
                    timestamp_ms: sample.evidence.timestamp_ms,
                    age_ms: sample.evidence.age_ms,
                    price_state: sample.evidence.price_state,
                    price_reason: sample.evidence.price_reason,
                },
            });
        }
        if exit_price_sol > oracle_spot_price + invariant_tolerance {
            let snapshot_id = Self::evidence_snapshot_id(&sample.evidence);
            warn!(
                truth_status = ?PriceTruthStatus::SemanticViolation,
                sample_slot = ?sample.evidence.slot,
                oracle_spot_price,
                reserve_in = sample.curve.virtual_token_reserves,
                reserve_out = sample.curve.virtual_sol_reserves,
                exit_qty = exit_token_amount_raw,
                computed_exit_price = exit_price_sol,
                formula_id = "bonding_curve.calculate_sell_price.v1",
                snapshot_id = %snapshot_id,
                source_path = "trigger.price_truth.resolve_shadow_exit",
                "PriceTruthResolver: shadow exit semantic violation"
            );
            return Err(PriceTruthError::SemanticViolation {
                reason: "exit_fill_above_oracle_spot".to_string(),
                evidence: PriceTruthEvidence {
                    source: sample.evidence.source,
                    status: PriceTruthStatus::SemanticViolation,
                    detail: Some(format!(
                        "semantic_violation=exit_fill_above_oracle_spot; oracle_spot_price={oracle_spot_price}; computed_exit_price={exit_price_sol}; reserve_in={}; reserve_out={}; exit_qty={exit_token_amount_raw}; formula_id=bonding_curve.calculate_sell_price.v1; snapshot_id={snapshot_id}; source_path=trigger.price_truth.resolve_shadow_exit",
                        sample.curve.virtual_token_reserves,
                        sample.curve.virtual_sol_reserves,
                    )),
                    slot: sample.evidence.slot,
                    timestamp_ms: sample.evidence.timestamp_ms,
                    age_ms: sample.evidence.age_ms,
                    price_state: sample.evidence.price_state,
                    price_reason: sample.evidence.price_reason,
                },
            });
        }
        if !entry_value_sol.is_finite() || entry_value_sol <= 0.0 {
            return Err(PriceTruthError::Failure {
                reason: format!("invalid_entry_value_sol={entry_value_sol}"),
                evidence: PriceTruthEvidence {
                    source: sample.evidence.source,
                    status: PriceTruthStatus::Failure,
                    detail: Some("shadow entry value is missing or invalid".to_string()),
                    slot: sample.evidence.slot,
                    timestamp_ms: sample.evidence.timestamp_ms,
                    age_ms: sample.evidence.age_ms,
                    price_state: sample.evidence.price_state,
                    price_reason: sample.evidence.price_reason,
                },
            });
        }
        let gross_pnl_sol = exit_value_sol - entry_value_sol;
        let net_pnl_sol = gross_pnl_sol - estimated_costs_sol;
        let pnl_pct = if entry_value_sol > 0.0 {
            (gross_pnl_sol / entry_value_sol) * 100.0
        } else {
            0.0
        };

        Ok(ShadowExitTruth {
            exit_price_sol,
            exit_token_amount_raw,
            entry_value_sol,
            exit_value_sol,
            gross_pnl_sol,
            net_pnl_sol,
            estimated_costs_sol,
            pnl_pct,
            evidence: sample.evidence.clone(),
        })
    }
}

/// Entry Price Extractor - Simplified interface for extracting entry prices
///
/// This struct wraps `TransactionMonitor` functionality in a clean API
/// specifically designed for extracting entry prices from buy transactions.
pub struct EntryPriceExtractor {
    /// Underlying transaction monitor
    monitor: TransactionMonitor,
    /// RPC client reference (for direct access if needed)
    rpc_client: Arc<RpcClient>,
}

impl EntryPriceExtractor {
    /// Create a new EntryPriceExtractor
    ///
    /// # Arguments
    /// * `rpc_client` - RPC client for fetching transaction data
    ///
    /// # Example
    /// ```ignore
    /// let rpc_client = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
    /// let extractor = EntryPriceExtractor::new(rpc_client);
    /// ```
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            monitor: TransactionMonitor::new(rpc_client.clone()),
            rpc_client,
        }
    }

    /// Extract entry price information from a confirmed buy transaction
    ///
    /// This method fetches the transaction from the blockchain, parses the
    /// pre/post token balances, and calculates the entry price.
    ///
    /// # Arguments
    /// * `signature` - The transaction signature to analyze
    /// * `user_wallet` - The wallet that performed the buy (payer)
    /// * `token_mint` - The token mint that was purchased
    ///
    /// # Returns
    /// * `Ok(EntryPriceInfo)` - Entry price information if extraction succeeds
    /// * `Err(EntryPriceError)` - If transaction not found, not a buy, or parse error
    ///
    /// # Example
    /// ```ignore
    /// let info = extractor.extract_from_signature(&signature, &wallet, &mint).await?;
    /// println!("Bought {} tokens at {} lamports/token", info.tokens_received, info.price_lamports_per_token);
    /// ```
    pub async fn extract_from_signature(
        &self,
        signature: &Signature,
        user_wallet: &Pubkey,
        token_mint: &Pubkey,
    ) -> Result<EntryPriceInfo, EntryPriceError> {
        debug!(
            signature = %signature,
            wallet = %user_wallet,
            mint = %token_mint,
            "Extracting entry price from transaction"
        );

        // Fetch the transaction
        let tx = self
            .monitor
            .fetch_transaction(signature)
            .await
            .map_err(|e| EntryPriceError::TransactionNotFound(e.to_string()))?;

        // Extract the buy metadata
        let metadata = self
            .monitor
            .extract_buy_metadata(&tx, user_wallet, token_mint, *signature)
            .map_err(|e| {
                if e.to_string().contains("No tokens received") {
                    EntryPriceError::NoBalanceChange
                } else {
                    EntryPriceError::ParseError(e.to_string())
                }
            })?;

        let info = EntryPriceInfo::from(metadata);

        info!(
            signature = %signature,
            entry_price = info.price_lamports_per_token,
            tokens_received = info.tokens_received,
            token_account = %info.token_account,
            token_balance_after_buy = info.token_balance_after_buy,
            token_decimals = info.token_decimals,
            token_program = ?info.token_program,
            sol_spent = info.sol_spent,
            slot = info.slot,
            "Entry price extracted successfully"
        );

        Ok(info)
    }

    /// Extract entry price with retry on transient failures
    ///
    /// This method will retry the extraction up to `max_retries` times
    /// with exponential backoff on transient failures (RPC errors).
    ///
    /// # Arguments
    /// * `signature` - The transaction signature to analyze
    /// * `user_wallet` - The wallet that performed the buy (payer)
    /// * `token_mint` - The token mint that was purchased
    /// * `max_retries` - Maximum number of retry attempts
    ///
    /// # Returns
    /// * `Ok(EntryPriceInfo)` - Entry price information if extraction succeeds
    /// * `Err(EntryPriceError)` - If all retries fail or non-transient error
    pub async fn extract_with_retry(
        &self,
        signature: &Signature,
        user_wallet: &Pubkey,
        token_mint: &Pubkey,
        max_retries: u32,
    ) -> Result<EntryPriceInfo, EntryPriceError> {
        let mut last_error = None;

        for attempt in 0..=max_retries {
            match self
                .extract_from_signature(signature, user_wallet, token_mint)
                .await
            {
                Ok(info) => return Ok(info),
                Err(e) => {
                    // Only retry on RPC/transient errors
                    match &e {
                        EntryPriceError::RpcError(_) | EntryPriceError::TransactionNotFound(_) => {
                            if attempt < max_retries {
                                // Cap exponent using MAX_BACKOFF_EXPONENT to limit delay to ~6.4 seconds
                                // Using saturating_pow for defensive programming even though exp is capped
                                let exp = attempt.min(MAX_BACKOFF_EXPONENT);
                                let delay = std::time::Duration::from_millis(
                                    100 * 2_u64.saturating_pow(exp),
                                );
                                debug!(
                                    attempt = attempt + 1,
                                    max_retries = max_retries,
                                    delay_ms = delay.as_millis(),
                                    error = %e,
                                    "Retrying entry price extraction"
                                );
                                tokio::time::sleep(delay).await;
                                last_error = Some(e);
                                continue;
                            }
                        }
                        // Don't retry on parse/calculation errors or no balance change
                        _ => return Err(e),
                    }
                    last_error = Some(e);
                }
            }
        }

        // Return the last error if we have one, otherwise indicate max retries exceeded
        match last_error {
            Some(e) => Err(e),
            None => Err(EntryPriceError::MaxRetriesExceeded(format!(
                "Failed after {} attempts",
                max_retries + 1
            ))),
        }
    }

    /// Calculate entry price from known values (without fetching transaction)
    ///
    /// This is a utility method for cases where you already have the
    /// SOL spent and tokens received values from another source.
    ///
    /// # Arguments
    /// * `sol_spent_lamports` - SOL spent in lamports
    /// * `tokens_received` - Number of tokens received
    ///
    /// # Returns
    /// * `Ok(u64)` - Entry price (lamports per token, 1e9 scaled)
    /// * `Err(EntryPriceError)` - If tokens_received is 0 or calculation fails
    pub fn calculate_entry_price(
        sol_spent_lamports: u64,
        tokens_received: u64,
    ) -> Result<u64, EntryPriceError> {
        if tokens_received == 0 {
            return Err(EntryPriceError::NoBalanceChange);
        }

        BuyTransactionMetadata::calculate_entry_price(sol_spent_lamports, tokens_received)
            .map_err(|e| EntryPriceError::CalculationError(e.to_string()))
    }

    /// Get access to the underlying RPC client
    pub fn rpc_client(&self) -> &Arc<RpcClient> {
        &self.rpc_client
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::shadow_ledger::types::PriceReason;
    use std::str::FromStr;

    #[test]
    fn test_entry_price_error_display() {
        let err = EntryPriceError::TransactionNotFound("ABC123".to_string());
        assert!(err.to_string().contains("Transaction not found"));
        assert!(err.to_string().contains("ABC123"));

        let err = EntryPriceError::ParseError("invalid data".to_string());
        assert!(err.to_string().contains("Failed to parse"));

        let err = EntryPriceError::CalculationError("overflow".to_string());
        assert!(err.to_string().contains("Calculation error"));

        let err = EntryPriceError::NoBalanceChange;
        assert!(err.to_string().contains("No balance change"));

        let err = EntryPriceError::RpcError("timeout".to_string());
        assert!(err.to_string().contains("RPC error"));

        let err = EntryPriceError::MaxRetriesExceeded("Failed after 3 attempts".to_string());
        assert!(err.to_string().contains("Max retries exceeded"));
    }

    #[test]
    fn test_entry_price_info_from_metadata() {
        let metadata = BuyTransactionMetadata {
            signature: Signature::default(),
            mint: Pubkey::new_unique(),
            sol_spent: 1_000_000_000,
            tokens_received: 1_000_000,
            entry_price: 1_000_000_000_000,
            slot: 12345,
            token_account: Pubkey::new_unique(),
            token_balance_after_buy: 1_000_000,
            token_decimals: 6,
            token_program: Some(
                Pubkey::from_str("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb")
                    .expect("token program"),
            ),
            fee_recipient: Some(Pubkey::new_unique()),
        };

        let info = EntryPriceInfo::from(metadata.clone());
        assert_eq!(info.price_lamports_per_token, metadata.entry_price);
        assert_eq!(info.tokens_received, metadata.tokens_received);
        assert_eq!(info.sol_spent, metadata.sol_spent);
        assert_eq!(info.slot, metadata.slot);
        assert_eq!(info.token_account, metadata.token_account);
        assert_eq!(
            info.token_balance_after_buy,
            metadata.token_balance_after_buy
        );
        assert_eq!(info.token_decimals, metadata.token_decimals);
        assert_eq!(info.token_program, metadata.token_program);
        assert_eq!(info.fee_recipient, metadata.fee_recipient);
    }

    #[test]
    fn test_calculate_entry_price() {
        // Test: 1 SOL for 1M tokens
        let result = EntryPriceExtractor::calculate_entry_price(1_000_000_000, 1_000_000);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1_000_000_000_000);

        // Test: 0.1 SOL for 10M tokens
        let result = EntryPriceExtractor::calculate_entry_price(100_000_000, 10_000_000);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 10_000_000_000);

        // Test: zero tokens should fail
        let result = EntryPriceExtractor::calculate_entry_price(1_000_000_000, 0);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EntryPriceError::NoBalanceChange
        ));
    }

    #[test]
    fn test_extractor_creation() {
        let rpc_client = Arc::new(RpcClient::new("http://localhost:8899".to_string()));
        let extractor = EntryPriceExtractor::new(rpc_client.clone());

        // Verify RPC client is accessible
        let _ = extractor.rpc_client();
    }

    #[test]
    fn test_price_truth_resolver_resolves_shadow_exit_without_fallback() {
        let snapshot = MarketSnapshot {
            slot: Some(42),
            timestamp_ms: 1_000,
            price_sol_per_token: 2_000.0,
            price_state: PriceState::Valid,
            reserve_base: 500_000.0,
            reserve_quote: 1.0,
            ..MarketSnapshot::default()
        };

        let sample =
            PriceTruthResolver::resolve_shadow_exit_sample(&snapshot, 1_250, 1_000).unwrap();
        assert_eq!(sample.evidence.status, PriceTruthStatus::Resolved);

        let exit_token_amount_raw = 250_000;
        let exit_qty_tokens = exit_token_amount_raw as f64 / PUMP_TOKEN_DECIMAL_FACTOR_F64;
        let expected_exit_value_sol =
            sample.curve.calculate_sell_price(exit_token_amount_raw) as f64 / LAMPORTS_PER_SOL_F64;
        let truth =
            PriceTruthResolver::resolve_shadow_exit(1.0, exit_token_amount_raw, &sample, 0.0)
                .expect("shadow exit truth");
        assert!((truth.entry_value_sol - 0.25).abs() < 1e-9);
        assert!((truth.exit_value_sol - expected_exit_value_sol).abs() < 1e-9);
        assert!((truth.exit_price_sol - (expected_exit_value_sol / exit_qty_tokens)).abs() < 1e-9);
        assert!((truth.gross_pnl_sol - (expected_exit_value_sol - 0.25)).abs() < 1e-9);
        assert!((truth.net_pnl_sol - (expected_exit_value_sol - 0.25)).abs() < 1e-9);
        assert!((truth.pnl_pct - (((expected_exit_value_sol - 0.25) / 0.25) * 100.0)).abs() < 1e-9);
    }

    #[test]
    fn test_price_truth_resolver_marks_stale_shadow_sample() {
        let snapshot = MarketSnapshot {
            slot: Some(7),
            timestamp_ms: 1_000,
            price_sol_per_token: 1_500.0,
            price_state: PriceState::Valid,
            reserve_base: 1_000_000.0,
            reserve_quote: 1.5,
            ..MarketSnapshot::default()
        };

        let err = PriceTruthResolver::resolve_shadow_exit_sample(&snapshot, 3_000, 1_000)
            .expect_err("stale sample");
        assert_eq!(err.status(), PriceTruthStatus::Stale);
        assert_eq!(err.evidence().slot, Some(7));
    }

    #[test]
    fn test_price_truth_resolver_requires_backfill_for_unknown_shadow_sample() {
        let snapshot = MarketSnapshot {
            timestamp_ms: 500,
            price_sol_per_token: 1.0,
            price_state: PriceState::Unknown,
            price_reason: Some(PriceReason::MissingPriceData),
            ..MarketSnapshot::default()
        };

        let err = PriceTruthResolver::resolve_shadow_exit_sample(&snapshot, 800, 1_000)
            .expect_err("backfill-required sample");
        assert_eq!(err.status(), PriceTruthStatus::BackfillRequired);
        assert_eq!(
            err.evidence().price_reason,
            Some(PriceReason::MissingPriceData)
        );
    }

    #[test]
    fn test_price_truth_resolver_normalizes_shadow_ledger_price_to_sol_per_token() {
        let snapshot = MarketSnapshot {
            slot: Some(9),
            timestamp_ms: 1_000,
            price_sol_per_token: 28.0,
            price_state: PriceState::Valid,
            reserve_base: 250_000.0,
            reserve_quote: 0.007,
            ..MarketSnapshot::default()
        };

        let sample =
            PriceTruthResolver::resolve_shadow_exit_sample(&snapshot, 1_100, 1_000).unwrap();
        assert!((sample.exit_price_sol - 0.028).abs() < 1e-12);
    }

    #[test]
    fn test_price_truth_resolver_rejects_exit_above_oracle_spot_as_semantic_violation() {
        let sample = ShadowExitPriceSample {
            exit_price_sol: 5.4928389038831215e-8,
            curve: BondingCurve {
                discriminator: 0,
                virtual_token_reserves: 434_000_000_000_000,
                virtual_sol_reserves: 42_049_314_424,
                real_token_reserves: 434_000_000_000_000,
                real_sol_reserves: 42_049_314_424,
                token_total_supply: 434_000_000_000_000,
                complete: 0,
                _padding: [0; 7],
            },
            evidence: PriceTruthEvidence {
                source: PriceTruthSource::ShadowLedgerSnapshot,
                status: PriceTruthStatus::Resolved,
                detail: None,
                slot: Some(414_525_981),
                timestamp_ms: Some(1_776_708_559_073),
                age_ms: Some(301_241),
                price_state: Some(PriceState::Valid),
                price_reason: None,
            },
        };

        let err = PriceTruthResolver::resolve_shadow_exit(
            5.829440431458688e-8,
            30_020_034_008,
            &sample,
            0.0,
        )
        .expect_err("semantic violation");
        assert_eq!(err.status(), PriceTruthStatus::SemanticViolation);
        let detail = err
            .evidence()
            .detail
            .as_deref()
            .expect("semantic violation detail");
        assert!(detail.contains("semantic_violation=exit_fill_above_oracle_spot"));
        assert!(detail.contains("oracle_spot_price="));
        assert!(detail.contains("computed_exit_price="));
        assert!(detail.contains("source_path=trigger.price_truth.resolve_shadow_exit"));
    }
}
